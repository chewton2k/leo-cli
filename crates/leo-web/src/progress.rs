use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::chat::Streamer;
use crate::graph::Writer;

const MOST_SHOWN: f64 = 0.97;
const MOST_METERS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Kind {
    pub name: &'static str,
    pub chars: f64,
    pub secs: f64,
}

pub const KEY: Kind = Kind {
    name: "key",
    chars: 6_000.0,
    secs: 45.0,
};
pub const GRADE: Kind = Kind {
    name: "grade",
    chars: 1_500.0,
    secs: 10.0,
};
pub const COMBINE: Kind = Kind {
    name: "combine",
    chars: 6_000.0,
    secs: 60.0,
};
pub const LANGUAGE: Kind = Kind {
    name: "language",
    chars: 600.0,
    secs: 6.0,
};
pub const COVERAGE: Kind = Kind {
    name: "coverage",
    chars: 500.0,
    secs: 5.0,
};

const KINDS: [Kind; 5] = [KEY, GRADE, COMBINE, COVERAGE, LANGUAGE];
const KEPT_RUNS: usize = 12;
const ON_PACE: f64 = 0.9;
const SLOW_AFTER: f64 = 1.25;

fn timed(elapsed: f64, expected: f64) -> f64 {
    let expected = expected.max(0.5);
    if elapsed <= expected {
        ON_PACE * elapsed / expected
    } else {
        ON_PACE + (MOST_SHOWN - ON_PACE) * (1.0 - (-(elapsed - expected) / expected).exp())
    }
}

fn upper(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(|a, b| a.total_cmp(b));
    Some(values[(values.len() * 3 / 4).min(values.len() - 1)])
}

pub struct Meter {
    id: u64,
    kind: Kind,
    chars: f64,
    secs: f64,
    got: AtomicUsize,
    started: Instant,
}

impl Meter {
    pub fn add(&self, text: &str) {
        self.got.fetch_add(text.chars().count(), Ordering::Relaxed);
    }

    pub fn restart(&self) {
        self.got.store(0, Ordering::Relaxed);
    }

    fn fraction_at(&self, elapsed: f64) -> f64 {
        let written = self.got.load(Ordering::Relaxed) as f64 / self.chars.max(1.0);
        (written.min(1.0) * MOST_SHOWN)
            .max(timed(elapsed, self.secs))
            .clamp(0.0, MOST_SHOWN)
    }

    pub fn view(&self) -> serde_json::Value {
        let elapsed = self.started.elapsed().as_secs_f64();
        serde_json::json!({
            "fraction": (self.fraction_at(elapsed) * 1000.0).round() / 1000.0,
            "seconds": elapsed as u64,
            "expected": self.secs.round() as u64,
            "left": (self.secs - elapsed).max(0.0).round() as u64,
            "slow": elapsed > self.secs * SLOW_AFTER,
        })
    }
}

#[derive(Default)]
pub struct Board {
    meters: Mutex<HashMap<String, Arc<Meter>>>,
    runs: Mutex<HashMap<&'static str, Vec<(f64, f64)>>>,
    next: AtomicUsize,
    kept: Option<std::path::PathBuf>,
}

pub struct Ticket {
    board: Arc<Board>,
    key: String,
    pub meter: Arc<Meter>,
}

impl Drop for Ticket {
    fn drop(&mut self) {
        self.board.finish(&self.key, &self.meter);
    }
}

impl Board {
    pub fn kept_in(chats: &std::path::Path) -> Board {
        let mut runs = HashMap::new();
        let _ = crate::db::for_chats(chats).with(|c| {
            let mut rows = c.prepare("SELECT kind, data FROM timings")?;
            let found =
                rows.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            for (name, data) in found.flatten() {
                let Some(kind) = KINDS.iter().find(|k| k.name == name) else {
                    continue;
                };
                if let Ok(list) = serde_json::from_str::<Vec<(f64, f64)>>(&data) {
                    let list: Vec<_> = list
                        .into_iter()
                        .filter(|(secs, chars)| {
                            secs.is_finite() && *secs > 0.0 && chars.is_finite() && *chars > 0.0
                        })
                        .take(KEPT_RUNS)
                        .collect();
                    runs.insert(kind.name, list);
                }
            }
            Ok(())
        });
        Board {
            runs: Mutex::new(runs),
            kept: Some(chats.to_path_buf()),
            ..Board::default()
        }
    }

