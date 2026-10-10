use super::*;
use crate::routes::sources;
fn source(id: &str) -> leo_core::recording::Archive {
    leo_core::recording::Archive {
        id: id.into(),
        started: chrono::Utc::now(),
        passages: vec![leo_core::recording::Passage {
            start_secs: 0,
            end_secs: 30,
            text: "Dijkstra finds shortest paths".into(),
            speaker: "Microphone".into(),
        }],
        points: vec![leo_core::recording::Point {
            at_secs: 5,
            text: "explain a priority queue".into(),
        }],
        context: String::new(),
        wants: String::new(),
        warnings: vec![],
        trace: vec![],
    }
}
#[test]
fn retained_sources_are_editable_with_conflict_protection_and_regeneration_is_a_preview() {
    let (mut state, _d, ids) = state_with(&[("Graphs", "cs130")]);
    let id = ids[0].clone();
    store_now(&state, |store| {
        store.find_note_mut(&id).unwrap().body = "Existing note".into();
        store.save().unwrap();
        let mut kept = source("session-1234");
        kept.wants = "Focus on what is on the exam".into();
        leo_core::recording::save(&store.notes_dir, &id, &kept).unwrap();
        Ok(())
    })
    .unwrap();
    let page = json_of(run(sources::sources(
        State(state.clone()),
        Path(id.clone()),
        Query(serde_json::from_value(serde_json::json!({"q":"shortest"})).unwrap()),
    )));
    assert_eq!(page["matches"].as_array().unwrap().len(), 1);
    let edit = serde_json::json!({"source":"session-1234","base":page["versions"]["session-1234"],"points":[{"at_secs":5,"text":"priority queue"}],"passages":[{"start_secs":0,"end_secs":30,"speaker":"Microphone","text":"Dijkstra’s algorithm"}]});
    let put = || {
        run(sources::edit_sources(
            State(state.clone()),
            Path(id.clone()),
            Json(serde_json::from_value(edit.clone()).unwrap()),
        ))
    };
    assert_eq!(put().status(), StatusCode::OK);
    assert_eq!(put().status(), StatusCode::CONFLICT);
    let asked = Arc::new(Mutex::new(Vec::new()));
    let heard = Arc::clone(&asked);
    state.regenerator = Some(Arc::new(move |sources, profile| {
        assert_eq!(sources[0].passages[0].text, "Dijkstra’s algorithm");
        heard.lock().unwrap().push(profile.wants.clone());
        Ok(("Generated".into(), "Preview body".into()))
    }));
    run(sources::regenerate(
        State(state.clone()),
        Path(id.clone()),
        Json(serde_json::from_value(serde_json::json!({"wants":"Just the decisions"})).unwrap()),
    ));
    let response = json_of(run(sources::regenerate(
        State(state.clone()),
        Path(id.clone()),
        Json(serde_json::from_value(serde_json::json!({})).unwrap()),
    )));
    assert_eq!(response["body"], "Preview body");
    assert_eq!(
        *asked.lock().unwrap(),
        ["Just the decisions", "Focus on what is on the exam"],
        "what the user wants is kept with the recording and can be changed when writing again"
    );
    assert!(response["base"].is_string());
    store_now(&state, |store| {
        assert_eq!(store.find_note(&id).unwrap().body, "Existing note");
        Ok(())
    })
    .unwrap();
}
