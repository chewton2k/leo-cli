pub mod capture;
pub mod mic;
#[cfg(test)]
mod stress;
pub mod transcriber;
pub mod wav;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ai::chat::{clock, Jotted};

pub const SEGMENT_SECS: u64 = 300;
pub const OVERLAP_SECS: u64 = 1;
const LOCK_FRESH: Duration = Duration::from_secs(30);
const HEARTBEAT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Point {
    pub at_secs: u64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub started: DateTime<Utc>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub append_to: Option<String>,
    #[serde(default)]
    pub dir: String,
    #[serde(default)]
    pub screen: bool,
    pub segment_secs: u64,
    #[serde(default)]
    pub points: Vec<Point>,
    #[serde(default)]
    pub stopped: bool,
    #[serde(default)]
    pub saved: bool,
}

impl Manifest {
    pub fn new(
        title: Option<String>,
        append_to: Option<String>,
        dir: &str,
        screen: bool,
    ) -> Manifest {
        Manifest {
            started: Utc::now(),
            title,
            append_to,
            dir: dir.to_string(),
            screen,
            segment_secs: SEGMENT_SECS,
            points: Vec::new(),
            stopped: false,
            saved: false,
        }
    }

    pub fn jotted(&self) -> Vec<Jotted> {
        self.points
            .iter()
            .map(|p| Jotted {
                at_secs: p.at_secs,
                text: p.text.clone(),
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SegmentState {
    Recording,
    Waiting,
    Done(String),
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub index: u32,
    pub state: SegmentState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    pub index: u32,
    pub start_secs: u64,
    pub end_secs: u64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Assembled {
    pub parts: Vec<Part>,
    pub failed: Vec<(u64, u64)>,
    pub pending: usize,
}

impl Assembled {
    pub fn text(&self) -> String {
        let mut out = String::new();
        for part in &self.parts {
            out = crate::ai::live::stitch(&out, &part.text);
        }
        out
    }

    pub fn is_silent(&self) -> bool {
        self.failed.is_empty() && self.parts.iter().all(|p| p.text.trim().is_empty())
    }
}

pub fn assemble(segments: &[Segment], segment_secs: u64) -> Assembled {
    let mut out = Assembled::default();
    for seg in segments {
        let start = seg.index as u64 * segment_secs;
        let end = start + segment_secs;
        match &seg.state {
            SegmentState::Done(text) => out.parts.push(Part {
                index: seg.index,
                start_secs: start,
                end_secs: end,
                text: text.clone(),
            }),
            SegmentState::Failed(_) => out.failed.push((start, end)),
            SegmentState::Recording | SegmentState::Waiting => out.pending += 1,
        }
    }
    out
}

pub fn failure_notice(failed: &[(u64, u64)], dir: &Path) -> String {
    if failed.is_empty() {
        return String::new();
    }
    let spans: Vec<String> = failed
        .iter()
        .map(|(a, b)| format!("{}–{}", clock(*a), clock(*b)))
        .collect();
    format!(
        "> Parts of this recording could not be transcribed ({}), even after retrying. \
         Their audio is kept in `{}`.",
        spans.join(", "),
        dir.display()
    )
}

pub struct Session {
    pub dir: PathBuf,
    pub manifest: Manifest,
}

pub struct Lock {
    path: PathBuf,
    alive: Arc<AtomicBool>,
}

impl Drop for Lock {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Relaxed);
        let _ = std::fs::remove_file(&self.path);
    }
}

fn index_of(name: &str) -> Option<(u32, &str)> {
    let rest = name.strip_prefix("seg-")?;
    let (digits, ext) = rest.split_once('.')?;
    Some((digits.parse().ok()?, ext))
}

pub fn root() -> Result<PathBuf> {
    Ok(leo_core::paths::data_dir()?.join("recordings"))
}

fn write_atomic(path: &Path, text: &str) -> Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn process_alive(pid: u32) -> bool {
    if pid == std::process::id() {
        return true;
    }
    if cfg!(unix) {
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    } else if cfg!(windows) {
        std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).contains(&format!("\"{pid}\"")))
            .unwrap_or(true)
    } else {
        true
    }
}

pub fn locked(dir: &Path) -> bool {
    let path = dir.join("lock");
    let fresh = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|age| age < LOCK_FRESH);
    if !fresh {
        return false;
    }
    match std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| t.trim().parse::<u32>().ok())
    {
        Some(pid) => process_alive(pid),
        None => true,
    }
}

