use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::ai::chat::Jotted;
use crate::ai::live;
use crate::session::capture::{Capture, Source};
use crate::session::transcriber::{Policy, TranscribeFn, Transcriber, Update};
use crate::session::{self, wav, Manifest, Point, Session};

const POLL: Duration = Duration::from_millis(250);

pub const SILENT_RECORDING: &str = "No sound was recorded. macOS may not be letting \
     this terminal use the microphone: System Settings > Privacy & Security > \
     Microphone, then restart the terminal. :doctor re-checks it.";

pub const SILENT_BROWSER: &str = "No sound was recorded. Check that the browser may use \
     the microphone and that the right one is chosen, then try again.";

const LIVE_MOST_SECS: u64 = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Started(String),
    Progress {
        label: String,
        steps: Option<(usize, usize)>,
    },
    Clock {
        secs: u64,
        paused: bool,
    },
    Transcript(String),
    Fallback {
        from: String,
        to: String,
    },
    Warning(String),
    Failed(String),
    Finished {
        transcript: String,
        session: PathBuf,
    },
}

pub enum Input {
    Device(Source),
    Fed(Receiver<Vec<i16>>),
}

#[derive(Clone, Default)]
pub struct Controls {
    pub stop: Arc<AtomicBool>,
    pub pause: Arc<AtomicBool>,
    pub points: Arc<Mutex<Vec<Jotted>>>,
}

pub struct Request {
    pub title: Option<String>,
    pub append_to: Option<String>,
    pub dir: String,
    pub screen: bool,
    pub input: Input,
}

pub fn transcribe_segment() -> TranscribeFn {
    Arc::new(|path: &Path| {
        crate::ai::transcribe_outcome(path)
            .map(|o| o.value)
            .map_err(|e| e.to_string())
    })
}

