use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::wav;
use crate::ai::live;

pub type TranscribeFn = Arc<dyn Fn(&Path) -> Result<String, String> + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Update {
    Done {
        index: u32,
        text: String,
    },
    Retrying {
        index: u32,
        attempt: u32,
        error: String,
        wait: Duration,
    },
    Failed {
        index: u32,
        error: String,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct Policy {
    pub first_wait: Duration,
    pub most_wait: Duration,
    pub attempts_after_stop: u32,
    pub idle: Duration,
    pub workers: usize,
    pub finish_limit: Duration,
}

impl Default for Policy {
    fn default() -> Policy {
        Policy {
            first_wait: Duration::from_secs(5),
            most_wait: Duration::from_secs(300),
            attempts_after_stop: 6,
            idle: Duration::from_millis(500),
            workers: 3,
            finish_limit: Duration::from_secs(120),
        }
    }
}

impl Policy {
    pub fn wait(&self, attempt: u32) -> Duration {
        let factor = 2u32.saturating_pow(attempt.saturating_sub(1).min(16));
        self.first_wait.saturating_mul(factor).min(self.most_wait)
    }
}

pub struct Transcriber {
    cancel: Arc<AtomicBool>,
    recording: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

pub fn rate_limited(error: &str) -> bool {
    let e = error.to_lowercase();
    e.contains("429")
        || e.contains("rate limit")
        || e.contains("rate-limit")
        || e.contains("too many requests")
}

pub fn waiting(dir: &Path) -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<u32> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let digits = name.strip_prefix("seg-")?.strip_suffix(".wav")?;
            if digits.contains('.') {
                return None;
            }
            let index: u32 = digits.parse().ok()?;
            let settled = dir.join(format!("seg-{index:05}.txt")).exists()
                || dir.join(format!("seg-{index:05}.err")).exists();
            (!settled).then_some(index)
        })
        .collect();
    out.sort();
    out
}

fn recording_now(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|entries| {
        entries
            .flatten()
            .any(|e| e.file_name().to_string_lossy().ends_with(".part.wav"))
    })
}

fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

fn text_path(dir: &Path, index: u32) -> PathBuf {
    dir.join(format!("seg-{index:05}.txt"))
}

impl Transcriber {
    pub fn start(
        dir: &Path,
        transcribe: TranscribeFn,
        policy: Policy,
        recording: bool,
        events: Sender<Update>,
    ) -> Transcriber {
        let cancel = Arc::new(AtomicBool::new(false));
        let live_flag = Arc::new(AtomicBool::new(recording));
        let thread = {
            let dir = dir.to_path_buf();
            let (cancel, live_flag) = (Arc::clone(&cancel), Arc::clone(&live_flag));
            std::thread::Builder::new()
                .name("leo-transcriber".into())
                .spawn(move || run(&dir, transcribe, policy, &cancel, &live_flag, &events))
                .ok()
        };
        Transcriber {
            cancel,
            recording: live_flag,
            thread,
        }
    }

