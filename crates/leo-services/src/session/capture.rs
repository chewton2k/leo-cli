use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{bail, Context, Result};

use super::mic::Mic;
use super::wav::{Writer, RATE};
use super::OVERLAP_SECS;

const TAIL_SECS: u64 = 90;
const BLOCK: usize = RATE as usize / 10;

#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    Microphone,
    Screen,
    Replay { path: PathBuf, speed: f64 },
    Synthetic { secs: u64, speed: f64 },
}

impl Source {
    pub fn from_env(screen: bool) -> Source {
        let speed = std::env::var("LEO_FAKE_SPEED")
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|s| *s > 0.0)
            .unwrap_or(1.0);
        match std::env::var("LEO_FAKE_AUDIO") {
            Ok(path) => Source::Replay {
                path: PathBuf::from(path),
                speed,
            },
            Err(_) if screen => Source::Screen,
            Err(_) => Source::Microphone,
        }
    }
}

#[derive(Default)]
pub struct Shared {
    recorded: AtomicU64,
    tail: Mutex<VecDeque<i16>>,
    ended: AtomicBool,
    problem: Mutex<Option<String>>,
}

pub struct Capture {
    stop: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
    shared: Arc<Shared>,
    mic: Option<Mic>,
    thread: Option<JoinHandle<()>>,
}

fn replay_samples(path: &Path) -> Result<Vec<i16>> {
    let bytes =
        std::fs::read(path).with_context(|| format!("could not read {}", path.display()))?;
    let samples = crate::ai::provider::audio::read_wav(&bytes)
        .map_err(|e| anyhow::anyhow!("could not read {}: {e}", path.display()))?;
    Ok(samples
        .iter()
        .map(|x| (x.clamp(-1.0, 1.0) * 32767.0).round() as i16)
        .collect())
}

pub fn synthetic(second: u64) -> Vec<i16> {
    let silent = (second / 60) % 7 == 3;
    (0..RATE as u64)
        .map(|i| {
            if silent {
                0
            } else if (i / 20) % 2 == 0 {
                3000
            } else {
                -3000
            }
        })
        .collect()
}

enum Feed {
    Device {
        rx: Receiver<Vec<i16>>,
        pending: VecDeque<i16>,
        problem: Arc<Mutex<Option<String>>>,
    },
    Samples {
        samples: Vec<i16>,
        at: usize,
        speed: f64,
    },
    Synthetic {
        secs: u64,
        second: u64,
        speed: f64,
    },
}

