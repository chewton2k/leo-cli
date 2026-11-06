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
        std::fs::remove_dir_all(dir)?;
    }
    Ok(())
}

impl Housekeeper for Housekeeping {
    fn pending_recordings(&self) -> Result<Vec<leo_web::record::Pending>> {
        crate::record::pending()
    }
    fn discard_recording(&self, id: &str) -> Result<()> {
        discard_recording(&session::root()?, id)
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
