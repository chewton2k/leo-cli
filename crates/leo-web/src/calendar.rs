use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{bail, Result};
use axum::{
    extract::{Path as UrlPath, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::ics::{self, Event};
use crate::routes::auth::{secure_request, Peer};
use crate::AppState;

pub const MOST_BYTES: u64 = 5_000_000;
const MOST_CALENDARS: usize = 5;
const STALE_AFTER_MINUTES: i64 = 15;
const LOOK_BACK_HOURS: i64 = 3;
const LOOK_AHEAD_DAYS: i64 = 7;
const KEPT_DAYS: i64 = 14;
const LIVE_MINUTES: i64 = 2;
const KEPT_EXPANSIONS: usize = 12;
const MOST_DAYS_SHOWN: i64 = 43;
const MOST_REACH_DAYS: i64 = 400;
const OLD_FILES: [&str; 3] = [
    "calendar-google.json",
    "calendar-status.json",
    "calendar-events.json",
];

type Expansion = (String, Arc<Vec<Event>>);

#[derive(Default)]
pub(crate) struct Calendar {
    busy: Arc<AtomicBool>,
    expanded: Mutex<VecDeque<Expansion>>,
}

#[derive(Debug, Default)]
struct Feed {
    fetched_at: Option<DateTime<Utc>>,
    tried_at: Option<DateTime<Utc>>,
    etag: Option<String>,
    modified: Option<String>,
    problem: Option<String>,
}

fn db_for(notes: &Path) -> Arc<crate::db::Db> {
    crate::db::at(&data(notes).join(crate::db::FILE))
}

fn time_of(text: Option<String>) -> Option<DateTime<Utc>> {
    text.and_then(|t| DateTime::parse_from_rfc3339(&t).ok())
        .map(|t| t.with_timezone(&Utc))
}

fn feed_of(notes: &Path, id: &str) -> Feed {
    db_for(notes)
        .with(|c| {
            c.query_row(
                "SELECT fetched_at, tried_at, etag, modified, problem FROM calendar_feeds WHERE feed = ?1",
                [id],
                |r| {
                    Ok(Feed {
                        fetched_at: time_of(r.get(0)?),
                        tried_at: time_of(r.get(1)?),
                        etag: r.get(2)?,
                        modified: r.get(3)?,
                        problem: r.get(4)?,
                    })
                },
            )
        })
        .unwrap_or_default()
}

fn body_of(notes: &Path, id: &str) -> Option<(DateTime<Utc>, Vec<u8>)> {
    db_for(notes)
        .with(|c| {
            c.query_row(
                "SELECT fetched_at, body FROM calendar_feeds WHERE feed = ?1 AND body IS NOT NULL",
                [id],
                |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Vec<u8>>(1)?)),
            )
        })
        .ok()
        .and_then(|(at, body)| time_of(at).map(|at| (at, body)))
}

impl Calendar {
    pub(crate) fn clear_cached(
        &self,
        path: &Path,
        request: &crate::storage::Request,
    ) -> Result<String> {
        let _busy = Busy::take(&self.busy).ok_or_else(|| anyhow::anyhow!("Leo is refreshing your calendars. Wait for that to finish, then clear the cached events."))?;
        let result = crate::storage_saved::act_on(path, request)
            .ok_or_else(|| anyhow::anyhow!("Choose an action offered in Storage."))??;
        self.expanded
            .lock()
            .map_err(|_| {
                anyhow::anyhow!("Calendar cache could not be cleared. Restart Leo and try again.")
            })?
            .clear();
        Ok(result)
    }

