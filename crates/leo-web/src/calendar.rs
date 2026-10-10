use crate::{
    routes::auth::{local_request, Peer},
    AppState,
};
use anyhow::{bail, Context, Result};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use base64::Engine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

const MOST_JSON_BYTES: u64 = 24_000_000;
const MOST_EVENTS: usize = 2500;
const MOST_PAGES: usize = 10;
const SCOPE: &str = "https://www.googleapis.com/auth/calendar.events.readonly";
#[derive(Default)]
pub(crate) struct Calendar {
    busy: Arc<AtomicBool>,
}
#[derive(Serialize, Deserialize)]
struct Account {
    client_id: String,
    secret_id: String,
    calendar_id: String,
}
#[derive(Serialize, Deserialize)]
struct Credentials {
    client_secret: String,
    refresh_token: String,
}

fn account(notes: &Path, secrets: &dyn crate::CalendarSecrets) -> Result<(Account, Credentials)> {
    check_file(&path(notes))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path(notes), std::fs::Permissions::from_mode(0o600))?;
    }
    let value: serde_json::Value = read(&path(notes))?;
    let account = if value.get("refresh_token").is_some() {
        let secret_id = format!("google-calendar-{}", uuid::Uuid::new_v4());
        let credentials = Credentials {
            client_secret: value["client_secret"].as_str().unwrap_or("").into(),
            refresh_token: value["refresh_token"]
                .as_str()
                .context("Refresh token missing")?
                .into(),
        };
        secrets.set(&secret_id, &serde_json::to_string(&credentials)?)?;
        let account = Account {
            client_id: value["client_id"]
                .as_str()
                .context("Client id missing")?
                .into(),
            calendar_id: value["calendar_id"].as_str().unwrap_or("primary").into(),
            secret_id,
        };
        leo_core::recording::write_json(&path(notes), &account)?;
        account
    } else {
        serde_json::from_value(value)?
    };
    let value = secrets
        .get(&account.secret_id)?
        .context("Google Calendar credentials are missing; connect again")?;
    Ok((account, serde_json::from_str(&value)?))
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Event {
    pub id: String,
    pub title: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub context: String,
    pub link: String,
}
#[derive(Default, Serialize, Deserialize)]
struct Cache {
    synced: Option<DateTime<Utc>>,
    events: Vec<Event>,
}
fn path(notes: &Path) -> PathBuf {
    notes.parent().unwrap_or(notes).join("calendar-google.json")
}
fn cache_path(notes: &Path) -> PathBuf {
    notes.parent().unwrap_or(notes).join("calendar-events.json")
}
fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    check_file(path)?;
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}
fn check_file(path: &Path) -> Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    anyhow::ensure!(
        meta.is_file() && !meta.file_type().is_symlink() && meta.len() <= MOST_JSON_BYTES,
        "Calendar settings must be a regular file within the size limit"
    );
    Ok(())
}
fn json(response: reqwest::blocking::Response) -> Result<serde_json::Value> {
    let mut bytes = Vec::new();
    response.take(MOST_JSON_BYTES + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() as u64 <= MOST_JSON_BYTES,
        "Google returned too much calendar data"
    );
    Ok(serde_json::from_slice(&bytes)?)
}

fn error(code: StatusCode, message: impl ToString) -> Response {
    (code, Json(serde_json::json!({"error":message.to_string()}))).into_response()
}
async fn notes(state: &AppState) -> Result<PathBuf, StatusCode> {
    state.with_store(|s| Ok(s.notes_dir.clone())).await
}
fn client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}
fn secret() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

pub(crate) async fn status(State(state): State<AppState>) -> Response {
    let notes = match notes(&state).await {
        Ok(n) => n,
        Err(c) => return c.into_response(),
    };
    let connected = path(&notes).exists();
    match if cache_path(&notes).exists() { read::<Cache>(&cache_path(&notes)) } else { Ok(Cache::default()) } {
        Ok(cache) => Json(serde_json::json!({"connected":connected,"busy":state.calendar.busy.load(Ordering::Relaxed),"synced":cache.synced,"events":cache.events.into_iter().filter(|e| e.end >= Utc::now()).collect::<Vec<_>>()})).into_response(),
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "The calendar cache could not be read."),
    }
}

