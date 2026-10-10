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
            Ok("done".into())
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
    let on_disk: Vec<String> = crate::chat_files::texts(
        &state.chats,
        "chat-docs-0001",
        &[doc["id"].as_str().unwrap().to_string()],
    )
    .into_iter()
    .map(|(_, text)| text)
    .collect();
    assert_eq!(on_disk.len(), 1);
    assert!(
        !on_disk[0].contains("%PDF"),
        "only the text is kept, never the file"
    );

    let body = chat::ChatBody {
        scope: Default::default(),
        messages: vec![chat::Turn {
            role: "user".into(),
            text: "what do my slides say?".into(),
        }],
        mode: None,
        note: None,
        refs: vec![],
        chat: Some("chat-docs-0001".into()),
        files: vec![doc["id"].as_str().unwrap().to_string()],
        access: None,
        recent: vec![],
        practice: false,
        mark: false,
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
            Ok("done".into())
        },
    ));
    let body = chat::ChatBody {
        scope: Default::default(),
        messages: vec![chat::Turn {
            role: "user".into(),
            text: "how do heaps work?".into(),
        }],
        mode: Some("study".into()),
        note: Some(ids[0].clone()),
        refs: vec![],
        chat: None,
        files: vec![],
        access: None,
        recent: vec![],
        practice: false,
        mark: false,
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
    assert!(
        lines[0]["answer"].is_string(),
        "the answer's id comes first, for steering"
    );
    assert_eq!(lines[1]["sources"][0]["title"], "Heaps");
    assert_eq!(lines[1]["sources"][0]["why"], "open");
    assert_eq!(lines[2]["t"], "Heaps keep the minimum on top ");
    assert_eq!(lines[3]["t"], "[n1].");
    assert_eq!(lines[4]["done"], true);
    let prompt = seen.lock().unwrap().clone();
    assert!(prompt.contains("Mode: study."), "{prompt}");
    assert!(prompt.contains("A binary heap backs a priority queue."));
    assert!(prompt.contains("User: how do heaps work?"));
}