impl Feed {
    fn next(&mut self, stop: &AtomicBool, buf: &mut Vec<i16>) -> std::io::Result<bool> {
        buf.clear();
        match self {
            Feed::Device {
                rx,
                pending,
                problem,
            } => {
                while pending.len() < BLOCK {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    if let Some(p) = problem.lock().ok().and_then(|p| p.clone()) {
                        return Err(std::io::Error::other(p));
                    }
                    match rx.recv_timeout(Duration::from_millis(200)) {
                        Ok(chunk) => pending.extend(chunk),
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
                let take = pending.len().min(BLOCK);
                buf.extend(pending.drain(..take));
                Ok(take > 0)
            }
            Feed::Samples { samples, at, speed } => {
                if stop.load(Ordering::Relaxed) || *at >= samples.len() {
                    return Ok(false);
                }
                let step = ((BLOCK as f64 * *speed) as usize).clamp(BLOCK, RATE as usize * 60);
                let end = (*at + step).min(samples.len());
                buf.extend_from_slice(&samples[*at..end]);
                *at = end;
                std::thread::sleep(Duration::from_millis(100));
                Ok(true)
            }
            Feed::Synthetic {
                secs,
                second,
                speed,
            } => {
                if stop.load(Ordering::Relaxed) || *second >= *secs {
                    return Ok(false);
                }
                let per_tick = (*speed / 10.0).max(0.1);
                let seconds = (per_tick.ceil() as u64).clamp(1, 600);
                for _ in 0..seconds {
                    if *second >= *secs {
                        break;
                    }
                    buf.extend(synthetic(*second));
                    *second += 1;
                }
                let pause = Duration::from_secs_f64(seconds as f64 / *speed);
                std::thread::sleep(pause.min(Duration::from_millis(100)));
                Ok(true)
            }
        }
    }
}

impl Capture {
    pub fn start(
        dir: &Path,
        first_index: u32,
        segment_secs: u64,
        source: Source,
    ) -> Result<Capture> {
        let mut mic = None;
        let feed = match source {
            Source::Microphone | Source::Screen => {
                let (opened, rx) = Mic::open(source == Source::Screen)?;
                let problem = Arc::clone(&opened.problem);
                mic = Some(opened);
                Feed::Device {
                    rx,
                    pending: VecDeque::new(),
                    problem,
                }
            }
            Source::Replay { path, speed } => Feed::Samples {
                samples: replay_samples(&path)?,
                at: 0,
                speed,
            },
            Source::Synthetic { secs, speed } => Feed::Synthetic {
                secs,
                second: 0,
                speed,
            },
        };
        let writer = Writer::open(
            dir,
            first_index,
            segment_secs * RATE as u64,
            (OVERLAP_SECS * RATE as u64) as usize,
        )?;
        let stop = Arc::new(AtomicBool::new(false));
        let pause = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Shared::default());
        let thread = {
            let (stop, pause, shared) =
                (Arc::clone(&stop), Arc::clone(&pause), Arc::clone(&shared));
            std::thread::Builder::new()
                .name("leo-capture".into())
                .spawn(move || run(feed, writer, &stop, &pause, &shared))?
        };
        Ok(Capture {
            stop,
            pause,
            shared,
            mic,
            thread: Some(thread),
        })
    }

    pub fn recorded_samples(&self) -> u64 {
        self.shared.recorded.load(Ordering::Relaxed)
    }

    pub fn recorded_secs(&self) -> f64 {
        self.recorded_samples() as f64 / RATE as f64
    }

    pub fn set_paused(&self, paused: bool) {
        self.pause.store(paused, Ordering::Relaxed);
    }

    pub fn paused(&self) -> bool {
        self.pause.load(Ordering::Relaxed)
    }

    pub fn ended(&self) -> bool {
        self.shared.ended.load(Ordering::Relaxed)
    }

    pub fn problem(&self) -> Option<String> {
        self.shared.problem.lock().ok().and_then(|p| p.clone())
    }

    pub fn tail(&self, from: u64, most: usize) -> (u64, Vec<i16>) {
        let Ok(tail) = self.shared.tail.lock() else {
            return (from, Vec::new());
        };
        let end = self.recorded_samples();
        let first = end.saturating_sub(tail.len() as u64);
        let start = from.clamp(first, end).max(end.saturating_sub(most as u64));
        let skip = (start - first) as usize;
        (start, tail.iter().skip(skip).copied().collect())
    }

    pub fn stop(mut self) -> Result<()> {
        self.finish()
    }

    fn finish(&mut self) -> Result<()> {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(mic) = self.mic.as_mut() {
            mic.close();
        }
        if let Some(thread) = self.thread.take() {
            thread
                .join()
                .map_err(|_| anyhow::anyhow!("the recording thread crashed"))?;
        }
        match self.problem() {
            Some(p) if self.recorded_samples() == 0 => bail!(p),
            _ => Ok(()),
        }
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

fn run(mut feed: Feed, mut writer: Writer, stop: &AtomicBool, pause: &AtomicBool, shared: &Shared) {
    let mut buf = Vec::with_capacity(BLOCK);
    let limit = (TAIL_SECS * RATE as u64) as usize;
    let problem = loop {
        match feed.next(stop, &mut buf) {
            Ok(true) => {}
            Ok(false) => {
                break (!stop.load(Ordering::Relaxed))
                    .then(|| "The recorder stopped on its own.".to_string());
            }
            Err(e) => break Some(format!("Reading the audio failed: {e}")),
        }
        if pause.load(Ordering::Relaxed) || buf.is_empty() {
            continue;
        }
        if let Err(e) = writer.push(&buf) {
            break Some(format!(
                "Could not save the recording ({e}). Is the disk full?"
            ));
        }
        shared
            .recorded
            .fetch_add(buf.len() as u64, Ordering::Relaxed);
        if let Ok(mut tail) = shared.tail.lock() {
            tail.extend(buf.iter().copied());
            let extra = tail.len().saturating_sub(limit);
            tail.drain(..extra);
        }
    };
    let _ = writer.finish(RATE as u64 / 2);
    if let Some(p) = problem {
        if let Ok(mut slot) = shared.problem.lock() {
            *slot = Some(p);
        }
    }
    shared.ended.store(true, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::wav;

    fn wait_for(capture: &Capture, samples: u64) {
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while capture.recorded_samples() < samples && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn a_fast_synthetic_hour_becomes_twelve_segments() {
        let dir = tempfile::tempdir().unwrap();
        let capture = Capture::start(
            dir.path(),
            0,
            300,
            Source::Synthetic {
                secs: 3600,
                speed: 36_000.0,
            },
        )
        .unwrap();
        wait_for(&capture, 3600 * RATE as u64);
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while !capture.ended() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(capture.ended());
        assert_eq!(
            capture.problem().as_deref(),
            Some("The recorder stopped on its own.")
        );
        capture.stop().unwrap();
        let mut names: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        names.sort();
        assert_eq!(names.len(), 12, "{names:?}");
        assert!(names
            .iter()
            .all(|n| n.ends_with(".wav") && !n.contains("part")));
        let second = wav::read(&dir.path().join("seg-00001.wav")).unwrap();
        assert_eq!(second.len() as u64, (300 + OVERLAP_SECS) * RATE as u64);
    }

    #[test]
    fn paused_audio_is_never_written() {
        let dir = tempfile::tempdir().unwrap();
        let capture = Capture::start(
            dir.path(),
            0,
            300,
            Source::Synthetic {
                secs: 100_000,
                speed: 50.0,
            },
        )
        .unwrap();
        wait_for(&capture, RATE as u64);
        capture.set_paused(true);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut at_pause = capture.recorded_samples();
        loop {
            std::thread::sleep(Duration::from_millis(250));
            let now = capture.recorded_samples();
            if now == at_pause || std::time::Instant::now() > deadline {
                break;
            }
            at_pause = now;
        }
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(capture.recorded_samples(), at_pause);
        capture.set_paused(false);
        wait_for(&capture, at_pause + RATE as u64);
        capture.stop().unwrap();
        let written = wav::read(&dir.path().join("seg-00000.wav")).unwrap().len() as u64;
        assert!(written >= at_pause + RATE as u64);
        assert!(written < at_pause + 120 * RATE as u64);
    }

    #[test]
    fn the_tail_gives_the_latest_audio_from_a_point() {
        let dir = tempfile::tempdir().unwrap();
        let capture = Capture::start(
            dir.path(),
            0,
            300,
            Source::Synthetic {
                secs: 5,
                speed: 1000.0,
            },
        )
        .unwrap();
        wait_for(&capture, 5 * RATE as u64);
        let (start, samples) = capture.tail(2 * RATE as u64, 10 * RATE as usize);
        assert_eq!(start, 2 * RATE as u64);
        assert_eq!(samples.len(), 3 * RATE as usize);
        let (start, samples) = capture.tail(0, RATE as usize);
        assert_eq!(start, 4 * RATE as u64);
        assert_eq!(samples.len(), RATE as usize);
        capture.stop().unwrap();
    }
}
