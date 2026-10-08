use super::*;

#[test]
fn keys_count_as_safe_only_over_https_or_on_this_computer() {
    let with = |pairs: &[(&'static str, &'static str)]| {
        let mut headers = axum::http::HeaderMap::new();
        for (k, v) in pairs {
            headers.insert(*k, HeaderValue::from_static(v));
        }
        secure_request(&headers)
    };
    assert!(with(&[
        ("host", "abc.trycloudflare.com"),
        ("x-forwarded-proto", "https")
    ]));
    assert!(with(&[("host", "127.0.0.1:8742")]));
    assert!(with(&[("host", "localhost:8742")]));
    assert!(with(&[("host", "[::1]:8742")]));
    assert!(!with(&[("host", "192.168.1.50:8742")]));
    assert!(!with(&[("host", "localhost.evil.example")]));
    assert!(!with(&[]));
}

#[test]
fn browsers_are_listed_and_signed_out_one_by_one_or_all_but_this_one() {
    let (state, _d, _ids) = state_with(&[]);
    let now = chrono::Utc::now();
    let mine = state
        .gate
        .sessions
        .start("Chrome/130.0 (Macintosh; Mac OS X)", now);
    let phone = state.gate.sessions.start("(iPhone) Safari/604.1", now);
    let tablet = state.gate.sessions.start("(iPad) Safari/604.1", now);
    let me = || Some(axum::Extension(CurrentSession(mine.clone())));

    let listed = run(list_sessions(State(state.clone()), me())).0;
    let list = listed["sessions"].as_array().unwrap();
    assert_eq!(list.len(), 3);
    assert_eq!(list[0]["current"], true);
    assert_eq!(list[0]["device"], "Chrome on Mac");
    assert!(
        !listed.to_string().contains(&mine),
        "secrets never reach the page"
    );

    let phone_handle = sessions::handle_of(&phone);
    let one = run(end_sessions(
        State(state.clone()),
        me(),
        Json(EndSessions {
            handle: Some(phone_handle),
            others: false,
        }),
    ));
    assert_eq!(json_of(one)["ended"], 1);
    assert!(!state.gate.sessions.check(&phone, now));
    assert!(state.gate.sessions.check(&tablet, now));

    let rest = run(end_sessions(
        State(state.clone()),
        me(),
        Json(EndSessions {
            handle: None,
            others: true,
        }),
    ));
    assert_eq!(json_of(rest)["ended"], 1);
    assert!(!state.gate.sessions.check(&tablet, now));
    assert!(state.gate.sessions.check(&mine, now));

    let unclear = run(end_sessions(
        State(state.clone()),
        None,
        Json(EndSessions {
            handle: None,
            others: true,
        }),
    ));
    assert_eq!(unclear.status(), StatusCode::BAD_REQUEST);
}

#[test]
fn a_new_link_is_built_for_the_address_in_use_and_the_old_code_stops_working() {
    let (state, _d, _ids) = state_with(&[]);
    let old = "0123456789abcdef0123456789abcdef";
    assert!(state.gate.token_matches(old));
    let mut headers = host("abc.trycloudflare.com");
    headers.insert("x-forwarded-proto", HeaderValue::from_static("https"));
    let made = json_of(run(new_link(State(state.clone()), headers)));
    let link = made["link"].as_str().unwrap();
    assert!(
        link.starts_with("https://abc.trycloudflare.com/?token="),
        "{link}"
    );
    let code = link.rsplit('=').next().unwrap();
    assert!(state.gate.token_matches(code));
    assert!(!state.gate.token_matches(old));
    let local = json_of(run(new_link(State(state.clone()), host("127.0.0.1:3131"))));
    assert!(local["link"]
        .as_str()
        .unwrap()
        .starts_with("http://127.0.0.1:3131/?token="));
}
