use crate::record::Source;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::{Path, PathBuf},
};

const MOST_CHUNKS: usize = 100_000;
const MOST_SESSIONS: usize = 1000;
const MOST_CHUNK_BYTES: usize = 640_000;

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Journal {
    pub id: String,
    pub directory: String,
    pub title: Option<String>,
    pub source: Source,
    pub profile: leo_core::workflows::Profile,
    #[serde(default)]
    pub points: Vec<(u64, String)>,
}

pub(crate) fn root(notes: &Path) -> PathBuf {
    notes.parent().unwrap_or(notes).join("browser-audio")
}
pub(crate) fn folder(notes: &Path, id: &str) -> Result<PathBuf> {
    if !leo_core::recording::valid_id(id) {
        bail!("Invalid recording id");
    }
    leo_core::paths::contained_path(
        notes.parent().unwrap_or(notes),
        &Path::new("browser-audio").join(id),
    )
}
pub(crate) fn save(notes: &Path, meta: &Journal) -> Result<()> {
    leo_core::recording::write_json(&folder(notes, &meta.id)?.join("session.json"), meta)
}
pub(crate) fn chunks(notes: &Path, id: &str) -> Result<Vec<PathBuf>> {
    let mut found: Vec<_> = std::fs::read_dir(folder(notes, id)?)?
        .take(MOST_CHUNKS + 2)
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "pcm"))
        .collect();
    anyhow::ensure!(
        found.len() <= MOST_CHUNKS,
        "Too much recovery audio. Finish this recording first."
    );
    found.sort();
    Ok(found)
}
pub(crate) fn append(notes: &Path, id: &str, seq: u64, bytes: &[u8]) -> Result<bool> {
    anyhow::ensure!(
        seq < MOST_CHUNKS as u64 && !bytes.is_empty() && bytes.len() <= MOST_CHUNK_BYTES,
        "Audio exceeds the recording limit. Finish this recording first."
    );
    let dir = folder(notes, id)?;
    let target = leo_core::paths::contained_path(&dir, Path::new(&format!("{seq:020}.pcm")))?;
    if target.exists() {
        if std::fs::read(target)? != bytes {
            bail!("This audio sequence already contains different samples");
        }
        return Ok(false);
    }
    if seq > 0 && !dir.join(format!("{:020}.pcm", seq - 1)).exists() {
        bail!("Audio is out of order");
    }
    let mut file = tempfile::NamedTempFile::new_in(&dir)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    if let Err(error) = file.persist_noclobber(&target) {
        if error.error.kind() == std::io::ErrorKind::AlreadyExists {
            anyhow::ensure!(
                std::fs::read(target)? == bytes,
                "This sequence already contains different samples"
            );
            return Ok(false);
        }
        return Err(error.error.into());
    }
    #[cfg(unix)]
    std::fs::File::open(dir)?.sync_all()?;
    Ok(true)
}
pub(crate) fn pending(notes: &Path) -> Result<Vec<Journal>> {
    let dir = root(notes);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for (index, e) in std::fs::read_dir(dir)?.take(MOST_SESSIONS + 1).enumerate() {
        anyhow::ensure!(
            index < MOST_SESSIONS,
            "Too many interrupted recordings. Recover an existing recording first."
        );
        let e = e?;
        if e.file_type()?.is_dir() {
            let id = e.file_name().to_string_lossy().into_owned();
            let path =
                leo_core::paths::contained_path(&folder(notes, &id)?, Path::new("session.json"))?;
            if path.exists() {
                anyhow::ensure!(
                    std::fs::metadata(&path)?.len() <= 8_000_000,
                    "Recovery metadata is too large"
                );
                let meta: Journal = serde_json::from_slice(&std::fs::read(path)?)?;
                anyhow::ensure!(
                    meta.id == id,
                    "Recovery recording id does not match its folder"
                );
                out.push(meta);
            }
        }
    }
    Ok(out)
}
