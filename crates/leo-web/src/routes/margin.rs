use std::time::Instant;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::margin::{self, Prefs, Refused, Thread};
use crate::routes::notes::version_of;
use crate::usage::Purpose;
use crate::AppState;

#[derive(Deserialize)]
pub(crate) struct ReadBody {
    version: String,
    blocks: Vec<String>,
}

#[derive(Deserialize)]
pub(crate) struct AskBody {
    version: String,
    quote: String,
    #[serde(default)]
    prefix: String,
    #[serde(default)]
    suffix: String,
    question: String,
}

#[derive(Deserialize)]
pub(crate) struct ReplyBody {
    text: String,
}

#[derive(Deserialize)]
pub(crate) struct StatusBody {
    status: String,
}

#[derive(Deserialize)]
pub(crate) struct ModeBody {
    mode: String,
}

#[derive(Deserialize, Default)]
pub(crate) struct PrefsBody {
    default_mode: Option<String>,
    layout: Option<String>,
    pace: Option<String>,
    cap: Option<u32>,
}

struct ReadFacts {
    along: bool,
    over_cap: bool,
    spoke_soon: bool,
    full: bool,
}

struct Open {
    title: String,
    body: String,
    version: String,
    created: DateTime<Utc>,
}

fn said(code: StatusCode, why: &str, error: &str) -> Response {
    (
        code,
        Json(serde_json::json!({ "why": why, "error": error })),
    )
        .into_response()
}

fn silent() -> Response {
    Json(serde_json::json!({ "note": null })).into_response()
}

async fn open_note(state: &AppState, id: &str) -> Result<Open, Box<Response>> {
    if !margin::valid_note(id) {
        return Err(Box::new(StatusCode::BAD_REQUEST.into_response()));
    }
    let id = id.to_string();
    state
        .read_store(move |store| {
            let note = store
                .notes
                .iter()
                .find(|n| n.id == id)
                .ok_or(StatusCode::NOT_FOUND)?;
            Ok(Open {
                title: note.title.clone(),
                body: note.body.clone(),
                version: version_of(note),
                created: note.created_at,
            })
        })
        .await
        .map_err(|code| Box::new(code.into_response()))
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, Box<Response>> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| Box::new(StatusCode::INTERNAL_SERVER_ERROR.into_response()))
}

pub(crate) async fn note_margin(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let note = match open_note(&state, &id).await {
        Ok(note) => note,
        Err(refused) => return *refused,
    };
    let chats = state.chats.clone();
    let view = blocking(move || {
        serde_json::json!({
            "mode": margin::mode_of(&chats, &id, note.created),
            "threads": margin::open_on(&chats, &id),
        })
    })
    .await;
    match view {
        Ok(view) => Json(view).into_response(),
        Err(refused) => *refused,
    }
}

pub(crate) async fn set_note_mode(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ModeBody>,
) -> Response {
    if let Err(refused) = open_note(&state, &id).await {
        return *refused;
    }
    if !margin::valid_mode(&body.mode) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let chats = state.chats.clone();
    let mode = body.mode.clone();
    match blocking(move || margin::set_mode(&chats, &id, &mode)).await {
        Ok(true) => Json(serde_json::json!({ "mode": body.mode })).into_response(),
        Ok(false) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        Err(refused) => *refused,
    }
}

fn settings_view(state: &AppState) -> serde_json::Value {
    let prefs = margin::prefs(&state.chats);
    serde_json::json!({
        "default_mode": prefs.default_mode,
        "layout": prefs.layout,
        "pace": prefs.pace,
        "cap": prefs.cap,
        "caps": margin::DAILY_CAPS,
        "today": margin::reads_today(&state.chats),
        "problem": state.margin.problem(),
    })
}

pub(crate) async fn settings(State(state): State<AppState>) -> Response {
    let view = state.clone();
    match blocking(move || settings_view(&view)).await {
        Ok(view) => Json(view).into_response(),
        Err(refused) => *refused,
    }
}

