use super::*;
use crate::routes::combine;

fn bodies(state: &AppState, ids: &[String], texts: [&str; 2]) {
    store_now(state, |store| {
        for (id, text) in ids.iter().zip(texts) {
            store.find_note_mut(id).unwrap().body = text.into();
        }
        store.save().unwrap();
        Ok(())
    })
    .unwrap();
}

fn combined(state: &AppState, into: &str, with: &str) -> Response {
    run(combine::combine(
        State(state.clone()),
        Path(into.to_string()),
        Json(serde_json::from_value(serde_json::json!({ "with": with })).unwrap()),
    ))
}

fn transcript(id: &str) -> leo_core::recording::Archive {
    leo_core::recording::Archive {
        id: id.into(),
        started: chrono::Utc::now(),
        passages: vec![leo_core::recording::Passage {
            start_secs: 0,
            end_secs: 5,
            text: format!("said in {id}"),
            speaker: String::new(),
        }],
        points: vec![],
        context: String::new(),
        wants: String::new(),
        warnings: vec![],
        trace: vec![],
        from: String::new(),
    }
}

#[test]
fn combining_keeps_both_notes_whole_and_both_transcripts_at_once_without_an_ai() {
    let (state, _d, ids) = state_with(&[("Graphs", ""), ("BFS", "")]);
    bodies(
        &state,
        &ids,
        ["A graph has nodes and edges.", "BFS uses a queue."],
    );
    let notes = state.fresh().notes_dir.clone();
    leo_core::recording::save(&notes, &ids[0], &transcript("session-graphs")).unwrap();
    leo_core::recording::save(&notes, &ids[1], &transcript("session-bfs01")).unwrap();
    let saved = json_of(combined(&state, &ids[0], &ids[1]));
    assert_eq!(saved["before"], "A graph has nodes and edges.");
    assert_eq!(saved["removed"], ids[1].as_str());
    assert_eq!(saved["sources"], serde_json::json!(["session-bfs01"]));
    let store = state.fresh();
    let body = store.find_note(&ids[0]).unwrap().body.clone();
    assert!(
        body.starts_with(
            "## Graphs\n\nA graph has nodes and edges.\n\n---\n\n## BFS\n\nBFS uses a queue."
        ),
        "{body}"
    );
    assert!(store.find_note(&ids[1]).is_none());
    assert!(
        store.trashed().iter().any(|t| t.id == ids[1]),
        "the other note can be restored"
    );
    drop(store);
    let both = leo_core::recording::load(&notes, &ids[0]).unwrap();
    assert_eq!(both.len(), 2);
    assert!(both
        .iter()
        .any(|s| s.id == "session-bfs01" && s.from == "BFS"));
    let undone = run(combine::uncombine(
        State(state.clone()),
        Path(ids[0].clone()),
        Json(serde_json::from_value(serde_json::json!({ "sources": saved["sources"] })).unwrap()),
    ));
    assert_eq!(undone.status(), StatusCode::NO_CONTENT);
    assert_eq!(leo_core::recording::load(&notes, &ids[0]).unwrap().len(), 1);
}

#[test]
fn combining_needs_two_different_notes() {
    let (state, _d, ids) = state_with(&[("Graphs", ""), ("BFS", "")]);
    assert_eq!(
        combined(&state, &ids[0], &ids[0]).status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        combined(&state, &ids[0], "nope").status(),
        StatusCode::NOT_FOUND
    );
    assert!(state.fresh().find_note(&ids[1]).is_some());
}
