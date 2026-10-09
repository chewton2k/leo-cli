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

fn chat_lines(state: &AppState, question: &str) -> Vec<serde_json::Value> {
    let body = chat::ChatBody {
        messages: vec![chat::Turn {
            role: "user".into(),
            text: question.into(),
        }],
        mode: None,
        note: None,
        refs: vec![],
        chat: None,
        files: vec![],
    };
    let text = run(async {
        let response = chat_reply(State(state.clone()), Json(body)).await;
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    });
    text.lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn scripted(replies: Vec<&'static str>) -> (chat::Streamer, Arc<Mutex<Vec<String>>>) {
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&prompts);
    let replies = Arc::new(Mutex::new(replies.into_iter()));
    let streamer: chat::Streamer = Arc::new(
        move |system: &str,
              user: &str,
              _: u32,
              piece: &mut dyn FnMut(&str),
              _: &mut dyn FnMut()| {
            seen.lock().unwrap().push(format!("{system}\n{user}"));
            let reply = replies.lock().unwrap().next().unwrap_or("Out of script.");
            for chunk in reply.as_bytes().chunks(7) {
                piece(std::str::from_utf8(chunk).unwrap());
            }
            Ok(reply.to_string())
        },
    );
    (streamer, prompts)
}

#[test]
fn felix_searches_opens_and_suggests_a_change_with_tools_then_answers() {
    let (mut state, _d, ids) = state_with(&[("Graph traversals", "cs130"), ("Calendar", "")]);
    {
        let mut store = state.fresh();
        store.find_note_mut(&ids[0]).unwrap().body =
            "BFS takes the newest vertex from a queue.".into();
        store.save().unwrap();
    }
    let (streamer, prompts) = scripted(vec![
        "<tool>{\"name\": \"search_notes\", \"query\": \"queue\"}</tool>",
        "Let me check it.\n<tool>{\"name\": \"edit_note\", \"note\": \"n1\", \"find\": \"newest\", \"replace\": \"oldest\", \"why\": \"a queue is first in, first out\"}</tool>",
        "Your note says BFS takes the newest vertex [n1], but a queue gives the oldest; I suggested a fix.",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines(&state, "is my BFS note right?");
    let steps: Vec<&str> = lines.iter().filter_map(|l| l["step"].as_str()).collect();
    assert_eq!(
        steps,
        [
            "Searched your notes for “queue”",
            "Suggested a change to “Graph traversals”"
        ]
    );
    let sources = lines.iter().rfind(|l| l.get("sources").is_some()).unwrap();
    assert_eq!(sources["sources"][0]["title"], "Graph traversals");
    let proposal = lines.iter().find_map(|l| l.get("proposal")).unwrap();
    assert_eq!(proposal["kind"], "edit");
    assert_eq!(proposal["note"], ids[0].as_str());
    assert_eq!(proposal["find"], "newest");
    let shown: String = lines.iter().filter_map(|l| l["t"].as_str()).collect();
    assert!(
        !shown.contains("<tool"),
        "a tool call never reaches the page: {shown}"
    );
    let last_restart = lines
        .iter()
        .rposition(|l| l.get("restart").is_some())
        .unwrap();
    let before: String = lines[..last_restart]
        .iter()
        .filter_map(|l| l["t"].as_str())
        .collect();
    assert_eq!(
        before, "Let me check it.\n",
        "words before a tool call are shown, then taken back"
    );
    let after: String = lines[last_restart..]
        .iter()
        .filter_map(|l| l["t"].as_str())
        .collect();
    assert_eq!(after, "Your note says BFS takes the newest vertex [n1], but a queue gives the oldest; I suggested a fix.");
    assert_eq!(lines.last().unwrap()["done"], true);
    let prompts = prompts.lock().unwrap();
    assert_eq!(prompts.len(), 3);
    assert!(prompts[0].contains(&tools::manual()));
    assert!(
        prompts[0].trim_end().ends_with(tools::REMINDER),
        "the reminder comes last"
    );
    assert!(prompts[1].contains("<tool_result name=\"search_notes\">\n[n1] \"Graph traversals\" in cs130: BFS takes the newest"), "{}", prompts[1]);
    assert!(prompts[2].contains("Suggested. The user sees the change"));
    assert_eq!(
        state.fresh().find_note(&ids[0]).unwrap().body,
        "BFS takes the newest vertex from a queue.",
        "a suggestion changes nothing by itself"
    );
}

#[test]
fn felix_stops_using_tools_after_six_and_answers() {
    let (mut state, _d, _ids) = state_with(&[("Heaps", "")]);
    let (streamer, prompts) =
        scripted(vec!["<tool>{\"name\": \"search_notes\", \"query\": \"heap\"}</tool>"; 6]);
    state.chat = Some(streamer);
    let lines = chat_lines(&state, "loop forever");
    assert_eq!(lines.iter().filter(|l| l.get("step").is_some()).count(), 6);
    let prompts = prompts.lock().unwrap();
    assert_eq!(prompts.len(), 7);
    assert!(prompts[6].contains("You have used all the tools"));
    assert!(!prompts[6].contains(tools::REMINDER));
    assert!(!prompts[6].contains("### search_notes"));
    let shown: String = lines.iter().filter_map(|l| l["t"].as_str()).collect();
    assert!(shown.ends_with("Out of script."));
}

#[test]
fn a_broken_tool_call_is_explained_to_the_model_instead_of_failing() {
    let (mut state, _d, _ids) = state_with(&[]);
    let (streamer, prompts) = scripted(vec![
        "<tool>{search_notes: heaps}</tool>",
        "Sorry, here is the answer.",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines(&state, "heaps?");
    assert_eq!(lines.last().unwrap()["done"], true);
    assert!(
        prompts.lock().unwrap()[1].contains("That did not work: that tool call is not valid JSON")
    );
}

#[test]
fn a_suggested_change_is_applied_only_while_the_text_is_still_there() {
    let (state, _d, ids) = state_with(&[("Graph traversals", "")]);
    {
        let mut store = state.fresh();
        store.find_note_mut(&ids[0]).unwrap().body = "BFS takes the newest vertex.".into();
        store.save().unwrap();
    }
    let apply = |find: &str, replace: &str| {
        run(apply_suggestion(
            State(state.clone()),
            Path(ids[0].clone()),
            Json(Suggestion {
                find: find.into(),
                replace: replace.into(),
            }),
        ))
    };
    let applied = apply("newest", "oldest");
    assert_eq!(applied.status(), StatusCode::OK);
    let applied = json_of(applied);
    assert_eq!(applied["before"], "BFS takes the newest vertex.");
    assert_eq!(applied["body"], "BFS takes the oldest vertex.");
    let after = applied["version"].as_str().unwrap().to_string();
    let undo = |base: &str| {
        run(update_note(
            State(state.clone()),
            Path(ids[0].clone()),
            Json(UpdateBody {
                title: None,
                body: Some("BFS takes the newest vertex.".into()),
                tags: None,
                pinned: None,
                base: Some(base.into()),
            }),
        ))
    };
    assert!(
        undo("stale-version").is_err(),
        "an undo after another edit is refused"
    );
    assert!(undo(&after).is_ok());
    assert_eq!(
        state.fresh().find_note(&ids[0]).unwrap().body,
        "BFS takes the newest vertex."
    );
    assert_eq!(apply("newest", "oldest").status(), StatusCode::OK);
    assert_eq!(
        state.fresh().find_note(&ids[0]).unwrap().body,
        "BFS takes the oldest vertex."
    );
    assert_eq!(apply("newest", "oldest").status(), StatusCode::CONFLICT);
    assert_eq!(
        apply("", "## Practice\n- trace BFS").status(),
        StatusCode::OK
    );
    assert_eq!(
        state.fresh().find_note(&ids[0]).unwrap().body,
        "BFS takes the oldest vertex.\n\n## Practice\n- trace BFS"
    );
    let missing = run(apply_suggestion(
        State(state.clone()),
        Path("nope".into()),
        Json(Suggestion {
            find: String::new(),
            replace: "x".into(),
        }),
    ));
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
}

#[test]
fn a_change_the_user_asked_for_is_asked_for_once_more_when_the_model_only_talks() {
    let (mut state, _d, ids) = state_with(&[("Graph traversals", "")]);
    {
        let mut store = state.fresh();
        store.find_note_mut(&ids[0]).unwrap().body = "BFS uses a stack.".into();
        store.save().unwrap();
    }
    let (streamer, prompts) = scripted(vec![
        "I can't edit notes here, but BFS uses a queue.",
        "<tool>{\"name\": \"edit_note\", \"note\": \"Graph traversals\", \"find\": \"stack\", \"replace\": \"queue\"}</tool>",
        "I suggested the fix; you can apply it.",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines(&state, "Please fix my graph traversals note");
    assert!(lines.iter().any(|l| l.get("proposal").is_some()));
    let last_restart = lines
        .iter()
        .rposition(|l| l.get("restart").is_some())
        .unwrap();
    let after: String = lines[last_restart..]
        .iter()
        .filter_map(|l| l["t"].as_str())
        .collect();
    assert_eq!(after, "I suggested the fix; you can apply it.");
    let prompts = prompts.lock().unwrap();
    assert!(
        prompts[1].contains("Felix replied: I can't edit notes here")
            && prompts[1].contains(tools::NUDGE)
    );

    let (mut state, _d, _ids) = state_with(&[("Heaps", "")]);
    let (streamer, prompts) = scripted(vec!["Nothing needs fixing.", "Still nothing to fix."]);
    state.chat = Some(streamer);
    let lines = chat_lines(&state, "fix anything wrong in my heaps note");
    let shown: String = lines
        .iter()
        .skip(
            lines
                .iter()
                .rposition(|l| l.get("restart").is_some())
                .unwrap_or(0),
        )
        .filter_map(|l| l["t"].as_str())
        .collect();
    assert_eq!(shown, "Still nothing to fix.");
    assert_eq!(prompts.lock().unwrap().len(), 2, "the nudge is given once");
}

#[test]
fn felix_is_given_web_tools_only_when_his_ai_cannot_search() {
    for needed in [true, false] {
        let (mut state, _d, _ids) = state_with(&[]);
        let (streamer, prompts) = scripted(vec!["Plain answer."]);
        state.chat = Some(streamer);
        state.web = Some(crate::Web {
            search: Arc::new(|_: &str| Ok(vec![])),
            page: Arc::new(|_: &str| Ok(String::new())),
            needed: Arc::new(move || needed),
        });
        chat_lines(&state, "hello");
        let prompt = prompts.lock().unwrap()[0].clone();
        assert_eq!(
            prompt.contains("### web_search"),
            needed,
            "needed = {needed}"
        );
    }
}

#[test]
fn a_model_that_says_its_tools_are_missing_is_told_once_that_they_are_not() {
    let (mut state, _d, _ids) = state_with(&[("Heaps", "")]);
    let (streamer, prompts) = scripted(vec![
        "I can't access the search_notes tool in this chat.",
        "<tool>{\"name\": \"search_notes\", \"query\": \"heaps\"}</tool>",
        "Found it [n1].",
        "unused",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines(&state, "what do my notes say about heaps?");
    let steps: Vec<&str> = lines.iter().filter_map(|l| l["step"].as_str()).collect();
    assert_eq!(steps, ["Searched your notes for “heaps”"]);
    let prompts = prompts.lock().unwrap();
    assert!(prompts[1].contains(tools::UNSTUCK));
    assert_eq!(prompts.len(), 3);
}
