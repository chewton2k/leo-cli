use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde::{Deserialize, Serialize};

use crate::routes::auth::{local_request, Peer};
use crate::routes::notes::{directory as valid_directory, save};
use crate::{store_now, AppState};

pub const FORGOTTEN_AFTER: Duration = Duration::from_secs(60);
pub const MOST_POINTS: usize = 200;
pub const MOST_TRANSCRIPT_CHARS: usize = 200_000;
pub const AUDIO_BYTES: usize = 4 * 1024 * 1024;
pub const LEVELS_KEPT: usize = 48;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Browser,
    Tab,
    Microphone,
    Screen,
}

impl Source {
    pub fn fed_by_browser(self) -> bool {
        matches!(self, Source::Browser | Source::Tab)
    }

    pub fn on_this_computer(self) -> bool {
        matches!(self, Source::Microphone | Source::Screen)
    }

    pub fn is_sound(self) -> bool {
        matches!(self, Source::Tab | Source::Screen)
    }
}

pub struct Listening {
    pub audio: Option<Receiver<Vec<i16>>>,
    pub screen: bool,
    pub directory: String,
    pub title: Option<String>,
    pub stop: Arc<AtomicBool>,
    pub pause: Arc<AtomicBool>,
    pub points: Arc<Mutex<Vec<(u64, String)>>>,
}

pub enum Heard {
    Clock {
        secs: u64,
        paused: bool,
        level: f32,
    },
    Step {
        label: String,
        steps: Option<(usize, usize)>,
    },
    Transcript(String),
    Warning(String),
}

pub type Commit = Arc<dyn Fn(&std::path::Path, &str) -> Result<()> + Send + Sync>;

#[derive(Default)]
pub struct Recorded {
    pub title: String,
    pub body: String,
    pub source: Option<leo_core::recording::Archive>,
    pub commit: Option<Commit>,
}

pub type Listener =
    Arc<dyn Fn(Listening, &mut dyn FnMut(Heard)) -> Result<Recorded> + Send + Sync>;

#[derive(Debug, Clone, Serialize)]
pub struct RecordView {
    pub id: String,
    pub source: Source,
    pub state: &'static str,
    pub secs: u64,
    pub step: String,
    pub steps: Option<(usize, usize)>,
    pub transcript: String,
    pub warnings: Vec<String>,
    pub points: Vec<(u64, String)>,
    pub levels: Vec<f32>,
    pub levels_start: u64,
    pub note: Option<String>,
    pub error: Option<String>,
}

impl RecordView {
    fn push_level(&mut self, level: f32) {
        self.levels
            .push((level.clamp(0.0, 1.0) * 1000.0).round() / 1000.0);
        let extra = self.levels.len().saturating_sub(LEVELS_KEPT);
        self.levels.drain(..extra);
        self.levels_start += extra as u64;
    }
}

pub(crate) struct RecordJob {
    view: RecordView,
    audio: Option<Sender<Vec<i16>>>,
    stop: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
    points: Arc<Mutex<Vec<(u64, String)>>>,
    seen: Instant,
}

impl RecordJob {
    pub(crate) fn view(&self) -> RecordView {
        self.view.clone()
    }

    fn live(&self) -> bool {
        matches!(self.view.state, "starting" | "recording" | "paused")
    }

    fn busy(&self) -> bool {
        self.live() || self.view.state == "writing"
    }

    fn end(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if self.live() {
            self.view.state = "writing";
            self.view.step = "Transcribing the recording".into();
        }
    }
}

pub(crate) type Recordings = Arc<Mutex<Option<RecordJob>>>;

fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(serde_json::json!({ "error": message }))).into_response()
}

fn with_job<R>(
    state: &AppState,
    id: &str,
    work: impl FnOnce(&mut RecordJob) -> R,
) -> Result<R, (StatusCode, &'static str)> {
    let mut held = state
        .recording
        .lock()
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "leo hit a problem."))?;
    match held.as_mut() {
        Some(job) if job.view.id == id => {
            job.seen = Instant::now();
            Ok(work(job))
        }
        _ => Err((StatusCode::NOT_FOUND, "That recording is over.")),
    }
}

fn change(recordings: &Recordings, id: &str, work: impl FnOnce(&mut RecordJob)) {
    if let Ok(mut held) = recordings.lock() {
        if let Some(job) = held.as_mut().filter(|j| j.view.id == id) {
            work(job);
        }
    }
}

pub fn decode(bytes: &[u8]) -> Vec<i16> {
    let (pairs, _) = bytes.as_chunks::<2>();
    pairs.iter().map(|pair| i16::from_le_bytes(*pair)).collect()
}

fn keep_tail(text: String) -> String {
    let count = text.chars().count();
    if count <= MOST_TRANSCRIPT_CHARS {
        return text;
    }
    text.chars().skip(count - MOST_TRANSCRIPT_CHARS).collect()
}

