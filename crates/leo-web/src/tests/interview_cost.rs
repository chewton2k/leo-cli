use super::*;

fn prepared(state: &AppState) -> (String, Vec<String>) {
    let id = "grade-cost-practice";
    let framework = serde_json::from_value(serde_json::json!({"title":"Practice","track":"coding","company":"","role":"","language":"Python","style":"coach","date":"","weekly_minutes":120,"skills":["Approach"],"wants":""})).unwrap();
    crate::interview::configure(&state.chats, id, 0, framework).unwrap();
    let challenge = serde_json::from_value(serde_json::json!({"id":"grade-cost-problem","title":"Pairs","question":"Find a pair.","minutes":30,"skills":["Approach"],"origin":"generated","sources":[]})).unwrap();
    let mut item = crate::interview::set_challenge(&state.chats, id, challenge).unwrap();
    for i in 0..2 {
        item = crate::interview::change(
            &state.chats,
            id,
            crate::interview::Change::Submit {
                revision: item.revision,
                attempt: format!("grade-cost-attempt-{i}"),
                challenge: "grade-cost-problem".into(),
                workspace: crate::interview::Workspace {
                    answer: "Use a hash map.".into(),
                    ..Default::default()
                },
                tests: vec![],
            },
        )
        .unwrap();
    }
    let key = serde_json::json!({"criteria":[{"id":"c1","text":"Approach"}],"verified":true});
    crate::db::for_chats(&state.chats).with(|c| c.execute(
        "INSERT INTO interview_keys VALUES (?1, 'grade-cost-problem', 'ready', '2026-10-13T12:00:00Z', ?2)",
        rusqlite::params![id, key.to_string()],
    )).unwrap();
    (
        id.into(),
        item.attempts.iter().map(|a| a.id.clone()).collect(),
    )
}

async fn grade(state: AppState, id: String, attempt: String) -> Response {
    crate::routes::interviews::grade(
        State(state),
        Path(id),
        Json(serde_json::from_value(serde_json::json!({"attempt":attempt})).unwrap()),
    )
    .await
}

#[test]
fn concurrent_grades_share_only_the_same_attempt_and_cached_results_do_not_call_the_model() {
    for same in [true, false] {
        let (mut state, _dir, _) = state_with(&[]);
        let (id, attempts) = prepared(&state);
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = Arc::clone(&calls);
        let (started, observed) = std::sync::mpsc::channel();
        let release = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let wait = Arc::clone(&release);
        state.quick = Some(Arc::new(move |_, _, _| {
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            started.send(()).unwrap();
            let (lock, condition) = &*wait;
            let held = lock.lock().unwrap();
            let (held, timed) = condition
                .wait_timeout_while(held, std::time::Duration::from_secs(5), |ready| !*ready)
                .unwrap();
            assert!(!timed.timed_out());
            drop(held);
            Ok(r#"{"criteria":[{"id":"c1","verdict":"met","why":"A suitable approach."}],"overall":"Ready"}"#.into())
        }));
        run(async {
            let first = tokio::spawn(grade(state.clone(), id.clone(), attempts[0].clone()));
            let observed = tokio::task::spawn_blocking(move || {
                observed
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap();
                observed
            })
            .await
            .unwrap();
            let second_attempt = if same { &attempts[0] } else { &attempts[1] };
            let second = tokio::spawn(grade(state.clone(), id.clone(), second_attempt.clone()));
            if same {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            } else {
                tokio::task::spawn_blocking(move || {
                    observed.recv_timeout(std::time::Duration::from_secs(5))
                })
                .await
                .unwrap()
                .unwrap();
            }
            let (lock, condition) = &*release;
            *lock.lock().unwrap() = true;
            condition.notify_all();
            for response in [first.await.unwrap(), second.await.unwrap()] {
                assert_eq!(response.status(), StatusCode::OK);
            }
            assert_eq!(
                calls.load(std::sync::atomic::Ordering::SeqCst),
                if same { 1 } else { 2 }
            );
            assert_eq!(
                grade(state.clone(), id.clone(), attempts[0].clone())
                    .await
                    .status(),
                StatusCode::OK
            );
            assert_eq!(
                calls.load(std::sync::atomic::Ordering::SeqCst),
                if same { 1 } else { 2 }
            );
        });
    }
}

#[test]
fn a_failed_grade_releases_its_admission_for_retry() {
    let (mut state, _dir, _) = state_with(&[]);
    let (id, attempts) = prepared(&state);
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = Arc::clone(&calls);
    state.quick = Some(Arc::new(move |_, _, _| {
        if count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
            anyhow::bail!("fixture failure");
        }
        Ok(r#"{"criteria":[{"id":"c1","verdict":"met"}],"overall":"Ready"}"#.into())
    }));
    assert_eq!(
        run(grade(state.clone(), id.clone(), attempts[0].clone())).status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        run(grade(state, id, attempts[0].clone())).status(),
        StatusCode::OK
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
}
