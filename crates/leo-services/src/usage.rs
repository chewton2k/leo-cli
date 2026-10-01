use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use chrono::{DateTime, Local, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const STALE_AFTER: Duration = Duration::from_secs(5 * 60);
pub const CODEX_WAIT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Window {
    pub used: f64,
    pub resets_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub five_hour: Option<Window>,
    pub seven_day: Option<Window>,
    pub seen_at: DateTime<Utc>,
}

pub type Seen = BTreeMap<String, Usage>;

fn window(value: &Value, used_key: &str, scale: f64, resets_key: &str) -> Option<Window> {
    let used = value[used_key].as_f64()? / scale;
    Some(Window {
        used,
        resets_at: value[resets_key].as_i64(),
    })
}

pub fn from_claude(info: &Value, now: DateTime<Utc>) -> Option<Usage> {
    let windows = &info["unifiedWindows"];
    let mut usage = Usage {
        five_hour: window(&windows["five_hour"], "utilization", 1.0, "resetsAt"),
        seven_day: window(&windows["seven_day"], "utilization", 1.0, "resetsAt"),
        seen_at: now,
    };
    if usage.five_hour.is_none() && usage.seven_day.is_none() {
        let only = window(info, "utilization", 1.0, "resetsAt");
        match info["rateLimitType"].as_str() {
            Some("five_hour") => usage.five_hour = only,
            Some("seven_day") => usage.seven_day = only,
            _ => return None,
        }
    }
    Some(usage)
}

pub fn from_codex(result: &Value, now: DateTime<Utc>) -> Option<Usage> {
    let limits = &result["rateLimits"];
    let mut usage = Usage {
        five_hour: None,
        seven_day: None,
        seen_at: now,
    };
    for (slot, fallback_to_week) in [("primary", false), ("secondary", true)] {
        let value = &limits[slot];
        let Some(found) = window(value, "usedPercent", 100.0, "resetsAt") else {
            continue;
        };
        let week = match value["windowDurationMins"].as_i64() {
            Some(minutes) => minutes >= 24 * 60,
            None => fallback_to_week,
        };
        if week {
            usage.seven_day = Some(found);
        } else {
            usage.five_hour = Some(found);
        }
    }
    (usage.five_hour.is_some() || usage.seven_day.is_some()).then_some(usage)
}

fn percent(window: &Window, now: DateTime<Utc>) -> u32 {
    if window.resets_at.is_some_and(|at| at <= now.timestamp()) {
        return 0;
    }
    (window.used * 100.0).round().clamp(0.0, 100.0) as u32
}

fn age(seen_at: DateTime<Utc>, now: DateTime<Utc>) -> Option<String> {
    let secs = (now - seen_at).num_seconds().max(0) as u64;
    if secs < STALE_AFTER.as_secs() {
        return None;
    }
    Some(match secs {
        s if s < 3600 => format!("{} min ago", s / 60),
        s if s < 86_400 => format!("{} h ago", s / 3600),
        s => format!("{} d ago", s / 86_400),
    })
}

pub fn label(usage: &Usage, now: DateTime<Utc>) -> String {
    let mut parts = Vec::new();
    if let Some(w) = &usage.five_hour {
        parts.push(format!("5h: {}%", percent(w, now)));
    }
    if let Some(w) = &usage.seven_day {
        parts.push(format!("7d: {}%", percent(w, now)));
    }
    if let Some(age) = age(usage.seen_at, now) {
        parts.push(age);
    }
    parts.join(", ")
}

fn resets(at: i64, now: DateTime<Utc>) -> String {
    let Some(when) = Local.timestamp_opt(at, 0).single() else {
        return String::new();
    };
    let today = now.with_timezone(&Local).date_naive() == when.date_naive();
    let text = if today {
        when.format("%-I:%M %p").to_string()
    } else {
        when.format("%a %-I:%M %p").to_string()
    };
    format!(" (resets {text})")
}

pub fn detail(usage: &Usage, now: DateTime<Utc>) -> String {
    let mut parts = Vec::new();
    for (name, w) in [("5 hours", &usage.five_hour), ("7 days", &usage.seven_day)] {
        if let Some(w) = w {
            let reset = match w.resets_at {
                Some(at) if at > now.timestamp() => resets(at, now),
                _ => String::new(),
            };
            parts.push(format!("{name}: {}% used{reset}", percent(w, now)));
        }
    }
    if let Some(age) = age(usage.seen_at, now) {
        parts.push(format!("as of {age}"));
    }
    parts.join("; ")
}

fn file() -> Option<PathBuf> {
    leo_core::paths::config_dir()
        .ok()
        .map(|dir| dir.join("usage.json"))
}

pub fn load_from(path: &Path) -> Seen {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save_to(path: &Path, provider: &str, usage: Usage) {
    let mut all = load_from(path);
    all.insert(provider.to_string(), usage);
    let Ok(text) = serde_json::to_string_pretty(&all) else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let temp = path.with_extension("json.new");
    if std::fs::write(&temp, text).is_ok() {
        let _ = std::fs::rename(&temp, path);
    }
}

pub fn load() -> Seen {
    file().map(|path| load_from(&path)).unwrap_or_default()
}

pub fn save(provider: &str, usage: Usage) {
    if let Some(path) = file() {
        save_to(&path, provider, usage);
    }
}

pub fn ask_codex(program: &Path) -> Option<Usage> {
    let mut child = Command::new(program)
        .arg("app-server")
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let stdout = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            let Ok(message) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if message["id"] == 2 {
                let _ = tx.send(message);
                break;
            }
        }
    });
    if let Some(mut stdin) = child.stdin.take() {
        let version = env!("CARGO_PKG_VERSION");
        let _ = writeln!(
            stdin,
            r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"clientInfo":{{"name":"leo","version":"{version}"}}}}}}"#
        );
        let _ = writeln!(stdin, r#"{{"jsonrpc":"2.0","method":"initialized"}}"#);
        let _ = writeln!(
            stdin,
            r#"{{"jsonrpc":"2.0","id":2,"method":"account/rateLimits/read"}}"#
        );
        let answer = rx.recv_timeout(CODEX_WAIT).ok();
        drop(stdin);
        let _ = child.kill();
        let _ = child.wait();
        return from_codex(&answer?["result"], Utc::now());
    }
    let _ = child.kill();
    let _ = child.wait();
    None
}

