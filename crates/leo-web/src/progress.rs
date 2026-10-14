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
pub const COVERAGE: Kind = Kind {
    name: "coverage",
    chars: 500.0,
    secs: 5.0,
};

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

    pub fn fraction(&self) -> f64 {
        let written = self.got.load(Ordering::Relaxed) as f64 / self.chars.max(1.0);
        let waited = 1.0 - (-self.started.elapsed().as_secs_f64() / self.secs.max(0.5)).exp();
        written.max(waited * 0.9).clamp(0.0, MOST_SHOWN)
    }

    pub fn view(&self) -> serde_json::Value {
        serde_json::json!({
            "fraction": (self.fraction() * 1000.0).round() / 1000.0,
            "seconds": self.started.elapsed().as_secs(),
            "expected": self.secs.round() as u64,
        })
    }
}

#[derive(Default)]
pub struct Board {
    meters: Mutex<HashMap<String, Arc<Meter>>>,
    learned: Mutex<HashMap<&'static str, (f64, f64)>>,
    next: AtomicUsize,
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
    pub fn start(self: &Arc<Self>, key: String, kind: Kind) -> Option<Ticket> {
        let (chars, secs) = self
            .learned
            .lock()
            .ok()
            .and_then(|l| l.get(kind.name).copied())
            .unwrap_or((kind.chars, kind.secs));
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
        let secs = meter.started.elapsed().as_secs_f64();
        let chars = meter.got.load(Ordering::Relaxed) as f64;
        if let Ok(mut learned) = self.learned.lock() {
            let (old_chars, old_secs) = learned
                .get(meter.kind.name)
                .copied()
                .unwrap_or((meter.kind.chars, meter.kind.secs));
            let chars = if chars > 0.0 {
                old_chars * 0.6 + chars * 0.4
            } else {
                old_chars
            };
            learned.insert(meter.kind.name, (chars, old_secs * 0.6 + secs * 0.4));
        }
    }

    pub fn view(&self, key: &str) -> Option<serde_json::Value> {
        self.meters.lock().ok()?.get(key).map(|m| m.view())
    }
}

pub fn valid_key(key: &str) -> bool {
    key.len() <= 200
        && key.split_once(':').is_some_and(|(kind, rest)| {
            [KEY.name, GRADE.name, COVERAGE.name].contains(&kind)
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
    fn a_meter_moves_with_text_and_time_never_reaches_the_end_and_learns_how_long_jobs_take() {
        let board = Arc::new(Board::default());
        let ticket = board.start("key:practice-1:problem-1".into(), KEY).unwrap();
        assert!(
            board
                .start("key:practice-1:problem-1".into(), KEY)
                .is_none(),
            "one meter per job"
        );
        assert!(ticket.meter.fraction() < 0.05);
        ticket.meter.add(&"x".repeat(3_000));
        assert!((ticket.meter.fraction() - 0.5).abs() < 0.05);
        ticket.meter.add(&"x".repeat(30_000));
        assert_eq!(ticket.meter.fraction(), MOST_SHOWN);
        ticket.meter.restart();
        assert!(ticket.meter.fraction() < 0.05);
        let view = board.view("key:practice-1:problem-1").unwrap();
        assert_eq!(view["expected"], 45);
        ticket.meter.add(&"x".repeat(2_000));
        drop(ticket);
        assert!(board.view("key:practice-1:problem-1").is_none());
        let again = board.start("key:practice-1:problem-1".into(), KEY).unwrap();
        assert!(again.meter.chars < KEY.chars && again.meter.secs < KEY.secs);
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
        assert!((ticket.meter.fraction() - 0.5).abs() < 0.05);
        let without = metered(plain, None, Arc::clone(&ticket.meter));
        without("s", "u", 10).unwrap();
        assert!(ticket.meter.fraction() > 0.9);
        assert!(valid_key("coverage:practice-1:problem-1"));
        assert!(!valid_key("other:x") && !valid_key("key:") && !valid_key("key:a/b"));
    }
}