    pub(crate) fn events(
        &self,
        notes: &Path,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Vec<(String, String, Event)> {
        let linking: Linking = read(&settings_path(notes));
        let mut out = Vec::new();
        for linked in &linking.calendars {
            let Some((fetched, body)) = body_of(notes, &linked.id) else {
                continue;
            };
            let key = format!(
                "{}|{}|{}|{}",
                linked.id,
                fetched.timestamp(),
                from.timestamp(),
                to.timestamp()
            );
            let kept = self.expanded.lock().ok().and_then(|kept| {
                kept.iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, e)| Arc::clone(e))
            });
            let events = kept.unwrap_or_else(|| {
                let events = Arc::new(ics::read(&String::from_utf8_lossy(&body), from, to).events);
                if let Ok(mut kept) = self.expanded.lock() {
                    kept.push_back((key, Arc::clone(&events)));
                    while kept.len() > KEPT_EXPANSIONS {
                        kept.pop_front();
                    }
                }
                events
            });
            out.extend(
                events
                    .iter()
                    .map(|e| (linked.id.clone(), linked.name.clone(), e.clone())),
            );
        }
        out.sort_by_key(|(_, _, e)| e.start);
        out
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Linked {
    id: String,
    name: String,
    added_at: DateTime<Utc>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Linking {
    calendars: Vec<Linked>,
}

fn data(notes: &Path) -> PathBuf {
    notes.parent().unwrap_or(notes).to_path_buf()
}

pub fn settings_path(notes: &Path) -> PathBuf {
    data(notes).join("calendars.json")
}

pub fn cache_path(notes: &Path) -> PathBuf {
    data(notes).join("calendar-cache.json")
}

fn account(id: &str) -> String {
    format!("calendar-link-{id}")
}

fn read<T: Default + serde::de::DeserializeOwned>(path: &Path) -> T {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return T::default();
    };
    if !meta.is_file() || meta.len() > MOST_BYTES * 4 {
        return T::default();
    }
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn link_of(text: &str) -> Result<String> {
    let text = text.trim();
    let text = match text.strip_prefix("webcal://") {
        Some(rest) => format!("https://{rest}"),
        None => text.to_string(),
    };
    let Some((scheme, rest)) = text.split_once("://") else {
        bail!("That is not a calendar link. Copy the whole address, starting with https://.");
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty() || rest.contains(char::is_whitespace) {
        bail!("That is not a calendar link. Copy the whole address, starting with https://.");
    }
    if scheme != "https" {
        bail!("Use the calendar's https:// address.");
    }
    if text.len() > 2000 {
        bail!("That link is too long to be a calendar address.");
    }
    Ok(text)
}

fn parsed(bytes: &[u8], now: DateTime<Utc>) -> Result<ics::Calendar> {
    let text = String::from_utf8_lossy(bytes);
    if !text.contains("BEGIN:VCALENDAR") {
        bail!("That link does not lead to a calendar. In Google Calendar, copy “Secret address in iCal format”.");
    }
    Ok(ics::read(
        &text,
        now - Duration::days(1),
        now + Duration::days(KEPT_DAYS),
    ))
}

fn refresh(notes: &Path, access: &dyn crate::CalendarAccess, now: DateTime<Utc>) {
    let linking: Linking = read(&settings_path(notes));
    let db = db_for(notes);
    let stamp = now.to_rfc3339();
    for linked in &linking.calendars {
        let before = feed_of(notes, &linked.id);
        let fetched = access
            .get(&account(&linked.id))
            .and_then(|link| link.ok_or_else(|| anyhow::anyhow!("the link is missing")))
            .and_then(|link| {
                access.fetch_since(&link, before.etag.as_deref(), before.modified.as_deref())
            });
        let problem = "leo could not read this calendar just now. If it keeps happening, remove it and paste its link again.";
        let _ = db.with(|c| match &fetched {
            Ok(crate::Fetched { body: None, .. }) => c.execute(
                "INSERT INTO calendar_feeds (feed, fetched_at, tried_at, problem) VALUES (?1, ?2, ?2, NULL) ON CONFLICT(feed) DO UPDATE SET fetched_at = excluded.fetched_at, tried_at = excluded.tried_at, problem = NULL",
                rusqlite::params![linked.id, stamp],
            ),
            Ok(crate::Fetched { body: Some(bytes), etag, modified })
                if bytes.len() as u64 <= MOST_BYTES && String::from_utf8_lossy(bytes).contains("BEGIN:VCALENDAR") =>
            {
                c.execute(
                    "INSERT INTO calendar_feeds (feed, fetched_at, tried_at, etag, modified, problem, body) VALUES (?1, ?2, ?2, ?3, ?4, NULL, ?5) ON CONFLICT(feed) DO UPDATE SET fetched_at = excluded.fetched_at, tried_at = excluded.tried_at, etag = excluded.etag, modified = excluded.modified, problem = NULL, body = excluded.body",
                    rusqlite::params![linked.id, stamp, etag, modified, bytes],
                )
            }
            _ => c.execute(
                "INSERT INTO calendar_feeds (feed, tried_at, problem) VALUES (?1, ?2, ?3) ON CONFLICT(feed) DO UPDATE SET tried_at = excluded.tried_at, problem = excluded.problem",
                rusqlite::params![linked.id, stamp, problem],
            ),
        });
    }
    let known: Vec<String> = linking.calendars.iter().map(|c| c.id.clone()).collect();
    let _ = db.with(|c| {
        let mut statement = c.prepare("SELECT feed FROM calendar_feeds")?;
        let all: Vec<String> = statement.query_map([], |r| r.get(0))?.flatten().collect();
        for gone in all.iter().filter(|f| !known.contains(f)) {
            c.execute("DELETE FROM calendar_feeds WHERE feed = ?1", [gone])?;
        }
        Ok(())
    });
    let _ = std::fs::remove_file(cache_path(notes));
}

fn synced(notes: &Path, linking: &Linking) -> Option<DateTime<Utc>> {
    linking
        .calendars
        .iter()
        .map(|c| feed_of(notes, &c.id).fetched_at)
        .collect::<Option<Vec<_>>>()
        .and_then(|times| times.into_iter().min())
}

fn calendars_json(notes: &Path, linking: &Linking) -> Vec<serde_json::Value> {
    linking
        .calendars
        .iter()
        .map(|c| {
            serde_json::json!({
                "id": c.id,
                "name": c.name,
                "added_at": c.added_at,
                "problem": feed_of(notes, &c.id).problem,
            })
        })
        .collect()
}

fn status(notes: &Path, calendar: &Calendar, busy: bool, now: DateTime<Utc>) -> serde_json::Value {
    let linking: Linking = read(&settings_path(notes));
    let from = now - Duration::hours(LOOK_BACK_HOURS);
    let to = now + Duration::days(LOOK_AHEAD_DAYS);
    let events: Vec<Event> = calendar
        .events(notes, from, to)
        .into_iter()
        .map(|(_, _, e)| e)
        .filter(|e| !e.all_day && e.end >= now - Duration::minutes(30))
        .collect();
    serde_json::json!({
        "connected": !linking.calendars.is_empty(),
        "calendars": calendars_json(notes, &linking),
        "synced": synced(notes, &linking),
        "refreshing": busy,
        "events": events,
    })
}

fn stale(notes: &Path, now: DateTime<Utc>, minutes: i64) -> bool {
    let linking: Linking = read(&settings_path(notes));
    linking.calendars.iter().any(|c| {
        feed_of(notes, &c.id)
            .tried_at
            .is_none_or(|t| now - t > Duration::minutes(minutes))
    })
}

fn forget_old_google(notes: &Path, access: &dyn crate::CalendarAccess) {
    let old = data(notes).join(OLD_FILES[0]);
    if let Some(value) = std::fs::read(&old)
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
    {
        if let Some(id) = value["secret_id"].as_str() {
            let _ = access.delete(id);
        }
    }
    for name in OLD_FILES {
        let _ = std::fs::remove_file(data(notes).join(name));
    }
}

fn error(code: StatusCode, message: impl ToString) -> Response {
    (
        code,
        Json(serde_json::json!({ "error": message.to_string() })),
    )
        .into_response()
}

async fn notes_of(state: &AppState) -> Result<PathBuf, StatusCode> {
    state.with_store(|s| Ok(s.notes_dir.clone())).await
}

struct Busy(Arc<AtomicBool>);

impl Busy {
    fn take(flag: &Arc<AtomicBool>) -> Option<Busy> {
        (!flag.swap(true, Ordering::AcqRel)).then(|| Busy(Arc::clone(flag)))
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

pub(crate) async fn get(State(state): State<AppState>) -> Response {
    let notes = match notes_of(&state).await {
        Ok(n) => n,
        Err(code) => return code.into_response(),
    };
    let now = Utc::now();
    if let Some(access) = state.calendar_access.clone() {
        let old = notes.clone();
        let _ = tokio::task::spawn_blocking(move || forget_old_google(&old, access.as_ref())).await;
    }
    if let (Some(access), true) = (
        state.calendar_access.clone(),
        stale(&notes, now, STALE_AFTER_MINUTES),
    ) {
        if let Some(busy) = Busy::take(&state.calendar.busy) {
            let notes = notes.clone();
            std::thread::spawn(move || {
                let _busy = busy;
                refresh(&notes, access.as_ref(), Utc::now());
            });
        }
    }
    let busy = state.calendar.busy.load(Ordering::Acquire);
    Json(status(&notes, &state.calendar, busy, now)).into_response()
}

#[derive(Deserialize)]
pub(crate) struct Add {
    link: String,
    #[serde(default)]
    name: String,
}

pub(crate) async fn add(
    State(state): State<AppState>,
    Extension(peer): Extension<Peer>,
    headers: HeaderMap,
    Json(body): Json<Add>,
) -> Response {
    if !secure_request(&headers, peer) {
        return error(
            StatusCode::FORBIDDEN,
            "A calendar link is private, so add it on leo's own computer or through the https link.",
        );
    }
    let Some(access) = state.calendar_access.clone() else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Calendars are not available in this copy of leo.",
        );
    };
    let link = match link_of(&body.link) {
        Ok(link) => link,
        Err(e) => return error(StatusCode::BAD_REQUEST, e),
    };
    let notes = match notes_of(&state).await {
        Ok(n) => n,
        Err(code) => return code.into_response(),
    };
    let Some(busy) = Busy::take(&state.calendar.busy) else {
        return error(
            StatusCode::CONFLICT,
            "leo is reading your calendars right now. Try again in a moment.",
        );
    };
    let name: String = body.name.trim().chars().take(80).collect();
    let shared = Arc::clone(&state.calendar);
    let done = tokio::task::spawn_blocking(move || -> Result<serde_json::Value> {
        let _busy = busy;
        let mut linking: Linking = read(&settings_path(&notes));
        if linking.calendars.len() >= MOST_CALENDARS {
            bail!("leo can follow up to five calendars. Remove one first.");
        }
        let now = Utc::now();
        let bytes = access.fetch(&link).map_err(|_| {
            anyhow::anyhow!("leo could not open that link. Check that it is the calendar's secret address and that this computer is online.")
        })?;
        let calendar = parsed(&bytes, now)?;
        let id = uuid::Uuid::new_v4().simple().to_string();
        access.set(&account(&id), &link)?;
        linking.calendars.push(Linked {
            id: id.clone(),
            name: if name.is_empty() {
                calendar.name.unwrap_or_else(|| "Calendar".into())
            } else {
                name
            },
            added_at: now,
        });
        if let Err(e) = leo_core::recording::write_json(&settings_path(&notes), &linking) {
            let _ = access.delete(&account(&id));
            return Err(e);
        }
        refresh(&notes, access.as_ref(), now);
        Ok(status(&notes, &shared, false, now))
    })
    .await;
    match done {
        Ok(Ok(value)) => Json(value).into_response(),
        Ok(Err(e)) => error(StatusCode::BAD_REQUEST, e),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The calendar could not be added.",
        ),
    }
}

pub(crate) async fn remove(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
) -> Response {
    let Some(access) = state.calendar_access.clone() else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Calendars are not available in this copy of leo.",
        );
    };
    let notes = match notes_of(&state).await {
        Ok(n) => n,
        Err(code) => return code.into_response(),
    };
    let Some(busy) = Busy::take(&state.calendar.busy) else {
        return error(
            StatusCode::CONFLICT,
            "leo is reading your calendars right now. Try again in a moment.",
        );
    };
    let calendar = Arc::clone(&state.calendar);
    let done = tokio::task::spawn_blocking(move || -> Result<serde_json::Value> {
        let _busy = busy;
        let mut linking: Linking = read(&settings_path(&notes));
        let before = linking.calendars.len();
        linking.calendars.retain(|c| c.id != id);
        if linking.calendars.len() == before {
            bail!("That calendar is already gone.");
        }
        access.delete(&account(&id))?;
        leo_core::recording::write_json(&settings_path(&notes), &linking)?;
        refresh(&notes, access.as_ref(), Utc::now());
        Ok(status(&notes, &calendar, false, Utc::now()))
    })
    .await;
    match done {
        Ok(Ok(value)) => Json(value).into_response(),
        Ok(Err(e)) => error(StatusCode::NOT_FOUND, e),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The calendar could not be removed.",
        ),
    }
}

