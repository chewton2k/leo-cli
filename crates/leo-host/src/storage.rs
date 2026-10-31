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
    if !std::fs::symlink_metadata(&root).ok()?.is_dir() {
        return None;
    }
    let mut items: Vec<Item> = std::fs::read_dir(&root)
        .ok()?
        .take(MOST_RECORDINGS)
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
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
        about: "Local speech models used for dictation and recording. Manage individual models in Settings → Speech models.".into(),
        path: dir.display().to_string(),
        bytes,
        items: Vec::new(),
        actions: Vec::new(),
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
                Some("Search and Felix find notes by their words only until leo downloads the model again (about 134 MB) the next time leo opens, or with `leo update`."),
                false,
            )]
        } else {
            Vec::new()
        },
    }
}

fn python_runtime_at(dir: &Path) -> Area {
    let bytes = size_of(dir);
    Area {
        id: "python-runtime".into(), title: "Python practice runtime".into(),
        about: "The Python engine downloaded for Run in coding practice. Your code runs in your browser. Removing this download keeps practices, code, attempts and reviews; a future Run downloads the engine again.".into(),
        path: dir.display().to_string(), bytes, items: vec![],
        actions: if bytes > 0 { vec![act("remove", "Remove Python runtime", Some("Only the downloaded Python engine is removed. Your practices and code stay. The next Python Run may download about 12 MB again; an engine already loaded in this browser keeps working until the page is closed."), false)] } else { vec![] },
    }
}

fn application() -> Option<Area> {
    let path = std::env::current_exe().ok()?;
    Some(Area {
        id: "application".into(), title: "Leo application".into(),
        about: "The Leo program, including the website and its coding editor. Keep it to use Leo. To remove the installed application, use leo uninstall in your terminal.".into(),
        path: path.display().to_string(), bytes: size_of(&path), items: vec![], actions: vec![],
    })
}

const MOST_MODEL_FILES: usize = 1000;

fn other_models() -> Area {
    let root = leo_services::config::choice::models_dir();
    let named = [
        leo_services::providers::speech_model_dir(),
        leo_services::meaning::dir(),
        leo_services::pyodide::dir(),
    ];
    let items: Vec<Item> = std::fs::read_dir(&root)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .take(MOST_MODEL_FILES)
        .filter(|e| {
            e.file_type().is_ok_and(|t| t.is_file() || t.is_dir()) && !named.contains(&e.path())
        })
        .map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let locked = name.ends_with(".part");
            Item {
                id: name.clone(),
                label: name,
                detail: if locked {
                    "A download in progress; wait for it to finish".into()
                } else {
                    "Other downloaded or older model files".into()
                },
                bytes: size_of(&e.path()),
                when: e
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .map(chrono::DateTime::from),
                locked,
            }
        })
        .collect();
    let can_delete = items.iter().any(|i| !i.locked);
    Area { id: "other-models".into(), title: "Other downloaded model files".into(), about: "Older models and other files in Leo's model download folder. The speech model, meaning model and Python engine are listed separately.".into(), path: root.display().to_string(), bytes: items.iter().map(|i| i.bytes).sum(), items, actions: if can_delete { vec![act("delete", "Delete selected", Some("Selected downloaded files are removed from this computer. Leo may need to download a model again if you choose to use it."), true)] } else { vec![] } }
}

fn remove_cached_dir(dir: &Path, name: &str) -> Result<String> {
    let Ok(meta) = std::fs::symlink_metadata(dir) else {
        return Ok(format!("No {name} download is present."));
    };
    if !meta.is_dir() {
        bail!(
            "Choose a downloaded folder shown in Storage; symbolic links cannot be removed here."
        );
    }
    let checked = leo_core::paths::contained_path(
        dir.parent()
            .ok_or_else(|| anyhow!("That download folder could not be checked."))?,
        Path::new(
            dir.file_name()
                .ok_or_else(|| anyhow!("That download folder could not be checked."))?,
        ),
    )?;
    if std::fs::read_dir(&checked)?
        .take(MOST_MODEL_FILES)
        .flatten()
        .any(|e| e.file_name().to_string_lossy().ends_with(".part"))
    {
        bail!("{name} is being downloaded. Wait for it to finish, then remove it in Storage.");
    }
    std::fs::remove_dir_all(&checked).map_err(|_| anyhow!("Could not remove {name}. Stop any recording or practice Run, check that Leo can write to its download folder, then try again."))?;
    Ok(format!("Removed {name}. Your saved work stays."))
}

