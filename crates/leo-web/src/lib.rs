pub mod chat;
pub mod graph;
mod token;
pub mod tunnel;

pub use chat::Streamer;
pub use graph::Writer;

pub trait SettingsApi: Send + Sync {
    fn describe(&self, notes_dir: &std::path::Path) -> serde_json::Value;
    fn apply(&self, change: &serde_json::Value, secure: bool) -> Result<String>;
    fn test(&self, task: &str) -> Result<String>;
}

#[derive(Clone, Default)]
pub struct Powers {
    pub writer: Option<Writer>,
    pub chat: Option<Streamer>,
    pub settings: Option<Arc<dyn SettingsApi>>,
}

#[cfg(test)]
use std::sync::MutexGuard;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use axum::{
    extract::{Path, Query, Request, State},
    http::{header, HeaderValue, StatusCode},
    middleware::Next,
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Json, Router,
};
use colored::Colorize;
use serde::Deserialize;

use leo_core::store::Store;

#[derive(Clone)]
struct AppState {
    store: Arc<Mutex<Storage>>,
    token: String,
    graphs: Arc<graph::Graphs>,
    chat: Option<Streamer>,
    settings: Option<Arc<dyn SettingsApi>>,
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

const HTML: &str = include_str!("web/index.html");
const APP_JS: &str = include_str!("web/app.js");
const MARKDOWN_JS: &str = include_str!("web/markdown.js");
const EDITING_JS: &str = include_str!("web/editing.js");
const SAVING_JS: &str = include_str!("web/saving.js");
const DOC_JS: &str = include_str!("web/doc.js");
const GRAPH_JS: &str = include_str!("web/graph.js");
const CHAT_JS: &str = include_str!("web/chat.js");

const COOKIE_DAYS: u32 = 30;

#[derive(Debug, Clone, Copy)]
pub struct ServeOptions {
    pub port: u16,
    pub local: bool,
    pub new_token: bool,
}

pub async fn serve(options: ServeOptions, powers: Powers) -> Result<()> {
    let store = Store::load()?;
    let graphs = Arc::new(graph::Graphs::for_notes(&store.notes_dir, powers.writer));
    let chat = powers.chat;
    let settings = powers.settings;
    let count = store.notes.len();
    let token = token::load_or_create(
        &leo_core::paths::config_dir()?.join("serve-token"),
        options.new_token,
    )?;
    if !options.local && !leo_core::paths::on_path("cloudflared") {
        anyhow::bail!(tunnel::MISSING);
    }

    let (listener, port) = bind(options.port).await?;
    let app = router(AppState {
        store: Arc::new(Mutex::new(Storage {
            store,
            reload: false,
        })),
        token: token.clone(),
        graphs,
        chat,
        settings,
    });

    let tunnel = if !options.local {
        println!();
        println!("  {}", "Opening a link that works from anywhere…".dimmed());
        Some(tunnel::start(port).await?)
    } else {
        None
    };

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
    if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
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
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    drop(tunnel);
    Ok(())
}

pub fn hyperlink(url: &str, shown: &str) -> String {
    format!("\x1b]8;;{url}\x1b\\{shown}\x1b]8;;\x1b\\")
}

pub fn styled_link(url: &str, terminal: bool, program: Option<&str>) -> String {
    let shown = url.cyan().underline().to_string();
    if terminal && program != Some("Apple_Terminal") {
        hyperlink(url, &shown)
    } else {
        shown
    }
}

fn clickable(url: &str) -> String {
    use std::io::IsTerminal;
    let program = std::env::var("TERM_PROGRAM").ok();
    styled_link(url, std::io::stdout().is_terminal(), program.as_deref())
}

fn open_on_enter(url: String) {
    use std::io::{BufRead, IsTerminal};
    if !std::io::stdin().is_terminal() {
        return;
    }
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            if line.is_err() {
                break;
            }
            match leo_core::obsidian::open_link(&url) {
                Ok(()) => println!("  {}", "Opened in your browser.".dimmed()),
                Err(e) => println!(
                    "  Could not open a browser ({e}). The link above works in any browser."
                ),
            }
        }
    });
}

async fn bind(wanted: u16) -> Result<(tokio::net::TcpListener, u16)> {
    let mut last = None;
    for port in wanted..wanted.saturating_add(20) {
        match tokio::net::TcpListener::bind(std::net::SocketAddr::from(([0, 0, 0, 0], port))).await
        {
            Ok(listener) => return Ok((listener, port)),
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => last = Some(e),
            Err(e) => return Err(e.into()),
        }
    }
    Err(anyhow::anyhow!(
        "ports {wanted} to {} are all in use ({}); try `leo serve --port 4000`",
        wanted.saturating_add(19),
        last.map(|e| e.to_string()).unwrap_or_default()
    ))
}

