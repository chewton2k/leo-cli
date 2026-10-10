use super::*;

fn hearing() -> record::Listener {
    Arc::new(
        |listening: record::Listening, heard: &mut dyn FnMut(record::Heard)| {
            use std::sync::atomic::Ordering;
            use std::sync::mpsc::RecvTimeoutError;
            let rx = listening.audio.expect("audio from the browser");
            let mut samples = 0;
            loop {
                heard(record::Heard::Clock {
                    secs: 1,
                    paused: listening.pause.load(Ordering::Relaxed),
                    level: 0.25,
                });
                match rx.recv_timeout(std::time::Duration::from_millis(20)) {
                    Ok(chunk) => samples += chunk.len(),
                    Err(RecvTimeoutError::Timeout) => {
                        if listening.stop.load(Ordering::Relaxed) {
                            break;
                        }
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
            heard(record::Heard::Step {
                label: "Writing the notes".into(),
                steps: Some((1, 2)),
            });
            let points: Vec<String> = listening
                .points
                .lock()
                .unwrap()
                .iter()
                .map(|(_, text)| text.clone())
                .collect();
            Ok(record::Recorded {
                title: "Lecture".into(),
                body: format!("heard {samples} samples; points: {}", points.join(", ")),
                ..Default::default()
            })
        },
    )
}

fn start_recording(state: &AppState, source: record::Source, at: &str) -> Response {
    run(record::start(
        State(state.clone()),
        Extension(peer_at(at)),
        host(at),
        Json(
            serde_json::from_value(serde_json::json!({
                "directory": "cs130",
                "source": source,
            }))
            .unwrap(),
        ),
    ))
}

fn recording_until(state: &AppState, done: impl Fn(&str) -> bool) -> record::RecordView {
    let started = std::time::Instant::now();
    loop {
        let view = state
            .recording
            .lock()
            .unwrap()
            .as_ref()
            .map(|j| j.view())
            .unwrap();
        if done(view.state) {
            return view;
        }
        assert!(started.elapsed().as_secs() < 10, "stuck at {view:?}");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn a_recording_from_the_browser_hears_every_chunk_and_becomes_a_note() {
    let (mut state, _d, _ids) = state_with(&[]);
    state.listener = Some(hearing());
    let started = start_recording(
        &state,
        record::Source::Browser,
        "my-laptop.trycloudflare.com",
    );
    assert_eq!(started.status(), StatusCode::ACCEPTED);
    let id = json_of(started)["id"].as_str().unwrap().to_string();
    recording_until(&state, |s| s == "recording");

    let chunk: Vec<u8> = (0..1600i16).flat_map(|s| s.to_le_bytes()).collect();
    for _ in 0..3 {
        let sent = run(record::audio(
            State(state.clone()),
            Path(id.clone()),
            Query(Default::default()),
            axum::body::Bytes::from(chunk.clone()),
        ));
        assert_eq!(sent.status(), StatusCode::NO_CONTENT);
    }
    let jotted = run(record::point(
        State(state.clone()),
        Path(id.clone()),
        Json(serde_json::from_value(serde_json::json!({ "text": "exam is on BFS" })).unwrap()),
    ));
    assert_eq!(json_of(jotted)["points"][0][1], "exam is on BFS");
    let paused = run(record::pause(
        State(state.clone()),
        Path(id.clone()),
        Json(serde_json::from_value(serde_json::json!({ "paused": true })).unwrap()),
    ));
    assert_eq!(json_of(paused)["state"], "paused");
    recording_until(&state, |s| s == "paused");

    let busy = start_recording(&state, record::Source::Browser, "localhost:4000");
    assert_eq!(busy.status(), StatusCode::CONFLICT);

    let stopped = run(record::stop(State(state.clone()), Path(id.clone())));
    assert_eq!(json_of(stopped)["state"], "writing");
    let late = run(record::audio(
        State(state.clone()),
        Path(id.clone()),
        Query(Default::default()),
        axum::body::Bytes::from(chunk.clone()),
    ));
    assert_eq!(late.status(), StatusCode::CONFLICT);

    let view = recording_until(&state, |s| s == "done" || s == "failed");
    assert_eq!(view.state, "done", "{view:?}");
    let note = state
        .fresh()
        .find_note(view.note.as_deref().unwrap())
        .unwrap()
        .clone();
    assert_eq!(note.title, "Lecture");
    assert_eq!(note.directory, "cs130");
    assert_eq!(note.body, "heard 4800 samples; points: exam is on BFS");
    assert!(!view.levels.is_empty());
    assert!(
        view.levels.len() <= record::LEVELS_KEPT,
        "only the last few seconds are kept"
    );
    assert!(view.levels.iter().all(|l| *l == 0.25));

    let gone = run(record::status(State(state.clone()), Path("nope".into())));
    assert_eq!(gone.status(), StatusCode::NOT_FOUND);
    let again = start_recording(&state, record::Source::Browser, "localhost");
    assert_eq!(again.status(), StatusCode::ACCEPTED);
    let id = json_of(again)["id"].as_str().unwrap().to_string();
    run(record::stop(State(state.clone()), Path(id)));
    recording_until(&state, |s| s == "done");
}

#[test]
fn the_computers_own_microphone_only_answers_a_page_on_that_computer() {
    let (mut state, _d, _ids) = state_with(&[]);
    state.listener = Some(Arc::new(
        |_: record::Listening, _: &mut dyn FnMut(record::Heard)| {
            Err(anyhow::anyhow!("No sound was recorded."))
        },
    ));
    for (source, at) in [
        (record::Source::Microphone, "my-laptop.trycloudflare.com"),
        (record::Source::Screen, "192.168.1.20:4000"),
    ] {
        let refused = start_recording(&state, source, at);
        assert_eq!(refused.status(), StatusCode::FORBIDDEN, "{at}");
        assert!(state.recording.lock().unwrap().is_none());
    }
    let mut tunnelled = host("localhost");
    tunnelled.insert("x-forwarded-proto", HeaderValue::from_static("https"));
    assert!(!local_request(&tunnelled, HERE));
    assert!(local_request(&host("127.0.0.1:4000"), HERE));
    assert!(local_request(&host("[::1]:4000"), HERE));
    assert!(
        !local_request(&host("127.0.0.1:4000"), ELSEWHERE),
        "a device on the network that claims to be this computer is not"
    );

    let allowed = start_recording(&state, record::Source::Screen, "127.0.0.1:4000");
    assert_eq!(allowed.status(), StatusCode::ACCEPTED);
    let view = recording_until(&state, |s| s == "failed" || s == "done");
    assert_eq!(view.state, "failed");
    assert_eq!(view.error.as_deref(), Some("No sound was recorded."));
    assert!(state.fresh().notes.is_empty());
}

#[test]
fn a_tabs_sound_is_recorded_from_any_browser_as_screen_audio() {
    let (mut state, _d, _ids) = state_with(&[]);
    let seen: Arc<Mutex<Option<(bool, bool)>>> = Default::default();
    let saw = Arc::clone(&seen);
    state.listener = Some(Arc::new(
        move |listening: record::Listening, _: &mut dyn FnMut(record::Heard)| {
            *saw.lock().unwrap() = Some((listening.screen, listening.audio.is_some()));
            Ok(record::Recorded {
                title: "Lecture video".into(),
                body: "notes".into(),
                ..Default::default()
            })
        },
    ));
    let started = start_recording(&state, record::Source::Tab, "my-laptop.trycloudflare.com");
    assert_eq!(started.status(), StatusCode::ACCEPTED);
    let view = recording_until(&state, |s| s == "done" || s == "failed");
    assert_eq!(view.state, "done", "{view:?}");
    assert_eq!(
        *seen.lock().unwrap(),
        Some((true, true)),
        "screen audio, sent by the browser"
    );
    assert!(record::Source::Tab.fed_by_browser());
    assert!(!record::Source::Tab.on_this_computer());
    assert!(!record::Source::Browser.is_sound());
}

#[test]
fn recording_is_refused_without_a_recorder_or_into_a_folder_outside_the_notes() {
    let (mut state, _d, _ids) = state_with(&[]);
    let none = start_recording(&state, record::Source::Browser, "localhost");
    assert_eq!(none.status(), StatusCode::SERVICE_UNAVAILABLE);
    state.listener = Some(hearing());
    let outside = run(record::start(
        State(state.clone()),
        Extension(HERE),
        host("localhost"),
        Json(
            serde_json::from_value(serde_json::json!({
                "directory": "../outside",
                "source": "browser",
            }))
            .unwrap(),
        ),
    ));
    assert_eq!(outside.status(), StatusCode::BAD_REQUEST);
    assert!(state.recording.lock().unwrap().is_none());
}

#[test]
fn audio_arrives_as_little_endian_samples_and_an_odd_byte_is_ignored() {
    assert_eq!(record::decode(&[1, 0, 0xfe, 0xff, 7]), vec![1, -2]);
}

#[test]
fn retried_sequences_are_durable_and_never_counted_twice() {
    let (mut state, _d, _ids) = state_with(&[]);
    state.listener = Some(hearing());
    let id = json_of(start_recording(
        &state,
        record::Source::Browser,
        "127.0.0.1",
    ))["id"]
        .as_str()
        .unwrap()
        .to_string();
    recording_until(&state, |s| s == "recording");
    let send = |seq: u64, bytes: Vec<u8>| {
        run(record::audio(
            State(state.clone()),
            Path(id.clone()),
            Query(serde_json::from_value(serde_json::json!({"seq":seq})).unwrap()),
            bytes.into(),
        ))
    };
    assert_eq!(send(0, vec![1, 0, 2, 0]).status(), StatusCode::OK);
    assert_eq!(send(0, vec![1, 0, 2, 0]).status(), StatusCode::OK);
    assert_eq!(send(0, vec![9, 0]).status(), StatusCode::CONFLICT);
    assert_eq!(send(2, vec![3, 0]).status(), StatusCode::CONFLICT);
    assert_eq!(send(1, vec![3]).status(), StatusCode::BAD_REQUEST);
    store_now(&state, |store| {
        assert_eq!(
            crate::record_journal::chunks(&store.notes_dir, &id)
                .unwrap()
                .len(),
            1
        );
        Ok(())
    })
    .unwrap();
    run(record::stop(State(state.clone()), Path(id)));
    let view = recording_until(&state, |s| s == "done");
    store_now(&state, |store| {
        assert!(store
            .find_note(view.note.as_ref().unwrap())
            .unwrap()
            .body
            .contains("heard 2 samples"));
        Ok(())
    })
    .unwrap();
}
#[test]
fn a_point_or_stop_during_an_audio_save_waits_for_it_instead_of_failing() {
    let (mut state, _d, _ids) = state_with(&[]);
    state.listener = Some(hearing());
    let id = json_of(start_recording(
        &state,
        record::Source::Browser,
        "127.0.0.1",
    ))["id"]
        .as_str()
        .unwrap()
        .to_string();
    recording_until(&state, |s| s == "recording");
    let saving = state.recording.lock().unwrap().as_ref().unwrap().writing();
    let busy_for_a_moment = || {
        saving.store(true, std::sync::atomic::Ordering::Release);
        let saving = saving.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(150));
            saving.store(false, std::sync::atomic::Ordering::Release);
        })
    };
    let done = busy_for_a_moment();
    let point = run(record::point(
        State(state.clone()),
        Path(id.clone()),
        Json(serde_json::from_value(serde_json::json!({"text": "heaps"})).unwrap()),
    ));
    assert_eq!(point.status(), StatusCode::OK);
    done.join().unwrap();
    let done = busy_for_a_moment();
    let stopped = run(record::stop(State(state.clone()), Path(id)));
    assert_eq!(stopped.status(), StatusCode::OK);
    done.join().unwrap();
    let view = recording_until(&state, |s| s == "done");
    assert_eq!(view.points.len(), 1);
}

#[test]
fn a_panicking_listener_keeps_its_audio_journal_for_recovery() {
    let (mut state, _d, _ids) = state_with(&[]);
    state.listener = Some(Arc::new(|_, _| panic!("simulated failure")));
    let id = json_of(start_recording(
        &state,
        record::Source::Browser,
        "127.0.0.1",
    ))["id"]
        .as_str()
        .unwrap()
        .to_string();
    recording_until(&state, |s| s == "failed");
    assert_eq!(
        run(record::audio(
            State(state.clone()),
            Path(id.clone()),
            Query(serde_json::from_value(serde_json::json!({"seq":0})).unwrap()),
            vec![1, 0].into()
        ))
        .status(),
        StatusCode::OK
    );
    store_now(&state, |store| {
        let pending = crate::record_journal::pending(&store.notes_dir).unwrap();
        assert_eq!(pending[0].id, id);
        Ok(())
    })
    .unwrap();
}
#[test]
fn source_commit_runs_after_the_note_is_visible_on_disk_and_recovery_uses_the_same_id() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let (mut state, _d, _ids) = state_with(&[]);
    let committed = Arc::new(AtomicBool::new(false));
    let seen = committed.clone();
    state.listener = Some(Arc::new(move |input, _| {
        let archive = leo_core::recording::Archive {
            id: input.id,
            started: chrono::Utc::now(),
            passages: vec![],
            points: vec![],
            template: String::new(),
            context: String::new(),
            warnings: vec![],
            trace: vec![],
        };
        let seen = seen.clone();
        Ok(record::Recorded {
            title: "Saved once".into(),
            body: "body".into(),
            source: Some(archive),
            commit: Some(Arc::new(move |notes, id| {
                let store = leo_core::store::Store::load_from(notes)?;
                assert!(store.find_note(id).is_some());
                seen.store(true, Ordering::Relaxed);
                Ok(())
            })),
        })
    }));
    let id = json_of(start_recording(
        &state,
        record::Source::Browser,
        "127.0.0.1",
    ))["id"]
        .as_str()
        .unwrap()
        .to_string();
    let view = recording_until(&state, |s| s == "done");
    assert_eq!(view.note.as_deref(), Some(id.as_str()));
    assert!(committed.load(Ordering::Relaxed));
}

#[test]
fn call_pcm_is_split_into_two_independent_tracks() {
    let (mut state, _d, _ids) = state_with(&[]);
    state.listener = Some(Arc::new(|input, _| {
        assert!(input.call);
        let you = input.audio.unwrap().recv().unwrap();
        let others = input.other_audio.unwrap().recv().unwrap();
        assert_eq!(you, [1, 2]);
        assert_eq!(others, [100, 200]);
        Ok(record::Recorded {
            title: "Call".into(),
            body: "Two tracks".into(),
            ..Default::default()
        })
    }));
    let id = json_of(start_recording(&state, record::Source::Call, "127.0.0.1"))["id"]
        .as_str()
        .unwrap()
        .to_string();
    let bytes: Vec<u8> = [1i16, 100, 2, 200]
        .into_iter()
        .flat_map(i16::to_le_bytes)
        .collect();
    assert_eq!(
        run(record::audio(
            State(state.clone()),
            Path(id),
            Query(Default::default()),
            bytes.into()
        ))
        .status(),
        StatusCode::NO_CONTENT
    );
    recording_until(&state, |s| s == "done");
}

#[test]
fn recovery_stays_in_saving_state_and_delivers_every_persisted_chunk() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let (mut state, _d, _ids) = state_with(&[]);
    let ready = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let listener = hearing();
    let started = ready.clone();
    let resume = release.clone();
    state.listener = Some(Arc::new(move |input, heard| {
        heard(record::Heard::Clock {
            secs: 1,
            paused: false,
            level: 0.0,
        });
        started.store(true, Ordering::Release);
        for _ in 0..1000 {
            if resume.load(Ordering::Acquire) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        listener(input, heard)
    }));
    let id = "recovery-0001".to_string();
    store_now(&state, |store| {
        let meta = crate::record_journal::Journal {
            id: id.clone(),
            directory: "cs130".into(),
            title: None,
            source: record::Source::Browser,
            profile: Default::default(),
            points: vec![],
        };
        crate::record_journal::save(&store.notes_dir, &meta).unwrap();
        for seq in 0..20 {
            crate::record_journal::append(&store.notes_dir, &id, seq, &[1, 0]).unwrap();
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(
        run(record::recover(State(state.clone()), Path(id.clone()))).status(),
        StatusCode::ACCEPTED
    );
    for _ in 0..1000 {
        if ready.load(Ordering::Acquire) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(ready.load(Ordering::Acquire));
    let view = json_of(run(record::status(State(state.clone()), Path(id.clone()))));
    assert_eq!(view["state"], "writing");
    assert_eq!(
        run(record::audio(
            State(state.clone()),
            Path(id),
            Query(serde_json::from_value(serde_json::json!({"seq":20})).unwrap()),
            vec![2, 0].into()
        ))
        .status(),
        StatusCode::CONFLICT
    );
    release.store(true, Ordering::Release);
    let view = recording_until(&state, |s| s == "done");
    store_now(&state, |store| {
        assert!(store
            .find_note(view.note.as_ref().unwrap())
            .unwrap()
            .body
            .contains("heard 20 samples"));
        Ok(())
    })
    .unwrap();
}
