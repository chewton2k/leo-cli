use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use axum::{
    body::Bytes,
    extract::{Path, Query, State},
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
const WAIT_FOR_WRITES: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Browser,
    Tab,
    Call,
    Microphone,
    Screen,
}

impl Source {
    pub fn fed_by_browser(self) -> bool {
        matches!(self, Source::Browser | Source::Tab | Source::Call)
    }

    pub fn on_this_computer(self) -> bool {
        matches!(self, Source::Microphone | Source::Screen)
    }

    pub fn is_sound(self) -> bool {
        matches!(self, Source::Tab | Source::Screen)
    }
}

pub struct Listening {
    pub id: String,
    pub profile: leo_core::recording::Profile,
    pub audio: Option<Receiver<Vec<i16>>>,
    pub other_audio: Option<Receiver<Vec<i16>>>,
    pub call: bool,
    pub screen: bool,
    pub directory: String,
    pub title: Option<String>,
    pub stop: Arc<AtomicBool>,
    pub finish_now: Arc<AtomicBool>,
    pub pause: Arc<AtomicBool>,
    pub points: Arc<Mutex<Vec<(u64, String)>>>,
    pub wants: Arc<Mutex<Option<String>>>,
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
pub type Regenerator = Arc<
    dyn Fn(
            Vec<leo_core::recording::Archive>,
            leo_core::recording::Profile,
        ) -> Result<(String, String)>
        + Send
        + Sync,
>;

#[derive(Default)]
pub struct Recorded {
    pub title: String,
    pub body: String,
    pub source: Option<leo_core::recording::Archive>,
    pub commit: Option<Commit>,
}

pub type Listener = Arc<dyn Fn(Listening, &mut dyn FnMut(Heard)) -> Result<Recorded> + Send + Sync>;

#[derive(Debug, Clone, Serialize)]
pub struct RecordView {
    pub next_seq: u64,
    pub id: String,
    pub source: Source,
    pub state: &'static str,
    pub secs: u64,
    pub step: String,
    pub steps: Option<(usize, usize)>,
    pub transcript: String,
    pub warnings: Vec<String>,
    pub points: Vec<(u64, String)>,
    pub wants: String,
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
    journal: Option<(std::path::PathBuf, crate::record_journal::Journal)>,
    next_seq: u64,
    writing: Arc<AtomicBool>,
    audio: Option<Sender<Vec<i16>>>,
    other_audio: Option<Sender<Vec<i16>>>,
    stop: Arc<AtomicBool>,
    finish_now: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
    points: Arc<Mutex<Vec<(u64, String)>>>,
    wants: Arc<Mutex<Option<String>>>,
    seen: Instant,
}

struct Writing(Arc<AtomicBool>);
impl Writing {
    fn take(job: &RecordJob) -> anyhow::Result<Self> {
        if job.writing.swap(true, Ordering::AcqRel) {
            anyhow::bail!("Audio or a point is being saved. Try again in a moment.");
        }
        Ok(Self(job.writing.clone()))
    }
}
impl Drop for Writing {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl RecordJob {
    pub(crate) fn view(&self) -> RecordView {
        self.view.clone()
    }

    #[cfg(test)]
    pub(crate) fn writing(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.writing)
    }

    fn live(&self) -> bool {
        matches!(self.view.state, "starting" | "recording" | "paused")
    }

