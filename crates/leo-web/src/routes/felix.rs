use std::sync::Arc;

use anyhow::Result;
use axum::extract::{Path, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::routes::notes::{save, NoteResponse};
use crate::routes::uploads::{safe_file_name, ImportFileBody};
use crate::{chat, chat_files, chats, review, store_now, tools, AppState, UploadFile};

fn ndjson(value: serde_json::Value) -> String {
    format!("{value}\n")
}

pub(crate) async fn list_chat_files(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    if !chats::valid_id(&id) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let dir = state.chats.clone();
    match tokio::task::spawn_blocking(move || chat_files::list(&dir, &id)).await {
        Ok(docs) => Json(docs).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub(crate) async fn add_chat_file(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ImportFileBody>,
) -> Response {
    let refuse = |status: StatusCode, message: String| {
        (status, Json(serde_json::json!({ "error": message }))).into_response()
    };
    if !chats::valid_id(&id) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Some(reader) = state.reader.clone() else {
        return refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "leo serve was started without a way to read files.".into(),
        );
    };
    let bytes = {
        use base64::Engine;
        match base64::engine::general_purpose::STANDARD.decode(body.data.as_bytes()) {
            Ok(bytes) => bytes,
            Err(_) => {
                return refuse(
                    StatusCode::BAD_REQUEST,
                    format!("{} did not arrive intact; try again.", body.name),
                )
            }
        }
    };
    let name = safe_file_name(&body.name);
    let dir = state.chats.clone();
    let file = UploadFile {
        name: name.clone(),
        mime: body.mime,
        bytes,
    };
    let done = tokio::task::spawn_blocking(move || -> Result<chat_files::Doc> {
        if chat_files::list(&dir, &id).len() >= chat_files::MOST_FILES {
            anyhow::bail!(
                "a chat holds up to {} documents; remove one first",
                chat_files::MOST_FILES
            );
        }
        let text = reader(file, &mut |_| {})?;
        chat_files::add(&dir, &id, &name, &text, chrono::Utc::now())
    })
    .await;
    match done {
        Ok(Ok(doc)) => (StatusCode::CREATED, Json(doc)).into_response(),
        Ok(Err(e)) => refuse(StatusCode::UNPROCESSABLE_ENTITY, e.to_string()),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub(crate) async fn remove_chat_file(
    State(state): State<AppState>,
    Path((id, doc)): Path<(String, String)>,
) -> StatusCode {
    let dir = state.chats.clone();
    match tokio::task::spawn_blocking(move || chat_files::remove(&dir, &id, &doc)).await {
        Ok(true) => StatusCode::NO_CONTENT,
        _ => StatusCode::NOT_FOUND,
    }
}

pub(crate) async fn get_review(State(state): State<AppState>) -> Response {
    let dir = state.chats.clone();
    match tokio::task::spawn_blocking(move || review::missed(&dir, chrono::Utc::now())).await {
        Ok(missed) => Json(missed).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(serde::Deserialize)]
pub(crate) struct ReviewDone {
    #[serde(default)]
    pub(crate) done: Vec<String>,
}

pub(crate) async fn mark_reviewed(
    State(state): State<AppState>,
    Json(body): Json<ReviewDone>,
) -> Response {
    if body.done.iter().any(|key| !review::valid_key(key)) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let dir = state.chats.clone();
    match tokio::task::spawn_blocking(move || review::mark_reviewed(&dir, &body.done)).await {
        Ok(Ok(added)) => Json(serde_json::json!({ "reviewed": added })).into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub(crate) async fn list_chats(State(state): State<AppState>) -> Response {
    let dir = state.chats.clone();
    let days = state
        .with_store(|store| Ok(leo_core::keep::load(&store.notes_dir).chat_days))
        .await
        .unwrap_or(None);
    match tokio::task::spawn_blocking(move || {
        let now = chrono::Utc::now();
        chats::tidy(&dir, days, now);
        chat_files::tidy_orphans(&dir, now);
        chats::list(&dir)
    })
    .await
    {
        Ok(list) => Json(list).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub(crate) async fn get_chat(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let dir = state.chats.clone();
    match tokio::task::spawn_blocking(move || chats::load(&dir, &id)).await {
        Ok(Some(chat)) => Json(chat).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub(crate) async fn put_chat(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<chats::Saving>,
) -> Response {
    if !chats::valid_id(&id) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let dir = state.chats.clone();
    let writer = state.graphs.writer();
    match tokio::task::spawn_blocking(move || chats::save(&dir, &id, body, chrono::Utc::now()))
        .await
    {
        Ok(Ok(chat)) => {
            if let Some(writer) = writer.filter(|_| chat.wants_name()) {
                name_in_background(state.chats.clone(), &chat, writer);
            }
            Json(chat.summary()).into_response()
        }
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

static NAMING: std::sync::LazyLock<std::sync::Mutex<std::collections::BTreeSet<String>>> =
    std::sync::LazyLock::new(Default::default);

pub(crate) fn name_in_background(
    dir: std::path::PathBuf,
    chat: &chats::Chat,
    writer: crate::graph::Writer,
) {
    let id = chat.id.clone();
    if !NAMING
        .lock()
        .is_ok_and(|mut naming| naming.insert(id.clone()))
    {
        return;
    }
    let (system, user) = chats::name_prompt(chat);
    std::thread::spawn(move || {
        if let Some(name) = writer(&system, &user, 40)
            .ok()
            .and_then(|reply| chats::clean_name(&reply))
        {
            chats::rename(&dir, &id, &name);
        }
        if let Ok(mut naming) = NAMING.lock() {
            naming.remove(&id);
        }
    });
}

pub(crate) async fn delete_chat(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> StatusCode {
    let dir = state.chats.clone();
    match tokio::task::spawn_blocking(move || chats::remove(&dir, &id)).await {
        Ok(true) => StatusCode::NO_CONTENT,
        _ => StatusCode::NOT_FOUND,
    }
}

struct Ask {
    system: String,
    conversation: String,
    sources: Vec<chat::SourceRef>,
    room: usize,
    wanted: String,
}

fn answer(
    state: &AppState,
    streamer: &chat::Streamer,
    ask: Ask,
    tx: &tokio::sync::mpsc::UnboundedSender<String>,
) -> Result<Option<chat::Spent>> {
    let Ask {
        system,
        mut conversation,
        sources,
        room,
        wanted,
    } = ask;
    let send = |value: serde_json::Value| {
        let _ = tx.send(ndjson(value));
    };
    let web = state.web.clone().filter(|w| (w.needed)());
    let with_tools = format!("{system}\n\n{}", tools::manual_with(web.is_some()));
    let last_word = format!("{system}\n\n{}", tools::NO_MORE_TOOLS);
    let mut desk = tools::Desk::new(sources, room).with_web(web);
    let mut nudged = !tools::wants_change(&wanted);
    let mut unstuck = false;
    let mut spent: Option<chat::Spent> = None;
    for step in 0..=tools::MOST_STEPS {
        let last = step == tools::MOST_STEPS;
        let gate = std::cell::RefCell::new(tools::Gate::default());
        let asked = if last {
            conversation.clone()
        } else {
            format!("{conversation}\n\n{}", tools::REMINDER)
        };
        let reply = streamer(
            if last { &last_word } else { &with_tools },
            &asked,
            chat::REPLY_TOKENS,
            &mut |piece| {
                gate.borrow_mut()
                    .push(piece, &mut |t| send(serde_json::json!({ "t": t })))
            },
            &mut || {
                gate.borrow_mut().reset();
                send(serde_json::json!({ "restart": true }));
            },
        )?;
        if let Some(more) = reply.spent {
            let more = chat::Spent { steps: 1, ..more };
            spent = Some(match spent {
                Some(so_far) => so_far.plus(more),
                None => more,
            });
        }
        let reply = reply.text;
        let mut gate = gate.into_inner();
        let call = if last { None } else { tools::find_call(&reply) };
        let call = match call {
            None if !last && !unstuck && tools::claims_no_tools(&reply) => {
                unstuck = true;
                if gate.shown {
                    send(serde_json::json!({ "restart": true }));
                }
                conversation = format!(
                    "{conversation}\n\nFelix replied: {}\n\n{}",
                    tools::without_calls(&reply),
                    tools::UNSTUCK
                );
                continue;
            }
            None if !last && !nudged && desk.proposals == 0 => {
                nudged = true;
                if gate.shown {
                    send(serde_json::json!({ "restart": true }));
                }
                conversation = format!(
                    "{conversation}\n\nFelix replied: {}\n\n{}",
                    tools::without_calls(&reply),
                    tools::NUDGE
                );
                continue;
            }
            None => {
                gate.finish(&mut |t| send(serde_json::json!({ "t": t })));
                return Ok(spent);
            }
            Some(Ok(call)) => call,
            Some(Err(problem)) => tools::Call {
                name: "invalid".into(),
                args: serde_json::json!({ "problem": problem }),
            },
        };
        if gate.shown {
            send(serde_json::json!({ "restart": true }));
        }
        let before = desk.sources.len();
        let done = if call.name == "invalid" {
            tools::Done {
                step: "Tried to use a tool".into(),
                result: format!(
                    "That did not work: {}. Write the call as one line: <tool>{{\"name\": \"search_notes\", \"query\": \"...\"}}</tool>",
                    call.text("problem")
                ),
                proposal: None,
                found: Vec::new(),
            }
        } else if tools::is_web(&call.name) {
            desk.run_web(&call)
        } else {
            let graphs = Arc::clone(&state.graphs);
            store_now(state, |store| {
                let cache = graphs.load();
                Ok(desk.run(store, &cache, &call))
            })
            .map_err(|_| anyhow::anyhow!("leo could not read the notes"))?
        };
        send(serde_json::json!({ "step": done.step, "tool": call.name, "found": done.found }));
        if desk.sources.len() != before {
            send(serde_json::json!({ "sources": desk.sources }));
        }
        if let Some(proposal) = &done.proposal {
            send(serde_json::json!({ "proposal": proposal }));
        }
        conversation = tools::continued(&conversation, &call, &done);
    }
    Ok(spent)
}

#[derive(serde::Deserialize)]
pub(crate) struct Suggestion {
    #[serde(default)]
    pub(crate) find: String,
    pub(crate) replace: String,
}

#[derive(serde::Serialize)]
pub(crate) struct Applied {
    #[serde(flatten)]
    pub(crate) note: NoteResponse,
    pub(crate) before: String,
}

pub(crate) async fn apply_suggestion(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(change): Json<Suggestion>,
) -> Response {
    let applied = state
        .with_store(move |store| {
            let note = store.find_note_mut(&id).ok_or(StatusCode::NOT_FOUND)?;
            let body = if change.find.is_empty() {
                let base = note.body.trim_end();
                if base.is_empty() {
                    change.replace.clone()
                } else {
                    format!("{base}\n\n{}", change.replace.trim_start())
                }
            } else if note.body.matches(change.find.as_str()).count() == 1 {
                note.body.replacen(&change.find, &change.replace, 1)
            } else {
                return Err(StatusCode::CONFLICT);
            };
            let before = std::mem::replace(&mut note.body, body);
            note.updated_at = chrono::Utc::now();
            let applied = Applied {
                note: NoteResponse::from_note(note),
                before,
            };
            save(store)?;
            Ok(applied)
        })
        .await;
    match applied {
        Ok(applied) => Json(applied).into_response(),
        Err(StatusCode::CONFLICT) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": "The note changed since Felix suggested this, so the text it would replace is not there any more." })),
        )
            .into_response(),
        Err(code) => code.into_response(),
    }
}

pub(crate) async fn chat_reply(
    State(state): State<AppState>,
    Json(body): Json<chat::ChatBody>,
) -> Response {
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
    let attached = body.refs.clone();
    let room = state
        .room
        .as_ref()
        .map_or(chat::ROOM, |measure| measure())
        .clamp(chat::LEAST_ROOM, chat::MOST_ROOM);
    let gathered = state
        .with_store(move |store| {
            let cache = graphs.load();
            Ok(chat::gather(
                store,
                &cache,
                note.as_deref(),
                &attached,
                &question,
                room,
            ))
        })
        .await;
    let (sources, notes) = match gathered {
        Ok(found) => found,
        Err(code) => return code.into_response(),
    };
    let documents = match &body.chat {
        Some(chat) if !body.files.is_empty() => chat_files::texts(&state.chats, chat, &body.files),
        _ => Vec::new(),
    };
    let (system, user) = chat::prompt(mode, &notes, &documents, &body.messages);
    let wanted = body
        .messages
        .last()
        .map(|t| t.text.clone())
        .unwrap_or_default();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let _ = tx.send(ndjson(serde_json::json!({ "sources": sources })));
    let worker = state.clone();
    tokio::task::spawn_blocking(move || {
        let ask = Ask {
            system,
            conversation: user,
            sources,
            room,
            wanted,
        };
        let end = match answer(&worker, &streamer, ask, &tx) {
            Ok(spent) => {
                if let Some(spent) = spent {
                    let _ = tx.send(ndjson(serde_json::json!({ "spent": spent })));
                }
                serde_json::json!({ "done": true })
            }
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