fn print_qr(link: &str) {
    if let Ok(code) = qrcode::QrCode::new(link) {
        use qrcode::render::unicode::Dense1x2;
        let qr = code
            .render::<Dense1x2>()
            .dark_color(Dense1x2::Light)
            .light_color(Dense1x2::Dark)
            .build();
        println!();
        for line in qr.lines() {
            println!("  {line}");
        }
        println!();
    }
}

fn keep_awake() -> Option<std::process::Child> {
    if !cfg!(target_os = "macos") || !leo_core::paths::on_path("caffeinate") {
        return None;
    }
    std::process::Command::new("caffeinate")
        .args(["-i", "-w", &std::process::id().to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()
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
        .route("/api/search", get(search_notes))
        .route("/api/tags", get(list_tags))
        .route("/api/dirs", get(list_dirs).post(create_dir))
        .route("/api/folders", get(list_folders))
        .route("/api/trash", get(list_trash))
        .route("/api/trash/{id}/restore", post(restore_note))
        .route("/app.js", get(app_js))
        .route("/markdown.js", get(markdown_js))
        .route("/editing.js", get(editing_js))
        .route("/doc.js", get(doc_js))
        .route("/saving.js", get(saving_js))
        .route("/graph.js", get(graph_js))
        .route("/chat.js", get(chat_js))
        .route("/api/chat", post(chat_reply))
        .route("/api/settings", get(get_settings).post(change_setting))
        .route("/api/settings/test", post(test_setting))
        .route("/api/graph", get(get_graph))
        .route("/api/graph/status", get(graph_status))
        .route("/api/graph/build", post(build_graph))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ))
        .layer(axum::middleware::from_fn(security_headers))
        .with_state(state)
}

async fn index() -> Html<&'static str> {
    Html(HTML)
}

fn javascript(source: &'static str) -> Response {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        source,
    )
        .into_response()
}

async fn app_js() -> Response {
    javascript(APP_JS)
}

async fn markdown_js() -> Response {
    javascript(MARKDOWN_JS)
}

async fn editing_js() -> Response {
    javascript(EDITING_JS)
}

async fn saving_js() -> Response {
    javascript(SAVING_JS)
}

async fn doc_js() -> Response {
    javascript(DOC_JS)
}

async fn graph_js() -> Response {
    javascript(GRAPH_JS)
}

async fn chat_js() -> Response {
    javascript(CHAT_JS)
}

fn secure_request(headers: &axum::http::HeaderMap) -> bool {
    let forwarded = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("https"));
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let name = if host.starts_with('[') {
        host.split(']')
            .next()
            .map(|h| format!("{h}]"))
            .unwrap_or_default()
    } else {
        host.split(':').next().unwrap_or("").to_string()
    };
    forwarded || matches!(name.as_str(), "localhost" | "127.0.0.1" | "[::1]")
}

fn no_settings() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({ "error": "Settings are not available from this server." })),
    )
        .into_response()
}

async fn get_settings(State(state): State<AppState>, headers: axum::http::HeaderMap) -> Response {
    let Some(settings) = state.settings.clone() else {
        return no_settings();
    };
    let notes_dir = match state.with_store(|store| Ok(store.notes_dir.clone())).await {
        Ok(dir) => dir,
        Err(code) => return code.into_response(),
    };
    let secure = secure_request(&headers);
    match tokio::task::spawn_blocking(move || settings.describe(&notes_dir)).await {
        Ok(mut page) => {
            page["secure"] = serde_json::Value::Bool(secure);
            Json(page).into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn change_setting(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(change): Json<serde_json::Value>,
) -> Response {
    let Some(settings) = state.settings.clone() else {
        return no_settings();
    };
    let notes_dir = match state.with_store(|store| Ok(store.notes_dir.clone())).await {
        Ok(dir) => dir,
        Err(code) => return code.into_response(),
    };
    let secure = secure_request(&headers);
    let done = tokio::task::spawn_blocking(move || {
        settings.apply(&change, secure).map(|message| {
            let mut page = settings.describe(&notes_dir);
            page["secure"] = serde_json::Value::Bool(secure);
            (message, page)
        })
    })
    .await;
    match done {
        Ok(Ok((message, page))) => {
            Json(serde_json::json!({ "message": message, "settings": page })).into_response()
        }
        Ok(Err(e)) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn test_setting(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let Some(settings) = state.settings.clone() else {
        return no_settings();
    };
    let task = body
        .get("task")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();
    match tokio::task::spawn_blocking(move || settings.test(&task)).await {
        Ok(Ok(message)) => Json(serde_json::json!({ "message": message })).into_response(),
        Ok(Err(e)) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

fn ndjson(value: serde_json::Value) -> String {
    format!("{value}\n")
}

async fn chat_reply(State(state): State<AppState>, Json(body): Json<chat::ChatBody>) -> Response {
    let Some(streamer) = state.chat.clone() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "leo serve was started without AI." })),
        )
            .into_response();
    };
    if body
        .messages
        .last()
        .is_none_or(|t| t.role != "user" || t.text.trim().is_empty())
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let mode = chat::mode_of(body.mode.as_deref());
    let question = chat::question_of(&body.messages);
    let graphs = Arc::clone(&state.graphs);
    let note = body.note.clone();
    let gathered = state
        .with_store(move |store| {
            let cache = graphs.load();
            Ok(chat::gather(store, &cache, note.as_deref(), &question))
        })
        .await;
    let (sources, notes) = match gathered {
        Ok(found) => found,
        Err(code) => return code.into_response(),
    };
    let (system, user) = chat::prompt(mode, &notes, &body.messages);
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let _ = tx.send(ndjson(serde_json::json!({ "sources": sources })));
    tokio::task::spawn_blocking(move || {
        let pieces = tx.clone();
        let restarts = tx.clone();
        let result = streamer(
            &system,
            &user,
            chat::REPLY_TOKENS,
            &mut |text| {
                let _ = pieces.send(ndjson(serde_json::json!({ "t": text })));
            },
            &mut || {
                let _ = restarts.send(ndjson(serde_json::json!({ "restart": true })));
            },
        );
        let end = match result {
            Ok(_) => serde_json::json!({ "done": true }),
            Err(e) => serde_json::json!({ "error": e.to_string() }),
        };
        let _ = tx.send(ndjson(end));
    });
    let stream = futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv()
            .await
            .map(|line| (Ok::<_, std::io::Error>(line), rx))
    });
    let mut response = axum::body::Body::from_stream(stream).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-ndjson; charset=utf-8"),
    );
    response
        .headers_mut()
        .insert("x-accel-buffering", HeaderValue::from_static("no"));
    response
}

