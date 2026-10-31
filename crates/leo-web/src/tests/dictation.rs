use super::*;
struct Fake(Mutex<Vec<String>>);
impl crate::dictation::DictationApi for Fake {
    fn view(&self) -> anyhow::Result<serde_json::Value> {
        Ok(serde_json::json!({"settings":{}}))
    }
    fn change(&self, change: serde_json::Value) -> anyhow::Result<serde_json::Value> {
        anyhow::ensure!(change["action"] == "copy", "Choose a dictation action.");
        self.0
            .lock()
            .unwrap()
            .push(change["text"].as_str().unwrap().into());
        self.view()
    }
}
#[test]
fn dictation_copy_uses_only_the_kept_text_and_successes_are_json() {
    let (mut state, dir, _) = state_with(&[]);
    let fake = Arc::new(Fake(Mutex::new(Vec::new())));
    state.dictation = Some(fake.clone());
    crate::dictation::remember(dir.path(), "kept text").unwrap();
    let response = run(crate::dictation::get(State(state.clone())));
    assert_eq!(response.status(), StatusCode::OK);
    let view = json_of(response);
    let id = view["recent"][0]["id"].clone();
    let response = run(crate::dictation::change(
        State(state.clone()),
        Ok(Json(
            serde_json::json!({"action":"copy","id":id,"text":"untrusted replacement"}),
        )),
    ));
    assert_eq!(response.status(), StatusCode::OK);
    assert!(json_of(response)["recent"].is_array());
    assert_eq!(*fake.0.lock().unwrap(), vec!["kept text"]);
    let response = run(crate::dictation::change(
        State(state.clone()),
        Ok(Json(serde_json::json!({"action":"copy","id":999}))),
    ));
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let response = run(crate::dictation::change(
        State(state),
        Ok(Json(serde_json::json!({"action":"clear"}))),
    ));
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_of(response)["recent"], serde_json::json!([]));
}
