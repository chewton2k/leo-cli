pub mod chat;
pub mod chat_files;
pub mod chats;
pub mod export;
pub mod graph;
pub mod record;
pub mod review;
mod routes;
pub mod search;
pub mod sessions;
pub mod storage;
mod terminal;
#[cfg(test)]
mod tests;
mod token;
pub mod tools;
pub mod tunnel;

#[cfg(test)]
use std::sync::MutexGuard;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use axum::{
    http::StatusCode,
    routing::{get, post},
    Router,
};
use colored::Colorize;
use leo_core::store::Store;

use crate::routes::activity::activity;
use crate::routes::assets::{
    app_js, chat_js, doc_js, editing_js, favicon, graph_js, index, markdown_js, recorder_js,
    recording_js, saving_js,
};
use crate::routes::auth::{
    auth_middleware, end_sessions, list_sessions, new_link, note_peer, security_headers,
};
use crate::routes::downloads::{export_zip, originals_zip};
use crate::routes::felix::{
    add_chat_file, apply_suggestion, chat_reply, delete_chat, get_chat, get_review,
    list_chat_files, list_chats, mark_reviewed, put_chat, remove_chat_file,
};
use crate::routes::housekeeping::{change_storage, get_keep, get_storage, set_keep};
use crate::routes::map::{build_graph, get_graph, graph_status};
use crate::routes::notes::{
    create_dir, create_note, delete_note, get_note, list_dirs, list_folders, list_notes, move_dir,
    move_note, search_notes, toggle_checkbox, update_note,
};
use crate::routes::pictures::{add_picture, get_picture, PICTURE_BYTES};
use crate::routes::settings::{change_setting, get_settings, test_setting};
use crate::routes::trash::{
    delete_from_trash, list_trash, move_to_trash, restore_many, restore_note,
};
use crate::routes::uploads::{
    get_original, import_status, list_originals, start_import, ImportJob, IMPORT_BYTES,
};
use crate::terminal::{bind, clickable, keep_awake, open_on_enter, print_qr, should_open};

pub use chat::{Reply, Spent, Streamer};
pub use graph::Writer;

pub trait SettingsApi: Send + Sync {
    fn describe(&self, notes_dir: &std::path::Path) -> serde_json::Value;
    fn apply(&self, change: &serde_json::Value, secure: bool) -> Result<String>;
    fn test(&self, task: &str) -> Result<String>;
}

#[derive(Debug, Clone)]
pub struct UploadFile {
    pub name: String,
    pub mime: String,
    pub bytes: Vec<u8>,
}

pub type Room = Arc<dyn Fn() -> usize + Send + Sync>;

#[derive(Debug, Clone, PartialEq)]
pub struct WebHit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

pub type WebSearch = Arc<dyn Fn(&str) -> Result<Vec<WebHit>> + Send + Sync>;
pub type WebPage = Arc<dyn Fn(&str) -> Result<String> + Send + Sync>;

