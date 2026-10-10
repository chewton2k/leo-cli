use super::*;

const PICTURE: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\x01\0\0\0\0\xc8\x08\x02\0\0\0";

#[test]
fn an_upload_becomes_a_note_in_its_folder_and_keeps_the_original() {
    use base64::Engine;
    let (mut state, _d, _ids) = state_with(&[]);
    state.importer = Some(Arc::new(
        |files: Vec<UploadFile>, wants: &str, progress: &mut dyn FnMut(&str, usize, usize)| {
            assert_eq!(wants, "the problem, then the code");
            progress("Writing the note", 0, 1);
            assert_eq!(files[0].bytes, b"%PDF fake");
            Ok(Made {
                title: "Graph search".to_string(),
                body: "## BFS\n- uses a queue\n\n![The BFS tree](figure:1)\n\n![again](figure:1)\n![nothing](figure:9)".to_string(),
                figures: vec![
                    Figure {
                        place: "slide 2".into(),
                        bytes: PICTURE.to_vec(),
                        photo: false,
                    },
                    Figure {
                        place: "board.jpg".into(),
                        bytes: PICTURE.to_vec(),
                        photo: true,
                    },
                ],
            })
        },
    ));
    let body = ImportBody {
        directory: "cs130".into(),
        title: None,
        wants: "the problem, then the code".into(),
        files: vec![ImportFileBody {
            name: "../../lecture 4.pdf".into(),
            mime: "application/pdf".into(),
            data: base64::engine::general_purpose::STANDARD.encode(b"%PDF fake"),
        }],
    };
    let response = run(start_import(State(state.clone()), Json(body)));
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let bytes = run(axum::body::to_bytes(response.into_body(), usize::MAX)).unwrap();
    let id = serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let job = wait_for(&state, &id);
    assert_eq!(job.state, "done", "{job:?}");
    let note_id = job.note.unwrap();
    let note = state.fresh().find_note(&note_id).unwrap().clone();
    assert_eq!(note.title, "Graph search");
    assert_eq!(note.directory, "cs130");
    let pictures: Vec<String> = note
        .body
        .lines()
        .flat_map(|l| {
            leo_core::attachments::pictures_in(l)
                .into_iter()
                .map(|(_, s)| s.target)
        })
        .collect();
    assert_eq!(pictures.len(), 2, "{}", note.body);
    assert!(
        note.body
            .starts_with("## BFS\n- uses a queue\n\n![The BFS tree](attachments/"),
        "{}",
        note.body
    );
    assert!(
        note.body
            .contains("\n\n## Photos\n\n![board.jpg](attachments/"),
        "{}",
        note.body
    );
    assert!(!note.body.contains("figure:"), "{}", note.body);
    assert!(
        note.body
            .contains("\n\n---\n*From lecture 4.pdf, uploaded "),
        "{}",
        note.body
    );
    let notes_dir = state.fresh().notes_dir.clone();
    for picture in &pictures {
        assert!(picture.ends_with(".png"), "{picture}");
        assert_eq!(std::fs::read(notes_dir.join(picture)).unwrap(), PICTURE);
    }
    let listed = run(list_originals(State(state.clone()), Path(note_id.clone())));
    let listed = run(axum::body::to_bytes(listed.into_body(), usize::MAX)).unwrap();
    assert!(String::from_utf8_lossy(&listed).contains("\"name\":\"lecture 4.pdf\""));
    let file = run(get_original(
        State(state.clone()),
        Path((note_id.clone(), "lecture 4.pdf".into())),
        Query(OriginalView::default()),
    ));
    assert_eq!(file.status(), StatusCode::OK);
    assert_eq!(file.headers()[header::CONTENT_TYPE], "application/pdf");
    assert!(file.headers()[header::CONTENT_DISPOSITION]
        .to_str()
        .unwrap()
        .starts_with("attachment"));
    assert!(file.headers().get("x-frame-options").is_none());
    let shown = run(get_original(
        State(state.clone()),
        Path((note_id.clone(), "lecture 4.pdf".into())),
        Query(OriginalView {
            view: Some("1".into()),
        }),
    ));
    assert!(shown.headers()[header::CONTENT_DISPOSITION]
        .to_str()
        .unwrap()
        .starts_with("inline"));
    assert_eq!(shown.headers()["x-frame-options"], "SAMEORIGIN");
    assert!(shown.headers()["content-security-policy"]
        .to_str()
        .unwrap()
        .contains("frame-ancestors 'self'"));
    assert!(viewable("board.JPG") && viewable("a.pdf"));
    assert!(!viewable("slides.pptx") && !viewable("page.html") && !viewable("x.svg"));
    for bad in ["../graph.json", "..", "a/b", ".hidden"] {
        let refused = run(get_original(
            State(state.clone()),
            Path((note_id.clone(), bad.into())),
            Query(OriginalView::default()),
        ));
        assert_eq!(refused.status(), StatusCode::NOT_FOUND, "{bad}");
    }
    let refused = run(list_originals(
        State(state.clone()),
        Path("../notes".into()),
    ));
    assert_eq!(refused.status(), StatusCode::NOT_FOUND);
}

