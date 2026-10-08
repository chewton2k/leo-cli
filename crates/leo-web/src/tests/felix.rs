use super::*;

#[test]
fn a_document_given_to_felix_is_kept_as_text_and_read_with_the_question() {
    use base64::Engine;
    let (mut state, _d, _ids) = state_with(&[]);
    state.reader = Some(Arc::new(|file: UploadFile, _: &mut dyn FnMut(&str)| {
        assert_eq!(file.bytes, b"%PDF fake");
        Ok(format!("Text of {}: Dijkstra uses a heap.", file.name))
    }));
    let seen = Arc::new(Mutex::new(String::new()));
    let saw = Arc::clone(&seen);
    state.chat = Some(Arc::new(
        move |_: &str, user: &str, _: u32, piece: &mut dyn FnMut(&str), _: &mut dyn FnMut()| {
            *saw.lock().unwrap() = user.to_string();
            piece("It says Dijkstra uses a heap (slides.pdf).");
            Ok("done".to_string())
        },
    ));
    let upload = |name: &str, data: &[u8]| {
        run(add_chat_file(
            State(state.clone()),
            Path("chat-docs-0001".into()),
            Json(ImportFileBody {
                name: name.into(),
                mime: "application/pdf".into(),
                data: base64::engine::general_purpose::STANDARD.encode(data),
            }),
        ))
    };
    let added = upload("../../slides.pdf", b"%PDF fake");
    assert_eq!(added.status(), StatusCode::CREATED);
    let doc = json_of(added);
    assert_eq!(doc["name"], "slides.pdf");
    let on_disk: Vec<_> = std::fs::read_dir(state.chats.join("chat-docs-0001.files"))
        .unwrap()
        .flatten()
        .map(|e| std::fs::read_to_string(e.path()).unwrap())
        .collect();
    assert_eq!(on_disk.len(), 1);
    assert!(
        !on_disk[0].contains("%PDF"),
        "only the text is kept, never the file"
    );

    let body = chat::ChatBody {
        messages: vec![chat::Turn {
            role: "user".into(),
            text: "what do my slides say?".into(),
        }],
        mode: None,
        note: None,
        refs: vec![],
        chat: Some("chat-docs-0001".into()),
        files: vec![doc["id"].as_str().unwrap().to_string()],
    };
    run(async {
        let response = chat_reply(State(state.clone()), Json(body)).await;
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
    });
    let prompt = seen.lock().unwrap().clone();
    assert!(
        prompt.contains("<document id=\"d1\" name=\"slides.pdf\">"),
        "{prompt}"
    );
    assert!(prompt.contains("Text of slides.pdf: Dijkstra uses a heap."));

    let listed = json_of(run(list_chat_files(
        State(state.clone()),
        Path("chat-docs-0001".into()),
    )));
    assert_eq!(listed.as_array().unwrap().len(), 1);
    let gone = run(remove_chat_file(
        State(state.clone()),
        Path(("chat-docs-0001".into(), doc["id"].as_str().unwrap().into())),
    ));
    assert_eq!(gone, StatusCode::NO_CONTENT);
    let bad = run(list_chat_files(State(state.clone()), Path("../x".into())));
    assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
    state.reader = Some(Arc::new(|_: UploadFile, _: &mut dyn FnMut(&str)| {
        anyhow::bail!("leo cannot read song.mp3 yet")
    }));
    let unreadable = run(add_chat_file(
        State(state.clone()),
        Path("chat-docs-0001".into()),
        Json(ImportFileBody {
            name: "song.mp3".into(),
            mime: "audio/mpeg".into(),
            data: "AAAA".into(),
        }),
    ));
    assert_eq!(unreadable.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(json_of(unreadable)["error"]
        .as_str()
        .unwrap()
        .contains("song.mp3"));
}

#[test]
fn a_chat_reply_streams_its_sources_then_the_answer() {
    let (mut state, _d, ids) = state_with(&[("Heaps", "cs130")]);
    {
        let mut store = state.fresh();
        store.find_note_mut(&ids[0]).unwrap().body = "A binary heap backs a priority queue.".into();
        store.save().unwrap();
    }
    let seen = Arc::new(Mutex::new(String::new()));
    let saw = Arc::clone(&seen);
    state.chat = Some(Arc::new(
        move |system: &str,
              user: &str,
              _: u32,
              piece: &mut dyn FnMut(&str),
              _: &mut dyn FnMut()| {
            *saw.lock().unwrap() = format!("{system}\n{user}");
            piece("Heaps keep the minimum on top ");
            piece("[n1].");
            Ok("done".to_string())
        },
    ));
    let body = chat::ChatBody {
        messages: vec![chat::Turn {
            role: "user".into(),
            text: "how do heaps work?".into(),
        }],
        mode: Some("study".into()),
        note: Some(ids[0].clone()),
        refs: vec![],
        chat: None,
        files: vec![],
    };
    let text = run(async {
        let response = chat_reply(State(state.clone()), Json(body)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    });
    let lines: Vec<serde_json::Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines[0]["sources"][0]["title"], "Heaps");
    assert_eq!(lines[0]["sources"][0]["why"], "open");
    assert_eq!(lines[1]["t"], "Heaps keep the minimum on top ");
    assert_eq!(lines[2]["t"], "[n1].");
    assert_eq!(lines[3]["done"], true);
    let prompt = seen.lock().unwrap().clone();
    assert!(prompt.contains("Mode: study."), "{prompt}");
    assert!(prompt.contains("A binary heap backs a priority queue."));
    assert!(prompt.contains("User: how do heaps work?"));
}

#[test]
fn a_chat_without_ai_or_a_question_is_refused() {
    let (state, _d, _ids) = state_with(&[]);
    let ask = |text: &str| chat::ChatBody {
        messages: vec![chat::Turn {
            role: "user".into(),
            text: text.into(),
        }],
        mode: None,
        note: None,
        refs: vec![],
        chat: None,
        files: vec![],
    };
    let status = run(async {
        chat_reply(State(state.clone()), Json(ask("hi")))
            .await
            .status()
    });
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    let mut with = state.clone();
    with.chat = Some(Arc::new(
        |_: &str, _: &str, _: u32, _: &mut dyn FnMut(&str), _: &mut dyn FnMut()| {
            anyhow::bail!("no AI for writing is chosen")
        },
    ));
    let status = run(async {
        chat_reply(State(with.clone()), Json(ask("   ")))
            .await
            .status()
    });
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let text = run(async {
        let response = chat_reply(State(with.clone()), Json(ask("hi"))).await;
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    });
    assert!(
        text.lines()
            .last()
            .unwrap()
            .contains("no AI for writing is chosen"),
        "{text}"
    );
}
