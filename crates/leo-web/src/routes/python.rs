use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};

use std::sync::{Arc, Mutex};

use crate::{AppState, Runtime};

pub(crate) struct Kind {
    files: &'static [(&'static str, &'static str)],
    bytes: u64,
    mb: u32,
    failed: &'static str,
}

pub(crate) static PYTHON: Kind = Kind {
    files: &[
        ("pyodide.js", "text/javascript; charset=utf-8"),
        ("pyodide.asm.js", "text/javascript; charset=utf-8"),
        ("pyodide.asm.wasm", "application/wasm"),
        ("python_stdlib.zip", "application/zip"),
        ("pyodide-lock.json", "application/json"),
    ],
    bytes: 12_262_929,
    mb: 12,
    failed: "Python could not be downloaded. Check the connection and try again.",
};

pub(crate) static CPP: Kind = Kind {
    files: &[
        ("bundle.js", "text/javascript; charset=utf-8"),
        ("llvm-resources.tar", "application/x-tar"),
        ("llvm.core.wasm", "application/wasm"),
        ("llvm.core2.wasm", "application/wasm"),
        ("llvm.core3.wasm", "application/wasm"),
        ("llvm.core4.wasm", "application/wasm"),
    ],
    bytes: 27_112_311,
    mb: 27,
    failed: "The C++ compiler could not be downloaded. Check the connection and try again.",
};

fn parts(state: &AppState, kind: &Kind) -> (Option<Runtime>, Arc<Mutex<Option<String>>>) {
    if std::ptr::eq(kind, &CPP) {
        (state.cpp.clone(), Arc::clone(&state.cpp_fetch))
    } else {
        (state.python.clone(), Arc::clone(&state.python_fetch))
    }
}

fn fetched(dir: &std::path::Path, total: u64) -> u64 {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter_map(|e| e.metadata().ok())
                .filter(|m| m.is_file())
                .map(|m| m.len())
                .sum::<u64>()
                .min(total)
        })
        .unwrap_or(0)
}

fn view(state: &AppState, kind: &Kind) -> serde_json::Value {
    let (runtime, slot) = parts(state, kind);
    let Some(runtime) = runtime else {
        return serde_json::json!({ "status": "unavailable" });
    };
    if (runtime.ready)() {
        return serde_json::json!({ "status": "ready" });
    }
    let fetching = slot.lock().unwrap_or_else(|e| e.into_inner()).clone();
    match fetching.as_deref() {
        Some("fetching") => {
            serde_json::json!({ "status": "fetching", "mb": kind.mb, "done": fetched(&runtime.dir, kind.bytes), "total": kind.bytes })
        }
        Some(problem) => serde_json::json!({ "status": "failed", "error": problem, "mb": kind.mb }),
        None => serde_json::json!({ "status": "missing", "mb": kind.mb }),
    }
}

async fn status_of(state: AppState, kind: &'static Kind) -> Response {
    match tokio::task::spawn_blocking(move || view(&state, kind)).await {
        Ok(view) => Json(view).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn fetch_of(state: AppState, kind: &'static Kind) -> Response {
    let (runtime, slot) = parts(&state, kind);
    let Some(runtime) = runtime else {
        return Json(serde_json::json!({ "status": "unavailable" })).into_response();
    };
    {
        let mut held = slot.lock().unwrap_or_else(|e| e.into_inner());
        if held.as_deref() != Some("fetching") && !(runtime.ready)() {
            *held = Some("fetching".into());
            let done = Arc::clone(&slot);
            let started = std::thread::Builder::new()
                .name("leo-runtime".into())
                .spawn(move || {
                    let outcome = (runtime.fetch)();
                    *done.lock().unwrap_or_else(|e| e.into_inner()) = match outcome {
                        Ok(()) => None,
                        Err(_) => Some(kind.failed.into()),
                    };
                });
            if started.is_err() {
                *held = None;
            }
        }
    }
    status_of(state, kind).await
}

async fn file_of(state: AppState, kind: &'static Kind, name: String) -> Response {
    let (runtime, _) = parts(&state, kind);
    let (Some(runtime), Some((_, content))) =
        (runtime, kind.files.iter().find(|(file, _)| *file == name))
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !(runtime.ready)() {
        return StatusCode::NOT_FOUND.into_response();
    }
    match tokio::fs::read(runtime.dir.join(&name)).await {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, *content),
                (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
                (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

pub(crate) async fn status(State(state): State<AppState>) -> Response {
    status_of(state, &PYTHON).await
}

pub(crate) async fn fetch(State(state): State<AppState>) -> Response {
    fetch_of(state, &PYTHON).await
}

pub(crate) async fn file(State(state): State<AppState>, Path(name): Path<String>) -> Response {
    file_of(state, &PYTHON, name).await
}

pub(crate) async fn cpp_status(State(state): State<AppState>) -> Response {
    status_of(state, &CPP).await
}

pub(crate) async fn cpp_fetch(State(state): State<AppState>) -> Response {
    fetch_of(state, &CPP).await
}

pub(crate) async fn cpp_file(State(state): State<AppState>, Path(name): Path<String>) -> Response {
    file_of(state, &CPP, name).await
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
        state.python = Some(crate::Runtime {
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

    #[test]
    fn only_the_pinned_compiler_files_are_served_and_a_download_reports_bytes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("bundle.js"), "export {}").unwrap();
        std::fs::write(dir.path().join("clang.tgz.part"), vec![0u8; 1000]).unwrap();
        let (mut state, _d, _) = crate::tests::state_for_python();
        state.cpp = Some(crate::Runtime {
            dir: dir.path().to_path_buf(),
            ready: std::sync::Arc::new(|| true),
            fetch: std::sync::Arc::new(|| Ok(())),
        });
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let served = runtime.block_on(cpp_file(State(state.clone()), Path("bundle.js".into())));
        assert_eq!(served.status(), StatusCode::OK);
        assert_eq!(served.headers()["access-control-allow-origin"], "*");
        for refused in ["clang.tgz.part", "pyodide.js", "../bundle.js"] {
            let answer = runtime.block_on(cpp_file(State(state.clone()), Path(refused.into())));
            assert_eq!(answer.status(), StatusCode::NOT_FOUND, "{refused}");
        }
        assert_eq!(fetched(dir.path(), CPP.bytes), 1009);
        state.cpp = None;
        assert_eq!(view(&state, &CPP)["status"], "unavailable");
        assert_eq!(view(&state, &PYTHON)["status"], "unavailable");
    }
}
