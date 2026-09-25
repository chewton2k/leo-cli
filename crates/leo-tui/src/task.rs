//! The worker thread.
//!
//! The event loop must never block on the network, a subprocess, or the
//! microphone, so anything slow runs here and reports back over a channel. The
//! worker never touches the `Store`: it emits a final transcript and the App
//! saves, which keeps all persistence on one thread.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use leo_services::ai::live;
use leo_services::listen::Recorder;

/// How often the worker wakes to check the clock and the stop flag.
const POLL: Duration = Duration::from_millis(250);

/// Progress from a background job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskEvent {
    Started {
        label: String,
    },
    /// What is happening now, and how far along it is when that is knowable.
    /// `steps` drives a real progress bar; `None` means an unknown duration, so
    /// the UI shows a spinner instead of inventing a percentage.
    Progress {
        label: String,
        steps: Option<(usize, usize)>,
    },
    /// The full raw transcript so far.
    Transcript(String),
    /// A provider degraded mid-job; shown once in the status line.
    ProviderFallback {
        from: String,
        to: String,
    },
    /// The job finished and produced this transcript for the App to save.
    Finished {
        transcript: String,
    },
    /// A transcript has been structured into a note. `title` is `None` when the
    /// result is being appended to an existing note.
    Structured {
        title: Option<String>,
        body: String,
    },
    /// Answer text as it arrives, accumulated. Shown in the preview so a slow
    /// model reads as working rather than hung.
    Streaming(String),
    /// A question across the notes has been answered.
    Answered {
        question: String,
        text: String,
    },
    /// A note's `@leo` prompts have been expanded; the App writes it back.
    Expanded {
        note: String,
        body: String,
        count: usize,
    },
    /// A background push finished.
    Pushed,
    Failed(String),
}

/// A running background job.
pub struct Job {
    rx: Receiver<TaskEvent>,
    stop: Arc<AtomicBool>,
    /// Set while a recording is paused. Only the listen worker reads it.
    pause: Arc<AtomicBool>,
    done: bool,
}

impl Job {
    /// Ask the job to wind down. It still reports a final result, so a stop is
    /// not a cancel: audio already recorded is transcribed and saved.
    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// Pause or resume a recording.
    pub fn set_paused(&self, paused: bool) {
        self.pause.store(paused, Ordering::Relaxed);
    }

    pub fn paused(&self) -> bool {
        self.pause.load(Ordering::Relaxed)
    }

    pub fn stop_requested(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    /// True once a terminal event has been observed.
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// A job that has already sent `events` and is still running, for driving the App
    /// through a job's lifecycle in tests without a worker thread.
    #[cfg(test)]
    pub fn scripted(events: Vec<TaskEvent>) -> Job {
        let (tx, rx) = mpsc::channel();
        for event in events {
            tx.send(event).unwrap();
        }
        // A live worker keeps its sender until it finishes; dropping it here
        // would read as the worker dying.
        std::mem::forget(tx);
        Job {
            rx,
            stop: Arc::new(AtomicBool::new(false)),
            pause: Arc::new(AtomicBool::new(false)),
            done: false,
        }
    }

    /// Take everything the worker has sent since the last call. Never blocks.
    pub fn drain(&mut self) -> Vec<TaskEvent> {
        let mut out = Vec::new();
        loop {
            match self.rx.try_recv() {
                Ok(event) => {
                    if matches!(
                        event,
                        TaskEvent::Finished { .. }
                            | TaskEvent::Failed(_)
                            | TaskEvent::Structured { .. }
                            | TaskEvent::Answered { .. }
                    ) {
                        self.done = true;
                    }
                    out.push(event);
                }
                Err(TryRecvError::Empty) => break,
                // The worker thread ended without a terminal event.
                Err(TryRecvError::Disconnected) => {
                    self.done = true;
                    break;
                }
            }
        }
        out
    }
}

/// Push the notes repository on a worker thread.
///
/// On a worker because a push is a network round trip: doing it on the event loop
/// would freeze the interface for as long as the remote takes, which is exactly
/// the thing an automatic feature must never do to someone who did not ask for it
/// right now.
pub fn start_push(notes_dir: std::path::PathBuf) -> Job {
    let (tx, rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));

