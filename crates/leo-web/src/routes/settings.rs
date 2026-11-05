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

pub(crate) async fn refresh_usage(State(state): State<AppState>) -> Response {
    let Some(settings) = state.settings.clone() else {
        return no_settings();
    };
    let Ok(permit) = state.usage_refreshes.clone().try_acquire_owned() else {
        return (StatusCode::CONFLICT, Json(serde_json::json!({ "error": "Usage is already refreshing. Wait a moment and try again." }))).into_response();
    };
    match tokio::task::spawn_blocking(move || {
        let _permit = permit;
        settings.refresh_usage();
    })
    .await
    {
        Ok(()) => get_settings(State(state)).await,
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "Could not refresh usage. Try again in a moment." })),
        )
            .into_response(),
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

pub(crate) async fn setup_backup(
    State(state): State<AppState>,
    body: Result<Json<serde_json::Value>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Some(settings) = state.settings.clone() else {
        return no_settings();
    };
    let Ok(Json(body)) = body else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error":"Enter a GitHub repository name to set up Backup."})),
        )
            .into_response();
    };
    let Some(name) = body["name"]
        .as_str()
        .filter(|s| s.len() <= 201)
        .map(str::to_owned)
    else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error":"Use a repository name such as leo-notes."})),
        )
            .into_response();
    };
    let Ok(permit) = state.backup_setups.clone().try_acquire_owned() else {
        return (StatusCode::CONFLICT,Json(serde_json::json!({"error":"Backup setup is already running. Let it finish, then try again."}))).into_response();
    };
    let notes = match state.with_store(|s| Ok(s.notes_dir.clone())).await {
        Ok(notes) => notes,
        Err(status) => return status.into_response(),
    };
    match tokio::task::spawn_blocking(move || { let _permit=permit; settings.setup_backup(&notes,&name).map(|message|(message,settings.describe(&notes))) }).await {
        Ok(Ok((message,page)))=>Json(serde_json::json!({"message":message,"settings":page})).into_response(),
        Ok(Err(e))=>(StatusCode::BAD_REQUEST,Json(serde_json::json!({"error":e.to_string()}))).into_response(),
        Err(_)=>(StatusCode::INTERNAL_SERVER_ERROR,Json(serde_json::json!({"error":"Backup setup stopped unexpectedly. Check Health and try again."}))).into_response(),
    }
}