async fn auth_middleware(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let query = request.uri().query().unwrap_or("");
    if let Some(given) = query_param(query, "token") {
        if token::same(given, &state.token) {
            let https = request
                .headers()
                .get("x-forwarded-proto")
                .is_some_and(|v| v.as_bytes().eq_ignore_ascii_case(b"https"));
            let cookie = session_cookie(&state.token, https);
            let mut response =
                if request.method() == axum::http::Method::GET && request.uri().path() == "/" {
                    Redirect::to("/").into_response()
                } else {
                    next.run(request).await
                };
            if let Ok(value) = HeaderValue::from_str(&cookie) {
                response.headers_mut().insert(header::SET_COOKIE, value);
            }
            return response;
        }
    }

    let from_cookie = request
        .headers()
        .get(header::COOKIE)
        .and_then(|c| c.to_str().ok())
        .is_some_and(|cookies| {
            cookies.split(';').any(|part| {
                part.trim()
                    .strip_prefix("leo_token=")
                    .is_some_and(|v| token::same(v, &state.token))
            })
        });
    if from_cookie {
        return next.run(request).await;
    }

    if request.uri().path() == "/" {
        return (StatusCode::UNAUTHORIZED, Html(LOCKED)).into_response();
    }
    StatusCode::UNAUTHORIZED.into_response()
}

fn session_cookie(token: &str, https: bool) -> String {
    format!(
        "leo_token={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{}",
        COOKIE_DAYS * 24 * 3600,
        if https { "; Secure" } else { "" }
    )
}

const LOCKED: &str = "<!doctype html><meta name=viewport content='width=device-width'>\
<title>leo</title><body style='font-family:system-ui;padding:2em;line-height:1.5'>\
<h2>This page needs its link</h2><p>Open the link <code>leo serve</code> printed, \
or scan its QR code again. If the link was changed with \
<code>leo serve --new-token</code>, older links no longer work.</p>";

async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    for (name, value) in [
        ("referrer-policy", "no-referrer"),
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
        ("cache-control", "no-store"),
        (
            "content-security-policy",
            "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
             img-src 'self' data:; connect-src 'self'; base-uri 'none'; form-action 'self'; \
             frame-ancestors 'none'",
        ),
    ] {
        headers.insert(name, HeaderValue::from_static(value));
    }
    response
}

fn query_param<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == key {
                return Some(v);
            }
        }
    }
    None
}

// ── Query params ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ListParams {
    tag: Option<String>,
    limit: Option<usize>,
    dir: Option<String>,
}

#[derive(Deserialize)]
struct SearchParams {
    q: Option<String>,
}

#[derive(Deserialize)]
struct ToggleParams {
    checkbox: usize,
}

#[derive(Deserialize)]
struct DirParams {
    parent: Option<String>,
}

// ── Request bodies ────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CreateBody {
    title: String,
    body: Option<String>,
    tags: Option<Vec<String>>,
    directory: Option<String>,
}

#[derive(Deserialize)]
struct UpdateBody {
    title: Option<String>,
    body: Option<String>,
    tags: Option<Vec<String>>,
    pinned: Option<bool>,
    base: Option<String>,
}

