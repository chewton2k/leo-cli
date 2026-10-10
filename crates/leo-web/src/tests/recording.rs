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
            Ok(record::Recorded { title: "Lecture".into(), body: format!("heard {samples} samples; points: {}", points.join(", ")), ..Default::default() })
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
            Ok(record::Recorded { title: "Lecture video".into(), body: "notes".into(), ..Default::default() })
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