fn changed(mut prefs: Prefs, body: PrefsBody) -> Option<Prefs> {
    if let Some(mode) = body.default_mode {
        if !margin::valid_mode(&mode) {
            return None;
        }
        if mode != prefs.default_mode {
            prefs.since = Some(Utc::now());
        }
        prefs.default_mode = mode;
    }
    if let Some(layout) = body.layout {
        if !matches!(layout.as_str(), "both" | "right" | "marks") {
            return None;
        }
        prefs.layout = layout;
    }
    if let Some(pace) = body.pace {
        if !matches!(pace.as_str(), "regular" | "slow") {
            return None;
        }
        prefs.pace = pace;
    }
    if let Some(cap) = body.cap {
        if !margin::DAILY_CAPS.contains(&cap) {
            return None;
        }
        prefs.cap = cap;
    }
    Some(prefs)
}

pub(crate) async fn change_settings(
    State(state): State<AppState>,
    Json(body): Json<PrefsBody>,
) -> Response {
    let view = state.clone();
    let done = blocking(move || {
        let next = changed(margin::prefs(&view.chats), body)?;
        margin::set_prefs(&view.chats, &next).then(|| settings_view(&view))
    })
    .await;
    match done {
        Ok(Some(view)) => Json(view).into_response(),
        Ok(None) => StatusCode::BAD_REQUEST.into_response(),
        Err(refused) => *refused,
    }
}

fn wanted_blocks(body: &ReadBody) -> Result<Vec<String>, Box<Response>> {
    let too_much = body.blocks.len() > margin::MOST_BLOCKS
        || body
            .blocks
            .iter()
            .any(|block| block.chars().count() > margin::MOST_BLOCK_CHARS)
        || body.blocks.iter().map(|b| b.chars().count()).sum::<usize>() > margin::MOST_READ_CHARS;
    if too_much {
        return Err(Box::new(StatusCode::PAYLOAD_TOO_LARGE.into_response()));
    }
    let blocks: Vec<String> = body
        .blocks
        .iter()
        .map(|block| block.trim().to_string())
        .filter(|block| !block.is_empty())
        .collect();
    if blocks.is_empty() {
        return Err(Box::new(StatusCode::BAD_REQUEST.into_response()));
    }
    Ok(blocks)
}

