use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const MOST_MESSAGES: usize = 400;
pub const MOST_TITLE: usize = 80;
pub const CHAT_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Ref {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chat {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub refs: Vec<Ref>,
    #[serde(default)]
    pub messages: Vec<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Summary {
    pub id: String,
    pub title: String,
    pub mode: String,
    pub count: usize,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Saving {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub refs: Vec<Ref>,
    #[serde(default)]
    pub messages: Vec<serde_json::Value>,
}

pub fn dir_for(notes_dir: &Path) -> PathBuf {
    notes_dir.parent().unwrap_or(notes_dir).join("chats")
}

pub fn valid_id(id: &str) -> bool {
    (8..=64).contains(&id.len()) && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn path_of(dir: &Path, id: &str) -> Option<PathBuf> {
    valid_id(id).then(|| dir.join(format!("{id}.json")))
}

fn title_of(given: &str, messages: &[serde_json::Value]) -> String {
    let first = messages
        .iter()
        .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))
        .and_then(|m| m.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("");
    let picked = if given.trim().is_empty() {
        first
    } else {
        given
    };
    let line = picked.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut title: String = line.chars().take(MOST_TITLE).collect();
    if line.chars().count() > MOST_TITLE {
        title.push('…');
    }
    if title.is_empty() {
        "New chat".to_string()
    } else {
        title
    }
}

pub fn load(dir: &Path, id: &str) -> Option<Chat> {
    let path = path_of(dir, id)?;
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn list(dir: &Path) -> Vec<Summary> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<Summary> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let id = name.strip_suffix(".json")?;
            let chat = load(dir, id)?;
            Some(Summary {
                id: chat.id,
                title: chat.title,
                mode: chat.mode,
                count: chat.messages.len(),
                updated_at: chat.updated_at,
            })
        })
        .collect();
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(a.id.cmp(&b.id)));
    out
}

pub fn save(dir: &Path, id: &str, saving: Saving, now: DateTime<Utc>) -> Result<Chat> {
    let path = path_of(dir, id).context("that is not a chat id")?;
    let created_at = load(dir, id).map_or(now, |old| old.created_at);
    let mut messages = saving.messages;
    if messages.len() > MOST_MESSAGES {
        messages.drain(..messages.len() - MOST_MESSAGES);
    }
    let chat = Chat {
        id: id.to_string(),
        title: title_of(&saving.title, &messages),
        mode: saving.mode,
        refs: saving
            .refs
            .into_iter()
            .take(crate::chat::MOST_ATTACHED)
            .collect(),
        messages,
        created_at,
        updated_at: now,
    };
    std::fs::create_dir_all(dir).with_context(|| format!("could not make {}", dir.display()))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(&chat)?)?;
    std::fs::rename(&tmp, &path)?;
    Ok(chat)
}

pub fn tidy(dir: &Path, days: Option<u32>, now: DateTime<Utc>) -> usize {
    let Some(days) = days else {
        return 0;
    };
    let cutoff = now - chrono::Duration::days(i64::from(days));
    list(dir)
        .iter()
        .filter(|c| c.updated_at < cutoff && remove(dir, &c.id))
        .count()
}

pub fn remove(dir: &Path, id: &str) -> bool {
    let gone = path_of(dir, id).is_some_and(|p| std::fs::remove_file(p).is_ok());
    crate::chat_files::remove_all(dir, id);
    gone
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(role: &str, text: &str) -> serde_json::Value {
        serde_json::json!({ "role": role, "text": text })
    }

    fn at(minute: u32) -> DateTime<Utc> {
        chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 10, 7, 12, minute, 0).unwrap()
    }

    #[test]
    fn a_chat_is_kept_on_disk_named_after_its_first_question_and_listed_newest_first() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let first = save(
            &dir,
            "chat-aaaa-1",
            Saving {
                title: String::new(),
                mode: "study".into(),
                refs: vec![Ref {
                    id: "n1".into(),
                    title: "Heaps".into(),
                }],
                messages: vec![
                    said("user", "  quiz me\non   heaps "),
                    said("assistant", "Sure"),
                ],
            },
            at(1),
        )
        .unwrap();
        assert_eq!(first.title, "quiz me on heaps");
        save(
            &dir,
            "chat-bbbb-2",
            Saving {
                title: String::new(),
                mode: "chat".into(),
                refs: vec![],
                messages: vec![],
            },
            at(2),
        )
        .unwrap();
        let again = save(
            &dir,
            "chat-aaaa-1",
            Saving {
                title: String::new(),
                mode: "study".into(),
                refs: first.refs.clone(),
                messages: vec![
                    said("user", "quiz me on heaps"),
                    said("assistant", "Sure"),
                    said("user", "the root"),
                ],
            },
            at(3),
        )
        .unwrap();
        assert_eq!(again.created_at, at(1));
        assert_eq!(again.updated_at, at(3));

        let listed = list(&dir);
        assert_eq!(
            listed.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            ["chat-aaaa-1", "chat-bbbb-2"]
        );
        assert_eq!(listed[0].count, 3);
        assert_eq!(listed[1].title, "New chat");
        assert_eq!(load(&dir, "chat-aaaa-1").unwrap().refs, first.refs);

        assert!(remove(&dir, "chat-bbbb-2"));
        assert!(!remove(&dir, "chat-bbbb-2"));
        assert_eq!(list(&dir).len(), 1);
    }

    #[test]
    fn old_chats_leave_when_a_limit_is_set_and_stay_forever_otherwise() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let empty = || Saving {
            title: String::new(),
            mode: String::new(),
            refs: vec![],
            messages: vec![said("user", "hi")],
        };
        save(&dir, "chat-old-0001", empty(), at(0)).unwrap();
        let later = at(0) + chrono::Duration::days(40);
        save(&dir, "chat-new-0002", empty(), later).unwrap();
        assert_eq!(tidy(&dir, None, later), 0);
        assert_eq!(list(&dir).len(), 2);
        assert_eq!(tidy(&dir, Some(30), later), 1);
        let left: Vec<String> = list(&dir).into_iter().map(|c| c.id).collect();
        assert_eq!(left, ["chat-new-0002"]);
    }

    #[test]
    fn ids_cannot_reach_outside_the_folder_and_long_chats_keep_their_end() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        for bad in [
            "../../notes/x",
            "short",
            "a/b/c/d/e/f",
            "chat id with spaces",
            "",
        ] {
            assert!(!valid_id(bad), "{bad}");
            assert!(save(
                &dir,
                bad,
                Saving {
                    title: String::new(),
                    mode: String::new(),
                    refs: vec![],
                    messages: vec![]
                },
                at(0)
            )
            .is_err());
            assert!(load(&dir, bad).is_none());
            assert!(!remove(&dir, bad));
        }
        let messages: Vec<_> = (0..MOST_MESSAGES + 10)
            .map(|i| said("user", &format!("m{i}")))
            .collect();
        let long = "word ".repeat(100);
        let chat = save(
            &dir,
            "chat-long-123",
            Saving {
                title: long,
                mode: String::new(),
                refs: vec![],
                messages,
            },
            at(0),
        )
        .unwrap();
        assert_eq!(chat.messages.len(), MOST_MESSAGES);
        assert_eq!(chat.messages[0]["text"], "m10");
        assert_eq!(chat.title.chars().count(), MOST_TITLE + 1);
        assert!(chat.title.ends_with('…'));
        std::fs::write(dir.join("broken-chat-1.json"), "{not json").unwrap();
        assert_eq!(list(&dir).len(), 1);
    }
}
