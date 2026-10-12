use super::*;
use crate::routes::tasks::{
    change_task, create_task, delete_task, import_tasks, list_tasks, promote_task, task_ics,
};

fn change(state: &AppState, id: &str, body: serde_json::Value) -> Response {
    run(change_task(
        State(state.clone()),
        Path(id.to_string()),
        Json(serde_json::from_value(body).unwrap()),
    ))
}

fn listed(state: &AppState) -> Vec<serde_json::Value> {
    json_of(run(list_tasks(State(state.clone()))))["tasks"]
        .as_array()
        .unwrap()
        .clone()
}

fn body_of(state: &AppState, id: &str) -> String {
    state
        .fresh()
        .notes
        .iter()
        .find(|n| n.id == id)
        .unwrap()
        .body
        .clone()
}

#[test]
fn note_action_items_are_ticked_by_desired_state_and_dated_in_the_note() {
    let (state, _d, ids) = state_with(&[("Lecture 5", "bio")]);
    {
        let mut store = state.fresh();
        store.find_note_mut(&ids[0]).unwrap().body = "## Action items\n- [ ] Submit HW 4\n- [ ] Essay due Oct 16\n## Check yourself\n- [ ] What is a cell?".into();
        store.save().unwrap();
    }
    let tasks = listed(&state);
    assert_eq!(tasks.len(), 2, "{tasks:?}");
    let hw = tasks.iter().find(|t| t["title"] == "Submit HW 4").unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    for _ in 0..2 {
        assert_eq!(
            change(&state, &hw, serde_json::json!({"done": true})).status(),
            200
        );
    }
    assert!(
        body_of(&state, &ids[0]).contains("- [x] Submit HW 4"),
        "a repeated tick stays ticked"
    );
    let dated = json_of(change(
        &state,
        &hw,
        serde_json::json!({"date": "2026-10-16", "time": "23:59"}),
    ));
    assert_eq!(dated["date"], "2026-10-16");
    assert_eq!(
        dated["id"],
        hw.as_str(),
        "the task keeps its id after its text changes"
    );
    assert!(body_of(&state, &ids[0])
        .contains("- [x] Submit HW 4 — due Friday, October 16, 2026 at 11:59 pm."));
    let essay = listed(&state)
        .iter()
        .find(|t| t["title"] == "Essay due Oct 16")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let unsure = change(&state, &essay, serde_json::json!({"date": "2026-10-16"}));
    assert_eq!(unsure.status(), 409);
    let preview = json_of(unsure)["preview"].clone();
    assert_eq!(
        preview["after"],
        "Essay due Oct 16 — due Friday, October 16, 2026."
    );
    assert!(
        body_of(&state, &ids[0]).contains("- [ ] Essay due Oct 16\n"),
        "nothing changes before confirming"
    );
    assert_eq!(
        change(
            &state,
            &essay,
            serde_json::json!({"date": "2026-10-16", "confirm": true})
        )
        .status(),
        200
    );
    assert!(body_of(&state, &ids[0]).contains("Essay due Oct 16 — due Friday, October 16, 2026."));
    assert_eq!(
        run(delete_task(State(state.clone()), Path(hw.clone()))).status(),
        409
    );
}

#[test]
fn a_promoted_checkbox_and_a_standalone_task_live_and_leave_cleanly() {
    let (state, _d, ids) = state_with(&[("Plans", "")]);
    {
        let mut store = state.fresh();
        store.find_note_mut(&ids[0]).unwrap().body =
            "Ideas\n- [ ] Ask tutor about the exam\nEnd".into();
        store.save().unwrap();
    }
    assert!(listed(&state).is_empty(), "plain checkboxes are not tasks");
    let promoted = run(promote_task(
        State(state.clone()),
        Json(serde_json::from_value(serde_json::json!({"note": ids[0], "line": 1})).unwrap()),
    ));
    assert_eq!(promoted.status(), 201);
    let promoted = json_of(promoted);
    assert_eq!(promoted["kind"], "exam");
    assert_eq!(listed(&state).len(), 1);
    let id = promoted["id"].as_str().unwrap().to_string();
    assert_eq!(
        run(delete_task(State(state.clone()), Path(id))).status(),
        204
    );
    assert!(listed(&state).is_empty());
    assert_eq!(
        body_of(&state, &ids[0]),
        "Ideas\n- [ ] Ask tutor about the exam\nEnd",
        "unpromoting leaves the note alone"
    );

    let made = run(create_task(State(state.clone()), Json(serde_json::from_value(serde_json::json!({"title": "  Book dentist  ", "date": "2026-11-02", "time": "9:30"})).unwrap())));
    assert_eq!(made.status(), 201);
    let made = json_of(made);
    assert_eq!(
        (made["title"].as_str(), made["time"].as_str()),
        (Some("Book dentist"), Some("09:30:00"))
    );
    let id = made["id"].as_str().unwrap().to_string();
    assert_eq!(
        change(&state, &id, serde_json::json!({"date": null})).status(),
        200
    );
    assert_eq!(listed(&state)[0]["date"], serde_json::Value::Null);
    assert_eq!(
        run(task_ics(State(state.clone()), Path(id.clone()))).status(),
        400,
        "an undated task asks for a date first"
    );
    change(&state, &id, serde_json::json!({"date": "2026-11-02"}));
    let ics = run(task_ics(State(state.clone()), Path(id.clone())));
    let text = String::from_utf8(
        run(axum::body::to_bytes(ics.into_body(), usize::MAX))
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(text.contains("SUMMARY:Book dentist") && text.contains("DTSTART;VALUE=DATE:20261102"));
    let removed = json_of(run(delete_task(State(state.clone()), Path(id.clone()))));
    assert!(listed(&state).is_empty());
    let again = run(import_tasks(
        State(state.clone()),
        Json(
            serde_json::from_value(serde_json::json!({"tasks": [removed.clone(), removed]}))
                .unwrap(),
        ),
    ));
    assert_eq!(
        json_of(again)["added"],
        1,
        "importing the same task twice adds it once"
    );
    assert_eq!(listed(&state).len(), 1);
    assert_eq!(
        run(create_task(
            State(state.clone()),
            Json(serde_json::from_value(serde_json::json!({"title": " "})).unwrap())
        ))
        .status(),
        400
    );
}