#[derive(Deserialize)]
struct CreateDirBody {
    path: String,
}

#[derive(Deserialize)]
struct MoveBody {
    directory: String,
}

// ── Response types ────────────────────────────────────────────────────────

#[derive(Debug, serde::Serialize)]
struct NoteResponse {
    id: String,
    title: String,
    body: String,
    created_at: String,
    updated_at: String,
    tags: Vec<String>,
    directory: String,
    pinned: bool,
    version: String,
}

fn version_of(note: &leo_core::notes::Note) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let parts = [
        note.title.as_str(),
        note.body.as_str(),
        &note.tags.join("\u{1f}"),
    ];
    for part in parts {
        for byte in part.bytes().chain([0u8]) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    format!("{hash:016x}")
}

impl NoteResponse {
    fn from_note(n: &leo_core::notes::Note) -> Self {
        NoteResponse {
            id: n.id.clone(),
            title: n.title.clone(),
            body: n.body.clone(),
            created_at: n.created_at.to_rfc3339(),
            updated_at: n.updated_at.to_rfc3339(),
            tags: n.tags.clone(),
            directory: n.directory.clone(),
            pinned: n.pinned,
            version: version_of(n),
        }
    }
}

#[derive(serde::Serialize)]
struct TagResponse {
    tag: String,
    count: usize,
}

// ── Handlers ──────────────────────────────────────────────────────────────