#[test]
fn a_chat_without_ai_or_a_question_is_refused() {
    let (state, _d, _ids) = state_with(&[]);
    let ask = |text: &str| chat::ChatBody {
        scope: Default::default(),
        messages: vec![chat::Turn {
            role: "user".into(),
            text: text.into(),
        }],
        mode: None,
        note: None,
        refs: vec![],
        chat: None,
        files: vec![],
        access: None,
        recent: vec![],
        practice: false,
        mark: false,
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
    chat_lines_with(state, question, None)
}

fn chat_lines_with(
    state: &AppState,
    question: &str,
    access: Option<&str>,
) -> Vec<serde_json::Value> {
    let body = chat::ChatBody {
        scope: Default::default(),
        messages: vec![chat::Turn {
            role: "user".into(),
            text: question.into(),
        }],
        mode: None,
        note: None,
        refs: vec![],
        chat: None,
        files: vec![],
        access: access.map(str::to_string),
        recent: vec![],
        practice: false,
        mark: false,
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
            Ok(reply.into())
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
    assert!(
        lines.iter().all(|l| l.get("restart").is_none()),
        "words before a tool call are kept, not taken back"
    );
    let shown: String = lines.iter().filter_map(|l| l["t"].as_str()).collect();
    assert_eq!(shown, "Let me check it.\n\n\nYour note says BFS takes the newest vertex [n1], but a queue gives the oldest; I suggested a fix.");
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
fn felix_stops_using_tools_after_the_most_steps_and_answers() {
    let (mut state, _d, _ids) = state_with(&[("Heaps", "")]);
    let (streamer, prompts) = scripted(vec![
        "<tool>{\"name\": \"search_notes\", \"query\": \"heap\"}</tool>";
        tools::MOST_STEPS
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines(&state, "loop forever");
    assert_eq!(
        lines.iter().filter(|l| l.get("step").is_some()).count(),
        tools::MOST_STEPS
    );
    let prompts = prompts.lock().unwrap();
    assert_eq!(prompts.len(), tools::MOST_STEPS + 1);
    let last = &prompts[tools::MOST_STEPS];
    assert!(last.contains("You have used all the tools"));
    assert!(!last.contains(tools::REMINDER));
    assert!(!last.contains("### search_notes"));
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
fn an_answer_is_one_request_and_is_never_taken_back_or_asked_again() {
    let shown = |lines: &[serde_json::Value]| -> String {
        lines.iter().filter_map(|l| l["t"].as_str()).collect()
    };
    for (question, reply) in [
        (
            "based on this transcription, can you answer these questions: 6. Can you describe a time you took initiative to make a positive difference? 8. If you were given the opportunity to improve one aspect of Clubhouse's platform, what would you focus on and why?",
            "6. When our club lost its venue, I organised a new one.",
        ),
        (
            "fix anything wrong in my heaps note",
            "Your heaps note looks right to me.",
        ),
        ("how would you improve this note?", "I would add an example."),
    ] {
        let (mut state, _d, _ids) = state_with(&[("Heaps", "")]);
        let (streamer, prompts) = scripted(vec![reply, "A second answer nobody asked for."]);
        state.chat = Some(streamer);
        let lines = chat_lines(&state, question);
        assert_eq!(shown(&lines), reply, "{question}");
        assert!(lines
            .iter()
            .all(|l| l.get("restart").is_none() && l.get("reset").is_none()));
        assert_eq!(prompts.lock().unwrap().len(), 1, "{question}");
    }

    let (mut state, _d, ids) = state_with(&[("Graph traversals", "")]);
    {
        let mut store = state.fresh();
        store.find_note_mut(&ids[0]).unwrap().body = "BFS uses a stack.".into();
        store.save().unwrap();
    }
    let (streamer, _prompts) = scripted(vec![
        "<tool>{\"name\": \"edit_note\", \"note\": \"Graph traversals\", \"find\": \"stack\", \"replace\": \"queue\"}</tool>",
        "I suggested changing stack to queue; apply it if it looks right.",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines(&state, "Please fix my graph traversals note");
    assert!(lines.iter().any(|l| l.get("proposal").is_some()));
    assert_eq!(
        shown(&lines),
        "I suggested changing stack to queue; apply it if it looks right."
    );
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

#[test]
fn what_every_step_of_an_answer_spent_is_added_up_and_sent_before_done() {
    let (mut state, _d, _) = state_with(&[("Graph traversals", "cs130")]);
    let replies = Arc::new(Mutex::new(
        vec![
            "<tool>{\"name\": \"search_notes\", \"query\": \"queue\"}</tool>",
            "A queue is first in, first out.",
        ]
        .into_iter(),
    ));
    state.chat = Some(Arc::new(
        move |_: &str, _: &str, _: u32, piece: &mut dyn FnMut(&str), _: &mut dyn FnMut()| {
            let text = replies.lock().unwrap().next().unwrap_or("Out of script.");
            piece(text);
            Ok(chat::Reply {
                text: text.into(),
                spent: Some(chat::Spent {
                    by: "Anthropic".into(),
                    model: Some("claude-sonnet-5-5".into()),
                    input: 1000,
                    output: 100,
                    cost: Some(0.003),
                    ..chat::Spent::default()
                }),
            })
        },
    ));
    let lines = chat_lines(&state, "what is a queue?");
    let at = lines.iter().position(|l| l.get("spent").is_some()).unwrap();
    assert_eq!(lines[at + 1]["done"], true);
    let spent = &lines[at]["spent"];
    assert_eq!(
        (spent["input"].as_u64(), spent["output"].as_u64()),
        (Some(2000), Some(200))
    );
    assert_eq!(spent["steps"], 2);
    assert!((spent["cost"].as_f64().unwrap() - 0.006).abs() < 1e-9);
    assert_eq!(spent["model"], "claude-sonnet-5-5");
}

#[test]
fn once_felix_has_answered_the_writing_ai_names_the_chat_in_the_background() {
    let (mut state, dir, _) = state_with(&[]);
    let asked = Arc::new(Mutex::new(0));
    let count = Arc::clone(&asked);
    let writer: crate::graph::Writer = Arc::new(move |system: &str, _: &str, _: u32| {
        assert!(system.contains("You name a conversation"));
        *count.lock().unwrap() += 1;
        Ok("\"Queues in breadth-first search\"".to_string())
    });
    state.graphs = Arc::new(crate::graph::Graphs::for_notes(
        &dir.path().join("notes"),
        Some(writer),
    ));
    let put = |messages: serde_json::Value| {
        let body: crate::chats::Saving =
            serde_json::from_value(serde_json::json!({ "mode": "chat", "messages": messages }))
                .unwrap();
        json_of(run(crate::routes::felix::put_chat(
            State(state.clone()),
            Path("chat-name-0001".into()),
            Json(body),
        )))
    };
    let first = put(serde_json::json!([{ "role": "user", "text": "help with bfs" }]));
    assert_eq!(first["title"], "help with bfs");
    put(
        serde_json::json!([{ "role": "user", "text": "help with bfs" }, { "role": "assistant", "text": "BFS uses a queue." }]),
    );
    let started = std::time::Instant::now();
    while crate::chats::load(&state.chats, "chat-name-0001").is_none_or(|c| !c.named) {
        assert!(started.elapsed().as_secs() < 5, "the chat was never named");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let later = put(
        serde_json::json!([{ "role": "user", "text": "help with bfs" }, { "role": "assistant", "text": "BFS uses a queue." }, { "role": "user", "text": "and dfs?" }]),
    );
    assert_eq!(later["title"], "Queues in breadth-first search");
    assert!(later["about"]
        .as_str()
        .unwrap()
        .starts_with("BFS uses a queue."));
    assert_eq!(*asked.lock().unwrap(), 1);
}

#[test]
fn in_auto_felix_changes_and_makes_notes_at_once_and_links_instead_of_citing_itself() {
    let (mut state, _d, ids) = state_with(&[("Graph traversals", "cs130"), ("Heaps", "cs130")]);
    {
        let mut store = state.fresh();
        store.find_note_mut(&ids[0]).unwrap().body = "BFS takes the newest vertex.".into();
        store.save().unwrap();
    }
    let (streamer, prompts) = scripted(vec![
        "<tool>{\"name\": \"search_notes\", \"query\": \"heaps\"}</tool>",
        "<tool>{\"name\": \"edit_note\", \"note\": \"Graph traversals\", \"find\": \"newest\", \"replace\": \"oldest [n1]\"}</tool>",
        "<tool>{\"name\": \"create_note\", \"title\": \"Queues\", \"body\": \"First in, first out. See [n1].\", \"folder\": \"cs130\"}</tool>",
        "I fixed it and made a note on queues.",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines_with(
        &state,
        "fix my bfs note and make one on queues",
        Some("auto"),
    );
    let shown: Vec<&serde_json::Value> = lines.iter().filter_map(|l| l.get("proposal")).collect();
    assert_eq!(shown.len(), 2);
    assert!(shown.iter().all(|p| p["state"] == "applied"));
    assert_eq!(shown[0]["before"], "BFS takes the newest vertex.");
    assert!(shown[0]["after"].as_str().is_some_and(|v| !v.is_empty()));
    let store = state.fresh();
    assert_eq!(
        store.find_note(&ids[0]).unwrap().body,
        "BFS takes the oldest vertex."
    );
    let made = store.find_note(shown[1]["made"].as_str().unwrap()).unwrap();
    assert_eq!(
        (
            made.title.as_str(),
            made.directory.as_str(),
            made.body.as_str()
        ),
        (
            "Queues",
            "cs130",
            "First in, first out. See [[Graph traversals]]."
        )
    );
    let prompts = prompts.lock().unwrap();
    assert!(prompts[0].contains("leo applies your changes at once"));
    assert!(prompts[2].contains("Changed. leo applied it"));
}

#[test]
fn read_only_felix_has_no_change_tools_and_is_refused_if_it_tries() {
    let (mut state, _d, ids) = state_with(&[("Graph traversals", "cs130")]);
    let (streamer, prompts) = scripted(vec![
        "<tool>{\"name\": \"edit_note\", \"note\": \"Graph traversals\", \"replace\": \"more\"}</tool>",
        "I would add a line about queues; switch to Ask or Auto to let me.",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines_with(&state, "please fix my note", Some("read"));
    assert!(lines.iter().all(|l| l.get("proposal").is_none()));
    assert_eq!(state.fresh().find_note(&ids[0]).unwrap().body, "");
    let prompts = prompts.lock().unwrap();
    assert!(!prompts[0].contains("### edit_note") && !prompts[0].contains("### create_note"));
    assert!(prompts[0].contains("Read only"));
    assert!(prompts[0].trim_end().ends_with(tools::READ_REMINDER));
    assert!(prompts[1].contains("this chat is read only"));
    assert_eq!(prompts.len(), 2, "no nudge to change anything");
}

struct Native {
    script: Vec<(&'static str, serde_json::Value)>,
    answer: &'static str,
    results: Arc<Mutex<Vec<String>>>,
    fail: bool,
}

impl chat::Conversation for Native {
    fn native(&self) -> bool {
        true
    }

    fn say(&mut self, _text: &str, exchange: chat::Exchange<'_>) -> anyhow::Result<chat::Reply> {
        if self.fail {
            anyhow::bail!("the session could not start");
        }
        for (name, args) in self.script.drain(..) {
            let result = (exchange.call)(name, &args);
            self.results.lock().unwrap().push(result);
        }
        (exchange.piece)(self.answer);
        Ok(chat::Reply {
            text: self.answer.into(),
            spent: None,
        })
    }
}

#[test]
fn a_native_session_answers_once_even_when_no_change_was_proposed() {
    struct Twice(Arc<Mutex<Vec<String>>>, Vec<&'static str>);
    impl chat::Conversation for Twice {
        fn native(&self) -> bool {
            true
        }
        fn say(&mut self, text: &str, exchange: chat::Exchange<'_>) -> anyhow::Result<chat::Reply> {
            self.0.lock().unwrap().push(text.to_string());
            let reply = self.1.remove(0);
            (exchange.piece)(reply);
            Ok(chat::Reply::from(reply))
        }
    }
    let (mut state, _d, _) = state_with(&[("Interview prep", "")]);
    let heard = Arc::new(Mutex::new(Vec::new()));
    let ears = Arc::clone(&heard);
    state.converse = Some(Arc::new(
        move |_: &chat::Instructions, _: &[chat::ToolSpec]| {
            Some(Box::new(Twice(
                Arc::clone(&ears),
                vec![
                    "Here are your interview answers.",
                    "No note change is needed. Here are the interview responses again: ...",
                ],
            )) as Box<dyn chat::Conversation>)
        },
    ));
    state.chat = Some(scripted(vec!["never used"]).0);
    let lines = chat_lines(&state, "write my interview answers into my prep note");
    assert_eq!(heard.lock().unwrap().len(), 1);
    assert!(lines.iter().all(|l| l.get("restart").is_none()));
    let shown: String = lines.iter().filter_map(|l| l["t"].as_str()).collect();
    assert_eq!(shown, "Here are your interview answers.");
}

#[test]
fn a_model_with_native_tools_calls_leos_tools_directly_and_answers() {
    let (mut state, _d, _) = state_with(&[("Graph traversals", "cs130")]);
    let results = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&results);
    let instructions = Arc::new(Mutex::new(String::new()));
    let told = Arc::clone(&instructions);
    state.converse = Some(Arc::new(
        move |given: &chat::Instructions, specs: &[chat::ToolSpec]| {
            *told.lock().unwrap() = given.native.to_string();
            assert!(specs
                .iter()
                .any(|s| s.name == "search_notes" && s.schema["required"][0] == "query"));
            Some(Box::new(Native {
                script: vec![
                    ("search_notes", serde_json::json!({ "query": "graph" })),
                    ("open_note", serde_json::json!({})),
                ],
                answer: "BFS uses a queue [n1].",
                results: Arc::clone(&seen),
                fail: false,
            }) as Box<dyn chat::Conversation>)
        },
    ));
    state.chat = Some(scripted(vec!["never used"]).0);
    let lines = chat_lines(&state, "what is in my graph note?");
    let steps: Vec<&str> = lines.iter().filter_map(|l| l["step"].as_str()).collect();
    assert_eq!(steps[0], "Searched your notes for “graph”");
    let results = results.lock().unwrap();
    assert!(results[0].contains("Graph traversals"));
    assert!(
        results[1].starts_with("That did not work:"),
        "{}",
        results[1]
    );
    let shown: String = lines.iter().filter_map(|l| l["t"].as_str()).collect();
    assert_eq!(shown, "BFS uses a queue [n1].");
    assert!(instructions.lock().unwrap().contains("call them as tools"));
    assert!(!instructions.lock().unwrap().contains("<tool>{"));
}

#[test]
fn a_session_hears_only_what_is_new_and_a_failed_native_start_falls_back() {
    struct Session(Arc<Mutex<Vec<String>>>, Vec<&'static str>);
    impl chat::Conversation for Session {
        fn native(&self) -> bool {
            false
        }
        fn say(&mut self, text: &str, exchange: chat::Exchange<'_>) -> anyhow::Result<chat::Reply> {
            self.0
                .lock()
                .unwrap()
                .push(format!("{text}|{}", exchange.tail));
            let reply = self.1.remove(0);
            (exchange.piece)(reply);
            Ok(chat::Reply::from(reply))
        }
    }
    let (mut state, _d, _) = state_with(&[("Heaps", "")]);
    let heard = Arc::new(Mutex::new(Vec::new()));
    let ears = Arc::clone(&heard);
    state.converse = Some(Arc::new(
        move |_: &chat::Instructions, _: &[chat::ToolSpec]| {
            Some(Box::new(Session(
                Arc::clone(&ears),
                vec![
                    "<tool>{\"name\": \"search_notes\", \"query\": \"heap\"}</tool>",
                    "Heaps keep the minimum on top.",
                ],
            )) as Box<dyn chat::Conversation>)
        },
    ));
    state.chat = Some(scripted(vec!["never used"]).0);
    let lines = chat_lines(&state, "what is a heap?");
    let heard = heard.lock().unwrap();
    assert_eq!(heard.len(), 2);
    assert!(heard[0].contains("<conversation>") && heard[0].ends_with(tools::REMINDER));
    assert!(
        heard[1].starts_with("<tool_result name=\"search_notes\">"),
        "{}",
        heard[1]
    );
    assert!(
        !heard[1].contains("<conversation>"),
        "only the new part goes to a live session"
    );
    let shown: String = lines.iter().filter_map(|l| l["t"].as_str()).collect();
    assert_eq!(shown, "Heaps keep the minimum on top.");

    let (mut state, _d, _) = state_with(&[("Heaps", "")]);
    state.converse = Some(Arc::new(|_: &chat::Instructions, _: &[chat::ToolSpec]| {
        Some(Box::new(Native {
            script: vec![],
            answer: "",
            results: Default::default(),
            fail: true,
        }) as Box<dyn chat::Conversation>)
    }));
    let (streamer, prompts) = scripted(vec!["Answered the old way."]);
    state.chat = Some(streamer);
    let lines = chat_lines(&state, "what is a heap?");
    let shown: String = lines.iter().filter_map(|l| l["t"].as_str()).collect();
    assert_eq!(shown, "Answered the old way.");
    assert_eq!(prompts.lock().unwrap().len(), 1);
    assert_eq!(lines.last().unwrap()["done"], true);
}

#[test]
fn felix_looks_at_a_notes_pictures_and_their_captions_come_along_next_time() {
    let (mut state, dir, ids) = state_with(&[("Heaps", "")]);
    let notes = dir.path().join("notes");
    std::fs::create_dir_all(notes.join("attachments")).unwrap();
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    png.extend_from_slice(&[0; 80]);
    std::fs::write(notes.join("attachments/heap.png"), &png).unwrap();
    {
        let mut store = state.fresh();
        store.find_note_mut(&ids[0]).unwrap().body = "![a heap](attachments/heap.png)".into();
        store.save().unwrap();
    }
    state.seer = Some(Arc::new(
        |_: &str, user: &str, pictures: Vec<crate::captions::Picture>| {
            assert_eq!(pictures.len(), 1);
            assert_eq!(user, "Describe this picture.");
            Ok("A binary min heap with 2 at the root.".into())
        },
    ));
    let (streamer, prompts) = scripted(vec![
        "<tool>{\"name\": \"look_at_picture\", \"note\": \"Heaps\"}</tool>",
        "It shows a min heap.",
        "Still a min heap.",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines(&state, "what is in my heaps picture?");
    let steps: Vec<&str> = lines.iter().filter_map(|l| l["step"].as_str()).collect();
    assert_eq!(steps, ["Looked at the pictures in “Heaps”"]);
    assert!(prompts.lock().unwrap()[1]
        .contains("Picture 1: a heap (heap.png): A binary min heap with 2 at the root."));
    chat_lines(&state, "and the heaps picture again?");
    assert!(
        prompts.lock().unwrap()[2].contains(
            "![a heap](attachments/heap.png) [Picture: A binary min heap with 2 at the root.]"
        ),
        "the caption is now part of the note Felix reads"
    );
}

#[test]
fn felix_gives_a_practice_question_then_stops_and_waits_for_the_answer() {
    let (mut state, _d, _) = state_with(&[("Graph traversals", "cs130")]);
    let (streamer, prompts) = scripted(vec![
        "<tool>{\"name\": \"quiz\", \"kind\": \"multiple_choice\", \"question\": \"What does BFS use?\", \"options\": \"a stack | a queue\", \"answer\": \"a heap\"}</tool>",
        "<tool>{\"name\": \"quiz\", \"kind\": \"multiple_choice\", \"question\": \"What does BFS use?\", \"options\": \"a stack | a queue | a heap\", \"answer\": \"a queue\", \"explain\": \"Oldest first.\"}</tool>",
        "Give it a try!",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines(&state, "quiz me on BFS");
    let quizzes: Vec<&serde_json::Value> = lines.iter().filter_map(|l| l.get("quiz")).collect();
    assert_eq!(quizzes.len(), 1, "only the valid quiz is shown: {lines:?}");
    assert_eq!(quizzes[0]["kind"], "multiple_choice");
    assert_eq!(
        quizzes[0]["options"],
        serde_json::json!(["a stack", "a queue", "a heap"])
    );
    assert_eq!(quizzes[0]["answer"], "a queue");
    assert_eq!(quizzes[0]["explain"], "Oldest first.");
    let prompts = prompts.lock().unwrap();
    assert!(prompts[1].contains("the answer must be one of the options"));
    assert!(prompts[2].contains(tools::ASKED));
    assert!(
        prompts[2].contains("You have used all the tools"),
        "after asking, Felix only finishes his reply"
    );
    let shown: String = lines.iter().filter_map(|l| l["t"].as_str()).collect();
    assert_eq!(shown, "Give it a try!");
    assert!(lines.first().unwrap()["answer"].is_string());
}

#[test]
fn felix_asks_a_question_with_choices_once_per_reply() {
    let (mut state, _d, _) = state_with(&[]);
    let results = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&results);
    state.converse = Some(Arc::new(
        move |_: &chat::Instructions, specs: &[chat::ToolSpec]| {
            assert!(specs.iter().any(|s| s.name == "ask_user"));
            assert!(specs.iter().any(|s| s.name == "quiz"));
            Some(Box::new(Native {
                script: vec![
                    (
                        "ask_user",
                        serde_json::json!({ "question": "Which week?", "options": "Week 1 | Week 2 |  | Week 3" }),
                    ),
                    ("ask_user", serde_json::json!({ "question": "And again?" })),
                    ("search_notes", serde_json::json!({ "query": "heap" })),
                ],
                answer: "Tell me which week.",
                results: Arc::clone(&seen),
                fail: false,
            }) as Box<dyn chat::Conversation>)
        },
    ));
    state.chat = Some(scripted(vec!["never used"]).0);
    let lines = chat_lines_with(&state, "summarise the lecture", Some("read"));
    let asks: Vec<&serde_json::Value> = lines.iter().filter_map(|l| l.get("ask")).collect();
    assert_eq!(asks.len(), 1);
    assert_eq!(asks[0]["question"], "Which week?");
    assert_eq!(
        asks[0]["options"],
        serde_json::json!(["Week 1", "Week 2", "Week 3"])
    );
    let results = results.lock().unwrap();
    assert_eq!(results[0], tools::ASKED);
    assert_eq!(results[1], tools::ALREADY_ASKED);
    assert_eq!(
        results[2],
        tools::ALREADY_ASKED,
        "nothing else runs once the user was asked"
    );
}

#[test]
fn a_message_sent_while_felix_works_reaches_him_at_his_next_step() {
    let (mut state, _d, _) = state_with(&[("Heaps", "")]);
    let steering = state.steering.clone();
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&prompts);
    let step = Arc::new(Mutex::new(0));
    state.chat = Some(Arc::new(
        move |system: &str,
              user: &str,
              _: u32,
              piece: &mut dyn FnMut(&str),
              _: &mut dyn FnMut()| {
            seen.lock().unwrap().push(format!("{system}\n{user}"));
            let mut at = step.lock().unwrap();
            *at += 1;
            let reply = if *at == 1 {
                for id in steering.open_ids() {
                    steering.add(&id, "only the min-heap part").unwrap();
                }
                "<tool>{\"name\": \"search_notes\", \"query\": \"heap\"}</tool>"
            } else {
                "A min-heap keeps the smallest at the root."
            };
            piece(reply);
            Ok(reply.into())
        },
    ));
    let lines = chat_lines(&state, "explain heaps");
    let steered: Vec<&serde_json::Value> = lines.iter().filter_map(|l| l.get("steered")).collect();
    assert_eq!(steered, [&serde_json::json!(["only the min-heap part"])]);
    let prompts = prompts.lock().unwrap();
    assert!(prompts[1].contains("User: only the min-heap part"));
    assert!(prompts[1].contains("take it into account"));
    assert!(
        state.steering.open_ids().is_empty(),
        "a finished answer takes no more messages"
    );
}

#[test]
fn messages_can_be_added_only_to_an_answer_still_being_written() {
    let (state, _d, _) = state_with(&[]);
    let steer = state.steering.open();
    let post = |id: &str, text: &str| {
        run(steer_answer(
            State(state.clone()),
            Path(id.to_string()),
            Json(crate::routes::felix::Steering { text: text.into() }),
        ))
    };
    let accepted = post(steer.id(), "shorter please");
    assert_eq!(accepted.status(), StatusCode::ACCEPTED);
    assert_eq!(json_of(accepted)["waiting"], 1);
    assert_eq!(post(steer.id(), " ").status(), StatusCode::BAD_REQUEST);
    let id = steer.id().to_string();
    drop(steer);
    let late = post(&id, "too late");
    assert_eq!(late.status(), StatusCode::GONE);
    assert_eq!(json_of(late)["error"], "That answer has finished.");
}

#[test]
fn a_long_chat_is_remembered_in_a_summary_instead_of_forgotten() {
    let (mut state, dir, _) = state_with(&[]);
    let summaries = Arc::new(Mutex::new(Vec::new()));
    let told = Arc::clone(&summaries);
    let writer: crate::graph::Writer = Arc::new(move |system: &str, user: &str, _: u32| {
        if system.contains("You name a conversation") {
            return Ok("A name".into());
        }
        assert!(system.contains("You keep the memory of a long study chat"));
        told.lock().unwrap().push(user.to_string());
        Ok("The student is revising graph search for a Friday exam.".into())
    });
    state.graphs = Arc::new(crate::graph::Graphs::for_notes(
        &dir.path().join("notes"),
        Some(writer),
    ));
    let (streamer, prompts) = scripted(vec!["First.", "Second."]);
    state.chat = Some(streamer);
    let mut messages: Vec<chat::Turn> = (0..31)
        .map(|i| chat::Turn {
            role: if i % 2 == 0 { "user" } else { "assistant" }.into(),
            text: if i == 0 {
                "My exam is on Friday; I study graph search.".into()
            } else {
                format!("message {i}")
            },
        })
        .collect();
    let id = "chat-memory-0001";
    crate::chats::save(
        &state.chats,
        id,
        serde_json::from_value(serde_json::json!({ "mode": "study", "messages": [] })).unwrap(),
        chrono::Utc::now(),
    )
    .unwrap();
    let ask = |messages: &[chat::Turn]| {
        let body = chat::ChatBody {
            scope: Default::default(),
            messages: messages.to_vec(),
            mode: None,
            note: None,
            refs: vec![],
            chat: Some(id.into()),
            files: vec![],
            access: None,
            recent: vec![],
            practice: false,
            mark: false,
        };
        run(async {
            let response = chat_reply(State(state.clone()), Json(body)).await;
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            assert!(String::from_utf8_lossy(&bytes).contains("done"));
        });
    };
    ask(&messages);
    let first = prompts.lock().unwrap()[0].clone();
    assert!(
        first.contains("<earlier_in_this_chat>") && first.contains("User: My exam is on Friday"),
        "before a summary exists, older messages come along shortened"
    );
    let started = std::time::Instant::now();
    while crate::chats::load(&state.chats, id).is_none_or(|c| c.memory.is_none()) {
        assert!(started.elapsed().as_secs() < 5, "nothing was remembered");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let memory = crate::chats::load(&state.chats, id)
        .unwrap()
        .memory
        .unwrap();
    assert_eq!(memory.upto, 31 - chat::turns_for(chat::ROOM));
    assert!(summaries.lock().unwrap()[0].contains("My exam is on Friday"));
    crate::chats::save(
        &state.chats,
        id,
        serde_json::from_value(
            serde_json::json!({ "mode": "study", "messages": [{ "role": "user", "text": "x" }] }),
        )
        .unwrap(),
        chrono::Utc::now(),
    )
    .unwrap();
    assert!(
        crate::chats::load(&state.chats, id)
            .unwrap()
            .memory
            .is_some(),
        "the page saving the chat keeps the memory"
    );
    messages.push(chat::Turn {
        role: "assistant".into(),
        text: "message 31".into(),
    });
    messages.push(chat::Turn {
        role: "user".into(),
        text: "what was my deadline?".into(),
    });
    ask(&messages);
    let second = prompts.lock().unwrap()[1].clone();
    assert!(second.contains("What the chat covered before (a summary):\nThe student is revising graph search for a Friday exam."));
    assert!(
        !second.contains("User: My exam is on Friday"),
        "what the summary covers is not repeated"
    );
    assert!(second.contains("User: message 18"), "{second}");
}

#[test]
fn planning_is_in_the_standing_rules_and_never_added_because_of_a_numbered_list() {
    let (mut state, _d, _) = state_with(&[]);
    let (streamer, prompts) = scripted(vec!["Done.", "Done."]);
    state.chat = Some(streamer);
    chat_lines_with(&state, "fix every typo in all my notes", Some("read"));
    chat_lines_with(
        &state,
        "answer these:\n1. Tell us about yourself.\n2. Why Clubhouse?\n3. Describe a team.",
        Some("read"),
    );
    let prompts = prompts.lock().unwrap();
    for prompt in prompts.iter() {
        assert!(!prompt.contains("## This is a big request"));
        assert!(prompt
            .contains("A long message or a numbered list of questions alone is not such a task"));
    }
}

#[test]
fn felix_and_search_find_a_note_that_means_the_same_without_sharing_a_word() {
    let (mut state, _d, ids) = state_with(&[("Graph search", ""), ("Biology", "")]);
    {
        let mut store = state.fresh();
        store.find_note_mut(&ids[0]).unwrap().body =
            "Take the oldest item from a FIFO queue.".into();
        store.find_note_mut(&ids[1]).unwrap().body = "Leaves turn light into sugar.".into();
        store.save().unwrap();
    }
    let meaning = crate::vectors::fake_meaning();
    state.meaning = Some(Arc::clone(&meaning));
    crate::vectors::catch_up(&state.fresh(), &state.vectors, &meaning, 100);
    let (streamer, prompts) = scripted(vec!["It uses a queue [n1]."]);
    state.chat = Some(streamer);
    chat_lines(&state, "explain breadth first please");
    let prompt = prompts.lock().unwrap()[0].clone();
    assert!(
        prompt.contains("close in meaning to the question"),
        "{prompt}"
    );
    assert!(prompt.contains("oldest item from a FIFO queue"));
    assert!(!prompt.contains("Leaves turn light"));
    let found = json_of(
        run(crate::routes::notes::search_notes(
            State(state.clone()),
            Query(serde_json::from_value(serde_json::json!({ "q": "breadth first" })).unwrap()),
        ))
        .map(axum::response::IntoResponse::into_response)
        .unwrap(),
    );
    assert_eq!(found[0]["title"], "Graph search");
    assert_eq!(found[0]["why"]["kind"], "meaning");
}

#[test]
fn felix_works_numbers_out_with_the_calculator_instead_of_guessing() {
    let (mut state, _d, _) = state_with(&[]);
    let (streamer, prompts) = scripted(vec![
        "<tool>{\"name\": \"calculate\", \"steps\": \"f(x) = x^2 + 4*cos(x)\\nx = 1\\nx = x - (2x - 4 sin(x))/(2 - 4 cos(x))\\nf(1.381966)\"}</tool>",
        "Newton jumps to x = -7.472741.",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines_with(&state, "do question 1 of my homework", Some("read"));
    let steps: Vec<&str> = lines.iter().filter_map(|l| l["step"].as_str()).collect();
    assert_eq!(steps, ["Calculated 4 steps"]);
    let prompts = prompts.lock().unwrap();
    assert!(prompts[0].contains("### calculate"));
    assert!(prompts[0].contains("never estimate a value in your head"));
    assert!(prompts[0].contains("\"start with question 1\" means do question 1"));
    assert!(prompts[1].contains("x = -7.4727"), "{}", prompts[1]);
    assert!(prompts[1].contains("f(1.381966) = 2.66067"));
}

#[test]
fn a_practice_answer_is_marked_once_and_never_nudged_into_changing_a_note() {
    let (mut state, _d, _) = state_with(&[]);
    let (streamer, prompts) = scripted(vec![
        "[[incorrect]] Not quite: compare both ends.",
        "unused",
    ]);
    state.chat = Some(streamer);
    let body = chat::ChatBody {
        scope: Default::default(),
        messages: vec![chat::Turn {
            role: "user".into(),
            text: "Quiz answer. Question: why f'(x)=0?\nMy answer: dunno\nMark it: start your reply with [[correct]] or [[incorrect]]".into(),
        }],
        mode: Some("study".into()),
        note: None,
        refs: vec![],
        chat: None,
        files: vec![],
        access: None,
        recent: vec![],
        practice: true,
        mark: false,
    };
    let text = run(async {
        let response = chat_reply(State(state.clone()), Json(body)).await;
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    });
    assert!(
        !text.contains("restart"),
        "the marking is never taken back: {text}"
    );
    assert_eq!(
        prompts.lock().unwrap().len(),
        1,
        "no second try asking for a note change"
    );
    assert!(!prompts.lock().unwrap()[0].contains("## This is a big request"));
    assert!(
        !prompts.lock().unwrap()[0].contains("### edit_note"),
        "marking an answer is read only, so the note-fixing reminder never comes up"
    );
}

#[test]
fn an_agent_that_breaks_mid_answer_is_replaced_cleanly_by_the_text_protocol() {
    struct Breaks;
    impl chat::Conversation for Breaks {
        fn native(&self) -> bool {
            true
        }
        fn say(&mut self, _: &str, exchange: chat::Exchange<'_>) -> anyhow::Result<chat::Reply> {
            (exchange.piece)("Half an answer");
            let _ = (exchange.call)(
                "quiz",
                &serde_json::json!({ "kind": "free_response", "question": "Why?", "answer": "Because." }),
            );
            anyhow::bail!("claude stopped: the model's tool call could not be parsed")
        }
    }
    let (mut state, _d, _) = state_with(&[]);
    state.converse = Some(Arc::new(|_: &chat::Instructions, _: &[chat::ToolSpec]| {
        Some(Box::new(Breaks) as Box<dyn chat::Conversation>)
    }));
    let (streamer, _) = scripted(vec![
        "<tool>{\"name\": \"quiz\", \"kind\": \"free_response\", \"question\": \"Why a queue?\", \"answer\": \"Oldest first.\"}</tool>",
        "Try the card below.",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines(&state, "quiz me");
    let reset = lines
        .iter()
        .position(|l| l.get("reset").is_some())
        .expect("what was shown is cleared");
    let after: Vec<&serde_json::Value> = lines[reset..]
        .iter()
        .filter_map(|l| l.get("quiz"))
        .collect();
    assert_eq!(after.len(), 1, "the redo can ask its own card: {lines:?}");
    assert_eq!(after[0]["question"], "Why a queue?");
    let shown: String = lines[reset..]
        .iter()
        .filter_map(|l| l["t"].as_str())
        .collect();
    assert_eq!(shown, "Try the card below.");
    assert_eq!(
        lines.last().unwrap()["done"],
        true,
        "no error reaches the user"
    );
}

#[test]
fn a_failed_answer_is_tried_once_more_before_the_user_sees_an_error() {
    let (mut state, _d, _) = state_with(&[]);
    let calls = Arc::new(Mutex::new(0));
    let count = Arc::clone(&calls);
    state.chat = Some(Arc::new(
        move |_: &str, _: &str, _: u32, piece: &mut dyn FnMut(&str), _: &mut dyn FnMut()| {
            let mut n = count.lock().unwrap();
            *n += 1;
            if *n == 1 {
                piece("Broken start");
                anyhow::bail!("the model's tool call could not be parsed");
            }
            piece("A min-heap keeps the smallest on top.");
            Ok(chat::Reply {
                text: "A min-heap keeps the smallest on top.".into(),
                spent: None,
            })
        },
    ));
    let lines = chat_lines(&state, "what is a heap?");
    assert!(lines.iter().any(|l| l.get("reset").is_some()));
    assert!(lines.iter().all(|l| l.get("error").is_none()), "{lines:?}");
    assert_eq!(*calls.lock().unwrap(), 2);
}

#[test]
fn a_marking_without_a_verdict_gets_one_from_a_short_check() {
    let (mut state, _d, _) = state_with(&[]);
    let (streamer, prompts) = scripted(vec!["Close, but you left out the endpoints.", "incorrect"]);
    state.chat = Some(streamer);
    let body = chat::ChatBody {
        scope: Default::default(),
        messages: vec![chat::Turn {
            role: "user".into(),
            text: "Quiz answer. Question: what do you compare?\nMy answer: the middle\nA model answer to compare with: f(a), f(b) and critical points".into(),
        }],
        mode: Some("study".into()),
        note: None,
        refs: vec![],
        chat: None,
        files: vec![],
        access: None,
        recent: vec![],
        practice: true,
        mark: true,
    };
    let text = run(async {
        let response = chat_reply(State(state.clone()), Json(body)).await;
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    });
    assert!(text.contains("{\"verdict\":\"incorrect\"}"), "{text}");
    let prompts = prompts.lock().unwrap();
    assert_eq!(prompts.len(), 2);
    assert!(prompts[1].starts_with(crate::routes::felix::JUDGE));
}
