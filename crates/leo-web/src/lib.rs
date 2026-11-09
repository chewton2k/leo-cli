mod board_tools;
mod boards;
mod browse;
pub mod calc;
mod calendar;
mod store_refresh;
pub const CALENDAR_BYTES: u64 = calendar::MOST_BYTES;
pub mod app_settings;
pub mod captions;
pub mod chat;
pub mod chat_files;
pub mod chats;
mod combine;
pub mod db;
pub mod dictation;
pub mod export;
pub mod graph;
mod ics;
mod keeping;
pub mod margin;
pub mod ollama;
mod originals;
mod progress;
mod read_cache;
pub mod record;
pub mod reply;
pub mod review;
mod routes;
mod routing;
pub mod search;
pub mod sessions;
mod shutdown;
pub mod speech;
pub mod steer;
mod tasks;
#[cfg(test)]
mod tests;
mod token;
pub mod tools;
pub mod usage;
pub mod vectors;
mod warm;
mod work;

use std::future::IntoFuture;
#[cfg(test)]
use std::sync::MutexGuard;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use axum::{
    http::StatusCode,
    routing::{get, post},
    Router,
};
use leo_core::store::Store;

use crate::routes::activity::{activity, page_visible};
use crate::routes::assets::{
    app_js, chat_js, doc_js, editing_js, favicon, index, markdown_js, recording_js, saving_js,
};
use crate::routes::auth::{auth_middleware, note_peer, same_host, security_headers};
use crate::routes::downloads::{export_zip, originals_zip};
use crate::routes::felix::{
    add_chat_file, apply_suggestion, change_answer_access, chat_reply, delete_chat, get_chat,
    get_review, list_chat_files, list_chats, mark_reviewed, put_chat, remove_chat_file,
    steer_answer,
};
use crate::routes::housekeeping::{get_keep, set_keep};
use crate::routes::notes::{
    create_dir, create_note, delete_note, get_note, list_dirs, list_folders, list_notes, move_dir,
    move_note, rename_dir, search_notes, toggle_checkbox, update_note,
};
use crate::routes::pictures::{add_picture, get_picture, PICTURE_BYTES};
use crate::routes::settings::{change_setting, get_settings, test_setting};
use crate::routes::trash::{
    delete_from_trash, list_trash, move_to_trash, restore_many, restore_note,
};
use crate::routes::uploads::{
    get_original, import_status, list_originals, start_import, ImportJob, IMPORT_BYTES,
};

pub use chat::{Conversation, Converser, Exchange, Instructions, Reply, Spent, Streamer, ToolSpec};
pub use graph::Writer;
pub use vectors::Meaning;

pub trait SettingsApi: Send + Sync {
    fn describe(&self, notes_dir: &std::path::Path) -> serde_json::Value;
    fn refresh_usage(&self, _claude: bool) -> Option<String> {
        None
    }
    fn apply(&self, change: &serde_json::Value) -> Result<String>;
    fn test(&self, task: &str) -> Result<String>;
}

#[derive(Debug, Clone)]
pub struct UploadFile {
    pub name: String,
    pub mime: String,
    pub bytes: Vec<u8>,
}

pub type Room = Arc<dyn Fn() -> usize + Send + Sync>;

pub struct Fetched {
    pub body: Option<Vec<u8>>,
    pub etag: Option<String>,
    pub modified: Option<String>,
}

pub trait CalendarAccess: Send + Sync {
    fn fetch(&self, url: &str) -> Result<Vec<u8>>;
    fn fetch_since(
        &self,
        url: &str,
        _etag: Option<&str>,
        _modified: Option<&str>,
    ) -> Result<Fetched> {
        Ok(Fetched {
            body: Some(self.fetch(url)?),
            etag: None,
            modified: None,
        })
    }
    fn get(&self, account: &str) -> Result<Option<String>>;
    fn set(&self, account: &str, value: &str) -> Result<()>;
    fn delete(&self, account: &str) -> Result<()>;
}

pub type Reader = Arc<dyn Fn(UploadFile, &mut dyn FnMut(&str)) -> Result<String> + Send + Sync>;