pub fn refresh_codex(cfg: &crate::config::Config) {
    let Some(pc) = cfg.provider("codex") else {
        return;
    };
    let bin = pc.bin.as_deref().unwrap_or("codex");
    let Some(program) = crate::ai::provider::agent_cli::locate(bin) else {
        return;
    };
    if let Some(usage) = ask_codex(&program) {
        save("codex", usage);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(secs, 0).unwrap()
    }

    #[test]
    fn claude_code_reports_both_windows() {
        let info: Value = serde_json::from_str(
            r#"{"status":"allowed_warning","rateLimitType":"seven_day","utilization":0.93,
                "unifiedWindows":{"five_hour":{"utilization":0.09,"resetsAt":2000},
                                  "seven_day":{"utilization":0.93,"resetsAt":9000}}}"#,
        )
        .unwrap();
        let usage = from_claude(&info, at(1000)).unwrap();
        assert_eq!(label(&usage, at(1000)), "5h: 9%, 7d: 93%");
    }

    #[test]
    fn an_older_claude_report_with_one_window_still_counts() {
        let info: Value =
            serde_json::from_str(r#"{"rateLimitType":"five_hour","utilization":0.45}"#).unwrap();
        let usage = from_claude(&info, at(1000)).unwrap();
        assert_eq!(label(&usage, at(1000)), "5h: 45%");
        assert!(from_claude(&serde_json::json!({}), at(1000)).is_none());
    }

    #[test]
    fn codex_windows_are_told_apart_by_their_length() {
        let result: Value = serde_json::from_str(
            r#"{"rateLimits":{"primary":{"usedPercent":2,"windowDurationMins":300,"resetsAt":5000},
                              "secondary":{"usedPercent":47,"windowDurationMins":10080,"resetsAt":9000}}}"#,
        )
        .unwrap();
        let usage = from_codex(&result, at(1000)).unwrap();
        assert_eq!(label(&usage, at(1000)), "5h: 2%, 7d: 47%");
        assert!(from_codex(&serde_json::json!({"rateLimits": {}}), at(1000)).is_none());
    }

    #[test]
    fn a_window_that_has_reset_reads_zero_and_old_numbers_say_how_old() {
        let usage = Usage {
            five_hour: Some(Window {
                used: 0.8,
                resets_at: Some(1500),
            }),
            seven_day: Some(Window {
                used: 0.5,
                resets_at: Some(900_000),
            }),
            seen_at: at(1000),
        };
        assert_eq!(label(&usage, at(1200)), "5h: 80%, 7d: 50%");
        assert_eq!(
            label(&usage, at(1000 + 12 * 60)),
            "5h: 0%, 7d: 50%, 12 min ago"
        );
        assert_eq!(
            label(&usage, at(1000 + 3 * 3600)),
            "5h: 0%, 7d: 50%, 3 h ago"
        );
        let text = detail(&usage, at(1200));
        assert!(text.starts_with("5 hours: 80% used (resets "), "{text}");
        assert!(text.contains("7 days: 50% used (resets "), "{text}");
    }

    #[test]
    fn the_latest_numbers_are_kept_per_provider() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("usage.json");
        assert!(load_from(&path).is_empty());
        let usage = |used| Usage {
            five_hour: Some(Window {
                used,
                resets_at: None,
            }),
            seven_day: None,
            seen_at: at(1000),
        };
        save_to(&path, "claude_code", usage(0.1));
        save_to(&path, "codex", usage(0.2));
        save_to(&path, "claude_code", usage(0.3));
        let all = load_from(&path);
        assert_eq!(all.len(), 2);
        assert_eq!(all["claude_code"].five_hour.unwrap().used, 0.3);
        assert_eq!(all["codex"].five_hour.unwrap().used, 0.2);
        std::fs::write(&path, "not json").unwrap();
        assert!(load_from(&path).is_empty());
    }
}
