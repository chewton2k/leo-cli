use super::*;
use crate::routes::workflows;
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
        template: "lecture".into(),
        context: String::new(),
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
        leo_core::recording::save(&store.notes_dir, &id, &source("session-1234")).unwrap();
        Ok(())
    })
    .unwrap();
    let page = json_of(run(workflows::sources(
        State(state.clone()),
        Path(id.clone()),
        Query(serde_json::from_value(serde_json::json!({"q":"shortest"})).unwrap()),
    )));
    assert_eq!(page["matches"].as_array().unwrap().len(), 1);
    let edit = serde_json::json!({"source":"session-1234","base":page["versions"]["session-1234"],"points":[{"at_secs":5,"text":"priority queue"}],"passages":[{"start_secs":0,"end_secs":30,"speaker":"Microphone","text":"Dijkstra’s algorithm"}]});
    let put = || {
        run(workflows::edit_sources(
            State(state.clone()),
            Path(id.clone()),
            Json(serde_json::from_value(edit.clone()).unwrap()),
        ))
    };
    assert_eq!(put().status(), StatusCode::OK);
    assert_eq!(put().status(), StatusCode::CONFLICT);
    state.regenerator = Some(Arc::new(|sources, profile, _| {
        assert_eq!(profile.template, "meeting");
        assert_eq!(sources[0].passages[0].text, "Dijkstra’s algorithm");
        Ok(("Generated".into(), "Preview body".into()))
    }));
    let response = json_of(run(workflows::regenerate(
        State(state.clone()),
        Path(id.clone()),
        Json(serde_json::from_value(serde_json::json!({"template":"meeting"})).unwrap()),
    )));
    assert_eq!(response["body"], "Preview body");
    assert!(response["base"].is_string());
    store_now(&state, |store| {
        assert_eq!(store.find_note(&id).unwrap().body, "Existing note");
        Ok(())
    })
    .unwrap();
}
#[test]
fn scoped_tools_cannot_open_or_search_notes_outside_the_selected_folder() {
    let (state, _d, ids) = state_with(&[("Included", "cs130"), ("Private", "other")]);
    store_now(&state, |store| {
        store.find_note_mut(&ids[0]).unwrap().body = "queue".into();
        store.find_note_mut(&ids[1]).unwrap().body = "queue secret".into();
        let call = |args: serde_json::Value| crate::tools::Call {
            name: args["name"].as_str().unwrap().into(),
            args,
        };
        let mut desk =
            crate::tools::Desk::new(vec![], crate::chat::ROOM).with_scope(crate::chat::Scope {
                folder: Some("cs130".into()),
                ..Default::default()
            });
        let cache = crate::graph::Cache::default();
        let result = desk.run(
            store,
            &cache,
            &call(serde_json::json!({"name":"open_note","note":"Private"})),
        );
        assert!(!result.result.contains("queue secret"));
        let result = desk.run(
            store,
            &cache,
            &call(serde_json::json!({"name":"search_notes","query":"queue"})),
        );
        assert!(!result.result.contains("Private"));
        Ok(())
    })
    .unwrap();
}