#[derive(Debug, Clone, PartialEq)]
pub struct Figure {
    pub place: String,
    pub bytes: Vec<u8>,
    pub photo: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Made {
    pub title: String,
    pub body: String,
    pub figures: Vec<Figure>,
}

pub type Importer = Arc<
    dyn Fn(Vec<UploadFile>, &str, &mut dyn FnMut(&str, usize, usize)) -> Result<Made> + Send + Sync,
>;

pub type Lighter = Arc<dyn Fn() -> bool + Send + Sync>;

#[derive(Clone, Default)]
pub struct Powers {
    pub writer: Option<Writer>,
    pub quick: Option<Writer>,
    pub toolless: Option<Writer>,
    pub housekeeping: Option<Writer>,
    pub usage: Arc<usage::Usage>,
    pub chat: Option<Streamer>,
    pub light_chat: Option<Streamer>,
    pub lighter: Option<Lighter>,
    pub toolless_chat: Option<Streamer>,
    pub settings: Option<Arc<dyn SettingsApi>>,
    pub speech: Option<Arc<dyn speech::SpeechApi>>,
    pub ollama: Option<Arc<dyn ollama::OllamaApi>>,
    pub dictation: Option<Arc<dyn dictation::DictationApi>>,
    pub app_settings: Option<Arc<dyn app_settings::AppApi>>,
    pub importer: Option<Importer>,
    pub listener: Option<record::Listener>,
    pub regenerator: Option<record::Regenerator>,
    pub calendar_access: Option<Arc<dyn CalendarAccess>>,
    pub housekeeper: Option<Arc<dyn record::Housekeeper>>,
    pub reader: Option<Reader>,
    pub room: Option<Room>,
    pub context: Option<Room>,
    pub converse: Option<chat::Converser>,
    pub seer: Option<captions::Seer>,
    pub meaning: Option<vectors::Meaning>,
}

#[derive(Clone)]
struct AppState {
    visible: Arc<std::sync::atomic::AtomicBool>,
    shown: Arc<tokio::sync::Notify>,
    covered: Arc<std::sync::atomic::AtomicBool>,
    page_seen: Arc<std::sync::atomic::AtomicU64>,
    refresh_requested: Arc<std::sync::atomic::AtomicBool>,
    store: Arc<Mutex<Storage>>,
    gate: Arc<sessions::Gate>,
    graphs: Arc<graph::Graphs>,
    quick: Option<Writer>,
    toolless: Option<Writer>,
    margin: Arc<margin::Reads>,
    housekeeping: Option<Writer>,
    usage: Arc<usage::Usage>,
    progress: Arc<progress::Board>,
    warm: Arc<warm::Slot>,
    work: Arc<work::Work>,
    chat: Option<Streamer>,
    light_chat: Option<Streamer>,
    lighter: Option<Lighter>,
    toolless_chat: Option<Streamer>,
    settings: Option<Arc<dyn SettingsApi>>,
    speech: Option<Arc<dyn speech::SpeechApi>>,
    ollama: Option<Arc<dyn ollama::OllamaApi>>,
    ollama_pulls: Arc<ollama::Pulls>,
    dictation: Option<Arc<dyn dictation::DictationApi>>,
    app_settings: Option<Arc<dyn app_settings::AppApi>>,
    usage_refreshes: Arc<tokio::sync::Semaphore>,
    importer: Option<Importer>,
    imports: Arc<Mutex<std::collections::HashMap<String, ImportJob>>>,
    import_slots: Arc<routes::uploads::ImportSlots>,
    zip_slots: Arc<tokio::sync::Semaphore>,
    combines: Arc<Mutex<std::collections::HashMap<String, routes::combine::CombineJob>>>,
    listener: Option<record::Listener>,
    regenerator: Option<record::Regenerator>,
    calendar_access: Option<Arc<dyn CalendarAccess>>,
    recording: record::Recordings,
    capture_activity: leo_core::activity::Gate,
    discarded_recordings: Arc<Mutex<std::collections::VecDeque<String>>>,
    calendar: Arc<calendar::Calendar>,
    source_writing: Arc<std::sync::atomic::AtomicBool>,
    chats: std::path::PathBuf,
    housekeeper: Option<Arc<dyn record::Housekeeper>>,
    reader: Option<Reader>,
    room: Option<Room>,
    context: Option<Room>,
    converse: Option<chat::Converser>,
    seer: Option<captions::Seer>,
    captions: Arc<captions::Captions>,
    originals: Arc<originals::Originals>,
    activity: Arc<Activity>,
    steering: steer::Steering,
    meaning: Option<vectors::Meaning>,
    vectors: Arc<vectors::Vectors>,
}

struct Activity {
    born: std::time::Instant,
    last: std::sync::atomic::AtomicU64,
}

impl Default for Activity {
    fn default() -> Activity {
        Activity {
            born: std::time::Instant::now(),
            last: std::sync::atomic::AtomicU64::new(0),
        }
    }
}

impl Activity {
    fn touch(&self) {
        let now = self.born.elapsed().as_millis() as u64;
        self.last.store(now, std::sync::atomic::Ordering::Relaxed);
    }

