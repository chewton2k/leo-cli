use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::AppState;

fn no_settings() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({ "error": "Settings are not available from this server." })),
    )
        .into_response()
}

pub(crate) async fn get_settings(State(state): State<AppState>) -> Response {
    let Some(settings) = state.settings.clone() else {
        return no_settings();
    };
    let notes_dir = match state.with_store(|store| Ok(store.notes_dir.clone())).await {
        Ok(dir) => dir,
        Err(code) => return code.into_response(),
    };
    match tokio::task::spawn_blocking(move || settings.describe(&notes_dir)).await {
        Ok(mut page) => {
            page["felix_usage"] = serde_json::json!(state.usage.snapshot());
            page["felix_tools"] = serde_json::json!(state.usage.tools());
            Json(page).into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub(crate) async fn change_setting(
    State(state): State<AppState>,
    Json(change): Json<serde_json::Value>,
) -> Response {
    let Some(settings) = state.settings.clone() else {
        return no_settings();
    };
    let notes_dir = match state.with_store(|store| Ok(store.notes_dir.clone())).await {
        Ok(dir) => dir,
        Err(code) => return code.into_response(),
    };
    let warm = std::sync::Arc::clone(&state.warm);
    let usage = std::sync::Arc::clone(&state.usage);
    let done = tokio::task::spawn_blocking(move || {
        settings.apply(&change).map(|message| {
            warm.clear();
            let mut page = settings.describe(&notes_dir);
            page["felix_usage"] = serde_json::json!(usage.snapshot());
            page["felix_tools"] = serde_json::json!(usage.tools());
            (message, page)
        })
    })
    .await;
    match done {
        Ok(Ok((message, page))) => {
            Json(serde_json::json!({ "message": message, "settings": page })).into_response()
        }
        Ok(Err(e)) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub(crate) async fn test_setting(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let Some(settings) = state.settings.clone() else {
        return no_settings();
    };
    let task = body
        .get("task")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();
    match tokio::task::spawn_blocking(move || settings.test(&task)).await {
        Ok(Ok(message)) => Json(serde_json::json!({ "message": message })).into_response(),
        Ok(Err(e)) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
