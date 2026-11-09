use crate::AppState;
use anyhow::Result;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub trait OllamaApi: Send + Sync {
    fn view(&self) -> Value;
    fn pull(
        &self,
        model: &str,
        progress: &mut dyn FnMut(u64, u64, &str),
        stop: &AtomicBool,
    ) -> Result<()>;
}

#[derive(Default)]
pub(crate) struct Pulls(Mutex<Option<(Value, Arc<AtomicBool>)>>);

impl Pulls {
    fn shown(&self) -> Value {
        self.0
            .lock()
            .ok()
            .and_then(|pull| pull.as_ref().map(|(shown, _)| shown.clone()))
            .unwrap_or_default()
    }

    fn update(&self, stop: &Arc<AtomicBool>, change: impl FnOnce(&mut Value)) {
        if let Ok(mut pull) = self.0.lock() {
            if let Some((shown, _)) = pull.as_mut().filter(|(shown, current)| {
                Arc::ptr_eq(current, stop) && shown["state"] == "pulling"
            }) {
                change(shown);
            }
        }
    }
}

fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}

async fn view(state: &AppState) -> Response {
    let Some(api) = state.ollama.clone() else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Ollama setup is unavailable. Reopen Leo and try again.",
        );
    };
    match tokio::task::spawn_blocking(move || api.view()).await {
        Ok(mut view) => {
            view["pull"] = state.ollama_pulls.shown();
            Json(view).into_response()
        }
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Ollama could not be checked. Try again in a moment.",
        ),
    }
}

pub(crate) async fn get(State(state): State<AppState>) -> Response {
    view(&state).await
}

pub(crate) async fn change(
    State(state): State<AppState>,
    change: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Ok(Json(change)) = change else {
        return error(StatusCode::BAD_REQUEST, "This request could not be read.");
    };
    let Some(api) = state.ollama.clone() else {
        return view(&state).await;
    };
    match change["action"].as_str() {
        Some("cancel") => {
            if let Ok(mut pull) = state.ollama_pulls.0.lock() {
                if let Some((shown, stop)) = pull.as_mut().filter(|(s, _)| s["state"] == "pulling")
                {
                    stop.store(true, Ordering::Relaxed);
                    shown["state"] = json!("cancelled");
                }
            }
            view(&state).await
        }
        Some("pull") => {
            let model = change["model"].as_str().unwrap_or_default().to_string();
            let listed = {
                let api = Arc::clone(&api);
                tokio::task::spawn_blocking(move || api.view()).await
            };
            let Ok(listed) = listed else {
                return error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Ollama could not be checked. Try again in a moment.",
                );
            };
            let Some(entry) = listed["models"]
                .as_array()
                .and_then(|models| models.iter().find(|m| m["name"] == model.as_str()))
            else {
                return error(StatusCode::BAD_REQUEST, "Choose a model from the list.");
            };
            if entry["fits"] != true {
                return error(
                    StatusCode::BAD_REQUEST,
                    "That model is too big for this computer. Choose one that fits.",
                );
            }
            let stop = Arc::new(AtomicBool::new(false));
            {
                let Ok(mut pull) = state.ollama_pulls.0.lock() else {
                    return error(StatusCode::INTERNAL_SERVER_ERROR, "Try again in a moment.");
                };
                if pull.as_ref().is_some_and(|(s, _)| s["state"] == "pulling") {
                    return error(
                        StatusCode::CONFLICT,
                        "A model is already downloading. Wait for it to finish or cancel it.",
                    );
                }
                *pull = Some((
                    json!({ "model": model, "state": "pulling", "completed": 0, "total": 0, "status": "" }),
                    Arc::clone(&stop),
                ));
            }
            let pulls = Arc::clone(&state.ollama_pulls);
            std::thread::spawn(move || {
                let outcome = api.pull(
                    &model,
                    &mut |completed, total, status| {
                        pulls.update(&stop, |shown| {
                            shown["completed"] = json!(completed);
                            shown["total"] = json!(total);
                            shown["status"] = json!(status);
                        })
                    },
                    &stop,
                );
                pulls.update(&stop, |shown| match outcome {
                    Ok(()) => shown["state"] = json!("done"),
                    Err(problem) => {
                        shown["state"] = json!("failed");
                        shown["error"] = json!(problem.to_string());
                    }
                });
            });
            view(&state).await
        }
        _ => error(StatusCode::BAD_REQUEST, "Choose Download or Cancel."),
    }
}