    fn idle(&self) -> std::time::Duration {
        let last = self.last.load(std::sync::atomic::Ordering::Relaxed);
        self.born
            .elapsed()
            .saturating_sub(std::time::Duration::from_millis(last))
    }
}

fn counts_as_use(method: &axum::http::Method, path: &str) -> bool {
    if !path.starts_with("/api/") || path == "/api/visible" {
        return false;
    }
    let polling = path == "/api/activity"
        || matches!(
            path,
            "/api/app"
                | "/api/dictation"
                | "/api/speech"
                | "/api/writing/ollama"
                | "/api/tasks"
                | "/api/processes"
        )
        || path.starts_with("/api/record/")
        || path.starts_with("/api/import/");
    !(method == axum::http::Method::GET && polling)
}

async fn note_activity(
    axum::extract::State(state): axum::extract::State<AppState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    if counts_as_use(request.method(), request.uri().path()) {
        state.activity.touch();
    }
    next.run(request).await
}

const GRAPH_CHECK: std::time::Duration = std::time::Duration::from_secs(30);
const PICTURES_PER_TICK: usize = 4;

async fn keep_graph_current(state: AppState) {
    let mut tick = tokio::time::interval_at(tokio::time::Instant::now() + GRAPH_CHECK, GRAPH_CHECK);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut read_at = None;
    let mut graphed_at = None;
    let mut stuck = None;
    let mut swept_at = None;
    loop {
        tick.tick().await;
        if margin::sweep_due(swept_at) {
            swept_at = Some(std::time::Instant::now());
            let chats = state.chats.clone();
            let _ = state
                .read_store(move |store| Ok(margin::sweep_store(&chats, store)))
                .await;
        }
        if !state.visible.load(std::sync::atomic::Ordering::Acquire)
            || state.covered.load(std::sync::atomic::Ordering::Acquire)
        {
            let _ = tokio::time::timeout(margin::SWEEP_EVERY, state.shown.notified()).await;
            continue;
        }
        if state.capture_activity.busy() {
            continue;
        }
        let Ok(seen) = state.read_store(|store| Ok(store.fingerprint())).await else {
            continue;
        };
        if read_at != Some(seen) {
            if let Some(meaning) = state.meaning.clone().filter(|m| m(&[], false).is_ok()) {
                let vectors = Arc::clone(&state.vectors);
                let listed = Arc::clone(&vectors);
                if let Ok(work) = state
                    .read_store(move |store| {
                        let work = listed.stale(store, vectors::PIECES_PER_TICK);
                        listed.forget_gone(store);
                        Ok(work)
                    })
                    .await
                {
                    if work.is_empty() {
                        read_at = Some(seen);
                    } else {
                        let _ = tokio::task::spawn_blocking(move || {
                            vectors::read_in(&vectors, &meaning, &work)
                        })
                        .await;
                    }
                }
            }
        }
        let idle = state.activity.idle();
        if idle < graph::UPDATE_WHEN_IDLE
            || graphed_at == Some(seen)
            || stuck.is_some_and(|(at, when): (_, std::time::Instant)| {
                at == seen && when.elapsed() < graph::RETRY_AUTO_AFTER
            })
        {
            continue;
        }
        let Ok(sources) = state
            .read_store(|store| Ok(graph::sources(&store.notes)))
            .await
        else {
            continue;
        };
        let graphs = Arc::clone(&state.graphs);
        let Ok((mut settled, mut busy)) = tokio::task::spawn_blocking(move || {
            let started = graphs.update_if_due(sources.clone(), idle);
            (
                !started && graphs.settled(&sources),
                started || graphs.building().is_some(),
            )
        })
        .await
        else {
            continue;
        };
        if let Some(seer) = state.seer.clone() {
            let captions = Arc::clone(&state.captions);
            let listed = Arc::clone(&captions);
            let Ok(paths) = state
                .read_store(move |store| {
                    Ok(captions::uncaptioned(store, &listed, PICTURES_PER_TICK))
                })
                .await
            else {
                continue;
            };
            if !paths.is_empty() {
                settled = false;
                busy |= tokio::task::spawn_blocking(move || {
                    captions::caption_paths(&paths, &captions, &seer)
                })
                .await
                .is_ok_and(|done| done > 0);
            }
        }
        if settled && !state.captions.waiting() {
            graphed_at = Some(seen);
        } else if !busy {
            stuck = Some((seen, std::time::Instant::now()));
        }
    }
}

struct Storage {
    store: Store,
    reload: bool,
    changes: Option<store_refresh::Changes>,
}

impl std::ops::Deref for Storage {
    type Target = Store;
    fn deref(&self) -> &Store {
        &self.store
    }
}

impl std::ops::DerefMut for Storage {
    fn deref_mut(&mut self) -> &mut Store {
        &mut self.store
    }
}

impl AppState {
    async fn with_store<R: Send + 'static>(
        &self,
        work: impl FnOnce(&mut Store) -> Result<R, StatusCode> + Send + 'static,
    ) -> Result<R, StatusCode> {
        self.access_store(true, work).await
    }

