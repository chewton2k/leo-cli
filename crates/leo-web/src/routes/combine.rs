use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;

use super::notes::{save, NoteResponse};
use crate::{combine, AppState};

fn error(code: StatusCode, message: &str) -> Response {
    (code, Json(serde_json::json!({ "error": message }))).into_response()
}

#[derive(Deserialize)]
pub(crate) struct Combine {
    with: String,
}

pub(crate) async fn combine(
    State(state): State<AppState>,
    Path(into): Path<String>,
    Json(body): Json<Combine>,
) -> Response {
    let done = state
        .with_store(move |store| {
            let from = store
                .find_note(&body.with)
                .ok_or(StatusCode::NOT_FOUND)?
                .clone();
            let target = store.find_note(&into).ok_or(StatusCode::NOT_FOUND)?.clone();
            if target.id == from.id {
                return Err(StatusCode::BAD_REQUEST);
            }
            if target.body.chars().count() + from.body.chars().count() > combine::MOST_CHARS {
                return Err(StatusCode::PAYLOAD_TOO_LARGE);
            }
            let joined = combine::joined(&store.notes_dir, &target, &from);
            let notes_dir = store.notes_dir.clone();
            let copied = leo_core::recording::copy_to(&notes_dir, &from.id, &target.id, &from.title)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            let note = store.find_note_mut(&target.id).ok_or(StatusCode::NOT_FOUND)?;
            let before = std::mem::replace(&mut note.body, joined);
            note.updated_at = chrono::Utc::now();
            let response = NoteResponse::from_note(note);
            store.delete_notes(std::slice::from_ref(&from.id));
            if let Err(code) = save(store) {
                let _ = leo_core::recording::remove(&notes_dir, &target.id, &copied);
                return Err(code);
            }
            Ok(serde_json::json!({ "note": response, "before": before, "removed": from.id, "sources": copied }))
        })
        .await;
    match done {
        Ok(value) => Json(value).into_response(),
        Err(StatusCode::BAD_REQUEST) => {
            error(StatusCode::BAD_REQUEST, "Drop a note on a different note.")
        }
        Err(StatusCode::PAYLOAD_TOO_LARGE) => error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "These notes are too long to combine. Move the parts you want by hand.",
        ),
        Err(StatusCode::NOT_FOUND) => error(
            StatusCode::NOT_FOUND,
            "One of those notes is gone. Refresh and try again.",
        ),
        Err(code) => error(code, "The notes could not be combined."),
    }
}

#[derive(Deserialize)]
pub(crate) struct Uncombine {
    #[serde(default)]
    sources: Vec<String>,
}

pub(crate) async fn uncombine(
    State(state): State<AppState>,
    Path(into): Path<String>,
    Json(body): Json<Uncombine>,
) -> Response {
    if body.sources.len() > 1000 {
        return error(StatusCode::BAD_REQUEST, "Too many recordings.");
    }
    let done = state
        .with_store(move |store| {
            store.find_note(&into).ok_or(StatusCode::NOT_FOUND)?;
            leo_core::recording::remove(&store.notes_dir, &into, &body.sources)
                .map_err(|_| StatusCode::BAD_REQUEST)
        })
        .await;
    match done {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(code) => error(code, "The recordings could not be moved back."),
    }
}
