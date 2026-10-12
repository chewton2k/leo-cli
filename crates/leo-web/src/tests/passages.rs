use super::*;
use crate::routes::insert::{insert_into_note, InsertBody};
use crate::routes::notes::version_of;
use crate::routes::related::{get_reading, put_reading, related_notes, ReadingBody, RelatedQuery};

fn body_of(state: &AppState, id: &str) -> (String, String) {
    let store = state.fresh();
    let note = store.notes.iter().find(|n| n.id == id).unwrap();
    (note.body.clone(), version_of(note))
}

fn insert(state: &AppState, id: &str, body: serde_json::Value) -> Response {
    let body: InsertBody = serde_json::from_value(body).unwrap();
    run(insert_into_note(
        State(state.clone()),
        Path(id.to_string()),
        Json(body),
    ))
}

#[test]
fn a_passage_is_inserted_once_per_request_and_only_into_the_version_it_was_chosen_for() {
    let (state, _d, ids) = state_with(&[("Collect", "")]);
    {
        let mut store = state.fresh();
        store.find_note_mut(&ids[0]).unwrap().body = "Line one\nLine two".into();
        store.save().unwrap();
    }
    let (_, base) = body_of(&state, &ids[0]);
    let first = insert(
        &state,
        &ids[0],
        serde_json::json!({"text": "Kept **bit**", "line": 1, "base": base, "request": "req-1"}),
    );
    assert_eq!(first.status(), 200);
    let first = json_of(first);
    assert_eq!(first["before"], "Line one\nLine two");
    assert_eq!(
        body_of(&state, &ids[0]).0,
        "Line one\n\nKept **bit**\n\nLine two"
    );
    let again = json_of(insert(
        &state,
        &ids[0],
        serde_json::json!({"text": "Kept **bit**", "line": 1, "base": base, "request": "req-1"}),
    ));
    assert_eq!(
        again["version"], first["version"],
        "the same press inserts once"
    );
    assert_eq!(
        body_of(&state, &ids[0]).0,
        "Line one\n\nKept **bit**\n\nLine two"
    );
    let stale = insert(
        &state,
        &ids[0],
        serde_json::json!({"text": "Other", "base": base, "request": "req-2"}),
    );
    assert_eq!(stale.status(), 409);
    assert_eq!(
        insert(
            &state,
            &ids[0],
            serde_json::json!({"text": "  ", "base": base})
        )
        .status(),
        400
    );
}

#[test]
fn related_notes_come_from_the_graph_and_wiki_links_without_the_note_itself() {
    let (state, _d, ids) = state_with(&[("Heaps", ""), ("Graphs", ""), ("Sorting", "")]);
    {
        let mut store = state.fresh();
        store.find_note_mut(&ids[0]).unwrap().body = "See [[Graphs]] and [[Heaps]].".into();
        store.save().unwrap();
    }
    let found = json_of(run(related_notes(
        State(state.clone()),
        Path(ids[0].clone()),
        Query(RelatedQuery { limit: Some(4) }),
    )));
    assert_eq!(found["notes"].as_array().unwrap().len(), 1);
    assert_eq!(found["notes"][0]["title"], "Graphs");
    assert_eq!(found["notes"][0]["why"], "Linked from this note");
    assert_eq!(found["more"], false);
    let missing = run(related_notes(
        State(state.clone()),
        Path("nope".into()),
        Query(RelatedQuery { limit: None }),
    ));
    assert_eq!(missing.status(), 404);
}

#[test]
fn a_reading_position_is_kept_per_note() {
    let (state, _d, ids) = state_with(&[("Long read", "")]);
    let empty = json_of(run(get_reading(State(state.clone()), Path(ids[0].clone()))));
    assert_eq!(empty["anchor"], serde_json::Value::Null);
    let anchor = serde_json::json!({"line": 12, "text": "Dijkstra", "dy": 40});
    let put = run(put_reading(
        State(state.clone()),
        Path(ids[0].clone()),
        Json(
            serde_json::from_value::<ReadingBody>(serde_json::json!({ "anchor": anchor })).unwrap(),
        ),
    ));
    assert_eq!(put.status(), 204);
    let got = json_of(run(get_reading(State(state.clone()), Path(ids[0].clone()))));
    assert_eq!(got["anchor"], anchor);
    let huge = serde_json::json!({ "anchor": { "text": "x".repeat(4000) } });
    let refused = run(put_reading(
        State(state.clone()),
        Path(ids[0].clone()),
        Json(serde_json::from_value::<ReadingBody>(huge).unwrap()),
    ));
    assert_eq!(refused.status(), 400);
}
