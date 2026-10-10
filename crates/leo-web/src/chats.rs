use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const MOST_MESSAGES: usize = 400;
pub const MOST_TITLE: usize = 80;
pub const MOST_ABOUT: usize = 110;
const SHORT_QUESTION_WORDS: usize = 6;
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
    #[serde(default)]
    pub named: bool,
    #[serde(default)]
    pub about: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<Memory>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Memory {
    pub upto: usize,
    pub hash: String,
    pub text: String,
}

impl Chat {
    pub fn summary(&self) -> Summary {
        Summary {
            id: self.id.clone(),
            title: self.title.clone(),
            named: self.named,
            about: self.about.clone(),
            mode: self.mode.clone(),
            count: self.messages.len(),
            updated_at: self.updated_at,
        }
    }

    pub fn wants_name(&self) -> bool {
        !self.named && first_text(&self.messages, "assistant").is_some_and(|t| !t.trim().is_empty())
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Summary {
    pub id: String,
    pub title: String,
    pub named: bool,
    pub about: String,
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

fn first_of<'a>(messages: &'a [serde_json::Value], role: &str) -> Option<&'a serde_json::Value> {
    messages
        .iter()
        .find(|m| m.get("role").and_then(|r| r.as_str()) == Some(role))
}

fn first_text<'a>(messages: &'a [serde_json::Value], role: &str) -> Option<&'a str> {
    first_of(messages, role)
        .and_then(|m| m.get("text"))
        .and_then(|t| t.as_str())
}

fn clipped(text: &str, most: usize) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out: String = line.chars().take(most).collect();
    if line.chars().count() > most {
        out.push('…');
    }
    out
}

fn note_of(refs: &[Ref], messages: &[serde_json::Value]) -> Option<String> {
    let asked = first_of(messages, "user");
    let attached = asked
        .and_then(|m| m.get("refs"))
        .and_then(|r| r.as_array())
        .and_then(|r| r.first())
        .and_then(|r| r.get("title"))
        .and_then(|t| t.as_str())
        .map(str::to_string);
    let named = asked
        .and_then(|m| m.get("docs"))
        .and_then(|d| d.as_array())
        .and_then(|d| d.first())
        .and_then(|d| d.as_str())
        .map(str::to_string);
    let used = first_of(messages, "assistant")
        .and_then(|m| m.get("sources"))
        .and_then(|s| s.as_array())
        .and_then(|s| s.first())
        .and_then(|s| s.get("title"))
        .and_then(|t| t.as_str())
        .map(str::to_string);
    attached
        .or(named)
        .or_else(|| refs.first().map(|r| r.title.clone()))
        .or(used)
        .filter(|t| !t.trim().is_empty())
}

fn title_of(given: &str, refs: &[Ref], messages: &[serde_json::Value]) -> String {
    if !given.trim().is_empty() {
        return clipped(given, MOST_TITLE);
    }
    let asked = clipped(first_text(messages, "user").unwrap_or(""), MOST_TITLE);
    if asked.is_empty() {
        return "New chat".to_string();
    }
    let short = asked.split_whitespace().count() <= SHORT_QUESTION_WORDS;
    match note_of(refs, messages) {
        Some(note) if short && !asked.to_lowercase().contains(&note.to_lowercase()) => {
            let asked = asked.trim_end_matches(['?', '.', '!', ' ']);
            clipped(&format!("{asked}: {note}"), MOST_TITLE)
        }
        _ => asked,
    }
}

fn about_of(messages: &[serde_json::Value]) -> String {
    let said = first_text(messages, "assistant").unwrap_or("");
    let said = said.replace("[[correct]]", "").replace("[[incorrect]]", "");
    let mut fenced = false;
    let plain: String = said
        .lines()
        .filter(|l| {
            if l.trim_start().starts_with("```") {
                fenced = !fenced;
                return false;
            }
            !fenced
        })
        .map(|l| l.trim_start_matches(['#', '>', '-', '*', ' ', '|']))
        .filter(|l| !l.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let mut cleaned = String::with_capacity(plain.len());
    let mut chars = plain.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '[' && chars.peek() == Some(&'n') {
            let rest: String = chars.clone().take_while(|c| *c != ']').collect();
            if rest.len() > 1 && rest[1..].chars().all(|c| c.is_ascii_digit()) {
                for _ in 0..=rest.len() {
                    chars.next();
                }
                continue;
            }
        }
        if !matches!(c, '*' | '_' | '`') {
            cleaned.push(c);
        }
    }
    clipped(&cleaned.replace(" .", ".").replace(" ,", ","), MOST_ABOUT)
}