fn caught<T>(work: impl FnOnce() -> T) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).map_err(|payload| {
        payload
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown error".to_string())
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

fn clock_label(word: &str, secs: u64) -> String {
    format!("{word} {}", crate::ai::chat::clock(secs))
}

pub struct Live {
    pace: Duration,
    interval: Duration,
    silent_slices: usize,
    warned_silent: bool,
    finals: std::collections::BTreeMap<u32, String>,
    settled: u32,
    base: u64,
    settled_at: u64,
    committed: String,
    tail: String,
    shown: String,
}

impl Live {
    pub fn new(pace: Duration) -> Live {
        Live {
            pace,
            interval: pace,
            silent_slices: 0,
            warned_silent: false,
            finals: Default::default(),
            settled: 0,
            base: 0,
            settled_at: 0,
            committed: String::new(),
            tail: String::new(),
            shown: String::new(),
        }
    }

    fn display(&self) -> String {
        let mut out = String::new();
        for text in self.finals.values() {
            out = live::stitch(&out, text);
        }
        live::stitch(&out, &live::stitch(&self.committed, &self.tail))
    }

    fn apply(&mut self, update: Update, emit: &dyn Fn(Event), segment_samples: u64) {
        match update {
            Update::Done { index, text } => {
                self.finals.insert(index, text);
            }
            Update::Failed { index, error } => {
                self.finals.insert(index, String::new());
                emit(Event::Warning(format!(
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
                    emit(Event::Warning(format!(
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
            self.committed.clear();
            self.tail.clear();
            self.settled_at = self.settled_at.max(base);
        }
    }

    fn show(&mut self, emit: &dyn Fn(Event)) {
        let shown = self.display();
        if shown != self.shown {
            self.shown = shown.clone();
            emit(Event::Transcript(shown));
        }
    }
}

fn live_pace() -> Duration {
    if crate::ai::parallel_transcriptions() == 1 {
        live::ROLL_INTERVAL
    } else {
        live::CLOUD_ROLL_INTERVAL
    }
}

fn hear(dir: &Path, samples: &[i16], emit: &dyn Fn(Event)) -> Option<String> {
    let path = dir.join("live.wav");
    wav::write(&path, samples).ok()?;
    let result = crate::ai::transcribe_outcome(&path);
    let _ = std::fs::remove_file(&path);
    let outcome = result.ok()?;
    for f in &outcome.fallbacks {
        emit(Event::Fallback {
            from: f.from.clone(),
            to: f.to.clone(),
        });
    }
    Some(if live::is_silence_artifact(&outcome.value) {
        String::new()
    } else {
        outcome.value
    })
}

fn roll(capture: &Capture, dir: &Path, state: &mut Live, emit: &dyn Fn(Event), fed: bool) {
    let rate = wav::RATE as u64;
    let from = state.settled_at.max(state.base);
    let (start, samples) = capture.tail(from, (LIVE_MOST_SECS * rate) as usize);
    if start > state.settled_at {
        state.settled_at = start;
    }
    if (samples.len() as u64) < rate {
        return;
    }
    let end = start + samples.len() as u64;
    if live::is_silent(wav::peak(&samples)) {
        if samples.len() as u64 >= live::SETTLE_AFTER_SECS * rate {
            state.settled_at = end;
            state.tail.clear();
        }
        state.silent_slices += 1;
        if state.silent_slices >= 4 && !state.warned_silent {
            state.warned_silent = true;
            emit(if fed {
                Event::Fallback {
                    from: "no sound from the microphone".to_string(),
                    to: "check that the browser may use it and the right one is chosen".to_string(),
                }
            } else {
                Event::Fallback {
                    from: "no sound from the microphone".to_string(),
                    to: "check System Settings > Privacy & Security > Microphone".to_string(),
                }
            });
        }
        return;
    }
    state.silent_slices = 0;
    let mut open = &samples[..];
    if samples.len() as u64 >= live::SETTLE_AFTER_SECS * rate {
        let cut = live::quietest(
            &samples,
            (live::SETTLE_EARLIEST_SECS * rate) as usize,
            samples.len() - (live::SETTLE_KEEP_SECS * rate) as usize,
            (rate / 10) as usize,
        );
        let Some(settled) = hear(dir, &samples[..cut], emit) else {
            state.interval = live::backoff(state.interval);
            return;
        };
        state.committed = live::stitch(&state.committed, &settled);
        state.settled_at = start + cut as u64;
        state.tail.clear();
        open = &samples[cut..];
    }
    match hear(dir, open, emit) {
        Some(text) => {
            state.interval = state.pace;
            state.tail = text;
        }
        None => state.interval = live::backoff(state.interval),
    }
}

pub fn finish(
    mut session: Session,
    transcriber: Transcriber,
    updates: &Receiver<Update>,
    state: &mut Live,
    emit: &dyn Fn(Event),
    silent: &str,
) {
    session.manifest.stopped = true;
    let _ = session.save();
    transcriber.recording_ended();
    let segment_samples = session.manifest.segment_secs * wav::RATE as u64;
    while !transcriber.is_finished() {
        for update in updates.try_iter() {
            state.apply(update, emit, segment_samples);
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
        emit(Event::Progress {
            label: "Transcribing the recording".to_string(),
            steps: (total > 1).then_some((done, total)),
        });
        state.show(emit);
        std::thread::sleep(POLL);
    }
    transcriber.wait();
    for update in updates.try_iter() {
        state.apply(update, emit, segment_samples);
    }
    let assembled = session.assemble();
    if assembled.is_silent() && session.manifest.points.is_empty() {
        let _ = std::fs::remove_dir_all(&session.dir);
        emit(Event::Failed(silent.to_string()));
        return;
    }
    emit(Event::Finished {
        transcript: assembled.text(),
        session: session.dir.clone(),
    });
}

pub fn resume(dir: &Path, emit: &dyn Fn(Event)) {
    let session = match Session::open(dir) {
        Ok(s) => s,
        Err(e) => return emit(Event::Failed(e.to_string())),
    };
    let _lock = match session.lock() {
        Ok(l) => l,
        Err(e) => return emit(Event::Failed(e.to_string())),
    };
    emit(Event::Started(
        "Finishing an interrupted recording".to_string(),
    ));
    if let Err(e) = session.recover_parts() {
        emit(Event::Warning(format!(
            "some audio could not be recovered ({e})"
        )));
    }
    crate::ai::warm_credentials();
    let (updates_tx, updates) = mpsc::channel();
    let transcriber = Transcriber::start(
        &session.dir,
        transcribe_segment(),
        Policy {
            workers: crate::ai::parallel_transcriptions(),
            ..Policy::default()
        },
        false,
        updates_tx,
    );
    let mut state = Live::new(live::ROLL_INTERVAL);
    finish(
        session,
        transcriber,
        &updates,
        &mut state,
        emit,
        SILENT_RECORDING,
    );
}

pub fn record(request: Request, controls: &Controls, emit: &dyn Fn(Event)) {
    let Request {
        title,
        append_to,
        dir,
        screen,
        input,
    } = request;
    let fed = matches!(input, Input::Fed(_));
    let started = session::root().and_then(|root| {
        let session = Session::create(&root, Manifest::new(title, append_to, &dir, screen))?;
        let lock = session.lock()?;
        let opened = match input {
            Input::Device(source) => {
                Capture::start(&session.dir, 0, session.manifest.segment_secs, source)
            }
            Input::Fed(rx) => Capture::fed(&session.dir, 0, session.manifest.segment_secs, rx),
        };
        match opened {
            Ok(capture) => Ok((session, lock, capture)),
            Err(e) => {
                let _ = std::fs::remove_dir_all(&session.dir);
                Err(e)
            }
        }
    });
    let (mut session, _lock, capture) = match started {
        Ok(parts) => parts,
        Err(e) => return emit(Event::Failed(e.to_string())),
    };
    emit(Event::Started("Recording".to_string()));
    crate::ai::warm_credentials();

    let (updates_tx, updates) = mpsc::channel();
    let transcriber = Transcriber::start(
        &session.dir,
        transcribe_segment(),
        Policy {
            workers: crate::ai::parallel_transcriptions(),
            ..Policy::default()
        },
        true,
        updates_tx,
    );
    let segment_samples = session.manifest.segment_secs * wav::RATE as u64;
    let mut state = Live::new(live_pace());
    let mut last_roll = Instant::now() - live::ROLL_INTERVAL;
    let mut saved_points = 0;

    while !controls.stop.load(Ordering::Relaxed) && !capture.ended() {
        std::thread::sleep(POLL);
        capture.set_paused(controls.pause.load(Ordering::Relaxed));

        if let Ok(points) = controls.points.lock() {
            if points.len() != saved_points {
                saved_points = points.len();
                session.manifest.points = points_of(&points);
                let _ = session.save();
            }
        }
        for update in updates.try_iter() {
            state.apply(update, emit, segment_samples);
        }

        let secs = capture.recorded_secs() as u64;
        let backlog = session::transcriber::waiting(&session.dir).len();
        let word = if capture.paused() {
            "Paused"
        } else {
            "Recording"
        };
        let mut label = clock_label(word, secs);
        if backlog > 1 {
            label.push_str(&format!(" · {backlog} parts waiting to be transcribed"));
        }
        emit(Event::Clock {
            secs,
            paused: capture.paused(),
        });
        emit(Event::Progress { label, steps: None });

        if !capture.paused() && backlog <= 1 && last_roll.elapsed() >= state.interval {
            last_roll = Instant::now();
            let dir = session.dir.clone();
            if let Err(message) = caught(|| roll(&capture, &dir, &mut state, emit, fed)) {
                state.interval = live::backoff(state.interval);
                leo_core::diag::warn(format!(
                    "live transcription hit a problem ({message}); the recording continues"
                ));
            }
        }
        state.show(emit);
    }

    if let Some(problem) = capture.problem() {
        emit(Event::Warning(format!(
            "{problem} What was recorded is being saved."
        )));
    }
    if let Ok(points) = controls.points.lock() {
        session.manifest.points = points_of(&points);
    }
    emit(Event::Progress {
        label: "Transcribing the recording".to_string(),
        steps: None,
    });
    if let Err(e) = capture.stop() {
        emit(Event::Warning(e.to_string()));
    }
    let silent = if fed {
        SILENT_BROWSER
    } else {
        SILENT_RECORDING
    };
    finish(session, transcriber, &updates, &mut state, emit, silent);
}

pub struct Written {
    pub title: String,
    pub body: String,
    pub problems: Vec<String>,
}

pub fn write_up(
    dir: &Path,
    existing: Option<&str>,
    progress: &(dyn Fn(usize, usize) + Sync),
) -> anyhow::Result<Written> {
    let session = Session::open(dir)?;
    let assembled = session.assemble();
    let points = session.manifest.jotted();
    let fallback = session
        .manifest
        .started
        .with_timezone(&chrono::Local)
        .format("Recording, %b %-d %-I:%M %p")
        .to_string();
    let structured = crate::ai::long::structure_recording(
        &assembled.parts,
        &points,
        existing,
        &fallback,
        &|prompt, max| crate::ai::chat_outcome(prompt, max).map(|o| o.value),
        progress,
    );
    let notice = session::failure_notice(&assembled.failed, dir);
    let body = if notice.is_empty() {
        structured.body
    } else {
        format!("{notice}\n\n{}", structured.body)
    };
    session.finish()?;
    Ok(Written {
        title: structured.title,
        body,
        problems: structured.problems,
    })
}