#[test]
fn a_failed_upload_says_why_and_bad_requests_are_refused() {
    use base64::Engine;
    let (mut state, _d, _ids) = state_with(&[]);
    let file = || ImportFileBody {
        name: "board.jpg".into(),
        mime: "image/jpeg".into(),
        data: base64::engine::general_purpose::STANDARD.encode([1, 2, 3]),
    };
    let none = run(start_import(
        State(state.clone()),
        Json(ImportBody {
            directory: String::new(),
            title: None,
            wants: String::new(),
            files: vec![file()],
        }),
    ));
    assert_eq!(none.status(), StatusCode::SERVICE_UNAVAILABLE);
    state.importer = Some(Arc::new(
        |_: Vec<UploadFile>, _: &str, _: &mut dyn FnMut(&str, usize, usize)| {
            anyhow::bail!("qwen3:8b cannot read images")
        },
    ));
    let response = run(start_import(
        State(state.clone()),
        Json(ImportBody {
            directory: String::new(),
            title: None,
            wants: String::new(),
            files: vec![file()],
        }),
    ));
    let bytes = run(axum::body::to_bytes(response.into_body(), usize::MAX)).unwrap();
    let id = serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let job = wait_for(&state, &id);
    assert_eq!(job.state, "failed");
    assert_eq!(job.error.as_deref(), Some("qwen3:8b cannot read images"));
    let outside = run(start_import(
        State(state.clone()),
        Json(ImportBody {
            directory: "../outside".into(),
            title: None,
            wants: String::new(),
            files: vec![file()],
        }),
    ));
    assert_eq!(outside.status(), StatusCode::BAD_REQUEST);
    let empty = run(start_import(
        State(state.clone()),
        Json(ImportBody {
            directory: String::new(),
            title: None,
            wants: String::new(),
            files: vec![],
        }),
    ));
    assert_eq!(empty.status(), StatusCode::BAD_REQUEST);
    let garbled = run(start_import(
        State(state.clone()),
        Json(ImportBody {
            directory: String::new(),
            title: None,
            wants: String::new(),
            files: vec![ImportFileBody {
                name: "x.pdf".into(),
                mime: String::new(),
                data: "%%%".into(),
            }],
        }),
    ));
    assert_eq!(garbled.status(), StatusCode::BAD_REQUEST);
    assert_eq!(safe_file_name("../../etc/passwd"), "passwd");
    assert_eq!(safe_file_name("C:\\x\\notes?.pdf"), "notes-.pdf");
    assert_eq!(safe_file_name(".."), "upload");
}

