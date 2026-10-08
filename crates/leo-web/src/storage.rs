use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use leo_core::store::Store;

use crate::{chats, graph::Graphs};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Item {
    pub id: String,
    pub label: String,
    pub detail: String,
    pub bytes: u64,
    pub when: Option<DateTime<Utc>>,
    pub locked: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Act {
    pub id: String,
    pub label: String,
    pub confirm: Option<String>,
    pub selected: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Area {
    pub id: String,
    pub title: String,
    pub about: String,
    pub path: String,
    pub bytes: u64,
    pub items: Vec<Item>,
    pub actions: Vec<Act>,
}

pub trait Housekeeper: Send + Sync {
    fn areas(&self) -> Vec<Area>;
    fn act(&self, area: &str, action: &str, items: &[String]) -> Option<Result<String>>;
}

#[derive(Debug, Deserialize)]
pub struct Request {
    pub area: String,
    pub action: String,
    #[serde(default)]
    pub items: Vec<String>,
}

pub const OLD_CHAT_DAYS: i64 = 30;

pub fn size_of(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.file_type().is_symlink() {
        return 0;
    }
    if meta.is_file() {
        return meta.len();
    }
    std::fs::read_dir(path)
        .map(|entries| entries.flatten().map(|e| size_of(&e.path())).sum())
        .unwrap_or(0)
}

fn sized(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} bytes");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if value >= 10.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

pub fn act(id: &str, label: &str, confirm: Option<&str>, selected: bool) -> Act {
    Act {
        id: id.into(),
        label: label.into(),
        confirm: confirm.map(str::to_string),
        selected,
    }
}

pub fn data_dir(notes_dir: &Path) -> PathBuf {
    notes_dir.parent().unwrap_or(notes_dir).to_path_buf()
}

fn attachments_dir(notes_dir: &Path) -> PathBuf {
    data_dir(notes_dir).join("attachments")
}

fn modified(path: &Path) -> Option<DateTime<Utc>> {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .map(DateTime::<Utc>::from)
}

fn notes_bytes(notes_dir: &Path) -> u64 {
    std::fs::read_dir(notes_dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| {
                    let name = e.file_name();
                    name != ".trash" && name != ".git"
                })
                .map(|e| size_of(&e.path()))
                .sum()
        })
        .unwrap_or(0)
}