    async fn read_store<R: Send + 'static>(
        &self,
        work: impl FnOnce(&Store) -> Result<R, StatusCode> + Send + 'static,
    ) -> Result<R, StatusCode> {
        self.access_store(false, move |store| work(store)).await
    }

    async fn access_store<R: Send + 'static>(
        &self,
        writing: bool,
        work: impl FnOnce(&mut Store) -> Result<R, StatusCode> + Send + 'static,
    ) -> Result<R, StatusCode> {
        let state = self.clone();
        tokio::task::spawn_blocking(move || {
            let mut store = state
                .store
                .lock()
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            let force = state
                .refresh_requested
                .swap(false, std::sync::atomic::Ordering::AcqRel);
            let check =
                writing || force || store.changes.as_mut().is_none_or(|changes| changes.due());
            if store.reload || (check && store.changed_on_disk()) {
                store
                    .refresh()
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                store.reload = false;
            }
            let result = work(&mut store);
            if writing && result.is_err() {
                store.reload = true;
            }
            result
        })
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    }

    #[cfg(test)]
    fn fresh(&self) -> MutexGuard<'_, Storage> {
        let mut store = self.store.lock().unwrap();
        if store.reload || store.changed_on_disk() {
            store.refresh().unwrap();
            store.reload = false;
        }
        store
    }
}

#[derive(Debug, Clone)]
pub struct StartOptions {
    pub port: u16,
    pub token: String,
}

pub(crate) struct Places {
    pub(crate) store: Store,
    pub(crate) sessions: std::path::PathBuf,
}

pub struct Running {
    port: u16,
    token: String,
    state: AppState,
    stop: tokio::sync::oneshot::Sender<()>,
    server: tokio::task::JoinHandle<std::io::Result<()>>,
    background: Background,
}

struct Background {
    graph: tokio::task::JoinHandle<()>,
    idle: tokio::task::JoinHandle<()>,
    expiry: tokio::task::JoinHandle<()>,
}

impl Background {
    fn stop(&self) {
        self.graph.abort();
        self.idle.abort();
        self.expiry.abort();
    }
}

