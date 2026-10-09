use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::Result;

use leo_core::attachments;
use leo_core::store::Store;

pub struct Picture {
    pub mime: String,
    pub bytes: Vec<u8>,
}

pub type Seer = Arc<dyn Fn(&str, &str, Vec<Picture>) -> Result<String> + Send + Sync>;

pub const DESCRIBE: &str = "You describe pictures from someone's study notes for a reader who cannot see them. Say what kind of picture it is, then its content: any text, labels and numbers, the structure of a diagram, a chart's axes and trend, code, or the steps shown, and what it explains. Use interpretable language. Two to six sentences. Reply with the description only.";
const MOST_PICTURE_BYTES: u64 = 20 * 1024 * 1024;

pub struct Captions {
    path: PathBuf,
    known: Mutex<BTreeMap<String, String>>,
}

impl Captions {
    pub fn for_notes(notes_dir: &Path) -> Captions {
        let base = notes_dir.parent().unwrap_or(notes_dir);
        Captions::at(base.join("captions.json"))
    }

    pub fn at(path: PathBuf) -> Captions {
        let known = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Captions {
            path,
            known: Mutex::new(known),
        }
    }

    pub fn get(&self, key: &str) -> Option<String> {
        self.known.lock().ok()?.get(key).cloned()
    }

    pub fn put(&self, key: String, caption: String) {
        let Ok(mut known) = self.known.lock() else {
            return;
        };
        known.insert(key, caption);
        if let Ok(text) = serde_json::to_string_pretty(&*known) {
            let tmp = self.path.with_extension("json.tmp");
            if std::fs::write(&tmp, text).is_ok() {
                let _ = std::fs::rename(&tmp, &self.path);
            }
        }
    }

    pub fn len(&self) -> usize {
        self.known.lock().map(|k| k.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

pub fn key_of(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some(format!("{}:{}:{modified}", path.display(), meta.len()))
}

pub fn pictures_of(notes_dir: &Path, dir: &str, body: &str) -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = Vec::new();
    for line in body.lines() {
        for (_, shown) in attachments::pictures_in(line) {
            if let Some(path) = attachments::resolve(notes_dir, dir, &shown.target) {
                if !out.iter().any(|(_, p)| *p == path) {
                    out.push((shown.alt.clone(), path));
                }
            }
        }
    }
    out
}

pub fn captioned(notes_dir: &Path, dir: &str, body: &str, captions: &Captions) -> String {
    let mut out = String::with_capacity(body.len());
    for (i, line) in body.lines().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let found = attachments::pictures_in(line);
        if found.is_empty() {
            out.push_str(line);
            continue;
        }
        let mut at = 0;
        for (range, shown) in found {
            out.push_str(&line[at..range.end]);
            at = range.end;
            let caption = attachments::resolve(notes_dir, dir, &shown.target)
                .and_then(|path| key_of(&path))
                .and_then(|key| captions.get(&key));
            if let Some(caption) = caption {
                out.push_str(&format!(" [Picture: {caption}]"));
            }
        }
        out.push_str(&line[at..]);
    }
    out
}

pub fn picture(path: &Path) -> Option<Picture> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() > MOST_PICTURE_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let mime = attachments::mime_of(attachments::kind_of(&bytes)?)?.to_string();
    Some(Picture { mime, bytes })
}

pub fn uncaptioned(store: &Store, captions: &Captions, most: usize) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for note in &store.notes {
        for (_, path) in pictures_of(&store.notes_dir, &note.directory, &note.body) {
            if out.len() >= most {
                return out;
            }
            let missing = key_of(&path).is_some_and(|key| captions.get(&key).is_none());
            if missing && !out.contains(&path) {
                out.push(path);
            }
        }
    }
    out
}

pub fn describe(path: &Path, seer: &Seer, question: &str) -> Result<String> {
    let picture = picture(path).ok_or_else(|| anyhow::anyhow!("that picture cannot be read"))?;
    let user = if question.trim().is_empty() {
        "Describe this picture.".to_string()
    } else {
        format!(
            "Describe this picture, and answer this about it: {}",
            question.trim()
        )
    };
    let said = seer(DESCRIBE, &user, vec![picture])?;
    Ok(said.split_whitespace().collect::<Vec<_>>().join(" "))
}

pub fn caption_some(store: &Store, captions: &Captions, seer: &Seer, most: usize) -> usize {
    caption_paths(&uncaptioned(store, captions, most), captions, seer)
}

pub fn caption_paths(paths: &[PathBuf], captions: &Captions, seer: &Seer) -> usize {
    let mut done = 0;
    for path in paths {
        let path = path.as_path();
        let Some(key) = key_of(path) else {
            continue;
        };
        match describe(path, seer, "") {
            Ok(caption) if !caption.is_empty() => {
                captions.put(key, caption);
                done += 1;
            }
            _ => break,
        }
    }
    done
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png() -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        bytes.extend_from_slice(&[0, 0, 0, 2, 0, 0, 0, 2, 8, 2, 0, 0, 0]);
        bytes.extend_from_slice(&[0; 64]);
        bytes
    }

    #[test]
    fn pictures_get_a_caption_once_and_felix_reads_it_inline() {
        let dir = tempfile::tempdir().unwrap();
        let notes = dir.path().join("notes");
        let mut store = Store::load_from(&notes).unwrap();
        std::fs::create_dir_all(notes.join("attachments")).unwrap();
        std::fs::write(notes.join("attachments/heap.png"), png()).unwrap();
        store
            .create_note(
                "Heaps",
                "A heap:\n![min heap](attachments/heap.png) and ![[missing.png]]",
                vec![],
                "",
            )
            .unwrap();
        let captions = Captions::for_notes(&notes);
        assert_eq!(uncaptioned(&store, &captions, 10).len(), 1);
        let calls = Arc::new(Mutex::new(0));
        let counted = Arc::clone(&calls);
        let seer: Seer = Arc::new(move |system: &str, user: &str, pictures: Vec<Picture>| {
            assert!(system.contains("Use interpretable language"));
            assert_eq!(user, "Describe this picture.");
            assert_eq!(pictures[0].mime, "image/png");
            *counted.lock().unwrap() += 1;
            Ok("A min heap\n with 2 at the root.".into())
        });
        assert_eq!(caption_some(&store, &captions, &seer, 10), 1);
        assert_eq!(
            caption_some(&store, &captions, &seer, 10),
            0,
            "already captioned"
        );
        assert_eq!(*calls.lock().unwrap(), 1);
        let body = &store.notes[0].body;
        assert_eq!(
            captioned(&notes, "", body, &Captions::for_notes(&notes)),
            "A heap:\n![min heap](attachments/heap.png) [Picture: A min heap with 2 at the root.] and ![[missing.png]]",
            "captions are kept on disk"
        );
    }
}