    pub(crate) fn busy(&self) -> bool {
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

async fn writes_done(state: &AppState, id: &str) {
    let until = Instant::now() + WAIT_FOR_WRITES;
    while Instant::now() < until
        && with_job(state, id, |job| job.writing.load(Ordering::Acquire)).unwrap_or(false)
    {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
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
    let pending = match state.with_store(|store| Ok(store.notes_dir.clone())).await {
        Ok(notes) => tokio::task::spawn_blocking(move || crate::record_journal::pending(&notes))
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    let job = state
        .recording
        .lock()
        .ok()
        .and_then(|held| held.as_ref().map(|j| j.view.clone()));
    Json(serde_json::json!({
        "available": state.listener.is_some(),
        "local": local_request(&headers, peer),
        "job": job,
        "pending": pending,
    }))
    .into_response()
}

#[derive(Serialize, Deserialize)]
pub(crate) struct StartBody {
    #[serde(default)]
    directory: String,
    #[serde(default)]
    title: Option<String>,
    source: Source,
    #[serde(default)]
    profile: Option<leo_core::recording::Profile>,
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
    let checked = state
        .with_store({
            let directory = directory.clone();
            move |store| {
                valid_directory(store, &directory)?;
                Ok(store.notes_dir.clone())
            }
        })
        .await;
    let notes = match checked {
        Ok(notes) => notes,
        Err(code) => return error(code, "Check the folder before recording."),
    };
    let profile = match body.profile.clone().unwrap_or_default().checked() {
        Ok(profile) => profile,
        Err(e) => return error(StatusCode::BAD_REQUEST, &e.to_string()),
    };
    let title = body
        .title
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty());
    let id = uuid::Uuid::new_v4().simple().to_string();
    let stop = Arc::new(AtomicBool::new(false));
    let pause = Arc::new(AtomicBool::new(false));
    let finish_now = Arc::new(AtomicBool::new(false));
    let points: Arc<Mutex<Vec<(u64, String)>>> = Default::default();
    let wants: Arc<Mutex<Option<String>>> = Default::default();
    let (sender, audio) = if body.source.fed_by_browser() {
        let (tx, rx) = std::sync::mpsc::channel();
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };
    let (other_sender, other_audio) = if body.source == Source::Call {
        let (tx, rx) = std::sync::mpsc::channel();
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };
    let journal = if body.source.fed_by_browser() {
        let meta = crate::record_journal::Journal {
            id: id.clone(),
            directory: directory.clone(),
            title: title.clone(),
            source: body.source,
            profile: profile.clone(),
            points: Vec::new(),
        };
        if crate::record_journal::save(&notes, &meta).is_err() {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not save the audio journal. Check free disk space.",
            );
        }
        Some((notes, meta))
    } else {
        None
    };
    {
        let Ok(mut held) = state.recording.lock() else {
            return error(StatusCode::INTERNAL_SERVER_ERROR, "leo hit a problem.");
        };
        if held
            .as_ref()
            .is_some_and(|j| j.busy() || j.writing.load(Ordering::Acquire))
        {
            drop(held);
            if let Some((notes, meta)) = &journal {
                if let Ok(dir) = crate::record_journal::folder(notes, &meta.id) {
                    let _ = std::fs::remove_dir_all(dir);
                }
            }
            return error(
                StatusCode::CONFLICT,
                "A recording is already going. Stop it first.",
            );
        }
        *held = Some(RecordJob {
            journal,
            next_seq: 0,
            writing: Default::default(),
            view: RecordView {
                next_seq: 0,
                id: id.clone(),
                source: body.source,
                state: "starting",
                secs: 0,
                step: "Starting".into(),
                steps: None,
                transcript: String::new(),
                warnings: Vec::new(),
                points: Vec::new(),
                wants: profile.wants.clone(),
                levels: Vec::new(),
                levels_start: 0,
                note: None,
                error: None,
            },
            audio: sender,
            other_audio: other_sender,
            stop: Arc::clone(&stop),
            finish_now: finish_now.clone(),
            pause: Arc::clone(&pause),
            points: Arc::clone(&points),
            wants: Arc::clone(&wants),
            seen: Instant::now(),
        });
    }
    let listening = Listening {
        id: id.clone(),
        profile,
        audio,
        other_audio,
        call: body.source == Source::Call,
        screen: body.source.is_sound(),
        directory: directory.clone(),
        title: title.clone(),
        stop,
        finish_now,
        pause,
        points,
        wants,
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
            if job.view.source.fed_by_browser() {
                let warning="The recording page is offline. Its unsent audio is kept on that device; reopen it to reconnect and save.";
                if !job.view.warnings.iter().any(|w| w == warning) {
                    job.view.warnings.push(warning.into());
                }
                job.seen = Instant::now();
                continue;
            }
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
    let written = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        listener(listening, &mut |heard| {
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
        })
    }))
    .unwrap_or_else(|_| {
        Err(anyhow::anyhow!(
            "Recording worker stopped unexpectedly; its audio is available for recovery."
        ))
    });
    let recorded = match written {
        Ok(found) => found,
        Err(e) => {
            change(&recordings, &id, |job| {
                job.stop.store(true, Ordering::Relaxed);
                job.audio = None;
                job.other_audio = None;
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
    if let Some(source) = &source {
        let archived = store_now(&state, |store| Ok(store.notes_dir.clone())).and_then(|notes| {
            leo_core::recording::save(&notes, &source.id, source)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
        });
        if archived.is_err() {
            change(&recordings, &id, |job| {
                job.view.state = "failed";
                job.view.error = Some("The recording sources could not be saved. Check free disk space and recover this recording.".into());
            });
            return;
        }
    }
    let made = store_now(&state, move |store| {
        if !directory.is_empty() && !store.dir_exists(&directory) {
            store.create_dir(&directory);
        }
        let stable_id = source.as_ref().map(|s| s.id.clone());
        if let Some(id) = &stable_id {
            if store.find_note(id).is_some() {
                return Ok((id.clone(), store.notes_dir.clone()));
            }
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
        save(store)?;
        Ok((note, store.notes_dir.clone()))
    });
    let mut warning = None;
    if let Ok((note, notes)) = &made {
        if let Some(commit) = &commit {
            if let Err(e) = commit(notes, note) {
                warning = Some(format!(
                    "The note is saved; recovery cleanup will retry: {e}"
                ));
            }
        }
        let journal = recordings.lock().ok().and_then(|held| {
            held.as_ref()
                .filter(|j| j.view.id == id && !j.writing.load(Ordering::Acquire))
                .and_then(|j| j.journal.clone())
        });
        if warning.is_none() {
            if let Some((notes, meta)) = journal {
                if let Ok(dir) = crate::record_journal::folder(&notes, &meta.id) {
                    let _ = std::fs::remove_dir_all(dir);
                }
            }
        }
    }
    change(&recordings, &id, |job| match made {
        Ok((note, _)) => {
            if let Some(warning) = warning {
                job.view.warnings.push(warning);
            }
            job.view.state = "done";
            job.view.note = Some(note);
        }
        Err(_) => {
            job.view.state = "failed";
            job.view.error = Some(
                "The note could not be saved. Recover the recording after checking disk space."
                    .into(),
            );
        }
    });
}

pub(crate) async fn status(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match with_job(&state, &id, |job| job.view.clone()) {
        Ok(view) => Json(view).into_response(),
        Err((status, message)) => error(status, message),
    }
}

fn audio_tracks(bytes: &[u8], call: bool) -> anyhow::Result<(Vec<i16>, Vec<i16>)> {
    if !bytes.len().is_multiple_of(if call { 4 } else { 2 }) {
        anyhow::bail!("Audio must contain whole PCM frames.");
    }
    let samples = decode(bytes);
    if call {
        Ok(samples
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| (pair[0], pair[1]))
            .unzip())
    } else {
        Ok((samples, Vec::new()))
    }
}
fn send_audio(
    tx: &Sender<Vec<i16>>,
    other: Option<&Sender<Vec<i16>>>,
    bytes: &[u8],
) -> anyhow::Result<()> {
    let (samples, system) = audio_tracks(bytes, other.is_some())?;
    tx.send(samples)?;
    if let Some(other) = other {
        other.send(system)?;
    }
    Ok(())
}

#[derive(Default, Deserialize)]
pub(crate) struct AudioQuery {
    seq: Option<u64>,
}

pub(crate) async fn audio(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<AudioQuery>,
    bytes: Bytes,
) -> Response {
    if !bytes.len().is_multiple_of(2) {
        return error(
            StatusCode::BAD_REQUEST,
            "Audio must contain whole PCM samples.",
        );
    }
    if !state
        .recording
        .lock()
        .is_ok_and(|held| held.as_ref().is_some_and(|j| j.view.id == id))
    {
        if let Some(seq) = query.seq {
            let notes = match state.with_store(|store| Ok(store.notes_dir.clone())).await {
                Ok(notes) => notes,
                Err(code) => return error(code, "The recovery folder could not be opened."),
            };
            let result = tokio::task::spawn_blocking(move || {
                crate::record_journal::append(&notes, &id, seq, &bytes)
                    .map_err(|_| StatusCode::CONFLICT)?;
                let next = crate::record_journal::chunks(&notes, &id)
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                    .len();
                Ok(serde_json::json!({"next_seq":next}))
            })
            .await
            .unwrap_or(Err(StatusCode::INTERNAL_SERVER_ERROR));
            return match result {
                Ok(v) => Json(v).into_response(),
                Err(code) => error(
                    code,
                    "This audio cannot be appended. Check recovery recordings.",
                ),
            };
        }
    }
    let upload = match with_job(&state, &id, |job| {
        let lease = Writing::take(job)?;
        if !job.view.source.fed_by_browser() {
            anyhow::bail!("This recording uses audio from Leo’s computer.");
        }
        if job.view.source == Source::Call && !bytes.len().is_multiple_of(4) {
            anyhow::bail!("Call audio must contain whole stereo frames.");
        }
        let seq = query.seq.unwrap_or(job.next_seq);
        if !job.live() && job.view.state != "failed" && seq >= job.next_seq {
            anyhow::bail!("This recording has stopped accepting audio.");
        }
        Ok((
            lease,
            job.journal.clone(),
            seq,
            job.next_seq,
            job.audio.clone(),
            job.other_audio.clone(),
        ))
    }) {
        Ok(Ok(upload)) => upload,
        Ok(Err(e)) => return error(StatusCode::CONFLICT, &e.to_string()),
        Err((code, message)) => return error(code, message),
    };
    let (lease, journal, seq, next, tx, other) = upload;
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<u64> {
        if let Some((notes, meta)) = journal {
            if !crate::record_journal::append(&notes, &meta.id, seq, &bytes)? {
                return Ok(next.max(seq + 1));
            }
        } else if seq != next {
            anyhow::bail!("Audio is out of order; retry the pending chunk.");
        }
        if let Some(tx) = tx {
            send_audio(&tx, other.as_ref(), &bytes)?;
        }
        Ok(next.max(seq + 1))
    })
    .await;
    let response = match result {
        Ok(Ok(next)) => {
            let _ = with_job(&state, &id, |job| {
                job.next_seq = next;
                job.view.next_seq = next;
            });
            if query.seq.is_some() {
                Json(serde_json::json!({"next_seq":next})).into_response()
            } else {
                StatusCode::NO_CONTENT.into_response()
            }
        }
        Ok(Err(e)) => error(StatusCode::CONFLICT, &e.to_string()),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Audio could not be saved. Retry the pending chunk.",
        ),
    };
    drop(lease);
    response
}

const REPLAY_CHUNKS: usize = 4;

pub(crate) async fn recover(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let Some(listener) = state.listener.clone() else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "Recording is unavailable.");
    };
    let notes = match state.with_store(|store| Ok(store.notes_dir.clone())).await {
        Ok(notes) => notes,
        Err(code) => return error(code, "The recovery folder could not be read."),
    };
    let recovery = tokio::task::spawn_blocking({
        let id = id.clone();
        move || {
            let meta = crate::record_journal::pending(&notes)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                .into_iter()
                .find(|m| m.id == id)
                .ok_or(StatusCode::NOT_FOUND)?;
            let chunks = crate::record_journal::chunks(&notes, &id)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            Ok((notes, meta, chunks))
        }
    })
    .await
    .unwrap_or(Err(StatusCode::INTERNAL_SERVER_ERROR));
    let (notes, meta, chunks) = match recovery {
        Ok(v) => v,
        Err(code) => return error(code, "The recovery audio could not be read."),
    };
    let (tx, rx) = std::sync::mpsc::sync_channel(REPLAY_CHUNKS);
    let (other_tx, other_audio) = if meta.source == Source::Call {
        let (tx, rx) = std::sync::mpsc::sync_channel(REPLAY_CHUNKS);
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };
    let stop = Arc::new(AtomicBool::new(false));
    let pause = Arc::new(AtomicBool::new(false));
    let finish_now = Arc::new(AtomicBool::new(false));
    let points = Arc::new(Mutex::new(meta.points.clone()));
    let wants: Arc<Mutex<Option<String>>> = Default::default();
    {
        let Ok(mut held) = state.recording.lock() else {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        };
        if held
            .as_ref()
            .is_some_and(|j| j.busy() || j.writing.load(Ordering::Acquire))
        {
            return error(StatusCode::CONFLICT, "Stop the current recording first.");
        }
        *held = Some(RecordJob {
            view: RecordView {
                next_seq: chunks.len() as u64,
                id: id.clone(),
                source: meta.source,
                state: "writing",
                secs: 0,
                step: "Recovering saved audio".into(),
                steps: None,
                transcript: String::new(),
                warnings: vec!["Recovered audio from the last interrupted recording.".into()],
                points: meta.points.clone(),
                wants: meta.profile.wants.clone(),
                levels: Vec::new(),
                levels_start: 0,
                note: None,
                error: None,
            },
            next_seq: chunks.len() as u64,
            writing: Default::default(),
            journal: Some((notes, meta.clone())),
            audio: None,
            other_audio: None,
            stop: stop.clone(),
            finish_now: finish_now.clone(),
            pause: pause.clone(),
            points: points.clone(),
            wants: wants.clone(),
            seen: Instant::now(),
        });
    }
    let listening = Listening {
        id: id.clone(),
        profile: meta.profile,
        audio: Some(rx),
        other_audio,
        call: meta.source == Source::Call,
        screen: meta.source.is_sound(),
        directory: meta.directory.clone(),
        title: meta.title.clone(),
        stop: stop.clone(),
        finish_now,
        pause,
        points,
        wants,
    };
    let worker = state.clone();
    let job = id.clone();
    std::thread::spawn(move || run(worker, job, listener, listening, meta.directory, meta.title));
    let replay_id = id.clone();
    std::thread::spawn(move || {
        let id = replay_id;
        for path in chunks {
            match std::fs::read(&path) {
                Ok(bytes) => {
                    let sent = (|| -> anyhow::Result<()> {
                        let (samples, system) = audio_tracks(&bytes, other_tx.is_some())?;
                        tx.send(samples)?;
                        if let Some(other) = &other_tx {
                            other.send(system)?;
                        }
                        Ok(())
                    })();
                    if sent.is_err() {
                        break;
                    }
                }
                Err(_) => {
                    change(&state.recording, &id, |j| {
                        j.view
                            .warnings
                            .push("A saved audio chunk could not be read.".into())
                    });
                    break;
                }
            }
        }
        stop.store(true, Ordering::Relaxed);
    });
    (StatusCode::ACCEPTED, Json(serde_json::json!({"id":id}))).into_response()
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
    if text.len() > 8000 {
        return error(StatusCode::BAD_REQUEST, "Keep a point under 8,000 bytes.");
    }
    append_point(&state, &id, text, None, None).await
}

async fn append_point(
    state: &AppState,
    id: &str,
    text: String,
    at: Option<u64>,
    warning: Option<String>,
) -> Response {
    writes_done(state, id).await;
    let input = match with_job(state, id, |job| -> anyhow::Result<_> {
        if !job.live() || job.view.points.len() >= MOST_POINTS {
            anyhow::bail!("This recording is not taking points.");
        }
        let lease = Writing::take(job)?;
        let at = at.unwrap_or(job.view.secs).min(job.view.secs);
        let mut journal = job.journal.clone();
        if let Some((_, meta)) = &mut journal {
            meta.points.push((at, text.clone()));
        }
        Ok((lease, at, journal))
    }) {
        Ok(Ok(input)) => input,
        Ok(Err(e)) => return error(StatusCode::CONFLICT, &e.to_string()),
        Err((code, message)) => return error(code, message),
    };
    let (lease, at, journal) = input;
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        if let Some((notes, meta)) = &journal {
            crate::record_journal::save(notes, meta)?;
        }
        Ok(journal)
    })
    .await;
    let journal = match result {
        Ok(Ok(journal)) => journal,
        _ => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "The point could not be saved. Check free disk space and retry.",
            )
        }
    };
    let result = with_job(state, id, |job| {
        job.journal = journal;
        if let Ok(mut points) = job.points.lock() {
            points.push((at, text.clone()));
        }
        job.view.points.push((at, text));
        if let Some(warning) = warning {
            job.view.warnings.push(warning);
        }
        job.view.clone()
    });
    drop(lease);
    match result {
        Ok(view) => Json(view).into_response(),
        Err((code, message)) => error(code, message),
    }
}