#[derive(Clone)]
pub struct Web {
    pub search: WebSearch,
    pub page: WebPage,
    pub needed: Arc<dyn Fn() -> bool + Send + Sync>,
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

pub type Importer =
    Arc<dyn Fn(Vec<UploadFile>, &mut dyn FnMut(&str, usize, usize)) -> Result<Made> + Send + Sync>;

#[derive(Clone, Default)]
pub struct Powers {
    pub writer: Option<Writer>,
    pub chat: Option<Streamer>,
    pub settings: Option<Arc<dyn SettingsApi>>,
    pub importer: Option<Importer>,
    pub listener: Option<record::Listener>,
    pub housekeeper: Option<Arc<dyn storage::Housekeeper>>,
    pub reader: Option<Reader>,
    pub room: Option<Room>,
    pub web: Option<Web>,
}

#[derive(Clone)]
struct AppState {
    store: Arc<Mutex<Storage>>,
    gate: Arc<sessions::Gate>,
    graphs: Arc<graph::Graphs>,
    chat: Option<Streamer>,
    settings: Option<Arc<dyn SettingsApi>>,
    importer: Option<Importer>,
    imports: Arc<Mutex<std::collections::HashMap<String, ImportJob>>>,
    listener: Option<record::Listener>,
    recording: record::Recordings,
    chats: std::path::PathBuf,
    housekeeper: Option<Arc<dyn storage::Housekeeper>>,
    reader: Option<Reader>,
    room: Option<Room>,
    web: Option<Web>,
}

struct Storage {
    store: Store,
    reload: bool,
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
        let state = self.clone();
        tokio::task::spawn_blocking(move || {
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

#[derive(Debug, Clone, Copy)]
pub struct ServeOptions {
    pub port: u16,
    pub local: bool,
    pub new_token: bool,
}

pub async fn serve(options: ServeOptions, powers: Powers) -> Result<()> {
    let store = Store::load()?;
    let graphs = Arc::new(graph::Graphs::for_notes(&store.notes_dir, powers.writer));
    let chats = chats::dir_for(&store.notes_dir);
    let chat = powers.chat;
    let settings = powers.settings;
    let importer = powers.importer;
    let recorder = powers.listener;
    let housekeeper = powers.housekeeper;
    let reader = powers.reader;
    let room = powers.room;
    let web = powers.web;
    let count = store.notes.len();
    let token_path = leo_core::paths::config_dir()?.join("serve-token");
    let sessions_path = leo_core::paths::config_dir()?.join("serve-sessions.json");
    let token = token::load_or_create(&token_path, options.new_token)?;
    if !options.local && !leo_core::paths::on_path("cloudflared") {
        anyhow::bail!(tunnel::MISSING);
    }

    let (listener, port) = bind(options.port).await?;
    let gate = Arc::new(sessions::Gate::new(
        token.clone(),
        Some(token_path),
        if options.new_token {
            let fresh = sessions::Sessions::load(&sessions_path);
            fresh.end_all();
            fresh
        } else {
            sessions::Sessions::load(&sessions_path)
        },
    ));
    let app = router(AppState {
        store: Arc::new(Mutex::new(Storage {
            store,
            reload: false,
        })),
        gate: Arc::clone(&gate),
        graphs,
        chat,
        settings,
        importer,
        imports: Default::default(),
        listener: recorder,
        recording: Default::default(),
        chats,
        housekeeper,
        reader,
        room,
        web,
    });

    let tunnel = if !options.local {
        println!();
        println!("  {}", "Opening a link that works from anywhere…".dimmed());
        Some(tunnel::start(port).await?)
    } else {
        None
    };
    gate.sessions
        .retire_links(tunnel.as_ref().and_then(|t| t.url.split("://").nth(1)));

    let local_ip = local_ip_address::local_ip()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|_| "127.0.0.1".to_string());
    let wifi = format!("http://{local_ip}:{port}/?token={token}");

    println!();
    println!(
        "  {} {}",
        "leo serve".bold(),
        format!("· {count} note{}", if count == 1 { "" } else { "s" }).dimmed()
    );
    if port != options.port {
        println!(
            "  {}",
            format!("Port {} was busy, so this uses {port}.", options.port).dimmed()
        );
    }
    println!();
    match &tunnel {
        Some(tunnel) => {
            let anywhere = format!("{}/?token={token}", tunnel.url);
            println!("  {}", "Your link, from any network".bold());
            println!("    {}", clickable(&anywhere));
            print_qr(&anywhere);
            println!(
                "  {} anyone with the whole link can read and edit your notes. Keep it",
                "Careful:".yellow().bold()
            );
            println!("  to yourself; `leo serve --new-token` retires every old link.");
        }
        None => {
            println!("  {}", "On this Wi-Fi".bold());
            println!("    {}", clickable(&wifi));
            print_qr(&wifi);
            println!(
                "  {}",
                "Away from this Wi-Fi? `leo serve` without --local gives a link that works on any network."
                    .dimmed()
            );
            println!(
                "  {} {} {}",
                "iPhone:".bold(),
                "if Safari refuses the page, turn off Settings > Apps > Safari >".dimmed(),
                "HTTPS Upgrade".dimmed()
            );
        }
    }
    println!();
    let here = format!("http://127.0.0.1:{port}/?token={token}");
    let opened = should_open(
        std::io::IsTerminal::is_terminal(&std::io::stdin()),
        std::io::IsTerminal::is_terminal(&std::io::stdout()),
        std::env::var_os("LEO_NO_OPEN").is_some(),
    ) && leo_core::open::link(&here).is_ok();
    if opened {
        println!(
            "  {} {}",
            "Opened in your browser.".bold(),
            "Press Enter to open it again.".dimmed()
        );
    } else if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        println!(
            "  {} {}",
            "Press Enter".bold(),
            "to open it in a browser on this computer.".dimmed()
        );
    }
    println!(
        "  {}",
        "Scan the code with your phone's camera. Keep this window open; Ctrl-C stops.".dimmed()
    );
    println!();
    open_on_enter(here);

    let _awake = keep_awake();
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await?;
    drop(tunnel);
    Ok(())
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
        .route("/api/notes/{id}/move", post(move_note))
        .route("/api/notes/{id}/suggestion", post(apply_suggestion))
        .route("/api/search", get(search_notes))
        .route("/api/dirs", get(list_dirs).post(create_dir))
        .route("/api/dirs/move", post(move_dir))
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
        .route("/editing.js", get(editing_js))
        .route("/doc.js", get(doc_js))
        .route("/saving.js", get(saving_js))
        .route("/graph.js", get(graph_js))
        .route("/chat.js", get(chat_js))
        .route("/api/chat", post(chat_reply))
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
        .route("/api/storage", get(get_storage).post(change_storage))
        .route("/api/keep", get(get_keep).post(set_keep))
        .route("/api/sessions", get(list_sessions))
        .route("/api/sessions/end", post(end_sessions))
        .route("/api/sessions/new-link", post(new_link))
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
        .route(
            "/api/import",
            post(start_import).layer(axum::extract::DefaultBodyLimit::max(IMPORT_BYTES)),
        )
        .route("/api/import/{id}", get(import_status))
        .route("/api/activity", get(activity))
        .route("/api/image", get(get_picture))
        .route(
            "/api/images",
            post(add_picture).layer(axum::extract::DefaultBodyLimit::max(PICTURE_BYTES)),
        )
        .route("/api/record", get(record::overview).post(record::start))
        .route("/api/record/{id}", get(record::status))
        .route(
            "/api/record/{id}/audio",
            post(record::audio).layer(axum::extract::DefaultBodyLimit::max(record::AUDIO_BYTES)),
        )
        .route("/api/record/{id}/pause", post(record::pause))
        .route("/api/record/{id}/point", post(record::point))
        .route("/api/record/{id}/stop", post(record::stop))
        .route("/recorder.js", get(recorder_js))
        .route("/recording.js", get(recording_js))
        .route("/api/notes/{id}/originals", get(list_originals))
        .route("/api/notes/{id}/originals.zip", get(originals_zip))
        .route("/api/notes/{id}/originals/{name}", get(get_original))
        .route("/api/graph", get(get_graph))
        .route("/api/graph/status", get(graph_status))
        .route("/api/graph/build", post(build_graph))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ))
        .route("/favicon.svg", get(favicon))
        .layer(axum::middleware::from_fn(security_headers))
        .layer(axum::middleware::from_fn(note_peer))
        .with_state(state)
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
