use std::sync::Arc;

use anyhow::Result;
use axum::extract::{Path, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::routes::uploads::{safe_file_name, ImportFileBody};
use crate::{chat, chat_files, chats, review, AppState, UploadFile};

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
    match tokio::task::spawn_blocking(move || chats::save(&dir, &id, body, chrono::Utc::now()))
        .await
    {
        Ok(Ok(chat)) => Json(chats::Summary {
            id: chat.id,
            title: chat.title,
            mode: chat.mode,
            count: chat.messages.len(),
            updated_at: chat.updated_at,
        })
        .into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
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
    let gathered = state
        .with_store(move |store| {
            let cache = graphs.load();
            Ok(chat::gather(
                store,
                &cache,
                note.as_deref(),
                &attached,
                &question,
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
