use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::chats::valid_id;

pub const MOST_FILES: usize = 10;
pub const UPLOAD_BYTES: usize = 40 * 1024 * 1024;
pub const ORPHAN_HOURS: i64 = 24;
pub const EXCERPT_CHARS: usize = 320;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Stored {
    id: String,
    name: String,
    added_at: DateTime<Utc>,
    text: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Doc {
    pub id: String,
    pub name: String,
    pub chars: usize,
    pub bytes: u64,
    pub added_at: DateTime<Utc>,
    pub excerpt: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Held {
    pub chat: String,
    pub docs: usize,
    pub bytes: u64,
    pub last: Option<DateTime<Utc>>,
}

fn folder(dir: &Path, chat: &str) -> Option<PathBuf> {
    valid_id(chat).then(|| dir.join(format!("{chat}.files")))
}

fn valid_doc(id: &str) -> bool {
    id.len() == 32 && id.chars().all(|c| c.is_ascii_hexdigit())
}

fn read(path: &Path) -> Option<(Stored, u64)> {
    let text = std::fs::read_to_string(path).ok()?;
    let bytes = text.len() as u64;
    Some((serde_json::from_str(&text).ok()?, bytes))
}

fn stored(dir: &Path, chat: &str) -> Vec<(Stored, u64)> {
    let Some(folder) = folder(dir, chat) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut out: Vec<(Stored, u64)> = entries.flatten().filter_map(|e| read(&e.path())).collect();
    out.sort_by(|a, b| a.0.added_at.cmp(&b.0.added_at).then(a.0.id.cmp(&b.0.id)));
    out
}

fn info((doc, bytes): &(Stored, u64)) -> Doc {
    Doc {
        id: doc.id.clone(),
        name: doc.name.clone(),
        chars: doc.text.chars().count(),
        bytes: *bytes,
        added_at: doc.added_at,
        excerpt: excerpt_of(&doc.text),
    }
}

fn excerpt_of(text: &str) -> String {
    let mut out = String::new();
    for line in text
        .lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
    {
        if line.is_empty() || line.starts_with("```") {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&line);
        if out.chars().count() >= EXCERPT_CHARS {
            break;
        }
    }
    out.chars().take(EXCERPT_CHARS).collect()
}

pub fn add(dir: &Path, chat: &str, name: &str, text: &str, now: DateTime<Utc>) -> Result<Doc> {
    let Some(folder) = folder(dir, chat) else {
        bail!("that is not a chat");
    };
    if stored(dir, chat).len() >= MOST_FILES {
        bail!("a chat holds up to {MOST_FILES} documents; remove one first");
    }
    std::fs::create_dir_all(&folder)?;
    let doc = Stored {
        id: uuid::Uuid::new_v4().simple().to_string(),
        name: name.chars().take(120).collect(),
        added_at: now,
        text: text.to_string(),
    };
    let json = serde_json::to_string(&doc)?;
    let path = folder.join(format!("{}.json", doc.id));
    std::fs::write(&path, &json)?;
    Ok(info(&(doc, json.len() as u64)))
}

pub fn list(dir: &Path, chat: &str) -> Vec<Doc> {
    stored(dir, chat).iter().map(info).collect()
}

pub fn texts(dir: &Path, chat: &str, ids: &[String]) -> Vec<(String, String)> {
    stored(dir, chat)
        .into_iter()
        .filter(|(doc, _)| ids.contains(&doc.id))
        .map(|(doc, _)| (doc.name, doc.text))
        .collect()
}

pub fn remove(dir: &Path, chat: &str, doc: &str) -> bool {
    valid_doc(doc)
        && folder(dir, chat)
            .is_some_and(|f| std::fs::remove_file(f.join(format!("{doc}.json"))).is_ok())
}

pub fn remove_all(dir: &Path, chat: &str) -> bool {
    folder(dir, chat).is_some_and(|f| f.is_dir() && std::fs::remove_dir_all(f).is_ok())
}

pub fn held(dir: &Path) -> Vec<Held> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<Held> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let chat = name.strip_suffix(".files")?.to_string();
            let docs = stored(dir, &chat);
            Some(Held {
                docs: docs.len(),
                bytes: docs.iter().map(|(_, b)| b).sum(),
                last: docs.iter().map(|(d, _)| d.added_at).max(),
                chat,
            })
        })
        .collect();
    out.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.chat.cmp(&b.chat)));
    out
}