pub fn rename(dir: &Path, id: &str, title: &str) -> bool {
    let title = clipped(title, MOST_TITLE);
    if title.is_empty() || !valid_id(id) {
        return false;
    }
    db_of(dir)
        .with(|c| {
            let changed = c.execute(
                "UPDATE chats SET title = ?1, named = 1 WHERE id = ?2",
                rusqlite::params![title, id],
            )?;
            c.execute(
                "UPDATE chats_text SET title = ?1 WHERE id = ?2",
                rusqlite::params![title, id],
            )?;
            Ok(changed > 0)
        })
        .unwrap_or(false)
}

pub fn name_prompt(chat: &Chat) -> (String, String) {
    let asked = clipped(first_text(&chat.messages, "user").unwrap_or(""), 600);
    let answered = clipped(first_text(&chat.messages, "assistant").unwrap_or(""), 900);
    let system = "You name a conversation for a list of past chats. Reply with a title of 3 to 7 words that says what it is about, so it stands apart from other chats on similar subjects: name the topic, not the request (\"Dijkstra vs BFS on weighted graphs\", not \"Explain this simply\"). Use interpretable language. Plain text only: no quotes, no full stop, no preamble.".to_string();
    let user = format!(
        "<question>\n{asked}\n</question>\n<answer>\n{answered}\n</answer>\n\nWrite the title only."
    );
    (system, user)
}

pub fn clean_name(reply: &str) -> Option<String> {
    let line = reply.lines().map(str::trim).find(|l| !l.is_empty())?;
    let line = line
        .trim_start_matches(|c: char| c == '#' || c.is_whitespace())
        .trim_start_matches("Title:")
        .trim()
        .trim_matches(['"', '\'', '*', '“', '”', '`'])
        .trim_end_matches('.')
        .trim();
    let words = line.split_whitespace().count();
    (1..=12).contains(&words).then(|| clipped(line, MOST_TITLE))
}

fn stamp(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

fn when(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text).map_or_else(|_| Utc::now(), |t| t.with_timezone(&Utc))
}

static MIGRATED: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

pub(crate) fn db_of(dir: &Path) -> std::sync::Arc<crate::db::Db> {
    let db = crate::db::for_chats(dir);
    let mut done = MIGRATED.lock().unwrap_or_else(|e| e.into_inner());
    if !done.iter().any(|d| d == dir) {
        done.push(dir.to_path_buf());
        drop(done);
        bring_in_files(dir, &db);
    }
    db
}

fn bring_in_files(dir: &Path, db: &crate::db::Db) {
    if dir.file_name().is_none_or(|n| n != "chats") || !dir.is_dir() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut chats = Vec::new();
    let mut docs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if let Some(id) = name.strip_suffix(".json").filter(|id| valid_id(id)) {
            if let Some(chat) = std::fs::read_to_string(&path)
                .ok()
                .and_then(|t| serde_json::from_str::<Chat>(&t).ok())
                .filter(|c| c.id == id)
            {
                chats.push(chat);
            }
        } else if name.ends_with(".files") && path.is_dir() {
            docs.extend(crate::chat_files::from_folder(
                &path,
                name.trim_end_matches(".files"),
            ));
        }
    }
    let reviewed = crate::review::from_file(dir);
    let saved = db.with(|c| {
        let tx = c.transaction()?;
        for chat in &chats {
            if tx.query_row(
                "SELECT COUNT(*) FROM chats WHERE id = ?1",
                [&chat.id],
                |r| r.get::<_, i64>(0),
            )? == 0
            {
                write_in(&tx, chat)?;
            }
        }
        crate::chat_files::insert_all(&tx, &docs)?;
        crate::review::insert_all(&tx, &reviewed)?;
        tx.commit()
    });
    if saved.is_ok() {
        crate::db::put_aside(dir);
    }
}

fn body_of(chat: &Chat) -> String {
    chat.messages
        .iter()
        .filter_map(|m| m.get("text").and_then(|t| t.as_str()))
        .collect::<Vec<_>>()
        .join("\n")
}

fn json<T: Serialize + ?Sized>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".into())
}

fn write_in(c: &rusqlite::Connection, chat: &Chat) -> rusqlite::Result<()> {
    c.execute(
        "INSERT OR REPLACE INTO chats (id, title, mode, refs, messages, named, about, memory, count, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        rusqlite::params![
            chat.id,
            chat.title,
            chat.mode,
            json(&chat.refs),
            json(&chat.messages),
            chat.named,
            chat.about,
            chat.memory.as_ref().map(json),
            chat.messages.len() as i64,
            stamp(chat.created_at),
            stamp(chat.updated_at),
        ],
    )?;
    c.execute("DELETE FROM chats_text WHERE id = ?1", [&chat.id])?;
    c.execute(
        "INSERT INTO chats_text (id, title, body) VALUES (?1, ?2, ?3)",
        rusqlite::params![chat.id, chat.title, body_of(chat)],
    )?;
    Ok(())
}

