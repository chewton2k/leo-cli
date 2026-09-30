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

use leo_services::ai::chat::Jotted;
use leo_services::ai::live;
use leo_services::session::capture::{Capture, Source};
use leo_services::session::transcriber::{Policy, Transcriber, Update};
use leo_services::session::{self, wav, Manifest, Part, Point, Session};

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
        session: Option<PathBuf>,
    },
    Warning(String),
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
    Checked(Vec<leo_services::doctor::Section>),
    Failed(String),
}

/// A running background job.
pub struct Job {
    rx: Receiver<TaskEvent>,
    stop: Arc<AtomicBool>,
    /// Set while a recording is paused. Only the listen worker reads it.
    pause: Arc<AtomicBool>,
    done: bool,
    points: Arc<std::sync::Mutex<Vec<Jotted>>>,
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

    pub fn add_point(&self, point: Jotted) {
        if let Ok(mut points) = self.points.lock() {
            points.push(point);
        }
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
            points: Default::default(),
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
                            | TaskEvent::Checked(_)
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
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown error".to_string())
}

fn caught<T>(work: impl FnOnce() -> T) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
        .map_err(|payload| panic_message(&*payload))
}

fn spawn_guarded(tx: mpsc::Sender<TaskEvent>, work: impl FnOnce() + Send + 'static) {
    thread::spawn(move || {
        if let Err(message) = caught(work) {
            let _ = tx.send(TaskEvent::Failed(format!(
                "leo hit an internal error and stopped this task ({message}). Your notes are safe."
            )));
        }
    });
}

pub fn start_push(notes_dir: std::path::PathBuf) -> Job {
    let (tx, rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));

    spawn_guarded(tx.clone(), move || {
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
        points: Default::default(),
    }
}

pub fn start_update_check() -> std::sync::mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        if let Some(version) = leo_services::update::available() {
            let _ = tx.send(version);
        }
    });
    rx
}