pub(crate) async fn refresh_now(State(state): State<AppState>) -> Response {
    let Some(access) = state.calendar_access.clone() else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Calendars are not available in this copy of leo.",
        );
    };
    let notes = match notes_of(&state).await {
        Ok(n) => n,
        Err(code) => return code.into_response(),
    };
    let Some(busy) = Busy::take(&state.calendar.busy) else {
        return Json(status(&notes, &state.calendar, true, Utc::now())).into_response();
    };
    let calendar = Arc::clone(&state.calendar);
    let done = tokio::task::spawn_blocking(move || {
        let _busy = busy;
        refresh(&notes, access.as_ref(), Utc::now());
        status(&notes, &calendar, false, Utc::now())
    })
    .await;
    match done {
        Ok(value) => Json(value).into_response(),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The calendars could not be read.",
        ),
    }
}

#[derive(Deserialize)]
pub(crate) struct Window {
    from: String,
    to: String,
    #[serde(default)]
    live: Option<String>,
}

fn window_of(w: &Window, now: DateTime<Utc>) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    let from = DateTime::parse_from_rfc3339(&w.from)
        .ok()?
        .with_timezone(&Utc);
    let to = DateTime::parse_from_rfc3339(&w.to)
        .ok()?
        .with_timezone(&Utc);
    let reach = Duration::days(MOST_REACH_DAYS);
    (to > from
        && to - from <= Duration::days(MOST_DAYS_SHOWN)
        && from > now - reach
        && to < now + reach)
        .then_some((from, to))
}

