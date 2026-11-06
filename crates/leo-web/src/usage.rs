use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::Serialize;

use crate::chat::Spent;

const LATENCIES: usize = 64;
const KEPT_DAYS: u64 = 90;
const WEEK_DAYS: u64 = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    Answer,
    ToolContinuation,
    Retry,
    StudyGrading,
    Summary,
    OtherWriting,
    QuickReview,
    MarginRead,
}

#[derive(Default)]
struct Counts {
    calls: u64,
    failures: u64,
    input: u64,
    cached: u64,
    output: u64,
    unreported: u64,
    estimated: u64,
    latency: VecDeque<u64>,
}

#[derive(Default)]
pub struct Usage {
    rows: Mutex<BTreeMap<Purpose, Counts>>,
    tools: Mutex<Tools>,
    saved_in: OnceLock<PathBuf>,
}

#[derive(Clone, Default, Serialize)]
pub struct Tools {
    calls: u64,
    failures: u64,
    reused: u64,
}

#[derive(Debug, Default, PartialEq, Eq, Serialize)]
pub struct Model {
    pub model: String,
    pub calls: u64,
    pub input: u64,
    pub cached: u64,
    pub output: u64,
}

#[derive(Serialize)]
pub struct Row {
    purpose: Purpose,
    calls: u64,
    failures: u64,
    input: u64,
    cached: u64,
    output: u64,
    unreported: u64,
    estimated: u64,
    median_ms: u64,
    p95_ms: u64,
}

fn day_of(ago: u64) -> Option<String> {
    let today = chrono::Local::now().date_naive();
    let day = today.checked_sub_days(chrono::Days::new(ago))?;
    Some(day.format("%Y-%m-%d").to_string())
}

impl Usage {
    pub fn save_in(&self, chats: PathBuf) {
        let _ = self.saved_in.set(chats);
    }

    fn save(&self, purpose: Purpose, spent: Option<&Spent>) {
        let Some(chats) = self.saved_in.get() else {
            return;
        };
        let (Some(today), Some(oldest)) = (day_of(0), day_of(KEPT_DAYS)) else {
            return;
        };
        let Ok(purpose) = serde_json::to_value(purpose) else {
            return;
        };
        let model = spent.and_then(|s| s.model.as_deref()).unwrap_or("");
        let (input, cached, output) = spent.map_or((0, 0, 0), |s| {
            (
                s.input.min(i64::MAX as u64),
                s.cached.min(s.input).min(i64::MAX as u64),
                s.output.min(i64::MAX as u64),
            )
        });
        let saved = crate::db::for_chats(chats).with(|c| {
            c.execute(
                "INSERT INTO usage_days (day, purpose, model, calls, input, cached, output)
                 VALUES (?1, ?2, ?3, 1, ?4, ?5, ?6)
                 ON CONFLICT (day, purpose, model) DO UPDATE SET
                   calls = calls + 1,
                   input = input + excluded.input,
                   cached = cached + excluded.cached,
                   output = output + excluded.output",
                rusqlite::params![
                    today,
                    purpose.as_str().unwrap_or(""),
                    model,
                    input as i64,
                    cached as i64,
                    output as i64
                ],
            )?;
            c.execute("DELETE FROM usage_days WHERE day < ?1", [oldest])
        });
        if let Err(e) = saved {
            leo_core::diag::warn(format!("could not save usage: {e}"));
        }
    }

    pub fn week(&self) -> Vec<Model> {
        let (Some(chats), Some(first)) = (self.saved_in.get(), day_of(WEEK_DAYS - 1)) else {
            return Vec::new();
        };
        let rows = crate::db::for_chats(chats).with(|c| {
            c.prepare(
                "SELECT model, SUM(calls), SUM(input), SUM(cached), SUM(output)
                 FROM usage_days WHERE day >= ?1 GROUP BY model ORDER BY SUM(input) DESC",
            )?
            .query_map([first], |r| {
                Ok(Model {
                    model: r.get(0)?,
                    calls: r.get::<_, i64>(1)?.max(0) as u64,
                    input: r.get::<_, i64>(2)?.max(0) as u64,
                    cached: r.get::<_, i64>(3)?.max(0) as u64,
                    output: r.get::<_, i64>(4)?.max(0) as u64,
                })
            })?
            .collect()
        });
        rows.unwrap_or_default()
    }