    fn estimate(&self, kind: Kind, expected: Option<f64>) -> (f64, f64) {
        let runs = self
            .runs
            .lock()
            .ok()
            .and_then(|r| r.get(kind.name).cloned())
            .unwrap_or_default();
        let mut chars: Vec<f64> = runs.iter().map(|r| r.1).collect();
        let mut secs: Vec<f64> = runs.iter().map(|r| r.0).collect();
        let mut pace: Vec<f64> = runs.iter().map(|r| r.0 / r.1).collect();
        match expected {
            Some(size) if size > 0.0 => {
                let pace = upper(&mut pace).unwrap_or(kind.secs / kind.chars.max(1.0));
                (size, (pace * size).clamp(5.0, 900.0))
            }
            _ => (
                upper(&mut chars).unwrap_or(kind.chars),
                upper(&mut secs).unwrap_or(kind.secs),
            ),
        }
    }

    pub fn start(self: &Arc<Self>, key: String, kind: Kind) -> Option<Ticket> {
        self.start_sized(key, kind, None)
    }

    pub fn start_sized(
        self: &Arc<Self>,
        key: String,
        kind: Kind,
        expected: Option<f64>,
    ) -> Option<Ticket> {
        let (chars, secs) = self.estimate(kind, expected);
        let mut meters = self.meters.lock().ok()?;
        if meters.contains_key(&key) || meters.len() >= MOST_METERS {
            return None;
        }
        let meter = Arc::new(Meter {
            id: self.next.fetch_add(1, Ordering::Relaxed) as u64,
            kind,
            chars,
            secs,
            got: AtomicUsize::new(0),
            started: Instant::now(),
        });
        meters.insert(key.clone(), Arc::clone(&meter));
        Some(Ticket {
            board: Arc::clone(self),
            key,
            meter,
        })
    }

    fn finish(&self, key: &str, meter: &Meter) {
        if let Ok(mut meters) = self.meters.lock() {
            if meters.get(key).is_some_and(|m| m.id == meter.id) {
                meters.remove(key);
            }
        }
        let chars = meter.got.load(Ordering::Relaxed) as f64;
        if chars <= 0.0 {
            return;
        }
        self.record(meter.kind, meter.started.elapsed().as_secs_f64(), chars);
    }

    fn record(&self, kind: Kind, secs: f64, chars: f64) {
        let list = {
            let Ok(mut runs) = self.runs.lock() else {
                return;
            };
            let list = runs.entry(kind.name).or_default();
            list.insert(0, (secs, chars));
            list.truncate(KEPT_RUNS);
            list.clone()
        };
        if let (Some(chats), Ok(data)) = (&self.kept, serde_json::to_string(&list)) {
            let _ = crate::db::for_chats(chats).with(|c| {
                c.execute(
                    "INSERT OR REPLACE INTO timings (kind, data) VALUES (?1, ?2)",
                    rusqlite::params![kind.name, data],
                )
            });
        }
    }

    pub fn view(&self, key: &str) -> Option<serde_json::Value> {
        self.meters.lock().ok()?.get(key).map(|m| m.view())
    }
}

pub fn valid_key(key: &str) -> bool {
    key.len() <= 200
        && key.split_once(':').is_some_and(|(kind, rest)| {
            [
                KEY.name,
                GRADE.name,
                COVERAGE.name,
                COMBINE.name,
                LANGUAGE.name,
            ]
            .contains(&kind)
                && !rest.is_empty()
                && rest
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == ':')
        })
}

