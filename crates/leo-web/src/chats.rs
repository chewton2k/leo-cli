use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
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

static WRITING: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn set_memory(dir: &Path, id: &str, memory: Memory) -> bool {
    let _held = WRITING.lock();
    let Some(mut chat) = load(dir, id) else {
        return false;
    };
    if chat
        .memory
        .as_ref()
        .is_some_and(|old| old.upto > memory.upto)
    {
        return false;
    }
    chat.memory = Some(memory);
    write(dir, &chat).is_ok()
}

pub fn remembered(dir: &Path) -> Vec<(String, String, usize, u64)> {
    list(dir)
        .into_iter()
        .filter_map(|summary| {
            let memory = load(dir, &summary.id)?.memory?;
            Some((
                summary.id,
                summary.title,
                memory.upto,
                memory.text.len() as u64,
            ))
        })
        .collect()
}

pub fn forget_memory(dir: &Path, id: &str) -> bool {
    let _held = WRITING.lock();
    let Some(mut chat) = load(dir, id).filter(|c| c.memory.is_some()) else {
        return false;
    };
    chat.memory = None;
    write(dir, &chat).is_ok()
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

fn path_of(dir: &Path, id: &str) -> Option<PathBuf> {
    valid_id(id).then(|| dir.join(format!("{id}.json")))
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
    let _held = WRITING.lock();
    let title = clipped(title, MOST_TITLE);
    let Some(mut chat) = load(dir, id).filter(|_| !title.is_empty()) else {
        return false;
    };
    chat.title = title;
    chat.named = true;
    write(dir, &chat).is_ok()
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
            load(dir, id).map(|chat| chat.summary())
        })
        .collect();
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(a.id.cmp(&b.id)));
    out
}

pub fn save(dir: &Path, id: &str, saving: Saving, now: DateTime<Utc>) -> Result<Chat> {
    path_of(dir, id).context("that is not a chat id")?;
    let _held = WRITING.lock();
    let old = load(dir, id);
    let created_at = old.as_ref().map_or(now, |old| old.created_at);
    let memory = old.as_ref().and_then(|old| old.memory.clone());
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
        updated_at: now,
    };
    write(dir, &chat)?;
    Ok(chat)
}

fn write(dir: &Path, chat: &Chat) -> Result<()> {
    let path = path_of(dir, &chat.id).context("that is not a chat id")?;
    std::fs::create_dir_all(dir).with_context(|| format!("could not make {}", dir.display()))?;
    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec_pretty(chat)?)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
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
        std::fs::write(dir.join("broken-chat-1.json"), "{not json").unwrap();
        assert_eq!(list(&dir).len(), 1);
    }
}