impl Drop for Background {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Running {
    pub fn set_visible(&self, visible: bool) -> bool {
        let changed = self
            .state
            .visible
            .swap(visible, std::sync::atomic::Ordering::AcqRel)
            != visible;
        if changed && visible {
            self.state
                .refresh_requested
                .store(true, std::sync::atomic::Ordering::Release);
            self.state
                .covered
                .store(false, std::sync::atomic::Ordering::Release);
            self.state.shown.notify_one();
        }
        changed
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn sign_in_url(&self) -> String {
        format!("http://127.0.0.1:{}/?token={}", self.port, self.token)
    }

    /// What Felix and leo's other AI work used since leo started, for the menu
    /// bar: "12 AI requests · 340k tokens (210k cached)".
    pub fn usage_line(&self) -> String {
        self.state.usage.line()
    }

    pub async fn stop(self) {
        let Running {
            state,
            stop,
            server,
            background,
            ..
        } = self;
        background.stop();
        let _ = stop.send(());
        shutdown::drain(shutdown::GRACE, server).await;
        drop(background);
        let warm = Arc::clone(&state.warm);
        drop(state);
        let cleanup = tokio::task::spawn_blocking(move || warm.clear());
        let _ = tokio::time::timeout(shutdown::CLEANUP, cleanup).await;
    }
}

pub fn fresh_token() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

pub fn saved_token() -> Result<String> {
    token::load_or_create(&leo_core::paths::config_dir()?.join("serve-token"), false)
}

pub async fn start(options: StartOptions, powers: Powers) -> Result<Running> {
    let places = Places {
        store: Store::load()?,
        sessions: leo_core::paths::config_dir()?.join("app-sessions.json"),
    };
    start_in(places, options, powers).await
}

pub(crate) async fn start_in(
    places: Places,
    options: StartOptions,
    powers: Powers,
) -> Result<Running> {
    let store = places.store;
    let notes = store.notes_dir.clone();
    let vectors = Arc::new(vectors::Vectors::for_notes(&store.notes_dir));
    let graphs = Arc::new(
        graph::Graphs::for_notes(&store.notes_dir, powers.writer)
            .with_room(powers.room.clone())
            .with_vectors(Arc::clone(&vectors)),
    );
    let chats = chats::dir_for(&store.notes_dir);
    powers.usage.save_in(chats.clone());
    let quick = powers.quick;
    let chat = powers.chat;
    let settings = powers.settings;
    let importer = powers.importer;
    let recorder = powers.listener;
    let housekeeper = powers.housekeeper;
    let reader = powers.reader;
    let room = powers.room;
    let converse = powers.converse;
    let seer = powers.seer;
    let meaning = powers.meaning;
    let toolless = powers.toolless;
    let toolless_chat = powers.toolless_chat;
    let light_chat = powers.light_chat;
    let lighter = powers.lighter;
    let captions = Arc::new(captions::Captions::for_notes(&store.notes_dir));
    let (listener, port) = bind(options.port).await?;
    let gate = Arc::new(sessions::Gate::new(
        options.token.clone(),
        sessions::Sessions::load(&places.sessions)?,
    ));
    let state = AppState {
        toolless,
        toolless_chat,
        light_chat,
        lighter,
        margin: Default::default(),
        visible: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        shown: Default::default(),
        covered: Default::default(),
        page_seen: Default::default(),
        refresh_requested: Default::default(),
        store: Arc::new(Mutex::new(Storage {
            changes: Some(store_refresh::Changes::new(&notes)),
            store,
            reload: false,
        })),
        gate: Arc::clone(&gate),
        graphs,
        quick,
        housekeeping: powers.housekeeping,
        usage: powers.usage,
        progress: Arc::new(progress::Board::kept_in(&chats)),
        warm: Default::default(),
        work: Default::default(),
        chat,
        settings,
        speech: powers.speech,
        ollama: powers.ollama,
        ollama_pulls: Default::default(),
        dictation: powers.dictation,
        app_settings: powers.app_settings,
        usage_refreshes: Arc::new(tokio::sync::Semaphore::new(1)),
        importer,
        imports: Default::default(),
        import_slots: Default::default(),
        zip_slots: routes::downloads::zip_slots(),
        combines: Default::default(),
        listener: recorder,
        regenerator: powers.regenerator,
        calendar_access: powers.calendar_access,
        recording: Default::default(),
        capture_activity: leo_core::activity::global().clone(),
        discarded_recordings: Default::default(),
        calendar: Default::default(),
        source_writing: Default::default(),
        chats,
        housekeeper,
        reader,
        room,
        context: powers.context,
        converse,
        seer,
        captions,
        originals: Default::default(),
        activity: Default::default(),
        steering: Default::default(),
        meaning,
        vectors,
    };

    let expiry = tokio::spawn({
        let state = state.clone();
        async move {
            let mut daily = tokio::time::interval(std::time::Duration::from_secs(86_400));
            loop {
                daily.tick().await;
                keeping::expire_now(&state).await;
            }
        }
    });

    let background = Background {
        graph: tokio::spawn(keep_graph_current(state.clone())),
        idle: tokio::spawn(stop_idle_sessions(Arc::clone(&state.warm))),
        expiry,
    };
    let app = router(state.clone()).layer(axum::middleware::from_fn(
        move |request: axum::extract::Request, next: axum::middleware::Next| {
            same_host(port, request, next)
        },
    ));
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            let _ = stopped.await;
        })
        .into_future(),
    );
    Ok(Running {
        port,
        token: options.token,
        state,
        stop,
        server,
        background,
    })
}

