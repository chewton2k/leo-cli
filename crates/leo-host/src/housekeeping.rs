use std::io::Read;
use std::path::Path;

use anyhow::Result;

use leo_services::session;
use leo_web::record::Housekeeper;

pub struct Housekeeping;

const MOST_RECORDINGS: usize = 1000;
const MOST_SESSION_BYTES: u64 = 8_000_000;

fn discard_recording(root: &Path, id: &str) -> Result<()> {
    anyhow::ensure!(
        leo_core::recording::valid_id(id),
        "That is not a recording leo knows."
    );
    if !root.exists() {
        return Ok(());
    }
    anyhow::ensure!(
        !std::fs::symlink_metadata(root)?.file_type().is_symlink(),
        "Recording folders cannot use symbolic links"
    );
    let mut dirs = Vec::new();
    for (index, entry) in std::fs::read_dir(root)?
        .take(MOST_RECORDINGS + 1)
        .enumerate()
    {
        anyhow::ensure!(index < MOST_RECORDINGS, "Too many recordings to check.");
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let dir = leo_core::paths::contained_path(root, Path::new(&entry.file_name()))?;
        let metadata = leo_core::paths::contained_path(&dir, Path::new("session.json"))?;
        let Ok(file) = std::fs::File::open(metadata) else {
            continue;
        };
        let mut bytes = Vec::new();
        file.take(MOST_SESSION_BYTES + 1).read_to_end(&mut bytes)?;
        anyhow::ensure!(
            bytes.len() as u64 <= MOST_SESSION_BYTES,
            "Recording metadata is too large. Try again later."
        );
        let Ok(manifest) = serde_json::from_slice::<session::Manifest>(&bytes) else {
            continue;
        };
        if [id.to_string(), format!("{id}-you"), format!("{id}-others")].contains(&manifest.id) {
            let lock = leo_core::paths::contained_path(&dir, Path::new("lock"))?;
            if lock.exists() {
                anyhow::ensure!(
                    std::fs::metadata(lock)?.len() <= 64,
                    "The recording lock could not be checked. Try again later."
                );
            }
            anyhow::ensure!(
                !session::locked(&dir),
                "The recording is still finishing. Try again in a moment."
            );
            dirs.push(dir);
        }
    }
    for dir in dirs {
        anyhow::ensure!(
            !session::locked(&dir),
            "The recording is still finishing. Try again in a moment."
        );
        std::fs::remove_dir_all(dir)?;
    }
    Ok(())
}

fn real_dirs(root: &Path) -> Vec<std::path::PathBuf> {
    if !std::fs::symlink_metadata(root).is_ok_and(|m| m.is_dir()) {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .take(MOST_RECORDINGS)
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| leo_core::paths::contained_path(root, Path::new(&e.file_name())).ok())
        .collect()
}

fn expire_saved(root: &Path, before: chrono::DateTime<chrono::Utc>) -> usize {
    real_dirs(root)
        .into_iter()
        .filter(|dir| !session::locked(dir))
        .filter(|dir| {
            session::Session::open(dir)
                .is_ok_and(|s| s.manifest.saved && s.manifest.started < before)
        })
        .filter(|dir| std::fs::remove_dir_all(dir).is_ok())
        .count()
}

fn expire_browser_audio(root: &Path, before: std::time::SystemTime) -> usize {
    real_dirs(root)
        .into_iter()
        .filter(|dir| {
            std::fs::symlink_metadata(dir)
                .and_then(|m| m.modified())
                .is_ok_and(|modified| modified < before)
        })
        .filter(|dir| std::fs::remove_dir_all(dir).is_ok())
        .count()
}

impl Housekeeper for Housekeeping {
    fn checks(&self) -> leo_web::record::Checks {
        leo_web::record::Checks {
            speech: leo_services::speech::library::library().is_ok_and(|l| l.selected().is_ok()),
            writing: leo_services::ai::writing_ready(),
            headset: session::mic::output_is_headset(),
        }
    }
    fn pending_recordings(&self) -> Result<Vec<leo_web::record::Pending>> {
        crate::record::pending()
    }
    fn discard_recording(&self, id: &str) -> Result<()> {
        discard_recording(&session::root()?, id)
    }
    fn expire_kept_audio(&self, before: chrono::DateTime<chrono::Utc>) -> usize {
        let saved = session::root().map_or(0, |root| expire_saved(&root, before));
        let browser = leo_core::paths::data_dir().map_or(0, |data| {
            expire_browser_audio(&data.join("browser-audio"), before.into())
        });
        saved + browser
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use leo_services::session::Session;

    #[test]
    fn discarding_a_call_removes_both_tracks_and_keeps_other_recordings() {
        let tmp = tempfile::tempdir().unwrap();
        let create = |id: &str| {
            let mut manifest = session::Manifest::new(None, None, "", false);
            manifest.id = id.into();
            Session::create(tmp.path(), manifest).unwrap().dir
        };
        let you = create("call-failed-01-you");
        let others = create("call-failed-01-others");
        let keep = create("keep-01");
        discard_recording(tmp.path(), "call-failed-01").unwrap();
        assert!(!you.exists());
        assert!(!others.exists());
        assert!(keep.exists());
        discard_recording(tmp.path(), "call-failed-01").unwrap();
        assert!(discard_recording(tmp.path(), "../outside").is_err());
    }

    #[test]
    fn kept_audio_past_its_time_goes_and_everything_else_stays() {
        let tmp = tempfile::tempdir().unwrap();
        let sessions = tmp.path().join("recordings");
        std::fs::create_dir(&sessions).unwrap();
        let create = |id: &str, saved: bool, days: i64| {
            let mut manifest = session::Manifest::new(None, None, "", false);
            manifest.id = id.into();
            manifest.saved = saved;
            manifest.started = chrono::Utc::now() - chrono::Duration::days(days);
            Session::create(&sessions, manifest).unwrap().dir
        };
        let old_saved = create("old-saved-01", true, 40);
        let new_saved = create("new-saved-01", true, 2);
        let old_pending = create("old-pending-01", false, 40);
        let locked = create("old-locked-01", true, 40);
        std::fs::write(locked.join("lock"), std::process::id().to_string()).unwrap();
        let before = chrono::Utc::now() - chrono::Duration::days(30);
        assert_eq!(expire_saved(&sessions, before), 1);
        assert!(!old_saved.exists());
        assert!(new_saved.exists() && old_pending.exists() && locked.exists());

        let browser = tmp.path().join("browser-audio");
        std::fs::create_dir_all(browser.join("left-behind")).unwrap();
        std::fs::write(browser.join("left-behind/0.webm"), b"audio").unwrap();
        let earlier = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
        assert_eq!(expire_browser_audio(&browser, earlier), 0);
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
        assert_eq!(expire_browser_audio(&browser, later), 1);
        assert!(!browser.join("left-behind").exists());
        assert_eq!(expire_browser_audio(&tmp.path().join("absent"), later), 0);
    }

    #[test]
    fn a_recording_in_use_is_never_discarded() {
        let tmp = tempfile::tempdir().unwrap();
        let mut manifest = session::Manifest::new(None, None, "", false);
        manifest.id = "running-01".into();
        let dir = Session::create(tmp.path(), manifest).unwrap().dir;
        std::fs::write(dir.join("lock"), std::process::id().to_string()).unwrap();
        assert!(discard_recording(tmp.path(), "running-01").is_err());
        assert!(dir.exists());
    }
}