impl Session {
    pub fn create(root: &Path, manifest: Manifest) -> Result<Session> {
        std::fs::create_dir_all(root)?;
        let stamp = manifest.started.format("%Y%m%d-%H%M%S").to_string();
        let mut dir = root.join(&stamp);
        let mut n = 1;
        while dir.exists() {
            n += 1;
            dir = root.join(format!("{stamp}-{n}"));
        }
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("could not create {}", dir.display()))?;
        let session = Session { dir, manifest };
        session.save()?;
        Ok(session)
    }

    pub fn open(dir: &Path) -> Result<Session> {
        let text = std::fs::read_to_string(dir.join("session.json"))
            .with_context(|| format!("no recording in {}", dir.display()))?;
        let manifest = serde_json::from_str(&text)
            .with_context(|| format!("unreadable recording in {}", dir.display()))?;
        Ok(Session {
            dir: dir.to_path_buf(),
            manifest,
        })
    }

    pub fn save(&self) -> Result<()> {
        write_atomic(
            &self.dir.join("session.json"),
            &serde_json::to_string_pretty(&self.manifest)?,
        )
    }

    pub fn lock(&self) -> Result<Lock> {
        let path = self.dir.join("lock");
        std::fs::write(&path, std::process::id().to_string())?;
        let alive = Arc::new(AtomicBool::new(true));
        let beat = Arc::clone(&alive);
        let target = path.clone();
        std::thread::spawn(move || {
            let mut waited = Duration::ZERO;
            while beat.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(250));
                waited += Duration::from_millis(250);
                if waited >= HEARTBEAT {
                    waited = Duration::ZERO;
                    if let Ok(file) = std::fs::OpenOptions::new().write(true).open(&target) {
                        let _ = file.set_modified(SystemTime::now());
                    }
                }
            }
        });
        Ok(Lock { path, alive })
    }

    pub fn unfinished(root: &Path) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(root) else {
            return Vec::new();
        };
        let mut out: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|dir| !locked(dir))
            .filter(|dir| Session::open(dir).is_ok_and(|s| !s.manifest.saved))
            .collect();
        out.sort();
        out
    }

    pub fn segments(&self) -> Vec<Segment> {
        let mut found: std::collections::BTreeMap<u32, SegmentState> = Default::default();
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        names.sort();
        for name in names {
            let Some((index, ext)) = index_of(&name) else {
                continue;
            };
            let state = match ext {
                "txt" => std::fs::read_to_string(self.dir.join(&name))
                    .ok()
                    .map(SegmentState::Done),
                "err" => Some(SegmentState::Failed(
                    std::fs::read_to_string(self.dir.join(&name)).unwrap_or_default(),
                )),
                "wav" => Some(SegmentState::Waiting),
                "part.wav" => Some(SegmentState::Recording),
                _ => None,
            };
            let Some(state) = state else { continue };
            let entry = found.entry(index).or_insert(state.clone());
            let rank = |s: &SegmentState| match s {
                SegmentState::Done(_) => 3,
                SegmentState::Failed(_) => 2,
                SegmentState::Waiting => 1,
                SegmentState::Recording => 0,
            };
            if rank(&state) > rank(entry) {
                *entry = state;
            }
        }
        found
            .into_iter()
            .map(|(index, state)| Segment { index, state })
            .collect()
    }

    pub fn assemble(&self) -> Assembled {
        assemble(&self.segments(), self.manifest.segment_secs)
    }

    pub fn recover_parts(&self) -> Result<()> {
        for seg in self.segments() {
            if seg.state == SegmentState::Recording {
                let part = self.dir.join(wav::part_name(seg.index));
                wav::seal(&part)?;
                std::fs::rename(&part, self.dir.join(wav::done_name(seg.index)))?;
            }
        }
        for entry in std::fs::read_dir(&self.dir)?.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".err") {
                let wav = self.dir.join(name.replace(".err", ".wav"));
                if wav.exists() {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        Ok(())
    }

    pub fn next_index(&self) -> u32 {
        self.segments().last().map_or(0, |s| s.index + 1)
    }

    pub fn finish(mut self) -> Result<()> {
        let keep_audio = !self.assemble().failed.is_empty();
        if keep_audio {
            self.manifest.saved = true;
            self.save()
        } else {
            std::fs::remove_dir_all(&self.dir)
                .with_context(|| format!("could not remove {}", self.dir.display()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> (Session, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let s = Session::create(tmp.path(), Manifest::new(None, None, "", false)).unwrap();
        (s, tmp)
    }

    #[test]
    fn a_session_survives_being_reopened() {
        let (mut s, _t) = session();
        s.manifest.points.push(Point {
            at_secs: 61,
            text: "exam".into(),
        });
        s.save().unwrap();
        let again = Session::open(&s.dir).unwrap();
        assert_eq!(again.manifest, s.manifest);
    }

    #[test]
    fn segment_states_come_from_the_files_and_text_wins() {
        let (s, _t) = session();
        std::fs::write(s.dir.join("seg-00000.txt"), "hello").unwrap();
        std::fs::write(s.dir.join("seg-00001.wav"), "x").unwrap();
        std::fs::write(s.dir.join("seg-00001.err"), "429").unwrap();
        std::fs::write(s.dir.join("seg-00002.wav"), "x").unwrap();
        std::fs::write(s.dir.join("seg-00003.part.wav"), "x").unwrap();
        std::fs::write(s.dir.join("seg-00004.txt"), "").unwrap();
        std::fs::write(s.dir.join("seg-00004.wav"), "x").unwrap();
        let states: Vec<SegmentState> = s.segments().into_iter().map(|s| s.state).collect();
        assert_eq!(
            states,
            vec![
                SegmentState::Done("hello".into()),
                SegmentState::Failed("429".into()),
                SegmentState::Waiting,
                SegmentState::Recording,
                SegmentState::Done(String::new()),
            ]
        );
        assert_eq!(s.next_index(), 5);
    }

    #[test]
    fn assembly_keeps_order_marks_failures_and_joins_overlaps() {
        let segments = vec![
            Segment {
                index: 0,
                state: SegmentState::Done("the cat sat on the mat".into()),
            },
            Segment {
                index: 1,
                state: SegmentState::Done("on the mat and slept".into()),
            },
            Segment {
                index: 2,
                state: SegmentState::Failed("timeout".into()),
            },
            Segment {
                index: 3,
                state: SegmentState::Waiting,
            },
        ];
        let a = assemble(&segments, 300);
        assert_eq!(a.text(), "the cat sat on the mat and slept");
        assert_eq!(a.failed, vec![(600, 900)]);
        assert_eq!(a.pending, 1);
        assert_eq!(a.parts[1].start_secs, 300);
        let notice = failure_notice(&a.failed, Path::new("/x"));
        assert!(notice.contains("10:00–15:00"), "{notice}");
    }

    #[test]
    fn crash_leftovers_are_sealed_and_retried() {
        let (s, _t) = session();
        wav::write(&s.dir.join("seg-00000.part.wav"), &[1, 2, 3]).unwrap();
        std::fs::write(s.dir.join("seg-00001.wav"), "x").unwrap();
        std::fs::write(s.dir.join("seg-00001.err"), "gave up").unwrap();
        s.recover_parts().unwrap();
        let states: Vec<SegmentState> = s.segments().into_iter().map(|s| s.state).collect();
        assert_eq!(states, vec![SegmentState::Waiting, SegmentState::Waiting]);
    }

    #[test]
    fn a_lock_left_by_a_process_that_died_does_not_count() {
        let (s, _t) = session();
        std::fs::write(s.dir.join("lock"), "999999").unwrap();
        assert!(!locked(&s.dir), "a dead process still holds the recording");
        std::fs::write(s.dir.join("lock"), std::process::id().to_string()).unwrap();
        assert!(locked(&s.dir));
        std::fs::write(s.dir.join("lock"), "not a pid").unwrap();
        assert!(
            locked(&s.dir),
            "an unreadable lock is treated as held while it is fresh"
        );
    }

    #[test]
    fn unfinished_sessions_are_found_unless_saved_or_in_use() {
        let tmp = tempfile::tempdir().unwrap();
        let a = Session::create(tmp.path(), Manifest::new(None, None, "", false)).unwrap();
        let mut b = Session::create(tmp.path(), Manifest::new(None, None, "", false)).unwrap();
        b.manifest.saved = true;
        b.save().unwrap();
        let c = Session::create(tmp.path(), Manifest::new(None, None, "", false)).unwrap();
        let _held = c.lock().unwrap();
        assert_eq!(Session::unfinished(tmp.path()), vec![a.dir.clone()]);
    }

    #[test]
    fn finishing_removes_the_audio_unless_something_failed() {
        let (s, t) = session();
        std::fs::write(s.dir.join("seg-00000.txt"), "ok").unwrap();
        let dir = s.dir.clone();
        s.finish().unwrap();
        assert!(!dir.exists());

        let s2 = Session::create(t.path(), Manifest::new(None, None, "", false)).unwrap();
        std::fs::write(s2.dir.join("seg-00000.err"), "no").unwrap();
        std::fs::write(s2.dir.join("seg-00000.wav"), "x").unwrap();
        let dir2 = s2.dir.clone();
        s2.finish().unwrap();
        assert!(dir2.exists());
        assert!(Session::open(&dir2).unwrap().manifest.saved);
    }
}
