use std::time::{Duration, Instant};

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
    #[serde(default)]
    most: Option<usize>,
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
    gap: Option<u32>,
    auto_model: Option<bool>,
}

struct ReadFacts {
    along: bool,
    cap: u32,
    gap: u32,
    quotes: Vec<String>,
}

enum Pass {
    Saved(Vec<Thread>),
    Silent,
    Failed,
    Refused(Response),
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

fn failed(state: &AppState) -> Response {
    state.margin.set_problem(Some(margin::PROBLEM.into()));
    Json(serde_json::json!({ "note": null, "failed": true })).into_response()
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
        "gap": prefs.gap,
        "gaps": margin::READ_GAPS,
        "auto_model": prefs.auto_model,
        "today": margin::reads_today(&state.chats, prefs.cap),
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
        if !matches!(pace.as_str(), "fast" | "regular" | "slow") {
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
    if let Some(gap) = body.gap {
        if !margin::READ_GAPS.contains(&gap) {
            return None;
        }
        prefs.gap = gap;
    }
    if let Some(auto_model) = body.auto_model {
        prefs.auto_model = auto_model;
    }
    Some(prefs)
}

pub(crate) async fn change_settings(
    State(state): State<AppState>,
    Json(body): Json<PrefsBody>,
) -> Response {
    let held = state.chats.clone();
    let before = match blocking(move || margin::prefs(&held)).await {
        Ok(before) => before,
        Err(refused) => return *refused,
    };
    let flips = body
        .default_mode
        .as_deref()
        .is_some_and(|mode| margin::valid_mode(mode) && mode != before.default_mode);
    if let (true, "along", Some(since)) = (flips, before.default_mode.as_str(), before.since) {
        let notes = state
            .read_store(move |store| {
                Ok(store
                    .notes
                    .iter()
                    .filter(|n| n.created_at >= since && margin::made_by_leo(&n.id))
                    .map(|n| n.id.clone())
                    .collect::<Vec<_>>())
            })
            .await;
        let held = state.chats.clone();
        let pinned = match notes {
            Ok(notes) => blocking(move || margin::pin_modes(&held, &notes, "along")).await,
            Err(code) => return code.into_response(),
        };
        match pinned {
            Ok(true) => {}
            Ok(false) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            Err(refused) => return *refused,
        }
    }
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

async fn pass(state: &AppState, id: String, body: ReadBody, through: bool) -> Pass {
    if !margin::valid_version(&body.version) {
        return Pass::Refused(StatusCode::BAD_REQUEST.into_response());
    }
    let blocks = match wanted_blocks(&body) {
        Ok(blocks) => blocks,
        Err(refused) => return Pass::Refused(*refused),
    };
    let note = match open_note(state, &id).await {
        Ok(note) => note,
        Err(refused) => return Pass::Refused(*refused),
    };
    if note.version != body.version {
        return Pass::Refused(StatusCode::CONFLICT.into_response());
    }
    if !blocks
        .iter()
        .all(|block| note.body.contains(block.as_str()))
    {
        return Pass::Refused(StatusCode::BAD_REQUEST.into_response());
    }
    let chats = state.chats.clone();
    let (facts, chats) = {
        let id = id.clone();
        let created = note.created;
        let held = chats.clone();
        let facts = blocking(move || {
            let prefs = margin::prefs(&held);
            ReadFacts {
                along: margin::mode_of(&held, &id, created) == "along",
                cap: prefs.cap,
                gap: prefs.gap,
                quotes: margin::open_on(&held, &id)
                    .into_iter()
                    .map(|thread| thread.quote)
                    .collect(),
            }
        })
        .await;
        match facts {
            Ok(facts) => (facts, chats),
            Err(refused) => return Pass::Refused(*refused),
        }
    };
    let full = facts.quotes.len() >= margin::MOST_OPEN;
    if !through {
        if !facts.along {
            return Pass::Refused(said(
                StatusCode::FORBIDDEN,
                "mode",
                "Felix is not reading along in this note.",
            ));
        }
        if !state.margin.was_edited(&id, &note.version) {
            return Pass::Refused(said(
                StatusCode::FORBIDDEN,
                "unedited",
                "Felix reads only what you changed.",
            ));
        }
    }
    if state.steering.any_open() {
        return Pass::Refused(said(
            StatusCode::TOO_MANY_REQUESTS,
            "felix",
            "Felix is answering you.",
        ));
    }
    if full {
        if !through {
            return Pass::Silent;
        }
        return Pass::Refused(said(
            StatusCode::CONFLICT,
            "full",
            "This note has many open margin notes. Resolve some, then read through again.",
        ));
    }
    let Some(writer) = state.toolless.clone() else {
        if through {
            state.margin.set_problem(Some(margin::PROBLEM.into()));
            return Pass::Refused(said(
                StatusCode::SERVICE_UNAVAILABLE,
                "no-ai",
                "Choose a writing AI in Settings, then read through again.",
            ));
        }
        return Pass::Failed;
    };
    let gap = if through {
        Duration::ZERO
    } else {
        Duration::from_secs(facts.gap.into())
    };
    let reading = match state.margin.begin(&id, gap) {
        Ok(reading) => reading,
        Err(Refused::Busy) => {
            return Pass::Refused(said(
                StatusCode::TOO_MANY_REQUESTS,
                "busy",
                "Felix is already reading.",
            ))
        }
        Err(Refused::Soon) => {
            return Pass::Refused(said(
                StatusCode::TOO_MANY_REQUESTS,
                "soon",
                "Felix just read this note.",
            ))
        }
    };
    let blocks = if through {
        blocks
    } else {
        let blocks = state.margin.unseen(&id, blocks);
        if blocks.is_empty() {
            return Pass::Silent;
        }
        let counted = chats.clone();
        let cap = facts.cap;
        if !matches!(
            blocking(move || margin::spend_read(&counted, cap)).await,
            Ok(true)
        ) {
            state.margin.forget_blocks(&id, &blocks);
            return Pass::Refused(said(
                StatusCode::TOO_MANY_REQUESTS,
                "daily",
                "Felix has read enough for today.",
            ));
        }
        blocks
    };
    let (system, tag, tokens) = if through {
        (margin::THROUGH_SYSTEM, "note_part", margin::THROUGH_TOKENS)
    } else {
        (margin::READ_SYSTEM, "changed_text", margin::READ_TOKENS)
    };
    let user = margin::read_prompt(&note.title, &blocks, tag);
    let answer = tokio::time::timeout(
        margin::READ_LIMIT,
        tokio::task::spawn_blocking(move || {
            let _reading = reading;
            writer(system, &user, tokens)
        }),
    )
    .await;
    let forget = |state: &AppState| {
        if !through {
            state.margin.forget_blocks(&id, &blocks);
        }
    };
    let reply = match answer {
        Ok(Ok(Ok(reply))) => reply,
        _ => {
            forget(state);
            return Pass::Failed;
        }
    };
    let most = body
        .most
        .unwrap_or(margin::MOST_PER_PASS)
        .clamp(1, margin::MOST_PER_PASS)
        .min(margin::MOST_OPEN - facts.quotes.len());
    let founds = if through {
        margin::parsed_many(&reply, most)
    } else {
        match margin::parsed(&reply) {
            margin::ReadAnswer::Silent => Some(Vec::new()),
            margin::ReadAnswer::Note(found) => Some(vec![found]),
            margin::ReadAnswer::Invalid => None,
        }
    };
    let Some(founds) = founds else {
        forget(state);
        return Pass::Failed;
    };
    state.margin.set_problem(None);
    let now = match open_note(state, &id).await {
        Ok(now) => now,
        Err(_) => return Pass::Silent,
    };
    let mut quotes = facts.quotes;
    let mut saved = Vec::new();
    for found in founds {
        if through
            && quotes
                .iter()
                .any(|q| q.contains(&found.quote) || found.quote.contains(q.as_str()))
        {
            continue;
        }
        let Some(place) = margin::placed(&now.body, &found.quote, &blocks) else {
            continue;
        };
        let thread = margin::new_thread(
            &id,
            "read",
            place,
            &found.quote,
            vec![margin::felix_says(&found.text)],
        );
        let held = chats.clone();
        let stored = thread.clone();
        if !matches!(
            blocking(move || margin::put(&held, &stored)).await,
            Ok(true)
        ) {
            break;
        }
        quotes.push(found.quote);
        saved.push(thread);
    }
    if saved.is_empty() {
        Pass::Silent
    } else {
        Pass::Saved(saved)
    }
}

pub(crate) async fn read(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ReadBody>,
) -> Response {
    match pass(&state, id, body, false).await {
        Pass::Saved(mut saved) => {
            Json(serde_json::json!({ "note": saved.remove(0) })).into_response()
        }
        Pass::Silent => silent(),
        Pass::Failed => failed(&state),
        Pass::Refused(refused) => refused,
    }
}

pub(crate) async fn analyze(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ReadBody>,
) -> Response {
    match pass(&state, id, body, true).await {
        Pass::Saved(saved) => Json(serde_json::json!({ "notes": saved })).into_response(),
        Pass::Silent => Json(serde_json::json!({ "notes": [] })).into_response(),
        Pass::Failed => {
            state.margin.set_problem(Some(margin::PROBLEM.into()));
            Json(serde_json::json!({ "notes": [], "failed": true })).into_response()
        }
        Pass::Refused(refused) => refused,
    }
}

pub(crate) async fn forget(
    State(state): State<AppState>,
    Path((id, thread)): Path<(String, String)>,
) -> Response {
    if !margin::valid_note(&id) || !margin::valid_note(&thread) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let chats = state.chats.clone();
    let (held, wanted) = (id, thread.clone());
    let found = blocking(move || margin::one(&chats, &held, &wanted)).await;
    let found = match found {
        Ok(Some(found)) => found,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(refused) => return *refused,
    };
    let Some(_replying) = state.margin.replying(&thread) else {
        return said(StatusCode::CONFLICT, "replying", "Felix is answering this.");
    };
    if found.messages.iter().any(|m| m.from != "felix") {
        return said(
            StatusCode::CONFLICT,
            "replied",
            "You replied to this, so it stays.",
        );
    }
    let chats = state.chats.clone();
    match blocking(move || margin::remove(&chats, &thread)).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(refused) => *refused,
    }
}

async fn answered(
    state: &AppState,
    note: &Open,
    thread: &Thread,
    question: &str,
) -> Result<String, Box<Response>> {
    let Some(streamer) = state.toolless_chat.clone() else {
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
    let usage = std::sync::Arc::clone(&state.usage);
    let reply = blocking(move || {
        let _foreground = foreground;
        let reply = streamer(
            margin::REPLY_SYSTEM,
            &user,
            margin::REPLY_TOKENS,
            &mut |_| {},
            &mut || {},
            &|| false,
        );
        let spent = reply.as_ref().ok().and_then(|r| r.spent.clone());
        usage.record(
            Purpose::OtherWriting,
            spent.as_ref(),
            started.elapsed(),
            reply.is_err(),
        );
        reply
    })
    .await?;
    let text = reply.map(|r| r.text).unwrap_or_default();
    let text: String = text.trim().chars().take(margin::MOST_REPLY).collect();
    let text = margin::without_open_fence(&text);
    if text.is_empty() {
        return Err(Box::new(said(
            StatusCode::BAD_GATEWAY,
            "no-answer",
            "Felix could not answer just now. Try again.",
        )));
    }
    Ok(text.to_string())
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
        || body.quote.chars().count() > margin::MOST_ASKED
        || body.prefix.chars().count() > margin::MOST_QUOTE
        || body.suffix.chars().count() > margin::MOST_QUOTE
    {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    }
    if body.quote.trim().chars().count() < margin::LEAST_QUOTE {
        return said(
            StatusCode::BAD_REQUEST,
            "short",
            "Select a few more words, then ask.",
        );
    }
    let note = match open_note(&state, &id).await {
        Ok(note) => note,
        Err(refused) => return *refused,
    };
    if note.version != body.version {
        return StatusCode::CONFLICT.into_response();
    }
    let Some(place) = margin::placed_exact(&note.body, &body.prefix, &body.quote, &body.suffix)
    else {
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
    let asked = if question.is_empty() {
        margin::UNSPOKEN
    } else {
        &question
    };
    let text = match answered(&state, &note, &thread, asked).await {
        Ok(text) => text,
        Err(refused) => return *refused,
    };
    thread.messages = vec![margin::felix_says(&text)];
    if !question.is_empty() {
        thread.messages.insert(0, margin::you_say(&question));
    }
    let chats = state.chats.clone();
    let saved = thread.clone();
    match blocking(move || margin::put(&chats, &saved)).await {
        Ok(true) => Json(thread).into_response(),
        Ok(false) => said(
            StatusCode::CONFLICT,
            "full",
            "This note has many open margin notes. Resolve one, then ask again.",
        ),
        Err(refused) => *refused,
    }
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
    let chats = state.chats.clone();
    let saved = found.clone();
    match blocking(move || margin::save_messages(&chats, &saved)).await {
        Ok(true) => Json(found).into_response(),
        Ok(false) => said(
            StatusCode::CONFLICT,
            "closed",
            "This note was closed while Felix answered.",
        ),
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
    let done = blocking(move || margin::set_status(&chats, &id, &thread, &body.status)).await;
    match done {
        Ok(margin::Status::Done(found)) => Json(found).into_response(),
        Ok(margin::Status::Missing) => StatusCode::NOT_FOUND.into_response(),
        Ok(margin::Status::Full) => said(
            StatusCode::CONFLICT,
            "full",
            "This note has many open margin notes. Resolve one first.",
        ),
        Err(refused) => *refused,
    }
}