fn chat_of(row: &rusqlite::Row) -> rusqlite::Result<Chat> {
    let text = |i: usize| -> rusqlite::Result<String> { row.get(i) };
    Ok(Chat {
        id: text(0)?,
        title: text(1)?,
        mode: text(2)?,
        refs: serde_json::from_str(&text(3)?).unwrap_or_default(),
        messages: serde_json::from_str(&text(4)?).unwrap_or_default(),
        named: row.get(5)?,
        about: text(6)?,
        memory: row
            .get::<_, Option<String>>(7)?
            .and_then(|m| serde_json::from_str(&m).ok()),
        created_at: when(&text(8)?),
        updated_at: when(&text(9)?),
    })
}

const CHAT_COLUMNS: &str =
    "id, title, mode, refs, messages, named, about, memory, created_at, updated_at";

fn load_in(c: &rusqlite::Connection, id: &str) -> rusqlite::Result<Option<Chat>> {
    use rusqlite::OptionalExtension;
    c.query_row(
        &format!("SELECT {CHAT_COLUMNS} FROM chats WHERE id = ?1"),
        [id],
        chat_of,
    )
    .optional()
}

pub fn load(dir: &Path, id: &str) -> Option<Chat> {
    if !valid_id(id) {
        return None;
    }
    db_of(dir).with(|c| load_in(c, id)).ok().flatten()
}

fn summary_of(row: &rusqlite::Row) -> rusqlite::Result<Summary> {
    Ok(Summary {
        id: row.get(0)?,
        title: row.get(1)?,
        named: row.get(2)?,
        about: row.get(3)?,
        mode: row.get(4)?,
        count: row.get::<_, i64>(5)? as usize,
        updated_at: when(&row.get::<_, String>(6)?),
    })
}

pub fn list(dir: &Path) -> Vec<Summary> {
    db_of(dir)
        .with(|c| {
            let mut found = c.prepare(
                "SELECT id, title, named, about, mode, count, updated_at FROM chats ORDER BY updated_at DESC, id",
            )?;
            let rows = found.query_map([], summary_of)?;
            rows.collect()
        })
        .unwrap_or_default()
}

pub fn search(dir: &Path, query: &str) -> Vec<Summary> {
    let words: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .take(12)
        .map(|w| format!("\"{w}\"*"))
        .collect();
    if words.is_empty() {
        return list(dir);
    }
    let matching = words.join(" ");
    db_of(dir)
        .with(|c| {
            let mut found = c.prepare(
                "SELECT c.id, c.title, c.named, c.about, c.mode, c.count, c.updated_at
                 FROM chats_text t JOIN chats c ON c.id = t.id
                 WHERE chats_text MATCH ?1 ORDER BY bm25(chats_text, 0.0, 4.0, 1.0), c.updated_at DESC LIMIT 100",
            )?;
            let rows = found.query_map([&matching], summary_of)?;
            rows.collect()
        })
        .unwrap_or_default()
}

pub fn save(dir: &Path, id: &str, saving: Saving, now: DateTime<Utc>) -> Result<Chat> {
    anyhow::ensure!(valid_id(id), "that is not a chat id");
    let db = db_of(dir);
    db.with(|c| {
        let tx = c.transaction()?;
        let old = load_in(&tx, id)?;
        let created_at = old.as_ref().map_or(now, |old| old.created_at);
        let memory = old.as_ref().and_then(|old| old.memory.clone());
        let engaged = old.as_ref().is_none_or(|old| {
            saving.messages.len() > old.messages.len()
                || (saving.messages.len() >= MOST_MESSAGES
                    && saving.messages.last() != old.messages.last())
        });
        let updated_at = match &old {
            Some(old) if !engaged => old.updated_at,
            _ => now,
        };
        let kept_name = old
            .filter(|old| old.named && saving.title.trim().is_empty())
            .map(|old| old.title);
        let mut messages = saving.messages;
        if messages.len() > MOST_MESSAGES {
            messages.drain(..messages.len() - MOST_MESSAGES);
        }
        let refs: Vec<Ref> = saving
            .refs
            .into_iter()
            .take(crate::chat::MOST_ATTACHED)
            .collect();
        let chat = Chat {
            id: id.to_string(),
            named: kept_name.is_some(),
            title: kept_name.unwrap_or_else(|| title_of(&saving.title, &refs, &messages)),
            about: about_of(&messages),
            memory,
            mode: saving.mode,
            refs,
            messages,
            created_at,
            updated_at,
        };
        write_in(&tx, &chat)?;
        tx.commit()?;
        Ok(chat)
    })
}

