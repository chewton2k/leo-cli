use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::token;

pub const COOKIE: &str = "leo_session";
pub const LEGACY_COOKIE: &str = "leo_token";
pub const KEEP_DAYS: i64 = 30;
const TOUCH_EVERY_SECS: i64 = 300;
const MOST: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Session {
    pub secret: String,
    pub device: String,
    pub created_at: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    #[serde(default)]
    pub site: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Seen {
    pub handle: String,
    pub device: String,
    pub created_at: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub current: bool,
    pub place: String,
}

const LINK_DOMAIN: &str = ".trycloudflare.com";

pub fn site_of(host: &str) -> String {
    let host = host.trim().to_ascii_lowercase();
    if host.starts_with('[') {
        return host
            .split(']')
            .next()
            .map(|h| format!("{h}]"))
            .unwrap_or_default();
    }
    host.split(':').next().unwrap_or("").to_string()
}

pub fn place_of(site: &str) -> String {
    match site {
        "" => String::new(),
        "localhost" | "127.0.0.1" | "[::1]" => "on this computer".into(),
        s if s.ends_with(LINK_DOMAIN) => "through the link from any network".into(),
        _ => "on your Wi-Fi".into(),
    }
}

pub struct Sessions {
    path: Option<PathBuf>,
    list: Mutex<Vec<Session>>,
}

pub fn handle_of(secret: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in format!("leo-session:{secret}").bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

pub fn device_of(user_agent: &str) -> String {
    let ua = user_agent.to_lowercase();
    let system = if ua.contains("iphone") {
        "iPhone"
    } else if ua.contains("ipad") {
        "iPad"
    } else if ua.contains("android") {
        "Android"
    } else if ua.contains("windows") {
        "Windows"
    } else if ua.contains("mac os x") || ua.contains("macintosh") {
        "Mac"
    } else if ua.contains("cros") {
        "ChromeOS"
    } else if ua.contains("linux") {
        "Linux"
    } else {
        ""
    };
    let browser = if ua.contains("edg/") {
        "Edge"
    } else if ua.contains("firefox/") || ua.contains("fxios/") {
        "Firefox"
    } else if ua.contains("opr/") {
        "Opera"
    } else if ua.contains("chrome/") || ua.contains("crios/") {
        "Chrome"
    } else if ua.contains("safari/") {
        "Safari"
    } else {
        ""
    };
    match (browser, system) {
        ("", "") => "A browser".to_string(),
        ("", system) => format!("A browser on {system}"),
        (browser, "") => browser.to_string(),
        (browser, system) => format!("{browser} on {system}"),
    }
}

impl Sessions {
    pub fn in_memory() -> Sessions {
        Sessions {
            path: None,
            list: Mutex::new(Vec::new()),
        }
    }

    pub fn load(path: &Path) -> Sessions {
        let list: Vec<Session> = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Sessions {
            path: Some(path.to_path_buf()),
            list: Mutex::new(list),
        }
    }

    fn save(&self, list: &[Session]) {
        let Some(path) = &self.path else {
            return;
        };
        if let Ok(text) = serde_json::to_string_pretty(list) {
            let _ = token::write_private(path, &text);
        }
    }

    fn change<R>(&self, work: impl FnOnce(&mut Vec<Session>) -> (R, bool)) -> R {
        let mut list = self.list.lock().unwrap_or_else(|e| e.into_inner());
        let (out, changed) = work(&mut list);
        if changed {
            self.save(&list);
        }
        out
    }

    pub fn start(&self, user_agent: &str, site: &str, now: DateTime<Utc>) -> String {
        let secret = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let session = Session {
            secret: secret.clone(),
            device: device_of(user_agent),
            created_at: now,
            last_seen: now,
            site: site_of(site),
        };
        self.change(|list| {
            list.retain(|s| now - s.last_seen < Duration::days(KEEP_DAYS));
            list.push(session);
            list.sort_by_key(|s| std::cmp::Reverse(s.last_seen));
            list.truncate(MOST);
            ((), true)
        });
        secret
    }

    pub fn check(&self, secret: &str, now: DateTime<Utc>) -> bool {
        self.change(|list| {
            let Some(found) = list.iter_mut().find(|s| token::same(&s.secret, secret)) else {
                return (false, false);
            };
            if now - found.last_seen >= Duration::days(KEEP_DAYS) {
                list.retain(|s| !token::same(&s.secret, secret));
                return (false, true);
            }
            let stale = (now - found.last_seen).num_seconds() >= TOUCH_EVERY_SECS;
            if stale {
                found.last_seen = now;
            }
            (true, stale)
        })
    }

    pub fn list(&self, current: Option<&str>, now: DateTime<Utc>) -> Vec<Seen> {
        let list = self.list.lock().unwrap_or_else(|e| e.into_inner());
        let mut out: Vec<Seen> = list
            .iter()
            .filter(|s| now - s.last_seen < Duration::days(KEEP_DAYS))
            .map(|s| Seen {
                handle: handle_of(&s.secret),
                device: s.device.clone(),
                created_at: s.created_at,
                last_seen: s.last_seen,
                current: current.is_some_and(|c| token::same(c, &s.secret)),
                place: place_of(&s.site),
            })
            .collect();
        out.sort_by(|a, b| {
            b.current
                .cmp(&a.current)
                .then(b.last_seen.cmp(&a.last_seen))
        });
        out
    }

    pub fn end(&self, handle: &str) -> bool {
        self.change(|list| {
            let before = list.len();
            list.retain(|s| handle_of(&s.secret) != handle);
            let gone = list.len() != before;
            (gone, gone)
        })
    }

    pub fn end_others(&self, current: &str) -> usize {
        self.change(|list| {
            let before = list.len();
            list.retain(|s| token::same(&s.secret, current));
            let gone = before - list.len();
            (gone, gone > 0)
        })
    }

    pub fn retire_links(&self, current: Option<&str>) -> usize {
        let current = current.map(site_of);
        self.change(|list| {
            let before = list.len();
            list.retain(|s| {
                !s.site.ends_with(LINK_DOMAIN) || current.as_deref() == Some(s.site.as_str())
            });
            let gone = before - list.len();
            (gone, gone > 0)
        })
    }

    pub fn end_all(&self) -> usize {
        self.change(|list| {
            let gone = list.len();
            list.clear();
            (gone, gone > 0)
        })
    }
}

pub struct Gate {
    token: std::sync::RwLock<String>,
    token_path: Option<PathBuf>,
    pub sessions: Sessions,
}

impl Gate {
    pub fn new(token: String, token_path: Option<PathBuf>, sessions: Sessions) -> Gate {
        Gate {
            token: std::sync::RwLock::new(token),
            token_path,
            sessions,
        }
    }

    pub fn token_matches(&self, given: &str) -> bool {
        let token = self.token.read().unwrap_or_else(|e| e.into_inner());
        token::same(given, &token)
    }

    pub fn new_link_code(&self) -> anyhow::Result<String> {
        let fresh = match &self.token_path {
            Some(path) => token::load_or_create(path, true)?,
            None => uuid::Uuid::new_v4().simple().to_string(),
        };
        *self.token.write().unwrap_or_else(|e| e.into_inner()) = fresh.clone();
        Ok(fresh)
    }
}

pub fn cookie_value<'a>(cookies: &'a str, name: &str) -> Option<&'a str> {
    cookies
        .split(';')
        .filter_map(|part| part.trim().split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(minutes: i64) -> DateTime<Utc> {
        chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 10, 8, 12, 0, 0).unwrap()
            + Duration::minutes(minutes)
    }

    #[test]
    fn each_browser_gets_its_own_session_and_one_can_be_signed_out() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("serve-sessions.json");
        let sessions = Sessions::load(&path);
        let phone = sessions.start(
            "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) Version/18.0 Mobile/15E148 Safari/604.1", "127.0.0.1:8742",
            at(0),
        );
        let laptop = sessions.start(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) Chrome/130.0 Safari/537.36",
            "127.0.0.1:8742",
            at(1),
        );
        assert_ne!(phone, laptop);
        assert_eq!(phone.len(), 64);
        assert!(sessions.check(&phone, at(2)));
        assert!(!sessions.check("nope", at(2)));

        let seen = sessions.list(Some(&laptop), at(3));
        assert_eq!(seen.len(), 2);
        assert_eq!(
            (seen[0].device.as_str(), seen[0].current),
            ("Chrome on Mac", true)
        );
        assert_eq!(seen[1].device, "Safari on iPhone");
        assert!(
            !seen.iter().any(|s| s.handle == phone || s.handle == laptop),
            "secrets never leave"
        );

        assert!(sessions.end(&seen[1].handle));
        assert!(!sessions.check(&phone, at(4)));
        assert!(sessions.check(&laptop, at(4)));

        let again = Sessions::load(&path);
        assert!(again.check(&laptop, at(5)), "sessions survive a restart");
        assert!(!again.check(&phone, at(5)));
    }

    #[test]
    fn signing_out_the_others_keeps_this_browser_and_old_sessions_expire() {
        let sessions = Sessions::in_memory();
        let mine = sessions.start("Firefox/131.0 (Windows NT 10.0)", "127.0.0.1:8742", at(0));
        let other = sessions.start("", "127.0.0.1:8742", at(0));
        assert_eq!(sessions.end_others(&mine), 1);
        assert!(!sessions.check(&other, at(1)));
        assert!(sessions.check(&mine, at(1)));
        let month = KEEP_DAYS * 24 * 60;
        assert!(
            !sessions.check(&mine, at(month + 10)),
            "a session unused for a month expires"
        );
        let a = sessions.start("", "127.0.0.1:8742", at(0));
        assert_eq!(sessions.end_all(), 1);
        assert!(!sessions.check(&a, at(1)));
    }

    #[test]
    fn last_seen_is_written_at_most_every_few_minutes() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("s.json");
        let sessions = Sessions::load(&path);
        let id = sessions.start("", "127.0.0.1:8742", at(0));
        let written = std::fs::metadata(&path).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(sessions.check(&id, at(1)));
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            written
        );
        assert!(sessions.check(&id, at(6)));
        assert_eq!(sessions.list(None, at(6))[0].last_seen, at(6));
    }

    #[test]
    fn devices_are_named_the_way_people_say_them() {
        let cases = [
            ("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/130.0 Safari/537.36 Edg/130.0", "Edge on Windows"),
            ("Mozilla/5.0 (Linux; Android 14; Pixel 7) Chrome/130.0 Mobile Safari/537.36", "Chrome on Android"),
            ("Mozilla/5.0 (iPad; CPU OS 17_0 like Mac OS X) CriOS/130.0 Mobile Safari/604.1", "Chrome on iPad"),
            ("Mozilla/5.0 (X11; Linux x86_64; rv:131.0) Gecko/20100101 Firefox/131.0", "Firefox on Linux"),
            ("curl/8.0", "A browser"),
        ];
        for (ua, want) in cases {
            assert_eq!(device_of(ua), want, "{ua}");
        }
    }

    #[test]
    fn a_new_link_code_retires_the_old_one_and_is_saved() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("serve-token");
        let first = token::load_or_create(&path, false).unwrap();
        let gate = Gate::new(first.clone(), Some(path.clone()), Sessions::in_memory());
        assert!(gate.token_matches(&first));
        let second = gate.new_link_code().unwrap();
        assert!(!gate.token_matches(&first));
        assert!(gate.token_matches(&second));
        assert_eq!(token::load_or_create(&path, false).unwrap(), second);
    }

    #[test]
    fn cookies_are_read_by_exact_name() {
        let header = "leo_token=old; leo_session=abc; other=1";
        assert_eq!(cookie_value(header, COOKIE), Some("abc"));
        assert_eq!(cookie_value(header, LEGACY_COOKIE), Some("old"));
        assert_eq!(cookie_value("xleo_session=bad", COOKIE), None);
    }

    #[test]
    fn a_session_says_where_it_signed_in_and_old_links_are_retired() {
        let sessions = Sessions::in_memory();
        let here = sessions.start(
            "Chrome/130.0 (Macintosh; Mac OS X)",
            "127.0.0.1:8742",
            at(0),
        );
        let wifi = sessions.start("(iPhone) Safari/604.1", "192.168.1.20:8742", at(0));
        sessions.start(
            "(iPhone) Safari/604.1",
            "old-words.trycloudflare.com",
            at(0),
        );
        sessions.start("(iPad) Safari/604.1", "new-words.trycloudflare.com", at(0));
        let places: Vec<String> = sessions
            .list(Some(&here), at(1))
            .into_iter()
            .map(|s| s.place)
            .collect();
        assert!(places.contains(&"on this computer".to_string()));
        assert!(places.contains(&"on your Wi-Fi".to_string()));
        assert!(places.contains(&"through the link from any network".to_string()));
        assert_eq!(
            sessions.retire_links(Some("new-words.trycloudflare.com")),
            1
        );
        assert_eq!(sessions.list(None, at(1)).len(), 3);
        assert_eq!(
            sessions.retire_links(None),
            1,
            "without a link, no link session can be used"
        );
        assert!(sessions.check(&here, at(2)) && sessions.check(&wifi, at(2)));
        assert_eq!(site_of("[::1]:8742"), "[::1]");
        assert_eq!(site_of("ABC.trycloudflare.com"), "abc.trycloudflare.com");
        assert_eq!(place_of(""), "");
    }
}