#[derive(Deserialize)]
pub(crate) struct Connect {
    client_id: String,
    #[serde(default)]
    client_secret: String,
    #[serde(default)]
    calendar_id: String,
}
pub(crate) async fn connect(
    State(state): State<AppState>,
    Extension(peer): Extension<Peer>,
    headers: HeaderMap,
    Json(body): Json<Connect>,
) -> Response {
    if !local_request(&headers, peer) {
        return error(
            StatusCode::FORBIDDEN,
            "Connect Google Calendar from the page on Leo's computer.",
        );
    }
    let Some(secrets) = state.calendar_secrets.clone() else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Calendar credential storage is unavailable.",
        );
    };
    if !body.client_id.ends_with(".apps.googleusercontent.com")
        || body.client_id.len() > 300
        || body.client_secret.len() > 500
        || body.calendar_id.len() > 500
    {
        return error(
            StatusCode::BAD_REQUEST,
            "Use a Desktop app OAuth client from Google Cloud.",
        );
    }
    let notes = match notes(&state).await {
        Ok(n) => n,
        Err(c) => return c.into_response(),
    };
    if state.calendar.busy.swap(true, Ordering::Relaxed) {
        return error(
            StatusCode::CONFLICT,
            "A calendar connection or sync is already in progress.",
        );
    }
    let listener = match std::net::TcpListener::bind(("127.0.0.1", 0)) {
        Ok(l) => l,
        Err(_) => {
            state.calendar.busy.store(false, Ordering::Relaxed);
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not open the Google sign-in callback.",
            );
        }
    };
    let redirect = format!(
        "http://127.0.0.1:{}/",
        listener.local_addr().expect("bound listener").port()
    );
    let nonce = secret();
    let verifier = secret();
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(Sha256::digest(verifier.as_bytes()));
    let mut url =
        url::Url::parse("https://accounts.google.com/o/oauth2/v2/auth").expect("constant URL");
    url.query_pairs_mut().extend_pairs([
        ("client_id", body.client_id.as_str()),
        ("redirect_uri", &redirect),
        ("response_type", "code"),
        ("scope", SCOPE),
        ("access_type", "offline"),
        ("prompt", "consent"),
        ("state", &nonce),
        ("code_challenge", &challenge),
        ("code_challenge_method", "S256"),
    ]);
    let busy = state.calendar.busy.clone();
    std::thread::spawn(move || {
        let result = receive(listener, &nonce).and_then(|code| {
            let response = client()?
                .post("https://oauth2.googleapis.com/token")
                .form(&[
                    ("client_id", body.client_id.as_str()),
                    ("client_secret", &body.client_secret),
                    ("code", &code),
                    ("code_verifier", &verifier),
                    ("redirect_uri", &redirect),
                    ("grant_type", "authorization_code"),
                ])
                .send()?;
            if !response.status().is_success() {
                bail!("Google sign-in did not complete. Check the OAuth client configuration.");
            }
            let token = json(response)?;
            let refresh = token["refresh_token"]
                .as_str()
                .context("Google did not grant offline access")?;
            let account = Account {
                client_id: body.client_id,
                secret_id: format!("google-calendar-{}", uuid::Uuid::new_v4()),
                calendar_id: if body.calendar_id.trim().is_empty() {
                    "primary".into()
                } else {
                    body.calendar_id
                },
            };
            let credentials = Credentials {
                client_secret: body.client_secret,
                refresh_token: refresh.into(),
            };
            secrets.set(&account.secret_id, &serde_json::to_string(&credentials)?)?;
            let old: Option<Account> = read(&path(&notes)).ok();
            if let Err(e) = leo_core::recording::write_json(&path(&notes), &account) {
                let _ = secrets.delete(&account.secret_id);
                return Err(e);
            }
            if let Some(old) = old {
                let _ = secrets.delete(&old.secret_id);
            }
            sync_calendar(&notes, secrets.as_ref())
        });
        let status = serde_json::json!({"error": result.err().map(|_| "Google Calendar connection did not finish. Try connecting again.")});
        let _ = leo_core::recording::write_json(
            &notes
                .parent()
                .unwrap_or(&notes)
                .join("calendar-status.json"),
            &status,
        );
        busy.store(false, Ordering::Relaxed);
    });
    Json(serde_json::json!({"url":url.as_str()})).into_response()
}

