use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::{progress, AppState};

pub(crate) async fn get(State(state): State<AppState>, Path(key): Path<String>) -> Response {
    if !progress::valid_key(&key) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    Json(
        state
            .progress
            .view(&key)
            .unwrap_or_else(|| serde_json::json!({ "fraction": null })),
    )
    .into_response()
}
