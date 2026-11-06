use super::*;

#[test]
fn study_quiz_marking_keeps_quick_writer_validation_and_verdicts() {
    let (mut state, _dir, _) = state_with(&[]);
    for correct in [false, true] {
        state.quick = Some(Arc::new(move |system, material, _| {
            assert_eq!(system, crate::routes::felix::MARK);
            assert!(material.contains("<student_answer>"));
            Ok(serde_json::json!({"correct": correct, "feedback": "The queue preserves arrival order."}).to_string())
        }));
        let response = run(crate::routes::felix::mark_quiz(
            State(state.clone()),
            Json(serde_json::from_value(serde_json::json!({
                "question": "Why does BFS use a queue?", "answer": "To explore in discovery order.", "given": "It keeps the oldest discovered node first."
            })).unwrap()),
        ));
        assert_eq!(response.status(), StatusCode::OK);
        let body = json_of(response);
        assert_eq!(body["correct"], correct);
        assert!(body["feedback"].as_str().is_some_and(|s| !s.is_empty()));
    }
    let response = run(crate::routes::felix::mark_quiz(
        State(state.clone()),
        Json(serde_json::from_value(serde_json::json!({"question": "Why a queue?", "answer": "Arrival order", "given": ""})).unwrap()),
    ));
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[test]
fn retired_interview_database_rows_are_preserved() {
    let (state, _dir, _) = state_with(&[]);
    let db = crate::db::for_chats(&state.chats);
    db.with(|c| {
        c.execute("INSERT INTO interviews (id, updated, data) VALUES ('retired-practice', '2026-11-05', '{\"kept\":true}')", [])?;
        c.execute("INSERT INTO interview_keys (interview, challenge, status, updated, data) VALUES ('retired-practice', 'problem', 'ready', '2026-11-05', '{\"kept\":true}')", [])?;
        c.execute("INSERT INTO interview_problem_undo (id, interview, updated, data) VALUES ('retired-undo', 'retired-practice', '2026-11-05', '{\"kept\":true}')", [])?;
        Ok(())
    }).unwrap();
    crate::chats::list(&state.chats);
    for table in ["interviews", "interview_keys", "interview_problem_undo"] {
        assert_eq!(
            db.with(
                |c| c.query_row(&format!("SELECT data FROM {table}"), [], |r| r
                    .get::<_, String>(0))
            )
            .unwrap(),
            "{\"kept\":true}"
        );
    }
}
