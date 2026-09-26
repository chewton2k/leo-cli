//! Whether a newer leo has been released.
//!
//! Releases go out on every push to main, so without this nobody would know.
//! It asks at most once a day, remembers the answer, and never slows anything
//! down: callers run it on a worker thread, and a failure is silence rather
//! than an error, since being offline is not something to report.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

const REPO: &str = "chewton2k/leo-cli";

/// How often GitHub is asked.
const EVERY: chrono::TimeDelta = chrono::TimeDelta::hours(24);

/// The last answer, kept next to the config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Cache {
    checked_at: DateTime<Utc>,
    /// The latest release as of `checked_at`, without the `v`. Empty when
    /// that check failed.
    latest: String,
}

/// The newer version, if one has been released. Asks GitHub at most once a
/// day; `LEO_NO_UPDATE_CHECK=1` turns it off.
pub fn available() -> Option<String> {
    if std::env::var("LEO_NO_UPDATE_CHECK").is_ok_and(|v| !v.is_empty() && v != "0") {
        return None;
    }
    let cache = leo_core::paths::config_dir()
        .ok()?
        .join("update-check.json");
    check(
        &cache,
        Utc::now(),
        env!("CARGO_PKG_VERSION"),
        latest_release,
    )
}

/// The command that updates leo in place.
pub fn install_command() -> String {
    format!("curl -fsSL https://raw.githubusercontent.com/{REPO}/main/install.sh | sh")
}

/// The version to tell the user about, if any. `fetch` is only called when
/// the cached answer is more than a day old.
fn check(
    cache_path: &Path,
    now: DateTime<Utc>,
    current: &str,
    fetch: impl FnOnce() -> Result<String>,
) -> Option<String> {
    let cached: Option<Cache> = std::fs::read_to_string(cache_path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok());
    let latest = match cached {
        Some(cache) if now - cache.checked_at < EVERY => cache.latest,
        _ => {
            // A failure is remembered too, so being offline does not mean a
            // request on every start.
            let latest = fetch().unwrap_or_default();
            let cache = Cache {
                checked_at: now,
                latest: latest.clone(),
            };
            if let Ok(text) = serde_json::to_string(&cache) {
                if let Some(parent) = cache_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(cache_path, text);
            }
            latest
        }
    };
    is_newer(&latest, current).then_some(latest)
}

/// The latest release's version, read from where GitHub's "latest" link
/// redirects. Not the API, which allows only 60 requests an hour per address.
fn latest_release() -> Result<String> {
    let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5))
        .user_agent(concat!("leo/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let response = client
        .head(format!("https://github.com/{REPO}/releases/latest"))
        .send()?;
    let location = response
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .context("GitHub did not say which release is the latest")?;
    tag_from_location(location).context("no version in GitHub's answer")
}

/// `.../releases/tag/v0.2.3` → `0.2.3`.
fn tag_from_location(location: &str) -> Option<String> {
    let tag = location.rsplit_once("/tag/")?.1;
    let version = tag.trim_start_matches('v');
    let valid = !version.is_empty() && version.split('.').all(|p| p.parse::<u64>().is_ok());
    valid.then(|| version.to_string())
}

/// Whether `candidate` is a later version than `current`, part by part as
/// numbers, so 0.2.10 is after 0.2.9. Anything unreadable is not newer.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    let parts = |v: &str| -> Option<Vec<u64>> {
        v.trim()
            .trim_start_matches('v')
            .split('.')
            .map(|p| p.parse().ok())
            .collect()
    };
    match (parts(candidate), parts(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn versions_compare_as_numbers() {
        assert!(is_newer("0.2.10", "0.2.9"));
        assert!(is_newer("0.3.0", "0.2.99"));
        assert!(is_newer("v1.0.0", "0.9.9"));
        assert!(!is_newer("0.2.1", "0.2.1"));
        assert!(!is_newer("0.2.0", "0.2.1"));
        assert!(!is_newer("", "0.2.1"));
        assert!(!is_newer("garbage", "0.2.1"));
    }

    #[test]
    fn the_version_comes_from_the_redirect() {
        assert_eq!(
            tag_from_location("https://github.com/chewton2k/leo-cli/releases/tag/v0.2.3")
                .as_deref(),
            Some("0.2.3")
        );
        assert_eq!(
            tag_from_location("https://github.com/chewton2k/leo-cli/releases"),
            None
        );
        assert_eq!(tag_from_location("https://x/releases/tag/nightly"), None);
    }

    /// A newer release is reported, and the answer is remembered for a day.
    #[test]
    fn asks_at_most_once_a_day() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = tmp.path().join("update-check.json");
        let asked = Cell::new(0);
        let fetch = || {
            asked.set(asked.get() + 1);
            Ok("0.2.5".to_string())
        };
        let now = Utc::now();

        assert_eq!(check(&cache, now, "0.2.1", fetch).as_deref(), Some("0.2.5"));
        assert_eq!(asked.get(), 1);
        let later_today = now + chrono::TimeDelta::hours(3);
        assert_eq!(
            check(&cache, later_today, "0.2.1", fetch).as_deref(),
            Some("0.2.5")
        );
        assert_eq!(asked.get(), 1, "asked again within the day");
        let tomorrow = now + chrono::TimeDelta::hours(25);
        check(&cache, tomorrow, "0.2.1", fetch);
        assert_eq!(asked.get(), 2);
    }

    #[test]
    fn nothing_is_said_when_up_to_date() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = tmp.path().join("update-check.json");
        assert_eq!(
            check(&cache, Utc::now(), "0.2.5", || Ok("0.2.5".into())),
            None
        );
    }

    /// Offline is not an error, and does not mean a request on every start.
    #[test]
    fn a_failed_check_is_quiet_and_remembered() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = tmp.path().join("update-check.json");
        let now = Utc::now();
        assert_eq!(
            check(&cache, now, "0.2.1", || anyhow::bail!("offline")),
            None
        );
        let asked = Cell::new(false);
        check(&cache, now + chrono::TimeDelta::hours(1), "0.2.1", || {
            asked.set(true);
            Ok("9.9.9".into())
        });
        assert!(!asked.get(), "retried within the day");
    }

    /// Reaches GitHub, so only by hand: `cargo test -p leo-services latest -- --ignored`.
    #[test]
    #[ignore]
    fn the_latest_release_is_read_from_github() {
        let latest = latest_release().unwrap();
        assert!(is_newer(&latest, "0.0.1"), "{latest}");
    }
}