    pub fn recording_ended(&self) {
        self.recording.store(false, Ordering::Relaxed);
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn is_finished(&self) -> bool {
        self.thread.as_ref().is_none_or(|t| t.is_finished())
    }

    pub fn wait(mut self) {
        self.recording_ended();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Transcriber {
    fn drop(&mut self) {
        if self.thread.is_some() {
            self.cancel();
        }
    }
}

#[derive(Default)]
struct Work {
    attempts: Mutex<HashMap<u32, (u32, Instant)>>,
    busy: Mutex<HashSet<u32>>,
    stop_seen: AtomicBool,
    stopped_at: Mutex<Option<Instant>>,
}

impl Work {
    fn claim(&self, dir: &Path) -> Option<u32> {
        let mut busy = self.busy.lock().ok()?;
        let attempts = self.attempts.lock().ok()?;
        let now = Instant::now();
        let index = waiting(dir)
            .into_iter()
            .find(|i| !busy.contains(i) && attempts.get(i).is_none_or(|(_, next)| now >= *next))?;
        busy.insert(index);
        Some(index)
    }

    fn release(&self, index: u32) {
        if let Ok(mut busy) = self.busy.lock() {
            busy.remove(&index);
        }
    }

    fn idle(&self) -> bool {
        self.busy.lock().is_ok_and(|b| b.is_empty())
    }
}

fn run(
    dir: &Path,
    transcribe: TranscribeFn,
    policy: Policy,
    cancel: &AtomicBool,
    recording: &AtomicBool,
    events: &Sender<Update>,
) {
    let work = Work::default();
    std::thread::scope(|scope| {
        for _ in 0..policy.workers.max(1) {
            scope.spawn(|| worker(dir, &transcribe, policy, cancel, recording, events, &work));
        }
    });
}

fn worker(
    dir: &Path,
    transcribe: &TranscribeFn,
    policy: Policy,
    cancel: &AtomicBool,
    recording: &AtomicBool,
    events: &Sender<Update>,
    work: &Work,
) {
    loop {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        let still_recording = recording.load(Ordering::Relaxed);
        if !still_recording && !work.stop_seen.swap(true, Ordering::Relaxed) {
            if let Ok(mut when) = work.stopped_at.lock() { *when = Some(Instant::now()); }
            if let Ok(mut attempts) = work.attempts.lock() {
                attempts.clear();
            }
        }
        let Some(index) = work.claim(dir) else {
            if waiting(dir).is_empty() && !still_recording && !recording_now(dir) && work.idle() {
                return;
            }
            std::thread::sleep(policy.idle);
            continue;
        };
        settle(dir, index, transcribe, policy, recording, events, work);
        work.release(index);
    }
}

fn settle(
    dir: &Path,
    index: u32,
    transcribe: &TranscribeFn,
    policy: Policy,
    recording: &AtomicBool,
    events: &Sender<Update>,
    work: &Work,
) {
    let path = dir.join(wav::done_name(index));
    let result = match wav::read(&path) {
        Ok(samples) if live::is_silent(wav::peak(&samples)) => Ok(String::new()),
        Ok(_) => transcribe(&path).map(|t| {
            if live::is_silence_artifact(&t) {
                String::new()
            } else {
                t.trim().to_string()
            }
        }),
        Err(e) => Err(e.to_string()),
    };
    let Ok(mut attempts) = work.attempts.lock() else {
        return;
    };
    match result {
        Ok(text) => {
            if let Err(e) = write_atomic(&text_path(dir, index), &text) {
                attempts.insert(index, (0, Instant::now() + policy.idle));
                let _ = events.send(Update::Retrying {
                    index,
                    attempt: 0,
                    error: format!("could not save the transcript: {e}"),
                    wait: policy.idle,
                });
                return;
            }
            let _ = std::fs::remove_file(&path);
            attempts.remove(&index);
            let _ = events.send(Update::Done { index, text });
        }
        Err(error) => {
            let attempt = attempts.get(&index).map_or(1, |(n, _)| n + 1);
            let finishing = !recording.load(Ordering::Relaxed);
            let expired = work.stopped_at.lock().ok().and_then(|v| *v).is_some_and(|at| at.elapsed() >= policy.finish_limit);
            if finishing && (expired || (attempt >= policy.attempts_after_stop && !rate_limited(&error))) {
                let _ = write_atomic(&dir.join(format!("seg-{index:05}.err")), &error);
                attempts.remove(&index);
                let _ = events.send(Update::Failed { index, error });
            } else {
                let mut wait = policy.wait(attempt);
                if finishing {
                    if let Some(at) = work.stopped_at.lock().ok().and_then(|v| *v) { wait = wait.min(policy.finish_limit.saturating_sub(at.elapsed())); }
                }
                attempts.insert(index, (attempt, Instant::now() + wait));
                let _ = events.send(Update::Retrying {
                    index,
                    attempt,
                    error,
                    wait,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;
    use std::sync::mpsc;

    fn quick() -> Policy {
        Policy {
            first_wait: Duration::from_millis(5),
            most_wait: Duration::from_millis(20),
            attempts_after_stop: 3,
            idle: Duration::from_millis(5),
            workers: 3,
            finish_limit: Duration::from_secs(120),
        }
    }

    fn loud(dir: &Path, index: u32) {
        wav::write(&dir.join(wav::done_name(index)), &vec![3000i16; 1600]).unwrap();
    }

    fn named(path: &Path) -> String {
        path.file_name().unwrap().to_string_lossy().to_string()
    }

    #[test]
    fn every_segment_gets_its_text_and_its_audio_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..5 {
            loud(dir.path(), i);
        }
        wav::write(&dir.path().join(wav::done_name(5)), &vec![0i16; 1600]).unwrap();
        let (tx, rx) = mpsc::channel();
        let t = Transcriber::start(
            dir.path(),
            Arc::new(|p: &Path| Ok(named(p))),
            quick(),
            false,
            tx,
        );
        t.wait();
        for i in 0..5 {
            let text = std::fs::read_to_string(dir.path().join(format!("seg-{i:05}.txt"))).unwrap();
            assert_eq!(text, format!("seg-{i:05}.wav"));
            assert!(!dir.path().join(wav::done_name(i)).exists());
        }
        assert_eq!(
            std::fs::read_to_string(dir.path().join("seg-00005.txt")).unwrap(),
            ""
        );
        assert_eq!(rx.try_iter().count(), 6);
    }

    #[test]
    fn a_flaky_provider_is_retried_until_it_answers() {
        let dir = tempfile::tempdir().unwrap();
        loud(dir.path(), 0);
        let calls = Arc::new(AtomicU32::new(0));
        let seen = Arc::clone(&calls);
        let (tx, rx) = mpsc::channel();
        let t = Transcriber::start(
            dir.path(),
            Arc::new(move |_: &Path| {
                if seen.fetch_add(1, Ordering::Relaxed) < 2 {
                    Err("429 rate limited".to_string())
                } else {
                    Ok("finally".to_string())
                }
            }),
            quick(),
            false,
            tx,
        );
        t.wait();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("seg-00000.txt")).unwrap(),
            "finally"
        );
        let updates: Vec<Update> = rx.try_iter().collect();
        assert!(matches!(updates.last(), Some(Update::Done { .. })));
        assert_eq!(
            updates
                .iter()
                .filter(|u| matches!(u, Update::Retrying { .. }))
                .count(),
            2
        );
    }

    #[test]
    fn a_segment_that_never_works_is_marked_and_keeps_its_audio() {
        let dir = tempfile::tempdir().unwrap();
        loud(dir.path(), 0);
        loud(dir.path(), 1);
        let (tx, _rx) = mpsc::channel();
        let t = Transcriber::start(
            dir.path(),
            Arc::new(|p: &Path| {
                if named(p).contains("00000") {
                    Err("offline".to_string())
                } else {
                    Ok("fine".to_string())
                }
            }),
            quick(),
            false,
            tx,
        );
        t.wait();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("seg-00000.err")).unwrap(),
            "offline"
        );
        assert!(dir.path().join("seg-00000.wav").exists());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("seg-00001.txt")).unwrap(),
            "fine"
        );
    }

    #[test]
    fn while_recording_it_waits_for_new_segments_and_never_gives_up() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("seg-00001.part.wav"), "recording").unwrap();
        loud(dir.path(), 0);
        let fails = Arc::new(AtomicU32::new(0));
        let seen = Arc::clone(&fails);
        let (tx, _rx) = mpsc::channel();
        let t = Transcriber::start(
            dir.path(),
            Arc::new(move |_: &Path| {
                if seen.fetch_add(1, Ordering::Relaxed) < 10 {
                    Err("503".to_string())
                } else {
                    Ok("late".to_string())
                }
            }),
            quick(),
            true,
            tx,
        );
        std::thread::sleep(Duration::from_millis(400));
        assert!(
            !dir.path().join("seg-00000.err").exists(),
            "gave up while still recording"
        );
        std::fs::remove_file(dir.path().join("seg-00001.part.wav")).unwrap();
        loud(dir.path(), 1);
        t.wait();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("seg-00000.txt")).unwrap(),
            "late"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("seg-00001.txt")).unwrap(),
            "late"
        );
    }

    #[test]
    fn a_rate_limit_is_waited_out_rather_than_given_up_on() {
        let dir = tempfile::tempdir().unwrap();
        loud(dir.path(), 0);
        let calls = Arc::new(AtomicU32::new(0));
        let seen = Arc::clone(&calls);
        let (tx, _rx) = mpsc::channel();
        let t = Transcriber::start(
            dir.path(),
            Arc::new(move |_: &Path| {
                if seen.fetch_add(1, Ordering::Relaxed) < 12 {
                    Err("groq: 429 Too Many Requests".to_string())
                } else {
                    Ok("patient".to_string())
                }
            }),
            quick(),
            false,
            tx,
        );
        t.wait();
        assert!(calls.load(Ordering::Relaxed) > quick().attempts_after_stop);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("seg-00000.txt")).unwrap(),
            "patient"
        );
        assert!(rate_limited("429"));
        assert!(rate_limited("Rate limit reached for model"));
        assert!(!rate_limited("401 unauthorized"));
    }

    #[test]
    fn several_segments_are_transcribed_at_once_and_stay_in_order() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..12 {
            loud(dir.path(), i);
        }
        let running = Arc::new(AtomicU32::new(0));
        let most = Arc::new(AtomicU32::new(0));
        let (r, m) = (Arc::clone(&running), Arc::clone(&most));
        let (tx, _rx) = mpsc::channel();
        let t = Transcriber::start(
            dir.path(),
            Arc::new(move |p: &Path| {
                let now = r.fetch_add(1, Ordering::SeqCst) + 1;
                m.fetch_max(now, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(60));
                r.fetch_sub(1, Ordering::SeqCst);
                Ok(named(p))
            }),
            Policy {
                workers: 4,
                ..quick()
            },
            false,
            tx,
        );
        t.wait();
        let most = most.load(Ordering::SeqCst);
        assert!(most >= 2, "segments were not worked on in parallel");
        assert!(most <= 4, "more workers ran than asked for");
        for i in 0..12 {
            let text = std::fs::read_to_string(dir.path().join(format!("seg-{i:05}.txt"))).unwrap();
            assert_eq!(text, format!("seg-{i:05}.wav"));
        }
    }

    #[test]
    fn waits_grow_and_are_capped() {
        let p = Policy::default();
        assert_eq!(p.wait(1), Duration::from_secs(5));
        assert_eq!(p.wait(2), Duration::from_secs(10));
        assert_eq!(p.wait(4), Duration::from_secs(40));
        assert_eq!(p.wait(30), Duration::from_secs(300));
    }
}
