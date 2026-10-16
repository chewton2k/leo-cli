use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use anyhow::{bail, Result};
use chrono::{DateTime, Utc};
use leo_core::store::Store;

use crate::storage::{act, Area, Item, Request};

const MOST_ITEMS: usize = 20_000;
const MOST_CALENDAR_NAMES_BYTES: u64 = 64_000;

fn when(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

fn area(path: &Path, id: &str, title: &str, about: &str, items: Vec<Item>, clear: &str) -> Area {
    let actions = if items.is_empty() {
        vec![]
    } else {
        vec![
            act("delete", "Delete selected", Some(clear), true),
            act("all", "Delete all", Some(clear), false),
        ]
    };
    Area {
        id: id.into(),
        title: title.into(),
        about: about.into(),
        path: path.display().to_string(),
        bytes: items.iter().map(|i| i.bytes).sum(),
        items,
        actions,
    }
}

pub fn areas(store: &Store, path: &Path) -> Result<Vec<Area>> {
    let db = crate::db::for_chats(path);
    let practices: Vec<Item> = db
        .with(|c| {
            let mut statement = c.prepare("SELECT id, CASE WHEN length(CAST(data AS BLOB)) <= ?2 THEN data ELSE '{}' END, updated, length(CAST(data AS BLOB)) FROM interviews ORDER BY updated DESC, id LIMIT ?1")?;
            let rows = statement.query_map([MOST_ITEMS as i64, crate::interview::BODY_BYTES as i64], |r| {
                let data: String = r.get(1)?;
                let value: serde_json::Value = serde_json::from_str(&data).unwrap_or_default();
                let framework = &value["framework"];
                let named = |key: &str| framework[key].as_str().unwrap_or("").to_string();
                let count = |key: &str| value[key].as_array().map_or(0, Vec::len);
                let info = [named("track"), named("company"), named("role")]
                    .into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
                Ok(Item {
                    id: r.get(0)?,
                    label: framework["title"].as_str().unwrap_or("Unreadable interview practice").into(),
                    detail: format!("{info} · {} problems · {} submitted attempts · {} scheduled sessions", count("problems").max(usize::from(value["challenge"].is_object())), count("attempts"), count("schedule")),
                    bytes: r.get::<_, i64>(3)?.max(0) as u64,
                    when: when(&r.get::<_, String>(2)?),
                    locked: false,
                })
            })?;
            rows.collect()
        })?;
    let titles: HashMap<&str, &str> = practices
        .iter()
        .map(|i| (i.id.as_str(), i.label.as_str()))
        .collect();
    let checks: Vec<Item> = db.with(|c| {
        let mut statement = c.prepare("SELECT interview, count(*), sum(status = 'ready'), sum(status = 'preparing'), sum(status = 'failed'), sum(length(CAST(data AS BLOB))), max(updated) FROM interview_keys GROUP BY interview ORDER BY max(updated) DESC LIMIT ?1")?;
        let rows = statement.query_map([MOST_ITEMS as i64], |r| {
            let id: String = r.get(0)?;
            Ok(Item {
                label: titles.get(id.as_str()).copied().unwrap_or("Checks for a deleted practice").into(),
                detail: format!("{} problems · {} ready · {} preparing · {} failed", r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?, r.get::<_, i64>(4)?),
                bytes: r.get::<_, i64>(5)?.max(0) as u64,
                when: when(&r.get::<_, String>(6)?), id, locked: false,
            })
        })?;
        rows.collect()
    })?;
    let note_title = |id: &str| {
        store
            .find_note(id)
            .map(|n| n.title.clone())
            .unwrap_or_else(|| "A note that is gone".into())
    };
    let reading: Vec<Item> = db.with(|c| {
        let mut statement = c.prepare(
            "SELECT note, at, length(CAST(anchor AS BLOB)) FROM reading ORDER BY at DESC LIMIT ?1",
        )?;
        let rows = statement.query_map([MOST_ITEMS as i64], |r| {
            let id: String = r.get(0)?;
            Ok(Item {
                label: note_title(&id),
                detail: "The last place you were reading".into(),
                bytes: r.get::<_, i64>(2)?.max(0) as u64,
                when: when(&r.get::<_, String>(1)?),
                id,
                locked: false,
            })
        })?;
        rows.collect()
    })?;
    let note_tasks: Vec<Item> = db.with(|c| {
        let mut statement = c.prepare("SELECT note, count(*), sum(length(CAST(text AS BLOB)) + length(CAST(before AS BLOB)) + length(CAST(after AS BLOB)) + coalesce(length(kind), 0) + length(CAST(description AS BLOB))) FROM note_tasks GROUP BY note ORDER BY note LIMIT ?1")?;
        let rows = statement.query_map([MOST_ITEMS as i64], |r| {
            let id: String = r.get(0)?;
            Ok(Item { label: note_title(&id), detail: format!("{} saved task classifications, promotions and calendar send states", r.get::<_, i64>(1)?), bytes: r.get::<_, i64>(2)?.max(0) as u64, when: None, id, locked: false })
        })?;
        rows.collect()
    })?;
    let calendar_names: HashMap<String, String> =
        std::fs::symlink_metadata(crate::calendar::settings_path(&store.notes_dir))
            .ok()
            .filter(|m| m.is_file() && m.len() < MOST_CALENDAR_NAMES_BYTES)
            .and_then(|_| {
                std::fs::File::open(crate::calendar::settings_path(&store.notes_dir)).ok()
            })
            .and_then(|f| {
                serde_json::from_reader::<_, serde_json::Value>(f.take(MOST_CALENDAR_NAMES_BYTES))
                    .ok()
            })
            .and_then(|v| v["calendars"].as_array().cloned())
            .unwrap_or_default()
            .into_iter()
            .take(MOST_ITEMS)
            .filter_map(|v| Some((v["id"].as_str()?.into(), v["name"].as_str()?.into())))
            .collect();
    let calendars: Vec<Item> = db.with(|c| {
        let mut statement = c.prepare("SELECT feed, coalesce(fetched_at, tried_at), coalesce(length(body), 0) FROM calendar_feeds ORDER BY feed LIMIT ?1")?;
        let rows = statement.query_map([MOST_ITEMS as i64], |r| {
            let id: String = r.get(0)?;
            Ok(Item { label: calendar_names.get(&id).cloned().unwrap_or_else(|| "Cached calendar".into()), detail: "A local copy of calendar events; private calendar links are kept separately".into(), bytes: r.get::<_, i64>(2)?.max(0) as u64, when: when(&r.get::<_, String>(1)?), id, locked: false })
        })?;
        rows.collect()
    })?;
    Ok(vec![
        area(db.path(), "interviews", "Interview practices", "Your frameworks, saved problems, code and answer drafts, whiteboards, submitted attempts, reviews and practice schedules. Open a practice to inspect it. Export includes these when Chats and interview practices is selected.", practices, "These practices, their workspaces, attempts, schedules and automatic check data are deleted permanently. Notes, standalone tasks and Felix conversations stay."),
        area(db.path(), "interview-checks", "Interview check data", "Automatic tests, grading criteria and reference answers, grouped by practice. Answers stay hidden during practice. Clearing this data keeps your work and reviews; Prepare them in the practice creates it again using your writing AI.", checks, "The automatic checks and reference data are removed. Your problems, code, whiteboards, attempts and saved reviews stay. Preparing checks again uses your writing AI."),
        area(db.path(), "reading", "Reading positions", "The places you left off in your notes, shared between devices connected to this Leo server.", reading, "These notes open at the top next time. The notes themselves stay."),
        area(db.path(), "note-task-data", "Note task settings", "Descriptions, saved classifications, promoted tasks and calendar send states for checkboxes in your notes. Dates and the checkboxes themselves remain in the Markdown notes.", note_tasks, "Saved task classifications, promotions and calendar send states are reset. Your notes, checkbox completion and written dates stay."),
        area(db.path(), "calendar-cache", "Cached calendar events", "The latest calendar events read from your connected calendars. Clearing this copy does not disconnect a calendar or change Google Calendar.", calendars, "The local copy of these calendar events is removed. Refresh Calendar to read it again; your connections and Google Calendar events stay."),
    ])
}

pub fn database_area(path: &Path, areas: &[Area]) -> Area {
    let db = crate::db::for_chats(path);
    let named: u64 = areas
        .iter()
        .filter(|a| a.path == db.path().display().to_string())
        .map(|a| a.bytes)
        .sum();
    let bytes = db.bytes().saturating_sub(named);
    Area {
        id: "database".into(), title: "Database space and indexes".into(),
        about: "Search indexes, bookkeeping and space available for reuse inside Leo's database. Category sizes show saved data; this accounts for the rest of the database files. Deleting records may leave reusable space. Compact reclaims that space without deleting notes, chats, practices or tasks.".into(),
        path: db.path().display().to_string(), bytes, items: vec![],
        actions: vec![act("compact", "Compact database", Some("Leo briefly pauses database writes while reclaiming unused space. All saved information stays."), false)],
    }
}

pub fn act_on(path: &Path, request: &Request) -> Option<Result<String>> {
    let table = match request.area.as_str() {
        "interviews" => ("interviews", "id"),
        "interview-checks" => ("interview_keys", "interview"),
        "reading" => ("reading", "note"),
        "note-task-data" => ("note_tasks", "note"),
        "calendar-cache" => ("calendar_feeds", "feed"),
        "database" if request.action == "compact" => {
            return Some(
                crate::db::for_chats(path)
                    .tidy()
                    .map(|()| "Compacted the database; all saved information stays.".into()),
            )
        }
        _ => return None,
    };
    if !matches!(request.action.as_str(), "delete" | "all") {
        return None;
    }
    Some((|| {
        if request.items.len() > MOST_ITEMS
            || request.items.iter().any(|id| !crate::chats::valid_id(id))
        {
            bail!("Choose valid items from Storage, then try again.");
        }
        let db = crate::db::for_chats(path);
        let gone = db.with(|c| {
            let tx = c.transaction()?;
            let gone = if request.action == "all" {
                if request.area == "interviews" {
                    tx.execute("DELETE FROM interview_keys", [])?;
                }
                tx.execute(&format!("DELETE FROM {}", table.0), [])?
            } else {
                let mut gone = 0;
                for id in &request.items {
                    if request.area == "interviews" {
                        tx.execute("DELETE FROM interview_keys WHERE interview = ?1", [id])?;
                    }
                    gone += tx.execute(
                        &format!("DELETE FROM {} WHERE {} = ?1", table.0, table.1),
                        [id],
                    )?;
                }
                gone
            };
            tx.commit()?;
            Ok(gone)
        })?;
        Ok(format!(
            "Removed {gone} saved record{}. Other information stays.",
            if gone == 1 { "" } else { "s" }
        ))
    })())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed() -> (tempfile::TempDir, Store, std::path::PathBuf, String) {
        let temp = tempfile::tempdir().unwrap();
        let mut store = Store::load_from(&temp.path().join("notes")).unwrap();
        let note = store
            .create_note("Study notes", "- [ ] Homework", vec![], "")
            .unwrap()
            .id
            .clone();
        store.save().unwrap();
        let path = crate::chats::dir_for(&store.notes_dir);
        let db = crate::db::for_chats(&path);
        db.with(|c| {
            for id in ["practice-one", "practice-two"] {
                let data = serde_json::json!({"framework":{"title":"Python café", "track":"coding", "company":"Pinterest", "role":"Intern"},"problems":[{},{}],"attempts":[{}],"schedule":[{}],"workspace":{"answer":"private code"}}).to_string();
                c.execute("INSERT INTO interviews VALUES (?1, '2026-10-13T12:00:00Z', ?2)", rusqlite::params![id, data])?;
                c.execute("INSERT INTO interview_keys VALUES (?1, 'problem-one', 'ready', '2026-10-13T12:00:00Z', ?2)", rusqlite::params![id, r#"{"reference":"hidden reference", "tests":[{"expected":"hidden answer"}]}"#])?;
            }
            c.execute("INSERT INTO reading VALUES (?1, 'café', '2026-10-13T12:00:00Z')", [&note])?;
            c.execute("INSERT INTO note_tasks (id, note, text, before, after, kind, promoted, sent_at) VALUES ('task-meta-one', ?1, 'Homework', '', '', 'assignment', 1, NULL)", [&note])?;
            c.execute("INSERT INTO calendar_feeds (feed, tried_at, body) VALUES ('calendar-one', '2026-10-13T12:00:00Z', ?1)", [b"private calendar events".as_slice()])?;
            Ok(())
        }).unwrap();
        (temp, store, path, note)
    }

    fn run(path: &Path, area: &str, action: &str, ids: &[&str]) -> Result<String> {
        act_on(
            path,
            &Request {
                area: area.into(),
                action: action.into(),
                items: ids.iter().map(|id| (*id).into()).collect(),
            },
        )
        .unwrap()
    }

    fn count(path: &Path, table: &str) -> i64 {
        crate::db::for_chats(path)
            .with(|c| c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0)))
            .unwrap()
    }

    #[test]
    fn saved_inventory_shows_practice_contents_and_sizes_without_revealing_answers() {
        let (_temp, store, path, _note) = seed();
        let list = areas(&store, &path).unwrap();
        assert_eq!(
            list.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
            [
                "interviews",
                "interview-checks",
                "reading",
                "note-task-data",
                "calendar-cache"
            ]
        );
        let practice = &list[0].items[0];
        assert_eq!(practice.label, "Python café");
        assert!(practice
            .detail
            .contains("2 problems · 1 submitted attempts · 1 scheduled sessions"));
        let actual: i64 = crate::db::for_chats(&path)
            .with(|c| {
                c.query_row(
                    "SELECT length(CAST(data AS BLOB)) FROM interviews WHERE id = ?1",
                    [&practice.id],
                    |r| r.get(0),
                )
            })
            .unwrap();
        assert_eq!(practice.bytes, u64::try_from(actual).unwrap());
        assert_eq!(list[2].bytes, "café".len() as u64);
        assert_eq!(list[2].items[0].label, "Study notes");
        let shown = serde_json::to_string(&list).unwrap();
        for private in [
            "private code",
            "hidden reference",
            "hidden answer",
            "private calendar events",
        ] {
            assert!(!shown.contains(private));
        }
        let named: u64 = list.iter().map(|a| a.bytes).sum();
        assert_eq!(
            named + database_area(&path, &list).bytes,
            crate::db::for_chats(&path).bytes()
        );
    }

    #[test]
    fn selected_and_bulk_cleanup_keep_every_other_category_in_either_order() {
        for first in ["interviews", "interview-checks"] {
            let (_temp, store, path, note) = seed();
            run(&path, first, "delete", &["practice-one"]).unwrap();
            assert_eq!(
                count(&path, "interviews"),
                if first == "interviews" { 1 } else { 2 }
            );
            assert_eq!(count(&path, "interview_keys"), 1);
            run(
                &path,
                if first == "interviews" {
                    "interview-checks"
                } else {
                    "interviews"
                },
                "delete",
                &["practice-one"],
            )
            .unwrap();
            assert_eq!(count(&path, "interviews"), 1);
            assert_eq!(count(&path, "interview_keys"), 1);
            assert!(run(
                &path,
                "interviews",
                "delete",
                &["practice-two", "../escape"]
            )
            .is_err());
            assert_eq!(count(&path, "interviews"), 1);
            run(&path, "reading", "delete", &[&note]).unwrap();
            assert_eq!(count(&path, "reading"), 0);
            assert_eq!(count(&path, "note_tasks"), 1);
            run(&path, "note-task-data", "all", &[]).unwrap();
            assert_eq!(count(&path, "note_tasks"), 0);
            assert_eq!(store.find_note(&note).unwrap().body, "- [ ] Homework");
            run(&path, "calendar-cache", "all", &[]).unwrap();
            assert_eq!(count(&path, "calendar_feeds"), 0);
            run(&path, "database", "compact", &[]).unwrap();
            assert_eq!(count(&path, "interviews"), 1);
            run(&path, "interviews", "all", &[]).unwrap();
            assert_eq!(count(&path, "interview_keys"), 0);
        }
    }
}
