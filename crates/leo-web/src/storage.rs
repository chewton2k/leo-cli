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
                    name != ".trash" && name != ".git" && name != leo_core::attachments::DIR
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
        about: match leo_core::keep::load(notes_dir).trash_days {
            Some(days) => format!(
                "Deleted notes, kept {} so they can be restored, then removed on their own.",
                leo_core::keep::describe(Some(days))
            ),
            None => "Deleted notes, kept until you empty the trash.".to_string(),
        },
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
        about: "Every conversation, so you can go back to it. Kept in leo's database beside your notes; never backed up.".into(),
        path: chats::db_of(chats_dir).path().display().to_string(),
        bytes: summaries.iter().map(|c| chats::bytes_of(chats_dir, &c.id)).sum(),
        items: summaries
            .iter()
            .map(|c| Item {
                id: c.id.clone(),
                label: c.title.clone(),
                detail: format!("{} message{}", c.count, if c.count == 1 { "" } else { "s" }),
                bytes: chats::bytes_of(chats_dir, &c.id),
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

    out.push(pictures_area(store));

    let held = crate::chat_files::held(chats_dir);
    let titles: std::collections::HashMap<String, String> = summaries
        .iter()
        .map(|c| (c.id.clone(), c.title.clone()))
        .collect();
    out.push(Area {
        id: "chat-docs".into(),
        title: "Documents given to Felix".into(),
        about: "The text read from files you gave Felix in a chat. The files themselves are never kept, and the text goes when its chat is deleted.".into(),
        path: chats::db_of(chats_dir).path().display().to_string(),
        bytes: held.iter().map(|h| h.bytes).sum(),
        items: held
            .iter()
            .map(|h| Item {
                id: h.chat.clone(),
                label: titles
                    .get(&h.chat)
                    .cloned()
                    .unwrap_or_else(|| "A chat that never started".into()),
                detail: format!(
                    "{} document{}",
                    h.docs,
                    if h.docs == 1 { "" } else { "s" }
                ),
                bytes: h.bytes,
                when: h.last,
                locked: false,
            })
            .collect(),
        actions: if held.is_empty() {
            Vec::new()
        } else {
            vec![
                act(
                    "delete",
                    "Delete selected",
                    Some("Felix forgets these documents; the chats themselves stay."),
                    true,
                ),
                act(
                    "all",
                    "Delete every document",
                    Some("Felix forgets every document given to him; the chats stay."),
                    false,
                ),
            ]
        },
    });

    let map_bytes = graphs.bytes();
    out.push(Area {
        id: "map".into(),
        title: "Knowledge graph".into(),
        about: "What the AI found in each note and how notes connect. Clearing it means the next build asks the AI to read every note again.".into(),
        path: graphs.stored_in().display().to_string(),
        bytes: map_bytes,
        items: Vec::new(),
        actions: if map_bytes > 0 {
            vec![act(
                "clear",
                "Clear the knowledge graph",
                Some("The knowledge graph is emptied. Building it again sends your notes to the AI again."),
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
        ("chat-docs", "delete") => {
            let gone = request
                .items
                .iter()
                .filter(|chat| crate::chat_files::remove_all(chats_dir, chat))
                .count();
            Ok(format!(
                "Felix forgot the documents of {gone} chat{}.",
                if gone == 1 { "" } else { "s" }
            ))
        }
        ("chat-docs", "all") => {
            let gone = crate::chat_files::held(chats_dir)
                .iter()
                .filter(|h| crate::chat_files::remove_all(chats_dir, &h.chat))
                .count();
            Ok(format!(
                "Felix forgot the documents of {gone} chat{}.",
                if gone == 1 { "" } else { "s" }
            ))
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
        ("pictures", "delete") => remove_pictures(notes_dir, request.items.iter().cloned()),
        ("pictures", "unused") => {
            let users = picture_users(store);
            let unused = std::fs::read_dir(pictures_dir(notes_dir))
                .map(|entries| {
                    entries
                        .flatten()
                        .filter(|e| e.path().is_file())
                        .map(|e| e.file_name().to_string_lossy().to_string())
                        .filter(|name| users.get(name).is_none_or(Vec::is_empty))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            remove_pictures(notes_dir, unused.into_iter())
        }
        ("map", "clear") => match graphs.clear() {
            Ok(true) => Ok("Cleared the knowledge graph.".to_string()),
            Ok(false) => Err(anyhow::anyhow!(
                "The knowledge graph is being built right now; clear it when that finishes."
            )),
            Err(e) => Err(e),
        },
        _ => return None,
    };
    Some(done)
}

pub struct Kept<'a> {
    pub captions: &'a crate::captions::Captions,
    pub vectors: &'a crate::vectors::Vectors,
}

pub const CONFIG_FILES: [(&str, &str); 8] = [
    ("config.toml", "Your settings"),
    ("serve-token", "The code in the leo serve link"),
    ("serve-sessions.json", "Browsers signed in to leo serve"),
    (
        "usage.json",
        "Plan usage last seen for Claude Code and Codex",
    ),
    (
        "update-check.json",
        "When leo last looked for a new version",
    ),
    (
        "credentials.json",
        "API keys kept in a file where there is no keychain",
    ),
    (
        ".credentials-migrated",
        "Marks that old keys were moved to the keychain",
    ),
    (".env", "Settings for how leo itself starts"),
];

const SMALL_FILES: [(&str, &str); 4] = [
    ("keep.json", "How long the trash and chats are kept"),
    ("recent.json", "Notes opened lately in the terminal app"),
    (
        ".manual-installed",
        "Which version of the manual note was installed",
    ),
    (".tour-completed", "That the terminal tour was finished"),
];

const LISTED_ELSEWHERE: [&str; 10] = [
    "notes",
    "chats",
    "attachments",
    "graph.json",
    "captions.json",
    "meaning.json",
    "recordings",
    "leo.db",
    "leo.db-wal",
    "leo.db-shm",
];

pub fn leftover(name: &str) -> bool {
    name.ends_with(crate::db::BEFORE)
        || name.ends_with(".bak")
        || name.contains(".before-")
        || name == "history.txt"
        || name.ends_with(".tmp")
}

fn row_area(id: &str, title: &str, about: &str, path: &Path, bytes: u64, action: Act) -> Area {
    Area {
        id: id.into(),
        title: title.into(),
        about: about.into(),
        path: path.display().to_string(),
        bytes,
        items: Vec::new(),
        actions: if bytes > 0 { vec![action] } else { Vec::new() },
    }
}

pub fn more_areas(store: &Store, chats_dir: &Path, kept: &Kept) -> Vec<Area> {
    let mut out = Vec::new();
    let remembered = chats::remembered(chats_dir);
    if !remembered.is_empty() {
        out.push(Area {
            id: "memory".into(),
            title: "What Felix remembers of long chats".into(),
            about: "Summaries of the older part of long chats, so Felix keeps up once the first messages are out of view. Forgetting one means Felix sees only the recent messages of that chat until it is summarized again.".into(),
            path: chats_dir.display().to_string(),
            bytes: remembered.iter().map(|r| r.3).sum(),
            items: remembered
                .iter()
                .map(|(id, title, upto, bytes)| Item {
                    id: id.clone(),
                    label: title.clone(),
                    detail: format!("a summary of the first {upto} messages"),
                    bytes: *bytes,
                    when: None,
                    locked: false,
                })
                .collect(),
            actions: vec![
                act("forget", "Forget the selected summaries", None, true),
                act(
                    "forget-all",
                    "Forget every summary",
                    Some("Felix forgets what long chats covered before their recent messages. The chats themselves stay."),
                    false,
                ),
            ],
        });
    }
    let reviewed = crate::review::count(chats_dir);
    out.push(row_area(
        "review",
        "Study review progress",
        "Which missed questions you have already reviewed, so Study does not offer them again.",
        chats::db_of(chats_dir).path(),
        reviewed as u64 * 48,
        act(
            "reset",
            "Start review over",
            Some("Questions you already reviewed may be offered again."),
            false,
        ),
    ));
    out.push(row_area(
        "captions",
        "Picture descriptions",
        "What the AI saw in the pictures in your notes, so Felix can use them without looking again. Cleared descriptions are made again when leo is idle, which sends the pictures to the AI again.",
        kept.captions.path(),
        kept.captions.bytes(),
        act(
            "clear",
            "Clear picture descriptions",
            Some("The descriptions are deleted. leo describes the pictures again when it is idle, sending them to the AI again."),
            false,
        ),
    ));
    out.push(row_area(
        "meaning",
        "Index for finding notes by meaning",
        "How each note reads to the meaning model, so search and Felix find notes that mean the same thing in other words. It is made on this computer; nothing is sent anywhere.",
        kept.vectors.path(),
        kept.vectors.bytes(),
        act(
            "clear",
            "Clear the meaning index",
            Some("Finding by meaning stops until leo reads your notes again on this computer, a few minutes later."),
            false,
        ),
    ));
    let data = data_dir(&store.notes_dir);
    let mut items: Vec<Item> = std::fs::read_dir(&data)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    if LISTED_ELSEWHERE.contains(&name.as_str())
                        || CONFIG_FILES.iter().any(|(f, _)| *f == name)
                        || e.file_type().is_ok_and(|t| t.is_symlink())
                    {
                        return None;
                    }
                    let known = SMALL_FILES
                        .iter()
                        .find(|(f, _)| *f == name)
                        .map(|(_, what)| *what);
                    let old = leftover(&name);
                    Some(Item {
                        label: name.clone(),
                        detail: known.map(str::to_string).unwrap_or_else(|| {
                            if old {
                                "An old backup from an earlier version of leo".into()
                            } else {
                                "Not made by this version of leo".into()
                            }
                        }),
                        bytes: size_of(&e.path()),
                        when: modified(&e.path()),
                        locked: !old,
                        id: name,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    items.sort_by(|a, b| a.locked.cmp(&b.locked).then(a.label.cmp(&b.label)));
    if !items.is_empty() {
        let any_old = items.iter().any(|i| !i.locked);
        out.push(Area {
            id: "other".into(),
            title: "Other files beside your notes".into(),
            about: "Small files leo keeps, and old backups left by earlier versions. Only the old backups can be deleted here.".into(),
            path: data.display().to_string(),
            bytes: items.iter().map(|i| i.bytes).sum(),
            items,
            actions: if any_old {
                vec![act("delete", "Delete the selected old backups", None, true)]
            } else {
                Vec::new()
            },
        });
    }
    out
}

pub fn act_on_more(
    store: &Store,
    chats_dir: &Path,
    kept: &Kept,
    request: &Request,
) -> Option<Result<String>> {
    let done = match (request.area.as_str(), request.action.as_str()) {
        ("memory", "forget") => {
            let gone = request
                .items
                .iter()
                .filter(|id| chats::forget_memory(chats_dir, id))
                .count();
            Ok(format!(
                "Felix forgot the summaries of {gone} chat{}.",
                if gone == 1 { "" } else { "s" }
            ))
        }
        ("memory", "forget-all") => {
            let gone = chats::remembered(chats_dir)
                .iter()
                .filter(|(id, ..)| chats::forget_memory(chats_dir, id))
                .count();
            Ok(format!(
                "Felix forgot the summaries of {gone} chat{}.",
                if gone == 1 { "" } else { "s" }
            ))
        }
        ("review", "reset") => {
            crate::review::reset(chats_dir);
            Ok("Study review starts over.".to_string())
        }
        ("captions", "clear") => {
            kept.captions.clear();
            Ok("Cleared the picture descriptions.".to_string())
        }
        ("meaning", "clear") => {
            kept.vectors.clear();
            Ok(
                "Cleared the meaning index; leo reads your notes again on this computer."
                    .to_string(),
            )
        }
        ("other", "delete") => {
            let data = data_dir(&store.notes_dir);
            let mut gone = 0;
            for name in &request.items {
                if name.contains(['/', '\\']) || name.starts_with("..") || !leftover(name) {
                    return Some(Err(anyhow::anyhow!(
                        "Only old backups can be deleted here."
                    )));
                }
                let path = data.join(name);
                let removed = if path.is_dir() {
                    std::fs::remove_dir_all(&path).is_ok()
                } else {
                    path.is_file() && std::fs::remove_file(&path).is_ok()
                };
                if removed {
                    gone += 1;
                }
            }
            Ok(plural_deleted(gone, "old backup"))
        }
        _ => return None,
    };
    Some(done)
}

fn pictures_dir(notes_dir: &Path) -> PathBuf {
    notes_dir.join(leo_core::attachments::DIR)
}

fn picture_users(store: &Store) -> std::collections::HashMap<String, Vec<String>> {
    let dir = pictures_dir(&store.notes_dir);
    let mut users: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for note in &store.notes {
        for line in note.body.lines() {
            for (_, shown) in leo_core::attachments::pictures_in(line) {
                let Some(path) = leo_core::attachments::resolve(
                    &store.notes_dir,
                    &note.directory,
                    &shown.target,
                ) else {
                    continue;
                };
                if path.parent() != Some(dir.as_path()) {
                    continue;
                }
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let titles = users.entry(name).or_default();
                if !titles.contains(&note.title) {
                    titles.push(note.title.clone());
                }
            }
        }
    }
    users
}

fn pictures_area(store: &Store) -> Area {
    let dir = pictures_dir(&store.notes_dir);
    let users = picture_users(store);
    let mut items: Vec<Item> = std::fs::read_dir(&dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.path().is_file())
                .map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    let detail = match users.get(&name).map(Vec::as_slice) {
                        None | Some([]) => "Not in any note".to_string(),
                        Some([one]) => format!("In “{one}”"),
                        Some(many) => format!("In {} notes", many.len()),
                    };
                    Item {
                        label: name.clone(),
                        detail,
                        bytes: size_of(&e.path()),
                        when: modified(&e.path()),
                        locked: false,
                        id: name,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    items.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.label.cmp(&b.label)));
    let mut actions = Vec::new();
    if !items.is_empty() {
        actions.push(act(
            "delete",
            "Delete selected",
            Some("These pictures are deleted for good. A note that shows one will show a missing picture instead."),
            true,
        ));
        if items.iter().any(|i| i.detail == "Not in any note") {
            actions.push(act(
                "unused",
                "Delete pictures no note uses",
                Some("Pictures that no note shows any more are deleted for good."),
                false,
            ));
        }
    }
    Area {
        id: "pictures".into(),
        title: "Pictures in notes".into(),
        about: "Pictures you pasted into notes, and figures kept from uploaded slides, PDFs and photos. They sit beside your notes, so backups include them.".into(),
        path: dir.display().to_string(),
        bytes: size_of(&dir),
        items,
        actions,
    }
}

fn remove_pictures(notes_dir: &Path, names: impl Iterator<Item = String>) -> Result<String> {
    let dir = pictures_dir(notes_dir);
    let mut gone = 0;
    for name in names {
        let plain = !name.is_empty()
            && !name.starts_with('.')
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
        if !plain {
            bail!("That is not a picture leo knows.");
        }
        let path = leo_core::paths::contained_path(&dir, Path::new(&name))?;
        if path.is_file() && std::fs::remove_file(&path).is_ok() {
            gone += 1;
        }
    }
    Ok(plural_deleted(gone, "picture"))
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
        assert_eq!(
            ids,
            [
                "notes",
                "trash",
                "chats",
                "originals",
                "pictures",
                "chat-docs",
                "map"
            ]
        );
        assert!(area(&list, "chat-docs").items.is_empty());
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
        crate::chat_files::add(&chats_dir, "chat-new-0002", "slides.pdf", "text", at(30)).unwrap();
        crate::chat_files::add(&chats_dir, "chat-old-0001", "notes.txt", "text", at(1)).unwrap();
        let docs = areas(&store, &graphs, &chats_dir, at(31));
        let docs = area(&docs, "chat-docs");
        assert_eq!(docs.items.len(), 2);
        assert!(docs
            .items
            .iter()
            .any(|i| i.label == "chat-new-0002" && i.detail == "1 document"));
        assert_eq!(
            run("chat-docs", "delete", &["chat-new-0002", "../escape"]),
            "Felix forgot the documents of 1 chat."
        );
        assert!(crate::chat_files::list(&chats_dir, "chat-new-0002").is_empty());
        assert_eq!(
            chats::list(&chats_dir).len(),
            2,
            "the chats themselves stay"
        );
        assert_eq!(run("chats", "older", &[]), "Deleted 1 chat.");
        assert!(
            crate::chat_files::held(&chats_dir).is_empty(),
            "a deleted chat takes its documents with it"
        );
        assert_eq!(chats::list(&chats_dir).len(), 1);
        assert_eq!(
            run("originals", "orphans", &[]),
            "Deleted the uploaded files of 1 note."
        );
        assert_eq!(size_of(&attachments_dir(&store.notes_dir)), 10);
        assert_eq!(run("trash", "empty", &[]), "Emptied the trash (1 note).");
        assert!(store.trashed().is_empty());
        assert_eq!(run("map", "clear", &[]), "Cleared the knowledge graph.");
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
    fn everything_else_leo_keeps_is_listed_and_can_be_cleared_or_is_said_to_stay() {
        let (tmp, store, _graphs, chats_dir) = setup();
        let data = tmp.path();
        chats::set_memory(
            &chats_dir,
            "chat-new-0002",
            chats::Memory {
                upto: 20,
                hash: "h".into(),
                text: "The student studies graphs.".into(),
            },
        );
        let started = chats::list(&chats_dir)[0].id.clone();
        crate::review::mark_reviewed(&chats_dir, &[format!("{started}:0000000000000001")]).unwrap();
        let captions = crate::captions::Captions::for_notes(&store.notes_dir);
        captions.put("k".into(), "a heap".into());
        let vectors = crate::vectors::Vectors::for_notes(&store.notes_dir);
        crate::vectors::catch_up(&store, &vectors, &crate::vectors::fake_meaning(), 50);
        for name in [
            "keep.json",
            "config.toml",
            "notes.json.bak",
            "config.toml.before-8192",
            "mystery.dat",
        ] {
            std::fs::write(data.join(name), "x").unwrap();
        }
        let kept = Kept {
            captions: &captions,
            vectors: &vectors,
        };
        let list = more_areas(&store, &chats_dir, &kept);
        let ids: Vec<&str> = list.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["memory", "review", "captions", "meaning", "other"]);
        let memory = area(&list, "memory");
        assert_eq!(memory.items[0].id, "chat-new-0002");
        assert_eq!(memory.items[0].detail, "a summary of the first 20 messages");
        let other = area(&list, "other");
        let rows: Vec<(&str, bool)> = other
            .items
            .iter()
            .map(|i| (i.label.as_str(), i.locked))
            .collect();
        assert_eq!(
            rows,
            [
                ("config.toml.before-8192", false),
                ("notes.json.bak", false),
                ("keep.json", true),
                ("mystery.dat", true)
            ],
            "settings files are listed under settings, not here"
        );
        assert!(area(&list, "captions").bytes > 0 && area(&list, "meaning").bytes > 0);

        let ask = |area: &str, action: &str, items: &[&str]| {
            act_on_more(
                &store,
                &chats_dir,
                &kept,
                &Request {
                    area: area.into(),
                    action: action.into(),
                    items: items.iter().map(|s| s.to_string()).collect(),
                },
            )
            .unwrap()
        };
        assert_eq!(
            ask("memory", "forget", &["chat-new-0002"]).unwrap(),
            "Felix forgot the summaries of 1 chat."
        );
        assert!(chats::load(&chats_dir, "chat-new-0002")
            .unwrap()
            .memory
            .is_none());
        assert_eq!(
            chats::load(&chats_dir, "chat-new-0002")
                .unwrap()
                .messages
                .len(),
            1,
            "the chat stays"
        );
        ask("review", "reset", &[]).unwrap();
        assert_eq!(crate::review::count(&chats_dir), 0);
        ask("captions", "clear", &[]).unwrap();
        assert!(captions.is_empty() && captions.bytes() == 0);
        ask("meaning", "clear", &[]).unwrap();
        assert!(vectors.is_empty() && vectors.bytes() == 0);
        assert!(
            ask("other", "delete", &["keep.json"]).is_err(),
            "only old backups go"
        );
        assert!(ask("other", "delete", &["../notes"]).is_err());
        assert_eq!(
            ask(
                "other",
                "delete",
                &["notes.json.bak", "config.toml.before-8192"]
            )
            .unwrap(),
            plural_deleted(2, "old backup")
        );
        assert!(data.join("keep.json").exists() && !data.join("notes.json.bak").exists());
        let after = more_areas(&store, &chats_dir, &kept);
        let ids: Vec<&str> = after.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["review", "captions", "meaning", "other"]);
        assert!(
            area(&after, "other").actions.is_empty(),
            "nothing left that can go"
        );
    }

    #[test]
    fn sizes_read_like_a_file_browser() {
        assert_eq!(sized(0), "0 bytes");
        assert_eq!(sized(1536), "1.5 KB");
        assert_eq!(sized(670 * 1024 * 1024), "670 MB");
        assert_eq!(sized(3 * 1024 * 1024 * 1024), "3.0 GB");
    }

    #[test]
    fn pictures_show_which_notes_use_them_and_unused_ones_can_go() {
        let (_tmp, mut store, graphs, chats_dir) = setup();
        let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x10\0\0\0\x10\x08\x02\0\0\0";
        let used = leo_core::attachments::save(&store.notes_dir, "board.png", png).unwrap();
        let spare = leo_core::attachments::save(&store.notes_dir, "spare.png", png).unwrap();
        let id = store.notes[0].id.clone();
        store.find_note_mut(&id).unwrap().body = format!("Look:\n![Board]({used})");
        store.save().unwrap();
        let list = areas(&store, &graphs, &chats_dir, at(31));
        let pictures = area(&list, "pictures");
        let detail = |path: &str| {
            let name = path.rsplit('/').next().unwrap();
            pictures
                .items
                .iter()
                .find(|i| i.id == name)
                .unwrap()
                .detail
                .clone()
        };
        assert_eq!(detail(&used), "In “Kept”");
        assert_eq!(detail(&spare), "Not in any note");
        assert_eq!(pictures.bytes, 2 * png.len() as u64);
        assert!(pictures.actions.iter().any(|a| a.id == "unused"));
        let notes_before = area(&list, "notes").bytes;
        let extra = leo_core::attachments::save(&store.notes_dir, "extra.png", png).unwrap();
        let after = areas(&store, &graphs, &chats_dir, at(31));
        assert_eq!(
            area(&after, "notes").bytes,
            notes_before,
            "pictures are not counted as notes"
        );
        std::fs::remove_file(store.notes_dir.join(extra)).unwrap();

        let run = |action: &str, items: Vec<String>| {
            act_on(
                &store,
                &graphs,
                &chats_dir,
                &Request {
                    area: "pictures".into(),
                    action: action.into(),
                    items,
                },
                at(31),
            )
            .unwrap()
        };
        assert_eq!(run("unused", vec![]).unwrap(), "Deleted 1 picture.");
        assert!(store.notes_dir.join(&used).is_file());
        assert!(!store.notes_dir.join(&spare).exists());
        for bad in ["../notes/x.md", ".hidden", "a/b.png", ""] {
            assert!(run("delete", vec![bad.into()]).is_err(), "{bad}");
        }
        let name = used.rsplit('/').next().unwrap().to_string();
        assert_eq!(run("delete", vec![name]).unwrap(), "Deleted 1 picture.");
        assert!(!store.notes_dir.join(&used).exists());
    }
}
