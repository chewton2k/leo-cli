mod token;
mod tunnel;

use std::sync::{Arc, Mutex, MutexGuard};

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
    store: Arc<Mutex<Store>>,
    token: String,
}

impl AppState {
    fn fresh(&self) -> MutexGuard<'_, Store> {
        let mut store = self.store.lock().unwrap_or_else(|e| e.into_inner());
        if let Ok(reloaded) = Store::load_from(&store.notes_dir.clone()) {
            *store = reloaded;
        }
        store
    }
}

const HTML: &str = include_str!("web_ui.html");

const COOKIE_DAYS: u32 = 30;

#[derive(Debug, Clone, Copy)]
pub struct ServeOptions {
    pub port: u16,
    pub anywhere: bool,
    pub new_token: bool,
}

pub async fn serve(options: ServeOptions) -> Result<()> {
    let store = Store::load()?;
    let count = store.notes.len();
    let token = token::load_or_create(
        &leo_core::paths::config_dir()?.join("serve-token"),
        options.new_token,
    )?;
    if options.anywhere && !leo_core::paths::on_path("cloudflared") {
        anyhow::bail!(tunnel::MISSING);
    }

    let (listener, port) = bind(options.port).await?;
    let app = router(AppState {
        store: Arc::new(Mutex::new(store)),
        token: token.clone(),
    });

    let tunnel = if options.anywhere {
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
            println!("  {}", "From anywhere".bold());
            println!("    {}", anywhere.cyan().underline());
            print_qr(&anywhere);
            println!("  {}", "On this Wi-Fi".bold());
            println!("    {}", wifi.cyan().underline());
            println!();
            println!(
                "  {} anyone with the whole link can read and edit your notes. Keep it",
                "Careful:".yellow().bold()
            );
            println!("  to yourself; `leo serve --new-token` retires every old link.");
        }
        None => {
            println!("  {}", "On this Wi-Fi".bold());
            println!("    {}", wifi.cyan().underline());
            print_qr(&wifi);
            println!(
                "  {}",
                "Away from this Wi-Fi? `leo serve --anywhere` gives a link that works on any network."
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
    println!(
        "  {}",
        "Scan the code with your phone's camera. Keep this window open; Ctrl-C stops.".dimmed()
    );
    println!();

    let _awake = keep_awake();
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    drop(tunnel);
    Ok(())
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

#[derive(serde::Serialize)]
struct NoteResponse {
    id: String,
    title: String,
    body: String,
    created_at: String,
    updated_at: String,
    tags: Vec<String>,
    directory: String,
    pinned: bool,
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
        }
    }
}

#[derive(serde::Serialize)]
struct TagResponse {
    tag: String,
    count: usize,
}

// ── Handlers ──────────────────────────────────────────────────────────────

async fn list_notes(
    State(state): State<AppState>,
    Query(params): Query<ListParams>,
) -> Json<Vec<NoteResponse>> {
    let store = state.fresh();
    let limit = params.limit.unwrap_or(100);
    let notes = if let Some(ref dir) = params.dir {
        store.list_notes_in_dir(dir, params.tag.as_deref(), limit)
    } else {
        store.list_notes(params.tag.as_deref(), limit)
    };
    Json(notes.iter().map(|n| NoteResponse::from_note(n)).collect())
}

async fn get_note(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<NoteResponse>, StatusCode> {
    let store = state.fresh();
    match store.find_note(&id) {
        Some(n) => Ok(Json(NoteResponse::from_note(n))),
        None => Err(StatusCode::NOT_FOUND),
    }
}

async fn create_note(
    State(state): State<AppState>,
    Json(body): Json<CreateBody>,
) -> Result<impl IntoResponse, StatusCode> {
    let mut store = state.fresh();
    let tags = body.tags.unwrap_or_default();
    let note_body = body.body.unwrap_or_default();
    let dir = body
        .directory
        .unwrap_or_default()
        .trim_matches('/')
        .to_string();
    if !store.dir_exists(&dir) {
        store.create_dir(&dir);
    }
    let resp = match store.create_note(body.title, note_body, tags, &dir) {
        Ok(n) => NoteResponse::from_note(n),
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };
    store
        .save()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((StatusCode::CREATED, Json(resp)))
}

async fn update_note(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateBody>,
) -> Result<Json<NoteResponse>, StatusCode> {
    let mut store = state.fresh();
    let note = store.find_note_mut(&id).ok_or(StatusCode::NOT_FOUND)?;

    if let Some(title) = body.title {
        note.title = title;
    }
    if let Some(new_body) = body.body {
        note.body = new_body;
    }
    if let Some(tags) = body.tags {
        note.tags = tags;
    }
    note.updated_at = chrono::Utc::now();

    let resp = NoteResponse::from_note(note);
    store
        .save()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(resp))
}

async fn delete_note(State(state): State<AppState>, Path(id): Path<String>) -> StatusCode {
    let mut store = state.fresh();
    // Exactly one note: a prefix shared by several must not delete them all.
    let Some(full) = store.find_note(&id).map(|n| n.id.clone()) else {
        return StatusCode::NOT_FOUND;
    };
    store.delete_notes(&[full]);
    match store.save() {
        Ok(()) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn toggle_checkbox(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<ToggleParams>,
) -> Result<Json<NoteResponse>, StatusCode> {
    let mut store = state.fresh();
    store
        .toggle_checkbox(&id, params.checkbox)
        .ok_or(StatusCode::NOT_FOUND)?;
    let note = store.find_note(&id).ok_or(StatusCode::NOT_FOUND)?;
    let resp = NoteResponse::from_note(note);
    store
        .save()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(resp))
}

async fn move_note(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<MoveBody>,
) -> Result<Json<NoteResponse>, StatusCode> {
    let mut store = state.fresh();
    let dir = body.directory.trim_matches('/');
    if !store.dir_exists(dir) {
        return Err(StatusCode::NOT_FOUND);
    }
    store.move_note(&id, dir).ok_or(StatusCode::NOT_FOUND)?;
    let note = store.find_note(&id).ok_or(StatusCode::NOT_FOUND)?;
    let resp = NoteResponse::from_note(note);
    store
        .save()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(resp))
}

async fn search_notes(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Json<Vec<NoteResponse>> {
    let store = state.fresh();
    let q = params.q.unwrap_or_default();
    if q.is_empty() {
        return Json(vec![]);
    }
    let results = store.find(&q);
    Json(results.iter().map(|n| NoteResponse::from_note(n)).collect())
}

async fn list_tags(State(state): State<AppState>) -> Json<Vec<TagResponse>> {
    let store = state.fresh();
    Json(
        store
            .tags()
            .into_iter()
            .map(|(tag, count)| TagResponse { tag, count })
            .collect(),
    )
}

async fn list_dirs(
    State(state): State<AppState>,
    Query(params): Query<DirParams>,
) -> Json<Vec<String>> {
    let store = state.fresh();
    let parent = params.parent.unwrap_or_default();
    Json(store.subdirs(&parent))
}

async fn create_dir(State(state): State<AppState>, Json(body): Json<CreateDirBody>) -> StatusCode {
    let mut store = state.fresh();
    if store.create_dir(&body.path) {
        match store.save() {
            Ok(()) => StatusCode::CREATED,
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    } else {
        StatusCode::CONFLICT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with(notes: &[(&str, &str)]) -> (AppState, tempfile::TempDir, Vec<String>) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::load_from(&dir.path().join("notes")).unwrap();
        let ids = notes
            .iter()
            .map(|(title, d)| store.create_note(*title, "", vec![], d).unwrap().id.clone())
            .collect();
        store.save().unwrap();
        let state = AppState {
            store: Arc::new(Mutex::new(store)),
            token: String::new(),
        };
        (state, dir, ids)
    }

    fn run<F: std::future::Future>(f: F) -> F::Output {
        tokio::runtime::Runtime::new().unwrap().block_on(f)
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
}