    thread::spawn(move || {
        let _ = tx.send(TaskEvent::Started {
            label: "Backing up".to_string(),
        });
        match leo_core::sync::push(&notes_dir) {
            Ok(()) => {
                let _ = tx.send(TaskEvent::Pushed);
            }
            // Reported, never retried in a loop: a rejected push usually means
            // the remote moved on, and the fix is a pull the user should make
            // deliberately rather than have leo guess at.
            Err(e) => {
                let _ = tx.send(TaskEvent::Failed(e.to_string()));
            }
        }
    });

    Job {
        rx,
        stop,
        pause: Arc::new(AtomicBool::new(false)),
        done: false,
    }
}

/// Expand a note's `@leo` prompts on a worker thread, streaming the answer.
///
/// On the worker rather than the main thread because this is the one action that
/// could take a minute: it used to run inline, which froze the whole interface
/// with no way to tell working from hung.
pub fn start_ask(note: String, title: String, body: String) -> Job {
    let (tx, rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));

    thread::spawn(move || {
        let _ = tx.send(TaskEvent::Started {
            label: "Asking".to_string(),
        });

        // Accumulated answer text, reset when the chain falls through to another
        // provider: what came before belongs to the provider that just failed.
        let shown = Arc::new(std::sync::Mutex::new(String::new()));

        let result = {
            let (shown, tx) = (Arc::clone(&shown), tx.clone());
            let mut on_fragment = |fragment: &str| {
                if let Ok(mut text) = shown.lock() {
                    text.push_str(fragment);
                    let _ = tx.send(TaskEvent::Streaming(text.clone()));
                }
            };
            let shown_restart = Arc::clone(&shown);
            let mut on_restart = move || {
                if let Ok(mut text) = shown_restart.lock() {
                    text.clear();
                }
            };
            leo_services::ai::expand_prompts_streaming(
                &body,
                &title,
                &mut on_fragment,
                &mut on_restart,
            )
        };

        match result {
            Ok((expanded, count)) => {
                let _ = tx.send(TaskEvent::Expanded {
                    note,
                    body: expanded,
                    count,
                });
            }
            Err(e) => {
                let _ = tx.send(TaskEvent::Failed(e.to_string()));
            }
        }
    });

    Job {
        rx,
        stop,
        pause: Arc::new(AtomicBool::new(false)),
        done: false,
    }
}

/// Answer a question from `notes` — (title, directory, body) — on a worker
/// thread, streaming the answer as it arrives.
pub fn start_question(question: String, notes: Vec<(String, String, String)>) -> Job {
    let (tx, rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));

    thread::spawn(move || {
        let _ = tx.send(TaskEvent::Started {
            label: "Asking your notes".to_string(),
        });
        let shown = Arc::new(std::sync::Mutex::new(String::new()));
        let result = {
            let (shown, tx) = (Arc::clone(&shown), tx.clone());
            let mut on_fragment = |fragment: &str| {
                if let Ok(mut text) = shown.lock() {
                    text.push_str(fragment);
                    let _ = tx.send(TaskEvent::Streaming(text.clone()));
                }
            };
            let shown_restart = Arc::clone(&shown);
            let mut on_restart = move || {
                if let Ok(mut text) = shown_restart.lock() {
                    text.clear();
                }
            };
            leo_services::ai::answer_from_notes(
                &question,
                &notes,
                &mut on_fragment,
                &mut on_restart,
            )
        };
        let _ = tx.send(match result {
            Ok(text) => TaskEvent::Answered { question, text },
            Err(e) => TaskEvent::Failed(e.to_string()),
        });
    });

    Job {
        rx,
        stop,
        pause: Arc::new(AtomicBool::new(false)),
        done: false,
    }
}

