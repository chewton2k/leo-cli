use std::path::Path;

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

fn valid_doc(id: &str) -> bool {
    id.len() == 32 && id.chars().all(|c| c.is_ascii_hexdigit())
}

pub(crate) struct Imported {
    chat: String,
    doc: Stored,
}

pub(crate) fn from_folder(folder: &Path, chat: &str) -> Vec<Imported> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|text| serde_json::from_str::<Stored>(&text).ok())
        .filter(|doc| valid_doc(&doc.id) && valid_id(chat))
        .map(|doc| Imported {
            chat: chat.to_string(),
            doc,
        })
        .collect()
}

fn insert(c: &rusqlite::Connection, chat: &str, doc: &Stored) -> rusqlite::Result<usize> {
    c.execute(
        "INSERT OR IGNORE INTO chat_docs (id, chat, name, added_at, text) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![
            doc.id,
            chat,
            doc.name,
            doc.added_at.to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
            doc.text
        ],
    )
}

pub(crate) fn insert_all(c: &rusqlite::Connection, docs: &[Imported]) -> rusqlite::Result<()> {
    for item in docs {
        insert(c, &item.chat, &item.doc)?;
    }
    Ok(())
}

fn stored(dir: &Path, chat: &str) -> Vec<(Stored, u64)> {
    if !valid_id(chat) {
        return Vec::new();
    }
    crate::chats::db_of(dir)
        .with(|c| {
            let mut found = c.prepare(
                "SELECT id, name, added_at, text FROM chat_docs WHERE chat = ?1 ORDER BY added_at, id",
            )?;
            let rows = found.query_map([chat], |r| {
                let text: String = r.get(3)?;
                let name: String = r.get(1)?;
                let bytes = (text.len() + name.len()) as u64;
                Ok((
                    Stored {
                        id: r.get(0)?,
                        name,
                        added_at: DateTime::parse_from_rfc3339(&r.get::<_, String>(2)?)
                            .map_or_else(|_| Utc::now(), |t| t.with_timezone(&Utc)),
                        text,
                    },
                    bytes,
                ))
            })?;
            rows.collect()
        })
        .unwrap_or_default()
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
    if !valid_id(chat) {
        bail!("that is not a chat");
    }
    if stored(dir, chat).len() >= MOST_FILES {
        bail!("a chat holds up to {MOST_FILES} documents; remove one first");
    }
    let doc = Stored {
        id: uuid::Uuid::new_v4().simple().to_string(),
        name: name.chars().take(120).collect(),
        added_at: now,
        text: text.to_string(),
    };
    crate::chats::db_of(dir).with(|c| insert(c, chat, &doc))?;
    let bytes = (doc.text.len() + doc.name.len()) as u64;
    Ok(info(&(doc, bytes)))
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
        && crate::chats::db_of(dir)
            .with(|c| {
                c.execute(
                    "DELETE FROM chat_docs WHERE id = ?1 AND chat = ?2",
                    [doc, chat],
                )
            })
            .is_ok_and(|n| n > 0)
}

pub fn remove_all(dir: &Path, chat: &str) -> bool {
    crate::chats::db_of(dir)
        .with(|c| c.execute("DELETE FROM chat_docs WHERE chat = ?1", [chat]))
        .is_ok_and(|n| n > 0)
}

pub fn held(dir: &Path) -> Vec<Held> {
    crate::chats::db_of(dir)
        .with(|c| {
            let mut found = c.prepare(
                "SELECT chat, COUNT(*), SUM(length(text) + length(name)), MAX(added_at)
                 FROM chat_docs GROUP BY chat ORDER BY SUM(length(text) + length(name)) DESC, chat",
            )?;
            let rows = found.query_map([], |r| {
                Ok(Held {
                    chat: r.get(0)?,
                    docs: r.get::<_, i64>(1)? as usize,
                    bytes: r.get::<_, i64>(2)?.max(0) as u64,
                    last: r
                        .get::<_, Option<String>>(3)?
                        .and_then(|t| DateTime::parse_from_rfc3339(&t).ok())
                        .map(|t| t.with_timezone(&Utc)),
                })
            })?;
            rows.collect()
        })
        .unwrap_or_default()
}

pub fn tidy_orphans(dir: &Path, now: DateTime<Utc>) -> usize {
    let cutoff = now - chrono::Duration::hours(ORPHAN_HOURS);
    held(dir)
        .iter()
        .filter(|h| {
            crate::chats::load(dir, &h.chat).is_none()
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
        crate::chats::save(
            dir,
            "chat-kept-0002",
            serde_json::from_value(serde_json::json!({ "mode": "chat", "messages": [] })).unwrap(),
            at(1),
        )
        .unwrap();
        assert_eq!(tidy_orphans(dir, at(12)), 0, "not a day old yet");
        let tomorrow = at(1) + chrono::Duration::hours(25);
        assert_eq!(tidy_orphans(dir, tomorrow), 1);
        let left: Vec<String> = held(dir).into_iter().map(|h| h.chat).collect();
        assert_eq!(left, ["chat-kept-0002"]);
    }
}