#[derive(Deserialize)]
pub(crate) struct WantsBody {
    text: String,
}

pub(crate) async fn wants(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<WantsBody>,
) -> Response {
    let text = body.text.trim().to_string();
    if text.chars().count() > leo_core::recording::MOST_WANTS_CHARS {
        return error(
            StatusCode::BAD_REQUEST,
            "Keep what you want from the notes under 4,000 characters.",
        );
    }
    writes_done(&state, &id).await;
    let input = match with_job(&state, &id, |job| -> anyhow::Result<_> {
        if !job.live() {
            anyhow::bail!(
                "The notes are already being written; change this before you stop next time."
            );
        }
        let lease = Writing::take(job)?;
        let mut journal = job.journal.clone();
        if let Some((_, meta)) = &mut journal {
            meta.profile.wants = text.clone();
        }
        Ok((lease, journal))
    }) {
        Ok(Ok(input)) => input,
        Ok(Err(e)) => return error(StatusCode::CONFLICT, &e.to_string()),
        Err((code, message)) => return error(code, message),
    };
    let (lease, journal) = input;
    let saved = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        if let Some((notes, meta)) = &journal {
            crate::record_journal::save(notes, meta)?;
        }
        Ok(journal)
    })
    .await;
    let journal =
        match saved {
            Ok(Ok(journal)) => journal,
            _ => return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "What you want from the notes could not be saved. Check free disk space and retry.",
            ),
        };
    let result = with_job(&state, &id, |job| {
        job.journal = journal;
        if let Ok(mut wants) = job.wants.lock() {
            *wants = Some(text.clone());
        }
        job.view.wants = text;
        job.view.clone()
    });
    drop(lease);
    match result {
        Ok(view) => Json(view).into_response(),
        Err((code, message)) => error(code, message),
    }
}

