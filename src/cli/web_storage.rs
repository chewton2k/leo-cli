use std::io::Read;
use std::path::Path;

use anyhow::{anyhow, bail, Result};

use leo_services::session::{self, Session};
use leo_web::storage::{act, size_of, Area, Housekeeper, Item};

pub struct Housekeeping;

fn label_of(session: &Session) -> String {
    session.manifest.title.clone().unwrap_or_else(|| {
        session
            .manifest
            .started
            .with_timezone(&chrono::Local)
            .format("Recording, %b %-d %-I:%M %p")
            .to_string()
    })
}

fn recordings() -> Option<Area> {
    let root = session::root().ok()?;
    let mut items: Vec<Item> = std::fs::read_dir(&root)
        .ok()?
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let dir = e.path();
            let opened = Session::open(&dir).ok()?;
            let locked = session::locked(&dir);
            let detail = if locked {
                "Recording right now".to_string()
            } else if opened.manifest.saved {
                "Audio kept because part of it could not be transcribed".to_string()
            } else {
                "Interrupted; leo finishes it the next time you open leo or record".to_string()
            };
            Some(Item {
                id: e.file_name().to_string_lossy().to_string(),
                label: label_of(&opened),
                detail,
                bytes: size_of(&dir),
                when: Some(opened.manifest.started),
                locked,
            })
        })
        .collect();
    items.sort_by_key(|i| std::cmp::Reverse(i.when));
    let actions = if items.iter().any(|i| !i.locked) {
        vec![act(
            "delete",
            "Delete selected",
            Some(
                "The audio of these recordings is deleted for good; notes already written from them stay.",
            ),
            true,
        )]
    } else {
        Vec::new()
    };
    Some(Area {
        id: "recordings".into(),
        title: "Recording audio".into(),
        about: "Audio leo still holds: recordings that were interrupted, or kept because part of them could not be transcribed. Finished recordings are deleted once their note is written.".into(),
        path: root.display().to_string(),
        bytes: size_of(&root),
        items,
        actions,
    })
}

fn speech_model() -> Area {
    let dir = leo_services::providers::speech_model_dir();
    let bytes = size_of(&dir);
    Area {
        id: "speech".into(),
        title: "Speech model".into(),
        about: "Parakeet, the built-in speech recognizer that turns recordings into text on this computer.".into(),
        path: dir.display().to_string(),
        bytes,
        items: Vec::new(),
        actions: if bytes > 0 {
            vec![act(
                "remove",
                "Remove the speech model",
                Some("Recording with the built-in speech model stops working until `leo update` downloads it again (about 670 MB). Recording with OpenAI, Gemini or xAI keeps working."),
                false,
            )]
        } else {
            Vec::new()
        },
    }
}

fn meaning_model() -> Area {
    let dir = leo_services::meaning::dir();
    let bytes = size_of(&dir);
    Area {
        id: "meaning-model".into(),
        title: "Embedding model".into(),
        about: "bge-small, which reads your notes on this computer so search and Felix find notes that mean the same thing in other words.".into(),
        path: dir.display().to_string(),
        bytes,
        items: Vec::new(),
        actions: if bytes > 0 {
            vec![act(
                "remove",
                "Remove the meaning model",
                Some("Search and Felix find notes by their words only until leo downloads the model again (about 134 MB) with `leo update` or the next `leo serve`."),
                false,
            )]
        } else {
            Vec::new()
        },
    }
}

fn settings() -> Option<Area> {
    let dir = leo_core::paths::config_dir().ok()?;
    let items: Vec<Item> = leo_web::storage::CONFIG_FILES
        .iter()
        .filter_map(|(name, what)| {
            let path = dir.join(name);
            path.is_file().then(|| Item {
                id: (*name).to_string(),
                label: (*name).to_string(),
                detail: (*what).to_string(),
                bytes: size_of(&path),
                when: None,
                locked: true,
            })
        })
        .collect();
    Some(Area {
        id: "settings".into(),
        title: "Settings and small caches".into(),
        about: "Your settings, the code in the leo serve link, signed-in browsers and small caches such as plan usage. leo needs these; they are not deleted from here.".into(),
        path: dir.display().to_string(),
        bytes: items.iter().map(|i| i.bytes).sum(),
        items,
        actions: Vec::new(),
    })
}

fn delete_recordings(root: &Path, ids: &[String]) -> Result<String> {
    let mut gone = 0;
    for id in ids {
        if id.is_empty() || id.contains(['/', '\\']) || id.starts_with('.') {
            bail!("That is not a recording leo knows.");
        }
        let dir = root.join(id);
        if !dir.is_dir() {
            continue;
        }
        if session::locked(&dir) {
            bail!("That recording is still going; stop it first.");
        }
        std::fs::remove_dir_all(&dir)?;
        gone += 1;
    }
    Ok(format!(
        "Deleted {gone} recording{}.",
        if gone == 1 { "" } else { "s" }
    ))
}

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
        anyhow::ensure!(
            index < MOST_RECORDINGS,
            "Too many recordings. Delete old recordings in Storage first."
        );
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
            "Recording metadata is too large. Delete this recording in Storage."
        );
        let Ok(manifest) = serde_json::from_slice::<session::Manifest>(&bytes) else {
            continue;
        };
        if [id.to_string(), format!("{id}-you"), format!("{id}-others")].contains(&manifest.id) {
            let lock = leo_core::paths::contained_path(&dir, Path::new("lock"))?;
            if lock.exists() {
                anyhow::ensure!(
                    std::fs::metadata(lock)?.len() <= 64,
                    "The recording lock could not be checked. Delete this recording in Storage."
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
    fn discard_recording(&self, id: &str) -> Result<()> {
        discard_recording(&session::root()?, id)
    }
    fn areas(&self) -> Vec<Area> {
        let mut out = Vec::new();
        out.extend(recordings());
        out.push(speech_model());
        out.push(meaning_model());
        out.extend(settings());
        out
    }

    fn act(&self, area: &str, action: &str, items: &[String]) -> Option<Result<String>> {
        match (area, action) {
            ("recordings", "delete") => Some(
                session::root()
                    .map_err(|e| anyhow!(e))
                    .and_then(|root| delete_recordings(&root, items)),
            ),
            ("meaning-model", "remove") => {
                let dir = leo_services::meaning::dir();
                Some(
                    std::fs::remove_dir_all(&dir)
                        .map(|()| {
                            "Removed the meaning model. `leo update` or the next `leo serve` downloads it again.".to_string()
                        })
                        .map_err(|e| anyhow!("Could not remove {}: {e}", dir.display())),
                )
            }
            ("speech", "remove") => {
                let dir = leo_services::providers::speech_model_dir();
                Some(
                    std::fs::remove_dir_all(&dir)
                        .map(|()| {
                            "Removed the speech model. `leo update` downloads it again.".to_string()
                        })
                        .map_err(|e| anyhow!("Could not remove {}: {e}", dir.display())),
                )
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_recording_id_cannot_reach_outside_the_recordings_folder() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("2026-10-07-1")).unwrap();
        for bad in ["../notes", "..", "a/b", ".hidden", ""] {
            assert!(
                delete_recordings(tmp.path(), &[bad.to_string()]).is_err(),
                "{bad}"
            );
        }
        assert_eq!(
            delete_recordings(tmp.path(), &["2026-10-07-1".into(), "missing".into()]).unwrap(),
            "Deleted 1 recording."
        );
        assert!(!tmp.path().join("2026-10-07-1").exists());
    }
}