pub fn set_memory(dir: &Path, id: &str, memory: Memory) -> bool {
    let json = serde_json::to_string(&memory).unwrap_or_default();
    db_of(dir)
        .with(|c| {
            let tx = c.transaction()?;
            let Some(chat) = load_in(&tx, id)? else {
                return Ok(false);
            };
            if chat
                .memory
                .as_ref()
                .is_some_and(|old| old.upto > memory.upto)
            {
                return Ok(false);
            }
            tx.execute(
                "UPDATE chats SET memory = ?1 WHERE id = ?2",
                rusqlite::params![json, id],
            )?;
            tx.commit()?;
            Ok(true)
        })
        .unwrap_or(false)
}

pub fn remembered(dir: &Path) -> Vec<(String, String, usize, u64)> {
    db_of(dir)
        .with(|c| {
            let mut found = c.prepare(
                "SELECT id, title, memory FROM chats WHERE memory IS NOT NULL ORDER BY updated_at DESC, id",
            )?;
            let rows = found.query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
            })?;
            let rows: Vec<(String, String, String)> = rows.collect::<rusqlite::Result<_>>()?;
            Ok(rows
                .into_iter()
                .filter_map(|(id, title, memory)| {
                    let memory: Memory = serde_json::from_str(&memory).ok()?;
                    Some((id, title, memory.upto, memory.text.len() as u64))
                })
                .collect())
        })
        .unwrap_or_default()
}

pub fn forget_memory(dir: &Path, id: &str) -> bool {
    db_of(dir)
        .with(|c| {
            c.execute(
                "UPDATE chats SET memory = NULL WHERE id = ?1 AND memory IS NOT NULL",
                [id],
            )
        })
        .is_ok_and(|n| n > 0)
}

