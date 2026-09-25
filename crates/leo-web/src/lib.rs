use std::sync::{Arc, Mutex};

use anyhow::Result;
use axum::{
    extract::{Path, Query, Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use colored::Colorize;
use serde::Deserialize;
use tower_http::cors::CorsLayer;

use leo_core::store::Store;

#[derive(Clone)]
struct AppState {
    store: Arc<Mutex<Store>>,
    token: String,
}

const HTML: &str = include_str!("web_ui.html");

pub async fn serve(port: u16) -> Result<()> {
    let store = Store::load()?;

    // Generate a random auth token
    let token = uuid::Uuid::new_v4().simple().to_string();

    // Determine local IP
    let local_ip = local_ip_address::local_ip()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|_| "127.0.0.1".to_string());

    let state = AppState {
        store: Arc::new(Mutex::new(store)),
        token: token.clone(),
    };

    let app = Router::new()
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
        .layer(CorsLayer::permissive())
        .with_state(state);

    let network_url = format!("http://{local_ip}:{port}/?token={token}");
    let local_url = format!("http://localhost:{port}/?token={token}");

    println!("\n  {} {}", "Local:".bold(), local_url.cyan().underline());
    println!("  {} {}\n", "Network:".bold(), network_url.cyan().underline());

    // Print QR code for easy phone scanning
    if let Ok(code) = qrcode::QrCode::new(&network_url) {
        use qrcode::render::unicode::Dense1x2;
        let qr = code
            .render::<Dense1x2>()
            .dark_color(Dense1x2::Light)
            .light_color(Dense1x2::Dark)
            .build();
        println!("{qr}\n");
    }

    println!(
        "  {} If Safari blocks HTTP, go to {} and turn off {}",
        "iPhone:".bold(),
        "Settings > Apps > Safari".bold(),
        "HTTPS Upgrade".bold()
    );
    println!("  {}", "The token in the URL prevents unauthorized access.".dimmed());
    println!();
    println!("  {}\n", "Press Ctrl+C to stop".dimmed());

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn index() -> Html<&'static str> {
    Html(HTML)
}

// ── Auth middleware ───────────────────────────────────────────────────────

async fn auth_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Check ?token= query param
    let query = request.uri().query().unwrap_or("");
    if let Some(t) = query_param(query, "token") {
        if t == state.token {
            let mut response = next.run(request).await;
            // Set cookie so subsequent requests don't need ?token=
            let cookie = format!(
                "leo_token={}; Path=/; HttpOnly; SameSite=Lax; Max-Age=86400",
                state.token
            );
            response
                .headers_mut()
                .insert(header::SET_COOKIE, cookie.parse().unwrap());
            return Ok(response);
        }
    }

    // Check cookie
    if let Some(cookie_header) = request.headers().get(header::COOKIE) {
        if let Ok(cookies) = cookie_header.to_str() {
            for part in cookies.split(';') {
                if let Some(val) = part.trim().strip_prefix("leo_token=") {
                    if val == state.token {
                        return Ok(next.run(request).await);
                    }
                }
            }
        }
    }

    Err(StatusCode::UNAUTHORIZED)
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
    let store = state.store.lock().unwrap();
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
    let store = state.store.lock().unwrap();
    match store.find_note(&id) {
        Some(n) => Ok(Json(NoteResponse::from_note(n))),
        None => Err(StatusCode::NOT_FOUND),
    }
}

async fn create_note(
    State(state): State<AppState>,
    Json(body): Json<CreateBody>,
) -> Result<impl IntoResponse, StatusCode> {
    let mut store = state.store.lock().unwrap();
    let tags = body.tags.unwrap_or_default();
    let note_body = body.body.unwrap_or_default();
    let dir = body.directory.unwrap_or_default().trim_matches('/').to_string();
    if !store.dir_exists(&dir) {
        store.create_dir(&dir);
    }
    let resp = match store.create_note(body.title, note_body, tags, &dir) {
        Ok(n) => NoteResponse::from_note(n),
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };
    store.save().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((StatusCode::CREATED, Json(resp)))
}

async fn update_note(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateBody>,
) -> Result<Json<NoteResponse>, StatusCode> {
    let mut store = state.store.lock().unwrap();
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
    store.save().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(resp))
}

async fn delete_note(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> StatusCode {
    let mut store = state.store.lock().unwrap();
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
    let mut store = state.store.lock().unwrap();
    store
        .toggle_checkbox(&id, params.checkbox)
        .ok_or(StatusCode::NOT_FOUND)?;
    let note = store.find_note(&id).ok_or(StatusCode::NOT_FOUND)?;
    let resp = NoteResponse::from_note(note);
    store.save().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(resp))
}

async fn move_note(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<MoveBody>,
) -> Result<Json<NoteResponse>, StatusCode> {
    let mut store = state.store.lock().unwrap();
    let dir = body.directory.trim_matches('/');
    if !store.dir_exists(dir) {
        return Err(StatusCode::NOT_FOUND);
    }
    store.move_note(&id, dir).ok_or(StatusCode::NOT_FOUND)?;
    let note = store.find_note(&id).ok_or(StatusCode::NOT_FOUND)?;
    let resp = NoteResponse::from_note(note);
    store.save().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(resp))
}

async fn search_notes(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Json<Vec<NoteResponse>> {
    let store = state.store.lock().unwrap();
    let q = params.q.unwrap_or_default();
    if q.is_empty() {
        return Json(vec![]);
    }
    let results = store.find(&q);
    Json(results.iter().map(|n| NoteResponse::from_note(n)).collect())
}

async fn list_tags(State(state): State<AppState>) -> Json<Vec<TagResponse>> {
    let store = state.store.lock().unwrap();
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
    let store = state.store.lock().unwrap();
    let parent = params.parent.unwrap_or_default();
    Json(store.subdirs(&parent))
}

async fn create_dir(
    State(state): State<AppState>,
    Json(body): Json<CreateDirBody>,
) -> StatusCode {
    let mut store = state.store.lock().unwrap();
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
        let state = AppState { store: Arc::new(Mutex::new(store)), token: String::new() };
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
            let mut store = state.store.lock().unwrap();
            let first = store.notes[0].id.clone();
            store.notes[1].id = format!("{first}x");
            first
        };
        let status = run(delete_note(State(state.clone()), Path(prefix)));
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(state.store.lock().unwrap().notes.len(), 2);
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
        run(create_note(State(state.clone()), Json(body))).ok().unwrap();
        assert!(state.store.lock().unwrap().dir_exists("cs162"));
    }

    #[test]
    fn moving_to_a_missing_directory_is_refused() {
        let (state, _d, ids) = state_with(&[("A", "")]);
        let body = MoveBody { directory: "nowhere".to_string() };
        let out = run(move_note(State(state.clone()), Path(ids[0].clone()), Json(body)));
        assert_eq!(out.err(), Some(StatusCode::NOT_FOUND));
        assert_eq!(state.store.lock().unwrap().find_note(&ids[0]).unwrap().directory, "");
    }
}