    pub(crate) fn tool(&self, failed: bool, reused: bool) {
        let mut held = self.tools.lock().unwrap_or_else(|e| e.into_inner());
        held.calls = held.calls.saturating_add(1);
        held.failures = held.failures.saturating_add(u64::from(failed));
        held.reused = held.reused.saturating_add(u64::from(reused));
    }

    pub fn tools(&self) -> Tools {
        self.tools.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn record(&self, purpose: Purpose, spent: Option<&Spent>, elapsed: Duration, failed: bool) {
        self.save(purpose, spent);
        let mut held = self.rows.lock().unwrap_or_else(|e| e.into_inner());
        let row = held.entry(purpose).or_default();
        row.calls = row.calls.saturating_add(1);
        row.failures = row.failures.saturating_add(u64::from(failed));
        if let Some(spent) = spent {
            row.input = row.input.saturating_add(spent.input);
            row.cached = row.cached.saturating_add(spent.cached.min(spent.input));
            row.output = row.output.saturating_add(spent.output);
            row.estimated = row.estimated.saturating_add(u64::from(spent.estimated));
        } else {
            row.unreported = row.unreported.saturating_add(1);
        }
        if row.latency.len() == LATENCIES {
            row.latency.pop_front();
        }
        row.latency
            .push_back(elapsed.as_millis().min(u128::from(u64::MAX)) as u64);
    }

    pub fn line(&self) -> String {
        let held = self.rows.lock().unwrap_or_else(|e| e.into_inner());
        let (calls, input, cached, output) =
            held.values().fold((0u64, 0u64, 0u64, 0u64), |t, r| {
                (t.0 + r.calls, t.1 + r.input, t.2 + r.cached, t.3 + r.output)
            });
        drop(held);
        if calls == 0 {
            return "No AI requests since Leo started".into();
        }
        let tokens = input.saturating_add(output);
        let kind = if calls == 1 { "request" } else { "requests" };
        let cache = if cached > 0 {
            format!(" ({} cached)", short(cached))
        } else {
            String::new()
        };
        format!("{calls} AI {kind} · {} tokens{cache}", short(tokens))
    }

    pub fn snapshot(&self) -> Vec<Row> {
        let held = self.rows.lock().unwrap_or_else(|e| e.into_inner());
        held.iter()
            .map(|(purpose, row)| {
                let mut times: Vec<_> = row.latency.iter().copied().collect();
                times.sort_unstable();
                Row {
                    purpose: *purpose,
                    calls: row.calls,
                    failures: row.failures,
                    input: row.input,
                    cached: row.cached,
                    output: row.output,
                    unreported: row.unreported,
                    estimated: row.estimated,
                    median_ms: times
                        .get(times.len().saturating_sub(1) / 2)
                        .copied()
                        .unwrap_or(0),
                    p95_ms: times
                        .get((times.len() * 95).div_ceil(100).saturating_sub(1))
                        .copied()
                        .unwrap_or(0),
                }
            })
            .collect()
    }
}

fn short(n: u64) -> String {
    match n {
        n if n >= 1_000_000 => format!("{:.1}M", n as f64 / 1_000_000.0),
        n if n >= 1_000 => format!("{}k", n / 1_000),
        n => n.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_bar_line_adds_up_every_kind_of_work() {
        let usage = Usage::default();
        assert_eq!(usage.line(), "No AI requests since Leo started");
        let spent = Spent {
            input: 300_000,
            cached: 210_000,
            output: 40_000,
            ..Default::default()
        };
        usage.record(
            Purpose::Answer,
            Some(&spent),
            Duration::from_millis(5),
            false,
        );
        usage.record(Purpose::QuickReview, None, Duration::from_millis(5), false);
        assert_eq!(usage.line(), "2 AI requests · 340k tokens (210k cached)");
    }

    #[test]
    fn usage_separates_work_and_bounds_samples_without_retaining_content() {
        let usage = Usage::default();
        for i in 0..100 {
            usage.record(
                Purpose::Answer,
                Some(&Spent {
                    input: 100,
                    cached: 80,
                    output: 10,
                    ..Default::default()
                }),
                Duration::from_millis(i),
                false,
            );
        }
        usage.record(Purpose::Retry, None, Duration::from_secs(1), true);
        let held = usage.rows.lock().unwrap();
        assert_eq!(held[&Purpose::Answer].latency.len(), LATENCIES);
        drop(held);
        let rows = usage.snapshot();
        assert_eq!(
            (rows[0].calls, rows[0].input, rows[0].cached, rows[0].output),
            (100, 10_000, 8_000, 1_000)
        );
        assert_eq!((rows[0].median_ms, rows[0].p95_ms), (67, 96));
        assert_eq!(
            (rows[1].calls, rows[1].failures, rows[1].unreported),
            (1, 1, 1)
        );
    }

    fn spent(model: &str, input: u64, cached: u64, output: u64) -> Spent {
        Spent {
            model: Some(model.into()),
            input,
            cached,
            output,
            ..Default::default()
        }
    }

    #[test]
    fn saved_usage_survives_a_restart_and_adds_up_by_model() {
        let home = tempfile::tempdir().unwrap();
        let chats = home.path().join("chats");
        let first = Usage::default();
        first.save_in(chats.clone());
        let elapsed = Duration::from_millis(1);
        first.record(
            Purpose::Answer,
            Some(&spent("big", 1_000, 900, 50)),
            elapsed,
            false,
        );
        first.record(
            Purpose::Answer,
            Some(&spent("big", 1_000, 2_000, 50)),
            elapsed,
            true,
        );
        first.record(
            Purpose::Summary,
            Some(&spent("small", 100, 0, 5)),
            elapsed,
            false,
        );
        first.record(Purpose::QuickReview, None, elapsed, false);

        let restarted = Usage::default();
        restarted.save_in(chats.clone());
        assert_eq!(
            restarted.week(),
            vec![
                Model {
                    model: "big".into(),
                    calls: 2,
                    input: 2_000,
                    cached: 1_900,
                    output: 100
                },
                Model {
                    model: "small".into(),
                    calls: 1,
                    input: 100,
                    cached: 0,
                    output: 5
                },
                Model {
                    model: String::new(),
                    calls: 1,
                    input: 0,
                    cached: 0,
                    output: 0
                },
            ]
        );
    }

    #[test]
    fn saved_usage_leaves_old_days_out_and_forgets_past_its_limit() {
        let home = tempfile::tempdir().unwrap();
        let chats = home.path().join("chats");
        let usage = Usage::default();
        usage.save_in(chats.clone());
        let db = crate::db::for_chats(&chats);
        let row = "INSERT INTO usage_days VALUES (?1, 'answer', 'old', 1, 10, 0, 1)";
        db.with(|c| c.execute(row, [day_of(WEEK_DAYS).unwrap()]))
            .unwrap();
        db.with(|c| c.execute(row, [day_of(KEPT_DAYS + 1).unwrap()]))
            .unwrap();
        assert!(usage.week().is_empty());
        usage.record(
            Purpose::Answer,
            Some(&spent("new", 1, 0, 1)),
            Duration::ZERO,
            false,
        );
        let left: i64 = db
            .with(|c| c.query_row("SELECT COUNT(*) FROM usage_days", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(left, 2);
        assert_eq!(usage.week().len(), 1);
    }

    #[test]
    fn usage_without_a_place_to_save_still_counts_in_memory() {
        let usage = Usage::default();
        usage.record(Purpose::Answer, None, Duration::ZERO, false);
        assert_eq!(usage.snapshot().len(), 1);
        assert!(usage.week().is_empty());
    }
}