pub(crate) async fn stop(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    writes_done(&state, &id).await;
    match with_job(&state, &id, |job| {
        if job.writing.load(Ordering::Acquire) {
            return None;
        }
        job.end();
        Some(job.view.clone())
    }) {
        Ok(Some(view)) => Json(view).into_response(),
        Ok(None) => error(
            StatusCode::CONFLICT,
            "Audio or a point is still being saved. Try Stop again in a moment.",
        ),
        Err((status, message)) => error(status, message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> RecordView {
        RecordView {
            next_seq: 0,
            id: "r".into(),
            source: Source::Browser,
            state: "recording",
            secs: 0,
            step: String::new(),
            steps: None,
            transcript: String::new(),
            warnings: Vec::new(),
            points: Vec::new(),
            wants: String::new(),
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

#[derive(Deserialize)]
pub(crate) struct Snapshot {
    data: String,
    at: u64,
}
pub(crate) async fn snapshot(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Snapshot>,
) -> Response {
    use base64::Engine;
    let directory = match with_job(&state, &id, |job| {
        job.live().then(|| {
            job.journal
                .as_ref()
                .map(|(_, m)| m.directory.clone())
                .unwrap_or_default()
        })
    }) {
        Ok(Some(dir)) => dir,
        _ => {
            return error(
                StatusCode::CONFLICT,
                "Start a recording before capturing a slide.",
            )
        }
    };
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(&body.data) else {
        return error(StatusCode::BAD_REQUEST, "The picture did not arrive whole.");
    };
    let saved = state
        .with_store({
            let bytes = bytes.clone();
            move |store| {
                leo_core::attachments::save(&store.notes_dir, "slide.png", &bytes)
                    .map_err(|_| StatusCode::BAD_REQUEST)
            }
        })
        .await;
    let path = match saved {
        Ok(p) => p,
        Err(code) => return error(code, "The picture could not be saved."),
    };
    let mut point = format!("Slide at {}\n![Captured slide]({path})", body.at);
    let mut warning = None;
    if let Some(reader) = state.reader.clone() {
        let read = tokio::task::spawn_blocking(move || {
            reader(
                crate::UploadFile {
                    name: "slide.png".into(),
                    mime: "image/png".into(),
                    bytes,
                },
                &mut |_| {},
            )
        })
        .await;
        match read {
            Ok(Ok(text)) => {
                point.push('\n');
                point.extend(text.chars().take(6500));
            }
            _ => warning = Some("The slide image is kept; its text could not be read.".to_string()),
        }
    }
    let _ = directory;
    append_point(&state, &id, point, Some(body.at), warning).await
}

pub(crate) async fn finish_available(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    match with_job(&state, &id, |job| {
        if job.view.state != "writing" {
            return false;
        }
        job.finish_now.store(true, Ordering::Relaxed);
        job.view.step = "Finishing with the available transcript".into();
        true
    }) {
        Ok(true) => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({"finishing":true})),
        )
            .into_response(),
        _ => error(
            StatusCode::CONFLICT,
            "Stop recording before finishing with the available transcript.",
        ),
    }
}
