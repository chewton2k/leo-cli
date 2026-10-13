use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};

use crate::AppState;

pub(crate) const FILES: [(&str, &str); 5] = [
    ("pyodide.js", "text/javascript; charset=utf-8"),
    ("pyodide.asm.js", "text/javascript; charset=utf-8"),
    ("pyodide.asm.wasm", "application/wasm"),
    ("python_stdlib.zip", "application/zip"),
    ("pyodide-lock.json", "application/json"),
];

fn view(state: &AppState) -> serde_json::Value {
    let Some(python) = &state.python else {
        return serde_json::json!({ "status": "unavailable" });
    };
    if (python.ready)() {
        return serde_json::json!({ "status": "ready" });
    }
    let fetching = state
        .python_fetch
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    match fetching.as_deref() {
        Some("fetching") => serde_json::json!({ "status": "fetching", "mb": 12 }),
        Some(problem) => serde_json::json!({ "status": "failed", "error": problem, "mb": 12 }),
        None => serde_json::json!({ "status": "missing", "mb": 12 }),
    }
}

pub(crate) async fn status(State(state): State<AppState>) -> Response {
    match tokio::task::spawn_blocking(move || view(&state)).await {
        Ok(view) => Json(view).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub(crate) async fn fetch(State(state): State<AppState>) -> Response {
    let Some(python) = state.python.clone() else {
        return Json(serde_json::json!({ "status": "unavailable" })).into_response();
    };
    {
        let mut slot = state.python_fetch.lock().unwrap_or_else(|e| e.into_inner());
        if slot.as_deref() != Some("fetching") && !(python.ready)() {
            *slot = Some("fetching".into());
            let done = std::sync::Arc::clone(&state.python_fetch);
            let started = std::thread::Builder::new()
                .name("leo-python".into())
                .spawn(move || {
                    let outcome = (python.fetch)();
                    *done.lock().unwrap_or_else(|e| e.into_inner()) = match outcome {
                        Ok(()) => None,
                        Err(_) => Some(
                            "Python could not be downloaded. Check the connection and try again."
                                .into(),
                        ),
                    };
                });
            if started.is_err() {
                *slot = None;
            }
        }
    }
    status(State(state)).await
}

pub(crate) async fn file(State(state): State<AppState>, Path(name): Path<String>) -> Response {
    let (Some(python), Some((_, kind))) = (
        state.python.clone(),
        FILES.iter().find(|(file, _)| *file == name),
    ) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !(python.ready)() {
        return StatusCode::NOT_FOUND.into_response();
    }
    match tokio::fs::read(python.dir.join(&name)).await {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, *kind),
                (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
                (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_pinned_python_files_are_served() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pyodide.js"), "loadPyodide").unwrap();
        std::fs::write(dir.path().join("secret.txt"), "no").unwrap();
        let (mut state, _d, _) = crate::tests::state_for_python();
        let path = dir.path().to_path_buf();
        state.python = Some(crate::Python {
            dir: path,
            ready: std::sync::Arc::new(|| true),
            fetch: std::sync::Arc::new(|| Ok(())),
        });
        let run = |name: &str| {
            tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(file(State(state.clone()), Path(name.to_string())))
        };
        let served = run("pyodide.js");
        assert_eq!(served.status(), StatusCode::OK);
        assert_eq!(served.headers()["access-control-allow-origin"], "*");
        for refused in ["secret.txt", "../config.toml", "pyodide.js/../secret.txt"] {
            assert_eq!(run(refused).status(), StatusCode::NOT_FOUND, "{refused}");
        }
    }
}