pub(crate) fn marker_of(context: &str) -> Option<&str> {
    let at = context.find("leo-task:")? + "leo-task:".len();
    let rest = &context[at..];
    let end = rest
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '-')
        .unwrap_or(rest.len());
    (end >= 8).then(|| &rest[..end])
}

pub(crate) async fn schedule(
    State(state): State<AppState>,
    Query(window): Query<Window>,
) -> Response {
    let now = Utc::now();
    let Some((from, to)) = window_of(&window, now) else {
        return error(
            StatusCode::BAD_REQUEST,
            "Ask for at most six weeks, within a year of today.",
        );
    };
    let notes = match notes_of(&state).await {
        Ok(n) => n,
        Err(code) => return code.into_response(),
    };
    let live = matches!(window.live.as_deref(), Some("1" | "true"));
    let wait = if live {
        LIVE_MINUTES
    } else {
        STALE_AFTER_MINUTES
    };
    if let (Some(access), true) = (state.calendar_access.clone(), stale(&notes, now, wait)) {
        if let Some(busy) = Busy::take(&state.calendar.busy) {
            let notes = notes.clone();
            std::thread::spawn(move || {
                let _busy = busy;
                refresh(&notes, access.as_ref(), Utc::now());
            });
        }
    }
    let tasks = match crate::routes::tasks::all_listed(&state).await {
        Ok(all) => all,
        Err(code) => return code.into_response(),
    };
    let calendar = Arc::clone(&state.calendar);
    let busy = state.calendar.busy.load(Ordering::Acquire);
    let shown = tokio::task::spawn_blocking(move || {
        let linking: Linking = read(&settings_path(&notes));
        let events: Vec<serde_json::Value> = calendar
            .events(&notes, from, to)
            .into_iter()
            .map(|(feed, name, event)| {
                let task = marker_of(&event.context).and_then(|m| {
                    tasks
                        .iter()
                        .find(|t| t.id.starts_with(m))
                        .map(|t| t.id.clone())
                });
                let mut value = serde_json::json!(event);
                value["id"] = serde_json::json!(format!("{feed}:{}", event.id));
                value["feed"] = serde_json::json!(feed);
                value["calendar"] = serde_json::json!(name);
                value["task"] = serde_json::json!(task);
                value
            })
            .collect();
        serde_json::json!({
            "connected": !linking.calendars.is_empty(),
            "calendars": calendars_json(&notes, &linking),
            "synced": synced(&notes, &linking),
            "refreshing": busy,
            "events": events,
            "tasks": tasks,
        })
    })
    .await;
    match shown {
        Ok(value) => Json(value).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[test]
    fn clearing_cached_events_waits_for_refresh_and_keeps_calendar_connections() {
        let temp = tempfile::tempdir().unwrap();
        let notes = temp.path().join("notes");
        std::fs::create_dir(&notes).unwrap();
        let path = crate::chats::dir_for(&notes);
        let settings = settings_path(&notes);
        std::fs::write(&settings, "saved calendar connection").unwrap();
        crate::db::for_chats(&path).with(|c| c.execute("INSERT INTO calendar_feeds (feed, tried_at, body) VALUES ('calendar-one', 'now', X'0102')", [])).unwrap();
        let calendar = Calendar::default();
        let request = crate::storage::Request {
            area: "calendar-cache".into(),
            action: "all".into(),
            items: vec![],
        };
        let refresh = Busy::take(&calendar.busy).unwrap();
        assert!(calendar.clear_cached(&path, &request).is_err());
        assert_eq!(
            crate::db::for_chats(&path)
                .with(
                    |c| c.query_row("SELECT count(*) FROM calendar_feeds", [], |r| r
                        .get::<_, i64>(0))
                )
                .unwrap(),
            1
        );
        drop(refresh);
        calendar.clear_cached(&path, &request).unwrap();
        assert_eq!(
            crate::db::for_chats(&path)
                .with(
                    |c| c.query_row("SELECT count(*) FROM calendar_feeds", [], |r| r
                        .get::<_, i64>(0))
                )
                .unwrap(),
            0
        );
        assert_eq!(
            std::fs::read_to_string(settings).unwrap(),
            "saved calendar connection"
        );
    }

    #[derive(Default)]
    pub(crate) struct Fake {
        pub(crate) secrets: Mutex<HashMap<String, String>>,
        pub(crate) pages: Mutex<HashMap<String, String>>,
    }

    impl crate::CalendarAccess for Fake {
        fn fetch(&self, url: &str) -> Result<Vec<u8>> {
            self.pages
                .lock()
                .unwrap()
                .get(url)
                .map(|t| t.clone().into_bytes())
                .ok_or_else(|| anyhow::anyhow!("offline"))
        }
        fn get(&self, account: &str) -> Result<Option<String>> {
            Ok(self.secrets.lock().unwrap().get(account).cloned())
        }
        fn set(&self, account: &str, value: &str) -> Result<()> {
            self.secrets
                .lock()
                .unwrap()
                .insert(account.into(), value.into());
            Ok(())
        }
        fn delete(&self, account: &str) -> Result<()> {
            self.secrets.lock().unwrap().remove(account);
            Ok(())
        }
    }

    #[test]
    fn links_are_https_or_webcal_and_anything_else_says_what_to_copy() {
        assert_eq!(
            link_of(" webcal://calendar.google.com/calendar/ical/x/private-abc/basic.ics ")
                .unwrap(),
            "https://calendar.google.com/calendar/ical/x/private-abc/basic.ics"
        );
        assert!(link_of("https://calendar.google.com/calendar/ical/x/basic.ics").is_ok());
        for bad in [
            "",
            "calendar.google.com",
            "http://example.com/a.ics",
            "file:///etc/passwd",
            "https:// spaced",
        ] {
            assert!(link_of(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn an_old_google_sign_in_is_forgotten_with_its_stored_secret() {
        let dir = tempfile::tempdir().unwrap();
        let notes = dir.path().join("notes");
        std::fs::write(
            dir.path().join("calendar-google.json"),
            r#"{"client_id":"x","secret_id":"google-calendar-1","calendar_id":"primary"}"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("calendar-events.json"), "{}").unwrap();
        let fake = Fake::default();
        crate::CalendarAccess::set(&fake, "google-calendar-1", "{\"refresh_token\":\"t\"}")
            .unwrap();
        forget_old_google(&notes, &fake);
        assert!(fake.secrets.lock().unwrap().is_empty());
        for name in OLD_FILES {
            assert!(!dir.path().join(name).exists(), "{name}");
        }
    }
}