pub(crate) async fn overview(
    State(state): State<AppState>,
    Extension(peer): Extension<Peer>,
    headers: HeaderMap,
) -> Response {
    let job = state
        .recording
        .lock()
        .ok()
        .and_then(|held| held.as_ref().map(|j| j.view.clone()));
    Json(serde_json::json!({
        "available": state.listener.is_some(),
        "local": local_request(&headers, peer),
        "job": job,
    }))
    .into_response()
}

#[derive(Deserialize)]
pub(crate) struct StartBody {
    #[serde(default)]
    directory: String,
    #[serde(default)]
    title: Option<String>,
    source: Source,
}

pub(crate) async fn start(
    State(state): State<AppState>,
    Extension(peer): Extension<Peer>,
    headers: HeaderMap,
    Json(body): Json<StartBody>,
) -> Response {
    let Some(listener) = state.listener.clone() else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "leo serve was started without recording.",
        );
    };
    if body.source.on_this_computer() && !local_request(&headers, peer) {
        return error(
            StatusCode::FORBIDDEN,
            "The computer's own microphone and sound can only be recorded from a page open on that computer.",
        );
    }
    let directory = body.directory.trim().trim_matches('/').to_string();
    let checked = state.with_store({
        let directory = directory.clone();
        move |store| valid_directory(store, &directory)
    });
    if checked.await.is_err() {
        return error(StatusCode::BAD_REQUEST, "That folder name cannot be used.");
    }
    let title = body
        .title
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty());
    let id = uuid::Uuid::new_v4().simple().to_string();
    let stop = Arc::new(AtomicBool::new(false));
    let pause = Arc::new(AtomicBool::new(false));
    let points: Arc<Mutex<Vec<(u64, String)>>> = Default::default();
    let (sender, audio) = if body.source.fed_by_browser() {
        let (tx, rx) = std::sync::mpsc::channel();
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };
    {
        let Ok(mut held) = state.recording.lock() else {
            return error(StatusCode::INTERNAL_SERVER_ERROR, "leo hit a problem.");
        };
        if held.as_ref().is_some_and(|j| j.busy()) {
            return error(
                StatusCode::CONFLICT,
                "A recording is already going. Stop it first.",
            );
        }
        *held = Some(RecordJob {
            view: RecordView {
                id: id.clone(),
                source: body.source,
                state: "starting",
                secs: 0,
                step: "Starting".into(),
                steps: None,
                transcript: String::new(),
                warnings: Vec::new(),
                points: Vec::new(),
                levels: Vec::new(),
                levels_start: 0,
                note: None,
                error: None,
            },
            audio: sender,
            stop: Arc::clone(&stop),
            pause: Arc::clone(&pause),
            points: Arc::clone(&points),
            seen: Instant::now(),
        });
    }
    let listening = Listening {
        audio,
        screen: body.source.is_sound(),
        directory: directory.clone(),
        title: title.clone(),
        stop,
        pause,
        points,
    };
    let worker = state.clone();
    let job = id.clone();
    std::thread::spawn(move || run(worker, job, listener, listening, directory, title));
    let watched = Arc::clone(&state.recording);
    let job = id.clone();
    std::thread::spawn(move || watch(watched, job));
    (StatusCode::ACCEPTED, Json(serde_json::json!({ "id": id }))).into_response()
}

fn watch(recordings: Recordings, id: String) {
    loop {
        std::thread::sleep(Duration::from_secs(2));
        let Ok(mut held) = recordings.lock() else {
            return;
        };
        let Some(job) = held.as_mut().filter(|j| j.view.id == id) else {
            return;
        };
        if !job.live() {
            return;
        }
        if job.seen.elapsed() >= FORGOTTEN_AFTER {
            job.view.warnings.push(
                "The page that started this recording went away, so it was stopped and saved."
                    .into(),
            );
            job.end();
            return;
        }
    }
}