pub fn areas(store: &Store, graphs: &Graphs, chats_dir: &Path, now: DateTime<Utc>) -> Vec<Area> {
    let notes_dir = &store.notes_dir;
    let mut out = Vec::new();

    out.push(Area {
        id: "notes".into(),
        title: "Notes".into(),
        about: format!(
            "{} notes as Markdown files. Delete a note from its own page; it goes to the trash first.",
            store.notes.len()
        ),
        path: notes_dir.display().to_string(),
        bytes: notes_bytes(notes_dir),
        items: Vec::new(),
        actions: Vec::new(),
    });

    let trashed = store.trashed();
    out.push(Area {
        id: "trash".into(),
        title: "Trash".into(),
        about: format!(
            "Deleted notes, kept {} days so they can be restored, then removed on their own.",
            leo_core::store::TRASH_DAYS
        ),
        path: notes_dir.join(".trash").display().to_string(),
        bytes: size_of(&notes_dir.join(".trash")),
        items: trashed
            .iter()
            .map(|t| Item {
                id: t.id.clone(),
                label: t.title.clone(),
                detail: if t.directory.is_empty() {
                    "All notes".into()
                } else {
                    t.directory.clone()
                },
                bytes: 0,
                when: Some(t.deleted_at),
                locked: false,
            })
            .collect(),
        actions: if trashed.is_empty() {
            Vec::new()
        } else {
            vec![act(
                "empty",
                "Empty the trash",
                Some("These notes are deleted for good and cannot be restored."),
                false,
            )]
        },
    });

    let summaries = chats::list(chats_dir);
    let cutoff = now - chrono::Duration::days(OLD_CHAT_DAYS);
    let mut chat_actions = Vec::new();
    if !summaries.is_empty() {
        chat_actions.push(act(
            "delete",
            "Delete selected",
            Some("These conversations with Felix are deleted for good."),
            true,
        ));
        if summaries.iter().any(|c| c.updated_at < cutoff) {
            chat_actions.push(act(
                "older",
                &format!("Delete chats older than {OLD_CHAT_DAYS} days"),
                Some("Conversations nobody has added to in a month are deleted for good."),
                false,
            ));
        }
        chat_actions.push(act(
            "all",
            "Delete every chat",
            Some("Every conversation with Felix is deleted for good."),
            false,
        ));
    }
    out.push(Area {
        id: "chats".into(),
        title: "Chats with Felix".into(),
        about: "Every conversation, so you can go back to it. Never backed up.".into(),
        path: chats_dir.display().to_string(),
        bytes: size_of(chats_dir),
        items: summaries
            .iter()
            .map(|c| Item {
                id: c.id.clone(),
                label: c.title.clone(),
                detail: format!("{} message{}", c.count, if c.count == 1 { "" } else { "s" }),
                bytes: std::fs::metadata(chats_dir.join(format!("{}.json", c.id)))
                    .map(|m| m.len())
                    .unwrap_or(0),
                when: Some(c.updated_at),
                locked: false,
            })
            .collect(),
        actions: chat_actions,
    });

    let originals = attachments_dir(notes_dir);
    let mut kept: Vec<Item> = std::fs::read_dir(&originals)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.path().is_dir())
                .map(|e| {
                    let id = e.file_name().to_string_lossy().to_string();
                    let note = store.notes.iter().find(|n| n.id == id);
                    let files = std::fs::read_dir(e.path())
                        .map(|f| f.flatten().count())
                        .unwrap_or(0);
                    Item {
                        label: note
                            .map(|n| n.title.clone())
                            .unwrap_or_else(|| "A note that was deleted".into()),
                        detail: format!(
                            "{files} file{}{}",
                            if files == 1 { "" } else { "s" },
                            if note.is_none() {
                                " · its note is gone"
                            } else {
                                ""
                            }
                        ),
                        bytes: size_of(&e.path()),
                        when: modified(&e.path()),
                        locked: false,
                        id,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    kept.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.label.cmp(&b.label)));
    let mut original_actions = Vec::new();
    if !kept.is_empty() {
        original_actions.push(act(
            "delete",
            "Delete selected",
            Some("The uploaded files are deleted; the notes made from them stay."),
            true,
        ));
        if kept.iter().any(|k| k.detail.ends_with("its note is gone")) {
            original_actions.push(act(
                "orphans",
                "Delete files whose note is gone",
                Some("Uploads whose notes were deleted are removed."),
                false,
            ));
        }
    }
    out.push(Area {
        id: "originals".into(),
        title: "Uploaded files".into(),
        about: "The PDFs, slides and photos you uploaded, kept beside the notes made from them. Never backed up.".into(),
        path: originals.display().to_string(),
        bytes: size_of(&originals),
        items: kept,
        actions: original_actions,
    });

    let map_bytes = size_of(graphs.path());
    out.push(Area {
        id: "map".into(),
        title: "Map of ideas".into(),
        about: "What the AI found in each note and how notes connect. Clearing it means the next map asks the AI to read every note again.".into(),
        path: graphs.path().display().to_string(),
        bytes: map_bytes,
        items: Vec::new(),
        actions: if map_bytes > 0 {
            vec![act(
                "clear",
                "Clear the map",
                Some("The map is emptied. Building it again sends your notes to the AI again."),
                false,
            )]
        } else {
            Vec::new()
        },
    });

    let history = notes_dir.join(".git");
    if history.is_dir() {
        out.push(Area {
            id: "backup".into(),
            title: "Backup history".into(),
            about: "Every saved version of your notes, used by backup. leo manages it; it is not deleted from here.".into(),
            path: history.display().to_string(),
            bytes: size_of(&history),
            items: Vec::new(),
            actions: Vec::new(),
        });
    }
    out
}

