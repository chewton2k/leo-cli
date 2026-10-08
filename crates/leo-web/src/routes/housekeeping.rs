use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::{chats, storage, AppState};

async fn storage_now(state: &AppState) -> Result<serde_json::Value, StatusCode> {
    let graphs = Arc::clone(&state.graphs);
    let chats = state.chats.clone();
    let housekeeper = state.housekeeper.clone();
    state
        .with_store(move |store| {
            let mut areas = storage::areas(store, &graphs, &chats, chrono::Utc::now());
            if let Some(more) = housekeeper {
                areas.extend(more.areas());
            }
            Ok(storage::describe(areas))
        })
        .await
}

pub(crate) async fn get_storage(State(state): State<AppState>) -> Response {
    match storage_now(&state).await {
        Ok(page) => Json(page).into_response(),
        Err(code) => code.into_response(),
    }
}

pub(crate) async fn change_storage(
    State(state): State<AppState>,
    Json(request): Json<storage::Request>,
) -> Response {
    let graphs = Arc::clone(&state.graphs);
    let chats = state.chats.clone();
    let housekeeper = state.housekeeper.clone();
    let done = state
        .with_store(move |store| {
            let outcome = storage::act_on(store, &graphs, &chats, &request, chrono::Utc::now())
                .or_else(|| {
                    housekeeper.and_then(|h| h.act(&request.area, &request.action, &request.items))
                });
            Ok(outcome)
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
        Err(code) => return code.into_response(),
    };
    match storage_now(&state).await {
        Ok(page) => {
            Json(serde_json::json!({ "message": message, "storage": page })).into_response()
        }
        Err(code) => code.into_response(),
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