pub(crate) async fn read(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ReadBody>,
) -> Response {
    if !margin::valid_version(&body.version) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let blocks = match wanted_blocks(&body) {
        Ok(blocks) => blocks,
        Err(refused) => return *refused,
    };
    let note = match open_note(&state, &id).await {
        Ok(note) => note,
        Err(refused) => return *refused,
    };
    if note.version != body.version {
        return StatusCode::CONFLICT.into_response();
    }
    if !blocks
        .iter()
        .all(|block| note.body.contains(block.as_str()))
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let chats = state.chats.clone();
    let (facts, chats) = {
        let id = id.clone();
        let created = note.created;
        let held = chats.clone();
        let facts = blocking(move || ReadFacts {
            along: margin::mode_of(&held, &id, created) == "along",
            over_cap: margin::reads_today(&held) >= margin::prefs(&held).cap,
            spoke_soon: margin::last_read_note(&held, &id).is_some_and(|at| {
                Utc::now() - at < chrono::Duration::minutes(margin::NOTE_GAP_MINUTES)
            }),
            full: margin::open_on(&held, &id).len() >= margin::MOST_OPEN,
        })
        .await;
        match facts {
            Ok(facts) => (facts, chats),
            Err(refused) => return *refused,
        }
    };
    if !facts.along {
        return said(
            StatusCode::FORBIDDEN,
            "mode",
            "Felix is not reading along in this note.",
        );
    }
    if !state.margin.was_edited(&id, &note.version) {
        return said(
            StatusCode::FORBIDDEN,
            "unedited",
            "Felix reads only what you changed.",
        );
    }
    if state.steering.any_open() {
        return said(
            StatusCode::TOO_MANY_REQUESTS,
            "felix",
            "Felix is answering you.",
        );
    }
    if facts.over_cap {
        return said(
            StatusCode::TOO_MANY_REQUESTS,
            "daily",
            "Felix has read enough for today.",
        );
    }
    if facts.spoke_soon {
        return said(
            StatusCode::TOO_MANY_REQUESTS,
            "soon",
            "Felix spoke here a moment ago.",
        );
    }
    if facts.full {
        return silent();
    }
    let Some(writer) = state.toolless.clone() else {
        state.margin.set_problem(Some(margin::PROBLEM.into()));
        return silent();
    };
    let reading = match state.margin.begin(&id) {
        Ok(reading) => reading,
        Err(Refused::Busy) => {
            return said(
                StatusCode::TOO_MANY_REQUESTS,
                "busy",
                "Felix is already reading.",
            )
        }
        Err(Refused::Soon) => {
            return said(
                StatusCode::TOO_MANY_REQUESTS,
                "soon",
                "Felix just read this note.",
            )
        }
    };
    let blocks = state.margin.unseen(&id, blocks);
    if blocks.is_empty() {
        return silent();
    }
    let counted = chats.clone();
    if let Err(refused) = blocking(move || margin::count_read(&counted)).await {
        return *refused;
    }
    let user = margin::read_prompt(&note.title, &blocks);
    let answer = tokio::time::timeout(
        margin::READ_LIMIT,
        tokio::task::spawn_blocking(move || {
            let _reading = reading;
            writer(margin::READ_SYSTEM, &user, margin::READ_TOKENS)
        }),
    )
    .await;
    let reply = match answer {
        Ok(Ok(Ok(reply))) => reply,
        _ => {
            state.margin.set_problem(Some(margin::PROBLEM.into()));
            return silent();
        }
    };
    state.margin.set_problem(None);
    let Some(found) = margin::parsed(&reply) else {
        return silent();
    };
    let now = match open_note(&state, &id).await {
        Ok(now) => now,
        Err(_) => return silent(),
    };
    let Some(place) = margin::placed(&now.body, &found.quote, &blocks) else {
        return silent();
    };
    let thread = margin::new_thread(
        &id,
        "read",
        place,
        &found.quote,
        vec![margin::felix_says(&found.text)],
    );
    let saved = thread.clone();
    match blocking(move || margin::put(&chats, &saved)).await {
        Ok(true) => Json(serde_json::json!({ "note": thread })).into_response(),
        _ => silent(),
    }
}

async fn answered(
    state: &AppState,
    note: &Open,
    thread: &Thread,
    question: &str,
) -> Result<String, Box<Response>> {
    let Some(streamer) = state.chat.clone() else {
        return Err(Box::new(said(
            StatusCode::SERVICE_UNAVAILABLE,
            "no-ai",
            "Choose a writing AI in Settings, then ask again.",
        )));
    };
    let Some(foreground) = state.work.foreground() else {
        return Err(Box::new(said(
            StatusCode::SERVICE_UNAVAILABLE,
            "busy",
            "Felix is working on several answers. Try again in a moment.",
        )));
    };
    let user = margin::reply_prompt(&note.title, &note.body, thread, question);
    let started = Instant::now();
    let reply = blocking(move || {
        let _foreground = foreground;
        streamer(
            margin::REPLY_SYSTEM,
            &user,
            margin::REPLY_TOKENS,
            &mut |_| {},
            &mut || {},
        )
    })
    .await?;
    let failed = reply.is_err();
    let spent = reply.as_ref().ok().and_then(|r| r.spent.clone());
    state.usage.record(
        Purpose::OtherWriting,
        spent.as_ref(),
        started.elapsed(),
        failed,
    );
    let text = reply.map(|r| r.text).unwrap_or_default();
    let text = text.trim();
    if text.is_empty() {
        return Err(Box::new(said(
            StatusCode::BAD_GATEWAY,
            "no-answer",
            "Felix could not answer just now. Try again.",
        )));
    }
    Ok(text.chars().take(margin::MOST_REPLY).collect())
}

