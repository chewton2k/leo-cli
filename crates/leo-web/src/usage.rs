use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;

use crate::chat::Spent;

const LATENCIES: usize = 64;

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
}

#[derive(Clone, Default, Serialize)]
pub struct Tools {
    calls: u64,
    failures: u64,
    reused: u64,
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

impl Usage {
    pub(crate) fn tool(&self, failed: bool, reused: bool) {
        let mut held = self.tools.lock().unwrap_or_else(|e| e.into_inner());
        held.calls = held.calls.saturating_add(1);
        held.failures = held.failures.saturating_add(u64::from(failed));
        held.reused = held.reused.saturating_add(u64::from(reused));
    }

    pub fn tools(&self) -> Tools {
        self.tools.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub(crate) fn counted(
        self: &std::sync::Arc<Self>,
        writer: crate::graph::Writer,
        purpose: Purpose,
    ) -> crate::graph::Writer {
        let usage = std::sync::Arc::clone(self);
        std::sync::Arc::new(move |system, user, most| {
            let started = std::time::Instant::now();
            let reply = writer(system, user, most);
            usage.record(purpose, None, started.elapsed(), reply.is_err());
            reply
        })
    }

    pub fn record(&self, purpose: Purpose, spent: Option<&Spent>, elapsed: Duration, failed: bool) {
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

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn quick_model_calls_are_counted_as_quick_review() {
        let usage = std::sync::Arc::new(Usage::default());
        let fine: crate::graph::Writer = std::sync::Arc::new(|_, _, _| Ok("ok".into()));
        let broken: crate::graph::Writer = std::sync::Arc::new(|_, _, _| anyhow::bail!("down"));
        let fine = usage.counted(fine, Purpose::QuickReview);
        let broken = usage.counted(broken, Purpose::QuickReview);
        assert_eq!(fine("s", "u", 10).unwrap(), "ok");
        assert!(broken("s", "u", 10).is_err());
        let held = usage.rows.lock().unwrap();
        let row = &held[&Purpose::QuickReview];
        assert_eq!((row.calls, row.failures), (2, 1));
    }
}