fn remove_other_models(
    root: &Path,
    ids: &[String],
    named: &[std::path::PathBuf],
) -> Result<String> {
    if ids.len() > MOST_MODEL_FILES
        || ids.iter().any(|id| {
            id.is_empty()
                || id.len() > 255
                || id.contains(['/', '\\', ':'])
                || id == "."
                || id == ".."
                || id.ends_with(".part")
        })
    {
        bail!("Choose downloaded files shown in Storage; wait for active downloads to finish.");
    }
    let paths = ids
        .iter()
        .map(|id| leo_core::paths::contained_path(root, Path::new(id)))
        .collect::<Result<Vec<_>>>()?;
    if paths.iter().any(|p| named.contains(p)) {
        bail!("Use the speech, meaning or Python card to remove that download.");
    }
    let mut gone = 0;
    for path in paths {
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            remove_cached_dir(&path, "downloaded model")?;
        } else if meta.is_file() {
            std::fs::remove_file(&path).map_err(|_| anyhow!("A downloaded file could not be deleted. Check that Leo can write to its download folder, then try again."))?;
        } else {
            bail!("Choose a downloaded file shown in Storage.");
        }
        gone += 1;
    }
    Ok(format!(
        "Deleted {gone} downloaded files. Saved notes and practices stay."
    ))
}

fn settings() -> Option<Area> {
    let config = leo_core::paths::config_dir().ok()?;
    let mut roots = vec![config];
    if let Ok(data) = leo_core::paths::data_dir() {
        if !roots.contains(&data) {
            roots.push(data);
        }
    }
    let items: Vec<Item> = roots
        .iter()
        .flat_map(|dir| {
            leo_web::storage::CONFIG_FILES
                .iter()
                .filter_map(move |(name, what)| {
                    let path = dir.join(name);
                    std::fs::symlink_metadata(&path)
                        .ok()
                        .filter(|m| m.is_file())
                        .map(|meta| Item {
                            id: path.display().to_string(),
                            label: (*name).into(),
                            detail: format!("{what} · {}", dir.display()),
                            bytes: meta.len(),
                            when: meta.modified().ok().map(chrono::DateTime::from),
                            locked: true,
                        })
                })
        })
        .collect();
    Some(Area {
        id: "settings".into(), title: "Settings and small caches".into(),
        about: "Settings, protected credentials, the serve link, signed-in browsers and small usage caches, from both Leo's settings and data folders. Manage settings and sign-ins using their controls; secret values are never shown here.".into(),
        path: roots.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(" · "),
        bytes: items.iter().map(|i| i.bytes).sum(), items, actions: Vec::new(),
    })
}

fn writing_profiles_at(root: &Path) -> Area {
    let safe = std::fs::symlink_metadata(root).is_ok_and(|m| m.is_dir());
    let items: Vec<Item> = [("agy", "Google Antigravity"), ("copilot", "GitHub Copilot")]
        .into_iter()
        .filter(|_| safe)
        .filter_map(|(id, label)| {
            let path = leo_core::paths::contained_path(root, Path::new(id)).ok()?;
            let metadata = std::fs::symlink_metadata(&path).ok()?;
            if !metadata.is_dir() {
                return None;
            }
            Some(Item {
                id: id.into(),
                label: label.into(),
                detail: "Saved private sign-in state and caches from a removed writing provider."
                    .into(),
                bytes: size_of(&path),
                when: metadata
                    .modified()
                    .ok()
                    .map(chrono::DateTime::<chrono::Utc>::from),
                locked: false,
            })
        })
        .collect();
    let bytes = items.iter().map(|item| item.bytes).sum();
    Area {
        id: "writing-profiles".into(),
        title: "Legacy writing profiles".into(),
        about: "Saved data from removed Google Antigravity and GitHub Copilot support. Nothing is deleted automatically; private contents are never included in exports.".into(),
        path: root.display().to_string(),
        bytes,
        actions: if items.is_empty() { Vec::new() } else {
            vec![act("clear", "Clear selected profiles", Some("Saved sign-in details, conversation history and local caches for these removed writing providers are deleted. This cannot be undone. A profile in use cannot be cleared."), true)]
        },
        items,
    }
}