async fn bind(wanted: u16) -> Result<(tokio::net::TcpListener, u16)> {
    let mut last = None;
    for port in wanted..wanted.saturating_add(20) {
        if port != 0 {
            if let Err(e) = std::net::TcpListener::bind(("0.0.0.0", port)) {
                if e.kind() == std::io::ErrorKind::AddrInUse {
                    last = Some(e);
                    continue;
                }
            }
        }
        match tokio::net::TcpListener::bind(std::net::SocketAddr::from(([127, 0, 0, 1], port)))
            .await
        {
            Ok(listener) => {
                let port = listener.local_addr()?.port();
                return Ok((listener, port));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => last = Some(e),
            Err(e) => return Err(e.into()),
        }
    }
    Err(anyhow::anyhow!(
        "ports {wanted} to {} are all in use ({})",
        wanted.saturating_add(19),
        last.map(|e| e.to_string()).unwrap_or_default()
    ))
}

fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/api/notes", get(list_notes).post(create_note))
        .route(
            "/api/notes/{id}",
            get(get_note).patch(update_note).delete(delete_note),
        )
        .route("/api/notes/{id}/toggle", post(toggle_checkbox))
        .route("/api/calendar", get(calendar::get).post(calendar::add))
        .route("/api/calendar/refresh", post(calendar::refresh_now))
        .route(
            "/api/calendar/{id}",
            axum::routing::delete(calendar::remove),
        )
        .route(
            "/api/notes/{id}/recording",
            get(routes::sources::sources).put(routes::sources::edit_sources),
        )
        .route(
            "/api/notes/{id}/regenerate",
            post(routes::sources::regenerate),
        )
        .route("/api/notes/{id}/move", post(move_note))
        .route("/api/notes/{id}/combine", post(routes::combine::start))
        .route(
            "/api/notes/{id}/combined",
            post(routes::combine::keep).layer(axum::extract::DefaultBodyLimit::max(
                combine::MOST_CHARS * 8 + 65_536,
            )),
        )
        .route(
            "/api/notes/{id}/uncombine",
            post(routes::combine::uncombine),
        )
        .route(
            "/api/combines/{job}",
            get(routes::combine::job).delete(routes::combine::forget),
        )
        .route("/api/notes/{id}/suggestion", post(apply_suggestion))
        .route("/api/felix/act", post(routes::organize::act_route))
        .route(
            "/api/notes/{id}/insert",
            post(routes::insert::insert_into_note),
        )
        .route(
            "/api/notes/{id}/margin",
            get(routes::margin::note_margin).post(routes::margin::set_note_mode),
        )
        .route(
            "/api/notes/{id}/margin/read",
            post(routes::margin::read)
                .layer(axum::extract::DefaultBodyLimit::max(margin::BODY_BYTES)),
        )
        .route(
            "/api/notes/{id}/margin/analyze",
            post(routes::margin::analyze)
                .layer(axum::extract::DefaultBodyLimit::max(margin::BODY_BYTES)),
        )
        .route(
            "/api/notes/{id}/margin/threads",
            post(routes::margin::ask)
                .layer(axum::extract::DefaultBodyLimit::max(margin::BODY_BYTES)),
        )
        .route(
            "/api/notes/{id}/margin/threads/{thread}",
            post(routes::margin::set_status).delete(routes::margin::forget),
        )
        .route(
            "/api/notes/{id}/margin/threads/{thread}/reply",
            post(routes::margin::reply)
                .layer(axum::extract::DefaultBodyLimit::max(margin::BODY_BYTES)),
        )
        .route(
            "/api/margin",
            get(routes::margin::settings).post(routes::margin::change_settings),
        )
        .route(
            "/api/notes/{id}/related",
            get(routes::related::related_notes),
        )
        .route(
            "/api/notes/{id}/reading",
            get(routes::related::get_reading).put(routes::related::put_reading),
        )
        .route(
            "/api/tasks",
            get(routes::tasks::list_tasks).post(routes::tasks::create_task),
        )
        .route("/api/tasks/promote", post(routes::tasks::promote_task))
        .route("/api/tasks/import", post(routes::tasks::import_tasks))
        .route(
            "/api/tasks/{id}",
            axum::routing::patch(routes::tasks::change_task).delete(routes::tasks::delete_task),
        )
        .route("/api/tasks/{id}/ics", get(routes::tasks::task_ics))
        .route(
            "/api/boards",
            get(routes::boards::list_boards).post(routes::boards::create_board),
        )
        .route("/api/boards/restore", post(routes::boards::restore_board))
        .route(
            "/api/boards/{id}",
            get(routes::boards::get_board)
                .patch(routes::boards::change_board)
                .delete(routes::boards::delete_board),
        )
        .route("/api/boards/{id}/cards", post(routes::boards::create_card))
        .route(
            "/api/cards/{id}",
            axum::routing::patch(routes::boards::change_card).delete(routes::boards::delete_card),
        )
        .route("/api/cards/{id}/move", post(routes::boards::move_card))
        .route("/api/schedule", get(calendar::schedule))
        .route("/api/search", get(search_notes))
        .route("/api/dirs", get(list_dirs).post(create_dir))
        .route("/api/dirs/move", post(move_dir))
        .route("/api/dirs/rename", post(rename_dir))
        .route("/api/folders", get(list_folders))
        .route("/api/trash", get(list_trash))
        .route("/api/trash/{id}/restore", post(restore_note))
        .route("/api/trash/delete", post(delete_from_trash))
        .route("/api/trash/move", post(move_to_trash))
        .route("/api/trash/restore", post(restore_many))
        .route("/app.js", get(app_js))
        .route("/markdown.js", get(markdown_js))
        .route(
            routes::assets::MERMAID_PATH,
            get(routes::assets::mermaid_js),
        )
        .route(
            "/vendor/katex-0.16.11/{*path}",
            get(routes::assets::katex_file),
        )
        .route("/editing.js", get(editing_js))
        .route("/doc.js", get(doc_js))
        .route("/saving.js", get(saving_js))
        .route("/schedule.js", get(routes::assets::schedule_js))
        .route("/margin.js", get(routes::assets::margin_js))
        .route("/theme.js", get(routes::assets::theme_js))
        .route("/chat.js", get(chat_js))
        .route("/pane-resize.js", get(routes::assets::pane_resize_js))
        .route("/api/chat", post(chat_reply))
        .route("/api/quiz/mark", post(routes::felix::mark_quiz))
        .route(
            "/api/chat/compact",
            post(routes::felix::compact_chat)
                .layer(axum::extract::DefaultBodyLimit::max(chats::CHAT_BYTES)),
        )
        .route("/api/chat/{answer}/steer", post(steer_answer))
        .route("/api/chat/{answer}/access", post(change_answer_access))
        .route("/api/chats", get(list_chats))
        .route("/api/review", get(get_review).post(mark_reviewed))
        .route(
            "/api/chats/{id}/files",
            get(list_chat_files)
                .post(add_chat_file)
                .layer(axum::extract::DefaultBodyLimit::max(
                    chat_files::UPLOAD_BYTES,
                )),
        )
        .route(
            "/api/chats/{id}/files/{doc}",
            axum::routing::delete(remove_chat_file),
        )
        .route("/api/keep", get(get_keep).post(set_keep))
        .route(
            "/api/keep/preview",
            post(routes::housekeeping::preview_keep),
        )
        .route("/api/export", get(export_zip))
        .route(
            "/api/chats/{id}",
            get(get_chat)
                .put(put_chat)
                .delete(delete_chat)
                .layer(axum::extract::DefaultBodyLimit::max(chats::CHAT_BYTES)),
        )
        .route("/api/settings", get(get_settings).post(change_setting))
        .route("/api/settings/test", post(test_setting))
        .route("/api/settings/usage", post(routes::settings::refresh_usage))
        .route("/api/processes", get(routes::processes::list))
        .route(
            "/api/import",
            post(start_import)
                .layer(axum::extract::DefaultBodyLimit::max(IMPORT_BYTES))
                .layer(axum::middleware::from_fn_with_state(
                    state.clone(),
                    routes::uploads::import_limit,
                )),
        )
        .route("/api/import/{id}", get(import_status))
        .route("/api/activity", get(activity))
        .route("/api/visible", post(page_visible))
        .route("/api/image", get(get_picture))
        .route(
            "/api/images",
            post(add_picture).layer(axum::extract::DefaultBodyLimit::max(PICTURE_BYTES)),
        )
        .route(
            "/api/app",
            get(app_settings::get)
                .post(app_settings::change)
                .layer(axum::extract::DefaultBodyLimit::max(16_384)),
        )
        .route("/api/health", get(app_settings::health))
        .route(
            "/api/dictation",
            get(dictation::get)
                .post(dictation::change)
                .layer(axum::extract::DefaultBodyLimit::max(200_000)),
        )
        .route(
            "/api/speech",
            get(speech::get)
                .post(speech::change)
                .layer(axum::extract::DefaultBodyLimit::max(4096)),
        )
        .route(
            "/api/writing/ollama",
            get(ollama::get)
                .post(ollama::change)
                .layer(axum::extract::DefaultBodyLimit::max(4096)),
        )
        .route("/api/record", get(record::overview).post(record::start))
        .route(
            "/api/record/{id}",
            get(record::status).delete(record::discard),
        )
        .route("/api/record/{id}/pause", post(record::pause))
        .route("/api/record/{id}/point", post(record::point))
        .route("/api/record/{id}/wants", post(record::wants))
        .route(
            "/api/record/{id}/snapshot",
            post(record::snapshot).layer(axum::extract::DefaultBodyLimit::max(
                routes::pictures::PICTURE_BYTES,
            )),
        )
        .route("/api/record/{id}/stop", post(record::stop))
        .route("/api/record/{id}/finish", post(record::finish_available))
        .route("/api/record/{id}/recover", post(record::recover))
        .route("/recording.js", get(recording_js))
        .route("/api/notes/{id}/originals", get(list_originals))
        .route(
            "/api/notes/{id}/originals/{name}/passage",
            get(routes::felix::original_passage),
        )
        .route("/api/notes/{id}/originals.zip", get(originals_zip))
        .route("/api/notes/{id}/originals/{name}", get(get_original))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            note_activity,
        ))
        .route("/favicon.svg", get(favicon))
        .layer(axum::middleware::from_fn(security_headers))
        .layer(axum::middleware::from_fn(note_peer))
        .with_state(state)
}

