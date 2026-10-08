use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::chats::{self, Chat, Ref};

pub const DUE_AFTER_HOURS: i64 = 20;
pub const MOST_MISSED: usize = 20;
const MOST_REMEMBERED: usize = 2000;
const FILE: &str = "review.json";

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Missed {
    pub key: String,
    pub chat: String,
    pub question: String,
    pub answer: String,
    pub notes: Vec<Ref>,
    pub at: DateTime<Utc>,
    pub due: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Reviewed {
    #[serde(default)]
    reviewed: BTreeSet<String>,
}

fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

pub fn key_of(chat: &str, reply: &str) -> String {
    format!("{chat}:{:016x}", fnv(reply))
}

pub fn valid_key(key: &str) -> bool {
    key.rsplit_once(':').is_some_and(|(chat, hash)| {
        chats::valid_id(chat) && hash.len() == 16 && hash.chars().all(|c| c.is_ascii_hexdigit())
    })
}

fn text_of(message: &serde_json::Value) -> &str {
    message.get("text").and_then(|t| t.as_str()).unwrap_or("")
}

fn role_of(message: &serde_json::Value) -> &str {
    message.get("role").and_then(|r| r.as_str()).unwrap_or("")
}

fn verdict_off(text: &str) -> Option<(&str, &str)> {
    let rest = text.trim_start();
    for verdict in ["[[correct]]", "[[incorrect]]"] {
        if rest
            .get(..verdict.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(verdict))
        {
            return Some((
                &verdict[2..verdict.len() - 2],
                rest[verdict.len()..].trim_start(),
            ));
        }
    }
    None
}

fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("[n") {
        let (before, tail) = rest.split_at(start);
        out.push_str(before);
        match tail.find(']') {
            Some(end)
                if tail[2..end]
                    .chars()
                    .all(|c| c.is_ascii_digit() || c == ',' || c == ' ' || c == 'n') =>
            {
                rest = &tail[end + 1..];
            }
            _ => {
                out.push_str("[n");
                rest = &tail[2..];
            }
        }
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn clip(text: &str, most: usize) -> String {
    let mut out: String = text.chars().take(most).collect();
    if text.chars().count() > most {
        out.push('…');
    }
    out
}

fn question_in(text: &str) -> String {
    let text = verdict_off(text).map_or(text, |(_, rest)| rest);
    let paragraphs: Vec<&str> = text
        .split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    let asked = paragraphs
        .iter()
        .rev()
        .find(|p| p.contains('?'))
        .or(paragraphs.last())
        .copied()
        .unwrap_or("");
    clip(&plain(asked), 300)
}

fn cited_in(message: &serde_json::Value) -> Vec<Ref> {
    let text = text_of(message);
    let sources = message
        .get("sources")
        .and_then(|s| s.as_array())
        .cloned()
        .unwrap_or_default();
    let mut out: Vec<Ref> = Vec::new();
    for source in &sources {
        let (Some(n), Some(id), Some(title)) = (
            source.get("n").and_then(|n| n.as_u64()),
            source.get("id").and_then(|i| i.as_str()),
            source.get("title").and_then(|t| t.as_str()),
        ) else {
            continue;
        };
        let tag = format!("n{n}");
        let used = text.match_indices(&tag).any(|(at, _)| {
            let after = text[at + tag.len()..].chars().next();
            !after.is_some_and(|c| c.is_ascii_digit())
        });
        if used && !out.iter().any(|r| r.id == id) {
            out.push(Ref {
                id: id.to_string(),
                title: title.to_string(),
            });
        }
    }
    out.truncate(3);
    out
}

fn missed_in(chat: &Chat, now: DateTime<Utc>) -> Vec<Missed> {
    let mut out = Vec::new();
    for (i, reply) in chat.messages.iter().enumerate() {
        if role_of(reply) != "assistant" {
            continue;
        }
        let text = text_of(reply);
        if !matches!(verdict_off(text), Some(("incorrect", _))) {
            continue;
        }
        let answer = i
            .checked_sub(1)
            .map(|j| &chat.messages[j])
            .filter(|m| role_of(m) == "user")
            .map(|m| clip(&plain(text_of(m)), 200))
            .unwrap_or_default();
        let asked = i
            .checked_sub(2)
            .map(|j| &chat.messages[j])
            .filter(|m| role_of(m) == "assistant");
        let question = asked.map(|m| question_in(text_of(m))).unwrap_or_default();
        let mut notes = cited_in(reply);
        if notes.is_empty() {
            notes = asked.map(cited_in).unwrap_or_default();
        }
        let at = reply
            .get("at")
            .and_then(|a| a.as_str())
            .and_then(|a| DateTime::parse_from_rfc3339(a).ok())
            .map_or(chat.updated_at, |a| a.with_timezone(&Utc));
        out.push(Missed {
            key: key_of(&chat.id, text),
            chat: chat.id.clone(),
            question,
            answer,
            notes,
            due: at <= now - Duration::hours(DUE_AFTER_HOURS),
            at,
        });
    }
    out
}

fn reviewed(dir: &Path) -> Reviewed {
    std::fs::read_to_string(dir.join(FILE))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn missed(dir: &Path, now: DateTime<Utc>) -> Vec<Missed> {
    let done = reviewed(dir).reviewed;
    let mut out: Vec<Missed> = chats::list(dir)
        .iter()
        .filter_map(|summary| chats::load(dir, &summary.id))
        .flat_map(|chat| missed_in(&chat, now))
        .filter(|m| !done.contains(&m.key) && !m.question.is_empty())
        .collect();
    out.sort_by(|a, b| a.at.cmp(&b.at).then(a.key.cmp(&b.key)));
    let mut seen = BTreeSet::new();
    out.retain(|m| seen.insert(m.question.to_lowercase()));
    out.truncate(MOST_MISSED);
    out
}

pub fn mark_reviewed(dir: &Path, keys: &[String]) -> Result<usize> {
    let mut held = reviewed(dir);
    let before = held.reviewed.len();
    held.reviewed
        .extend(keys.iter().filter(|k| valid_key(k)).cloned());
    let added = held.reviewed.len() - before;
    held.reviewed.retain(|key| {
        key.rsplit_once(':')
            .is_some_and(|(chat, _)| dir.join(format!("{chat}.json")).exists())
    });
    while held.reviewed.len() > MOST_REMEMBERED {
        let first = held.reviewed.iter().next().cloned();
        if let Some(first) = first {
            held.reviewed.remove(&first);
        }
    }
    std::fs::create_dir_all(dir)?;
    let path = dir.join(FILE);
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(&held)?)?;
    std::fs::rename(&tmp, &path)?;
    Ok(added)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn said(role: &str, text: &str) -> serde_json::Value {
        serde_json::json!({ "role": role, "text": text })
    }

    fn at(day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, day, hour, 0, 0).unwrap()
    }

    fn study(dir: &Path, id: &str, messages: Vec<serde_json::Value>, when: DateTime<Utc>) {
        chats::save(
            dir,
            id,
            chats::Saving {
                title: String::new(),
                mode: "study".into(),
                refs: vec![],
                messages,
            },
            when,
        )
        .unwrap();
    }

    fn quiz() -> Vec<serde_json::Value> {
        vec![
            said("user", "quiz me on graphs"),
            serde_json::json!({
                "role": "assistant",
                "text": "Let's start.\n\nWhich data structure does BFS use to pick the next vertex? [n1]",
                "sources": [{"n": 1, "id": "note-graphs", "title": "Graph traversals"}],
            }),
            said("user", "a stack"),
            serde_json::json!({
                "role": "assistant",
                "text": "[[incorrect]] Not quite: BFS uses a queue [n2]. Next: what does DFS use?",
                "sources": [{"n": 2, "id": "note-queues", "title": "Queues"}],
                "at": "2026-10-05T09:00:00Z",
            }),
            said("user", "a stack"),
            said("assistant", "[[correct]] Yes. (Score: 1/2)"),
        ]
    }

    #[test]
    fn a_wrong_answer_is_remembered_with_its_question_answer_and_notes() {
        let dir = tempfile::tempdir().unwrap();
        study(dir.path(), "chat-00000001", quiz(), at(5, 9));
        let missed = missed(dir.path(), at(6, 12));
        assert_eq!(missed.len(), 1);
        let one = &missed[0];
        assert_eq!(
            one.question,
            "Which data structure does BFS use to pick the next vertex?"
        );
        assert_eq!(one.answer, "a stack");
        assert_eq!(
            one.notes,
            vec![Ref {
                id: "note-queues".into(),
                title: "Queues".into()
            }]
        );
        assert_eq!(one.at, at(5, 9));
        assert!(one.due);
        assert!(valid_key(&one.key));
        assert!(
            !missed_in(
                &chats::load(dir.path(), "chat-00000001").unwrap(),
                at(5, 10)
            )[0]
            .due
        );
    }

    #[test]
    fn a_reviewed_question_is_not_suggested_again_and_deleted_chats_are_forgotten() {
        let dir = tempfile::tempdir().unwrap();
        study(dir.path(), "chat-00000001", quiz(), at(5, 9));
        let key = missed(dir.path(), at(6, 12))[0].key.clone();
        assert_eq!(
            mark_reviewed(dir.path(), &[key.clone(), "../evil:0".into()]).unwrap(),
            1
        );
        assert!(missed(dir.path(), at(6, 12)).is_empty());
        assert!(chats::list(dir.path()).iter().all(|c| c.id != "review"));
        chats::remove(dir.path(), "chat-00000001");
        mark_reviewed(dir.path(), &[]).unwrap();
        assert!(reviewed(dir.path()).reviewed.is_empty());
    }

    #[test]
    fn citations_are_dropped_and_old_replies_fall_back_to_the_chat_time() {
        assert_eq!(
            plain("a queue [n2, n3] and [note] here"),
            "a queue and [note] here"
        );
        assert_eq!(
            question_in("[[correct]] Yes.\n\nWhat is a heap? [n1]"),
            "What is a heap?"
        );
        assert_eq!(question_in("Explain heaps."), "Explain heaps.");
        assert_eq!(verdict_off("ü"), None);
        let dir = tempfile::tempdir().unwrap();
        let mut messages = quiz();
        messages[3].as_object_mut().unwrap().remove("at");
        study(dir.path(), "chat-00000002", messages, at(7, 8));
        assert_eq!(missed(dir.path(), at(7, 9))[0].at, at(7, 8));
        assert!(!missed(dir.path(), at(7, 9))[0].due);
        assert!(!valid_key("chat-00000002:xyz"));
        assert_eq!(key_of("chat-00000002", "a"), key_of("chat-00000002", "a"));
    }
}