fn clear_writing_profiles(items: &[String]) -> Result<String> {
    anyhow::ensure!(
        !items.is_empty()
            && items.len() <= 2
            && items
                .iter()
                .all(|id| matches!(id.as_str(), "agy" | "copilot")),
        "Choose a legacy writing profile shown in Storage."
    );
    for name in items {
        leo_services::ai::provider::scoped_cli::clear_profile(name)?;
    }
    Ok("Cleared the selected legacy writing profiles.".into())
}

fn delete_recordings(root: &Path, ids: &[String]) -> Result<String> {
    if ids.len() > MOST_RECORDINGS || ids.iter().any(|id| !leo_core::recording::valid_id(id)) {
        bail!("That is not a recording leo knows.");
    }
    if !root.exists() {
        return Ok("Deleted 0 recordings.".into());
    }
    anyhow::ensure!(
        !std::fs::symlink_metadata(root)?.file_type().is_symlink(),
        "Recording folders cannot use symbolic links"
    );
    let mut dirs = Vec::new();
    for id in ids {
        let dir = leo_core::paths::contained_path(root, Path::new(id))?;
        let Ok(meta) = std::fs::symlink_metadata(&dir) else {
            continue;
        };
        if !meta.is_dir() {
            bail!("Choose a recording shown in Storage; symbolic links cannot be removed here.");
        }
        if session::locked(&dir) {
            bail!("That recording is still going; stop it first.");
        }
        dirs.push(dir);
    }
    for dir in &dirs {
        std::fs::remove_dir_all(dir)?;
    }
    let gone = dirs.len();
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
    fn pending_recordings(&self) -> Result<Vec<leo_web::record::Pending>> {
        crate::record::pending()
    }
    fn discard_recording(&self, id: &str) -> Result<()> {
        discard_recording(&session::root()?, id)
    }
    fn areas(&self) -> Vec<Area> {
        let mut out = Vec::new();
        out.extend(recordings());
        out.push(speech_model());
        out.push(meaning_model());
        out.push(python_runtime_at(&leo_services::pyodide::dir()));
        out.push(other_models());
        out.extend(application());
        out.extend(settings());
        if let Ok(root) = leo_services::ai::provider::scoped_cli::profile_root() {
            let area = writing_profiles_at(&root);
            if !area.items.is_empty() {
                out.push(area);
            }
        }
        out
    }

    fn act(&self, area: &str, action: &str, items: &[String]) -> Option<Result<String>> {
        match (area, action) {
            ("writing-profiles", "clear") => Some(clear_writing_profiles(items)),
            ("recordings", "delete") => Some(
                session::root()
                    .map_err(|e| anyhow!(e))
                    .and_then(|root| delete_recordings(&root, items)),
            ),
            ("meaning-model", "remove") => Some(remove_cached_dir(
                &leo_services::meaning::dir(),
                "the meaning model",
            )),
            ("speech", "remove") => Some(Err(anyhow!(
                "Manage downloaded models in Settings → Speech models."
            ))),
            ("python-runtime", "remove") => Some(remove_cached_dir(
                &leo_services::pyodide::dir(),
                "the Python runtime",
            )),
            ("other-models", "delete") => Some(remove_other_models(
                &leo_services::config::choice::models_dir(),
                items,
                &[
                    leo_services::providers::speech_model_dir(),
                    leo_services::meaning::dir(),
                    leo_services::pyodide::dir(),
                ],
            )),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writing_profiles_show_metadata_without_exposing_authentication_contents() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("agent-profiles");
        std::fs::create_dir_all(root.join("agy")).unwrap();
        std::fs::create_dir_all(root.join("copilot")).unwrap();
        std::fs::write(root.join("agy/token.json"), "fake-private-token").unwrap();
        std::fs::write(root.join("copilot/history.json"), "fake-private-chat").unwrap();
        let area = writing_profiles_at(&root);
        assert_eq!(area.items.len(), 2);
        assert!(area.bytes > 0);
        assert_eq!(area.actions[0].id, "clear");
        assert!(area.actions[0]
            .confirm
            .as_ref()
            .unwrap()
            .contains("cannot be undone"));
        let rendered = serde_json::to_string(&area).unwrap();
        assert!(!rendered.contains("fake-private-token"));
        assert!(!rendered.contains("fake-private-chat"));
        assert!(!rendered.contains("token.json"));
        assert!(clear_writing_profiles(&["../outside".into()]).is_err());
    }

    #[test]
    fn downloaded_runtime_sizes_and_removal_preserve_saved_work() {
        let temp = tempfile::tempdir().unwrap();
        let runtime = temp.path().join("python");
        std::fs::create_dir(&runtime).unwrap();
        std::fs::write(runtime.join("python.wasm"), vec![0; 2048]).unwrap();
        let saved = temp.path().join("leo.db");
        std::fs::write(&saved, "saved practices").unwrap();
        let area = python_runtime_at(&runtime);
        assert_eq!(area.bytes, 2048);
        assert_eq!(area.actions[0].id, "remove");
        std::fs::write(runtime.join("python.part"), "downloading").unwrap();
        assert!(remove_cached_dir(&runtime, "Python").is_err());
        assert!(runtime.join("python.wasm").exists());
        std::fs::remove_file(runtime.join("python.part")).unwrap();
        remove_cached_dir(&runtime, "Python").unwrap();
        assert!(!runtime.exists());
        assert_eq!(std::fs::read_to_string(saved).unwrap(), "saved practices");
        assert!(python_runtime_at(&runtime).actions.is_empty());
        remove_cached_dir(&runtime, "Python").unwrap();
    }

    #[test]
    fn other_download_cleanup_validates_all_paths_before_deleting() {
        let temp = tempfile::tempdir().unwrap();
        let keep = temp.path().join("speech");
        std::fs::create_dir(&keep).unwrap();
        let old = temp.path().join("older-model");
        std::fs::write(&old, "old").unwrap();
        for bad in [
            "../outside",
            "a/b",
            "a\\b",
            "..",
            ".",
            "",
            "speech",
            "active.part",
        ] {
            assert!(
                remove_other_models(
                    temp.path(),
                    &["older-model".into(), bad.into()],
                    std::slice::from_ref(&keep)
                )
                .is_err(),
                "{bad}"
            );
            assert!(old.exists());
        }
        remove_other_models(
            temp.path(),
            &["older-model".into()],
            std::slice::from_ref(&keep),
        )
        .unwrap();
        assert!(!old.exists());
        assert!(keep.exists());
    }

    #[cfg(unix)]
    #[test]
    fn downloaded_folders_cannot_follow_a_symbolic_link() {
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("keep"), "keep").unwrap();
        let link = temp.path().join("linked-model");
        std::os::unix::fs::symlink(outside.path(), &link).unwrap();
        assert!(remove_cached_dir(&link, "model").is_err());
        assert!(remove_other_models(temp.path(), &["linked-model".into()], &[]).is_err());
        assert!(outside.path().join("keep").exists());
    }

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
            delete_recordings(
                tmp.path(),
                &["2026-10-07-1".into(), "missing-recording".into()]
            )
            .unwrap(),
            "Deleted 1 recording."
        );
        assert!(!tmp.path().join("2026-10-07-1").exists());
    }
}
