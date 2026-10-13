use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::{chats, storage, AppState};

fn live_recording(state: &AppState) -> Option<String> {
    state.recording.lock().ok().and_then(|held| {
        held.as_ref()
            .filter(|job| job.busy())
            .map(|job| job.view().id)
    })
}

async fn storage_now(state: &AppState) -> Result<serde_json::Value, StatusCode> {
    let graphs = Arc::clone(&state.graphs);
    let chats = state.chats.clone();
    let housekeeper = state.housekeeper.clone();
    let captions = Arc::clone(&state.captions);
    let vectors = Arc::clone(&state.vectors);
    let recording = live_recording(state);
    let store = state.with_store(|store| Ok(store.read_snapshot())).await?;
    tokio::task::spawn_blocking(move || {
        let kept = storage::Kept {
            captions: &captions,
            vectors: &vectors,
            recording,
        };
        let mut areas = storage::areas(&store, &graphs, &chats, chrono::Utc::now());
        areas.extend(storage::more_areas(&store, &chats, &kept));
        areas.extend(crate::storage_saved::areas(&store, &chats)?);
        if let Some(more) = housekeeper {
            areas.extend(more.areas());
        }
        areas.push(crate::storage_saved::database_area(&chats, &areas));
        Ok::<_, anyhow::Error>(storage::describe(areas))
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

pub(crate) async fn get_storage(State(state): State<AppState>) -> Response {
    match storage_now(&state).await {
        Ok(page) => Json(page).into_response(),
        Err(code) => storage_error(code),
    }
}

fn storage_error(code: StatusCode) -> Response {
    (code, Json(serde_json::json!({"error":"Storage could not be refreshed. Check that Leo can read and write its data folder, then refresh Storage. If cleanup was already confirmed, inspect the list before trying again."}))).into_response()
}

pub(crate) async fn change_storage(
    State(state): State<AppState>,
    Json(request): Json<storage::Request>,
) -> Response {
    let graphs = Arc::clone(&state.graphs);
    let chats = state.chats.clone();
    let housekeeper = state.housekeeper.clone();
    let captions = Arc::clone(&state.captions);
    let vectors = Arc::clone(&state.vectors);
    let recording = live_recording(&state);
    let calendar = Arc::clone(&state.calendar);
    let store = match state.with_store(|store| Ok(store.read_snapshot())).await {
        Ok(store) => store,
        Err(code) => return code.into_response(),
    };
    let done = tokio::task::spawn_blocking(move || {
        let kept = storage::Kept {
            captions: &captions,
            vectors: &vectors,
            recording,
        };
        storage::act_on(&store, &graphs, &chats, &request, chrono::Utc::now())
            .or_else(|| storage::act_on_more(&store, &chats, &kept, &request))
            .or_else(|| {
                if request.area == "calendar-cache" {
                    Some(calendar.clear_cached(&chats, &request))
                } else {
                    crate::storage_saved::act_on(&chats, &request)
                }
            })
            .or_else(|| {
                housekeeper.and_then(|h| h.act(&request.area, &request.action, &request.items))
            })
    })
    .await;
    let message = match done {
        Ok(Some(Ok(message))) => message,
        Ok(Some(Err(e))) => {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response()
        }
        Ok(None) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "leo does not know how to do that." })),
            )
                .into_response()
        }
        Err(_) => return storage_error(StatusCode::INTERNAL_SERVER_ERROR),
    };
    match storage_now(&state).await {
        Ok(page) => {
            Json(serde_json::json!({ "message": message, "storage": page })).into_response()
        }
        Err(code) => storage_error(code),
    }
}

fn keep_page(keep: leo_core::keep::Keep) -> serde_json::Value {
    let choices = |list: &[Option<u32>]| {
        list.iter()
            .map(|d| serde_json::json!({ "days": d, "label": leo_core::keep::describe(*d) }))
            .collect::<Vec<_>>()
    };
    serde_json::json!({
        "trash_days": keep.trash_days,
        "chat_days": keep.chat_days,
        "trash_choices": choices(&leo_core::keep::TRASH_CHOICES),
        "chat_choices": choices(&leo_core::keep::CHAT_CHOICES),
    })
}

pub(crate) async fn get_keep(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    state
        .with_store(|store| Ok(Json(keep_page(leo_core::keep::load(&store.notes_dir)))))
        .await
}

pub(crate) async fn set_keep(
    State(state): State<AppState>,
    Json(keep): Json<leo_core::keep::Keep>,
) -> Response {
    let chats_dir = state.chats.clone();
    let done = state
        .with_store(move |store| {
            leo_core::keep::save(&store.notes_dir, &keep).map_err(|_| StatusCode::BAD_REQUEST)?;
            store.tidy_trash_now();
            chats::tidy(&chats_dir, keep.chat_days, chrono::Utc::now());
            Ok(keep_page(keep))
        })
        .await;
    match done {
        Ok(page) => Json(page).into_response(),
        Err(StatusCode::BAD_REQUEST) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Pick one of the choices offered." })),
        )
            .into_response(),
        Err(code) => code.into_response(),
    }
}