/// Turn a transcript into a note body on a worker thread.
///
/// The request takes seconds to tens of seconds, and doing it on the event loop
/// froze the whole UI — no spinner, no clock, no way to tell the difference
/// between working and hung. `existing` is the body to append to, when this is
/// an append rather than a new note.
pub fn start_structuring(
    transcript: String,
    existing: Option<String>,
    points: Vec<leo_services::ai::chat::Jotted>,
    length_secs: u64,
) -> Job {
    let (tx, rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));

    thread::spawn(move || {
        let _ = tx.send(TaskEvent::Progress {
            label: "Structuring notes".to_string(),
            steps: None,
        });

        let result = match &existing {
            Some(body) => leo_services::ai::chat_outcome(
                leo_services::ai::chat::build_append_prompt_with(
                    &transcript,
                    body,
                    &points,
                    length_secs,
                ),
                leo_services::ai::STRUCTURE_MAX_TOKENS,
            )
            .map(|outcome| (None, outcome)),
            None => leo_services::ai::chat_outcome(
                leo_services::ai::chat::build_structure_prompt_with(
                    &transcript,
                    &points,
                    length_secs,
                ),
                leo_services::ai::STRUCTURE_MAX_TOKENS,
            )
            .map(|outcome| {
                let (title, body) = leo_services::ai::chat::split_title_body(&outcome.value);
                (Some(title), chain_with_value(outcome, body))
            }),
        };

        match result {
            Ok((title, outcome)) => {
                for f in &outcome.fallbacks {
                    let _ = tx.send(TaskEvent::ProviderFallback {
                        from: f.from.clone(),
                        to: f.to.clone(),
                    });
                }
                // A new note's body was already cleaned when its title was split
                // off; an addition is cleaned here.
                let body = match title {
                    Some(_) => outcome.value.trim().to_string(),
                    None => leo_services::ai::chat::clean_reply(&outcome.value),
                };
                let _ = tx.send(TaskEvent::Structured { title, body });
            }
            Err(e) => {
                let _ = tx.send(TaskEvent::Failed(e.to_string()));
            }
        }
    });

    Job {
        rx,
        stop,
        pause: Arc::new(AtomicBool::new(false)),
        done: false,
    }
}

/// Replace an outcome's value, keeping the provider and fallback trail.
fn chain_with_value(
    outcome: leo_services::ai::chain::ChainOutcome<String>,
    value: String,
) -> leo_services::ai::chain::ChainOutcome<String> {
    leo_services::ai::chain::ChainOutcome {
        value,
        provider: outcome.provider,
        fallbacks: outcome.fallbacks,
    }
}

/// What to say when a recording contains no sound at all.
///
/// Names the cause rather than the symptom: on macOS a denied microphone
/// permission is not an error — `rec` succeeds and every sample is zero — so
/// this is nearly always a permission that was never granted.
const SILENT_RECORDING: &str = "No sound was recorded. macOS may not be letting \
     this terminal use the microphone: System Settings > Privacy & Security > \
     Microphone, then restart the terminal. `leo setup` re-checks it.";

/// Read a WAV's duration in whole seconds via sox.
fn wav_secs(path: &Path) -> Option<u64> {
    let out = std::process::Command::new("sox")
        .args(["--i", "-D", path.to_str()?])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<f64>()
        .ok()
        .map(|d| d as u64)
        .filter(|&d| d > 0)
}

/// Copy the growing recording and repair the copy's header, so a slice can be
/// cut from it. The live file's DataSize is still zero — `rec` only writes it on
/// exit — and `sox trim` on such a file yields nothing.
fn snapshot(source: &Path) -> Option<PathBuf> {
    let dest = std::env::temp_dir().join(format!("leo-live-snapshot-{}.wav", std::process::id()));
    std::fs::copy(source, &dest).ok()?;
    leo_services::listen::repair_wav_header(&dest);
    Some(dest)
}

/// Cut one slice out of a snapshot for transcription.
fn cut(source: &Path, slice: live::Slice) -> Option<PathBuf> {
    let dest = std::env::temp_dir().join(format!("leo-live-slice-{}.wav", std::process::id()));
    let ok = std::process::Command::new("sox")
        .arg(source)
        .arg(&dest)
        .arg("trim")
        .arg(slice.start_secs.to_string())
        .arg(slice.duration_secs.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if ok && dest.exists() && std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0) > 100 {
        Some(dest)
    } else {
        let _ = std::fs::remove_file(&dest);
        None
    }
}

/// State the two loops share across iterations.
struct Live {
    /// Current gap between slices. Grows on failure, resets on success.
    interval: std::time::Duration,
    /// Consecutive slices that contained no sound.
    silent_slices: usize,
    /// Whether the user has already been told the microphone is not heard.
    warned_silent: bool,
    transcript: String,
    /// Where the rolling loop has transcribed up to, in seconds.
    cursor: u64,
}