pub fn bytes_of(dir: &Path, id: &str) -> u64 {
    db_of(dir)
        .with(|c| {
            c.query_row(
                "SELECT length(messages) + length(refs) + length(title) + coalesce(length(memory), 0) FROM chats WHERE id = ?1",
                [id],
                |r| r.get::<_, i64>(0),
            )
        })
        .map_or(0, |n| n.max(0) as u64)
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
    db_of(dir)
        .with(|c| {
            let tx = c.transaction()?;
            let gone = tx.execute("DELETE FROM chats WHERE id = ?1", [id])?;
            tx.execute("DELETE FROM chats_text WHERE id = ?1", [id])?;
            tx.execute("DELETE FROM chat_docs WHERE chat = ?1", [id])?;
            tx.commit()?;
            Ok(gone > 0)
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chat_moves_up_only_when_a_message_is_added() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let early = chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 10, 7, 9, 0, 0).unwrap();
        let later = early + chrono::Duration::hours(3);
        let one = vec![serde_json::json!({ "role": "user", "text": "hi" })];
        save(
            &dir,
            "chat-order-0001",
            Saving {
                title: String::new(),
                mode: "chat".into(),
                refs: vec![],
                messages: one.clone(),
            },
            early,
        )
        .unwrap();
        let switched = save(
            &dir,
            "chat-order-0001",
            Saving {
                title: String::new(),
                mode: "study".into(),
                refs: vec![],
                messages: one.clone(),
            },
            later,
        )
        .unwrap();
        assert_eq!(switched.mode, "study");
        assert_eq!(
            switched.updated_at, early,
            "switching style is not activity"
        );
        let mut two = one;
        two.push(serde_json::json!({ "role": "assistant", "text": "hello" }));
        let answered = save(
            &dir,
            "chat-order-0001",
            Saving {
                title: String::new(),
                mode: "study".into(),
                refs: vec![],
                messages: two,
            },
            later,
        )
        .unwrap();
        assert_eq!(answered.updated_at, later);
    }

    fn said(role: &str, text: &str) -> serde_json::Value {
        serde_json::json!({ "role": role, "text": text })
    }

    fn at(minute: u32) -> DateTime<Utc> {
        chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 10, 7, 12, minute, 0).unwrap()
    }

    fn saving(messages: Vec<serde_json::Value>) -> Saving {
        Saving {
            title: String::new(),
            mode: "chat".into(),
            refs: vec![],
            messages,
        }
    }

    #[test]
    fn a_short_starter_question_is_told_apart_by_the_note_it_was_about() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let asked = serde_json::json!({ "role": "user", "text": "Explain this simply", "refs": [{ "id": "n1", "title": "Graph traversals" }] });
        let chat = save(
            &dir,
            "chat-cccc-1",
            saving(vec![asked, said("assistant", "Sure")]),
            at(1),
        )
        .unwrap();
        assert_eq!(chat.title, "Explain this simply: Graph traversals");

        let answered = serde_json::json!({ "role": "assistant", "text": "x", "sources": [{ "id": "a", "title": "Heaps" }] });
        let chat = save(
            &dir,
            "chat-cccc-2",
            saving(vec![said("user", "What are the key ideas here?"), answered]),
            at(1),
        )
        .unwrap();
        assert_eq!(chat.title, "What are the key ideas here: Heaps");

        let chat = save(
            &dir,
            "chat-cccc-3",
            saving(vec![said("user", "quiz me on heaps"), answered_by("Heaps")]),
            at(1),
        )
        .unwrap();
        assert_eq!(chat.title, "quiz me on heaps");
        let long = "why does breadth first search need a queue and not a stack here";
        let chat = save(
            &dir,
            "chat-cccc-4",
            saving(vec![said("user", long), answered_by("Heaps")]),
            at(1),
        )
        .unwrap();
        assert_eq!(chat.title, long);
    }

    fn answered_by(title: &str) -> serde_json::Value {
        serde_json::json!({ "role": "assistant", "text": "x", "sources": [{ "id": "a", "title": title }] })
    }

    #[test]
    fn the_description_is_the_start_of_felixs_first_answer_in_plain_words() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let reply = "[[correct]] ## Breadth-first search\n\n- **BFS** visits a graph level by level [n1], using a `queue` [n12].\n\n```\ncode\n```";
        let chat = save(
            &dir,
            "chat-dddd-1",
            saving(vec![said("user", "q"), said("assistant", reply)]),
            at(1),
        )
        .unwrap();
        assert_eq!(
            chat.about,
            "Breadth-first search BFS visits a graph level by level, using a queue."
        );
        assert_eq!(list(&dir)[0].about, chat.about);
        let long = "word ".repeat(80);
        let chat = save(
            &dir,
            "chat-dddd-2",
            saving(vec![said("user", "q"), said("assistant", &long)]),
            at(1),
        )
        .unwrap();
        assert!(chat.about.ends_with('…') && chat.about.chars().count() <= MOST_ABOUT + 1);
    }

    #[test]
    fn a_name_given_by_the_ai_survives_later_saves_and_is_asked_for_once() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("chats");
        let first = save(
            &dir,
            "chat-eeee-1",
            saving(vec![said("user", "help")]),
            at(1),
        )
        .unwrap();
        assert!(!first.wants_name());
        let answered = save(
            &dir,
            "chat-eeee-1",
            saving(vec![
                said("user", "help"),
                said("assistant", "BFS uses a queue."),
            ]),
            at(2),
        )
        .unwrap();
        assert!(answered.wants_name());
        let (system, user) = name_prompt(&answered);
        assert!(system.contains("Use interpretable language"));
        assert!(user.contains("BFS uses a queue."));

        assert!(rename(
            &dir,
            "chat-eeee-1",
            "Queues in breadth-first search"
        ));
        let later = save(
            &dir,
            "chat-eeee-1",
            saving(vec![
                said("user", "help"),
                said("assistant", "BFS uses a queue."),
                said("user", "more"),
            ]),
            at(3),
        )
        .unwrap();
        assert_eq!(later.title, "Queues in breadth-first search");
        assert!(!later.wants_name());
        assert!(!rename(&dir, "chat-nope-1", "x"));
        assert!(!rename(&dir, "chat-eeee-1", "   "));
    }

    #[test]
    fn an_ai_reply_is_trimmed_to_a_title_or_refused() {
        assert_eq!(
            clean_name("\"Heaps and priority queues.\"\n").as_deref(),
            Some("Heaps and priority queues")
        );
        assert_eq!(
            clean_name("Title: **BFS vs DFS**").as_deref(),
            Some("BFS vs DFS")
        );
        assert_eq!(
            clean_name("# Graph search\nmore").as_deref(),
            Some("Graph search")
        );
        assert_eq!(clean_name(""), None);
        assert_eq!(clean_name(&"word ".repeat(30)), None);
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
        assert!(save(&dir, "../outside", saving(vec![]), at(0)).is_err());
        assert_eq!(list(&dir).len(), 1);
    }
}
