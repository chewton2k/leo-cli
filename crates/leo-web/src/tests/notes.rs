use super::*;

#[test]
fn a_failed_read_does_not_reload_the_store_and_a_failed_write_does() {
    let (state, _dir, _ids) = state_with(&[("One", "")]);
    let missed: Result<(), StatusCode> = run(state.read_store(|_| Err(StatusCode::NOT_FOUND)));
    assert_eq!(missed, Err(StatusCode::NOT_FOUND));
    assert!(!state.store.lock().unwrap().reload);
    let failed: Result<(), StatusCode> =
        run(state.with_store(|_| Err(StatusCode::INTERNAL_SERVER_ERROR)));
    assert_eq!(failed, Err(StatusCode::INTERNAL_SERVER_ERROR));
    assert!(state.store.lock().unwrap().reload);
}

#[test]
fn a_panic_inside_the_store_reloads_it_from_disk_instead_of_failing_every_request() {
    let (state, _dir, ids) = state_with(&[("One", "")]);
    let id = ids[0].clone();
    let panicked: Result<(), StatusCode> = run(state.with_store(move |store| {
        store.find_note_mut(&id).unwrap().body = "Never saved".into();
        panic!("simulated failure inside the store");
    }));
    assert_eq!(panicked, Err(StatusCode::INTERNAL_SERVER_ERROR));
    assert!(state.store.is_poisoned());
    let bodies = run(state.read_store(|store| {
        Ok(store
            .notes
            .iter()
            .map(|note| note.body.clone())
            .collect::<Vec<_>>())
    }))
    .unwrap();
    assert_eq!(bodies, vec![String::new()]);
    assert!(!state.store.is_poisoned());
    let id = ids[0].clone();
    let again = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _: Result<(), StatusCode> = store_now(&state, move |store| {
            store.find_note_mut(&id).unwrap().body = "Never saved".into();
            panic!("simulated failure inside the store");
        });
    }));
    assert!(again.is_err() && state.store.is_poisoned());
    let body = store_now(&state, |store| Ok(store.notes[0].body.clone())).unwrap();
    assert_eq!(body, "");
}

#[test]
fn saving_another_note_preserves_external_creates_and_edits_before_watch_delivery() {
    for creating in [false, true] {
        let (state, _dir, ids) = state_with(&[("Local", ""), ("External", "")]);
        let root = state.fresh().notes_dir.clone();
        state.store.lock().unwrap().changes = Some(crate::store_refresh::Changes::delayed());
        if creating {
            std::fs::write(root.join("Added.md"), "Outside edit").unwrap();
        } else {
            let path = root.join("External.md");
            let mut text = std::fs::read_to_string(&path).unwrap();
            text.push_str("Outside edit");
            std::fs::write(path, text).unwrap();
        }
        let local = ids[0].clone();
        run(state.with_store(move |store| {
            store.find_note_mut(&local).unwrap().body = "Saved in Leo".into();
            store.save().unwrap();
            Ok(())
        }))
        .unwrap();
        let seen = run(state.read_store(|store| {
            Ok(store
                .notes
                .iter()
                .any(|note| note.body.contains("Outside edit")))
        }))
        .unwrap();
        assert!(
            seen,
            "external create={creating} was absorbed into an unread fingerprint"
        );
    }
}

