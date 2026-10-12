use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

use crate::{interview, AppState};

fn failed(why: interview::Error) -> Response {
    let code = match &why {
        interview::Error::Invalid(_) => StatusCode::BAD_REQUEST,
        interview::Error::Conflict => StatusCode::CONFLICT,
        interview::Error::Missing => StatusCode::NOT_FOUND,
        interview::Error::Storage => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (code, Json(serde_json::json!({"error": why.message()}))).into_response()
}

pub(crate) async fn list(State(state): State<AppState>) -> Response {
    match tokio::task::spawn_blocking(move || interview::list(&state.chats)).await {
        Ok(Ok(items)) => Json(items).into_response(),
        Ok(Err(why)) => failed(why),
        Err(_) => failed(interview::Error::Storage),
    }
}

pub(crate) async fn get(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match tokio::task::spawn_blocking(move || interview::load(&state.chats, &id)).await {
        Ok(Ok(item)) => Json(item).into_response(),
        Ok(Err(why)) => failed(why),
        Err(_) => failed(interview::Error::Storage),
    }
}

pub(crate) async fn put(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(change): Json<interview::Change>,
) -> Response {
    match tokio::task::spawn_blocking(move || interview::change(&state.chats, &id, change)).await {
        Ok(Ok(item)) => Json(item).into_response(),
        Ok(Err(why)) => failed(why),
        Err(_) => failed(interview::Error::Storage),
    }
}

pub(crate) async fn remove(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match tokio::task::spawn_blocking(move || interview::remove(&state.chats, &id)).await {
        Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err(why)) => failed(why),
        Err(_) => failed(interview::Error::Storage),
    }
}
