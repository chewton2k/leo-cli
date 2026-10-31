use crate::AppState;
use anyhow::Result;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::sync::Arc;

pub trait SpeechApi: Send + Sync {
    fn view(&self) -> Result<Value>;
    fn change(&self, change: Value) -> Result<Value>;
}

fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}

async fn call(api: Option<Arc<dyn SpeechApi>>, change: Option<Value>) -> Response {
    let Some(api) = api else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Speech models are unavailable. Reopen Leo and try again.",
        );
    };
    match tokio::task::spawn_blocking(move || match change {
        Some(change) => api.change(change),
        None => api.view(),
    })
    .await
    {
        Ok(Ok(view)) => Json(view).into_response(),
        Ok(Err(problem)) => error(StatusCode::BAD_REQUEST, &problem.to_string()),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Speech models could not be opened. Try again in a moment.",
        ),
    }
}

pub(crate) async fn get(State(state): State<AppState>) -> Response {
    call(state.speech, None).await
}

pub(crate) async fn change(
    State(state): State<AppState>,
    change: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Ok(Json(change)) = change else {
        return error(
            StatusCode::BAD_REQUEST,
            "This model change could not be read. Choose an action in Speech models.",
        );
    };
    call(state.speech, Some(change)).await
}