/// An ID prefix shared by several notes must not delete all of them.
#[test]
fn deleting_by_an_ambiguous_prefix_deletes_nothing() {
    let (state, _d, _ids) = state_with(&[("A", ""), ("B", "")]);
    let prefix = {
        let mut store = state.fresh();
        let first = store.notes[0].id.clone();
        store.notes[1].id = format!("{first}x");
        store.save().unwrap();
        first[..first.len() - 1].to_string()
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
        written: None,
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
            written: None,
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
fn a_new_folder_answers_with_json_so_the_page_can_read_it() {
    let (state, _dir, _) = state_with(&[]);
    let (status, Json(body)) = run(create_dir(
        State(state.clone()),
        Json(CreateDirBody {
            path: "cs130/week1".into(),
        }),
    ))
    .unwrap();
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body, serde_json::json!({ "path": "cs130/week1" }));
    assert_eq!(
        run(create_dir(
            State(state),
            Json(CreateDirBody {
                path: "cs130/week1".into(),
            }),
        ))
        .err(),
        Some(StatusCode::CONFLICT)
    );
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
            ))
            .err(),
            Some(StatusCode::BAD_REQUEST)
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
    let folders = dir.path().join("notes").join("directories.json");
    let _ = std::fs::remove_file(&folders);
    std::fs::create_dir(&folders).unwrap();
    let result = run(list_notes(
        State(state),
        Query(ListParams {
            tag: None,
            limit: None,
            dir: None,
            offset: 0,
            brief: false,
            sort: None,
            recent: None,
            show: None,
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

#[test]
fn a_large_folder_comes_in_pages_with_its_total_and_short_cards() {
    let names: Vec<String> = (0..250).map(|i| format!("Note {i:03}")).collect();
    let notes: Vec<(&str, &str)> = names.iter().map(|n| (n.as_str(), "big")).collect();
    let (state, _d, ids) = state_with(&notes);
    {
        let mut store = state.fresh();
        let body = format!("- [x] read\n- [ ] write\n{}", "long text ".repeat(200));
        store.find_note_mut(&ids[0]).unwrap().body = body;
        store.save().unwrap();
    }
    let page = |offset: usize, limit: usize| {
        let response = run(list_notes(
            State(state.clone()),
            Query(ListParams {
                tag: None,
                limit: Some(limit),
                dir: Some("big".into()),
                offset,
                brief: true,
                sort: None,
                recent: None,
                show: None,
            }),
        ))
        .unwrap();
        let total: usize = response.headers()["x-total"]
            .to_str()
            .unwrap()
            .parse()
            .unwrap();
        (total, json_of(response))
    };
    let (total, first) = page(0, 200);
    assert_eq!(total, 250);
    assert_eq!(first.as_array().unwrap().len(), 200);
    let (_, rest) = page(200, 200);
    assert_eq!(rest.as_array().unwrap().len(), 50);
    let mut seen: Vec<String> = first
        .as_array()
        .unwrap()
        .iter()
        .chain(rest.as_array().unwrap())
        .map(|n| n["id"].as_str().unwrap().to_string())
        .collect();
    seen.sort();
    seen.dedup();
    assert_eq!(
        seen.len(),
        250,
        "no note is skipped or repeated across pages"
    );
    let long = first
        .as_array()
        .unwrap()
        .iter()
        .chain(rest.as_array().unwrap())
        .find(|n| n["id"] == ids[0].as_str())
        .unwrap();
    assert_eq!(long["tasks"], serde_json::json!([1, 2]));
    assert_eq!(
        long["body"].as_str().unwrap().chars().count(),
        EXCERPT_CHARS
    );
}

#[test]
fn an_excerpt_starts_near_the_first_match_and_counts_only_real_tasks() {
    let body = format!("{}needle in the text", "é".repeat(500));
    let cut = excerpt(&body, &["needle".into()]);
    assert!(
        cut.starts_with('…') && cut.contains("needle in the text"),
        "{cut}"
    );
    assert_eq!(excerpt("short", &["absent".into()]), "short");
    let words = format!(
        "{} the queue holds work",
        "breadth first search ".repeat(10)
    );
    let cut = excerpt(&words, &["queue".into()]);
    assert!(
        ["…breadth ", "…first ", "…search "]
            .iter()
            .any(|w| cut.starts_with(w)),
        "{cut}"
    );
    assert_eq!(
        excerpt("İstanbul needle", &["needle".into()]),
        "İstanbul needle"
    );
    assert_eq!(
        tasks_in("- [x] a\n  - [X] b\n- [ ] c\n- [y] no\n-[ ] no\n* [ ] other bullet"),
        [2, 3]
    );
}

#[test]
fn a_folder_moves_into_another_with_its_notes_and_leaves_no_empty_folder_behind() {
    let (state, dir, ids) = state_with(&[("Lecture", "cs130/week1"), ("Other", "cs162")]);
    let moved = |from: &str, into: &str| {
        run(move_dir(
            State(state.clone()),
            Json(MoveDirBody {
                from: from.into(),
                into: into.into(),
            }),
        ))
    };
    let out = moved("cs130/week1", "cs162").unwrap();
    assert_eq!(out.0["path"], "cs162/week1");
    let store = state.fresh();
    assert_eq!(store.find_note(&ids[0]).unwrap().directory, "cs162/week1");
    assert!(!store.dir_exists("cs130/week1"));
    assert!(dir.path().join("notes/cs162/week1/Lecture.md").exists());
    assert!(!dir.path().join("notes/cs130/week1").exists());
    drop(store);

    assert_eq!(
        moved("cs162", "cs162/week1").err(),
        Some(StatusCode::CONFLICT)
    );
    assert_eq!(moved("nowhere", "cs162").err(), Some(StatusCode::NOT_FOUND));
    assert_eq!(moved("../outside", "").err(), Some(StatusCode::BAD_REQUEST));
    assert_eq!(
        moved("cs162", "../outside").err(),
        Some(StatusCode::BAD_REQUEST)
    );
    assert_eq!(moved("cs162/week1", "").unwrap().0["path"], "week1");
}

#[test]
fn folders_can_be_renamed_in_place_without_changing_note_identity() {
    let (state, dir, ids) = state_with(&[("Lecture", "cs130/week1"), ("Other", "cs130/week10")]);
    let rename = |from: &str, name: &str| {
        run(crate::routes::notes::rename_dir(
            State(state.clone()),
            Json(crate::routes::notes::RenameDirBody {
                from: from.into(),
                name: name.into(),
            }),
        ))
    };
    let out = rename("cs130/week1", "Week one").unwrap();
    assert_eq!(out.0["path"], "cs130/Week one");
    let store = state.fresh();
    assert_eq!(
        store.find_note(&ids[0]).unwrap().directory,
        "cs130/Week one"
    );
    assert_eq!(store.find_note(&ids[1]).unwrap().directory, "cs130/week10");
    assert!(dir.path().join("notes/cs130/Week one/Lecture.md").exists());
    assert!(!dir.path().join("notes/cs130/week1").exists());
    drop(store);
    assert_eq!(
        rename("cs130/Week one", "week10").err(),
        Some(StatusCode::CONFLICT)
    );
    for bad in ["../outside", "a/b", "a\\b", "", ".."] {
        assert_eq!(
            rename("cs130/Week one", bad).err(),
            Some(StatusCode::BAD_REQUEST)
        );
    }
    assert_eq!(rename("", "root").err(), Some(StatusCode::BAD_REQUEST));
    assert_eq!(rename("missing", "New").err(), Some(StatusCode::NOT_FOUND));
}

#[test]
fn library_sorting_happens_before_pagination_and_brief_results_include_visuals() {
    let (state, _dir, ids) = state_with(&[("Zebra", "B"), ("Alpha", "A"), ("Pinned", "")]);
    {
        let mut store = state.store.lock().unwrap();
        store.find_note_mut(&ids[2]).unwrap().pinned = true;
        store.find_note_mut(&ids[1]).unwrap().body =
            "# Lecture\nA useful description of this lecture.\n```mermaid\ngraph LR\n A --> B\n```"
                .into();
        store.save().unwrap();
    }
    let page = |sort: &str, offset: usize, recent: Option<String>| {
        run(list_notes(State(state.clone()), Query(serde_json::from_value(serde_json::json!({ "sort": sort, "offset": offset, "limit": 1, "brief": true, "recent": recent })).unwrap())))
    };
    for sort in ["title", "folder", "opened", "updated"] {
        let out = page(sort, 0, Some(ids[0].clone())).unwrap();
        assert_eq!(out.headers()["x-total"], "3");
        assert_eq!(json_of(out)[0]["id"], ids[2]);
    }
    let alpha = json_of(page("title", 1, None).unwrap());
    assert_eq!(alpha[0]["id"], ids[1]);
    assert_eq!(alpha[0]["summary"], "A useful description of this lecture.");
    assert_eq!(alpha[0]["preview"], "```mermaid\ngraph LR\n A --> B\n```");
    assert_eq!(
        json_of(page("opened", 1, Some(ids[0].clone())).unwrap())[0]["id"],
        ids[0]
    );
    assert_eq!(
        page("unknown", 0, None).err(),
        Some(StatusCode::BAD_REQUEST)
    );
    assert_eq!(
        page("opened", 0, Some("../escape".into())).err(),
        Some(StatusCode::BAD_REQUEST)
    );
}

#[test]
fn routine_client_errors_do_not_reload_every_note() {
    let (state, _dir, _ids) = state_with(&[("One", "")]);
    for code in [
        StatusCode::NOT_FOUND,
        StatusCode::CONFLICT,
        StatusCode::BAD_REQUEST,
    ] {
        let failed: Result<(), StatusCode> = run(state.with_store(move |_| Err(code)));
        assert_eq!(failed, Err(code));
        assert!(!state.store.lock().unwrap().reload, "{code}");
        let now: Result<(), StatusCode> = store_now(&state, |_| Err(code));
        assert_eq!(now, Err(code));
        assert!(!state.store.lock().unwrap().reload, "{code}");
    }
}

#[test]
fn a_new_folder_that_windows_cannot_hold_is_refused() {
    let (state, _dir, _ids) = state_with(&[]);
    for bad in ["Q&A?", "Week 1.", "CON", "Notes/CON", "Notes/What?"] {
        let made = run(create_note(
            State(state.clone()),
            Json(CreateBody {
                title: "Idea".into(),
                body: None,
                tags: None,
                directory: Some(bad.into()),
            }),
        ));
        assert_eq!(made.err(), Some(StatusCode::BAD_REQUEST), "{bad}");
        assert!(!state.fresh().dir_exists(bad), "{bad}");
    }
    assert!(state.fresh().notes.is_empty());
    let fine = run(create_note(
        State(state.clone()),
        Json(CreateBody {
            title: "Idea".into(),
            body: None,
            tags: None,
            directory: Some("Week 1".into()),
        }),
    ));
    assert!(fine.is_ok());
}

#[test]
fn a_stale_save_whose_content_already_reached_leo_is_not_a_conflict() {
    let (state, _dir, ids) = state_with(&[("Draft", "")]);
    let id = ids[0].clone();
    let send = |value: serde_json::Value| {
        run(update_note(
            State(state.clone()),
            Path(id.clone()),
            Json(serde_json::from_value::<UpdateBody>(value).unwrap()),
        ))
    };
    let first = send(serde_json::json!({ "body": "typed words" })).unwrap();
    let before = first.0.updated_at.clone();
    let again = send(serde_json::json!({ "body": "typed words", "base": "stale" })).unwrap();
    assert_eq!(again.0.body, "typed words");
    assert_eq!(again.0.version, first.0.version);
    assert_eq!(again.0.updated_at, before);
    let differing = send(serde_json::json!({ "body": "other words", "base": "stale" }));
    assert_eq!(differing.err(), Some(StatusCode::CONFLICT));
    assert_eq!(state.fresh().notes.len(), 1);
}

#[test]
fn the_unused_checkbox_toggle_route_is_gone() {
    run(async {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::load_from(&dir.path().join("notes")).unwrap();
        let id = store
            .create_note("Boxes", "- [ ] one", vec![], "")
            .unwrap()
            .id
            .clone();
        store.save().unwrap();
        let running = start_in(
            Places {
                store,
                sessions: dir.path().join("sessions.json"),
            },
            StartOptions {
                port: 0,
                token: "0123456789abcdef0123456789abcdef".into(),
            },
            Powers::default(),
        )
        .await
        .unwrap();
        let port = running.port();
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        let request = format!(
            "POST /api/notes/{id}/toggle?checkbox=1&token=0123456789abcdef0123456789abcdef HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        tokio::io::AsyncWriteExt::write_all(&mut stream, request.as_bytes())
            .await
            .unwrap();
        let mut reply = String::new();
        tokio::io::AsyncReadExt::read_to_string(&mut stream, &mut reply)
            .await
            .unwrap();
        assert!(reply.starts_with("HTTP/1.1 404"), "{reply}");
        running.stop().await;
    });
}

#[test]
fn folder_note_counts_include_everything_below_and_ignore_lookalike_names() {
    let (state, _dir, _ids) = state_with(&[
        ("A", "cs130"),
        ("B", "cs130/week1"),
        ("C", "cs130/week1"),
        ("D", "cs13"),
        ("E", ""),
    ]);
    let Json(all) = run(list_folders(State(state.clone()))).unwrap();
    let store = state.fresh();
    assert!(!all.is_empty());
    for dir in &all {
        assert_eq!(dir.notes, store.dir_contents(&dir.name).0, "{}", dir.name);
    }
    let count = |name: &str| all.iter().find(|d| d.name == name).unwrap().notes;
    assert_eq!(
        (count("cs130"), count("cs130/week1"), count("cs13")),
        (3, 2, 1)
    );
    drop(store);
    let Json(top) = run(list_dirs(
        State(state.clone()),
        Query(DirParams { parent: None }),
    ))
    .unwrap();
    assert_eq!(top.iter().find(|d| d.name == "cs130").unwrap().notes, 3);
    let Json(nested) = run(list_dirs(
        State(state),
        Query(DirParams {
            parent: Some("cs130".into()),
        }),
    ))
    .unwrap();
    assert_eq!(nested.iter().find(|d| d.name == "week1").unwrap().notes, 2);
}

#[test]
fn renaming_or_moving_a_folder_takes_its_pictures_and_other_files_along() {
    let (state, _dir, ids) = state_with(&[("Cells", "Bio"), ("Other", "Misc")]);
    let root = state.fresh().notes_dir.clone();
    std::fs::write(root.join("Bio/cell.png"), b"png").unwrap();
    std::fs::create_dir_all(root.join("Bio/img")).unwrap();
    std::fs::write(root.join("Bio/img/deep.png"), b"deep").unwrap();
    let renamed = run(crate::routes::notes::rename_dir(
        State(state.clone()),
        Json(crate::routes::notes::RenameDirBody {
            from: "Bio".into(),
            name: "Biology".into(),
        }),
    ))
    .unwrap();
    assert_eq!(renamed.0["path"], "Biology");
    assert_eq!(
        std::fs::read(root.join("Biology/cell.png")).unwrap(),
        b"png"
    );
    assert_eq!(
        std::fs::read(root.join("Biology/img/deep.png")).unwrap(),
        b"deep"
    );
    assert!(!root.join("Bio").exists());
    assert_eq!(
        state.fresh().find_note(&ids[0]).unwrap().directory,
        "Biology"
    );

    let _ = run(move_dir(
        State(state.clone()),
        Json(MoveDirBody {
            from: "Biology".into(),
            into: "Misc".into(),
        }),
    ))
    .unwrap();
    assert_eq!(
        std::fs::read(root.join("Misc/Biology/cell.png")).unwrap(),
        b"png"
    );
    assert!(!root.join("Biology").exists());
}

fn organize_call(name: &str, args: serde_json::Value) -> crate::tools::Call {
    crate::tools::Call {
        name: name.into(),
        args,
    }
}

#[test]
fn a_tool_name_organize_does_not_know_is_an_error_not_a_deleted_note() {
    let (state, _dir, _ids) = state_with(&[("Lecture 3", "")]);
    run(async {
        let state = state.clone();
        tokio::task::spawn_blocking(move || {
            let mut desk = crate::tools::Desk::new(vec![], 8000);
            let done = crate::routes::organize::run(
                &state,
                &mut desk,
                crate::tools::Access::Auto,
                &organize_call("archive_note", serde_json::json!({ "note": "Lecture 3" })),
            );
            assert!(done.proposal.is_none(), "{}", done.result);
            assert!(done.result.contains("archive_note"), "{}", done.result);
            assert!(!done.step.contains("delete"), "{}", done.step);
            let asked = crate::routes::organize::run(
                &state,
                &mut desk,
                crate::tools::Access::Auto,
                &organize_call("delete_note", serde_json::json!({ "note": "Lecture 3" })),
            );
            assert!(matches!(
                asked.proposal,
                Some(crate::tools::Proposal::DeleteNote { .. })
            ));
        })
        .await
        .unwrap();
    });
}

#[test]
fn moving_a_task_in_felix_keeps_its_end_time_and_the_undo_does_too() {
    let (state, _dir, _ids) = state_with(&[]);
    let made = run(crate::routes::tasks::create_task(
        State(state.clone()),
        Json(
            serde_json::from_value(serde_json::json!({
                "title": "Lab",
                "date": "2026-11-03",
                "time": "14:00",
                "end": "15:00"
            }))
            .unwrap(),
        ),
    ));
    let made = json_of(made);
    let id = made["id"].as_str().unwrap().to_string();
    run(async {
        let state = state.clone();
        tokio::task::spawn_blocking(move || {
            let moved = crate::tools::Proposal::UpdateTask {
                summary: "Move the lab".into(),
                id: id.clone(),
                description: None,
                done: None,
                date: Some("2026-11-05".into()),
                clear_date: false,
                time: None,
                task_kind: None,
                on: None,
            };
            let shown = crate::routes::organize::act_now(&state, &moved).unwrap();
            let stored = crate::tasks::load(&state.chats, &id).unwrap();
            assert_eq!(stored.date, chrono::NaiveDate::from_ymd_opt(2026, 11, 5));
            assert_eq!(stored.end, chrono::NaiveTime::from_hms_opt(15, 0, 0));
            let undo: crate::tools::Proposal =
                serde_json::from_value(shown["undo"].clone()).unwrap();
            crate::routes::organize::act_now(&state, &undo).unwrap();
            let back = crate::tasks::load(&state.chats, &id).unwrap();
            assert_eq!(back.date, chrono::NaiveDate::from_ymd_opt(2026, 11, 3));
            assert_eq!(back.end, chrono::NaiveTime::from_hms_opt(15, 0, 0));
        })
        .await
        .unwrap();
    });
}
