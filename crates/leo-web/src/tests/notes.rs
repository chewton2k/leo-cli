use super::*;

/// An ID prefix shared by several notes must not delete all of them.
#[test]
fn deleting_by_an_ambiguous_prefix_deletes_nothing() {
    let (state, _d, _ids) = state_with(&[("A", ""), ("B", "")]);
    let prefix = {
        let mut store = state.fresh();
        let first = store.notes[0].id.clone();
        store.notes[1].id = format!("{first}x");
        store.save().unwrap();
        first
    };
    let status = run(delete_note(State(state.clone()), Path(prefix)));
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(state.fresh().notes.len(), 2);
}

#[test]
fn creating_a_note_in_a_new_directory_registers_it() {
    let (state, _d, _ids) = state_with(&[]);
    let body = CreateBody {
        title: "Lecture".to_string(),
        body: None,
        tags: None,
        directory: Some("cs162".to_string()),
    };
    run(create_note(State(state.clone()), Json(body)))
        .ok()
        .unwrap();
    assert!(state.fresh().dir_exists("cs162"));
}

#[test]
fn moving_to_a_missing_directory_is_refused() {
    let (state, _d, ids) = state_with(&[("A", "")]);
    let body = MoveBody {
        directory: "nowhere".to_string(),
    };
    let out = run(move_note(
        State(state.clone()),
        Path(ids[0].clone()),
        Json(body),
    ));
    assert_eq!(out.err(), Some(StatusCode::NOT_FOUND));
    assert_eq!(
        state
            .store
            .lock()
            .unwrap()
            .find_note(&ids[0])
            .unwrap()
            .directory,
        ""
    );
}

#[test]
fn a_note_can_be_pinned_from_the_phone() {
    let (state, _d, ids) = state_with(&[("Syllabus", "")]);
    let body = UpdateBody {
        title: None,
        body: None,
        tags: None,
        pinned: Some(true),
        base: None,
    };
    let Json(note) = run(update_note(
        State(state.clone()),
        Path(ids[0].clone()),
        Json(body),
    ))
    .unwrap();
    assert!(note.pinned);
    assert!(state.fresh().find_note(&ids[0]).unwrap().pinned);
}

#[test]
fn folders_come_with_how_many_notes_they_hold() {
    let (state, _d, _ids) = state_with(&[("A", "cs130"), ("B", "cs130/lec"), ("C", "")]);
    let Json(dirs) = run(list_dirs(State(state), Query(DirParams { parent: None }))).unwrap();
    assert_eq!(dirs.len(), 1);
    assert_eq!(dirs[0].name, "cs130");
    assert_eq!(dirs[0].notes, 2);
}

#[test]
fn every_folder_is_listed_for_moving_a_note() {
    let (state, _d, _ids) = state_with(&[("A", "cs130"), ("B", "cs130/lec"), ("C", "Ideas")]);
    let Json(all) = run(list_folders(State(state))).unwrap();
    let names: Vec<&str> = all.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, ["cs130", "cs130/lec", "Ideas"]);
}

fn edit(
    state: &AppState,
    id: &str,
    body: &str,
    base: Option<String>,
) -> Result<NoteResponse, StatusCode> {
    run(update_note(
        State(state.clone()),
        Path(id.to_string()),
        Json(UpdateBody {
            title: None,
            body: Some(body.to_string()),
            tags: None,
            pinned: None,
            base,
        }),
    ))
    .map(|Json(n)| n)
}

#[test]
fn an_edit_based_on_the_current_version_is_saved_and_returns_the_next_version() {
    let (state, _d, ids) = state_with(&[("Shared", "")]);
    let current = run(get_note(State(state.clone()), Path(ids[0].clone())))
        .unwrap()
        .0
        .version;
    let saved = edit(&state, &ids[0], "from the phone", Some(current.clone())).unwrap();
    assert_eq!(saved.body, "from the phone");
    assert_ne!(saved.version, current);
}

#[test]
fn an_edit_based_on_an_old_version_is_refused_and_changes_nothing() {
    let (state, dir, ids) = state_with(&[("Shared", "")]);
    let seen = run(get_note(State(state.clone()), Path(ids[0].clone())))
        .unwrap()
        .0;
    let file = dir.path().join("notes").join("Shared.md");
    let text = std::fs::read_to_string(&file).unwrap();
    std::fs::write(&file, format!("{text}typed in Obsidian\n")).unwrap();

    let refused = edit(&state, &ids[0], "from the phone", Some(seen.version));
    assert_eq!(refused.unwrap_err(), StatusCode::CONFLICT);
    assert!(std::fs::read_to_string(&file)
        .unwrap()
        .contains("typed in Obsidian"));
}
#[test]
fn web_note_creation_directory_creation_and_moves_reject_traversal() {
    let (state, dir, ids) = state_with(&[("A", "")]);
    for path in [
        "../outside",
        "/tmp/outside",
        "nested/../../outside",
        "C:\\outside",
        ".trash",
    ] {
        let created = run(create_note(
            State(state.clone()),
            Json(CreateBody {
                title: "Escape".into(),
                body: None,
                tags: None,
                directory: Some(path.into()),
            }),
        ));
        assert_eq!(created.err(), Some(StatusCode::BAD_REQUEST));
        assert_eq!(
            run(create_dir(
                State(state.clone()),
                Json(CreateDirBody { path: path.into() })
            )),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            run(move_note(
                State(state.clone()),
                Path(ids[0].clone()),
                Json(MoveBody {
                    directory: path.into()
                })
            ))
            .err(),
            Some(StatusCode::BAD_REQUEST)
        );
    }
    assert!(!dir.path().join("outside").exists());
    assert_eq!(state.fresh().notes.len(), 1);
}

#[test]
fn a_reload_failure_is_reported_instead_of_serving_a_stale_store() {
    let (state, dir, _) = state_with(&[("A", "")]);
    std::fs::write(dir.path().join("notes/directories.json"), "not json").unwrap();
    let result = run(list_notes(
        State(state),
        Query(ListParams {
            tag: None,
            limit: None,
            dir: None,
        }),
    ));
    assert_eq!(result.err(), Some(StatusCode::INTERNAL_SERVER_ERROR));
}

#[test]
fn an_empty_folder_created_by_another_app_is_visible_without_restarting() {
    let (state, dir, _) = state_with(&[]);
    let mut other = Store::load_from(&dir.path().join("notes")).unwrap();
    other.create_dir("new-folder");
    other.save_files().unwrap();
    let Json(dirs) = run(list_dirs(State(state), Query(DirParams { parent: None }))).unwrap();
    assert_eq!(dirs[0].name, "new-folder");
}
#[test]
fn failed_operations_do_not_leave_unsaved_changes_in_the_cached_store() {
    let (state, _dir, ids) = state_with(&[("Original", "")]);
    let id = ids[0].clone();
    let failed: Result<(), StatusCode> = run(state.with_store(move |store| {
        store.find_note_mut(&id).unwrap().body = "Never saved".into();
        Err(StatusCode::INTERNAL_SERVER_ERROR)
    }));
    assert!(failed.is_err());
    let Json(note) = run(get_note(State(state), Path(ids[0].clone()))).unwrap();
    assert_eq!(note.body, "");
}
