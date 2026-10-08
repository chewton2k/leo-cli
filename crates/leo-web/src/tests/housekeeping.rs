use super::*;

#[test]
fn keeping_choices_are_saved_and_applied_right_away() {
    let (state, _d, _ids) = state_with(&[]);
    let old = chrono::Utc::now() - chrono::Duration::days(45);
    chats::save(
        &state.chats,
        "chat-old-0001",
        chats::Saving {
            title: String::new(),
            mode: String::new(),
            refs: vec![],
            messages: vec![serde_json::json!({"role": "user", "text": "hi"})],
        },
        old,
    )
    .unwrap();
    let page = run(get_keep(State(state.clone()))).unwrap().0;
    assert_eq!(page["trash_days"], 30);
    assert!(
        page["chat_days"].is_null(),
        "chats are kept forever unless asked"
    );
    assert_eq!(page["chat_choices"].as_array().unwrap().len(), 4);

    let bad = run(set_keep(
        State(state.clone()),
        Json(leo_core::keep::Keep {
            trash_days: Some(3),
            chat_days: None,
        }),
    ));
    assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
    assert_eq!(chats::list(&state.chats).len(), 1);

    let set = run(set_keep(
        State(state.clone()),
        Json(leo_core::keep::Keep {
            trash_days: None,
            chat_days: Some(30),
        }),
    ));
    assert_eq!(json_of(set)["chat_days"], 30);
    assert!(
        chats::list(&state.chats).is_empty(),
        "a 45-day-old chat goes at once"
    );
    let notes_dir = state.fresh().notes_dir.clone();
    assert_eq!(leo_core::keep::load(&notes_dir).trash_days, None);
}
