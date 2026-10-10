use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{bail, Result};
use axum::{
    extract::{Path as UrlPath, State},
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
const OLD_FILES: [&str; 3] = [
    "calendar-google.json",
    "calendar-status.json",
    "calendar-events.json",
];

#[derive(Default)]
pub(crate) struct Calendar {
    busy: Arc<AtomicBool>,
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

#[derive(Debug, Default, Serialize, Deserialize)]
struct Cache {
    synced: Option<DateTime<Utc>>,
    events: Vec<Event>,
    #[serde(default)]
    problems: std::collections::BTreeMap<String, String>,
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
    let mut cache = Cache {
        synced: Some(now),
        ..Default::default()
    };
    for linked in &linking.calendars {
        let fetched = access
            .get(&account(&linked.id))
            .and_then(|link| link.ok_or_else(|| anyhow::anyhow!("the link is missing")))
            .and_then(|link| access.fetch(&link))
            .and_then(|bytes| parsed(&bytes, now));
        match fetched {
            Ok(calendar) => cache.events.extend(calendar.events),
            Err(_) => {
                cache.problems.insert(
                    linked.id.clone(),
                    "leo could not read this calendar just now. If it keeps happening, remove it and paste its link again.".into(),
                );
            }
        }
    }
    cache.events.sort_by_key(|e| e.start);
    let _ = leo_core::recording::write_json(&cache_path(notes), &cache);
}

fn status(notes: &Path, busy: bool, now: DateTime<Utc>) -> serde_json::Value {
    let linking: Linking = read(&settings_path(notes));
    let cache: Cache = read(&cache_path(notes));
    let from = now - Duration::hours(LOOK_BACK_HOURS);
    let to = now + Duration::days(LOOK_AHEAD_DAYS);
    let events: Vec<&Event> = cache
        .events
        .iter()
        .filter(|e| e.end >= from && e.start <= to && e.end >= now - Duration::minutes(30))
        .collect();
    serde_json::json!({
        "connected": !linking.calendars.is_empty(),
        "calendars": linking.calendars.iter().map(|c| serde_json::json!({
            "id": c.id,
            "name": c.name,
            "added_at": c.added_at,
            "problem": cache.problems.get(&c.id),
        })).collect::<Vec<_>>(),
        "synced": cache.synced,
        "refreshing": busy,
        "events": events,
    })
}

fn stale(notes: &Path, now: DateTime<Utc>) -> bool {
    let linking: Linking = read(&settings_path(notes));
    let cache: Cache = read(&cache_path(notes));
    !linking.calendars.is_empty()
        && cache
            .synced
            .is_none_or(|t| now - t > Duration::minutes(STALE_AFTER_MINUTES))
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
    if let (Some(access), true) = (state.calendar_access.clone(), stale(&notes, now)) {
        if let Some(busy) = Busy::take(&state.calendar.busy) {
            let notes = notes.clone();
            std::thread::spawn(move || {
                let _busy = busy;
                refresh(&notes, access.as_ref(), Utc::now());
            });
        }
    }
    let busy = state.calendar.busy.load(Ordering::Acquire);
    Json(status(&notes, busy, now)).into_response()
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
        Ok(status(&notes, false, now))
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
        if linking.calendars.is_empty() {
            let _ = std::fs::remove_file(cache_path(&notes));
        } else {
            refresh(&notes, access.as_ref(), Utc::now());
        }
        Ok(status(&notes, false, Utc::now()))
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
        return Json(status(&notes, true, Utc::now())).into_response();
    };
    let done = tokio::task::spawn_blocking(move || {
        let _busy = busy;
        refresh(&notes, access.as_ref(), Utc::now());
        status(&notes, false, Utc::now())
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

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

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
