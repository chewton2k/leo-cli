use super::*;
use crate::calendar::tests::Fake;

const LINK: &str = "https://calendar.google.com/calendar/ical/me/private-abc/basic.ics";

fn feed() -> String {
    let start = (chrono::Utc::now() + chrono::Duration::hours(1)).format("%Y%m%dT%H%M%SZ");
    format!("BEGIN:VCALENDAR\nX-WR-CALNAME:Classes\nBEGIN:VEVENT\nUID:a\nSUMMARY:Algorithms lecture\nDTSTART:{start}\nDURATION:PT1H\nDESCRIPTION:Shortest paths\nEND:VEVENT\nEND:VCALENDAR\n")
}

fn add(state: &AppState, link: &str, at: &str) -> Response {
    run(crate::calendar::add(
        State(state.clone()),
        Extension(peer_at(at)),
        host(at),
        Json(serde_json::from_value(serde_json::json!({ "link": link })).unwrap()),
    ))
}

#[test]
fn a_pasted_calendar_link_shows_upcoming_events_and_is_never_sent_back() {
    let (mut state, dir, _ids) = state_with(&[]);
    let fake = Arc::new(Fake::default());
    fake.pages.lock().unwrap().insert(LINK.into(), feed());
    state.calendar_access = Some(fake.clone());

    assert_eq!(
        add(&state, LINK, "192.168.1.5").status(),
        StatusCode::FORBIDDEN
    );
    let wrong = add(
        &state,
        "https://example.com/not-a-calendar.ics",
        "127.0.0.1",
    );
    assert_eq!(wrong.status(), StatusCode::BAD_REQUEST);
    assert!(json_of(wrong)["error"]
        .as_str()
        .unwrap()
        .contains("could not open"));

    let page = json_of(add(
        &state,
        &LINK.replace("https://", "webcal://"),
        "127.0.0.1",
    ));
    assert_eq!(page["connected"], true);
    assert_eq!(page["calendars"][0]["name"], "Classes");
    assert_eq!(page["events"][0]["title"], "Algorithms lecture");
    assert!(page["events"][0]["context"]
        .as_str()
        .unwrap()
        .contains("Shortest paths"));
    let everything = format!(
        "{page}{}{}",
        std::fs::read_to_string(dir.path().join("calendars.json")).unwrap(),
        std::fs::read_to_string(dir.path().join("calendar-cache.json")).unwrap()
    );
    assert!(
        !everything.contains("private-abc"),
        "the secret link stays in the secret store"
    );
    assert_eq!(fake.secrets.lock().unwrap().len(), 1);

    fake.pages.lock().unwrap().clear();
    let refreshed = json_of(run(crate::calendar::refresh_now(State(state.clone()))));
    assert!(
        refreshed["calendars"][0]["problem"].is_string(),
        "an unreachable calendar says so"
    );

    let id = page["calendars"][0]["id"].as_str().unwrap().to_string();
    let gone = json_of(run(crate::calendar::remove(State(state.clone()), Path(id))));
    assert_eq!(gone["connected"], false);
    assert!(fake.secrets.lock().unwrap().is_empty());
}
