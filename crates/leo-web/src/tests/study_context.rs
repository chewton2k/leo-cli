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
fn native_and_text_sessions_are_reused_without_repeating_documents_and_refresh_changed_material() {
    for native in [false, true] {
        for practice in [false, true] {
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
            state.chat = Some(Arc::new(|_, _, _, _, _, _| {
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
                    serde_json::json!({"chat":"study-cache-0001", "access":"auto", "practice":practice, "note":notes[0], "files":files, "messages":messages}),
                );
            }
            let seen = heard.lock().unwrap();
            assert_eq!(
                opened.load(Ordering::SeqCst),
                1,
                "practice={practice}, native={native}"
            );
            assert!(seen[0].contains("first.pdf") && seen[0].contains("FIRST_NOTE_MARKER"));
            assert!(!seen[1].contains("first.pdf") && !seen[1].contains("FIRST_NOTE_MARKER"));
            assert!(
                seen[2].contains("SECOND_DOC_MARKER") && seen[2].contains("UPDATED_NOTE_MARKER")
            );
            assert!(
                !seen[3].contains("SECOND_DOC_MARKER") && !seen[3].contains("UPDATED_NOTE_MARKER")
            );
            if practice {
                assert!(seen[0].chars().count() < 90_000);
            } else {
                assert!(seen[0].chars().count() > 200_000);
            }
            drop(seen);
            append(&mut messages, "Help with an ordinary message.");
            request(
                &state,
                serde_json::json!({"chat":"study-cache-0001", "access":"read", "practice":false, "files":files, "messages":messages}),
            );
            assert_eq!(opened.load(Ordering::SeqCst), 2);
        }
    }
}

#[test]
fn retained_tool_context_counts_toward_the_warm_session_limit() {
    struct Large;
    impl chat::Conversation for Large {
        fn native(&self) -> bool {
            true
        }
        fn context_chars(&self) -> Option<usize> {
            Some(100_000)
        }
        fn say(&mut self, _: &str, exchange: chat::Exchange<'_>) -> anyhow::Result<chat::Reply> {
            (exchange.piece)("[[correct]] Ready.");
            Ok("[[correct]] Ready.".into())
        }
    }
    let (mut state, _dir, _) = state_with(&[]);
    state.room = Some(Arc::new(|| 32_000));
    state.chat = Some(Arc::new(|_, _, _, _, _, _| {
        panic!("the retained session should answer")
    }));
    let opened = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&opened);
    state.converse = Some(Arc::new(move |_, _| {
        count.fetch_add(1, Ordering::SeqCst);
        Some(Box::new(Large))
    }));
    let mut messages = vec![serde_json::json!({"role":"user","text":"Read my material."})];
    for turn in 0..2 {
        if turn > 0 {
            append(&mut messages, "Continue.");
        }
        request(
            &state,
            serde_json::json!({"chat":"large-context-0001","messages":messages}),
        );
        assert!(!state.warm.holding());
    }
    assert_eq!(opened.load(Ordering::SeqCst), 2);
}
