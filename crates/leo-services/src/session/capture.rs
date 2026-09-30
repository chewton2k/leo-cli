use std::collections::VecDeque;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{bail, Context, Result};

use super::wav::{self, Writer, RATE};
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
    child: Option<Child>,
    thread: Option<JoinHandle<()>>,
}

fn require_sox() -> Result<()> {
    let found = Command::new("rec")
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok();
    if !found {
        bail!(
            "Audio recording requires SoX. Install it:\n  \
             macOS:   brew install sox\n  \
             Linux:   sudo apt install sox\n  \
             Windows: choco install sox"
        );
    }
    Ok(())
}

fn spawn_rec(screen: bool) -> Result<Child> {
    require_sox()?;
    let device = screen.then(|| {
        std::env::var("LEO_SCREEN_DEVICE").unwrap_or_else(|_| "BlackHole 2ch".to_string())
    });
    let mut cmd = Command::new("rec");
    if let Some(dev) = &device {
        cmd.env("AUDIODEV", dev);
    }
    cmd.args([
        "-q",
        "-r",
        "16000",
        "-c",
        "1",
        "-b",
        "16",
        "-e",
        "signed-integer",
        "-t",
        "raw",
        "-",
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .spawn()
    .with_context(|| match &device {
        Some(dev) => format!(
            "Failed to start recording from screen audio device '{dev}'.\n  \
             Set up system audio capture:\n  \
             1. brew install blackhole-2ch\n  \
             2. Audio MIDI Setup → New Multi-Output Device (Speakers + BlackHole 2ch)\n  \
             3. Set that Multi-Output Device as System Output\n  \
             To use a different device: set LEO_SCREEN_DEVICE=<name>"
        ),
        None => "Failed to start recording".to_string(),
    })
}

fn replay_samples(path: &Path) -> Result<Vec<i16>> {
    let tmp = std::env::temp_dir().join(format!("leo-replay-{}.wav", std::process::id()));
    let ok = Command::new("sox")
        .arg(path)
        .args(["-r", "16000", "-c", "1", "-b", "16", "-e", "signed-integer"])
        .arg(&tmp)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    let samples = if ok { wav::read(&tmp) } else { wav::read(path) };
    let _ = std::fs::remove_file(&tmp);
    samples.with_context(|| format!("could not read {}", path.display()))
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
    Pipe(std::process::ChildStdout),
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
            Feed::Pipe(out) => {
                let mut bytes = [0u8; BLOCK * 2];
                let mut filled = 0;
                while filled < bytes.len() {
                    let n = out.read(&mut bytes[filled..])?;
                    if n == 0 {
                        break;
                    }
                    filled += n;
                }
                let even = filled & !1;
                buf.extend(
                    bytes[..even]
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|b| i16::from_le_bytes(*b)),
                );
                Ok(filled > 0)
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
        let mut child = None;
        let feed = match source {
            Source::Microphone | Source::Screen => {
                let mut c = spawn_rec(source == Source::Screen)?;
                let out = c
                    .stdout
                    .take()
                    .context("the recorder gave no audio stream")?;
                child = Some(c);
                Feed::Pipe(out)
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
            child,
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
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
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
        std::thread::sleep(Duration::from_millis(50));
        let at_pause = capture.recorded_samples();
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(capture.recorded_samples(), at_pause);
        capture.set_paused(false);
        wait_for(&capture, at_pause + RATE as u64);
        capture.stop().unwrap();
        let written = wav::read(&dir.path().join("seg-00000.wav")).unwrap().len() as u64;
        assert!(written >= at_pause + RATE as u64);
        assert!(written < at_pause + 40 * RATE as u64);
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