/// Start recording with rolling transcription, sent to the App as it grows.
pub fn start_listen(screen: bool) -> Job {
    let (tx, rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = Arc::clone(&stop);
    let pause = Arc::new(AtomicBool::new(false));
    let worker_pause = Arc::clone(&pause);

    thread::spawn(move || {
        let recorder = match Recorder::start(screen) {
            Ok(r) => r,
            Err(e) => {
                let _ = tx.send(TaskEvent::Failed(e.to_string()));
                return;
            }
        };
        let _ = tx.send(TaskEvent::Started {
            label: "Recording".to_string(),
        });

        // Resolve credentials while the first few seconds of audio accumulate.
        // A keychain read can take a very long time, and paying it inside the
        // rolling loop stalls transcription with nothing on screen to explain
        // the silence — which is exactly how live transcription came to look
        // like it did not work at all.
        leo_services::ai::warm_credentials();

        let mut state = Live {
            transcript: String::new(),
            cursor: 0,
            interval: live::ROLL_INTERVAL,
            silent_slices: 0,
            warned_silent: false,
        };
        // Due immediately: the first words should appear as soon as there is
        // enough audio to cut, not one interval later.
        let mut last_roll = Instant::now() - live::ROLL_INTERVAL;

        // Paused stretches, in seconds into the file. The recorder keeps
        // running through a pause; these are cut out before the final pass,
        // and the live loop skips them.
        let mut pauses: Vec<leo_services::listen::Pause> = Vec::new();
        let mut paused_since: Option<f64> = None;

        while !worker_stop.load(Ordering::Relaxed) {
            thread::sleep(POLL);

            let now = recorder.elapsed().as_secs_f64();
            match (worker_pause.load(Ordering::Relaxed), paused_since) {
                (true, None) => paused_since = Some(now),
                (false, Some(start)) => {
                    pauses.push((start, Some(now)));
                    paused_since = None;
                    // Carry on from here, past the overlap, so nothing said
                    // while paused reaches the live transcript.
                    state.cursor = now.ceil() as u64 + live::OVERLAP.as_secs();
                    last_roll = Instant::now() - state.interval;
                }
                _ => {}
            }

            let paused_for: f64 = pauses
                .iter()
                .map(|(start, end)| end.unwrap_or(now) - start)
                .sum::<f64>()
                + paused_since.map_or(0.0, |start| now - start);
            let secs = (now - paused_for).max(0.0) as u64;
            let state_word = if paused_since.is_some() {
                "Paused"
            } else {
                "Recording"
            };
            let _ = tx.send(TaskEvent::Progress {
                label: format!("{state_word} {:02}:{:02}", secs / 60, secs % 60),
                steps: None,
            });

            if paused_since.is_some() || last_roll.elapsed() < state.interval {
                continue;
            }
            last_roll = Instant::now();
            roll_once(recorder.path(), &mut state, &tx);
        }

        // Stopping is not cancelling: finish the recording and transcribe it
        // whole. The rolling transcript is lower quality by construction —
        // slices are stitched across boundaries — so the saved note uses a
        // single pass over the finished file, matching non-live behavior. If
        // that fails, fall back to the rolling text rather than losing the
        // recording entirely.
        let _ = tx.send(TaskEvent::Progress {
            label: "Transcribing the recording".to_string(),
            steps: None,
        });

        if let Some(start) = paused_since {
            pauses.push((start, None));
        }
        let finished = recorder
            .stop()
            .and_then(|path| leo_services::listen::cut_pauses(&path, &pauses));

        let final_transcript = match finished {
            Ok(path) => {
                // A recording with no sound in it must not be transcribed. The
                // result would be invented text saved as a note, which is worse
                // than an error: it looks like leo mis-heard rather than never
                // heard anything.
                let level = leo_services::listen::peak_amplitude(&path).unwrap_or(1.0);
                if live::is_silent(level) {
                    let _ = std::fs::remove_file(&path);
                    let _ = tx.send(TaskEvent::Failed(SILENT_RECORDING.to_string()));
                    return;
                }

                let report = tx.clone();
                let result = leo_services::ai::transcribe_outcome_with_progress(
                    &path,
                    &move |done, total| {
                        let _ = report.send(TaskEvent::Progress {
                            label: "Transcribing the recording".to_string(),
                            steps: Some((done, total)),
                        });
                    },
                );
                let _ = std::fs::remove_file(&path);
                match result {
                    Ok(outcome) => {
                        for f in &outcome.fallbacks {
                            let _ = tx.send(TaskEvent::ProviderFallback {
                                from: f.from.clone(),
                                to: f.to.clone(),
                            });
                        }
                        outcome.value
                    }
                    Err(e) if !state.transcript.trim().is_empty() => {
                        let _ = tx.send(TaskEvent::ProviderFallback {
                            from: format!("final transcription failed ({e})"),
                            to: "the live transcript".to_string(),
                        });
                        state.transcript.clone()
                    }
                    Err(e) => {
                        let _ = tx.send(TaskEvent::Failed(e.to_string()));
                        return;
                    }
                }
            }
            Err(e) => {
                // No audio at all: only worth reporting if the live loop never
                // heard anything either.
                if state.transcript.trim().is_empty() {
                    let _ = tx.send(TaskEvent::Failed(e.to_string()));
                    return;
                }
                state.transcript.clone()
            }
        };

        let _ = tx.send(TaskEvent::Finished {
            transcript: final_transcript,
        });
    });

    Job {
        rx,
        stop,
        pause,
        done: false,
    }
}

/// One rolling pass: cut the new tail, transcribe it, stitch it on.
fn roll_once(source: &Path, state: &mut Live, tx: &mpsc::Sender<TaskEvent>) {
    let Some(snap) = snapshot(source) else {
        return;
    };
    let recorded = wav_secs(&snap).unwrap_or(0);

    let Some(slice) = live::next_slice(state.cursor, recorded) else {
        let _ = std::fs::remove_file(&snap);
        return;
    };
    let Some(slice_path) = cut(&snap, slice) else {
        let _ = std::fs::remove_file(&snap);
        return;
    };
    let _ = std::fs::remove_file(&snap);

    // Never send silence. Whisper does not answer it with an empty string; it
    // invents filler, and "Thank you." is its favourite — which is how someone
    // saying "hello, my name is…" into a microphone macOS had muted got back
    // "thank you". Skipping also costs nothing, so idle stretches are free.
    let level = leo_services::listen::peak_amplitude(&slice_path).unwrap_or(1.0);
    if live::is_silent(level) {
        let _ = std::fs::remove_file(&slice_path);
        state.cursor = recorded;
        state.silent_slices += 1;
        // Two silent slices in a row is a quiet room. Sustained silence while
        // the user believes they are being recorded is a broken microphone, and
        // saying so is the whole difference between "leo is broken" and "macOS
        // needs to be told yes".
        if state.silent_slices >= 4 && !state.warned_silent {
            state.warned_silent = true;
            let _ = tx.send(TaskEvent::ProviderFallback {
                from: "no sound from the microphone".to_string(),
                to: "check System Settings > Privacy & Security > Microphone".to_string(),
            });
        }
        return;
    }
    state.silent_slices = 0;

    let result = leo_services::ai::transcribe_outcome(&slice_path);
    let _ = std::fs::remove_file(&slice_path);

    match result {
        Ok(outcome) => {
            for f in &outcome.fallbacks {
                let _ = tx.send(TaskEvent::ProviderFallback {
                    from: f.from.clone(),
                    to: f.to.clone(),
                });
            }
            state.interval = live::ROLL_INTERVAL;
            state.cursor = recorded;

            // Audio loud enough to pass the level check can still be too quiet
            // to transcribe, and comes back as the same invented filler.
            if live::is_silence_artifact(&outcome.value) {
                return;
            }

            state.transcript = live::stitch(&state.transcript, &outcome.value);
            let _ = tx.send(TaskEvent::Transcript(state.transcript.clone()));
        }
        // A failed slice is not fatal: the cursor stays put so the next pass
        // covers the same audio again. Back off, though — at a three-second
        // cadence, retrying a rate-limited provider at full speed is what keeps
        // it rate-limited.
        Err(e) => {
            state.interval = live::backoff(state.interval);
            let _ = tx.send(TaskEvent::Progress {
                label: format!("Transcription retrying ({e})"),
                steps: None,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    /// End-to-end proof that text arrives *while* recording, not only after.
    ///
    /// Ignored by default: it makes real transcription requests. Run with
    /// `LEO_FAKE_AUDIO=<wav> cargo test live_streams -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_streams_text_during_recording() {
        if std::env::var("LEO_FAKE_AUDIO").is_err() {
            panic!("set LEO_FAKE_AUDIO to a wav of speech");
        }
        let mut job = super::start_listen(false);

        let started = std::time::Instant::now();
        let mut first_text_at = None;
        let mut transcripts = Vec::new();

        // Watch for 13 seconds of a ~14 second recording.
        while started.elapsed() < std::time::Duration::from_secs(16) {
            for event in job.drain() {
                match event {
                    TaskEvent::Transcript(text) => {
                        if first_text_at.is_none() {
                            first_text_at = Some(started.elapsed());
                        }
                        println!("[{:>5.1}s] {text}", started.elapsed().as_secs_f64());
                        transcripts.push(text);
                    }
                    TaskEvent::Progress { label, .. } => {
                        if label.contains("retrying") {
                            println!("[{:>5.1}s] {label}", started.elapsed().as_secs_f64());
                        }
                    }
                    TaskEvent::ProviderFallback { from, to } => {
                        println!(
                            "[{:>5.1}s] fallback: {from} -> {to}",
                            started.elapsed().as_secs_f64()
                        );
                    }
                    _ => {}
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        job.request_stop();

        let first = first_text_at.expect("no transcript arrived while recording");
        println!(
            "first text after {:.1}s, {} updates",
            first.as_secs_f64(),
            transcripts.len()
        );
        assert!(
            first < std::time::Duration::from_secs(8),
            "first text took {:.1}s — not live",
            first.as_secs_f64()
        );
        assert!(
            transcripts.len() >= 2,
            "only {} update(s); text should build up as speech continues",
            transcripts.len()
        );
    }

    use super::*;

    /// A job whose worker never starts still drains cleanly and reports done,
    /// rather than blocking the event loop forever.
    #[test]
    fn a_dropped_worker_marks_the_job_done() {
        let (tx, rx) = mpsc::channel::<TaskEvent>();
        let mut job = Job {
            rx,
            stop: Arc::new(AtomicBool::new(false)),
            pause: Arc::new(AtomicBool::new(false)),
            done: false,
        };
        drop(tx);
        assert!(job.drain().is_empty());
        assert!(job.is_done());
    }

    #[test]
    fn draining_returns_events_in_order_and_notices_the_terminal_one() {
        let (tx, rx) = mpsc::channel();
        let mut job = Job {
            rx,
            stop: Arc::new(AtomicBool::new(false)),
            pause: Arc::new(AtomicBool::new(false)),
            done: false,
        };

        tx.send(TaskEvent::Started {
            label: "Recording".to_string(),
        })
        .unwrap();
        tx.send(TaskEvent::Transcript("hello".to_string())).unwrap();
        assert_eq!(job.drain().len(), 2);
        assert!(!job.is_done());

        tx.send(TaskEvent::Finished {
            transcript: "hello".to_string(),
        })
        .unwrap();
        let events = job.drain();
        assert_eq!(events.len(), 1);
        assert!(job.is_done());
    }

    #[test]
    fn pausing_is_visible_to_the_worker_and_can_be_undone() {
        let job = Job::scripted(vec![]);
        assert!(!job.paused());
        job.set_paused(true);
        assert!(job.paused());
        job.set_paused(false);
        assert!(!job.paused());
    }

    #[test]
    fn requesting_stop_is_visible_to_the_worker() {
        let (_tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let job = Job {
            rx,
            stop: Arc::clone(&stop),
            pause: Arc::new(AtomicBool::new(false)),
            done: false,
        };
        assert!(!job.stop_requested());
        job.request_stop();
        assert!(stop.load(Ordering::Relaxed));
        assert!(job.stop_requested());
    }

    #[test]
    fn a_failure_event_also_ends_the_job() {
        let (tx, rx) = mpsc::channel();
        let mut job = Job {
            rx,
            stop: Arc::new(AtomicBool::new(false)),
            pause: Arc::new(AtomicBool::new(false)),
            done: false,
        };
        tx.send(TaskEvent::Failed("no microphone".to_string()))
            .unwrap();
        job.drain();
        assert!(job.is_done());
    }
}
