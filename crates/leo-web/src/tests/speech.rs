use super::*;

struct Fake;
impl crate::speech::SpeechApi for Fake {
    fn view(&self) -> anyhow::Result<serde_json::Value> {
        Ok(serde_json::json!({ "models": [], "ready": false }))
    }
    fn change(&self, change: serde_json::Value) -> anyhow::Result<serde_json::Value> {
        anyhow::ensure!(change["id"] == "model", "Choose a model from the library.");
        self.view()
    }
}

#[test]
fn speech_changes_are_validated_and_every_success_is_json() {
    let (mut state, _dir, _) = state_with(&[]);
    state.speech = Some(Arc::new(Fake));
    let response = run(crate::speech::get(State(state.clone())));
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_of(response)["ready"], false);
    let response = run(crate::speech::change(
        State(state.clone()),
        Ok(Json(serde_json::json!({"id":"model", "action":"cancel"}))),
    ));
    assert_eq!(response.status(), StatusCode::OK);
    assert!(json_of(response)["models"].is_array());
    let response = run(crate::speech::change(
        State(state),
        Ok(Json(serde_json::json!({"id":"../bad"}))),
    ));
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(json_of(response)["error"].is_string());
}
