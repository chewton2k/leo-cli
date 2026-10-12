use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use leo_core::notes::checkbox_line;
use leo_core::store::Store;
use leo_core::tasks::{self as core, Located};
use serde::{Deserialize, Serialize};

pub const MOST_TASKS: usize = 10_000;
pub const MOST_TITLE: usize = 300;
pub const MOST_LISTED: usize = 500;
pub const KINDS: [&str; 6] = [
    "assignment",
    "exam",
    "meeting",
    "follow-up",
    "deadline",
    "other",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Task {
    pub id: String,
    pub title: String,
    #[serde(default = "other")]
    pub kind: String,
    #[serde(default)]
    pub done: bool,
    #[serde(default)]
    pub date: Option<NaiveDate>,
    #[serde(default)]
    pub time: Option<NaiveTime>,
    #[serde(default)]
    pub end: Option<NaiveTime>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub sent_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

fn other() -> String {
    "other".into()
}

#[derive(Debug, Clone, Default)]
pub struct Meta {
    pub id: String,
    pub note: String,
    pub text: String,
    pub before: String,
    pub after: String,
    pub kind: Option<String>,
    pub promoted: bool,
    pub sent_at: Option<DateTime<Utc>>,
}

pub fn kind_of(text: &str) -> String {
    let t = text.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| t.contains(w));
    if has(&["exam", "midterm", "final", "quiz "]) {
        "exam"
    } else if has(&[
        "submit",
        "homework",
        "assignment",
        "essay",
        "problem set",
        "lab report",
        "hw ",
    ]) {
        "assignment"
    } else if has(&["meeting", "meet with", "attend", "call with"]) {
        "meeting"
    } else if has(&[
        "send",
        "email",
        "follow up",
        "follow-up",
        "reply",
        "contact",
    ]) {
        "follow-up"
    } else {
        "other"
    }
    .into()
}

pub fn clean_kind(kind: &str) -> String {
    let k = kind.trim().to_lowercase();
    if KINDS.contains(&k.as_str()) {
        k
    } else {
        "other".into()
    }
}

fn fnv(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

pub fn note_task_id(note: &str, text: &str) -> String {
    format!("n{:016x}", fnv(&format!("{note}\0{}", text.trim())))
}

pub fn valid_id(id: &str) -> bool {
    (8..=64).contains(&id.len()) && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Listed {
    pub id: String,
    pub source: &'static str,
    pub title: String,
    pub kind: String,
    pub done: bool,
    pub date: Option<NaiveDate>,
    pub time: Option<NaiveTime>,
    pub end: Option<NaiveTime>,
    pub note: Option<NoteRef>,
    pub line: Option<usize>,
    pub found: bool,
    pub promoted: bool,
    pub sent_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct NoteRef {
    pub id: String,
    pub title: String,
    pub folder: String,
}

fn is_manual(tags: &[String]) -> bool {
    tags.iter().any(|t| t == "manual")
}

pub fn note_tasks(store: &Store, metas: &[Meta]) -> Vec<Listed> {
    let mut out = Vec::new();
    let mut by_note: BTreeMap<&str, Vec<&Meta>> = BTreeMap::new();
    for meta in metas {
        by_note.entry(meta.note.as_str()).or_default().push(meta);
    }
    for note in &store.notes {
        if is_manual(&note.tags) {
            continue;
        }
        let lines: Vec<&str> = note.body.lines().collect();
        let note_ref = NoteRef {
            id: note.id.clone(),
            title: note.title.clone(),
            folder: note.directory.clone(),
        };
        let mut claimed: HashSet<usize> = HashSet::new();
        for meta in by_note
            .get(note.id.as_str())
            .map(Vec::as_slice)
            .unwrap_or(&[])
        {
            let at = match core::locate(&note.body, &meta.text, &meta.before, &meta.after) {
                Located::At(n) => Some(n),
                _ => None,
            };
            if let Some(n) = at {
                claimed.insert(n);
            }
            let text = at
                .and_then(|n| checkbox_line(lines[n]).map(|c| c.text.to_string()))
                .unwrap_or_else(|| meta.text.clone());
            let done = at
                .and_then(|n| checkbox_line(lines[n]))
                .is_some_and(|c| c.ticked);
            out.push(listed(
                &meta.id,
                &text,
                done,
                meta.kind.clone(),
                &note_ref,
                at,
                at.is_some(),
                meta.promoted,
                meta.sent_at,
            ));
        }
        for n in core::action_items(&note.body) {
            if claimed.contains(&n) {
                continue;
            }
            let Some(found) = checkbox_line(lines[n]) else {
                continue;
            };
            if found.text.trim().is_empty() {
                continue;
            }
            let id = note_task_id(&note.id, found.text);
            if out.iter().any(|t: &Listed| t.id == id) {
                continue;
            }
            out.push(listed(
                &id,
                found.text,
                found.ticked,
                None,
                &note_ref,
                Some(n),
                true,
                false,
                None,
            ));
        }
        if out.len() > MOST_LISTED * 4 {
            break;
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn listed(
    id: &str,
    text: &str,
    done: bool,
    kind: Option<String>,
    note: &NoteRef,
    line: Option<usize>,
    found: bool,
    promoted: bool,
    sent_at: Option<DateTime<Utc>>,
) -> Listed {
    let due = core::due_in(text);
    Listed {
        id: id.to_string(),
        source: "note",
        title: text.trim().chars().take(MOST_TITLE).collect(),
        kind: kind.unwrap_or_else(|| kind_of(text)),
        done,
        date: due.map(|d| d.date),
        time: due.and_then(|d| d.time),
        end: None,
        note: Some(note.clone()),
        line,
        found,
        promoted,
        sent_at,
    }
}

pub fn standalone(task: &Task, store: Option<&Store>) -> Listed {
    let note = task.note.as_deref().and_then(|id| {
        store
            .and_then(|s| s.notes.iter().find(|n| n.id == id))
            .map(|n| NoteRef {
                id: n.id.clone(),
                title: n.title.clone(),
                folder: n.directory.clone(),
            })
    });
    Listed {
        id: task.id.clone(),
        source: "leo",
        title: task.title.clone(),
        kind: task.kind.clone(),
        done: task.done,
        date: task.date,
        time: task.time,
        end: task.end,
        note,
        line: None,
        found: true,
        promoted: false,
        sent_at: task.sent_at,
    }
}

pub fn sorted(mut all: Vec<Listed>) -> Vec<Listed> {
    all.sort_by(|a, b| {
        a.done
            .cmp(&b.done)
            .then(a.date.is_none().cmp(&b.date.is_none()))
            .then(a.date.cmp(&b.date))
            .then(a.time.is_none().cmp(&b.time.is_none()))
            .then(a.time.cmp(&b.time))
            .then(a.title.cmp(&b.title))
    });
    all.truncate(MOST_LISTED);
    all
}

pub fn ics_of(task: &Listed, stamp: DateTime<Utc>) -> String {
    let escape = |s: &str| {
        s.replace('\\', "\\\\")
            .replace(';', "\\;")
            .replace(',', "\\,")
            .replace(['\r', '\n'], " ")
    };
    let mut lines = vec![
        "BEGIN:VCALENDAR".to_string(),
        "VERSION:2.0".into(),
        "PRODID:-//leo//tasks//EN".into(),
        "BEGIN:VEVENT".into(),
        format!("UID:{}@leo", task.id),
        format!("DTSTAMP:{}", stamp.format("%Y%m%dT%H%M%SZ")),
        format!("SUMMARY:{}", escape(&task.title)),
    ];
    match (task.date, task.time) {
        (Some(date), Some(time)) => {
            let at = date.and_time(time);
            let start = task.end.map_or(at - chrono::Duration::minutes(30), |_| at);
            let end = task.end.map_or(at, |e| date.and_time(e));
            lines.push(format!("DTSTART:{}", start.format("%Y%m%dT%H%M%S")));
            lines.push(format!("DTEND:{}", end.format("%Y%m%dT%H%M%S")));
        }
        (Some(date), None) => {
            lines.push(format!("DTSTART;VALUE=DATE:{}", date.format("%Y%m%d")));
            lines.push(format!(
                "DTEND;VALUE=DATE:{}",
                (date + chrono::Duration::days(1)).format("%Y%m%d")
            ));
        }
        _ => {}
    }
    let from = task
        .note
        .as_ref()
        .map(|n| format!(" · {}", escape(&n.title)))
        .unwrap_or_default();
    lines.push(format!(
        "DESCRIPTION:From leo{from} · leo-task:{}",
        task.id.chars().take(12).collect::<String>()
    ));
    lines.push("END:VEVENT".into());
    lines.push("END:VCALENDAR".into());
    lines.join("\r\n") + "\r\n"
}

pub fn db_of(chats: &Path) -> std::sync::Arc<crate::db::Db> {
    crate::chats::db_of(chats)
}

pub fn load_all(chats: &Path) -> Vec<Task> {
    db_of(chats)
        .with(|c| {
            let mut statement =
                c.prepare("SELECT data FROM tasks ORDER BY updated DESC LIMIT ?1")?;
            let rows = statement.query_map([MOST_TASKS as i64], |r| r.get::<_, String>(0))?;
            Ok(rows
                .flatten()
                .filter_map(|text| serde_json::from_str::<Task>(&text).ok())
                .collect::<Vec<_>>())
        })
        .unwrap_or_default()
}

pub fn load(chats: &Path, id: &str) -> Option<Task> {
    db_of(chats)
        .with(|c| {
            c.query_row("SELECT data FROM tasks WHERE id = ?1", [id], |r| {
                r.get::<_, String>(0)
            })
        })
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
}

pub fn count(chats: &Path) -> usize {
    db_of(chats)
        .with(|c| c.query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get::<_, i64>(0)))
        .map_or(0, |n| n.max(0) as usize)
}

pub fn put(chats: &Path, task: &Task) -> anyhow::Result<()> {
    let text = serde_json::to_string(task)?;
    db_of(chats).with(|c| {
        c.execute(
            "INSERT INTO tasks (id, updated, data) VALUES (?1, ?2, ?3) ON CONFLICT(id) DO UPDATE SET updated = excluded.updated, data = excluded.data",
            [&task.id, &task.updated_at.to_rfc3339(), &text],
        )
    })?;
    Ok(())
}

pub fn remove(chats: &Path, id: &str) -> bool {
    db_of(chats)
        .with(|c| c.execute("DELETE FROM tasks WHERE id = ?1", [id]))
        .is_ok_and(|n| n > 0)
}

pub fn remove_done(chats: &Path) -> usize {
    let done: Vec<String> = load_all(chats)
        .into_iter()
        .filter(|t| t.done)
        .map(|t| t.id)
        .collect();
    done.iter().filter(|id| remove(chats, id)).count()
}

pub fn metas(chats: &Path) -> Vec<Meta> {
    db_of(chats)
        .with(|c| {
            let mut statement = c.prepare("SELECT id, note, text, before, after, kind, promoted, sent_at FROM note_tasks LIMIT ?1")?;
            let rows = statement.query_map([MOST_TASKS as i64], |r| {
                Ok(Meta {
                    id: r.get(0)?,
                    note: r.get(1)?,
                    text: r.get(2)?,
                    before: r.get(3)?,
                    after: r.get(4)?,
                    kind: r.get::<_, Option<String>>(5)?,
                    promoted: r.get::<_, i64>(6)? != 0,
                    sent_at: r
                        .get::<_, Option<String>>(7)?
                        .and_then(|t| DateTime::parse_from_rfc3339(&t).ok())
                        .map(|t| t.with_timezone(&Utc)),
                })
            })?;
            Ok(rows.flatten().collect::<Vec<_>>())
        })
        .unwrap_or_default()
}

pub fn put_meta(chats: &Path, meta: &Meta) -> anyhow::Result<()> {
    let sent = meta.sent_at.map(|t| t.to_rfc3339());
    db_of(chats).with(|c| {
        c.execute(
            "INSERT INTO note_tasks (id, note, text, before, after, kind, promoted, sent_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) ON CONFLICT(id) DO UPDATE SET note = excluded.note, text = excluded.text, before = excluded.before, after = excluded.after, kind = excluded.kind, promoted = excluded.promoted, sent_at = excluded.sent_at",
            rusqlite::params![meta.id, meta.note, meta.text, meta.before, meta.after, meta.kind, i64::from(meta.promoted), sent],
        )
    })?;
    Ok(())
}

pub fn remove_meta(chats: &Path, id: &str) -> bool {
    db_of(chats)
        .with(|c| c.execute("DELETE FROM note_tasks WHERE id = ?1", [id]))
        .is_ok_and(|n| n > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_with(body: &str) -> (tempfile::TempDir, Store, String) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::load_from(&dir.path().join("notes")).unwrap();
        let id = store
            .create_note("Lecture 5", body, vec![], "bio")
            .unwrap()
            .id
            .clone();
        (dir, store, id)
    }

    #[test]
    fn action_items_become_tasks_and_examples_do_not() {
        let body = "## Notes\n- [ ] an example checklist\n## Action items\n- [ ] Submit Homework 4 — due October 16, 2026 at 11:59 pm.\n- [x] Email Maya the slides\n## Check yourself\n- [ ] What is a heap?";
        let (_d, store, id) = store_with(body);
        let tasks = note_tasks(&store, &[]);
        let titles: Vec<&str> = tasks.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Submit Homework 4 — due October 16, 2026 at 11:59 pm.",
                "Email Maya the slides"
            ]
        );
        assert_eq!(tasks[0].date, NaiveDate::from_ymd_opt(2026, 10, 16));
        assert_eq!(tasks[0].time, NaiveTime::from_hms_opt(23, 59, 0));
        assert_eq!(tasks[0].kind, "assignment");
        assert!(tasks[1].done);
        assert_eq!(tasks[1].kind, "follow-up");
        assert_eq!(
            tasks[0].id,
            note_task_id(&id, "Submit Homework 4 — due October 16, 2026 at 11:59 pm.")
        );
    }

    #[test]
    fn a_promoted_checkbox_is_a_task_and_is_marked_when_it_cannot_be_found() {
        let body = "Plan\n- [ ] Ask tutor about the exam\nEnd";
        let (_d, store, id) = store_with(body);
        let meta = Meta {
            id: note_task_id(&id, "Ask tutor about the exam"),
            note: id.clone(),
            text: "Ask tutor about the exam".into(),
            before: "Plan".into(),
            after: "End".into(),
            kind: Some("exam".into()),
            promoted: true,
            sent_at: None,
        };
        let tasks = note_tasks(&store, std::slice::from_ref(&meta));
        assert_eq!(tasks.len(), 1);
        assert!(tasks[0].found && tasks[0].promoted);
        assert_eq!((tasks[0].line, tasks[0].kind.as_str()), (Some(1), "exam"));
        let gone = Meta {
            text: "Something else".into(),
            ..meta
        };
        let tasks = note_tasks(&store, &[gone]);
        assert!(!tasks[0].found);
    }

    #[test]
    fn tasks_sort_open_first_then_by_date_and_an_ics_names_its_task() {
        let now = Utc::now();
        let task = |id: &str, done: bool, date: Option<NaiveDate>| {
            standalone(
                &Task {
                    id: id.into(),
                    title: id.into(),
                    kind: "other".into(),
                    done,
                    date,
                    time: None,
                    end: None,
                    note: None,
                    sent_at: None,
                    created_at: now,
                    updated_at: now,
                },
                None,
            )
        };
        let all = sorted(vec![
            task("undated", false, None),
            task("done", true, NaiveDate::from_ymd_opt(2026, 1, 1)),
            task("later", false, NaiveDate::from_ymd_opt(2026, 12, 1)),
            task("sooner", false, NaiveDate::from_ymd_opt(2026, 11, 1)),
        ]);
        let order: Vec<&str> = all.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(order, ["sooner", "later", "undated", "done"]);
        let ics = ics_of(&all[0], now);
        assert!(ics.contains("DTSTART;VALUE=DATE:20261101"));
        assert!(ics.contains("DTEND;VALUE=DATE:20261102"));
        assert!(ics.contains("leo-task:sooner"));
        assert_eq!(kind_of("Biology midterm"), "exam");
        assert_eq!(clean_kind("Party"), "other");
    }
}
