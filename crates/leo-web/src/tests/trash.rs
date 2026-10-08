use super::*;

#[test]
fn a_deleted_note_is_in_the_trash_and_can_be_restored() {
    let (state, _d, ids) = state_with(&[("Lecture 4", "cs130")]);
    assert_eq!(
        run(delete_note(State(state.clone()), Path(ids[0].clone()))),
        StatusCode::NO_CONTENT
    );
    let Json(trash) = run(list_trash(State(state.clone()))).unwrap();
    assert_eq!(trash.len(), 1);
    assert_eq!(trash[0].title, "Lecture 4");
    assert_eq!(trash[0].directory, "cs130");

    let Json(back) = run(restore_note(State(state.clone()), Path(ids[0].clone()))).unwrap();
    assert_eq!(back.directory, "cs130");
    assert!(state.fresh().find_note(&ids[0]).is_some());
    let Json(trash) = run(list_trash(State(state.clone()))).unwrap();
    assert!(trash.is_empty());
    assert_eq!(
        run(restore_note(State(state), Path(ids[0].clone()))).unwrap_err(),
        StatusCode::NOT_FOUND
    );
}

#[test]
fn chosen_notes_and_folders_go_to_the_trash_and_the_root_is_refused() {
    let (state, _d, ids) = state_with(&[
        ("Keep", ""),
        ("Loose", ""),
        ("Graphs", "cs130"),
        ("Deep", "cs130/week1"),
        ("Other", "math"),
    ]);
    let moved = run(move_to_trash(
        State(state.clone()),
        Json(TrashMove {
            notes: vec![ids[1].clone(), "missing".into()],
            dirs: vec!["cs130".into()],
        }),
    ))
    .unwrap();
    assert_eq!(moved.0["notes"], 3);
    assert_eq!(moved.0["folders"], 1);
    let store = state.fresh();
    let left: Vec<&str> = store.notes.iter().map(|n| n.title.as_str()).collect();
    assert_eq!(left.len(), 2, "{left:?}");
    assert!(left.contains(&"Keep") && left.contains(&"Other"));
    assert!(!store.dir_exists("cs130"));
    assert_eq!(store.trashed().len(), 3);
    drop(store);
    for bad in ["", "/", "../outside"] {
        let refused = run(move_to_trash(
            State(state.clone()),
            Json(TrashMove {
                notes: vec![],
                dirs: vec![bad.into()],
            }),
        ));
        assert_eq!(refused.unwrap_err(), StatusCode::BAD_REQUEST, "{bad:?}");
    }
}
