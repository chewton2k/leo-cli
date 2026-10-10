//! Durable recording sources, shared by the terminal and browser.
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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
    crate::paths::contained_path(&root(notes), Path::new(note))
}

pub fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let parent = path.parent().ok_or_else(|| anyhow::anyhow!("Missing parent folder"))?;
    std::fs::create_dir_all(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
    }
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
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
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file() && entry.path().extension().is_some_and(|e| e == "json") {
            let source: Archive = serde_json::from_slice(&std::fs::read(entry.path())?)?;
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
            id: "session-123".into(), started: Utc::now(), passages: vec![], points: vec![],
            template: "lecture".into(), context: String::new(), warnings: vec![], trace: vec![],
        };
        save(&notes, "note-1234", &source).unwrap();
        save(&notes, "note-1234", &source).unwrap();
        assert_eq!(load(&notes, "note-1234").unwrap().len(), 1);
        assert!(save(&notes, "../../outside", &source).is_err());
    }
}