fn save(store: &Store) -> Result<(), StatusCode> {
    store.save().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn directory(store: &Store, path: &str) -> Result<(), StatusCode> {
    store
        .validate_directory(path)
        .map_err(|_| StatusCode::BAD_REQUEST)
}

async fn list_notes(
    State(state): State<AppState>,
    Query(params): Query<ListParams>,
) -> Result<Json<Vec<NoteResponse>>, StatusCode> {
    state
        .with_store(move |store| {
            let limit = params.limit.unwrap_or(100).min(1000);
            let notes = if let Some(ref dir) = params.dir {
                directory(store, dir)?;
                store.list_notes_in_dir(dir, params.tag.as_deref(), limit)
            } else {
                store.list_notes(params.tag.as_deref(), limit)
            };
            Ok(Json(
                notes.iter().map(|n| NoteResponse::from_note(n)).collect(),
            ))
        })
        .await
}

async fn get_note(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<NoteResponse>, StatusCode> {
    state
        .with_store(move |store| {
            let note = store.find_note(&id).ok_or(StatusCode::NOT_FOUND)?;
            Ok(Json(NoteResponse::from_note(note)))
        })
        .await
}

async fn create_note(
    State(state): State<AppState>,
    Json(body): Json<CreateBody>,
) -> Result<impl IntoResponse, StatusCode> {
    state
        .with_store(move |store| {
            let dir = body.directory.unwrap_or_default();
            directory(store, &dir)?;
            if !store.dir_exists(&dir) {
                store.create_dir(&dir);
            }
            let note = store
                .create_note(
                    body.title,
                    body.body.unwrap_or_default(),
                    body.tags.unwrap_or_default(),
                    &dir,
                )
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            let resp = NoteResponse::from_note(note);
            save(store)?;
            Ok((StatusCode::CREATED, Json(resp)))
        })
        .await
}

async fn update_note(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateBody>,
) -> Result<Json<NoteResponse>, StatusCode> {
    state
        .with_store(move |store| {
            let note = store.find_note_mut(&id).ok_or(StatusCode::NOT_FOUND)?;
            let edited = body.title.is_some() || body.body.is_some() || body.tags.is_some();
            if edited
                && body
                    .base
                    .as_deref()
                    .is_some_and(|base| base != version_of(note))
            {
                return Err(StatusCode::CONFLICT);
            }
            if let Some(title) = body.title {
                note.title = title;
            }
            if let Some(text) = body.body {
                note.body = text;
            }
            if let Some(tags) = body.tags {
                note.tags = tags;
            }
            if let Some(pinned) = body.pinned {
                note.pinned = pinned;
            }
            if edited {
                note.updated_at = chrono::Utc::now();
            }
            let resp = NoteResponse::from_note(note);
            save(store)?;
            Ok(Json(resp))
        })
        .await
}

async fn delete_note(State(state): State<AppState>, Path(id): Path<String>) -> StatusCode {
    state
        .with_store(move |store| {
            let full = store
                .find_note(&id)
                .map(|n| n.id.clone())
                .ok_or(StatusCode::NOT_FOUND)?;
            store.delete_notes(&[full]);
            save(store)?;
            Ok(StatusCode::NO_CONTENT)
        })
        .await
        .unwrap_or_else(|status| status)
}

async fn toggle_checkbox(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<ToggleParams>,
) -> Result<Json<NoteResponse>, StatusCode> {
    state
        .with_store(move |store| {
            store
                .toggle_checkbox(&id, params.checkbox)
                .ok_or(StatusCode::NOT_FOUND)?;
            let resp = NoteResponse::from_note(store.find_note(&id).ok_or(StatusCode::NOT_FOUND)?);
            save(store)?;
            Ok(Json(resp))
        })
        .await
}

async fn move_note(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<MoveBody>,
) -> Result<Json<NoteResponse>, StatusCode> {
    state
        .with_store(move |store| {
            directory(store, &body.directory)?;
            if !store.dir_exists(&body.directory) {
                return Err(StatusCode::NOT_FOUND);
            }
            store
                .move_note(&id, &body.directory)
                .ok_or(StatusCode::NOT_FOUND)?;
            let resp = NoteResponse::from_note(store.find_note(&id).ok_or(StatusCode::NOT_FOUND)?);
            save(store)?;
            Ok(Json(resp))
        })
        .await
}

async fn search_notes(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Result<Json<Vec<NoteResponse>>, StatusCode> {
    state
        .with_store(move |store| {
            let q = params.q.unwrap_or_default();
            let notes = if q.is_empty() { vec![] } else { store.find(&q) };
            Ok(Json(
                notes.iter().map(|n| NoteResponse::from_note(n)).collect(),
            ))
        })
        .await
}

async fn list_tags(State(state): State<AppState>) -> Result<Json<Vec<TagResponse>>, StatusCode> {
    state
        .with_store(|store| {
            Ok(Json(
                store
                    .tags()
                    .into_iter()
                    .map(|(tag, count)| TagResponse { tag, count })
                    .collect(),
            ))
        })
        .await
}

#[derive(serde::Serialize)]
struct DirResponse {
    name: String,
    notes: usize,
}

async fn list_dirs(
    State(state): State<AppState>,
    Query(params): Query<DirParams>,
) -> Result<Json<Vec<DirResponse>>, StatusCode> {
    state
        .with_store(move |store| {
            let parent = params.parent.unwrap_or_default();
            directory(store, &parent)?;
            Ok(Json(
                store
                    .subdirs(&parent)
                    .into_iter()
                    .map(|name| {
                        let full = if parent.is_empty() {
                            name.clone()
                        } else {
                            format!("{parent}/{name}")
                        };
                        DirResponse {
                            name,
                            notes: store.dir_contents(&full).0,
                        }
                    })
                    .collect(),
            ))
        })
        .await
}

async fn list_folders(State(state): State<AppState>) -> Result<Json<Vec<DirResponse>>, StatusCode> {
    state
        .with_store(|store| {
            let mut all = store.directories.clone();
            all.sort_by_key(|d| d.to_lowercase());
            Ok(Json(
                all.into_iter()
                    .map(|name| DirResponse {
                        notes: store.dir_contents(&name).0,
                        name,
                    })
                    .collect(),
            ))
        })
        .await
}

#[derive(serde::Serialize)]
struct TrashResponse {
    id: String,
    title: String,
    directory: String,
    deleted_at: String,
}

async fn list_trash(State(state): State<AppState>) -> Result<Json<Vec<TrashResponse>>, StatusCode> {
    state
        .with_store(|store| {
            Ok(Json(
                store
                    .trashed()
                    .into_iter()
                    .map(|t| TrashResponse {
                        id: t.id,
                        title: t.title,
                        directory: t.directory,
                        deleted_at: t.deleted_at.to_rfc3339(),
                    })
                    .collect(),
            ))
        })
        .await
}

async fn restore_note(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<NoteResponse>, StatusCode> {
    state
        .with_store(move |store| {
            store.restore(&id).ok_or(StatusCode::NOT_FOUND)?;
            save(store)?;
            Ok(Json(NoteResponse::from_note(
                store.find_note(&id).ok_or(StatusCode::NOT_FOUND)?,
            )))
        })
        .await
}

#[derive(serde::Serialize)]
struct GraphResponse {
    graph: graph::Graph,
    status: graph::Status,
}

async fn note_sources(state: &AppState) -> Result<Vec<graph::Source>, StatusCode> {
    state
        .with_store(|store| Ok(graph::sources(&store.notes)))
        .await
}

async fn get_graph(State(state): State<AppState>) -> Result<Json<GraphResponse>, StatusCode> {
    let sources = note_sources(&state).await?;
    let graphs = Arc::clone(&state.graphs);
    tokio::task::spawn_blocking(move || {
        let cache = graphs.load();
        Json(GraphResponse {
            graph: graph::assemble(&sources, &cache),
            status: graphs.status(&sources),
        })
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn graph_status(State(state): State<AppState>) -> Result<Json<graph::Status>, StatusCode> {
    let sources = note_sources(&state).await?;
    let graphs = Arc::clone(&state.graphs);
    tokio::task::spawn_blocking(move || Json(graphs.status(&sources)))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn build_graph(
    State(state): State<AppState>,
) -> Result<(StatusCode, Json<graph::Status>), StatusCode> {
    let sources = note_sources(&state).await?;
    let graphs = Arc::clone(&state.graphs);
    tokio::task::spawn_blocking(move || {
        graphs.start(sources.clone());
        (StatusCode::ACCEPTED, Json(graphs.status(&sources)))
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn create_dir(State(state): State<AppState>, Json(body): Json<CreateDirBody>) -> StatusCode {
    state
        .with_store(move |store| {
            directory(store, &body.path)?;
            if !store.create_dir(&body.path) {
                return Err(StatusCode::CONFLICT);
            }
            save(store)?;
            Ok(StatusCode::CREATED)
        })
        .await
        .unwrap_or_else(|status| status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_is_clickable_and_still_reads_as_itself() {
        let url = "https://example.trycloudflare.com/?token=abc";
        let link = hyperlink(url, url);
        assert_eq!(
            link,
            "\x1b]8;;https://example.trycloudflare.com/?token=abc\x1b\\https://example.trycloudflare.com/?token=abc\x1b]8;;\x1b\\"
        );
        assert!(!clickable(url).contains("\x1b]8"), "a pipe gets plain text");
        assert!(styled_link(url, true, Some("iTerm.app")).contains("\x1b]8;;"));
        assert!(styled_link(url, true, None).contains("\x1b]8;;"));
        let apple = styled_link(url, true, Some("Apple_Terminal"));
        assert!(
            !apple.contains("\x1b]8"),
            "Terminal.app finds plain links itself"
        );
        assert!(apple.contains(url));
    }

    fn state_with(notes: &[(&str, &str)]) -> (AppState, tempfile::TempDir, Vec<String>) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::load_from(&dir.path().join("notes")).unwrap();
        let ids = notes
            .iter()
            .map(|(title, d)| store.create_note(*title, "", vec![], d).unwrap().id.clone())
            .collect();
        store.save().unwrap();
        let graphs = Arc::new(graph::Graphs::for_notes(&store.notes_dir, None));
        let state = AppState {
            store: Arc::new(Mutex::new(Storage {
                store,
                reload: false,
            })),
            token: String::new(),
            graphs,
            chat: None,
            settings: None,
        };
        (state, dir, ids)
    }

    fn run<F: std::future::Future>(f: F) -> F::Output {
        tokio::runtime::Runtime::new().unwrap().block_on(f)
    }

    #[test]
    fn keys_count_as_safe_only_over_https_or_on_this_computer() {
        let with = |pairs: &[(&'static str, &'static str)]| {
            let mut headers = axum::http::HeaderMap::new();
            for (k, v) in pairs {
                headers.insert(*k, HeaderValue::from_static(v));
            }
            secure_request(&headers)
        };
        assert!(with(&[
            ("host", "abc.trycloudflare.com"),
            ("x-forwarded-proto", "https")
        ]));
        assert!(with(&[("host", "127.0.0.1:8742")]));
        assert!(with(&[("host", "localhost:8742")]));
        assert!(with(&[("host", "[::1]:8742")]));
        assert!(!with(&[("host", "192.168.1.50:8742")]));
        assert!(!with(&[("host", "localhost.evil.example")]));
        assert!(!with(&[]));
    }

    #[test]
    fn a_chat_reply_streams_its_sources_then_the_answer() {
        let (mut state, _d, ids) = state_with(&[("Heaps", "cs130")]);
        {
            let mut store = state.fresh();
            store.find_note_mut(&ids[0]).unwrap().body =
                "A binary heap backs a priority queue.".into();
            store.save().unwrap();
        }
        let seen = Arc::new(Mutex::new(String::new()));
        let saw = Arc::clone(&seen);
        state.chat = Some(Arc::new(
            move |system: &str,
                  user: &str,
                  _: u32,
                  piece: &mut dyn FnMut(&str),
                  _: &mut dyn FnMut()| {
                *saw.lock().unwrap() = format!("{system}\n{user}");
                piece("Heaps keep the minimum on top ");
                piece("[n1].");
                Ok("done".to_string())
            },
        ));
        let body = chat::ChatBody {
            messages: vec![chat::Turn {
                role: "user".into(),
                text: "how do heaps work?".into(),
            }],
            mode: Some("coach".into()),
            note: Some(ids[0].clone()),
        };
        let text = run(async {
            let response = chat_reply(State(state.clone()), Json(body)).await;
            assert_eq!(response.status(), StatusCode::OK);
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            String::from_utf8(bytes.to_vec()).unwrap()
        });
        let lines: Vec<serde_json::Value> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines[0]["sources"][0]["title"], "Heaps");
        assert_eq!(lines[0]["sources"][0]["why"], "open");
        assert_eq!(lines[1]["t"], "Heaps keep the minimum on top ");
        assert_eq!(lines[2]["t"], "[n1].");
        assert_eq!(lines[3]["done"], true);
        let prompt = seen.lock().unwrap().clone();
        assert!(prompt.contains("Mode: study coach"), "{prompt}");
        assert!(prompt.contains("A binary heap backs a priority queue."));
        assert!(prompt.contains("User: how do heaps work?"));
    }

    #[test]
    fn a_chat_without_ai_or_a_question_is_refused() {
        let (state, _d, _ids) = state_with(&[]);
        let ask = |text: &str| chat::ChatBody {
            messages: vec![chat::Turn {
                role: "user".into(),
                text: text.into(),
            }],
            mode: None,
            note: None,
        };
        let status = run(async {
            chat_reply(State(state.clone()), Json(ask("hi")))
                .await
                .status()
        });
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        let mut with = state.clone();
        with.chat = Some(Arc::new(
            |_: &str, _: &str, _: u32, _: &mut dyn FnMut(&str), _: &mut dyn FnMut()| {
                anyhow::bail!("no AI for writing is chosen")
            },
        ));
        let status = run(async {
            chat_reply(State(with.clone()), Json(ask("   ")))
                .await
                .status()
        });
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let text = run(async {
            let response = chat_reply(State(with.clone()), Json(ask("hi"))).await;
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            String::from_utf8(bytes.to_vec()).unwrap()
        });
        assert!(
            text.lines()
                .last()
                .unwrap()
                .contains("no AI for writing is chosen"),
            "{text}"
        );
    }

    /// An ID prefix shared by several notes must not delete all of them.
    #[test]
    fn deleting_by_an_ambiguous_prefix_deletes_nothing() {
        let (state, _d, _ids) = state_with(&[("A", ""), ("B", "")]);
        let prefix = {
            let mut store = state.fresh();
            let first = store.notes[0].id.clone();
            store.notes[1].id = format!("{first}x");
            store.save().unwrap();
            first
        };
        let status = run(delete_note(State(state.clone()), Path(prefix)));
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(state.fresh().notes.len(), 2);
    }

    #[test]
    fn creating_a_note_in_a_new_directory_registers_it() {
        let (state, _d, _ids) = state_with(&[]);
        let body = CreateBody {
            title: "Lecture".to_string(),
            body: None,
            tags: None,
            directory: Some("cs162".to_string()),
        };
        run(create_note(State(state.clone()), Json(body)))
            .ok()
            .unwrap();
        assert!(state.fresh().dir_exists("cs162"));
    }

    #[test]
    fn moving_to_a_missing_directory_is_refused() {
        let (state, _d, ids) = state_with(&[("A", "")]);
        let body = MoveBody {
            directory: "nowhere".to_string(),
        };
        let out = run(move_note(
            State(state.clone()),
            Path(ids[0].clone()),
            Json(body),
        ));
        assert_eq!(out.err(), Some(StatusCode::NOT_FOUND));
        assert_eq!(
            state
                .store
                .lock()
                .unwrap()
                .find_note(&ids[0])
                .unwrap()
                .directory,
            ""
        );
    }

    #[test]
    fn a_note_can_be_pinned_from_the_phone() {
        let (state, _d, ids) = state_with(&[("Syllabus", "")]);
        let body = UpdateBody {
            title: None,
            body: None,
            tags: None,
            pinned: Some(true),
            base: None,
        };
        let Json(note) = run(update_note(
            State(state.clone()),
            Path(ids[0].clone()),
            Json(body),
        ))
        .unwrap();
        assert!(note.pinned);
        assert!(state.fresh().find_note(&ids[0]).unwrap().pinned);
    }

    #[test]
    fn a_deleted_note_is_in_the_trash_and_can_be_restored() {
        let (state, _d, ids) = state_with(&[("Lecture 4", "cs130")]);
        assert_eq!(
            run(delete_note(State(state.clone()), Path(ids[0].clone()))),
            StatusCode::NO_CONTENT
        );
        let Json(trash) = run(list_trash(State(state.clone()))).unwrap();
        assert_eq!(trash.len(), 1);
        assert_eq!(trash[0].title, "Lecture 4");
        assert_eq!(trash[0].directory, "cs130");

        let Json(back) = run(restore_note(State(state.clone()), Path(ids[0].clone()))).unwrap();
        assert_eq!(back.directory, "cs130");
        assert!(state.fresh().find_note(&ids[0]).is_some());
        let Json(trash) = run(list_trash(State(state.clone()))).unwrap();
        assert!(trash.is_empty());
        assert_eq!(
            run(restore_note(State(state), Path(ids[0].clone()))).unwrap_err(),
            StatusCode::NOT_FOUND
        );
    }

    #[test]
    fn folders_come_with_how_many_notes_they_hold() {
        let (state, _d, _ids) = state_with(&[("A", "cs130"), ("B", "cs130/lec"), ("C", "")]);
        let Json(dirs) = run(list_dirs(State(state), Query(DirParams { parent: None }))).unwrap();
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].name, "cs130");
        assert_eq!(dirs[0].notes, 2);
    }

    #[test]
    fn every_folder_is_listed_for_moving_a_note() {
        let (state, _d, _ids) = state_with(&[("A", "cs130"), ("B", "cs130/lec"), ("C", "Ideas")]);
        let Json(all) = run(list_folders(State(state))).unwrap();
        let names: Vec<&str> = all.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["cs130", "cs130/lec", "Ideas"]);
    }

    fn edit(
        state: &AppState,
        id: &str,
        body: &str,
        base: Option<String>,
    ) -> Result<NoteResponse, StatusCode> {
        run(update_note(
            State(state.clone()),
            Path(id.to_string()),
            Json(UpdateBody {
                title: None,
                body: Some(body.to_string()),
                tags: None,
                pinned: None,
                base,
            }),
        ))
        .map(|Json(n)| n)
    }

    #[test]
    fn an_edit_based_on_the_current_version_is_saved_and_returns_the_next_version() {
        let (state, _d, ids) = state_with(&[("Shared", "")]);
        let current = run(get_note(State(state.clone()), Path(ids[0].clone())))
            .unwrap()
            .0
            .version;
        let saved = edit(&state, &ids[0], "from the phone", Some(current.clone())).unwrap();
        assert_eq!(saved.body, "from the phone");
        assert_ne!(saved.version, current);
    }

    #[test]
    fn an_edit_based_on_an_old_version_is_refused_and_changes_nothing() {
        let (state, dir, ids) = state_with(&[("Shared", "")]);
        let seen = run(get_note(State(state.clone()), Path(ids[0].clone())))
            .unwrap()
            .0;
        let file = dir.path().join("notes").join("Shared.md");
        let text = std::fs::read_to_string(&file).unwrap();
        std::fs::write(&file, format!("{text}typed in Obsidian\n")).unwrap();

        let refused = edit(&state, &ids[0], "from the phone", Some(seen.version));
        assert_eq!(refused.unwrap_err(), StatusCode::CONFLICT);
        assert!(std::fs::read_to_string(&file)
            .unwrap()
            .contains("typed in Obsidian"));
    }
    #[test]
    fn web_note_creation_directory_creation_and_moves_reject_traversal() {
        let (state, dir, ids) = state_with(&[("A", "")]);
        for path in [
            "../outside",
            "/tmp/outside",
            "nested/../../outside",
            "C:\\outside",
            ".trash",
        ] {
            let created = run(create_note(
                State(state.clone()),
                Json(CreateBody {
                    title: "Escape".into(),
                    body: None,
                    tags: None,
                    directory: Some(path.into()),
                }),
            ));
            assert_eq!(created.err(), Some(StatusCode::BAD_REQUEST));
            assert_eq!(
                run(create_dir(
                    State(state.clone()),
                    Json(CreateDirBody { path: path.into() })
                )),
                StatusCode::BAD_REQUEST
            );
            assert_eq!(
                run(move_note(
                    State(state.clone()),
                    Path(ids[0].clone()),
                    Json(MoveBody {
                        directory: path.into()
                    })
                ))
                .err(),
                Some(StatusCode::BAD_REQUEST)
            );
        }
        assert!(!dir.path().join("outside").exists());
        assert_eq!(state.fresh().notes.len(), 1);
    }

    #[test]
    fn a_reload_failure_is_reported_instead_of_serving_a_stale_store() {
        let (state, dir, _) = state_with(&[("A", "")]);
        std::fs::write(dir.path().join("notes/directories.json"), "not json").unwrap();
        let result = run(list_notes(
            State(state),
            Query(ListParams {
                tag: None,
                limit: None,
                dir: None,
            }),
        ));
        assert_eq!(result.err(), Some(StatusCode::INTERNAL_SERVER_ERROR));
    }

    #[test]
    fn an_empty_folder_created_by_another_app_is_visible_without_restarting() {
        let (state, dir, _) = state_with(&[]);
        let mut other = Store::load_from(&dir.path().join("notes")).unwrap();
        other.create_dir("new-folder");
        other.save_files().unwrap();
        let Json(dirs) = run(list_dirs(State(state), Query(DirParams { parent: None }))).unwrap();
        assert_eq!(dirs[0].name, "new-folder");
    }
    #[test]
    fn failed_operations_do_not_leave_unsaved_changes_in_the_cached_store() {
        let (state, _dir, ids) = state_with(&[("Original", "")]);
        let id = ids[0].clone();
        let failed: Result<(), StatusCode> = run(state.with_store(move |store| {
            store.find_note_mut(&id).unwrap().body = "Never saved".into();
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }));
        assert!(failed.is_err());
        let Json(note) = run(get_note(State(state), Path(ids[0].clone()))).unwrap();
        assert_eq!(note.body, "");
    }
}
