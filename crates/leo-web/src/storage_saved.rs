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
        about: "Search indexes, retained legacy data, bookkeeping and space available for reuse inside Leo's database. Category sizes show saved data; this accounts for the rest of the database files. Deleting records may leave reusable space. Compact reclaims that space without deleting notes, chats or tasks.".into(),
        path: db.path().display().to_string(), bytes, items: vec![],
        actions: vec![act("compact", "Compact database", Some("Leo briefly pauses database writes while reclaiming unused space. All saved information stays."), false)],
    }
}

pub fn act_on(path: &Path, request: &Request) -> Option<Result<String>> {
    let table = match request.area.as_str() {
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
                tx.execute(&format!("DELETE FROM {}", table.0), [])?
            } else {
                let mut gone = 0;
                for id in &request.items {
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
            c.execute("INSERT INTO interview_problem_undo VALUES ('old-undo', 'practice-one', '2000-01-01T00:00:00Z', '{}')", [])?;
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
    fn saved_inventory_excludes_retired_practice_data_and_keeps_shared_categories() {
        let (_temp, store, path, _note) = seed();
        let list = areas(&store, &path).unwrap();
        assert_eq!(
            list.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
            ["reading", "note-task-data", "calendar-cache"]
        );
        assert_eq!(list[0].bytes, "café".len() as u64);
        assert_eq!(list[0].items[0].label, "Study notes");
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
    fn shared_cleanup_and_retired_requests_never_delete_legacy_interview_rows() {
        let (_temp, store, path, note) = seed();
        for area in ["interviews", "interview-checks", "interview-problem-undo"] {
            for action in ["delete", "all"] {
                assert!(act_on(
                    &path,
                    &Request {
                        area: area.into(),
                        action: action.into(),
                        items: vec!["practice-one".into()]
                    }
                )
                .is_none());
            }
        }
        run(&path, "reading", "delete", &[&note]).unwrap();
        assert_eq!(count(&path, "reading"), 0);
        assert_eq!(count(&path, "note_tasks"), 1);
        run(&path, "note-task-data", "all", &[]).unwrap();
        run(&path, "calendar-cache", "all", &[]).unwrap();
        run(&path, "database", "compact", &[]).unwrap();
        assert_eq!(count(&path, "note_tasks"), 0);
        assert_eq!(count(&path, "calendar_feeds"), 0);
        assert_eq!(count(&path, "interviews"), 2);
        assert_eq!(count(&path, "interview_keys"), 2);
        assert_eq!(count(&path, "interview_problem_undo"), 1);
        assert_eq!(store.find_note(&note).unwrap().body, "- [ ] Homework");
        assert!(run(&path, "reading", "delete", &["../escape"]).is_err());
    }
}