fn receive(listener: std::net::TcpListener, nonce: &str) -> Result<String> {
    listener.set_nonblocking(true)?;
    let deadline = Instant::now() + Duration::from_secs(300);
    while Instant::now() < deadline {
        match listener.accept() {
            Ok((mut stream, _)) => {
                if stream.set_nonblocking(false).is_err()
                    || stream
                        .set_read_timeout(Some(Duration::from_secs(3)))
                        .is_err()
                {
                    continue;
                }
                let mut bytes = [0u8; 8192];
                let n = match stream.read(&mut bytes) {
                    Ok(n) if n > 0 => n,
                    _ => continue,
                };
                let request = String::from_utf8_lossy(&bytes[..n]);
                let target = request
                    .lines()
                    .next()
                    .and_then(|l| l.split_whitespace().nth(1))
                    .unwrap_or("");
                let Ok(url) = url::Url::parse(&format!("http://127.0.0.1{target}")) else {
                    continue;
                };
                let params: std::collections::HashMap<_, _> =
                    url.query_pairs().into_owned().collect();
                if params.get("state").is_none_or(|s| s != nonce) {
                    let _ = stream.write_all(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\nContent-Length: 0\r\n\r\n");
                    continue;
                }
                let text = "Sign-in received. Return to Leo to check the connection.";
                let _ = write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{text}",text.len());
                return params
                    .get("code")
                    .cloned()
                    .context("Google sign-in was declined");
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100))
            }
            Err(e) => return Err(e.into()),
        }
    }
    bail!("Google sign-in timed out")
}