fn run(
    state: AppState,
    id: String,
    listener: Listener,
    listening: Listening,
    directory: String,
    title: Option<String>,
) {
    let recordings = Arc::clone(&state.recording);
    let written = listener(listening, &mut |heard| {
        change(&recordings, &id, |job| match heard {
            Heard::Clock {
                secs,
                paused,
                level,
            } => {
                job.view.secs = secs;
                job.view.push_level(level);
                if job.live() {
                    job.view.state = if paused { "paused" } else { "recording" };
                }
            }
            Heard::Step { label, steps } => {
                if !job.live() {
                    job.view.step = label;
                    job.view.steps = steps;
                }
            }
            Heard::Transcript(text) => job.view.transcript = keep_tail(text),
            Heard::Warning(text) => {
                if job.view.warnings.last() != Some(&text) {
                    job.view.warnings.push(text);
                }
            }
        })
    });
    let recorded = match written {
        Ok(found) => found,
        Err(e) => {
            change(&recordings, &id, |job| {
                job.stop.store(true, Ordering::Relaxed);
                job.audio = None;
                job.view.state = "failed";
                job.view.error = Some(e.to_string());
            });
            return;
        }
    };
    change(&recordings, &id, |job| {
        job.view.state = "writing";
        job.view.step = "Saving the note".into();
    });
    let title = title.unwrap_or(recorded.title);
    let body = recorded.body;
    let source = recorded.source;
    let commit = recorded.commit;
    let made = store_now(&state, move |store| {
        if !directory.is_empty() && !store.dir_exists(&directory) {
            store.create_dir(&directory);
        }
        let stable_id = source.as_ref().map(|s| s.id.clone());
        if let Some(id) = &stable_id {
            if store.find_note(id).is_some() { return Ok((id.clone(), store.notes_dir.clone())); }
        }
        let mut note = store
            .create_note(title, body.trim().to_string(), vec![], &directory)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .id
            .clone();
        if let Some(id) = stable_id {
            store.find_note_mut(&note).expect("created note").id = id.clone();
            note = id;
        }
        if let Some(source) = &source {
            leo_core::recording::save(&store.notes_dir, &note, source).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        }
        save(store)?;
        Ok((note, store.notes_dir.clone()))
    });
    change(&recordings, &id, |job| match made {
        Ok((note, notes)) => {
            if let Some(commit) = &commit {
                if let Err(e) = commit(&notes, &note) { job.view.warnings.push(format!("The note is saved; recovery cleanup will retry: {e}")); }
            }
            job.view.state = "done";
            job.view.note = Some(note);
        }
        Err(_) => {
            job.view.state = "failed";
            job.view.error = Some("The note could not be saved.".into());
        }
    });
}

pub(crate) async fn status(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match with_job(&state, &id, |job| job.view.clone()) {
        Ok(view) => Json(view).into_response(),
        Err((status, message)) => error(status, message),
    }
}

pub(crate) async fn audio(
    State(state): State<AppState>,
    Path(id): Path<String>,
    bytes: Bytes,
) -> Response {
    let samples = decode(&bytes);
    match with_job(&state, &id, |job| match (&job.audio, job.live()) {
        (Some(tx), true) => {
            if !samples.is_empty() {
                let _ = tx.send(samples);
            }
            true
        }
        _ => false,
    }) {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => error(StatusCode::CONFLICT, "This recording is not listening."),
        Err((status, message)) => error(status, message),
    }
}

#[derive(Deserialize)]
pub(crate) struct PauseBody {
    paused: bool,
}

pub(crate) async fn pause(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PauseBody>,
) -> Response {
    match with_job(&state, &id, |job| {
        if job.live() {
            job.pause.store(body.paused, Ordering::Relaxed);
            job.view.state = if body.paused { "paused" } else { "recording" };
        }
        job.view.clone()
    }) {
        Ok(view) => Json(view).into_response(),
        Err((status, message)) => error(status, message),
    }
}

#[derive(Deserialize)]
pub(crate) struct PointBody {
    text: String,
}

pub(crate) async fn point(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PointBody>,
) -> Response {
    let text = body.text.trim().to_string();
    if text.is_empty() {
        return error(StatusCode::BAD_REQUEST, "Write the point first.");
    }
    match with_job(&state, &id, |job| {
        if !job.live() || job.view.points.len() >= MOST_POINTS {
            return None;
        }
        let at = job.view.secs;
        if let Ok(mut points) = job.points.lock() {
            points.push((at, text.clone()));
        }
        job.view.points.push((at, text));
        Some(job.view.clone())
    }) {
        Ok(Some(view)) => Json(view).into_response(),
        Ok(None) => error(StatusCode::CONFLICT, "This recording is not taking points."),
        Err((status, message)) => error(status, message),
    }
}

pub(crate) async fn stop(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match with_job(&state, &id, |job| {
        job.end();
        job.view.clone()
    }) {
        Ok(view) => Json(view).into_response(),
        Err((status, message)) => error(status, message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> RecordView {
        RecordView {
            id: "r".into(),
            source: Source::Browser,
            state: "recording",
            secs: 0,
            step: String::new(),
            steps: None,
            transcript: String::new(),
            warnings: Vec::new(),
            points: Vec::new(),
            levels: Vec::new(),
            levels_start: 0,
            note: None,
            error: None,
        }
    }

    #[test]
    fn levels_keep_the_last_few_seconds_and_count_what_scrolled_away() {
        let mut v = view();
        for i in 0..(LEVELS_KEPT + 5) {
            v.push_level(i as f32 / 1000.0);
        }
        assert_eq!(v.levels.len(), LEVELS_KEPT);
        assert_eq!(v.levels_start, 5);
        assert_eq!(v.levels[0], 0.005, "the first kept level is the sixth sent");
        v.push_level(7.0);
        assert_eq!(*v.levels.last().unwrap(), 1.0, "levels are clamped");
        assert_eq!(v.levels_start, 6);
    }
}
