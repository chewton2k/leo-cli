use crate::AppState;
use anyhow::Result;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
pub trait AppApi: Send + Sync {
    fn view(&self) -> Result<Value>;
    fn change(&self, value: Value) -> Result<Value>;
    fn health(&self, notes: &Path) -> Result<Value>;
}
fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({"error":message}))).into_response()
}
async fn call(
    api: Option<Arc<dyn AppApi>>,
    change: Option<Value>,
    notes: Option<PathBuf>,
    margin: Option<String>,
) -> Response {
    let Some(api) = api else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "These settings are available in the Leo app. Reopen Leo and try again.",
        );
    };
    match tokio::task::spawn_blocking(move || {
        if let Some(notes) = notes {
            let mut view = api.health(&notes)?;
            if let (Some(line), Some(sections)) = (margin, view["sections"].as_array_mut()) {
                sections.push(json!({
                    "title": "Felix",
                    "checks": [{ "name": "Reading along", "state": "warn", "detail": line, "fix": "Open Felix in Settings to change how he reads.", "section": "felix" }],
                }));
            }
            Ok(view)
        } else {
            if let Some(change) = change {
                api.change(change)?;
            }
            api.view()
        }
    })
    .await
    {
        Ok(Ok(view)) => Json(view).into_response(),
        Ok(Err(e)) => error(StatusCode::BAD_REQUEST, &e.to_string()),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "App settings could not be opened. Try again in a moment.",
        ),
    }
}
pub(crate) async fn get(State(state): State<AppState>) -> Response {
    call(state.app_settings, None, None, None).await
}
pub(crate) async fn change(
    State(state): State<AppState>,
    value: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Ok(Json(value)) = value else {
        return error(
            StatusCode::BAD_REQUEST,
            "This app setting could not be read. Choose an option in Settings.",
        );
    };
    call(state.app_settings, Some(value), None, None).await
}
pub(crate) async fn health(State(state): State<AppState>) -> Response {
    let notes = match state.with_store(|s| Ok(s.notes_dir.clone())).await {
        Ok(notes) => notes,
        Err(status) => return status.into_response(),
    };
    call(
        state.app_settings,
        None,
        Some(notes),
        state.margin.problem(),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fake;
    impl AppApi for Fake {
        fn view(&self) -> anyhow::Result<serde_json::Value> {
            Ok(serde_json::json!({"automatic_updates":true,"install_on_quit":true}))
        }
        fn change(&self, value: serde_json::Value) -> anyhow::Result<serde_json::Value> {
            anyhow::ensure!(value["action"] == "check_updates", "Choose an app setting.");
            self.view()
        }
        fn health(&self, _notes: &std::path::Path) -> anyhow::Result<serde_json::Value> {
            Ok(serde_json::json!({"sections":[]}))
        }
    }
    #[tokio::test]
    async fn app_changes_and_health_are_json_and_validate_actions() {
        let api = Some(std::sync::Arc::new(Fake) as std::sync::Arc<dyn AppApi>);
        assert_eq!(
            call(api.clone(), None, None, None).await.status(),
            axum::http::StatusCode::OK
        );
        assert_eq!(
            call(
                api.clone(),
                Some(serde_json::json!({"action":"check_updates"})),
                None,
                None
            )
            .await
            .status(),
            axum::http::StatusCode::OK
        );
        assert_eq!(
            call(
                api.clone(),
                Some(serde_json::json!({"action":"invalid"})),
                None,
                None
            )
            .await
            .status(),
            axum::http::StatusCode::BAD_REQUEST
        );
        assert_eq!(
            call(api, None, Some(std::path::PathBuf::from("unused")), None)
                .await
                .status(),
            axum::http::StatusCode::OK
        );
    }
}