pub fn act_on(
    store: &Store,
    graphs: &Graphs,
    chats_dir: &Path,
    request: &Request,
    now: DateTime<Utc>,
) -> Option<Result<String>> {
    let notes_dir = &store.notes_dir;
    let done = match (request.area.as_str(), request.action.as_str()) {
        ("trash", "empty") => store.empty_trash().map(|n| {
            format!(
                "Emptied the trash ({n} note{}).",
                if n == 1 { "" } else { "s" }
            )
        }),
        ("chats", "delete") => {
            let gone = request
                .items
                .iter()
                .filter(|id| chats::remove(chats_dir, id))
                .count();
            Ok(plural_deleted(gone, "chat"))
        }
        ("chats", "older") => {
            let cutoff = now - chrono::Duration::days(OLD_CHAT_DAYS);
            let gone = chats::list(chats_dir)
                .iter()
                .filter(|c| c.updated_at < cutoff && chats::remove(chats_dir, &c.id))
                .count();
            Ok(plural_deleted(gone, "chat"))
        }
        ("chats", "all") => {
            let gone = chats::list(chats_dir)
                .iter()
                .filter(|c| chats::remove(chats_dir, &c.id))
                .count();
            Ok(plural_deleted(gone, "chat"))
        }
        ("originals", "delete") => remove_originals(notes_dir, request.items.iter().cloned()),
        ("originals", "orphans") => {
            let orphans = std::fs::read_dir(attachments_dir(notes_dir))
                .map(|entries| {
                    entries
                        .flatten()
                        .map(|e| e.file_name().to_string_lossy().to_string())
                        .filter(|id| !store.notes.iter().any(|n| &n.id == id))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            remove_originals(notes_dir, orphans.into_iter())
        }
        ("map", "clear") => match graphs.clear() {
            Ok(true) => Ok("Cleared the map of ideas.".to_string()),
            Ok(false) => Err(anyhow::anyhow!(
                "The map is being built right now; clear it when that finishes."
            )),
            Err(e) => Err(e),
        },
        _ => return None,
    };
    Some(done)
}

fn plural_deleted(n: usize, what: &str) -> String {
    format!("Deleted {n} {what}{}.", if n == 1 { "" } else { "s" })
}

fn remove_originals(notes_dir: &Path, ids: impl Iterator<Item = String>) -> Result<String> {
    let base = attachments_dir(notes_dir);
    let mut gone = 0;
    for id in ids {
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            bail!("That is not an upload leo knows.");
        }
        let dir = base.join(&id);
        if dir.is_dir() {
            std::fs::remove_dir_all(&dir)?;
            gone += 1;
        }
    }
    Ok(format!(
        "Deleted the uploaded files of {gone} note{}.",
        if gone == 1 { "" } else { "s" }
    ))
}

pub fn describe(mut areas: Vec<Area>) -> serde_json::Value {
    areas.retain(|a| a.bytes > 0 || !a.items.is_empty() || a.id == "notes");
    let total: u64 = areas.iter().map(|a| a.bytes).sum();
    serde_json::json!({
        "total": total,
        "total_label": sized(total),
        "areas": areas
            .iter()
            .map(|a| {
                let mut v = serde_json::to_value(a).unwrap_or_default();
                v["size"] = serde_json::Value::String(sized(a.bytes));
                if let Some(items) = v["items"].as_array_mut() {
                    for (item, source) in items.iter_mut().zip(&a.items) {
                        item["size"] = serde_json::Value::String(if source.bytes > 0 {
                            sized(source.bytes)
                        } else {
                            String::new()
                        });
                    }
                }
                v
            })
            .collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(day: u32) -> DateTime<Utc> {
        chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 10, day, 12, 0, 0).unwrap()
    }

    fn setup() -> (tempfile::TempDir, Store, Graphs, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::load_from(&tmp.path().join("notes")).unwrap();
        let kept = store
            .create_note("Kept", "body text", vec![], "")
            .unwrap()
            .id
            .clone();
        let gone = store
            .create_note("Gone", "old", vec![], "")
            .unwrap()
            .id
            .clone();
        store.save().unwrap();
        assert!(store.delete_note(&gone));
        store.save().unwrap();
        let graphs = Graphs::for_notes(&store.notes_dir, None);
        std::fs::write(graphs.path(), "{}").unwrap();
        let originals = tmp.path().join("attachments");
        for (id, bytes) in [(&kept, 10), (&gone, 2000)] {
            std::fs::create_dir_all(originals.join(id)).unwrap();
            std::fs::write(originals.join(id).join("slides.pdf"), vec![0u8; bytes]).unwrap();
        }
        let chats_dir = chats::dir_for(&store.notes_dir);
        let september = chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 9, 1, 12, 0, 0).unwrap();
        for (id, when) in [("chat-old-0001", september), ("chat-new-0002", at(30))] {
            chats::save(
                &chats_dir,
                id,
                chats::Saving {
                    title: id.into(),
                    mode: "chat".into(),
                    refs: vec![],
                    messages: vec![serde_json::json!({"role": "user", "text": "hi"})],
                },
                when,
            )
            .unwrap();
        }
        (tmp, store, graphs, chats_dir)
    }

    fn area<'a>(list: &'a [Area], id: &str) -> &'a Area {
        list.iter().find(|a| a.id == id).unwrap()
    }

    #[test]
    fn every_place_leo_writes_is_listed_with_its_size_and_what_can_go() {
        let (_tmp, store, graphs, chats_dir) = setup();
        let list = areas(&store, &graphs, &chats_dir, at(31));
        let ids: Vec<&str> = list.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["notes", "trash", "chats", "originals", "map"]);
        assert!(area(&list, "notes").bytes > 0);
        assert!(area(&list, "notes").actions.is_empty());
        assert_eq!(area(&list, "trash").items[0].label, "Gone");
        let chat_actions: Vec<&str> = area(&list, "chats")
            .actions
            .iter()
            .map(|a| a.id.as_str())
            .collect();
        assert_eq!(chat_actions, ["delete", "older", "all"]);
        let originals = area(&list, "originals");
        assert_eq!(originals.bytes, 2010);
        assert_eq!(originals.items[0].label, "A note that was deleted");
        assert!(originals.items[0].detail.ends_with("its note is gone"));
        assert_eq!(originals.items[1].label, "Kept");
        let described = describe(list);
        assert_eq!(described["areas"][3]["size"], "2.0 KB");
        assert_eq!(described["areas"][3]["items"][0]["size"], "2.0 KB");
    }

    #[test]
    fn each_action_removes_only_what_it_says() {
        let (_tmp, store, graphs, chats_dir) = setup();
        let run = |area: &str, action: &str, items: &[&str]| {
            act_on(
                &store,
                &graphs,
                &chats_dir,
                &Request {
                    area: area.into(),
                    action: action.into(),
                    items: items.iter().map(|s| s.to_string()).collect(),
                },
                at(31),
            )
            .unwrap()
            .unwrap()
        };
        assert_eq!(run("chats", "older", &[]), "Deleted 1 chat.");
        assert_eq!(chats::list(&chats_dir).len(), 1);
        assert_eq!(
            run("originals", "orphans", &[]),
            "Deleted the uploaded files of 1 note."
        );
        assert_eq!(size_of(&attachments_dir(&store.notes_dir)), 10);
        assert_eq!(run("trash", "empty", &[]), "Emptied the trash (1 note).");
        assert!(store.trashed().is_empty());
        assert_eq!(run("map", "clear", &[]), "Cleared the map of ideas.");
        assert!(!graphs.path().exists());
        assert_eq!(
            run("chats", "delete", &["chat-new-0002", "../../notes"]),
            "Deleted 1 chat."
        );
        assert!(act_on(
            &store,
            &graphs,
            &chats_dir,
            &Request {
                area: "originals".into(),
                action: "delete".into(),
                items: vec!["../notes".into()],
            },
            at(31),
        )
        .unwrap()
        .is_err());
        assert!(store.notes_dir.join("Kept.md").exists());
        assert!(act_on(
            &store,
            &graphs,
            &chats_dir,
            &Request {
                area: "notes".into(),
                action: "delete".into(),
                items: vec![],
            },
            at(31),
        )
        .is_none());
    }

    #[test]
    fn sizes_read_like_a_file_browser() {
        assert_eq!(sized(0), "0 bytes");
        assert_eq!(sized(1536), "1.5 KB");
        assert_eq!(sized(670 * 1024 * 1024), "670 MB");
        assert_eq!(sized(3 * 1024 * 1024 * 1024), "3.0 GB");
    }
}
