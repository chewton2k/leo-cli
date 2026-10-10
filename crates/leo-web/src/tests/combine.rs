use super::*;
use crate::routes::combine;

fn with_writer(state: &mut AppState, reply: &'static str) -> Arc<Mutex<Vec<String>>> {
    let asked = Arc::new(Mutex::new(Vec::new()));
    let heard = Arc::clone(&asked);
    let notes = state.fresh().notes_dir.clone();
    state.graphs = Arc::new(graph::Graphs::for_notes(
        &notes,
        Some(Arc::new(move |system: &str, user: &str, _| {
            heard.lock().unwrap().push(format!("{system}\n{user}"));
            Ok(reply.to_string())
        })),
    ));
    asked
}

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

fn preview(state: &AppState, into: &str, with: &str) -> Response {
    run(combine::preview(
        State(state.clone()),
        Path(into.to_string()),
        Json(serde_json::from_value(serde_json::json!({ "with": with })).unwrap()),
    ))
}

fn keep(state: &AppState, into: &str, value: serde_json::Value) -> Response {
    run(combine::keep(
        State(state.clone()),
        Path(into.to_string()),
        Json(serde_json::from_value(value).unwrap()),
    ))
}

#[test]
fn two_notes_become_one_preview_that_keeps_everything_then_saving_trashes_the_other() {
    let (mut state, _d, ids) = state_with(&[("Graphs", ""), ("BFS", "")]);
    bodies(
        &state,
        &ids,
        [
            "A graph has nodes and edges.",
            "BFS uses a queue.\n\n- [x] read chapter 3\n\n![Tree](attachments/tree.png)",
        ],
    );
    let asked = with_writer(
        &mut state,
        "## Graphs\nA graph has nodes and edges.\n\n## BFS\nBFS uses a queue.\n\n- [x] read chapter 3",
    );
    let page = json_of(preview(&state, &ids[0], &ids[1]));
    assert_eq!(page["title"], "Graphs");
    assert_eq!(page["with_title"], "BFS");
    let body = page["body"].as_str().unwrap().to_string();
    assert!(body.contains("![Tree](attachments/tree.png)"), "{body}");
    assert_eq!(page["added"].as_array().unwrap().len(), 1);
    assert_eq!(page["kept_enough"], true);
    let prompt = asked.lock().unwrap()[0].clone();
    assert!(
        prompt.contains("A graph has nodes and edges.") && prompt.contains("BFS uses a queue.")
    );
    assert!(
        state.fresh().find_note(&ids[1]).is_some(),
        "a preview changes nothing"
    );

    let saved = json_of(keep(
        &state,
        &ids[0],
        serde_json::json!({ "with": ids[1], "body": body, "base": page["base"], "with_base": page["with_base"] }),
    ));
    assert_eq!(saved["before"], "A graph has nodes and edges.");
    assert_eq!(saved["removed"], ids[1].as_str());
    let store = state.fresh();
    assert!(store.find_note(&ids[1]).is_none());
    assert!(store
        .find_note(&ids[0])
        .unwrap()
        .body
        .contains("BFS uses a queue."));
    assert!(
        store.trashed().iter().any(|t| t.id == ids[1]),
        "the other note can be restored"
    );
}

#[test]
fn a_note_changed_while_combining_is_not_overwritten() {
    let (mut state, _d, ids) = state_with(&[("Graphs", ""), ("BFS", "")]);
    bodies(&state, &ids, ["edges", "queue"]);
    with_writer(&mut state, "edges and queue");
    let page = json_of(preview(&state, &ids[0], &ids[1]));
    bodies(&state, &ids, ["edges, edited meanwhile", "queue"]);
    let refused = keep(
        &state,
        &ids[0],
        serde_json::json!({ "with": ids[1], "body": page["body"], "base": page["base"], "with_base": page["with_base"] }),
    );
    assert_eq!(refused.status(), StatusCode::CONFLICT);
    let store = state.fresh();
    assert_eq!(
        store.find_note(&ids[0]).unwrap().body,
        "edges, edited meanwhile"
    );
    assert!(store.find_note(&ids[1]).is_some());
}

#[test]
fn combining_needs_an_ai_and_two_different_notes() {
    let (mut state, _d, ids) = state_with(&[("Graphs", ""), ("BFS", "")]);
    assert_eq!(
        preview(&state, &ids[0], &ids[1]).status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    with_writer(&mut state, "x");
    assert_eq!(
        preview(&state, &ids[0], &ids[0]).status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        preview(&state, &ids[0], "nope").status(),
        StatusCode::NOT_FOUND
    );
}
