use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

struct Fake;
impl crate::ollama::OllamaApi for Fake {
    fn view(&self) -> serde_json::Value {
        serde_json::json!({
            "installed": true,
            "running": true,
            "models": [
                { "name": "qwen3:4b", "fits": true },
                { "name": "gemma3:27b", "fits": false },
            ],
        })
    }
    fn pull(
        &self,
        _model: &str,
        progress: &mut dyn FnMut(u64, u64, &str),
        stop: &AtomicBool,
    ) -> anyhow::Result<()> {
        progress(4, 10, "pulling a");
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        anyhow::bail!("Download cancelled.")
    }
}

fn act(state: &AppState, body: serde_json::Value) -> (StatusCode, serde_json::Value) {
    let response = run(crate::ollama::change(State(state.clone()), Ok(Json(body))));
    (response.status(), json_of(response))
}

#[test]
fn ollama_pulls_refuse_too_big_models_and_a_second_download() {
    let (mut state, _dir, _) = state_with(&[]);
    state.ollama = Some(Arc::new(Fake));
    let (status, body) = act(
        &state,
        serde_json::json!({"action":"pull","model":"gemma3:27b"}),
    );
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("too big"));
    let (status, _) = act(
        &state,
        serde_json::json!({"action":"pull","model":"llama:70b"}),
    );
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, body) = act(
        &state,
        serde_json::json!({"action":"pull","model":"qwen3:4b"}),
    );
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["pull"]["model"], "qwen3:4b");
    assert_eq!(body["pull"]["state"], "pulling");
    let (status, body) = act(
        &state,
        serde_json::json!({"action":"pull","model":"qwen3:4b"}),
    );
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("already downloading"));
    let started = std::time::Instant::now();
    loop {
        let view = json_of(run(crate::ollama::get(State(state.clone()))));
        if view["pull"]["completed"] == 4 {
            assert_eq!(view["pull"]["total"], 10);
            assert_eq!(view["models"][0]["name"], "qwen3:4b");
            break;
        }
        assert!(started.elapsed().as_secs() < 10, "no progress arrived");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let (status, body) = act(&state, serde_json::json!({"action":"cancel"}));
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["pull"]["state"], "cancelled");
    std::thread::sleep(std::time::Duration::from_millis(50));
    let view = json_of(run(crate::ollama::get(State(state.clone()))));
    assert_eq!(view["pull"]["state"], "cancelled");
    assert!(view["pull"]["error"].is_null());
    let (status, _) = act(
        &state,
        serde_json::json!({"action":"pull","model":"qwen3:4b"}),
    );
    assert_eq!(status, StatusCode::OK);
    act(&state, serde_json::json!({"action":"cancel"}));
    let (status, _) = act(&state, serde_json::json!({"action":"delete"}));
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[test]
fn ollama_without_the_power_says_so() {
    let (state, _dir, _) = state_with(&[]);
    let response = run(crate::ollama::get(State(state)));
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(json_of(response)["error"].is_string());
}