#[test]
fn a_download_name_works_in_every_browser_whatever_the_title() {
    assert_eq!(
        attachment_header("Lecture 4 originals.zip"),
        "attachment; filename=\"Lecture 4 originals.zip\"; filename*=UTF-8''Lecture%204%20originals.zip"
    );
    let accented = attachment_header("Café \"notes\".zip");
    assert!(
        accented.starts_with("attachment; filename=\"Caf_ _notes_.zip\""),
        "{accented}"
    );
    assert!(
        accented.ends_with("filename*=UTF-8''Caf%C3%A9%20%22notes%22.zip"),
        "{accented}"
    );
    assert!(HeaderValue::from_str(&accented).is_ok());
}

#[test]
fn background_work_is_listed_until_it_is_done() {
    let file = |name: &str| UploadFile {
        name: name.into(),
        mime: String::new(),
        bytes: vec![],
    };
    assert_eq!(
        upload_label(&[file("slides.pdf")]),
        "Making a note from slides.pdf"
    );
    assert_eq!(
        upload_label(&[file("a.jpg"), file("b.jpg"), file("c.jpg")]),
        "Making a note from a.jpg and 2 more"
    );
    let job = |state: &'static str, dir: &str| ImportJob {
        state,
        label: "Making a note from slides.pdf".into(),
        dir: dir.into(),
        step: "Writing the note".into(),
        done: 1,
        total: 3,
        note: None,
        error: None,
    };
    let mut imports = std::collections::HashMap::new();
    imports.insert("a".to_string(), job("working", "cs130"));
    imports.insert("b".to_string(), job("done", ""));
    imports.insert("c".to_string(), job("failed", ""));
    let writing = record::RecordView {
        next_seq: 0,
        id: "r".into(),
        source: record::Source::Browser,
        state: "writing",
        secs: 60,
        step: "Writing the notes".into(),
        steps: Some((2, 4)),
        transcript: String::new(),
        warnings: vec![],
        points: vec![],
        wants: String::new(),
        levels: vec![],
        levels_start: 0,
        note: None,
        error: None,
    };
    let tasks = activity_tasks(&imports, Some(&writing), Some((3, 9)));
    let kinds: Vec<&str> = tasks.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        ["upload", "recording", "map"],
        "finished uploads are not listed"
    );
    assert_eq!(tasks[0].href, "#/f/cs130");
    assert_eq!((tasks[0].done, tasks[0].total), (1, 3));
    assert_eq!(
        (tasks[1].done, tasks[1].total, tasks[1].href.as_str()),
        (2, 4, "#/record")
    );
    assert_eq!(tasks[2].step, "3 of 9 steps");
    let still_recording = record::RecordView {
        next_seq: 0,
        state: "recording",
        ..writing
    };
    assert!(
        activity_tasks(&Default::default(), Some(&still_recording), None).is_empty(),
        "a recording that is still going has its own timer, not a progress bar"
    );
}

#[test]
fn figures_the_ai_did_not_place_are_kept_under_their_own_heading() {
    let figures = vec![
        Figure {
            place: "page 3".into(),
            bytes: PICTURE.to_vec(),
            photo: false,
        },
        Figure {
            place: "page 5".into(),
            bytes: PICTURE.to_vec(),
            photo: false,
        },
    ];
    let mut n = 0;
    let mut save = |f: &Figure| {
        n += 1;
        (f.place != "page 5").then(|| format!("attachments/{n}.png"))
    };
    assert_eq!(
        settle_figures("## Heaps\n- tree", &figures, &mut save),
        "## Heaps\n- tree\n\n## Figures\n\n![Figure from page 3](attachments/1.png)"
    );
    let kept = settle_figures(
        "Intro ![a heap](figure:2) and ![x](attachments/old.png)\n![the tree](figure:1)",
        &figures,
        &mut |f: &Figure| Some(format!("attachments/{}.png", f.place.replace(' ', "-"))),
    );
    assert_eq!(
        kept,
        "Intro ![a heap](attachments/page-5.png) and ![x](attachments/old.png)\n![the tree](attachments/page-3.png)"
    );
}
