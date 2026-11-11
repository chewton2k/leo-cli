use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::ai::chat::Jotted;
use crate::ai::live;
use crate::ai::long::{draft_key, draft_prompt, Drafted};
use crate::session::capture::{Capture, Source};
use crate::session::transcriber::{Policy, TranscribeFn, Transcriber, Update};
use crate::session::{self, wav, Manifest, Point, Session};

const POLL: Duration = Duration::from_millis(250);

pub const SILENT_RECORDING: &str = "No sound was recorded. macOS may not be letting \
     leo use the microphone: System Settings > Privacy & Security > \
     Microphone, then quit and reopen leo.";

const LIVE_MOST_SECS: u64 = 20;

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Started(String),
    Progress {
        label: String,
        steps: Option<(usize, usize)>,
    },
    Clock {
        secs: u64,
        paused: bool,
        level: f32,
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

#[derive(Clone, Default)]
pub struct Controls {
    pub finish_now: Arc<AtomicBool>,
    pub stop: Arc<AtomicBool>,
    pub pause: Arc<AtomicBool>,
    pub points: Arc<Mutex<Vec<Jotted>>>,
    pub wants: Arc<Mutex<Option<String>>>,
    pub preview: Option<Arc<AtomicBool>>,
}

impl Controls {
    fn preview_enabled(&self) -> bool {
        self.preview
            .as_ref()
            .is_none_or(|visible| visible.load(Ordering::Acquire))
    }
}

pub struct Request {
    pub id: Option<String>,
    pub profile: Option<leo_core::recording::Profile>,
    pub title: Option<String>,
    pub append_to: Option<String>,
    pub dir: String,
    pub screen: bool,
    pub input: Source,
    pub draft: Option<DraftFn>,
}

pub fn transcribe_segment() -> TranscribeFn {
    Arc::new(|path: &Path| {
        let session = Session::open(path.parent().unwrap_or(Path::new("."))).ok();
        let start = Instant::now();
        let outcome = crate::ai::transcribe_outcome(path).map_err(|e| e.to_string());
        if let Some(session) = session {
            let detail = match &outcome {
                Ok(o) => format!(
                    "{} via {}; {:.1}s; {} fallback(s)",
                    path.file_name().unwrap_or_default().to_string_lossy(),
                    o.provider,
                    start.elapsed().as_secs_f64(),
                    o.fallbacks.len()
                ),
                Err(_) => format!(
                    "{} failed after {:.1}s",
                    path.file_name().unwrap_or_default().to_string_lossy(),
                    start.elapsed().as_secs_f64()
                ),
            };
            session.trace("transcription", &detail);
        }
        outcome.map(|o| o.value)
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

pub type DraftFn =
    Arc<dyn Fn(crate::ai::chat::Prompt, u32) -> anyhow::Result<String> + Send + Sync>;

const DRAFTS: &str = "drafts.json";
const DRAFT_EVERY: Duration = Duration::from_secs(2);

pub struct Drafter {
    chat: DraftFn,
    budget: crate::ai::budget::Budget,
    done: Option<Drafted>,
    profile: leo_core::recording::Profile,
    running: Option<std::thread::JoinHandle<Option<Drafted>>>,
    ended: bool,
}

impl Drafter {
    pub fn new(chat: DraftFn, budget: crate::ai::budget::Budget) -> Drafter {
        Drafter {
            chat,
            budget,
            done: None,
            profile: Default::default(),
            running: None,
            ended: false,
        }
    }

    pub fn tick(&mut self, session: &Session, backlog: usize) {
        if self.running.as_ref().is_some_and(|run| run.is_finished()) {
            match self.running.take().map(|run| run.join()) {
                Some(Ok(Some(drafted))) => self.done = Some(drafted),
                _ => self.ended = true,
            }
        }
        if self.ended || self.running.is_some() || backlog > 0 {
            return;
        }
        if session.manifest.profile != self.profile {
            self.done = None;
            self.profile = session.manifest.profile.clone();
        }
        let upto = self.done.as_ref().map_or(0, |d| d.upto);
        let segments: Vec<session::Segment> = session
            .segments()
            .into_iter()
            .filter(|s| s.index >= upto)
            .collect();
        let run: Vec<session::Segment> = segments
            .iter()
            .enumerate()
            .take_while(|(i, s)| {
                s.index == upto + *i as u32 && matches!(s.state, session::SegmentState::Done(_))
            })
            .map(|(_, s)| s.clone())
            .collect();
        if run.is_empty() {
            self.ended = segments.first().is_some_and(|s| {
                s.index == upto && matches!(s.state, session::SegmentState::Failed(_))
            });
            return;
        }
        let fresh = session::assemble(&run, session.manifest.segment_secs).parts;
        let all = session.assemble().parts;
        let points = session.manifest.jotted();
        let profile = self.profile.clone();
        let dir = session.dir.clone();
        if fresh.iter().all(|p| p.text.trim().is_empty()) {
            if let Some(done) = self.done.take() {
                let upto = upto + run.len() as u32;
                let next = Drafted {
                    upto,
                    key: draft_key(&all, &points, &profile, upto),
                    notes: done.notes,
                };
                keep_draft(&dir, &next);
                self.done = Some(next);
            }
            return;
        }
        let notes = self
            .done
            .as_ref()
            .map_or("", |d| d.notes.as_str())
            .to_string();
        let Some((prompt, n)) = draft_prompt(&fresh, &notes, &points, self.budget) else {
            self.ended = true;
            return;
        };
        let Ok(lease) = leo_core::activity::global().begin() else {
            return;
        };
        let upto = fresh[n - 1].index + 1;
        let key = draft_key(&all, &points, &profile, upto);
        let chat = self.chat.clone();
        let prompt = crate::ai::long::with_profile(prompt, &profile);
        self.running = Some(std::thread::spawn(move || {
            let _lease = lease;
            let reply = caught(|| chat(prompt, crate::ai::STRUCTURE_MAX_TOKENS))
                .ok()?
                .ok()?;
            let reply = crate::ai::chat::clean_reply(&reply);
            if reply.is_empty() {
                return None;
            }
            let drafted = Drafted {
                upto,
                key,
                notes: [notes.trim(), reply.as_str()]
                    .iter()
                    .filter(|s| !s.is_empty())
                    .copied()
                    .collect::<Vec<_>>()
                    .join("\n\n"),
            };
            keep_draft(&dir, &drafted);
            Some(drafted)
        }));
    }

    pub fn finish(mut self, wait: bool, emit: &dyn Fn(Event)) {
        let Some(run) = self.running.take() else {
            return;
        };
        if wait {
            emit(Event::Progress {
                label: "Writing the notes so far".to_string(),
                steps: None,
            });
            let _ = run.join();
        }
    }
}

fn keep_draft(dir: &Path, drafted: &Drafted) {
    if !dir.join("session.json").exists() {
        return;
    }
    if let Ok(bytes) = serde_json::to_vec(drafted) {
        let _ = leo_core::paths::write_atomic(&dir.join(DRAFTS), bytes);
    }
}

pub fn drafts_for(
    session: &Session,
    parts: &[session::Part],
    existing: Option<&str>,
) -> Option<Drafted> {
    if existing.is_some() {
        return None;
    }
    let drafted: Drafted =
        serde_json::from_slice(&std::fs::read(session.dir.join(DRAFTS)).ok()?).ok()?;
    let key = draft_key(
        parts,
        &session.manifest.jotted(),
        &session.manifest.profile,
        drafted.upto,
    );
    (drafted.key == key).then_some(drafted)
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

fn live_model_ready() -> bool {
    !crate::speech::engine::engine().dictating()
        && crate::speech::library::library()
            .and_then(|l| l.selected())
            .is_ok_and(|m| m.live)
}

fn live_pace() -> Duration {
    if crate::ai::parallel_transcriptions() == 1 {
        live::ROLL_INTERVAL
    } else {
        live::CLOUD_ROLL_INTERVAL
    }
}

fn hear(samples: &[i16]) -> Option<String> {
    let selected = crate::speech::library::library().ok()?.selected().ok()?;
    let pcm = samples.iter().map(|s| *s as f32 / 32768.0).collect();
    let text = crate::speech::engine::engine()
        .transcribe(selected, pcm, crate::speech::engine::Priority::Live)
        .ok()?;
    Some(if live::is_silence_artifact(&text) {
        String::new()
    } else {
        text
    })
}

fn roll(capture: &Capture, state: &mut Live, emit: &dyn Fn(Event)) {
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
            emit(Event::Fallback {
                from: "no sound from the microphone".to_string(),
                to: "check System Settings > Privacy & Security > Microphone".to_string(),
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
        let Some(settled) = hear(&samples[..cut]) else {
            state.interval = live::backoff(state.interval);
            return;
        };
        state.committed = live::stitch(&state.committed, &settled);
        state.settled_at = start + cut as u64;
        state.tail.clear();
        open = &samples[cut..];
    }
    match hear(open) {
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
    finish_now: &AtomicBool,
) {
    session.manifest.stopped = true;
    if let Err(e) = session.save() {
        session.trace(
            "save-failed",
            &format!("could not mark the recording stopped: {e}"),
        );
    }
    transcriber.recording_ended();
    let segment_samples = session.manifest.segment_secs * wav::RATE as u64;
    let mut no_model = false;
    let mut take = |update: Update, state: &mut Live| {
        if let Update::Failed { error, .. } = &update {
            no_model |= session::transcriber::waiting_for_model(error);
        }
        state.apply(update, emit, segment_samples);
    };
    while !transcriber.is_finished() {
        if finish_now.load(Ordering::Relaxed) {
            transcriber.cancel();
        }
        for update in updates.try_iter() {
            take(update, state);
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
    if finish_now.load(Ordering::Relaxed) {
        for index in session::transcriber::waiting(&session.dir) {
            let path = session.dir.join(format!("seg-{index:05}.err"));
            let _ = std::fs::write(
                path,
                "Finished with available transcript; audio kept for retry",
            );
        }
        session.trace(
            "finish-available",
            "Queued transcription skipped by the user; audio kept for retry",
        );
    }
    for update in updates.try_iter() {
        take(update, state);
    }
    let assembled = session.assemble();
    if assembled.pending > 0 {
        emit(Event::Failed(if no_model {
            "No speech model was ready, so this recording is not transcribed yet. Its audio is kept: download a model in Settings → Speech models, then recover the recording.".into()
        } else {
            "Some transcript files could not be saved. Audio is kept; free disk space and recover the session.".into()
        }));
        return;
    }
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
    resume_with_controls(dir, &Controls::default(), emit)
}

pub fn resume_with_controls(dir: &Path, controls: &Controls, emit: &dyn Fn(Event)) {
    let _model = crate::speech::engine::engine().pin();
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
        &controls.finish_now,
    );
}

fn take_wants(session: &mut Session, controls: &Controls) -> bool {
    let Some(wants) = controls.wants.lock().ok().and_then(|w| w.clone()) else {
        return false;
    };
    if session.manifest.profile.wants == wants {
        return false;
    }
    session.manifest.profile.wants = wants;
    true
}

pub fn record(request: Request, controls: &Controls, emit: &dyn Fn(Event)) {
    let _activity = match leo_core::activity::global().begin() {
        Ok(lease) => lease,
        Err(e) => return emit(Event::Failed(e.to_string())),
    };
    let _audio = match crate::dictation::audio_gate::gate().recording() {
        Ok(guard) => guard,
        Err(e) => return emit(Event::Failed(e.to_string())),
    };
    let _awake = super::awake::Awake::recording();
    let _model = crate::speech::engine::engine().pin();
    let Request {
        id,
        profile,
        title,
        append_to,
        dir,
        screen,
        input,
        draft,
    } = request;
    let mut drafter = draft
        .filter(|_| append_to.is_none())
        .map(|chat| Drafter::new(chat, crate::ai::writing_budget()));
    let started = session::root().and_then(|root| {
        let mut manifest = Manifest::new(title, append_to, &dir, screen);
        if let Some(id) = id {
            manifest.id = id;
        }
        if let Some(profile) = profile {
            manifest.profile = profile;
        }
        let session = Session::create(&root, manifest)?;
        session.trace("capture", "Recording started");
        let lock = session.lock()?;
        let opened = Capture::start(&session.dir, 0, session.manifest.segment_secs, input);
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
    let mut last_draft = Instant::now();
    let mut saved_points = 0;
    let mut save_failed = false;
    let mut heard_up_to = 0;
    let mut lost_up_to = 0;

    while !controls.stop.load(Ordering::Relaxed) && !capture.ended() {
        std::thread::sleep(POLL);
        capture.set_paused(controls.pause.load(Ordering::Relaxed));

        let latest_points = controls.points.lock().ok().map(|points| points_of(&points));
        if let Some(points) = latest_points.filter(|points| points.len() != saved_points) {
            let count = points.len();
            session.manifest.points = points;
            match session.save() {
                Ok(()) => {
                    saved_points = count;
                    save_failed = false;
                }
                Err(e) if !std::mem::replace(&mut save_failed, true) => {
                    session.trace("save-failed", &format!("could not save typed points: {e}"));
                }
                Err(_) => {}
            }
        }
        if take_wants(&mut session, controls) {
            let _ = session.save();
        }
        for update in updates.try_iter() {
            state.apply(update, emit, segment_samples);
        }

        let lost = capture.dropped_samples();
        if lost > lost_up_to {
            let warning = format!(
                "An audio gap of {:.2}s occurred near {}",
                (lost - lost_up_to) as f64 / wav::RATE as f64,
                crate::ai::chat::clock(capture.recorded_secs() as u64)
            );
            session.trace("audio-gap", &warning);
            emit(Event::Warning(warning));
            lost_up_to = lost;
        }
        let secs = capture.recorded_secs() as u64;
        let backlog = session::transcriber::waiting(&session.dir).len();
        if let Some(drafter) = drafter
            .as_mut()
            .filter(|_| last_draft.elapsed() >= DRAFT_EVERY)
        {
            last_draft = Instant::now();
            drafter.tick(&session, backlog);
        }
        let word = if capture.paused() {
            "Paused"
        } else {
            "Recording"
        };
        let mut label = clock_label(word, secs);
        if backlog > 1 {
            label.push_str(&format!(" · {backlog} parts waiting to be transcribed"));
        }
        let recorded = capture.recorded_samples();
        let level = if capture.paused() {
            0.0
        } else {
            capture.level_since(heard_up_to)
        };
        heard_up_to = recorded;
        emit(Event::Clock {
            secs,
            paused: capture.paused(),
            level,
        });
        emit(Event::Progress { label, steps: None });

        if !capture.paused()
            && controls.preview_enabled()
            && backlog == 0
            && last_roll.elapsed() >= state.interval
            && live_model_ready()
        {
            last_roll = Instant::now();
            if let Err(message) = caught(|| roll(&capture, &mut state, emit)) {
                state.interval = live::backoff(state.interval);
                leo_core::diag::warn(format!(
                    "live transcription hit a problem ({message}); the recording continues"
                ));
            }
        }
        state.show(emit);
    }

    let lost = capture.dropped_samples();
    if lost > lost_up_to {
        let warning = format!(
            "An audio gap of {:.2}s occurred at the end of capture",
            (lost - lost_up_to) as f64 / wav::RATE as f64
        );
        session.trace("audio-gap", &warning);
        emit(Event::Warning(warning));
    }
    if let Some(problem) = capture.problem() {
        emit(Event::Warning(format!(
            "{problem} What was recorded is being saved."
        )));
    }
    final_points(&mut session, controls, emit);
    match capture.stop_with_stats() {
        Ok((samples, _)) => {
            session.manifest.duration_secs = Some(samples.div_ceil(wav::RATE as u64))
        }
        Err(e) => emit(Event::Warning(e.to_string())),
    }
    finish(
        session,
        transcriber,
        &updates,
        &mut state,
        emit,
        SILENT_RECORDING,
        &controls.finish_now,
    );
    if let Some(drafter) = drafter {
        drafter.finish(!controls.finish_now.load(Ordering::Relaxed), emit);
    }
}

fn final_points(session: &mut Session, controls: &Controls, emit: &dyn Fn(Event)) {
    emit(Event::Progress {
        label: "Transcribing the recording".to_string(),
        steps: None,
    });
    if let Ok(points) = controls.points.lock() {
        session.manifest.points = points_of(&points);
    }
    take_wants(session, controls);
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Written {
    pub title: String,
    pub body: String,
    pub problems: Vec<String>,
    #[serde(default)]
    pub writer_failed: bool,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Prepared {
    key: String,
    written: Written,
}

fn reusable(bytes: &[u8], key: &str) -> Option<Written> {
    serde_json::from_slice::<Prepared>(bytes)
        .ok()
        .filter(|prepared| prepared.key == key && !prepared.written.writer_failed)
        .map(|prepared| prepared.written)
}

pub fn write_up(
    dir: &Path,
    existing: Option<&str>,
    progress: &(dyn Fn(usize, usize) + Sync),
) -> anyhow::Result<Written> {
    let session = Session::open(dir)?;
    session.trace(
        "generation",
        "Writing notes from transcript and typed points",
    );
    let assembled = session.assemble();
    let points = session.manifest.jotted();
    use sha2::{Digest, Sha256};
    let key = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        Sha256::digest(serde_json::to_vec(&(
            assembled.text(),
            &assembled.failed,
            &session.manifest.points,
            &session.manifest.profile,
            existing,
        ))?),
    );
    let cached = dir.join("prepared.json");
    if let Some(written) = std::fs::read(&cached)
        .ok()
        .and_then(|bytes| reusable(&bytes, &key))
    {
        return Ok(written);
    }

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
        drafts_for(&session, &assembled.parts, existing).as_ref(),
        &fallback,
        &|prompt, max| {
            crate::ai::chat_outcome(
                crate::ai::long::with_profile(prompt, &session.manifest.profile),
                max,
            )
            .map(|o| {
                session.trace(
                    "generation-provider",
                    &format!("{}; {} fallback(s)", o.provider, o.fallbacks.len()),
                );
                o.value
            })
        },
        progress,
        crate::ai::writing_budget(),
    );
    let notice = session::failure_notice(&assembled.failed, dir);
    let body = if notice.is_empty() {
        structured.body
    } else {
        format!("{notice}\n\n{}", structured.body)
    };
    let label = session
        .manifest
        .profile
        .context
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("Calendar event:"))
        .and_then(|event| {
            leo_core::filename::tracking_label(event)
                .or_else(|| leo_core::filename::series_label(event))
        });
    let written = Written {
        title: leo_core::filename::with_label(&structured.title, label.as_deref()),
        body,
        problems: structured.problems,
        writer_failed: structured.writer_failed,
    };
    leo_core::recording::write_json(
        &cached,
        &Prepared {
            key,
            written: written.clone(),
        },
    )?;
    session.trace(
        "prepared",
        "Generated result saved; waiting for note persistence",
    );
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cached_write_up_is_reused_only_when_the_writer_succeeded() {
        let cached = |failed| {
            serde_json::to_vec(&Prepared {
                key: "k".into(),
                written: Written {
                    title: "T".into(),
                    body: "B".into(),
                    problems: Vec::new(),
                    writer_failed: failed,
                },
            })
            .unwrap()
        };
        assert_eq!(reusable(&cached(false), "k").unwrap().body, "B");
        assert!(reusable(&cached(false), "other").is_none());
        assert!(reusable(&cached(true), "k").is_none());
    }

    #[test]
    #[ignore = "benchmark; run by hand"]
    fn benchmark_preview_audio_preparation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("live.wav");
        let samples = vec![3000i16; 20 * wav::RATE as usize];
        for trial in 0..5 {
            let start = Instant::now();
            for _ in 0..8 {
                wav::write(&path, &samples).unwrap();
                std::hint::black_box(
                    samples
                        .iter()
                        .map(|s| *s as f32 / 32768.0)
                        .collect::<Vec<_>>(),
                );
                std::fs::remove_file(&path).unwrap();
            }
            let before = start.elapsed();
            let start = Instant::now();
            for _ in 0..8 {
                std::hint::black_box(
                    samples
                        .iter()
                        .map(|s| *s as f32 / 32768.0)
                        .collect::<Vec<_>>(),
                );
            }
            println!("trial={trial} previews=8 audio_secs=20 before_us={} after_us={} before_syncs=8 after_syncs=0", before.as_micros(), start.elapsed().as_micros());
        }
    }
    #[test]
    fn hiding_only_disables_preview_and_resume_keeps_capture_controls_intact() {
        let mut controls = Controls::default();
        assert!(controls.preview_enabled());
        let visible = Arc::new(AtomicBool::new(true));
        controls.preview = Some(visible.clone());
        for shown in [true, false, true, false] {
            visible.store(shown, Ordering::Release);
            assert_eq!(controls.preview_enabled(), shown);
            assert!(!controls.stop.load(Ordering::Acquire));
            assert!(!controls.pause.load(Ordering::Acquire));
            assert!(!controls.finish_now.load(Ordering::Acquire));
        }
    }

    #[test]
    fn a_recording_stopped_without_a_model_ends_unsaved_and_stays_recoverable() {
        let root = tempfile::tempdir().unwrap();
        let session = Session::create(root.path(), Manifest::new(None, None, "", false)).unwrap();
        let dir = session.dir.clone();
        wav::write(&dir.join(wav::done_name(0)), &vec![3000i16; 1600]).unwrap();
        let (tx, updates) = mpsc::channel();
        let transcriber = Transcriber::start(
            &dir,
            std::sync::Arc::new(|_| Err("Download a model in Settings → Speech models.".into())),
            Policy {
                first_wait: Duration::from_millis(5),
                idle: Duration::from_millis(5),
                finish_limit: Duration::from_millis(40),
                ..Policy::default()
            },
            false,
            tx,
        );
        let events = std::sync::Mutex::new(Vec::new());
        finish(
            session,
            transcriber,
            &updates,
            &mut Live::new(live::ROLL_INTERVAL),
            &|event| events.lock().unwrap().push(event),
            SILENT_RECORDING,
            &AtomicBool::new(false),
        );
        let events = events.into_inner().unwrap();
        assert!(
            matches!(events.last(), Some(Event::Failed(why)) if why.contains("speech model")),
            "{:?}",
            events.last()
        );
        assert!(dir.join(wav::done_name(0)).exists());
        assert_eq!(Session::unfinished(root.path()), [dir]);
    }

    #[test]
    fn points_arriving_with_stop_are_mirrored_before_the_final_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let mut session =
            Session::create(dir.path(), Manifest::new(None, None, "", false)).unwrap();
        let controls = Controls::default();
        final_points(&mut session, &controls, &|event| {
            if matches!(event, Event::Progress { .. }) {
                controls.points.lock().unwrap().push(Jotted {
                    at_secs: 4,
                    text: "The pasted slide".into(),
                });
            }
        });
        assert_eq!(session.manifest.points.len(), 1);
        assert_eq!(session.manifest.points[0].text, "The pasted slide");
    }

    fn drafting_session(dir: &Path, files: &[(&str, &str)]) -> Session {
        let session = Session::create(dir, Manifest::new(None, None, "", false)).unwrap();
        for (name, text) in files {
            std::fs::write(session.dir.join(name), text).unwrap();
        }
        session
    }

    fn counting(calls: &Arc<std::sync::atomic::AtomicUsize>, reply: &'static str) -> DraftFn {
        let calls = calls.clone();
        Arc::new(move |_, _| {
            calls.fetch_add(1, Ordering::SeqCst);
            if reply.is_empty() {
                anyhow::bail!("busy")
            }
            Ok(reply.to_string())
        })
    }

    fn settle(drafter: &mut Drafter, session: &Session) {
        let until = Instant::now() + Duration::from_secs(10);
        while drafter.running.is_some() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(10));
            drafter.tick(session, 1);
        }
    }

    fn budget() -> crate::ai::budget::Budget {
        crate::ai::budget::Budget::AGENT
    }

    #[test]
    fn drafts_cover_only_the_finished_start_of_the_recording() {
        let root = tempfile::tempdir().unwrap();
        let session = drafting_session(
            root.path(),
            &[
                ("seg-00000.txt", "disks have platters"),
                ("seg-00001.wav", ""),
                ("seg-00002.txt", "caching later"),
            ],
        );
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut drafter = Drafter::new(counting(&calls, "## Platters\n- spin"), budget());
        drafter.tick(&session, 1);
        assert_eq!(calls.load(Ordering::SeqCst), 0, "speech still behind");
        drafter.tick(&session, 0);
        settle(&mut drafter, &session);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let done = drafter.done.clone().unwrap();
        assert_eq!((done.upto, done.notes.as_str()), (1, "## Platters\n- spin"));
        let parts = session.assemble().parts;
        assert_eq!(drafts_for(&session, &parts, None), Some(done));
        assert_eq!(drafts_for(&session, &parts, Some("old")), None);
        let mut changed = Session::open(&session.dir).unwrap();
        changed.manifest.profile.wants = "proofs only".into();
        assert_eq!(drafts_for(&changed, &parts, None), None);
    }

    #[test]
    fn one_draft_runs_at_a_time_and_the_next_continues_the_notes() {
        let root = tempfile::tempdir().unwrap();
        let session = drafting_session(root.path(), &[("seg-00000.txt", "first stretch")]);
        let release = Arc::new(AtomicBool::new(false));
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (gate, count) = (release.clone(), calls.clone());
        let chat: DraftFn = Arc::new(move |prompt: crate::ai::chat::Prompt, _| {
            let n = count.fetch_add(1, Ordering::SeqCst);
            while !gate.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(5));
            }
            if n == 1 {
                assert!(prompt.user.contains("## One"));
            }
            Ok(format!("## {}\n- x", ["One", "Two"][n]))
        });
        let mut drafter = Drafter::new(chat, budget());
        drafter.tick(&session, 0);
        std::fs::write(session.dir.join("seg-00001.txt"), "second stretch").unwrap();
        std::thread::sleep(Duration::from_millis(50));
        drafter.tick(&session, 0);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        release.store(true, Ordering::SeqCst);
        settle(&mut drafter, &session);
        drafter.tick(&session, 0);
        settle(&mut drafter, &session);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let done = drafter.done.clone().unwrap();
        assert_eq!(
            (done.upto, done.notes.as_str()),
            (2, "## One\n- x\n\n## Two\n- x")
        );
    }

    #[test]
    fn a_failed_segment_or_a_failed_draft_ends_drafting() {
        let root = tempfile::tempdir().unwrap();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let failed = drafting_session(root.path(), &[("seg-00000.err", "x")]);
        let mut drafter = Drafter::new(counting(&calls, "## A"), budget());
        drafter.tick(&failed, 0);
        assert!(drafter.ended);
        let session = drafting_session(root.path(), &[("seg-00000.txt", "words")]);
        let mut drafter = Drafter::new(counting(&calls, ""), budget());
        drafter.tick(&session, 0);
        settle(&mut drafter, &session);
        std::fs::write(session.dir.join("seg-00001.txt"), "more").unwrap();
        drafter.tick(&session, 0);
        assert!(drafter.ended);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!session.dir.join(DRAFTS).exists());
    }

    #[test]
    fn a_silent_stretch_moves_the_draft_on_without_a_call() {
        let root = tempfile::tempdir().unwrap();
        let session = drafting_session(root.path(), &[("seg-00000.txt", "words")]);
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut drafter = Drafter::new(counting(&calls, "## A\n- a"), budget());
        drafter.tick(&session, 0);
        settle(&mut drafter, &session);
        std::fs::write(session.dir.join("seg-00001.txt"), "  ").unwrap();
        drafter.tick(&session, 0);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(drafter.done.as_ref().unwrap().upto, 2);
        let parts = session.assemble().parts;
        assert!(drafts_for(&session, &parts, None).is_some_and(|d| d.upto == 2));
    }

    #[test]
    fn a_draft_that_lands_after_the_session_is_gone_writes_nothing() {
        let root = tempfile::tempdir().unwrap();
        let session = drafting_session(root.path(), &[("seg-00000.txt", "words")]);
        let release = Arc::new(AtomicBool::new(false));
        let gate = release.clone();
        let chat: DraftFn = Arc::new(move |_, _| {
            while !gate.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok("## A".into())
        });
        let mut drafter = Drafter::new(chat, budget());
        drafter.tick(&session, 0);
        std::fs::remove_dir_all(&session.dir).unwrap();
        release.store(true, Ordering::SeqCst);
        drafter.finish(true, &|_| {});
        assert!(!session.dir.exists());
    }
}