pub fn tidy_orphans(dir: &Path, now: DateTime<Utc>) -> usize {
    let cutoff = now - chrono::Duration::hours(ORPHAN_HOURS);
    held(dir)
        .iter()
        .filter(|h| {
            !dir.join(format!("{}.json", h.chat)).exists()
                && h.last.is_none_or(|last| last < cutoff)
                && remove_all(dir, &h.chat)
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(hour: u32) -> DateTime<Utc> {
        chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 10, 8, hour, 0, 0).unwrap()
    }

    #[test]
    fn a_document_shows_its_first_lines_as_a_small_example() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let doc = add(
            &dir,
            "chat-xxxx-1",
            "w.pdf",
            "  Week 3\n\n```\nHeaps   keep\tthe minimum\n",
            at(1),
        )
        .unwrap();
        assert_eq!(doc.excerpt, "Week 3\nHeaps keep the minimum");
        let long = "word ".repeat(400);
        let doc = add(&dir, "chat-xxxx-1", "l.txt", &long, at(2)).unwrap();
        assert_eq!(doc.excerpt.chars().count(), EXCERPT_CHARS);
        assert_eq!(
            list(&dir, "chat-xxxx-1")[0].excerpt,
            "Week 3\nHeaps keep the minimum"
        );
    }

    #[test]
    fn documents_keep_only_their_text_and_belong_to_one_chat() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let a = add(
            &dir,
            "chat-aaaa-1",
            "slides.pdf",
            "Heaps keep the minimum at the root.",
            at(1),
        )
        .unwrap();
        let b = add(
            &dir,
            "chat-aaaa-1",
            "board.jpg",
            "Dijkstra uses a heap.",
            at(2),
        )
        .unwrap();
        add(&dir, "chat-bbbb-2", "other.txt", "elsewhere", at(3)).unwrap();
        assert_eq!(a.chars, 35);
        let listed = list(&dir, "chat-aaaa-1");
        assert_eq!(
            listed.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(),
            ["slides.pdf", "board.jpg"]
        );
        assert_eq!(
            texts(&dir, "chat-aaaa-1", std::slice::from_ref(&b.id)),
            [("board.jpg".to_string(), "Dijkstra uses a heap.".to_string())]
        );
        assert!(
            texts(&dir, "chat-bbbb-2", std::slice::from_ref(&b.id)).is_empty(),
            "another chat's document is not reachable"
        );
        assert!(remove(&dir, "chat-aaaa-1", &a.id));
        assert!(!remove(&dir, "chat-aaaa-1", "../../notes"));
        assert_eq!(list(&dir, "chat-aaaa-1").len(), 1);
        assert!(remove_all(&dir, "chat-aaaa-1"));
        assert!(list(&dir, "chat-aaaa-1").is_empty());
        assert!(add(&dir, "../escape", "x", "y", at(1)).is_err());
    }

    #[test]
    fn a_chat_holds_a_limited_number_of_documents() {
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..MOST_FILES {
            add(
                tmp.path(),
                "chat-full-0001",
                &format!("{i}.txt"),
                "x",
                at(1),
            )
            .unwrap();
        }
        let refused = add(tmp.path(), "chat-full-0001", "one more.txt", "x", at(1));
        assert!(refused.unwrap_err().to_string().contains("up to 10"));
    }

    #[test]
    fn documents_for_a_chat_that_never_started_are_cleared_after_a_day() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        add(dir, "chat-lost-0001", "a.txt", "x", at(1)).unwrap();
        add(dir, "chat-kept-0002", "b.txt", "x", at(1)).unwrap();
        std::fs::write(dir.join("chat-kept-0002.json"), "{}").unwrap();
        assert_eq!(tidy_orphans(dir, at(12)), 0, "not a day old yet");
        let tomorrow = at(1) + chrono::Duration::hours(25);
        assert_eq!(tidy_orphans(dir, tomorrow), 1);
        let left: Vec<String> = held(dir).into_iter().map(|h| h.chat).collect();
        assert_eq!(left, ["chat-kept-0002"]);
    }
}