pub(crate) async fn ask(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<AskBody>,
) -> Response {
    if !margin::valid_version(&body.version) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let question = body.question.trim().to_string();
    if question.chars().count() > margin::MOST_ASK
        || body.quote.chars().count() > margin::MOST_QUOTE
        || body.prefix.chars().count() > margin::MOST_QUOTE
        || body.suffix.chars().count() > margin::MOST_QUOTE
    {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    }
    if question.is_empty() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let note = match open_note(&state, &id).await {
        Ok(note) => note,
        Err(refused) => return *refused,
    };
    if note.version != body.version {
        return StatusCode::CONFLICT.into_response();
    }
    let exact = format!("{}{}{}", body.prefix, body.quote, body.suffix);
    let Some(place) = margin::placed(&note.body, &body.quote, &[exact]) else {
        return said(
            StatusCode::CONFLICT,
            "lost",
            "Those words are not in the note any more.",
        );
    };
    let chats = state.chats.clone();
    let held = id.clone();
    let open = match blocking(move || margin::open_on(&chats, &held).len()).await {
        Ok(open) => open,
        Err(refused) => return *refused,
    };
    if open >= margin::MOST_OPEN {
        return said(
            StatusCode::CONFLICT,
            "full",
            "This note has many open margin notes. Resolve one, then ask again.",
        );
    }
    let mut thread = margin::new_thread(&id, "ask", place, &body.quote, Vec::new());
    let text = match answered(&state, &note, &thread, &question).await {
        Ok(text) => text,
        Err(refused) => return *refused,
    };
    thread.messages = vec![margin::you_say(&question), margin::felix_says(&text)];
    saved_thread(&state, thread).await
}

pub(crate) async fn reply(
    State(state): State<AppState>,
    Path((id, thread)): Path<(String, String)>,
    Json(body): Json<ReplyBody>,
) -> Response {
    if !margin::valid_note(&thread) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let text = body.text.trim().to_string();
    if text.chars().count() > margin::MOST_ASK {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    }
    if text.is_empty() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let note = match open_note(&state, &id).await {
        Ok(note) => note,
        Err(refused) => return *refused,
    };
    let chats = state.chats.clone();
    let (held, wanted) = (id.clone(), thread.clone());
    let found = blocking(move || margin::one(&chats, &held, &wanted)).await;
    let mut found = match found {
        Ok(Some(found)) => found,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(refused) => return *refused,
    };
    if found.status != "open" {
        return StatusCode::CONFLICT.into_response();
    }
    if found.messages.len() + 2 > margin::MOST_MESSAGES {
        return said(
            StatusCode::CONFLICT,
            "long",
            "This thread is full. Open it in chat to go on.",
        );
    }
    let Some(_replying) = state.margin.replying(&thread) else {
        return said(
            StatusCode::CONFLICT,
            "busy",
            "Felix is already answering this.",
        );
    };
    let answer = match answered(&state, &note, &found, &text).await {
        Ok(answer) => answer,
        Err(refused) => return *refused,
    };
    found.messages.push(margin::you_say(&text));
    found.messages.push(margin::felix_says(&answer));
    found.updated_at = Utc::now();
    saved_thread(&state, found).await
}

async fn saved_thread(state: &AppState, thread: Thread) -> Response {
    let chats = state.chats.clone();
    let saved = thread.clone();
    match blocking(move || margin::put(&chats, &saved)).await {
        Ok(true) => Json(thread).into_response(),
        Ok(false) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        Err(refused) => *refused,
    }
}

pub(crate) async fn set_status(
    State(state): State<AppState>,
    Path((id, thread)): Path<(String, String)>,
    Json(body): Json<StatusBody>,
) -> Response {
    if !margin::valid_note(&id) || !margin::valid_note(&thread) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    if !matches!(body.status.as_str(), "open" | "resolved" | "dismissed") {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let chats = state.chats.clone();
    let done = blocking(move || {
        let mut found = margin::one(&chats, &id, &thread)?;
        found.status = body.status;
        found.updated_at = Utc::now();
        margin::put(&chats, &found).then_some(found)
    })
    .await;
    match done {
        Ok(Some(found)) => Json(found).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(refused) => *refused,
    }
}