pub fn metered(writer: Writer, streamer: Option<Streamer>, meter: Arc<Meter>) -> Writer {
    Arc::new(move |system: &str, user: &str, most: u32| match &streamer {
        Some(stream) => {
            let reply = stream(
                system,
                user,
                most,
                &mut |piece| meter.add(piece),
                &mut || meter.restart(),
            )?;
            Ok(reply.text)
        }
        None => {
            let text = writer(system, user, most)?;
            meter.add(&text);
            Ok(text)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_meter_keeps_an_even_pace_to_the_usual_time_then_slows_and_never_finishes_by_itself() {
        let board = Arc::new(Board::default());
        let ticket = board.start("key:practice-1:problem-1".into(), KEY).unwrap();
        assert!(
            board
                .start("key:practice-1:problem-1".into(), KEY)
                .is_none(),
            "one meter per job"
        );
        let m = &ticket.meter;
        assert!(m.fraction_at(0.0) < 0.01);
        let quarter = m.fraction_at(KEY.secs / 4.0);
        let half = m.fraction_at(KEY.secs / 2.0);
        assert!(
            (half - 2.0 * quarter).abs() < 0.01,
            "an even pace, not a curve that stalls early"
        );
        assert!((m.fraction_at(KEY.secs) - ON_PACE).abs() < 0.01);
        assert!(
            m.fraction_at(KEY.secs * 3.0) > ON_PACE && m.fraction_at(KEY.secs * 50.0) <= MOST_SHOWN
        );
        m.add(&"x".repeat(3_000));
        assert!(
            m.fraction_at(0.0) > 0.45,
            "streamed text moves it ahead of the clock"
        );
        m.add(&"x".repeat(30_000));
        assert_eq!(m.fraction_at(0.0), MOST_SHOWN);
        m.restart();
        assert!(m.fraction_at(0.0) < 0.01);
        let view = board.view("key:practice-1:problem-1").unwrap();
        assert_eq!(
            (view["expected"].as_u64(), view["slow"].as_bool()),
            (Some(45), Some(false))
        );
        drop(ticket);
        assert!(board.view("key:practice-1:problem-1").is_none());
        let failed = board.start("key:practice-1:problem-1".into(), KEY).unwrap();
        drop(failed);
        assert_eq!(
            board.estimate(KEY, None),
            (KEY.chars, KEY.secs),
            "a job that wrote nothing teaches nothing"
        );
    }

    #[test]
    fn the_usual_time_comes_from_recent_runs_and_survives_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let chats = dir.path().join("chats");
        std::fs::create_dir_all(&chats).unwrap();
        let board = Board::kept_in(&chats);
        for secs in [10.0, 12.0, 11.0, 30.0] {
            board.record(GRADE, secs, 800.0);
        }
        let (chars, secs) = board.estimate(GRADE, None);
        assert_eq!(
            (chars, secs),
            (800.0, 30.0),
            "the upper quarter of recent runs, so the end is rarely a jump"
        );
        for _ in 0..KEPT_RUNS {
            board.record(GRADE, 12.0, 800.0);
        }
        assert_eq!(board.estimate(GRADE, None).1, 12.0, "old runs fall away");
        let again = Board::kept_in(&chats);
        assert_eq!(again.estimate(GRADE, None).1, 12.0);
        assert_eq!(again.estimate(KEY, None), (KEY.chars, KEY.secs));
    }

    #[test]
    fn a_metered_writer_counts_streamed_text_and_progress_keys_are_checked() {
        let board = Arc::new(Board::default());
        let ticket = board.start("grade:attempt-1".into(), GRADE).unwrap();
        let plain: Writer = Arc::new(|_: &str, _: &str, _| Ok("y".repeat(750)));
        let streamer: Streamer = Arc::new(|_: &str, _: &str, _, piece, _| {
            piece(&"z".repeat(750));
            Ok("z".repeat(750).into())
        });
        let streamed = metered(
            Arc::clone(&plain),
            Some(streamer),
            Arc::clone(&ticket.meter),
        );
        assert_eq!(streamed("s", "u", 10).unwrap().len(), 750);
        assert!((ticket.meter.fraction_at(0.0) - 0.5).abs() < 0.05);
        let without = metered(plain, None, Arc::clone(&ticket.meter));
        without("s", "u", 10).unwrap();
        assert!(ticket.meter.fraction_at(0.0) > 0.9);
        assert!(valid_key("language:practice-1:problem-1"));
        assert!(valid_key("coverage:practice-1:problem-1"));
        assert!(!valid_key("other:x") && !valid_key("key:") && !valid_key("key:a/b"));
    }

    #[test]
    fn a_sized_job_expects_time_in_proportion_to_its_length() {
        let board = Arc::new(Board::default());
        let small = board
            .start_sized("combine:a".into(), COMBINE, Some(3_000.0))
            .unwrap();
        let large = board
            .start_sized("combine:b".into(), COMBINE, Some(24_000.0))
            .unwrap();
        assert_eq!(small.meter.view()["expected"], 30);
        assert_eq!(large.meter.view()["expected"], 240);
        assert!(valid_key("combine:a"));
    }
}
