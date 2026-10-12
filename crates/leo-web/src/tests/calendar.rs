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
        String::from_utf8_lossy(&std::fs::read(dir.path().join("leo.db")).unwrap_or_default())
    );
    assert!(!dir.path().join("calendar-cache.json").exists());
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

#[test]
fn the_schedule_shows_a_month_of_events_with_all_day_items_and_tasks_linked_by_marker() {
    let (mut state, _dir, _ids) = state_with(&[]);
    let fake = Arc::new(Fake::default());
    let task = json_of(run(crate::routes::tasks::create_task(
        State(state.clone()),
        Json(serde_json::from_value(serde_json::json!({"title": "Submit lab", "date": "2026-12-03", "time": "23:59", "kind": "assignment"})).unwrap()),
    )));
    let marker: String = task["id"].as_str().unwrap().chars().take(12).collect();
    let feed = format!("BEGIN:VCALENDAR\nX-WR-CALNAME:Classes\nBEGIN:VEVENT\nUID:final\nSUMMARY:Biology final\nDTSTART;VALUE=DATE:20261216\nEND:VEVENT\nBEGIN:VEVENT\nUID:lab\nSUMMARY:Due: Submit lab\nDTSTART:20261204T073000Z\nDURATION:PT30M\nDESCRIPTION:From leo · leo-task:{marker}\nEND:VEVENT\nEND:VCALENDAR\n");
    fake.pages.lock().unwrap().insert(LINK.into(), feed);
    state.calendar_access = Some(fake.clone());
    assert_eq!(add(&state, LINK, "127.0.0.1").status(), StatusCode::OK);
    let window = |from: &str, to: &str| {
        run(crate::calendar::schedule(
            State(state.clone()),
            Query(
                serde_json::from_value(serde_json::json!({"from": from, "to": to, "live": "1"}))
                    .unwrap(),
            ),
        ))
    };
    let month = json_of(window(
        "2026-12-01T00:00:00-08:00",
        "2027-01-01T00:00:00-08:00",
    ));
    let events = month["events"].as_array().unwrap();
    let final_exam = events
        .iter()
        .find(|e| e["title"] == "Biology final")
        .unwrap();
    assert_eq!(final_exam["all_day"], true);
    assert_eq!(final_exam["date"], "2026-12-16");
    let lab = events
        .iter()
        .find(|e| e["title"] == "Due: Submit lab")
        .unwrap();
    assert_eq!(
        lab["task"], task["id"],
        "the event Leo opened in Google links back to its task"
    );
    assert!(
        lab["id"].as_str().unwrap().contains(':'),
        "event ids are scoped by calendar"
    );
    assert_eq!(month["tasks"][0]["title"], "Submit lab");
    assert_eq!(
        window("2026-12-01T00:00:00Z", "2027-03-01T00:00:00Z").status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        window("nonsense", "2027-01-01T00:00:00Z").status(),
        StatusCode::BAD_REQUEST
    );
}
