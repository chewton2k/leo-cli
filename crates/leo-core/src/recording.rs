use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

const MOST_SOURCES: usize = 1000;
const MOST_SOURCE_BYTES: u64 = 64_000_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Passage {
    pub start_secs: u64,
    pub end_secs: u64,
    pub text: String,
    #[serde(default)]
    pub speaker: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Point {
    pub at_secs: u64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Trace {
    pub at: DateTime<Utc>,
    pub stage: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Archive {
    pub id: String,
    pub started: DateTime<Utc>,
    pub passages: Vec<Passage>,
    pub points: Vec<Point>,
    #[serde(default)]
    pub template: String,
    #[serde(default)]
    pub context: String,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub trace: Vec<Trace>,
}

pub fn valid_id(id: &str) -> bool {
    (8..=80).contains(&id.len()) && id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
}

pub fn root(notes: &Path) -> PathBuf {
    notes.parent().unwrap_or(notes).join("recording-sources")
}

fn folder(notes: &Path, note: &str) -> Result<PathBuf> {
    if !valid_id(note) {
        bail!("Invalid recording note id");
    }
    crate::paths::contained_path(
        notes.parent().unwrap_or(notes),
        &Path::new("recording-sources").join(note),
    )
}

pub fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Missing parent folder"))?;
    if let Ok(meta) = std::fs::symlink_metadata(parent) {
        anyhow::ensure!(
            !meta.file_type().is_symlink(),
            "Recording files cannot use symbolic links"
        );
    }
    std::fs::create_dir_all(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
    }
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    serde_json::to_writer_pretty(&mut file, value)?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| e.error)?;
    #[cfg(unix)]
    std::fs::File::open(parent)?.sync_all()?;
    Ok(())
}

pub fn save(notes: &Path, note: &str, source: &Archive) -> Result<()> {
    if !valid_id(&source.id) {
        bail!("Invalid recording id");
    }
    let dir = folder(notes, note)?;
    let path = crate::paths::contained_path(&dir, Path::new(&format!("{}.json", source.id)))?;
    write_json(&path, source)
}

pub fn load(notes: &Path, note: &str) -> Result<Vec<Archive>> {
    let dir = folder(notes, note)?;
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let mut bytes = 0u64;
    for (index, entry) in std::fs::read_dir(&dir)?.take(MOST_SOURCES + 1).enumerate() {
        anyhow::ensure!(
            index < MOST_SOURCES,
            "Too many recording sources in this note"
        );
        let entry = entry?;
        if entry.file_type()?.is_file() && entry.path().extension().is_some_and(|e| e == "json") {
            bytes = bytes.saturating_add(entry.metadata()?.len());
            anyhow::ensure!(
                bytes <= MOST_SOURCE_BYTES,
                "This note's recording sources exceed the size limit"
            );
            let source: Archive = serde_json::from_slice(&std::fs::read(entry.path())?)?;
            anyhow::ensure!(valid_id(&source.id), "Invalid saved recording id");
            out.push(source);
        }
    }
    out.sort_by_key(|s| s.started);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_are_private_durable_and_repeatable() {
        let temp = tempfile::tempdir().unwrap();
        let notes = temp.path().join("notes");
        let source = Archive {
            id: "session-123".into(),
            started: Utc::now(),
            passages: vec![],
            points: vec![],
            template: "lecture".into(),
            context: String::new(),
            warnings: vec![],
            trace: vec![],
        };
        save(&notes, "note-1234", &source).unwrap();
        save(&notes, "note-1234", &source).unwrap();
        assert_eq!(load(&notes, "note-1234").unwrap().len(), 1);
        assert!(save(&notes, "../../outside", &source).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn recording_roots_and_writers_reject_user_controlled_symlinks() {
        let temp = tempfile::tempdir().unwrap();
        let notes = temp.path().join("notes");
        let outside = temp.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, root(&notes)).unwrap();
        assert!(folder(&notes, "note-1234").is_err());
        assert!(write_json(&root(&notes).join("test.json"), &serde_json::json!({})).is_err());
        assert_eq!(std::fs::read_dir(outside).unwrap().count(), 0);
    }
}
