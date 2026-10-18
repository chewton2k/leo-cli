use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Kept {
    native: bool,
    heard: Arc<Mutex<Vec<String>>>,
}

impl chat::Conversation for Kept {
    fn native(&self) -> bool {
        self.native
    }

    fn say(&mut self, text: &str, exchange: chat::Exchange<'_>) -> anyhow::Result<chat::Reply> {
        self.heard.lock().unwrap().push(text.to_string());
        (exchange.piece)("[[correct]] Ready.");
        Ok("[[correct]] Ready.".into())
    }
}

fn request(state: &AppState, value: serde_json::Value) {
    let body = serde_json::from_value(value).unwrap();
    let raw = run(async {
        let response = chat_reply(State(state.clone()), Json(body)).await;
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        axum::body::to_bytes(response.into_body(), 1_000_000)
            .await
            .unwrap()
    });
    let lines: Vec<serde_json::Value> = std::str::from_utf8(&raw)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(lines.iter().all(|line| line.get("error").is_none()));
    assert!(lines.iter().any(|line| line["done"] == true));
}

fn append(messages: &mut Vec<serde_json::Value>, question: &str) {
    messages.push(serde_json::json!({"role":"assistant", "text":"[[correct]] Ready."}));
    messages.push(serde_json::json!({"role":"user", "text":question}));
}

#[test]
fn all_styles_reuse_native_and_text_sessions_without_repeating_documents_and_refresh_changed_material(
) {
    for native in [false, true] {
        for mode in chat::MODES {
            let (mut state, _dir, notes) = state_with(&[("Practice notes", "")]);
            state.room = Some(Arc::new(|| 360_000));
            let note = notes[0].clone();
            run(state.with_store(move |store| {
                store.find_note_mut(&note).unwrap().body = "FIRST_NOTE_MARKER".into();
                store.save().unwrap();
                Ok(())
            }))
            .unwrap();
            let first = crate::chat_files::add(
                &state.chats,
                "study-cache-0001",
                "first.pdf",
                &"d".repeat(300_000),
                chrono::Utc::now(),
            )
            .unwrap();
            let heard = Arc::new(Mutex::new(Vec::new()));
            let opened = Arc::new(AtomicUsize::new(0));
            let (ears, opens) = (Arc::clone(&heard), Arc::clone(&opened));
            state.chat = Some(Arc::new(|_, _, _, _, _| {
                panic!("the retained session should answer")
            }));
            state.converse = Some(Arc::new(move |_, _| {
                opens.fetch_add(1, Ordering::SeqCst);
                Some(Box::new(Kept {
                    native,
                    heard: Arc::clone(&ears),
                }) as Box<dyn chat::Conversation>)
            }));
            let mut files = vec![first.id];
            let mut messages =
                vec![serde_json::json!({"role":"user", "text":"Explain this practice material."})];
            for turn in 0..4 {
                if turn == 2 {
                    let note = notes[0].clone();
                    run(state.with_store(move |store| {
                        store.find_note_mut(&note).unwrap().body = "UPDATED_NOTE_MARKER".into();
                        store.save().unwrap();
                        Ok(())
                    }))
                    .unwrap();
                    files.push(
                        crate::chat_files::add(
                            &state.chats,
                            "study-cache-0001",
                            "second.pdf",
                            "SECOND_DOC_MARKER",
                            chrono::Utc::now(),
                        )
                        .unwrap()
                        .id,
                    );
                }
                if turn > 0 {
                    append(&mut messages, "Explain the next step.");
                }
                request(
                    &state,
                    serde_json::json!({"chat":"study-cache-0001", "mode":mode, "access":"auto", "practice":mode=="study", "note":notes[0], "files":files, "messages":messages}),
                );
            }
            let seen = heard.lock().unwrap();
            assert_eq!(opened.load(Ordering::SeqCst), 1, "{mode}, native={native}");
            assert!(seen[0].contains("first.pdf") && seen[0].contains("FIRST_NOTE_MARKER"));
            assert!(!seen[1].contains("first.pdf") && !seen[1].contains("FIRST_NOTE_MARKER"));
            assert!(
                seen[2].contains("SECOND_DOC_MARKER") && seen[2].contains("UPDATED_NOTE_MARKER")
            );
            assert!(
                !seen[3].contains("SECOND_DOC_MARKER") && !seen[3].contains("UPDATED_NOTE_MARKER")
            );
            if mode == "study" {
                assert!(seen[0].chars().count() < 90_000);
            } else {
                assert!(seen[0].chars().count() > 200_000);
            }
            drop(seen);
            append(&mut messages, "Help with an ordinary message.");
            request(
                &state,
                serde_json::json!({"chat":"study-cache-0001", "mode":mode, "access":"read", "practice":false, "files":files, "messages":messages}),
            );
            assert_eq!(opened.load(Ordering::SeqCst), 2);
        }
    }
}

#[test]
fn an_unchanged_practice_snapshot_is_not_repeated_but_changed_code_and_questions_are_sent() {
    for native in [false, true] {
        let (mut state, _dir, _) = state_with(&[]);
        let id = "material-cache-0001";
        let framework = serde_json::from_value(serde_json::json!({"title":"Coding", "track":"coding", "company":"", "role":"", "language":"Python", "style":"coach", "date":"", "weekly_minutes":120, "skills":["Approach"], "wants":""})).unwrap();
        crate::interview::configure(&state.chats, id, 0, framework).unwrap();
        for index in 0..2 {
            let challenge = serde_json::from_value(serde_json::json!({"id":format!("material-problem-{index}"), "title":format!("Problem {index}"), "question":"Find the pair.", "minutes":30, "skills":["Approach"], "topics":["Arrays"], "origin":"generated", "sources":[]})).unwrap();
            crate::interview::set_challenge(&state.chats, id, challenge).unwrap();
        }
        let heard = Arc::new(Mutex::new(Vec::new()));
        let ears = Arc::clone(&heard);
        state.chat = Some(Arc::new(|_, _, _, _, _| {
            panic!("the retained session should answer")
        }));
        state.converse = Some(Arc::new(move |_, _| {
            Some(Box::new(Kept {
                native,
                heard: Arc::clone(&ears),
            }) as Box<dyn chat::Conversation>)
        }));
        let mut messages =
            vec![serde_json::json!({"role":"user", "text":"Help with this problem."})];
        for index in 0..4 {
            if index > 0 {
                append(&mut messages, "Explain the next step.");
            }
            if index == 2 {
                let item = crate::interview::load(&state.chats, id).unwrap();
                crate::interview::change(
                    &state.chats,
                    id,
                    crate::interview::Change::Workspace {
                        revision: item.revision,
                        challenge: "material-problem-1".into(),
                        workspace: crate::interview::Workspace {
                            answer: "UPDATED_CODE_MARKER".into(),
                            ..Default::default()
                        },
                    },
                )
                .unwrap();
            }
            request(
                &state,
                serde_json::json!({"chat":id, "mode":"interview", "access":"ask", "interview_problem":if index==3 {"material-problem-0"} else {"material-problem-1"}, "messages":messages}),
            );
        }
        let seen = heard.lock().unwrap();
        assert!(seen[0].contains("<interview_material>"));
        assert!(!seen[1].contains("<interview_material>"));
        assert!(
            seen[2].contains("<interview_material>") && seen[2].contains("UPDATED_CODE_MARKER")
        );
        assert!(seen[3].contains("<interview_material>") && seen[3].contains("material-problem-0"));
    }
}
