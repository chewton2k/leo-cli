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
        interview_problem: None,
        mark: false,
        selection: None,
        session: None,
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
        interview_problem: None,
        mark: false,
        selection: None,
        session: None,
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
        interview_problem: None,
        mark: false,
        selection: None,
        session: None,
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
    chat_lines_in(state, question, access, None, None)
}

fn chat_lines_in(
    state: &AppState,
    question: &str,
    access: Option<&str>,
    session: Option<tools::Session>,
    selection: Option<chat::Selection>,
) -> Vec<serde_json::Value> {
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
        access: access.map(str::to_string),
        recent: vec![],
        practice: false,
        interview_problem: None,
        mark: false,
        selection,
        session,
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
fn permissions_changed_during_a_reply_gate_notes_and_repeating_tasks_in_both_protocols() {
    struct Changing {
        steering: crate::steer::Steering,
        access: tools::Access,
        call: tools::Call,
    }
    impl chat::Conversation for Changing {
        fn native(&self) -> bool {
            true
        }
        fn say(&mut self, _: &str, exchange: chat::Exchange<'_>) -> anyhow::Result<chat::Reply> {
            for id in self.steering.open_ids() {
                self.steering.set_access(&id, self.access).unwrap();
            }
            (exchange.call)(&self.call.name, &self.call.args);
            (exchange.piece)("Ready.");
            Ok("Ready.".into())
        }
    }
    for native in [false, true] {
        for (before, after) in [
            ("auto", "ask"),
            ("ask", "auto"),
            ("auto", "read"),
            ("read", "auto"),
            ("ask", "read"),
            ("read", "ask"),
        ] {
            for kind in ["note", "task"] {
                let (mut state, _dir, _) = state_with(&[]);
                let steering = state.steering.clone();
                let access = tools::Access::named(after);
                let call = if kind == "task" {
                    tools::Call {
                        name: "add_task".into(),
                        args: serde_json::json!({"title":"Interview studying", "date":"2026-10-12", "repeat":"mon, tue, wed, thu, fri"}),
                    }
                } else {
                    tools::Call {
                        name: "create_note".into(),
                        args: serde_json::json!({"title":"Interview studying", "body":"Practise interview questions", "folder":""}),
                    }
                };
                if native {
                    let call = call.clone();
                    state.converse = Some(Arc::new(
                        move |_: &chat::Instructions, _: &[chat::ToolSpec]| {
                            Some(Box::new(Changing {
                                steering: steering.clone(),
                                access,
                                call: call.clone(),
                            })
                                as Box<dyn chat::Conversation>)
                        },
                    ));
                    state.chat = Some(scripted(vec!["Ready."]).0);
                } else {
                    let first = Arc::new(std::sync::atomic::AtomicBool::new(true));
                    state.chat = Some(Arc::new(
                        move |_: &str,
                              _: &str,
                              _: u32,
                              piece: &mut dyn FnMut(&str),
                              _: &mut dyn FnMut()| {
                            let reply = if first.swap(false, std::sync::atomic::Ordering::SeqCst) {
                                for id in steering.open_ids() {
                                    steering.set_access(&id, access).unwrap();
                                }
                                let mut args = call.args.clone();
                                args["name"] = serde_json::json!(call.name);
                                format!("<tool>{args}</tool>")
                            } else {
                                "Ready.".into()
                            };
                            piece(&reply);
                            Ok(reply.into())
                        },
                    ));
                }
                let lines = chat_lines_with(&state, "Prepare it", Some(before));
                let proposals: Vec<_> = lines.iter().filter_map(|l| l.get("proposal")).collect();
                assert_eq!(
                    proposals.len(),
                    usize::from(after != "read"),
                    "{native} {before}->{after} kind={kind}: {lines:?}"
                );
                if let Some(proposal) = proposals.first() {
                    assert_eq!(proposal["state"] == "applied", after == "auto");
                }
                let count = if kind == "task" {
                    crate::tasks::count(&state.chats)
                } else {
                    state.store.lock().unwrap().store.notes.len()
                };
                assert_eq!(count, usize::from(after == "auto"));
                if kind == "task" && after == "auto" {
                    assert_eq!(
                        crate::tasks::load_all(&state.chats)[0].repeat,
                        [0, 1, 2, 3, 4]
                    );
                }
                assert!(lines.iter().all(|l| l.get("error").is_none()));
            }
        }
    }
}

#[test]
fn permission_updates_validate_access_and_only_change_the_live_answer() {
    let (state, _dir, _) = state_with(&[]);
    let steer = state.steering.open();
    let other = state.steering.open();
    let post = |access: &str| {
        run(crate::routes::felix::change_answer_access(
            State(state.clone()),
            Path(steer.id().into()),
            Json(crate::routes::felix::Permission {
                access: access.into(),
            }),
        ))
    };
    for (name, access) in [
        ("auto", tools::Access::Auto),
        ("ask", tools::Access::Ask),
        ("read", tools::Access::Read),
    ] {
        assert_eq!(post(name).status(), StatusCode::OK);
        assert_eq!(steer.access(tools::Access::Ask), access);
        assert_eq!(other.access(tools::Access::Ask), tools::Access::Ask);
    }
    assert_eq!(post("anything").status(), StatusCode::BAD_REQUEST);
    assert_eq!(steer.access(tools::Access::Auto), tools::Access::Read);
    let id = steer.id().to_string();
    drop(steer);
    assert_eq!(
        run(crate::routes::felix::change_answer_access(
            State(state),
            Path(id),
            Json(crate::routes::felix::Permission {
                access: "auto".into()
            })
        ))
        .status(),
        StatusCode::GONE
    );
}

#[test]
fn closing_the_reply_prevents_further_automatic_changes() {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let mut desk = tools::Desk::new(vec![], 32000)
        .with_access(tools::Access::Auto)
        .with_reply(tx);
    assert_eq!(desk.current_access(), tools::Access::Auto);
    drop(rx);
    assert_eq!(desk.current_access(), tools::Access::Read);
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
            messages: messages.to_vec(),
            mode: None,
            note: None,
            refs: vec![],
            chat: Some(id.into()),
            files: vec![],
            access: None,
            recent: vec![],
            practice: false,
            interview_problem: None,
            mark: false,
            selection: None,
            session: None,
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
        interview_problem: None,
        mark: false,
        selection: None,
        session: None,
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
        interview_problem: None,
        mark: true,
        selection: None,
        session: None,
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

#[test]
fn felix_quotes_the_uploaded_file_behind_a_note_and_says_where_it_is() {
    let (mut state, dir, ids) = state_with(&[("Lecture 4 slides", "cs130")]);
    {
        let mut store = state.fresh();
        store.find_note_mut(&ids[0]).unwrap().body =
            "## Shortest paths\nDijkstra fails with negative edges.".into();
        store.save().unwrap();
    }
    let folder = dir.path().join("attachments").join(&ids[0]);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("week4.pptx"), b"slides").unwrap();
    state.reader = Some(Arc::new(|file: UploadFile, _: &mut dyn FnMut(&str)| {
        assert_eq!(file.name, "week4.pptx");
        Ok("## Slide 1\nWeek 4\n\n## Slide 2\n\"Dijkstra's algorithm assumes all edge weights are non-negative.\"\nUse Bellman-Ford otherwise.".into())
    }));
    let (streamer, prompts) = scripted(vec![
        "<tool>{\"name\": \"read_original\", \"note\": \"n1\", \"find\": \"non-negative\"}</tool>",
        "The slide says \"Dijkstra's algorithm assumes all edge weights are non-negative.\" [f1 p2] Your note says it more briefly [n1 L2].",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines(
        &state,
        "what exactly did the slides say about negative edges?",
    );
    let files: Vec<&serde_json::Value> =
        lines.iter().filter(|l| l.get("files").is_some()).collect();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0]["files"][0]["name"], "week4.pptx");
    assert_eq!(files[0]["files"][0]["note"], ids[0].as_str());
    let prompts = prompts.lock().unwrap();
    assert!(prompts[0].contains("Made from an uploaded file (week4.pptx)"));
    assert!(
        prompts[0].contains("2| Dijkstra fails with negative edges."),
        "note lines are numbered"
    );
    assert!(
        prompts[1].contains("<file id=\"f1\" name=\"week4.pptx\""),
        "{}",
        prompts[1]
    );
    assert!(prompts[1].contains("(slide 2)\n4| ## Slide 2\n5| \"Dijkstra's algorithm assumes all edge weights are non-negative.\""), "{}", prompts[1]);
    assert!(prompts[1].contains("[f1 p4]"));
    drop(prompts);

    let shown = json_of(run(crate::routes::felix::original_passage(
        State(state.clone()),
        Path((ids[0].clone(), "week4.pptx".into())),
        axum::extract::Query(serde_json::from_value(serde_json::json!({"at": "p2"})).unwrap()),
    )));
    assert_eq!(shown["label"], "slide 2");
    assert_eq!(
        shown["lines"][1]["text"],
        "\"Dijkstra's algorithm assumes all edge weights are non-negative.\""
    );
    let outside = run(crate::routes::felix::original_passage(
        State(state.clone()),
        Path((ids[0].clone(), "../notes".into())),
        axum::extract::Query(serde_json::from_value(serde_json::json!({"at": "p2"})).unwrap()),
    ));
    assert_eq!(outside.status(), StatusCode::NOT_FOUND);
}

#[test]
fn an_edit_copied_with_line_numbers_still_finds_its_text() {
    let (mut state, _d, ids) = state_with(&[("Graph traversals", "")]);
    {
        let mut store = state.fresh();
        store.find_note_mut(&ids[0]).unwrap().body = "## BFS\nBFS uses a stack.".into();
        store.save().unwrap();
    }
    let (streamer, _) = scripted(vec![
        "<tool>{\"name\": \"edit_note\", \"note\": \"n1\", \"find\": \"2| BFS uses a stack.\", \"replace\": \"2| BFS uses a queue.\"}</tool>",
        "Suggested.",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines(&state, "please fix my graph traversals note");
    let proposal = lines
        .iter()
        .find_map(|l| l.get("proposal"))
        .expect("a suggestion");
    assert_eq!(proposal["find"], "BFS uses a stack.");
    assert_eq!(proposal["replace"], "BFS uses a queue.");
}

#[test]
fn interview_tools_share_the_native_and_text_paths_without_changing_notes_or_restarting_answers() {
    struct InterviewNative;
    impl chat::Conversation for InterviewNative {
        fn native(&self) -> bool {
            true
        }
        fn say(&mut self, _: &str, exchange: chat::Exchange<'_>) -> anyhow::Result<chat::Reply> {
            let bad = (exchange.call)(
                "interview_feedback",
                &serde_json::json!({"data":"{\"attempt\":\"invented-attempt\",\"summary\":\"Great\",\"scores\":[]}"}),
            );
            assert!(bad.starts_with("That did not work:"));
            let read = (exchange.call)("interview_progress", &serde_json::json!({}));
            assert!(read.contains("Coding practice"));
            (exchange.piece)("Let's work through your approach.");
            Ok("Let's work through your approach.".into())
        }
    }
    for native in [false, true] {
        let (mut state, _d, _) = state_with(&[]);
        let framework: crate::interview::Framework = serde_json::from_value(serde_json::json!({"title":"Coding practice","track":"coding","company":"Example","role":"Engineer","language":"Python","style":"coach","date":"","weekly_minutes":120,"skills":["Approach"],"wants":"Hints first"})).unwrap();
        crate::interview::configure(&state.chats, "interview-chat-01", 0, framework).unwrap();
        state.chat = Some(scripted(vec![r#"<tool>{"name":"interview_feedback","data":"{\"attempt\":\"invented-attempt\",\"summary\":\"Great\",\"scores\":[]}"}</tool>"#, r#"<tool>{"name":"interview_progress"}</tool>"#, "Let's work through your approach."]).0);
        if native {
            state.converse = Some(Arc::new(|given, specs| {
                assert!(given.native.contains("Mode: interview"));
                assert!(specs.iter().any(|s| s.name == "interview_feedback"));
                assert!(specs.iter().any(|s| s.name == "interview_attempt"));
                Some(Box::new(InterviewNative) as Box<dyn chat::Conversation>)
            }));
        }
        let body: chat::ChatBody = serde_json::from_value(serde_json::json!({"chat":"interview-chat-01","mode":"interview","messages":[{"role":"user","text":"Help me practise."}]})).unwrap();
        let bytes = run(async {
            let reply = chat_reply(State(state.clone()), Json(body)).await;
            axum::body::to_bytes(reply.into_body(), 1_000_000)
                .await
                .unwrap()
        });
        let lines: Vec<serde_json::Value> = std::str::from_utf8(&bytes)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert!(lines
            .iter()
            .all(|line| line.get("error").is_none() && line.get("restart").is_none()));
        assert!(lines.iter().all(|line| line.get("interview").is_none()));
        assert!(lines
            .iter()
            .any(|line| line["tool"] == "interview_progress"));
        let shown: String = lines.iter().filter_map(|line| line["t"].as_str()).collect();
        assert_eq!(shown, "Let's work through your approach.");
        assert!(crate::interview::load(&state.chats, "interview-chat-01")
            .unwrap()
            .attempts
            .is_empty());
        assert!(state.store.lock().unwrap().store.notes.is_empty());
    }
}

#[test]
fn a_study_session_plans_names_ideas_and_refuses_questions_once_covered() {
    let (mut state, _d, _) = state_with(&[("Heaps", "cs130")]);
    let (streamer, prompts) = scripted(vec![
        "<tool>{\"name\": \"study_plan\", \"ideas\": \"Root | Sift down\"}</tool>",
        "<tool>{\"name\": \"quiz\", \"kind\": \"free_response\", \"question\": \"What is at the root?\", \"answer\": \"the minimum\"}</tool>",
        "<tool>{\"name\": \"quiz\", \"kind\": \"free_response\", \"question\": \"What is at the root?\", \"answer\": \"the minimum\", \"idea\": \"1\"}</tool>",
        "Try it.",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines(&state, "quiz me on heaps");
    let plan: Vec<&serde_json::Value> = lines.iter().filter_map(|l| l.get("plan")).collect();
    assert_eq!(plan, [&serde_json::json!({"ideas": ["Root", "Sift down"]})]);
    let quizzes: Vec<&serde_json::Value> = lines.iter().filter_map(|l| l.get("quiz")).collect();
    assert_eq!(quizzes.len(), 1, "{lines:?}");
    assert_eq!(quizzes[0]["idea"], 1);
    assert!(prompts.lock().unwrap()[2]
        .contains("give \"idea\": the number of an idea still open: 1 (Root), 2 (Sift down)"));

    let (streamer, prompts) = scripted(vec![
        "<tool>{\"name\": \"quiz\", \"kind\": \"free_response\", \"question\": \"More?\", \"answer\": \"x\", \"idea\": \"2\"}</tool>",
        "You covered both ideas well.",
    ]);
    state.chat = Some(streamer);
    let covered = tools::Session {
        ideas: vec!["Root".into(), "Sift down".into()],
        closed: vec![true, true],
        asked: 2,
    };
    let lines = chat_lines_in(&state, "[[correct]] yes", None, Some(covered), None);
    assert!(lines.iter().all(|l| l.get("quiz").is_none()), "{lines:?}");
    assert!(prompts.lock().unwrap()[1].contains(tools::SESSION_DONE));

    let (streamer, _) = scripted(vec![
        "<tool>{\"name\": \"end_session\"}</tool>",
        "Good work today.",
    ]);
    state.chat = Some(streamer);
    let open = tools::Session {
        ideas: vec!["Root".into()],
        closed: vec![false],
        asked: 1,
    };
    let lines = chat_lines_in(&state, "that's enough for today", None, Some(open), None);
    assert!(
        lines
            .iter()
            .any(|l| l.get("session_end") == Some(&serde_json::json!(true))),
        "{lines:?}"
    );
}

#[test]
fn a_selected_passage_reaches_felix_as_quoted_material() {
    let (mut state, _d, _) = state_with(&[("Heaps", "cs130")]);
    let (streamer, prompts) = scripted(vec!["It means the root is smallest."]);
    state.chat = Some(streamer);
    let selection = chat::Selection {
        title: "Heaps".into(),
        from: 3,
        to: 3,
        text: "keeps the minimum".into(),
    };
    chat_lines_in(&state, "what does this mean?", None, None, Some(selection));
    let prompt = prompts.lock().unwrap()[0].clone();
    assert!(prompt.contains(
        "<selected_passage note=\"Heaps\" lines=\"3\">\nkeeps the minimum\n</selected_passage>"
    ));
    assert!(
        prompt.find("<selected_passage").unwrap() < prompt.rfind("what does this mean?").unwrap()
    );
}

#[test]
fn company_preparation_searches_and_creates_a_seeded_coding_workspace_on_both_protocols() {
    const CHALLENGE: &str = r#"{"id":"challenge-prep-01","title":"Pair sum","question":"Return the two matching indices.","minutes":25,"skills":["Approach"],"topics":["Hash maps"],"origin":"generated","sources":[{"title":"Public candidate report","url":"https://example.com/interview","kind":"reported"}],"coding":{"language":"Python","starter":"def pair_sum(nums, target):\n    pass","difficulty":"easy","examples":[{"input":"nums=[2,3], target=5","output":"[0,1]"}],"constraints":["Exactly one pair exists."]}}"#;
    struct PrepareNative {
        company: bool,
    }
    impl chat::Conversation for PrepareNative {
        fn native(&self) -> bool {
            true
        }
        fn say(&mut self, _: &str, exchange: chat::Exchange<'_>) -> anyhow::Result<chat::Reply> {
            let _ = (exchange.call)("interview_progress", &serde_json::json!({}));
            if self.company {
                let found = (exchange.call)(
                    "web_search",
                    &serde_json::json!({"query":"Pinterest software engineer past interview questions"}),
                );
                assert!(found.contains("Public candidate report"));
            }
            let mut incomplete: serde_json::Value = serde_json::from_str(CHALLENGE).unwrap();
            incomplete.as_object_mut().unwrap().remove("coding");
            let correction = (exchange.call)(
                "interview_challenge",
                &serde_json::json!({"data":incomplete.to_string()}),
            );
            assert!(correction.contains("unsolved function starter"));
            let saved = (exchange.call)(
                "interview_challenge",
                &serde_json::json!({"data":CHALLENGE}),
            );
            assert!(!saved.starts_with("That did not work:"), "{saved}");
            (exchange.piece)("Your problem is ready in the workspace.");
            Ok("Your problem is ready in the workspace.".into())
        }
    }
    for native in [false, true] {
        for company in if native { [true, false] } else { [false, true] } {
            let (mut state, _dir, _) = state_with(&[]);
            let queries = Arc::new(Mutex::new(Vec::new()));
            let seen = Arc::clone(&queries);
            state.web = Some(crate::Web {
                search: Arc::new(move |query| {
                    seen.lock().unwrap().push(query.to_owned());
                    Ok(vec![crate::WebHit {
                        title: "Public candidate report".into(),
                        url: "https://example.com/interview".into(),
                        snippet: "Unverified interview topics.".into(),
                    }])
                }),
                page: Arc::new(|_| Ok(String::new())),
                needed: Arc::new(|| true),
            });
            let framework = serde_json::from_value(serde_json::json!({"title":"Coding practice","track":"coding","company":if company {"Pinterest"} else {""},"role":"Software engineer","language":"Python","style":"coach","date":"","weekly_minutes":120,"skills":["Approach"],"wants":""})).unwrap();
            crate::interview::configure(&state.chats, "interview-prep-01", 0, framework).unwrap();
            let mut replies = vec![r#"<tool>{"name":"interview_progress"}</tool>"#.to_string()];
            if company {
                replies.push(r#"<tool>{"name":"web_search","query":"Pinterest software engineer past interview questions"}</tool>"#.to_string());
            }
            replies.push(format!(
                "<tool>{}</tool>",
                serde_json::json!({"name":"interview_challenge","data":CHALLENGE})
            ));
            replies.push("Your problem is ready in the workspace.".into());
            let replies = Arc::new(Mutex::new(replies.into_iter()));
            state.chat = Some(Arc::new(move |system, _, _, piece, _| {
                assert!(system.contains("automatically research before creating the challenge"));
                let reply = replies.lock().unwrap().next().unwrap();
                piece(&reply);
                Ok(reply.into())
            }));
            if native {
                state.converse = Some(Arc::new(move |given, specs| {
                    assert!(given.native.contains("unsolved function/class signature"));
                    assert!(specs.iter().any(|s| s.name == "web_search"));
                    Some(Box::new(PrepareNative { company }) as Box<dyn chat::Conversation>)
                }));
            }
            let body = serde_json::from_value(serde_json::json!({"chat":"interview-prep-01","mode":"interview","messages":[{"role":"user","text":"Start my practice and prepare a challenge."}]})).unwrap();
            let bytes = run(async {
                let reply = chat_reply(State(state.clone()), Json(body)).await;
                axum::body::to_bytes(reply.into_body(), 1_000_000)
                    .await
                    .unwrap()
            });
            let lines: Vec<serde_json::Value> = std::str::from_utf8(&bytes)
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            assert!(lines
                .iter()
                .all(|line| line.get("error").is_none() && line.get("restart").is_none()));
            assert_eq!(queries.lock().unwrap().len(), usize::from(company));
            let item = crate::interview::load(&state.chats, "interview-prep-01").unwrap();
            assert_eq!(
                item.workspace.answer,
                "def pair_sum(nums, target):\n    pass"
            );
            assert_eq!(
                item.challenge.unwrap().coding.unwrap().examples[0].output,
                "[0,1]"
            );
            assert!(state.store.lock().unwrap().store.notes.is_empty());
        }
    }
}

#[test]
fn felix_organizes_tasks_folders_and_notes_through_cards_that_undo() {
    let (mut state, _d, ids) = state_with(&[("Heaps", "cs130"), ("Old draft", "")]);
    {
        let mut store = state.fresh();
        store.create_dir("archive");
        store.save().unwrap();
    }
    let (streamer, prompts) = scripted(vec![
        "<tool>{\"name\": \"add_task\", \"title\": \"Submit lab\", \"date\": \"2026-10-16\", \"time\": \"23:59\", \"type\": \"assignment\"}</tool>",
        "<tool>{\"name\": \"move_note\", \"note\": \"Heaps\", \"folder\": \"archive\"}</tool>",
        "<tool>{\"name\": \"delete_note\", \"note\": \"Old draft\"}</tool>",
        "<tool>{\"name\": \"make_folder\", \"folder\": \"archive/labs\"}</tool>",
        "Here is what I suggest.",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines(&state, "tidy things up");
    let cards: Vec<serde_json::Value> = lines
        .iter()
        .filter_map(|l| l.get("proposal").cloned())
        .collect();
    let kinds: Vec<&str> = cards.iter().map(|c| c["kind"].as_str().unwrap()).collect();
    assert_eq!(
        kinds,
        ["add_task", "move_note", "delete_note", "make_folder"],
        "{lines:?}"
    );
    assert_eq!(
        cards[0]["summary"],
        "Add the task “Submit lab” · Fri, Oct 16 at 11:59 pm"
    );
    assert!(prompts.lock().unwrap()[1].contains("not done yet"));
    assert!(
        state.fresh().notes.iter().any(|n| n.id == ids[1]),
        "Ask first changes nothing by itself"
    );

    let act = |p: &serde_json::Value| {
        json_of(run(crate::routes::organize::act_route(
            State(state.clone()),
            Json(serde_json::from_value(p.clone()).unwrap()),
        )))
    };
    let moved = act(&cards[1]);
    assert_eq!(moved["state"], "applied");
    assert_eq!(
        state.fresh().find_note(&ids[0]).unwrap().directory,
        "archive"
    );
    act(&moved["undo"]);
    assert_eq!(state.fresh().find_note(&ids[0]).unwrap().directory, "cs130");
    let trashed = act(&cards[2]);
    assert!(state.fresh().find_note(&ids[1]).is_none());
    act(&trashed["undo"]);
    assert!(
        state.fresh().find_note(&ids[1]).is_some(),
        "Undo brings the note back from the trash"
    );
    let added = act(&cards[0]);
    let task = added["task"].as_str().unwrap().to_string();
    assert!(crate::tasks::load(&state.chats, &task).is_some());
    act(&added["undo"]);
    assert!(crate::tasks::load(&state.chats, &task).is_none());
    let folder = act(&cards[3]);
    assert!(state.fresh().dir_exists("archive/labs"));
    act(&folder["undo"]);
    assert!(!state.fresh().dir_exists("archive/labs"));
    for bad in ["", "..", "../outside", "/", "missing"] {
        let refused = run(crate::routes::organize::act_route(
            State(state.clone()),
            Json(
                serde_json::from_value(
                    serde_json::json!({"kind": "remove_folder", "summary": "x", "path": bad}),
                )
                .unwrap(),
            ),
        ));
        assert_eq!(refused.status(), 409, "{bad:?} is refused");
    }
    assert!(state.fresh().dir_exists("archive"));
    assert!(state.fresh().find_note(&ids[0]).is_some());
}

#[test]
fn felix_in_auto_does_it_at_once_and_read_only_can_only_look() {
    let (mut state, _d, ids) = state_with(&[("Heaps", "cs130")]);
    {
        let mut store = state.fresh();
        store.create_dir("archive");
        store.save().unwrap();
    }
    let (streamer, _) = scripted(vec![
        "<tool>{\"name\": \"move_folder\", \"folder\": \"cs130\", \"into\": \"archive\"}</tool>",
        "Moved it.",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines_with(&state, "put cs130 in archive", Some("auto"));
    let card = lines.iter().find_map(|l| l.get("proposal")).unwrap();
    assert_eq!(card["state"], "applied");
    assert_eq!(card["path"], "archive/cs130");
    assert_eq!(card["undo"]["kind"], "move_folder");
    assert_eq!(
        state.fresh().find_note(&ids[0]).unwrap().directory,
        "archive/cs130"
    );

    let (streamer, prompts) = scripted(vec![
        "<tool>{\"name\": \"delete_note\", \"note\": \"Heaps\"}</tool>",
        "<tool>{\"name\": \"list_schedule\", \"days\": \"3\"}</tool>",
        "Nothing is due.",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines_with(&state, "delete heaps and show my week", Some("read"));
    assert!(lines.iter().all(|l| l.get("proposal").is_none()));
    let prompts = prompts.lock().unwrap();
    assert!(
        !prompts[0].contains("\"delete_note\""),
        "read only leaves the change tools out of the manual"
    );
    assert!(prompts[1].contains("read only"));
    assert!(prompts[2].contains("Tasks and events from"));
    assert!(state.fresh().find_note(&ids[0]).is_some());
}

#[test]
fn felix_renames_pins_restores_and_deletes_folders_only_when_asked_and_finds_old_chats() {
    let (mut state, _d, ids) = state_with(&[("Heaps", "s26"), ("Gone note", "")]);
    {
        let mut store = state.fresh();
        store.delete_notes(std::slice::from_ref(&ids[1]));
        store.save().unwrap();
    }
    crate::chats::save(
        &state.chats,
        "chat-midterm-0001",
        serde_json::from_value(
            serde_json::json!({"title": "Midterm prep", "mode": "study", "messages": [
                {"role": "user", "text": "What is on the midterm?"},
                {"role": "assistant", "text": "The midterm covers heaps and Dijkstra."}
            ]}),
        )
        .unwrap(),
        chrono::Utc::now(),
    )
    .unwrap();
    let (streamer, prompts) = scripted(vec![
        "<tool>{\"name\": \"list_trash\"}</tool>",
        "<tool>{\"name\": \"search_chats\", \"query\": \"midterm\"}</tool>",
        "<tool>{\"name\": \"rename_note\", \"note\": \"Heaps\", \"title\": \"Heaps and priority queues\"}</tool>",
        "<tool>{\"name\": \"rename_folder\", \"folder\": \"s26\", \"new_name\": \"Spring 2026\"}</tool>",
        "<tool>{\"name\": \"pin_note\", \"note\": \"Heaps\"}</tool>",
        "<tool>{\"name\": \"restore_note\", \"note\": \"Gone note\"}</tool>",
        "<tool>{\"name\": \"delete_folder\", \"folder\": \"Spring 2026\"}</tool>",
        "Done.",
    ]);
    state.chat = Some(streamer);
    let lines = chat_lines_with(&state, "sort things out", Some("auto"));
    {
        let prompts = prompts.lock().unwrap();
        let at = prompts[1]
            .rfind("<tool_result name=\"list_trash")
            .unwrap_or(0);
        assert!(
            prompts[1].contains("“Gone note” from the top level"),
            "{}",
            &prompts[1][at..(at + 700).min(prompts[1].len())]
        );
        assert!(
            prompts[2].contains("“Midterm prep”")
                && prompts[2].contains("covers heaps and Dijkstra")
        );
    }
    let cards: Vec<&serde_json::Value> = lines.iter().filter_map(|l| l.get("proposal")).collect();
    let states: Vec<(&str, &str)> = cards
        .iter()
        .map(|c| {
            (
                c["kind"].as_str().unwrap(),
                c["state"].as_str().unwrap_or("new"),
            )
        })
        .collect();
    assert_eq!(
        states,
        [
            ("rename_note", "applied"),
            ("rename_folder", "applied"),
            ("pin_note", "applied"),
            ("restore_note", "applied"),
            ("delete_folder", "new"),
        ],
        "a folder delete waits for the user even in Auto"
    );
    let store = state.fresh();
    let heaps = store.find_note(&ids[0]).unwrap();
    assert_eq!(
        (heaps.title.as_str(), heaps.directory.as_str(), heaps.pinned),
        ("Heaps and priority queues", "Spring 2026", true)
    );
    assert!(store.find_note(&ids[1]).is_some());
    drop(store);

    let act = |p: &serde_json::Value| {
        json_of(run(crate::routes::organize::act_route(
            State(state.clone()),
            Json(serde_json::from_value(p.clone()).unwrap()),
        )))
    };
    act(&cards[1]["undo"]);
    assert_eq!(state.fresh().find_note(&ids[0]).unwrap().directory, "s26");
    let deleted = act(&serde_json::json!({"kind": "delete_folder", "summary": "x", "path": "s26"}));
    assert!(state.fresh().find_note(&ids[0]).is_none());
    act(&deleted["undo"]);
    assert_eq!(
        state.fresh().find_note(&ids[0]).unwrap().directory,
        "s26",
        "Undo brings the folder and its notes back"
    );
}

#[test]
fn interview_problems_and_calendar_context_work_on_native_and_text_sessions() {
    const PROBLEM: &str = r#"{"id":"challenge-context-02","title":"Category coverage","question":"Find a shortest covering segment.","minutes":45,"skills":["Approach"],"topics":["Sliding window"],"origin":"generated","sources":[],"coding":{"language":"Python","starter":"def coverage(feed, required):\n    pass","difficulty":"medium","examples":[{"input":"feed=['art'], required=['art']","output":"[0,0]"}],"constraints":["At most 200000 categories."]}}"#;
    struct ProblemsNative;
    impl chat::Conversation for ProblemsNative {
        fn native(&self) -> bool {
            true
        }
        fn say(&mut self, _: &str, exchange: chat::Exchange<'_>) -> anyhow::Result<chat::Reply> {
            let progress = (exchange.call)("interview_progress", &serde_json::json!({}));
            assert!(progress.contains("challenge-context-01"));
            let _ = (exchange.call)("list_schedule", &serde_json::json!({"days":"7"}));
            let created =
                (exchange.call)("interview_challenge", &serde_json::json!({"data":PROBLEM}));
            assert!(created.contains("challenge-context-02"));
            let reopened = (exchange.call)(
                "interview_problem",
                &serde_json::json!({"challenge":"challenge-context-01"}),
            );
            assert!(reopened.contains("Saved implementation"));
            (exchange.piece)("Problem 2 is saved. Problem 1 is open again.");
            Ok("Problem 2 is saved. Problem 1 is open again.".into())
        }
    }
    for native in [false, true] {
        let (mut state, _dir, _) = state_with(&[]);
        let framework = serde_json::from_value(serde_json::json!({"title":"Coding practice","track":"coding","company":"","role":"Intern","language":"Python","style":"coach","date":"","weekly_minutes":120,"skills":["Approach"],"wants":""})).unwrap();
        let id = "interview-context-01";
        crate::interview::configure(&state.chats, id, 0, framework).unwrap();
        let mut first: serde_json::Value = serde_json::from_str(PROBLEM).unwrap();
        first["id"] = "challenge-context-01".into();
        let first = crate::interview::set_challenge(
            &state.chats,
            id,
            serde_json::from_value(first).unwrap(),
        )
        .unwrap();
        crate::interview::change(
            &state.chats,
            id,
            crate::interview::Change::Workspace {
                revision: first.revision,
                challenge: "challenge-context-01".into(),
                workspace: crate::interview::Workspace {
                    answer: "Saved implementation".into(),
                    ..Default::default()
                },
            },
        )
        .unwrap();
        let replies = vec![
            r#"<tool>{"name":"interview_progress"}</tool>"#.to_string(),
            r#"<tool>{"name":"list_schedule","days":"7"}</tool>"#.to_string(),
            format!(
                "<tool>{}</tool>",
                serde_json::json!({"name":"interview_challenge","data":PROBLEM})
            ),
            r#"<tool>{"name":"interview_problem","challenge":"challenge-context-01"}</tool>"#
                .to_string(),
            "Problem 2 is saved. Problem 1 is open again.".into(),
        ];
        let replies = Arc::new(Mutex::new(replies.into_iter()));
        state.chat = Some(Arc::new(move |system, _, _, piece, _| {
            assert!(system.contains("give me this problem"));
            assert!(!system.contains("interview_schedule"));
            let reply = replies.lock().unwrap().next().unwrap();
            piece(&reply);
            Ok(reply.into())
        }));
        if native {
            state.converse = Some(Arc::new(|given, specs| {
                assert!(given.native.contains("give me this problem"));
                assert!(specs.iter().any(|s| s.name == "interview_problem"));
                assert!(specs.iter().any(|s| s.name == "list_schedule"));
                Some(Box::new(ProblemsNative) as Box<dyn chat::Conversation>)
            }));
        }
        let body = serde_json::from_value(serde_json::json!({"chat":id,"mode":"interview","messages":[{"role":"user","text":"Can you give me this problem please, then reopen problem 1?"}]})).unwrap();
        let bytes = run(async {
            let reply = chat_reply(State(state.clone()), Json(body)).await;
            axum::body::to_bytes(reply.into_body(), 1_000_000)
                .await
                .unwrap()
        });
        let lines: Vec<serde_json::Value> = std::str::from_utf8(&bytes)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert!(lines
            .iter()
            .all(|line| line.get("error").is_none() && line.get("restart").is_none()));
        let item = crate::interview::load(&state.chats, id).unwrap();
        assert_eq!(item.problems.len(), 2);
        assert_eq!(item.challenge.unwrap().id, "challenge-context-01");
        assert_eq!(item.workspace.answer, "Saved implementation");
        let text: String = lines.iter().filter_map(|line| line["t"].as_str()).collect();
        assert_eq!(text, "Problem 2 is saved. Problem 1 is open again.");
    }
}

#[test]
fn code_suggestions_are_cards_in_ask_and_applied_by_the_page_in_auto_in_native_and_text() {
    struct CodeNative;
    impl chat::Conversation for CodeNative {
        fn native(&self) -> bool {
            true
        }
        fn say(&mut self, _: &str, exchange: chat::Exchange<'_>) -> anyhow::Result<chat::Reply> {
            let read = (exchange.call)("interview_code", &serde_json::json!({}));
            assert!(read.contains("return 0"));
            (exchange.call)(
                "interview_code_edit",
                &serde_json::json!({"summary":"Use the corrected result", "find":"return 0", "replace":"return 1"}),
            );
            (exchange.piece)("Review the code change card.");
            Ok("Review the code change card.".into())
        }
    }
    for native in [false, true] {
        for access in ["ask", "auto", "read"] {
            let (mut state, _dir, _) = state_with(&[]);
            let id = "code-felix-practice";
            let framework = serde_json::from_value(serde_json::json!({"title":"Code review", "track":"coding", "company":"", "role":"", "language":"Python", "style":"coach", "date":"", "weekly_minutes":120, "skills":["Approach"], "wants":""})).unwrap();
            crate::interview::configure(&state.chats, id, 0, framework).unwrap();
            let challenge = serde_json::from_value(serde_json::json!({"id":"code-felix-problem", "title":"Return value", "question":"Return a result", "minutes":30, "skills":["Approach"], "origin":"generated", "sources":[], "coding":{"language":"Python", "starter":"def solve():\n    return 0", "difficulty":"easy", "examples":[{"input":"1","output":"1"}], "constraints":["Explain"]}})).unwrap();
            crate::interview::set_challenge(&state.chats, id, challenge).unwrap();
            state.chat = Some(scripted(vec![r#"<tool>{"name":"interview_code"}</tool>"#, r#"<tool>{"name":"interview_code_edit","summary":"Use the corrected result","find":"return 0","replace":"return 1"}</tool>"#, "Review the code change card."]).0);
            if native {
                state.converse = Some(Arc::new(|_, _| {
                    Some(Box::new(CodeNative) as Box<dyn chat::Conversation>)
                }));
            }
            let body: chat::ChatBody = serde_json::from_value(serde_json::json!({"chat":id,"mode":"interview","interview_problem":"code-felix-problem","access":access,"messages":[{"role":"user","text":"Suggest a focused correction."}]})).unwrap();
            let bytes = run(async {
                let response = chat_reply(State(state.clone()), Json(body)).await;
                axum::body::to_bytes(response.into_body(), 1_000_000)
                    .await
                    .unwrap()
            });
            let lines: Vec<serde_json::Value> = std::str::from_utf8(&bytes)
                .unwrap()
                .lines()
                .map(|l| serde_json::from_str(l).unwrap())
                .collect();
            assert!(lines.iter().all(|l| l.get("error").is_none()), "{lines:?}");
            let proposals: Vec<_> = lines.iter().filter_map(|l| l.get("proposal")).collect();
            assert_eq!(
                proposals.len(),
                usize::from(access != "read"),
                "{native} {access}: {lines:?}"
            );
            assert_eq!(
                crate::interview::load(&state.chats, id)
                    .unwrap()
                    .workspace
                    .answer,
                "def solve():\n    return 0"
            );
            if let Some(p) = proposals.first() {
                assert_ne!(p["state"], "applied");
                assert_eq!(
                    p["auto"].as_bool().unwrap_or(false),
                    access == "auto",
                    "{native} {access}"
                );
                let response = run(crate::routes::organize::act_route(
                    State(state.clone()),
                    Json(serde_json::from_value((*p).clone()).unwrap()),
                ));
                assert_eq!(response.status(), StatusCode::OK);
                assert_eq!(
                    crate::interview::load(&state.chats, id)
                        .unwrap()
                        .workspace
                        .answer,
                    "def solve():\n    return 1"
                );
            }
        }
    }
    assert!(tools::check(&tools::Call { name: "interview_code_edit".into(), args: serde_json::json!({"summary":"Delete the redundant line", "find":"redundant", "replace":""}) }).is_ok());
}

#[test]
fn a_follow_up_in_the_same_chat_reuses_one_warm_session_and_sends_only_whats_new() {
    struct Echo {
        heard: Arc<Mutex<Vec<String>>>,
        stopped: Arc<std::sync::atomic::AtomicUsize>,
    }
    impl chat::Conversation for Echo {
        fn native(&self) -> bool {
            true
        }
        fn say(&mut self, text: &str, exchange: chat::Exchange<'_>) -> anyhow::Result<chat::Reply> {
            self.heard.lock().unwrap().push(text.to_string());
            let reply = "Heaps keep the smallest key on top [n1].";
            (exchange.piece)(reply);
            Ok(chat::Reply::from(reply))
        }
    }
    impl Drop for Echo {
        fn drop(&mut self) {
            self.stopped
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }
    let (mut state, _d, _) = state_with(&[("Heaps", "algo"), ("Graphs", "algo")]);
    let heard = Arc::new(Mutex::new(Vec::new()));
    let opened = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let stopped = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (ears, opens, stops) = (
        Arc::clone(&heard),
        Arc::clone(&opened),
        Arc::clone(&stopped),
    );
    state.converse = Some(Arc::new(
        move |_: &chat::Instructions, _: &[chat::ToolSpec]| {
            opens.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Some(Box::new(Echo {
                heard: Arc::clone(&ears),
                stopped: Arc::clone(&stops),
            }) as Box<dyn chat::Conversation>)
        },
    ));
    state.chat = Some(scripted(vec!["never used"]).0);
    let ask = |chat: &str, messages: &[(&str, &str)], practice: bool| {
        let body = chat::ChatBody {
            messages: messages
                .iter()
                .map(|(role, text)| chat::Turn {
                    role: role.to_string(),
                    text: text.to_string(),
                })
                .collect(),
            mode: None,
            note: None,
            refs: vec![],
            chat: Some(chat.into()),
            files: vec![],
            access: None,
            recent: vec![],
            practice,
            interview_problem: None,
            mark: false,
            selection: None,
            session: None,
        };
        let text = run(async {
            let response = chat_reply(State(state.clone()), Json(body)).await;
            String::from_utf8(
                axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .unwrap()
                    .to_vec(),
            )
            .unwrap()
        });
        text.lines()
            .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
            .collect::<Vec<_>>()
    };
    let alive = || {
        opened.load(std::sync::atomic::Ordering::SeqCst)
            - stopped.load(std::sync::atomic::Ordering::SeqCst)
    };
    let first = ask("warm-chat-0001", &[("user", "what is a heap?")], false);
    assert!(first.iter().any(|l| l.get("done").is_some()));
    assert_eq!(opened.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(alive(), 1, "the finished answer's session stays warm");
    let second = ask(
        "warm-chat-0001",
        &[
            ("user", "what is a heap?"),
            ("assistant", "Heaps keep the smallest key on top."),
            ("user", "and how does heap sort use it?"),
        ],
        false,
    );
    assert!(second.iter().any(|l| l.get("done").is_some()));
    assert_eq!(
        opened.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "the follow-up reused the warm session"
    );
    let said = heard.lock().unwrap().clone();
    assert!(said[0].contains("<conversation>") && said[0].contains("title=\"Heaps\""));
    assert!(!said[1].contains("<conversation>"), "{}", said[1]);
    assert!(
        !said[1].contains("title=\"Heaps\""),
        "a note already read is not sent again"
    );
    assert!(said[1].contains("User: and how does heap sort use it?"));
    assert!(said[1].contains("<today>"));
    let sources = second.iter().find_map(|l| l.get("sources")).unwrap();
    assert!(sources
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["n"] == 1 && s["title"] == "Heaps"));
    ask("warm-chat-0002", &[("user", "what is a graph?")], false);
    assert_eq!(opened.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert_eq!(
        alive(),
        1,
        "another chat stops the warm session before starting its own"
    );
    ask(
        "warm-chat-0002",
        &[
            ("user", "what is a graph?"),
            ("assistant", "x"),
            ("user", "mark my answer"),
        ],
        true,
    );
    assert_eq!(alive(), 1, "practice marking never keeps a second session");
    ask(
        "warm-chat-0002",
        &[
            ("user", "edited question"),
            ("assistant", "x"),
            ("user", "and then?"),
        ],
        false,
    );
    assert_eq!(
        alive(),
        1,
        "an edited history starts fresh, still one session"
    );
    assert_eq!(opened.load(std::sync::atomic::Ordering::SeqCst), 4);
}

#[test]
fn felix_reads_every_test_case_and_fixes_one_as_a_card_in_ask_applied_in_auto_refused_in_read() {
    for access in ["ask", "auto", "read"] {
        let (mut state, _dir, _) = state_with(&[]);
        let id = "tests-felix-practice";
        let framework = serde_json::from_value(serde_json::json!({"title":"Tests", "track":"coding", "company":"", "role":"", "language":"Python", "style":"coach", "date":"", "weekly_minutes":120, "skills":["Approach"], "wants":""})).unwrap();
        crate::interview::configure(&state.chats, id, 0, framework).unwrap();
        let challenge = serde_json::from_value(serde_json::json!({"id":"tests-felix-problem", "title":"Add", "question":"Add two numbers", "minutes":30, "skills":["Approach"], "origin":"generated", "sources":[], "coding":{"language":"Python", "starter":"def add(a, b):\n    pass", "difficulty":"easy", "examples":[{"input":"1, 2","output":"3"}], "constraints":["Small"]}})).unwrap();
        crate::interview::set_challenge(&state.chats, id, challenge).unwrap();
        let key = serde_json::json!({"entry":"add","tests":[{"id":"t1","args":[1,2],"expected":3},{"id":"t2","args":[2,2],"expected":5,"hidden":true}],"criteria":[{"id":"c1","text":"Adds"}],"verified":true});
        crate::db::for_chats(&state.chats)
            .with(|c| c.execute("INSERT INTO interview_keys VALUES (?1, 'tests-felix-problem', 'ready', '2026-10-13T12:00:00Z', ?2)", rusqlite::params![id, key.to_string()]))
            .unwrap();
        let (chat, prompts) = scripted(vec![
            r#"<tool>{"name":"interview_test","data":"{\"action\":\"edit\",\"summary\":\"2 + 2 is 4\",\"test\":{\"id\":\"t2\",\"args\":[2,2],\"expected\":4,\"hidden\":true}}"}</tool>"#,
            "Fixed the hidden test.",
        ]);
        state.chat = Some(chat);
        let body: chat::ChatBody = serde_json::from_value(serde_json::json!({"chat":id,"mode":"interview","interview_problem":"tests-felix-problem","access":access,"messages":[{"role":"user","text":"Is test 2 right?"}]})).unwrap();
        let bytes = run(async {
            let response = chat_reply(State(state.clone()), Json(body)).await;
            axum::body::to_bytes(response.into_body(), 1_000_000)
                .await
                .unwrap()
        });
        let lines: Vec<serde_json::Value> = std::str::from_utf8(&bytes)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        let first = prompts.lock().unwrap()[0].clone();
        assert!(
            first.contains("<test_cases>")
                && first.contains("add(2, 2)")
                && first.contains("\"hidden\":true"),
            "{access}: Felix sees every test"
        );
        let proposals: Vec<_> = lines.iter().filter_map(|l| l.get("proposal")).collect();
        let expected = crate::interview_keys::ready(&state.chats, id, "tests-felix-problem")
            .unwrap()
            .tests[1]
            .expected
            .clone();
        match access {
            "read" => {
                assert!(proposals.is_empty());
                assert_eq!(expected, serde_json::json!(5));
            }
            "ask" => {
                assert_eq!(proposals[0]["kind"], "interview_test");
                assert_ne!(proposals[0]["state"], "applied");
                assert_eq!(proposals[0]["before"]["expected"], 5);
                assert_eq!(expected, serde_json::json!(5));
            }
            _ => {
                assert_eq!(proposals[0]["state"], "applied", "{lines:?}");
                assert_eq!(expected, serde_json::json!(4));
                let undo: tools::Proposal =
                    serde_json::from_value(proposals[0]["undo"].clone()).unwrap();
                let response = run(crate::routes::organize::act_route(
                    State(state.clone()),
                    Json(undo),
                ));
                assert_eq!(response.status(), StatusCode::OK);
                assert_eq!(
                    crate::interview_keys::ready(&state.chats, id, "tests-felix-problem")
                        .unwrap()
                        .tests[1]
                        .expected,
                    serde_json::json!(5)
                );
            }
        }
    }
}

#[test]
fn felix_lists_every_practice_reads_another_and_changes_a_problem_by_access() {
    for access in ["ask", "auto", "read"] {
        let (mut state, _dir, _) = state_with(&[]);
        let framework = |title: &str| {
            serde_json::from_value::<crate::interview::Framework>(serde_json::json!({"title":title, "track":"coding", "company":"", "role":"", "language":"Python", "style":"coach", "date":"", "weekly_minutes":120, "skills":["Approach"], "wants":""})).unwrap()
        };
        let id = "edit-felix-practice";
        let other = "other-felix-practice";
        crate::interview::configure(&state.chats, id, 0, framework("Arrays")).unwrap();
        crate::interview::configure(&state.chats, other, 0, framework("Graphs")).unwrap();
        let challenge = |cid: &str, title: &str| {
            serde_json::from_value::<crate::interview::Challenge>(serde_json::json!({"id":cid, "title":title, "question":"Add two numbers.", "minutes":30, "skills":["Approach"], "origin":"generated", "sources":[], "coding":{"language":"Python", "starter":"def add(a, b):\n    pass", "difficulty":"easy", "examples":[{"input":"1, 2","output":"3"}], "constraints":["Small"]}})).unwrap()
        };
        crate::interview::set_challenge(&state.chats, id, challenge("edit-felix-problem", "Add"))
            .unwrap();
        crate::interview::set_challenge(
            &state.chats,
            other,
            challenge("other-felix-problem", "Shortest path"),
        )
        .unwrap();
        crate::interview::change(
            &state.chats,
            id,
            serde_json::from_value(serde_json::json!({"action":"workspace","revision":crate::interview::load(&state.chats, id).unwrap().revision,"challenge":"edit-felix-problem","workspace":{"answer":"def add(a, b):\n    return a + b","reasoning":"","board":[]}})).unwrap(),
        )
        .unwrap();
        let (chat, prompts) = scripted(vec![
            r#"<tool>{"name":"interview_list"}</tool>"#,
            r#"<tool>{"name":"interview_progress","interview":"other-felix-practice"}</tool>"#,
            r#"<tool>{"name":"interview_problem_edit","data":"{\"summary\":\"Say the numbers may be negative\",\"changes\":{\"question\":\"Add two numbers, which may be negative.\",\"coding\":{\"constraints\":[\"-100 <= a, b <= 100\"]}}}"}</tool>"#,
            "Updated the problem.",
        ]);
        state.chat = Some(chat);
        let body: chat::ChatBody = serde_json::from_value(serde_json::json!({"chat":id,"mode":"interview","interview_problem":"edit-felix-problem","access":access,"messages":[{"role":"user","text":"Mention negative numbers."}]})).unwrap();
        let bytes = run(async {
            let response = chat_reply(State(state.clone()), Json(body)).await;
            axum::body::to_bytes(response.into_body(), 1_000_000)
                .await
                .unwrap()
        });
        let lines: Vec<serde_json::Value> = std::str::from_utf8(&bytes)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        let seen = prompts.lock().unwrap().join("\n");
        assert!(
            seen.contains("\"current\":true")
                && seen.contains("Shortest path")
                && seen.contains("other-felix-practice"),
            "{access}: Felix sees every practice"
        );
        assert!(
            seen.contains("other-felix-problem"),
            "{access}: Felix reads another practice"
        );
        let proposals: Vec<_> = lines.iter().filter_map(|l| l.get("proposal")).collect();
        let item = crate::interview::load(&state.chats, id).unwrap();
        let question = item.challenge.as_ref().unwrap().question.clone();
        assert_eq!(
            item.workspace.answer, "def add(a, b):\n    return a + b",
            "the user's code is never touched"
        );
        match access {
            "read" => {
                assert!(proposals.is_empty());
                assert_eq!(question, "Add two numbers.");
            }
            "ask" => {
                assert_eq!(proposals[0]["kind"], "interview_problem_edit");
                assert_ne!(proposals[0]["state"], "applied");
                assert_eq!(question, "Add two numbers.");
            }
            _ => {
                assert_eq!(proposals[0]["state"], "applied", "{lines:?}");
                assert_eq!(question, "Add two numbers, which may be negative.");
                assert_eq!(
                    item.problems[0].challenge.question, question,
                    "the saved problem changes too"
                );
                assert_eq!(
                    item.challenge
                        .as_ref()
                        .unwrap()
                        .coding
                        .as_ref()
                        .unwrap()
                        .constraints,
                    ["-100 <= a, b <= 100"]
                );
                assert_eq!(
                    item.challenge
                        .as_ref()
                        .unwrap()
                        .coding
                        .as_ref()
                        .unwrap()
                        .examples
                        .len(),
                    1,
                    "unchanged fields stay"
                );
                let undo: tools::Proposal =
                    serde_json::from_value(proposals[0]["undo"].clone()).unwrap();
                let response = run(crate::routes::organize::act_route(
                    State(state.clone()),
                    Json(undo.clone()),
                ));
                assert_eq!(response.status(), StatusCode::OK);
                assert_eq!(
                    crate::interview::load(&state.chats, id)
                        .unwrap()
                        .challenge
                        .unwrap()
                        .question,
                    "Add two numbers."
                );
                let stale = run(crate::routes::organize::act_route(
                    State(state.clone()),
                    Json(undo),
                ));
                assert_eq!(
                    stale.status(),
                    StatusCode::CONFLICT,
                    "a change built on an old version is refused"
                );
            }
        }
    }
}

#[test]
fn felix_tags_older_problems_in_one_change_by_access() {
    for access in ["ask", "auto", "read"] {
        let (mut state, _dir, _) = state_with(&[]);
        let id = "topics-felix-practice";
        let framework = serde_json::from_value::<crate::interview::Framework>(serde_json::json!({"title":"Coding", "track":"coding", "company":"", "role":"", "language":"Python", "style":"coach", "date":"", "weekly_minutes":120, "skills":["Approach"], "wants":""})).unwrap();
        crate::interview::configure(&state.chats, id, 0, framework).unwrap();
        let challenge = |cid: &str, title: &str, topics: &[&str]| {
            serde_json::from_value::<crate::interview::Challenge>(serde_json::json!({"id":cid, "title":title, "question":"Solve it.", "minutes":30, "skills":["Approach"], "topics":topics, "origin":"generated", "sources":[], "coding":{"language":"Python", "starter":"def solve():\n    pass", "difficulty":"easy", "examples":[{"input":"1","output":"1"}], "constraints":["Small"]}})).unwrap()
        };
        for (cid, title, topics) in [
            ("topics-old-one", "Number of islands", &[][..]),
            ("topics-old-two", "Climbing stairs", &[][..]),
            ("topics-new-three", "Course schedule", &["Graphs"][..]),
        ] {
            crate::interview::set_challenge(&state.chats, id, challenge(cid, title, topics))
                .unwrap();
        }
        let (chat, _prompts) = scripted(vec![
            r#"<tool>{"name":"interview_topics","data":"{\"summary\":\"Tag the older problems\",\"problems\":[{\"challenge\":\"topics-old-one\",\"topics\":[\"DFS\",\"Graphs\"]},{\"challenge\":\"topics-old-two\",\"topics\":[\"Dynamic programming\"]},{\"challenge\":\"topics-new-three\",\"topics\":[\"Graphs\"]}]}"}</tool>"#,
            "Sorted them.",
        ]);
        state.chat = Some(chat);
        let body: chat::ChatBody = serde_json::from_value(serde_json::json!({"chat":id,"mode":"interview","access":access,"messages":[{"role":"user","text":"Sort my problems into topics."}]})).unwrap();
        let bytes = run(async {
            let response = chat_reply(State(state.clone()), Json(body)).await;
            axum::body::to_bytes(response.into_body(), 1_000_000)
                .await
                .unwrap()
        });
        let lines: Vec<serde_json::Value> = std::str::from_utf8(&bytes)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        let proposals: Vec<_> = lines.iter().filter_map(|l| l.get("proposal")).collect();
        let topics = |cid: &str| {
            crate::interview::problem(&crate::interview::load(&state.chats, id).unwrap(), cid)
                .unwrap()
                .topics
        };
        match access {
            "read" => {
                assert!(proposals.is_empty());
                assert!(topics("topics-old-one").is_empty());
            }
            "ask" => {
                assert_eq!(proposals.len(), 1, "one card for every problem");
                assert_eq!(proposals[0]["kind"], "interview_topics");
                assert_eq!(
                    proposals[0]["title"], "Topics for 2 problems",
                    "an unchanged problem is left out"
                );
                assert_eq!(proposals[0]["changes"][0]["number"], 1);
                assert!(topics("topics-old-one").is_empty());
            }
            _ => {
                assert_eq!(proposals[0]["state"], "applied", "{lines:?}");
                assert_eq!(topics("topics-old-one"), ["DFS", "Graphs"]);
                assert_eq!(topics("topics-old-two"), ["Dynamic programming"]);
                let open = crate::interview::load(&state.chats, id).unwrap();
                assert_eq!(open.challenge.unwrap().topics, ["Graphs"]);
                let undo: tools::Proposal =
                    serde_json::from_value(proposals[0]["undo"].clone()).unwrap();
                let response = run(crate::routes::organize::act_route(
                    State(state.clone()),
                    Json(undo.clone()),
                ));
                assert_eq!(response.status(), StatusCode::OK);
                assert!(topics("topics-old-one").is_empty() && topics("topics-old-two").is_empty());
                let stale = run(crate::routes::organize::act_route(
                    State(state.clone()),
                    Json(undo),
                ));
                assert_eq!(stale.status(), StatusCode::CONFLICT);
            }
        }
    }
}

#[test]
fn felix_task_descriptions_use_the_same_cards_permissions_and_undo_in_both_protocols() {
    struct TaskCall(tools::Call);
    impl chat::Conversation for TaskCall {
        fn native(&self) -> bool {
            true
        }
        fn say(&mut self, _: &str, exchange: chat::Exchange<'_>) -> anyhow::Result<chat::Reply> {
            (exchange.call)(&self.0.name, &self.0.args);
            (exchange.piece)("Ready.");
            Ok("Ready.".into())
        }
    }
    for native in [false, true] {
        for access in ["ask", "auto", "read"] {
            let (mut state, _dir, _) = state_with(&[]);
            let install = |state: &mut AppState, name: &str, args: serde_json::Value| {
                let call = tools::Call {
                    name: name.into(),
                    args,
                };
                if native {
                    state.converse = Some(Arc::new(
                        move |_: &chat::Instructions, _: &[chat::ToolSpec]| {
                            Some(Box::new(TaskCall(call.clone())) as Box<dyn chat::Conversation>)
                        },
                    ));
                    state.chat = Some(scripted(vec!["Ready."]).0);
                } else {
                    let mut args = call.args.clone();
                    args["name"] = serde_json::json!(call.name);
                    let text = format!("<tool>{args}</tool>");
                    let replies =
                        Arc::new(Mutex::new(vec![text, "Ready.".to_string()].into_iter()));
                    state.chat = Some(Arc::new(
                        move |_: &str,
                              _: &str,
                              _: u32,
                              piece: &mut dyn FnMut(&str),
                              _: &mut dyn FnMut()| {
                            let reply = replies
                                .lock()
                                .unwrap()
                                .next()
                                .unwrap_or_else(|| "Ready.".into());
                            piece(&reply);
                            Ok(reply.into())
                        },
                    ));
                }
            };
            let act = |state: &AppState, proposal: &serde_json::Value| {
                json_of(run(crate::routes::organize::act_route(
                    State(state.clone()),
                    Json(serde_json::from_value(proposal.clone()).unwrap()),
                )))
            };
            let settle = |state: &AppState, lines: &[serde_json::Value]| {
                let proposal = lines.iter().find_map(|line| line.get("proposal"));
                if access == "read" {
                    assert!(proposal.is_none());
                    return None;
                }
                let proposal = proposal.unwrap();
                Some(if access == "ask" {
                    act(state, proposal)
                } else {
                    proposal.clone()
                })
            };
            install(
                &mut state,
                "add_task",
                serde_json::json!({"title": "Read heaps", "description": "Read chapter 3\nBring café notes"}),
            );
            let lines =
                chat_lines_with(&state, "add a study task with these details", Some(access));
            settle(&state, &lines);
            if access == "read" {
                assert!(crate::tasks::load_all(&state.chats).is_empty());
                continue;
            }
            let task = crate::tasks::load_all(&state.chats).remove(0);
            assert_eq!(task.description, "Read chapter 3\nBring café notes");
            for value in ["Focus on examples", ""] {
                install(
                    &mut state,
                    "update_task",
                    serde_json::json!({"task": task.id, "description": value}),
                );
                let lines = chat_lines_with(&state, "change the task description", Some(access));
                let applied = settle(&state, &lines).unwrap();
                assert_eq!(
                    crate::tasks::load(&state.chats, &task.id)
                        .unwrap()
                        .description,
                    value
                );
                act(&state, &applied["undo"]);
                assert_eq!(
                    crate::tasks::load(&state.chats, &task.id)
                        .unwrap()
                        .description,
                    task.description
                );
            }
            install(
                &mut state,
                "list_schedule",
                serde_json::json!({"days": "7"}),
            );
            let mut desk = tools::Desk::new(vec![], 8000);
            let read = crate::routes::organize::run(
                &state,
                &mut desk,
                tools::Access::Read,
                &tools::Call {
                    name: "list_schedule".into(),
                    args: serde_json::json!({"days":"7"}),
                },
            );
            assert!(read.result.contains("Read chapter 3\\nBring café notes"));
            let mut before_delete = crate::tasks::load(&state.chats, &task.id).unwrap();
            before_delete.repeat = vec![0, 2];
            before_delete.done_on = vec![chrono::NaiveDate::from_ymd_opt(2026, 10, 19).unwrap()];
            before_delete.note = Some("note-example".into());
            crate::tasks::put(&state.chats, &before_delete).unwrap();
            install(
                &mut state,
                "delete_task",
                serde_json::json!({"task": task.id}),
            );
            let lines = chat_lines_with(&state, "delete that study task", Some(access));
            let deleted = settle(&state, &lines).unwrap();
            assert!(crate::tasks::load(&state.chats, &task.id).is_none());
            act(&state, &deleted["undo"]);
            let restored = crate::tasks::load(&state.chats, &task.id).unwrap();
            assert_eq!(restored.repeat, before_delete.repeat);
            assert_eq!(restored.done_on, before_delete.done_on);
            assert_eq!(restored.note, before_delete.note);
            assert!(restored.updated_at > before_delete.updated_at);
            assert_eq!(
                crate::tasks::load(&state.chats, &task.id)
                    .unwrap()
                    .description,
                task.description
            );
        }
    }
}