pub fn start_doctor(notes_dir: std::path::PathBuf, probe: leo_services::doctor::Probe) -> Job {
    let (tx, rx) = mpsc::channel();
    spawn_guarded(tx.clone(), move || {
        let config = leo_services::config::Config::load();
        let config_path = leo_services::config::Config::config_path().unwrap_or_default();
        let sections = leo_services::doctor::scan(
            &config,
            leo_services::config::secret::default_store().as_ref(),
            &notes_dir,
            &config_path,
            probe,
        );
        let _ = tx.send(TaskEvent::Checked(sections));
    });
    Job {
        rx,
        stop: Arc::new(AtomicBool::new(false)),
        pause: Arc::new(AtomicBool::new(false)),
        done: false,
        points: Default::default(),
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

    spawn_guarded(tx.clone(), move || {
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
        points: Default::default(),
    }
}

/// Answer a question from `notes` — (title, directory, body) — on a worker
/// thread, streaming the answer as it arrives.
pub fn start_question(question: String, notes: Vec<(String, String, String)>) -> Job {
    let (tx, rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));

    spawn_guarded(tx.clone(), move || {
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
        points: Default::default(),
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
    session: Option<PathBuf>,
    existing: Option<String>,
    points: Vec<Jotted>,
    fallback_title: String,
) -> Job {
    let (tx, rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));

    spawn_guarded(tx.clone(), move || {
        let _ = tx.send(TaskEvent::Progress {
            label: "Writing the notes".to_string(),
            steps: None,
        });
        let opened = session.as_deref().map(Session::open);
        let (parts, notice) = match opened {
            Some(Ok(s)) => {
                let assembled = s.assemble();
                let notice = session::failure_notice(&assembled.failed, &s.dir);
                (assembled.parts, notice)
            }
            _ => (
                vec![Part {
                    index: 0,
                    start_secs: 0,
                    end_secs: 0,
                    text: transcript.clone(),
                }],
                String::new(),
            ),
        };
        let report = tx.clone();
        let structured = leo_services::ai::long::structure_recording(
            &parts,
            &points,
            existing.as_deref(),
            &fallback_title,
            &|prompt, max| leo_services::ai::chat_outcome(prompt, max).map(|o| o.value),
            &move |done, total| {
                let _ = report.send(TaskEvent::Progress {
                    label: "Writing the notes".to_string(),
                    steps: (total > 1).then_some((done, total)),
                });
            },
        );
        for problem in structured.problems {
            let _ = tx.send(TaskEvent::Warning(problem));
        }
        let body = if notice.is_empty() {
            structured.body
        } else {
            format!("{notice}\n\n{}", structured.body)
        };
        let _ = tx.send(TaskEvent::Structured {
            title: existing.is_none().then_some(structured.title),
            body,
        });
    });

    Job {
        rx,
        stop,
        pause: Arc::new(AtomicBool::new(false)),
        done: false,
        points: Default::default(),
    }
}

/// What to say when a recording contains no sound at all.
///
/// Names the cause rather than the symptom: on macOS a denied microphone
/// permission is not an error — `rec` succeeds and every sample is zero — so
/// this is nearly always a permission that was never granted.
const SILENT_RECORDING: &str = "No sound was recorded. macOS may not be letting \
     this terminal use the microphone: System Settings > Privacy & Security > \
     Microphone, then restart the terminal. /doctor re-checks it.";

const LIVE_WINDOW_SECS: u64 = 30;

struct Live {
    interval: Duration,
    silent_slices: usize,
    warned_silent: bool,
    finals: std::collections::BTreeMap<u32, String>,
    settled: u32,
    base: u64,
    cursor: u64,
    text: String,
    shown: String,
}

impl Live {
    fn new() -> Live {
        Live {
            interval: live::ROLL_INTERVAL,
            silent_slices: 0,
            warned_silent: false,
            finals: Default::default(),
            settled: 0,
            base: 0,
            cursor: 0,
            text: String::new(),
            shown: String::new(),
        }
    }

    fn display(&self) -> String {
        let mut out = String::new();
        for text in self.finals.values() {
            out = live::stitch(&out, text);
        }
        live::stitch(&out, &self.text)
    }

    fn apply(&mut self, update: Update, tx: &mpsc::Sender<TaskEvent>, segment_samples: u64) {
        match update {
            Update::Done { index, text } => {
                self.finals.insert(index, text);
            }
            Update::Failed { index, error } => {
                self.finals.insert(index, String::new());
                let _ = tx.send(TaskEvent::Warning(format!(
                    "part {} of the recording could not be transcribed ({error}); its audio is kept",
                    index + 1
                )));
            }
            Update::Retrying {
                attempt,
                error,
                wait,
                ..
            } => {
                if attempt >= 2 {
                    let _ = tx.send(TaskEvent::Warning(format!(
                        "transcription is retrying ({error}); trying again in {}s, and nothing is lost",
                        wait.as_secs()
                    )));
                }
            }
        }
        while self.finals.contains_key(&self.settled) {
            self.settled += 1;
        }
        let base = self.settled as u64 * segment_samples;
        if base > self.base {
            self.base = base;
            self.text.clear();
            self.cursor = self.cursor.max(base);
        }
    }

    fn show(&mut self, tx: &mpsc::Sender<TaskEvent>) {
        let shown = self.display();
        if shown != self.shown {
            self.shown = shown.clone();
            let _ = tx.send(TaskEvent::Transcript(shown));
        }
    }
}

fn roll(capture: &Capture, dir: &Path, state: &mut Live, tx: &mpsc::Sender<TaskEvent>) {
    let rate = wav::RATE as u64;
    let from = state
        .cursor
        .saturating_sub(live::OVERLAP.as_secs() * rate)
        .max(state.base);
    let (start, samples) = capture.tail(from, (LIVE_WINDOW_SECS * rate) as usize);
    if (samples.len() as u64) < (live::OVERLAP.as_secs() + 1) * rate {
        return;
    }
    let end = start + samples.len() as u64;
    if live::is_silent(wav::peak(&samples)) {
        state.cursor = end;
        state.silent_slices += 1;
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
    let path = dir.join("live.wav");
    if wav::write(&path, &samples).is_err() {
        return;
    }
    let result = leo_services::ai::transcribe_outcome(&path);
    let _ = std::fs::remove_file(&path);
    match result {
        Ok(outcome) => {
            for f in &outcome.fallbacks {
                let _ = tx.send(TaskEvent::ProviderFallback {
                    from: f.from.clone(),
                    to: f.to.clone(),
                });
            }
            state.interval = live::ROLL_INTERVAL;
            state.cursor = end;
            if !live::is_silence_artifact(&outcome.value) {
                state.text = live::stitch(&state.text, &outcome.value);
            }
        }
        Err(_) => state.interval = live::backoff(state.interval),
    }
}

fn clock_label(word: &str, secs: u64) -> String {
    format!("{word} {}", leo_services::ai::chat::clock(secs))
}

fn transcribe_segment() -> leo_services::session::transcriber::TranscribeFn {
    Arc::new(|path: &Path| {
        leo_services::ai::transcribe_outcome(path)
            .map(|o| o.value)
            .map_err(|e| e.to_string())
    })
}

fn points_of(jotted: &[Jotted]) -> Vec<Point> {
    jotted
        .iter()
        .map(|p| Point {
            at_secs: p.at_secs,
            text: p.text.clone(),
        })
        .collect()
}

fn finish_session(
    mut session: Session,
    transcriber: Transcriber,
    updates: &mpsc::Receiver<Update>,
    state: &mut Live,
    tx: &mpsc::Sender<TaskEvent>,
) {
    session.manifest.stopped = true;
    let _ = session.save();
    transcriber.recording_ended();
    let segment_samples = session.manifest.segment_secs * wav::RATE as u64;
    while !transcriber.is_finished() {
        for update in updates.try_iter() {
            state.apply(update, tx, segment_samples);
        }
        let segments = session.segments();
        let total = segments.len();
        let done = segments
            .iter()
            .filter(|s| {
                matches!(
                    s.state,
                    session::SegmentState::Done(_) | session::SegmentState::Failed(_)
                )
            })
            .count();
        let _ = tx.send(TaskEvent::Progress {
            label: "Transcribing the recording".to_string(),
            steps: (total > 1).then_some((done, total)),
        });
        state.show(tx);
        thread::sleep(POLL);
    }
    transcriber.wait();
    for update in updates.try_iter() {
        state.apply(update, tx, segment_samples);
    }
    let assembled = session.assemble();
    if assembled.is_silent() && session.manifest.points.is_empty() {
        let _ = std::fs::remove_dir_all(&session.dir);
        let _ = tx.send(TaskEvent::Failed(SILENT_RECORDING.to_string()));
        return;
    }
    let _ = tx.send(TaskEvent::Finished {
        transcript: assembled.text(),
        session: Some(session.dir.clone()),
    });
}

pub fn start_listen(
    title: Option<String>,
    append_to: Option<String>,
    dir: String,
    screen: bool,
) -> Job {
    let (tx, rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = Arc::clone(&stop);
    let pause = Arc::new(AtomicBool::new(false));
    let worker_pause = Arc::clone(&pause);
    let points: Arc<std::sync::Mutex<Vec<Jotted>>> = Default::default();
    let worker_points = Arc::clone(&points);

    spawn_guarded(tx.clone(), move || {
        let started = session::root().and_then(|root| {
            let session = Session::create(&root, Manifest::new(title, append_to, &dir, screen))?;
            let lock = session.lock()?;
            match Capture::start(
                &session.dir,
                0,
                session.manifest.segment_secs,
                Source::from_env(screen),
            ) {
                Ok(capture) => Ok((session, lock, capture)),
                Err(e) => {
                    let _ = std::fs::remove_dir_all(&session.dir);
                    Err(e)
                }
            }
        });
        let (mut session, _lock, capture) = match started {
            Ok(parts) => parts,
            Err(e) => {
                let _ = tx.send(TaskEvent::Failed(e.to_string()));
                return;
            }
        };
        let _ = tx.send(TaskEvent::Started {
            label: "Recording".to_string(),
        });
        leo_services::ai::warm_credentials();

        let (updates_tx, updates) = mpsc::channel();
        let transcriber = Transcriber::start(
            &session.dir,
            transcribe_segment(),
            Policy {
                workers: leo_services::ai::parallel_transcriptions(),
                ..Policy::default()
            },
            true,
            updates_tx,
        );
        let segment_samples = session.manifest.segment_secs * wav::RATE as u64;
        let mut state = Live::new();
        let mut last_roll = Instant::now() - live::ROLL_INTERVAL;
        let mut saved_points = 0;

        while !worker_stop.load(Ordering::Relaxed) && !capture.ended() {
            thread::sleep(POLL);
            capture.set_paused(worker_pause.load(Ordering::Relaxed));

            if let Ok(points) = worker_points.lock() {
                if points.len() != saved_points {
                    saved_points = points.len();
                    session.manifest.points = points_of(&points);
                    let _ = session.save();
                }
            }
            for update in updates.try_iter() {
                state.apply(update, &tx, segment_samples);
            }

            let secs = capture.recorded_secs() as u64;
            let backlog = leo_services::session::transcriber::waiting(&session.dir).len();
            let word = if capture.paused() {
                "Paused"
            } else {
                "Recording"
            };
            let mut label = clock_label(word, secs);
            if backlog > 1 {
                label.push_str(&format!(" · {backlog} parts waiting to be transcribed"));
            }
            let _ = tx.send(TaskEvent::Progress { label, steps: None });

            if !capture.paused() && backlog <= 1 && last_roll.elapsed() >= state.interval {
                last_roll = Instant::now();
                let dir = session.dir.clone();
                if let Err(message) = caught(|| roll(&capture, &dir, &mut state, &tx)) {
                    state.interval = live::backoff(state.interval);
                    leo_core::diag::warn(format!(
                        "live transcription hit a problem ({message}); the recording continues"
                    ));
                }
            }
            state.show(&tx);
        }

        if let Some(problem) = capture.problem() {
            let _ = tx.send(TaskEvent::Warning(format!(
                "{problem} What was recorded is being saved."
            )));
        }
        if let Ok(points) = worker_points.lock() {
            session.manifest.points = points_of(&points);
        }
        let _ = tx.send(TaskEvent::Progress {
            label: "Transcribing the recording".to_string(),
            steps: None,
        });
        if let Err(e) = capture.stop() {
            let _ = tx.send(TaskEvent::Warning(e.to_string()));
        }
        finish_session(session, transcriber, &updates, &mut state, &tx);
    });

    Job {
        rx,
        stop,
        pause,
        done: false,
        points,
    }
}

pub fn start_resume(dir: PathBuf) -> Job {
    let (tx, rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(true));

    spawn_guarded(tx.clone(), move || {
        let session = match Session::open(&dir) {
            Ok(s) => s,
            Err(e) => {
                let _ = tx.send(TaskEvent::Failed(e.to_string()));
                return;
            }
        };
        let _lock = match session.lock() {
            Ok(l) => l,
            Err(e) => {
                let _ = tx.send(TaskEvent::Failed(e.to_string()));
                return;
            }
        };
        let _ = tx.send(TaskEvent::Started {
            label: "Finishing an interrupted recording".to_string(),
        });
        if let Err(e) = session.recover_parts() {
            let _ = tx.send(TaskEvent::Warning(format!(
                "some audio could not be recovered ({e})"
            )));
        }
        leo_services::ai::warm_credentials();
        let (updates_tx, updates) = mpsc::channel();
        let transcriber = Transcriber::start(
            &session.dir,
            transcribe_segment(),
            Policy {
                workers: leo_services::ai::parallel_transcriptions(),
                ..Policy::default()
            },
            false,
            updates_tx,
        );
        let mut state = Live::new();
        finish_session(session, transcriber, &updates, &mut state, &tx);
    });

    Job {
        rx,
        stop,
        pause: Arc::new(AtomicBool::new(false)),
        done: false,
        points: Default::default(),
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
        let mut job = super::start_listen(None, None, String::new(), false);

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
            points: Default::default(),
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
            points: Default::default(),
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
            session: None,
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
            points: Default::default(),
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
            points: Default::default(),
        };
        tx.send(TaskEvent::Failed("no microphone".to_string()))
            .unwrap();
        job.drain();
        assert!(job.is_done());
    }

    #[test]
    fn a_panic_is_caught_with_its_message() {
        assert_eq!(caught(|| 5), Ok(5));
        assert_eq!(caught(|| -> u8 { panic!("boom") }), Err("boom".to_string()));
        let detailed = caught(|| -> u8 { panic!("{} failed", "stitching") });
        assert_eq!(detailed, Err("stitching failed".to_string()));
    }

    #[test]
    fn a_task_that_panics_reports_a_failure_instead_of_taking_the_app_down() {
        let (tx, rx) = mpsc::channel();
        spawn_guarded(tx, || panic!("boom in a worker"));
        match rx.recv_timeout(Duration::from_secs(5)).unwrap() {
            TaskEvent::Failed(message) => {
                assert!(message.contains("boom in a worker"), "{message}");
                assert!(message.contains("notes are safe"), "{message}");
            }
            other => panic!("expected a failure, got {other:?}"),
        }
    }
}