fn event(value: &serde_json::Value) -> Result<Option<Event>> {
    if value["status"] == "cancelled" || value["start"]["dateTime"].is_null() {
        return Ok(None);
    }
    if value["attendees"].as_array().is_some_and(|a| {
        a.iter()
            .any(|p| p["self"] == true && p["responseStatus"] == "declined")
    }) {
        return Ok(None);
    }
    let date = |field: &str| -> Result<DateTime<Utc>> {
        Ok(DateTime::parse_from_rfc3339(
            value[field]["dateTime"]
                .as_str()
                .context("Event time missing")?,
        )?
        .with_timezone(&Utc))
    };
    let attendees = value["attendees"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|p| p["displayName"].as_str().or(p["email"].as_str()))
                .take(100)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    Ok(Some(Event {
        id: value["id"].as_str().context("Event id missing")?.into(),
        title: value["summary"].as_str().unwrap_or("Meeting").into(),
        start: date("start")?,
        end: date("end")?,
        context: format!(
            "{}\n{}\nAttendees: {}",
            value["description"].as_str().unwrap_or(""),
            value["location"].as_str().unwrap_or(""),
            attendees
        )
        .chars()
        .take(7500)
        .collect(),
        link: value["htmlLink"].as_str().unwrap_or("").into(),
    }))
}
fn sync_calendar(notes: &Path, secrets: &dyn crate::CalendarSecrets) -> Result<()> {
    let (account, credentials) = account(notes, secrets)?;
    let client = client()?;
    let response = client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("client_id", account.client_id.as_str()),
            ("client_secret", &credentials.client_secret),
            ("refresh_token", &credentials.refresh_token),
            ("grant_type", "refresh_token"),
        ])
        .send()?;
    if !response.status().is_success() {
        bail!("Google Calendar needs to be connected again.");
    }
    let token = json(response)?;
    let token = token["access_token"]
        .as_str()
        .context("Google did not return an access token")?;
    let now = Utc::now();
    let mut events = Vec::new();
    let mut page = String::new();
    let mut url = url::Url::parse("https://www.googleapis.com/calendar/v3/calendars/")?;
    url.path_segments_mut()
        .map_err(|_| anyhow::anyhow!("Invalid calendar URL"))?
        .pop_if_empty()
        .push(&account.calendar_id)
        .push("events");
    for _ in 0..MOST_PAGES {
        let response = client
            .get(url.clone())
            .bearer_auth(token)
            .query(&[
                ("timeMin", now.to_rfc3339()),
                ("timeMax", (now + chrono::Duration::days(30)).to_rfc3339()),
                ("singleEvents", "true".into()),
                ("orderBy", "startTime".into()),
                ("maxResults", "250".into()),
                ("pageToken", page.clone()),
            ])
            .send()?;
        if !response.status().is_success() {
            bail!("Google Calendar could not be synced. Check Calendar API access and the calendar id.");
        }
        let value = json(response)?;
        let items = value["items"]
            .as_array()
            .context("Calendar events missing")?;
        anyhow::ensure!(
            items.len() <= 250 && events.len() + items.len() <= MOST_EVENTS,
            "Google returned too many calendar events"
        );
        for item in items {
            if let Some(e) = event(item)? {
                events.push(e);
            }
        }
        page = value["nextPageToken"].as_str().unwrap_or("").into();
        if page.is_empty() {
            break;
        }
    }
    if !page.is_empty() {
        bail!("This calendar has too many events in the next month.");
    }
    leo_core::recording::write_json(
        &cache_path(notes),
        &Cache {
            synced: Some(now),
            events,
        },
    )
}
pub(crate) async fn sync(State(state): State<AppState>) -> Response {
    let notes = match notes(&state).await {
        Ok(n) => n,
        Err(c) => return c.into_response(),
    };
    if state.calendar.busy.swap(true, Ordering::Relaxed) {
        return error(
            StatusCode::CONFLICT,
            "A calendar connection or sync is in progress.",
        );
    }
    let Some(secrets) = state.calendar_secrets.clone() else {
        state.calendar.busy.store(false, Ordering::Relaxed);
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Calendar credential storage is unavailable.",
        );
    };
    let result = tokio::task::spawn_blocking(move || sync_calendar(&notes, secrets.as_ref())).await;
    state.calendar.busy.store(false, Ordering::Relaxed);
    match result {
        Ok(Ok(())) => status(State(state)).await,
        _ => error(
            StatusCode::BAD_GATEWAY,
            "Google Calendar could not sync. Reconnect if access expired.",
        ),
    }
}
pub(crate) async fn disconnect(
    State(state): State<AppState>,
    Extension(peer): Extension<Peer>,
    headers: HeaderMap,
) -> Response {
    if !local_request(&headers, peer) {
        return error(StatusCode::FORBIDDEN, "Disconnect from Leo's computer.");
    }
    let notes = match notes(&state).await {
        Ok(n) => n,
        Err(c) => return c.into_response(),
    };
    let Some(secrets) = state.calendar_secrets.clone() else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Calendar credential storage is unavailable.",
        );
    };
    if state.calendar.busy.swap(true, Ordering::AcqRel) {
        return error(
            StatusCode::CONFLICT,
            "Wait for the calendar operation to finish.",
        );
    }
    let result = tokio::task::spawn_blocking(move || -> Result<()> {
        if path(&notes).exists() {
            let (account, credentials) = account(&notes, secrets.as_ref())?;
            let response = client()?
                .post("https://oauth2.googleapis.com/revoke")
                .form(&[("token", credentials.refresh_token)])
                .send()?;
            if !response.status().is_success()
                && response.status() != reqwest::StatusCode::BAD_REQUEST
            {
                bail!("Google could not revoke access; try again.");
            }
            secrets.delete(&account.secret_id)?;
            std::fs::remove_file(path(&notes))?;
        }
        if cache_path(&notes).exists() {
            std::fs::remove_file(cache_path(&notes))?;
        }
        Ok(())
    })
    .await;
    state.calendar.busy.store(false, Ordering::Release);
    match result {
        Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
        _ => error(
            StatusCode::BAD_GATEWAY,
            "Could not disconnect Google Calendar.",
        ),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Secrets(std::sync::Mutex<std::collections::HashMap<String, String>>);
    impl crate::CalendarSecrets for Secrets {
        fn get(&self, key: &str) -> Result<Option<String>> {
            Ok(self.0.lock().unwrap().get(key).cloned())
        }
        fn set(&self, key: &str, value: &str) -> Result<()> {
            self.0.lock().unwrap().insert(key.into(), value.into());
            Ok(())
        }
        fn delete(&self, key: &str) -> Result<()> {
            self.0.lock().unwrap().remove(key);
            Ok(())
        }
    }

    #[test]
    fn legacy_credentials_move_to_the_secret_backend_and_leave_a_private_settings_file() {
        let temp = tempfile::tempdir().unwrap();
        let notes = temp.path().join("notes");
        std::fs::write(path(&notes),serde_json::to_vec(&serde_json::json!({"client_id":"test.apps.googleusercontent.com","client_secret":"private-client","refresh_token":"private-refresh","calendar_id":"primary"})).unwrap()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path(&notes), std::fs::Permissions::from_mode(0o644)).unwrap();
        }
        let secrets = Secrets::default();
        let (settings, credentials) = account(&notes, &secrets).unwrap();
        assert_eq!(credentials.refresh_token, "private-refresh");
        assert_eq!(credentials.client_secret, "private-client");
        let disk = std::fs::read_to_string(path(&notes)).unwrap();
        assert!(!disk.contains("private-refresh"));
        assert!(!disk.contains("private-client"));
        assert!(!disk.contains("refresh_token"));
        assert!(disk.contains(&settings.secret_id));
        let (_, again) = account(&notes, &secrets).unwrap();
        assert_eq!(again.refresh_token, "private-refresh");
        assert_eq!(secrets.0.lock().unwrap().len(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(path(&notes))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn calendar_settings_do_not_follow_links_or_change_their_targets() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let temp = tempfile::tempdir().unwrap();
        let notes = temp.path().join("notes");
        let target = temp.path().join("unrelated.json");
        std::fs::write(&target, b"{}").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644)).unwrap();
        symlink(&target, path(&notes)).unwrap();
        assert!(account(&notes, &Secrets::default()).is_err());
        assert_eq!(
            std::fs::metadata(target).unwrap().permissions().mode() & 0o777,
            0o644
        );
    }

    #[test]
    fn the_sign_in_callback_waits_for_a_slow_browser_and_ignores_empty_connections() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let browser = std::thread::spawn(move || {
            drop(std::net::TcpStream::connect(("127.0.0.1", port)).unwrap());
            let mut slow = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            std::thread::sleep(Duration::from_millis(300));
            slow.write_all(
                b"GET /?state=nonce-1&code=the-code HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
            )
            .unwrap();
            let mut answer = String::new();
            let _ = slow.read_to_string(&mut answer);
            answer
        });
        assert_eq!(receive(listener, "nonce-1").unwrap(), "the-code");
        assert!(browser.join().unwrap().starts_with("HTTP/1.1 200"));
    }

    #[test]
    fn events_keep_timezone_and_skip_declined_and_all_day() {
        let mut raw = serde_json::json!({"id":"a","summary":"Design","start":{"dateTime":"2026-11-04T09:00:00-08:00"},"end":{"dateTime":"2026-11-04T10:00:00-08:00"},"attendees":[{"displayName":"Leo"}]});
        let e = event(&raw).unwrap().unwrap();
        assert_eq!(e.start.to_rfc3339(), "2026-11-04T17:00:00+00:00");
        assert!(e.context.contains("Leo"));
        raw["attendees"] = serde_json::json!([{"self":true,"responseStatus":"declined"}]);
        assert!(event(&raw).unwrap().is_none());
        raw["start"] = serde_json::json!({"date":"2026-11-04"});
        assert!(event(&raw).unwrap().is_none());
    }
}