async fn stop_idle_sessions(warm: Arc<warm::Slot>) {
    let mut every = tokio::time::interval(std::time::Duration::from_secs(30));
    loop {
        every.tick().await;
        if !warm.holding() {
            continue;
        }
        let warm = Arc::clone(&warm);
        let _ = tokio::task::spawn_blocking(move || warm.tidy()).await;
    }
}

fn store_now<R>(
    state: &AppState,
    work: impl FnOnce(&mut Store) -> Result<R, StatusCode>,
) -> Result<R, StatusCode> {
    let mut store = state
        .store
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if store.reload || store.changed_on_disk() {
        store
            .refresh()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        store.reload = false;
    }
    let result = work(&mut store);
    if result.is_err() {
        store.reload = true;
    }
    result
}

// ── Query params ──────────────────────────────────────────────────────────

// ── Request bodies ────────────────────────────────────────────────────────

// ── Response types ────────────────────────────────────────────────────────

// ── Handlers ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod activity_tests {
    use super::*;

    #[test]
    fn polling_does_not_count_as_someone_using_leo() {
        use axum::http::Method;
        assert!(counts_as_use(&Method::PATCH, "/api/notes/abc"));
        assert!(counts_as_use(&Method::GET, "/api/notes"));
        assert!(counts_as_use(&Method::POST, "/api/record/r1/stop"));
        for polled in [
            "/api/app",
            "/api/dictation",
            "/api/speech",
            "/api/writing/ollama",
            "/api/tasks",
            "/api/processes",
            "/api/activity",
            "/api/record/r1",
            "/api/import/j1",
        ] {
            assert!(!counts_as_use(&Method::GET, polled), "{polled}");
        }
        assert!(!counts_as_use(&Method::GET, "/app.js"));
    }

    #[test]
    fn idle_time_restarts_whenever_leo_is_used() {
        let activity = Activity::default();
        std::thread::sleep(std::time::Duration::from_millis(30));
        assert!(activity.idle() >= std::time::Duration::from_millis(30));
        activity.touch();
        assert!(activity.idle() < std::time::Duration::from_millis(20));
    }
}
