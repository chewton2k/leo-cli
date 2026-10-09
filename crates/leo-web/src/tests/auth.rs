use super::*;

#[test]
fn keys_count_as_safe_only_over_https_or_on_this_computer() {
    let with = |pairs: &[(&'static str, &'static str)]| {
        let mut headers = axum::http::HeaderMap::new();
        for (k, v) in pairs {
            headers.insert(*k, HeaderValue::from_static(v));
        }
        secure_request(&headers, HERE)
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
    let mine =
        state
            .gate
            .sessions
            .start("Chrome/130.0 (Macintosh; Mac OS X)", "127.0.0.1:8742", now);
    let phone = state
        .gate
        .sessions
        .start("(iPhone) Safari/604.1", "abc.trycloudflare.com", now);
    let tablet = state
        .gate
        .sessions
        .start("(iPad) Safari/604.1", "192.168.1.20:8742", now);
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

#[test]
fn only_a_connection_from_this_computer_counts_as_this_computer() {
    use axum::extract::ConnectInfo;
    let from = |addr: &str| peer_of(Some(&ConnectInfo(addr.parse().unwrap())));
    assert_eq!(from("127.0.0.1:50000"), HERE);
    assert_eq!(from("[::1]:50000"), HERE);
    assert_eq!(from("192.168.1.20:50000"), ELSEWHERE);
    assert_eq!(peer_of(None), ELSEWHERE, "an unknown peer is not trusted");

    let mut spoofed = host("127.0.0.1:8742");
    assert!(!secure_request(&spoofed, ELSEWHERE));
    assert!(!local_request(&spoofed, ELSEWHERE));
    spoofed.insert("x-forwarded-proto", HeaderValue::from_static("https"));
    assert!(
        !secure_request(&spoofed, ELSEWHERE),
        "a forwarded https header counts only from the tunnel on this computer"
    );
    assert!(secure_request(&spoofed, HERE));
}
