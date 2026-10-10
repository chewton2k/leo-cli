use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;

use super::notes::{save, version_of, NoteResponse};
use crate::{combine, AppState};

fn error(code: StatusCode, message: &str) -> Response {
    (code, Json(serde_json::json!({ "error": message }))).into_response()
}

#[derive(Deserialize)]
pub(crate) struct Preview {
    with: String,
}

pub(crate) async fn preview(
    State(state): State<AppState>,
    Path(into): Path<String>,
    Json(body): Json<Preview>,
) -> Response {
    let Some(writer) = state.graphs.writer() else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Combining notes needs a writing AI. Choose one in Settings.",
        );
    };
    let found = state
        .with_store(move |store| {
            let a = store.find_note(&into).ok_or(StatusCode::NOT_FOUND)?.clone();
            let b = store
                .find_note(&body.with)
                .ok_or(StatusCode::NOT_FOUND)?
                .clone();
            if a.id == b.id {
                return Err(StatusCode::BAD_REQUEST);
            }
            Ok((a, b, store.notes_dir.clone()))
        })
        .await;
    let (into, from, notes_dir) = match found {
        Ok(found) => found,
        Err(StatusCode::BAD_REQUEST) => {
            return error(StatusCode::BAD_REQUEST, "Drop a note on a different note.")
        }
        Err(code) => return error(code, "One of those notes is gone. Refresh and try again."),
    };
    if into.body.chars().count() + from.body.chars().count() > combine::MOST_CHARS {
        return error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "These notes are too long to combine with the AI in one go. Move the parts you want by hand.",
        );
    }
    let written = tokio::task::spawn_blocking(move || {
        let reply = writer(
            combine::SYSTEM,
            &combine::prompt(&into, &from),
            combine::reply_tokens(&into, &from),
        )?;
        anyhow::ensure!(!reply.trim().is_empty(), "the AI sent back nothing");
        let combined = combine::settle(&notes_dir, &into, &from, &reply);
        Ok::<_, anyhow::Error>((into, from, combined))
    })
    .await;
    let (into, from, combined) = match written {
        Ok(Ok(done)) => done,
        _ => {
            return error(
                StatusCode::BAD_GATEWAY,
                "The AI could not combine these notes. Nothing was changed; try again.",
            )
        }
    };
    Json(serde_json::json!({
        "title": into.title,
        "with_title": from.title,
        "body": combined.body,
        "added": combined.added,
        "kept": combined.kept,
        "kept_enough": combined.kept >= combine::KEPT_ENOUGH,
        "base": version_of(&into),
        "with_base": version_of(&from),
    }))
    .into_response()
}

#[derive(Deserialize)]
pub(crate) struct Keep {
    with: String,
    body: String,
    base: String,
    with_base: String,
}

pub(crate) async fn keep(
    State(state): State<AppState>,
    Path(into): Path<String>,
    Json(body): Json<Keep>,
) -> Response {
    if body.body.trim().is_empty() || body.body.chars().count() > combine::MOST_CHARS * 2 {
        return error(
            StatusCode::BAD_REQUEST,
            "The combined note is empty or too long.",
        );
    }
    let done = state
        .with_store(move |store| {
            let from = store.find_note(&body.with).ok_or(StatusCode::NOT_FOUND)?;
            if version_of(from) != body.with_base {
                return Err(StatusCode::CONFLICT);
            }
            let from = from.id.clone();
            let note = store.find_note_mut(&into).ok_or(StatusCode::NOT_FOUND)?;
            if note.id == from {
                return Err(StatusCode::BAD_REQUEST);
            }
            if version_of(note) != body.base {
                return Err(StatusCode::CONFLICT);
            }
            let before = std::mem::replace(&mut note.body, body.body);
            note.updated_at = chrono::Utc::now();
            let response = NoteResponse::from_note(note);
            store.delete_notes(std::slice::from_ref(&from));
            save(store)?;
            Ok(serde_json::json!({ "note": response, "before": before, "removed": from }))
        })
        .await;
    match done {
        Ok(value) => Json(value).into_response(),
        Err(StatusCode::CONFLICT) => error(
            StatusCode::CONFLICT,
            "One of the notes changed while they were being combined. Combine them again.",
        ),
        Err(code) => error(code, "The combined note could not be saved."),
    }
}
