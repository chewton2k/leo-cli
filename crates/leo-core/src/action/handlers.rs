//! Applying an [`Action`] to the store, and the second-phase handlers that
//! finish what an [`Effect`] started.

use anyhow::Result;
use chrono::{DateTime, Utc};

use super::resolve::resolve_or_return;
use super::*;

/// Read-only context a handler needs from its shell.
#[derive(Debug, Clone, Copy)]
pub struct Ctx<'a> {
    /// Directory the user is currently in; `""` is root.
    pub current_dir: &'a str,
    /// Note IDs behind the current 1-based numbering.
    pub numbering: &'a [String],
    /// The note the user is looking at, which is what a command means when it
    /// names no note. `None` on the CLI, which has no selection.
    pub selected: Option<&'a str>,
    /// Notes marked in the panes. When there are any, a command that names no
    /// note means all of them rather than the selection.
    pub marked: &'a [String],
}

/// Put the marked notes, or else the selected one, into an action that named
/// none.
///
/// A command that leaves its note out means the one on screen, so nobody has to
/// read a number off the list to act on what they are already looking at. With
/// nothing selected — always the case on the CLI — it says so instead of
/// guessing.
pub fn fill_selected(
    action: Action,
    selected: Option<&str>,
    marked: &[String],
) -> std::result::Result<Action, Line> {
    let omitted = match &action {
        Action::Edit { note }
        | Action::Delete { note }
        | Action::Ask { note }
        | Action::Pin { note }
        | Action::Rename { note, .. } => note.is_empty(),
        Action::Mv { notes, .. } => notes.is_empty(),
        Action::Listen {
            append_to: Some(note),
            ..
        } => note.is_empty(),
        _ => false,
    };
    if !omitted {
        return Ok(action);
    }
    if !marked.is_empty() {
        match action {
            Action::Delete { .. } => {
                return Ok(Action::DeleteMany {
                    ids: marked.to_vec(),
                })
            }
            Action::Mv { dir, .. } => {
                return Ok(Action::Mv {
                    notes: marked.to_vec(),
                    dir,
                })
            }
            _ => {}
        }
    }
    let Some(id) = selected else {
        return Err(Line::bad(
            "No note selected. Select one, or name it: a number, a title, or an ID.",
        ));
    };
    let id = id.to_string();
    Ok(match action {
        Action::Edit { .. } => Action::Edit { note: id },
        Action::Delete { .. } => Action::Delete { note: id },
        Action::Ask { .. } => Action::Ask { note: id },
        Action::Pin { .. } => Action::Pin { note: id },
        Action::Rename { title, .. } => Action::Rename { note: id, title },
        Action::Mv { dir, .. } => Action::Mv {
            notes: vec![id],
            dir,
        },
        Action::Listen { title, screen, .. } => Action::Listen {
            title,
            append_to: Some(id),
            screen,
        },
        other => other,
    })
}

/// Apply an action. The only entry point a shell needs.
pub fn apply(action: Action, store: &mut Store, ctx: Ctx<'_>, ai: &dyn Ai) -> Result<Outcome> {
    let action = match fill_selected(action, ctx.selected, ctx.marked) {
        Ok(action) => action,
        Err(line) => return Ok(Outcome::line(line)),
    };
    match action {
        Action::New { title } => Ok(new_note(store, title, ctx.current_dir)),
        Action::List { tag, limit } => Ok(list(store, tag.as_deref(), limit, ctx.current_dir)),
        Action::View { note } => Ok(view(store, &note, ctx.numbering)),
        Action::Edit { note } => Ok(edit(store, &note, ctx.numbering)),
        Action::Delete { note } => Ok(delete(store, &note, ctx.numbering)),
        Action::DeleteMany { ids } => Ok(delete_many(store, &ids)),
        Action::Check { note, index } => check(store, &note, index, ctx.numbering),
        Action::Search { query } => Ok(search(store, &query)),
        Action::Listen {
            title,
            append_to,
            screen,
        } => Ok(listen(store, title, append_to, screen, ctx.current_dir)),
        Action::Ask { note } => ask(store, &note, ctx.numbering, ai),
        Action::Undo => undo(store),
        Action::Mkdir { name } => mkdir(store, &name, ctx.current_dir),
        Action::Cd { path } => Ok(cd(store, &path, ctx.current_dir)),
        Action::Mv { notes, dir } => mv(store, &notes, &dir, ctx.numbering),
        Action::Rename { note, title } => rename(store, &note, &title, ctx.numbering),
        Action::Pin { note } => pin(store, &note, ctx.numbering),
        Action::Rmdir { name, recursive } => rmdir(store, &name, recursive, ctx.current_dir),
        Action::Sync(a) => Ok(Outcome::effect(Effect::Sync(a))),
        Action::Trash(a) => trash(store, a),
        Action::Help => Ok(Outcome::effect(Effect::ShowHelp)),
        Action::Doctor => Ok(Outcome::effect(Effect::Doctor)),
        Action::Quit => Ok(Outcome::effect(Effect::Quit)),
    }
}

/// `new` — ask the shell to open an editor on a frontmatter template.
pub(super) fn new_note(store: &Store, line: Option<String>, current_dir: &str) -> Outcome {
    let (dir, title, tags) = split_new(store, line.as_deref().unwrap_or(""), current_dir);
    let path = std::env::temp_dir().join(format!("leo-new-{}.md", uuid::Uuid::new_v4()));
    Outcome::effect(Effect::Edit(EditRequest {
        seed: format!("---\ntitle: {title}\ntags: {}\n---\n", tags.join(", ")),
        path,
        target: EditTarget::NewNote {
            fallback_title: title,
            dir,
        },
    }))
}

/// Split `new`'s line into where the note goes, its title, and its tags.
///
/// `#word` is a tag. A leading `dir/` names a directory, relative to the
/// current one, when that directory exists — or when nothing follows the slash,
/// which asks for it to be made. Otherwise a slash is part of the title, so
/// "TCP/IP basics" stays a title.
pub fn split_new(store: &Store, line: &str, current_dir: &str) -> (String, String, Vec<String>) {
    let (tags, words): (Vec<&str>, Vec<&str>) = line
        .split_whitespace()
        .partition(|w| w.len() > 1 && w.starts_with('#'));
    let tags = tags.iter().map(|t| t[1..].to_string()).collect();

    if let Some((prefix, first)) = words.first().and_then(|w| w.rsplit_once('/')) {
        let dir = under(current_dir, prefix);
        if !prefix.is_empty() && (first.is_empty() || store.dir_exists(&dir)) {
            let title = std::iter::once(first)
                .chain(words[1..].iter().copied())
                .filter(|w| !w.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            return (dir, title, tags);
        }
    }
    (current_dir.to_string(), words.join(" "), tags)
}

/// `list` — subdirectories first, then notes, and renumber.
pub(super) fn list(store: &Store, tag: Option<&str>, limit: usize, dir: &str) -> Outcome {
    let subdirs = store.subdirs(dir);
    let notes = store.list_notes_in_dir(dir, tag, limit);

    if subdirs.is_empty() && notes.is_empty() {
        let line = if tag.is_some() {
            Line::dim("No notes with that tag.")
        } else {
            Line::dim("No notes yet. Type `new` to create one.")
        };
        return Outcome {
            selection: Some(Vec::new()),
            ..Outcome::line(line)
        };
    }

    let mut lines = vec![Line::blank()];
    for name in &subdirs {
        lines.push(Line::dir(format!("{name}/")));
    }
    if !subdirs.is_empty() && !notes.is_empty() {
        lines.push(Line::blank());
    }

    let mut selection = Vec::with_capacity(notes.len());
    for (i, note) in notes.iter().enumerate() {
        selection.push(note.id.clone());
        lines.push(Line::plain(format!(
            "{:>3} {}",
            i + 1,
            note.format_summary()
        )));
    }
    lines.push(Line::blank());

    Outcome {
        selection: Some(selection),
        ..Outcome::lines(lines)
    }
}

pub(super) fn view(store: &Store, note: &str, numbering: &[String]) -> Outcome {
    let id = match resolve(note, store, numbering) {
        Resolved::One(id) => id,
        other => return unresolved(note, other),
    };
    Outcome::effect(Effect::ShowNote { id })
}

/// `edit` — hand the shell a temp file seeded with the note's current content.
pub(super) fn edit(store: &Store, note: &str, numbering: &[String]) -> Outcome {
    let id = match resolve(note, store, numbering) {
        Resolved::One(id) => id,
        other => return unresolved(note, other),
    };
    let n = store.find_note(&id).expect("resolve returned a live id");
    let seed = format!(
        "---\ntitle: {}\ntags: {}\n---\n{}",
        n.title,
        n.tags.join(", "),
        n.body
    );
    let path = std::env::temp_dir().join(format!("leo-{}.md", &id[..std::cmp::min(8, id.len())]));
    Outcome::effect(Effect::Edit(EditRequest {
        path,
        seed,
        target: EditTarget::Existing {
            id: id.clone(),
            old_title: n.title.clone(),
            old_tags: n.tags.clone(),
            old_body: n.body.clone(),
        },
    }))
}

pub(super) fn delete(store: &Store, note: &str, numbering: &[String]) -> Outcome {
    let id = match resolve(note, store, numbering) {
        Resolved::One(id) => id,
        other => return unresolved(note, other),
    };
    let title = store
        .find_note(&id)
        .expect("resolve returned a live id")
        .title
        .clone();
    Outcome::effect(Effect::Confirm {
        prompt: format!("Delete {title}?"),
        on_yes: ConfirmedAction::DeleteNote { id, title },
    })
}

/// Delete several notes, asking once.
pub(super) fn delete_many(store: &Store, ids: &[String]) -> Outcome {
    let ids: Vec<String> = ids
        .iter()
        .filter(|id| store.find_note(id).is_some())
        .cloned()
        .collect();
    if ids.is_empty() {
        return Outcome::line(Line::dim("Nothing to delete."));
    }
    Outcome::effect(Effect::Confirm {
        prompt: format!("Delete {} note{}?", ids.len(), plural(ids.len())),
        on_yes: ConfirmedAction::DeleteNotes { ids },
    })
}

pub(super) fn check(
    store: &mut Store,
    note: &str,
    index: usize,
    numbering: &[String],
) -> Result<Outcome> {
    let id = resolve_or_return!(note, store, numbering);
    match store.toggle_checkbox(&id, index) {
        Some(state) => {
            store.save()?;
            Ok(Outcome {
                dirty: true,
                ..Outcome::line(Line::plain(state))
            })
        }
        None => Ok(Outcome::line(Line::bad(format!(
            "No checkbox #{index} in that note."
        )))),
    }
}

pub(super) fn search(store: &Store, query: &str) -> Outcome {
    let results = store.find(query);
    if results.is_empty() {
        return Outcome {
            selection: Some(Vec::new()),
            ..Outcome::line(Line::dim(format!("No notes match '{query}'.")))
        };
    }

    let mut lines = vec![Line::blank()];
    let mut selection = Vec::with_capacity(results.len());
    for (i, note) in results.iter().enumerate() {
        selection.push(note.id.clone());
        let dir_info = if note.directory.is_empty() {
            String::new()
        } else {
            format!("  {}/", note.directory)
        };
        lines.push(Line::plain(format!(
            "{:>3} {}{}",
            i + 1,
            note.format_summary(),
            dir_info
        )));
        if let Some(line) = note.matching_line(query) {
            lines.push(Line::dim(format!("      … {line}")));
        }
    }
    lines.push(Line::blank());
    Outcome {
        selection: Some(selection),
        ..Outcome::lines(lines)
    }
}

/// `listen` — validate the append target before spending time recording.
pub(super) fn listen(
    store: &Store,
    title: Option<String>,
    append_to: Option<String>,
    screen: bool,
    dir: &str,
) -> Outcome {
    if let Some(target) = &append_to {
        if store.find_by_index_or_prefix(target).is_none() {
            return Outcome::line(Line::bad(format!("No note found: {target}")));
        }
    }
    Outcome::effect(Effect::Listen(ListenRequest {
        screen,
        title,
        append_to,
        dir: dir.to_string(),
    }))
}

pub(super) fn ask(
    store: &mut Store,
    note: &str,
    numbering: &[String],
    ai: &dyn Ai,
) -> Result<Outcome> {
    // Words that name no note, and are more than one word, are a question for
    // all of them.
    if matches!(resolve(note, store, numbering), Resolved::None)
        && note.trim().contains(char::is_whitespace)
    {
        return Ok(Outcome::effect(Effect::AskNotes {
            question: note.trim().to_string(),
        }));
    }
    let id = resolve_or_return!(note, store, numbering);
    let (title, body) = {
        let n = store.find_note(&id).expect("resolve returned a live id");
        (n.title.clone(), n.body.clone())
    };

    let count = body.lines().filter(|l| is_leo_prompt(l).is_some()).count();
    if count == 0 {
        return Ok(Outcome::line(Line::dim(
            "No @leo prompts found in this note.",
        )));
    }

    let (expanded, _) = ai.expand_prompts(&body, &title)?;

    let n = store
        .find_note_mut(&id)
        .expect("resolve returned a live id");
    n.body = expanded;
    n.updated_at = chrono::Utc::now();
    let short = n.id[..std::cmp::min(8, n.id.len())].to_string();
    let title = n.title.clone();
    store.save()?;
    Ok(Outcome {
        dirty: true,
        ..Outcome::line(Line::good(format!("Updated \"{title}\" {short}")))
    })
}

/// `undo` — take back the last destructive change.
///
/// A handler rather than a TUI-only key, so the same step back works from the `:`
/// line and reuses the store's stack instead of a second one.
pub(super) fn undo(store: &mut Store) -> Result<Outcome> {
    match store.undo() {
        Some(what) => {
            store.save()?;
            Ok(Outcome {
                lines: vec![Line::good(what)],
                dirty: true,
                ..Outcome::default()
            })
        }
        None => Ok(Outcome::line(Line::dim("Nothing to undo."))),
    }
}

/// Join a name onto the current directory, tolerating stray slashes.
pub(super) fn under(current_dir: &str, name: &str) -> String {
    if current_dir.is_empty() {
        name.trim_matches('/').to_string()
    } else {
        format!("{}/{}", current_dir, name.trim_matches('/'))
    }
}

pub(super) fn mkdir(store: &mut Store, name: &str, current_dir: &str) -> Result<Outcome> {
    let full = under(current_dir, name);
    if store.dir_exists(&full) {
        return Ok(Outcome::line(Line::dim(format!(
            "Directory already exists: {full}/"
        ))));
    }
    store.create_dir(&full);
    store.save()?;
    Ok(Outcome {
        dirty: true,
        ..Outcome::line(Line::good(format!("Created {full}/")))
    })
}

/// `cd` — resolve `..`, `/`, `~`, and `../sibling` against the current
/// directory. Pure path arithmetic plus one existence check.
pub fn resolve_cd(
    path: &str,
    store: &Store,
    current_dir: &str,
) -> std::result::Result<String, String> {
    let target = path.trim();
    if target.is_empty() || target == "/" || target == "~" {
        return Ok(String::new());
    }

    let parent_of = |dir: &str| -> String {
        match dir.rfind('/') {
            Some(pos) => dir[..pos].to_string(),
            None => String::new(),
        }
    };

    if target == ".." {
        return Ok(parent_of(current_dir));
    }

    let mut base = current_dir.to_string();
    let mut remaining = target;
    while let Some(rest) = remaining.strip_prefix("../") {
        base = parent_of(&base);
        remaining = rest;
    }
    if remaining == ".." {
        base = parent_of(&base);
        remaining = "";
    }

    let full = if remaining.is_empty() {
        base
    } else if remaining.starts_with('/') || base.is_empty() {
        // An absolute path ignores the base; an empty base has nothing to join.
        remaining.trim_matches('/').to_string()
    } else {
        format!("{}/{}", base, remaining.trim_matches('/'))
    };

    if full.is_empty() || store.dir_exists(&full) {
        Ok(full)
    } else {
        Err(format!("No such directory: {full}/"))
    }
}

pub(super) fn cd(store: &Store, path: &str, current_dir: &str) -> Outcome {
    match resolve_cd(path, store, current_dir) {
        Ok(dir) => Outcome {
            new_dir: Some(dir),
            dirty: true,
            ..Outcome::empty()
        },
        Err(msg) => Outcome::line(Line::bad(msg)),
    }
}

/// `rename` — change a note's title and nothing else.
/// `pin` — keep a note at the top of its list, or let it go back into date
/// order. Not an edit, so it leaves the note's modified time alone.
pub(super) fn pin(store: &mut Store, note: &str, numbering: &[String]) -> Result<Outcome> {
    let id = resolve_or_return!(note, store, numbering);
    let n = store
        .find_note_mut(&id)
        .expect("resolve returned a live id");
    n.pinned = !n.pinned;
    let line = if n.pinned {
        format!("Pinned \"{}\" to the top.", n.title)
    } else {
        format!("Unpinned \"{}\".", n.title)
    };
    store.save()?;
    Ok(Outcome {
        dirty: true,
        select: Some(id),
        ..Outcome::line(Line::good(line))
    })
}

pub(super) fn rename(
    store: &mut Store,
    note: &str,
    title: &str,
    numbering: &[String],
) -> Result<Outcome> {
    let id = resolve_or_return!(note, store, numbering);
    let title = title.trim();
    let n = store
        .find_note_mut(&id)
        .expect("resolve returned a live id");
    let old = std::mem::replace(&mut n.title, title.to_string());
    n.updated_at = chrono::Utc::now();
    store.save()?;
    Ok(Outcome {
        dirty: true,
        ..Outcome::line(Line::good(format!("Renamed \"{old}\" to \"{title}\"")))
    })
}

pub(super) fn mv(
    store: &mut Store,
    notes: &[String],
    dir: &str,
    numbering: &[String],
) -> Result<Outcome> {
    if !dir.is_empty() && !store.dir_exists(dir) {
        return Ok(Outcome::line(Line::bad(format!(
            "No such directory: {dir}/"
        ))));
    }

    let mut lines = Vec::new();
    let mut ids = Vec::new();
    for arg in notes {
        match resolve(arg, store, numbering) {
            Resolved::One(id) => ids.push(id),
            other => lines.extend(unresolved(arg, other).lines),
        }
    }

    // One store call, so one `u` takes the whole move back.
    let moved = store.move_notes(&ids, dir);
    let dest = if dir.is_empty() { "/" } else { dir };
    match moved.as_slice() {
        [] => {}
        [title] => lines.push(Line::good(format!("Moved \"{title}\" to {dest}"))),
        many => lines.push(Line::good(format!("Moved {} notes to {dest}", many.len()))),
    }
    if !moved.is_empty() {
        store.save()?;
    }
    Ok(Outcome {
        dirty: !moved.is_empty(),
        ..Outcome::lines(lines)
    })
}

pub(super) fn rmdir(
    store: &mut Store,
    name: &str,
    recursive: bool,
    current_dir: &str,
) -> Result<Outcome> {
    let full = under(current_dir, name);
    if !store.dir_exists(&full) {
        return Ok(Outcome::line(Line::bad(format!(
            "No such directory: {full}/"
        ))));
    }

    if recursive {
        let (notes, dirs) = store.dir_contents(&full);
        // An empty directory needs no warning, so delete it outright.
        if notes == 0 && dirs <= 1 {
            store.delete_dir_recursive(&full);
            store.save()?;
            return Ok(Outcome {
                dirty: true,
                ..Outcome::line(Line::dim(format!("Removed {full}/")))
            });
        }
        // Otherwise say exactly what will be lost before asking.
        let mut what = Vec::new();
        if notes > 0 {
            what.push(format!("{notes} note{}", plural(notes)));
        }
        if dirs > 1 {
            what.push(format!(
                "{} subdirector{}",
                dirs - 1,
                if dirs - 1 == 1 { "y" } else { "ies" }
            ));
        }
        return Ok(Outcome::effect(Effect::Confirm {
            prompt: format!("Delete {full}/ and its {}?", what.join(" and ")),
            on_yes: ConfirmedAction::DeleteDir { path: full },
        }));
    }

    if store.delete_dir(&full) {
        store.save()?;
        Ok(Outcome {
            dirty: true,
            ..Outcome::line(Line::dim(format!("Removed {full}/")))
        })
    } else {
        // Name the way out rather than just refusing.
        Ok(Outcome::line(Line::bad(format!(
            "{full}/ is not empty — use `rmdir -r {name}` to delete it and its contents"
        ))))
    }
}

/// "s" unless there is exactly one.
pub(super) fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

// ── Second-phase handlers ───────────────────────────────────────────────────
// These take the result of an Effect the shell performed and finish the work.
// Keeping them pure over `&mut Store` means the editor, microphone, and
// confirmation prompt are the only parts a test cannot exercise.

/// Finish an editor session started by [`Effect::Edit`].
pub fn apply_edit(
    store: &mut Store,
    target: &EditTarget,
    raw: &str,
    ai: &dyn Ai,
) -> Result<Outcome> {
    let (parsed_title, parsed_tags, body) = parse_frontmatter(raw);

    match target {
        EditTarget::NewNote {
            fallback_title,
            dir,
        } => {
            if body.trim().is_empty() {
                return Ok(Outcome::line(Line::dim("Empty note, cancelled.")));
            }
            let title = if parsed_title.is_empty() {
                fallback_title.clone()
            } else {
                parsed_title
            };
            if !store.dir_exists(dir) {
                store.create_dir(dir);
            }
            let note = store.create_note(title, body, parsed_tags, dir)?;
            let id = note.id.clone();
            let short = id[..std::cmp::min(8, id.len())].to_string();
            store.save()?;
            Ok(Outcome {
                dirty: true,
                select: Some(id),
                ..Outcome::line(Line::good(format!("Created {short}")))
            })
        }

        EditTarget::Existing {
            id,
            old_title,
            old_tags,
            old_body,
        } => {
            let title = if parsed_title.is_empty() {
                old_title.clone()
            } else {
                parsed_title
            };
            let mut body = body;

            // Expand any @leo prompts the user added, in one pass, before saving.
            let count = body.lines().filter(|l| is_leo_prompt(l).is_some()).count();
            let mut lines = Vec::new();
            if count > 0 {
                match ai.expand_prompts(&body, &title) {
                    Ok((expanded, n)) => {
                        body = expanded;
                        lines.push(Line::dim(format!(
                            "Expanded {n} prompt{}",
                            if n == 1 { "" } else { "s" }
                        )));
                    }
                    // A failed expansion must not lose the user's edit.
                    Err(e) => lines.push(Line::warn(format!("Expansion failed: {e}"))),
                }
            }

            if title == *old_title && parsed_tags == *old_tags && body.trim() == old_body.trim() {
                lines.push(Line::dim("No changes."));
                return Ok(Outcome::lines(lines));
            }

            let note = store
                .find_note_mut(id)
                .ok_or_else(|| anyhow::anyhow!("note {id} disappeared while editing"))?;
            note.title = title.clone();
            note.tags = parsed_tags;
            note.body = body;
            note.updated_at = chrono::Utc::now();
            store.save()?;
            lines.push(Line::good(format!("Updated {title}")));
            Ok(Outcome {
                dirty: true,
                ..Outcome::lines(lines)
            })
        }
    }
}

/// Apply a confirmed destructive action.
pub fn apply_confirmed(store: &mut Store, action: &ConfirmedAction) -> Result<Outcome> {
    match action {
        ConfirmedAction::DeleteNote { id, .. } => {
            if store.delete_note(id) {
                store.save()?;
                Ok(Outcome {
                    dirty: true,
                    ..Outcome::line(Line::good(format!(
                        "Moved to the trash, kept {} days.",
                        crate::store::TRASH_DAYS
                    )))
                })
            } else {
                Ok(Outcome::line(Line::bad("Nothing deleted.")))
            }
        }

        ConfirmedAction::DeleteNotes { ids } => {
            let n = store.delete_notes(ids);
            if n > 0 {
                store.save()?;
            }
            Ok(Outcome {
                dirty: n > 0,
                ..Outcome::line(Line::good(format!(
                    "Moved {n} note{} to the trash, kept {} days.",
                    plural(n),
                    crate::store::TRASH_DAYS
                )))
            })
        }
        ConfirmedAction::DeleteDir { path } => {
            let (notes, dirs) = store.delete_dir_recursive(path);
            if notes == 0 && dirs == 0 {
                return Ok(Outcome::line(Line::bad("Nothing deleted.")));
            }
            store.save()?;
            let mut parts = vec![format!("Removed {path}/")];
            if notes > 0 {
                parts.push(format!(
                    "and moved its {notes} note{} to the trash",
                    plural(notes)
                ));
            }
            Ok(Outcome {
                dirty: true,
                ..Outcome::line(Line::good(parts.join(" ")))
            })
        }
        ConfirmedAction::EmptyTrash => {
            let n = store.empty_trash()?;
            Ok(Outcome::line(Line::good(format!(
                "Deleted {n} note{} for good.",
                plural(n)
            ))))
        }
    }
}

/// `trash` — list what was deleted, bring a note back, or empty it.
fn trash(store: &mut Store, action: TrashAction) -> Result<Outcome> {
    let trashed = store.trashed();
    match action {
        TrashAction::List => {
            if trashed.is_empty() {
                return Ok(Outcome::line(Line::dim(format!(
                    "The trash is empty. Deleted notes stay there for {} days.",
                    crate::store::TRASH_DAYS
                ))));
            }
            let now = Utc::now();
            let mut lines = vec![
                Line::plain(format!(
                    "In the trash, kept {} days after deleting:",
                    crate::store::TRASH_DAYS
                )),
                Line::blank(),
            ];
            for (i, note) in trashed.iter().enumerate() {
                lines.push(Line::plain(format!(
                    "{:>3}  {}   /{} · deleted {}",
                    i + 1,
                    note.title,
                    note.directory,
                    ago(note.deleted_at, now)
                )));
            }
            lines.push(Line::blank());
            lines.push(Line::dim(
                "trash restore <number> brings one back · trash empty deletes them for good",
            ));
            Ok(Outcome::lines(lines))
        }
        TrashAction::Restore { which } => {
            let found = match which.trim().parse::<usize>() {
                Ok(n) if n >= 1 => trashed.get(n - 1).cloned(),
                Ok(_) => None,
                Err(_) => {
                    let lower = which.trim().to_lowercase();
                    let mut matches = trashed
                        .iter()
                        .filter(|t| t.title.to_lowercase().contains(&lower));
                    match (matches.next(), matches.next()) {
                        (Some(one), None) => Some(one.clone()),
                        _ => None,
                    }
                }
            };
            let Some(note) = found else {
                return Ok(Outcome::line(Line::bad(format!(
                    "No note \"{which}\" in the trash. `trash` lists what is there."
                ))));
            };
            let Some(title) = store.restore(&note.id) else {
                return Ok(Outcome::line(Line::bad(format!(
                    "\"{}\" could not be restored.",
                    note.title
                ))));
            };
            store.save()?;
            Ok(Outcome {
                dirty: true,
                select: Some(note.id),
                ..Outcome::line(Line::good(format!(
                    "Restored \"{title}\" to /{}.",
                    note.directory
                )))
            })
        }
        TrashAction::Empty => {
            if trashed.is_empty() {
                return Ok(Outcome::line(Line::dim("The trash is already empty.")));
            }
            let n = trashed.len();
            Ok(Outcome::effect(Effect::Confirm {
                prompt: format!(
                    "Delete the {n} note{} in the trash for good? This cannot be undone.",
                    plural(n)
                ),
                on_yes: ConfirmedAction::EmptyTrash,
            }))
        }
    }
}

/// How long ago `then` was, the way a person would say it.
fn ago(then: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let secs = (now - then).num_seconds().max(0);
    let (n, unit) = match secs {
        0..60 => return "just now".to_string(),
        60..3600 => (secs / 60, "minute"),
        3600..86400 => (secs / 3600, "hour"),
        _ => (secs / 86400, "day"),
    };
    format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" })
}

/// Turn a finished recording's transcript into a saved note.
pub fn apply_transcript(
    store: &mut Store,
    req: &ListenRequest,
    transcript: &str,
    ai: &dyn Ai,
) -> Result<Outcome> {
    if transcript.trim().is_empty() {
        return Ok(Outcome::line(Line::dim("No speech detected.")));
    }

    if let Some(target) = &req.append_to {
        let existing = match store.find_by_index_or_prefix(target) {
            Some(n) => n.body.clone(),
            None => return Ok(Outcome::line(Line::bad(format!("No note found: {target}")))),
        };
        let addition = ai.structure_append(transcript, &existing)?;
        let note = store
            .find_by_index_or_prefix_mut(target)
            .expect("target existed a moment ago");
        note.body = format!("{}\n\n{}", note.body, addition);
        note.updated_at = chrono::Utc::now();
        let title = note.title.clone();
        let id = note.id.clone();
        let short = id[..std::cmp::min(8, id.len())].to_string();
        store.save()?;
        return Ok(Outcome {
            dirty: true,
            select: Some(id),
            ..Outcome::line(Line::good(format!("Updated \"{title}\" {short}")))
        });
    }

    let (ai_title, body) = ai.structure(transcript)?;
    let title = req.title.clone().unwrap_or(ai_title);
    let note = store.create_note(&title, &body, vec!["listen".to_string()], &req.dir)?;
    let id = note.id.clone();
    let short = id[..std::cmp::min(8, id.len())].to_string();
    store.save()?;
    Ok(Outcome {
        dirty: true,
        select: Some(id),
        ..Outcome::line(Line::good(format!("Created \"{title}\" {short}")))
    })
}

/// Recompute the note numbering after the store changed.
pub fn numbering_for(store: &Store, dir: &str) -> Vec<String> {
    store
        .list_notes_in_dir(dir, None, usize::MAX)
        .iter()
        .map(|n| n.id.clone())
        .collect()
}

/// The numbering while a search is active: every matching note in every
/// directory, ranked by [`Store::find`]. A blank query is no search, so it is
/// the directory's ordinary listing.
pub fn filtered_numbering(store: &Store, dir: &str, query: &str) -> Vec<String> {
    if query.trim().is_empty() {
        return numbering_for(store, dir);
    }
    store.find(query).iter().map(|n| n.id.clone()).collect()
}

#[cfg(test)]
mod handler_tests {
    use super::*;

    /// An `Ai` double: records what it was asked and returns canned answers, so
    /// handler tests never touch the network.
    struct FakeAi {
        expand_to: Option<String>,
        structured: (String, String),
        appended: String,
        fail: bool,
    }

    impl Default for FakeAi {
        fn default() -> Self {
            FakeAi {
                expand_to: None,
                structured: ("AI Title".to_string(), "- ai body".to_string()),
                appended: "- appended".to_string(),
                fail: false,
            }
        }
    }

    impl Ai for FakeAi {
        fn expand_prompts(&self, body: &str, _title: &str) -> Result<(String, usize)> {
            if self.fail {
                anyhow::bail!("no provider available");
            }
            match &self.expand_to {
                Some(text) => Ok((text.clone(), 1)),
                None => Ok((body.to_string(), 0)),
            }
        }
        fn structure(&self, _transcript: &str) -> Result<(String, String)> {
            if self.fail {
                anyhow::bail!("no provider available");
            }
            Ok(self.structured.clone())
        }
        fn structure_append(&self, _transcript: &str, _existing: &str) -> Result<String> {
            if self.fail {
                anyhow::bail!("no provider available");
            }
            Ok(self.appended.clone())
        }
    }

    /// A store on a temp directory, the same pattern `store.rs` tests use.
    fn temp_store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::load_from(&dir.path().join("notes")).unwrap();
        (store, dir)
    }

    fn ctx<'a>(dir: &'a str, numbering: &'a [String]) -> Ctx<'a> {
        Ctx {
            current_dir: dir,
            numbering,
            selected: None,
            marked: &[],
        }
    }

    fn seed(store: &mut Store, title: &str, body: &str, dir: &str) -> String {
        let id = store
            .create_note(title, body, vec![], dir)
            .unwrap()
            .id
            .clone();
        store.save().unwrap();
        id
    }

    fn ctx_selected<'a>(numbering: &'a [String], selected: &'a str) -> Ctx<'a> {
        Ctx {
            current_dir: "",
            numbering,
            selected: Some(selected),
            marked: &[],
        }
    }

    // ── the selected note ───────────────────────────────────────────────────

    /// Leaving the note out means "the one I'm looking at", so the user never
    /// has to read a number off the screen to act on what is already selected.
    #[test]
    fn verbs_that_take_one_note_parse_without_it() {
        let parsed = |line: &str| match parse(line) {
            Parsed::Action(a) => a,
            other => panic!("{line:?} did not parse: {other:?}"),
        };
        assert_eq!(
            parsed("edit"),
            Action::Edit {
                note: String::new()
            }
        );
        assert_eq!(
            parsed("delete"),
            Action::Delete {
                note: String::new()
            }
        );
        assert_eq!(
            parsed("ask"),
            Action::Ask {
                note: String::new()
            }
        );
        assert_eq!(
            parsed("mv cs130"),
            Action::Mv {
                notes: vec![],
                dir: "cs130".to_string()
            }
        );
        assert_eq!(
            parsed("listen add"),
            Action::Listen {
                title: None,
                append_to: Some(String::new()),
                screen: false
            }
        );
    }

    #[test]
    fn an_omitted_note_means_the_selected_one() {
        let (mut store, _d) = temp_store();
        seed(&mut store, "Other", "", "");
        let id = seed(&mut store, "Graphs", "", "");
        let numbering = numbering_for(&store, "");
        let out = apply(
            Action::Delete {
                note: String::new(),
            },
            &mut store,
            ctx_selected(&numbering, &id),
            &FakeAi::default(),
        )
        .unwrap();
        match out.effect {
            Effect::Confirm {
                on_yes: ConfirmedAction::DeleteNote { id: target, .. },
                ..
            } => {
                assert_eq!(target, id)
            }
            other => panic!("expected a delete confirmation, got {other:?}"),
        }
    }

    #[test]
    fn mv_without_notes_moves_the_selected_one() {
        let (mut store, _d) = temp_store();
        store.create_dir("cs130");
        let id = seed(&mut store, "Graphs", "", "");
        let numbering = numbering_for(&store, "");
        apply(
            Action::Mv {
                notes: vec![],
                dir: "cs130".to_string(),
            },
            &mut store,
            ctx_selected(&numbering, &id),
            &FakeAi::default(),
        )
        .unwrap();
        assert_eq!(store.find_note(&id).unwrap().directory, "cs130");
    }

    /// Undo has to reach disk, or quitting brings the undone change back.
    #[test]
    fn an_undo_is_saved() {
        let (mut store, _d) = temp_store();
        let id = seed(&mut store, "Doomed", "b", "");
        apply_confirmed(
            &mut store,
            &ConfirmedAction::DeleteNote {
                id: id.clone(),
                title: "Doomed".into(),
            },
        )
        .unwrap();
        apply(Action::Undo, &mut store, ctx("", &[]), &FakeAi::default()).unwrap();
        let reloaded = Store::load_from(&store.notes_dir).unwrap();
        assert!(
            reloaded.find_note(&id).is_some(),
            "the restored note is not on disk"
        );
    }

    /// A note that has just been made is the one the front end should show.
    #[test]
    fn a_new_note_asks_to_be_selected() {
        let (mut store, _d) = temp_store();
        let target = EditTarget::NewNote {
            fallback_title: "T".into(),
            dir: String::new(),
        };
        let out = apply_edit(
            &mut store,
            &target,
            "---\ntitle: T\n---\nbody",
            &FakeAi::default(),
        )
        .unwrap();
        assert_eq!(out.select.as_deref(), Some(store.notes[0].id.as_str()));
    }

    /// `/ask` with words that are not a note's name is a question for all the
    /// notes; the shell answers it, since that needs the AI.
    #[test]
    fn ask_with_a_question_asks_across_the_notes() {
        let (mut store, _d) = temp_store();
        seed(&mut store, "Graphs", "BFS", "");
        let out = apply(
            Action::Ask {
                note: "what did we cover about graphs".into(),
            },
            &mut store,
            ctx("", &[]),
            &FakeAi::default(),
        )
        .unwrap();
        assert_eq!(
            out.effect,
            Effect::AskNotes {
                question: "what did we cover about graphs".into()
            }
        );
    }

    /// A note's name still means that note's @leo lines, and one unknown word
    /// is still a note that was not found.
    #[test]
    fn ask_with_a_note_name_or_one_word_is_unchanged() {
        let (mut store, _d) = temp_store();
        seed(&mut store, "Graph traversals", "no prompts here", "");
        let named = apply(
            Action::Ask {
                note: "Graph traversals".into(),
            },
            &mut store,
            ctx("", &[]),
            &FakeAi::default(),
        )
        .unwrap();
        assert!(named.text().contains("No @leo prompts"), "{}", named.text());
        let unknown = apply(
            Action::Ask {
                note: "nosuchnote".into(),
            },
            &mut store,
            ctx("", &[]),
            &FakeAi::default(),
        )
        .unwrap();
        assert!(
            unknown.text().contains("No note found"),
            "{}",
            unknown.text()
        );
    }

    // ── marked notes ────────────────────────────────────────────────────────

    fn ctx_marked<'a>(numbering: &'a [String], marked: &'a [String]) -> Ctx<'a> {
        Ctx {
            current_dir: "",
            numbering,
            selected: marked.first().map(String::as_str),
            marked,
        }
    }

    /// With notes marked, a command that names none means all of them.
    #[test]
    fn delete_with_marks_asks_once_for_all_of_them() {
        let (mut store, _d) = temp_store();
        let a = seed(&mut store, "A", "", "");
        let b = seed(&mut store, "B", "", "");
        seed(&mut store, "C", "", "");
        let marked = vec![a.clone(), b.clone()];
        let numbering = numbering_for(&store, "");
        let out = apply(
            Action::Delete {
                note: String::new(),
            },
            &mut store,
            ctx_marked(&numbering, &marked),
            &FakeAi::default(),
        )
        .unwrap();
        let Effect::Confirm { prompt, on_yes } = out.effect else {
            panic!("expected a confirmation, got {:?}", out.effect);
        };
        assert_eq!(prompt, "Delete 2 notes?");
        apply_confirmed(&mut store, &on_yes).unwrap();
        assert_eq!(store.notes.len(), 1);
        store.undo().unwrap();
        assert_eq!(store.notes.len(), 3, "one undo brings both back");
    }

    #[test]
    fn mv_with_marks_moves_all_of_them_as_one_undo() {
        let (mut store, _d) = temp_store();
        store.create_dir("cs130");
        let a = seed(&mut store, "A", "", "");
        let b = seed(&mut store, "B", "", "");
        let marked = vec![a.clone(), b.clone()];
        let numbering = numbering_for(&store, "");
        apply(
            Action::Mv {
                notes: vec![],
                dir: "cs130".to_string(),
            },
            &mut store,
            ctx_marked(&numbering, &marked),
            &FakeAi::default(),
        )
        .unwrap();
        assert_eq!(store.find_note(&a).unwrap().directory, "cs130");
        assert_eq!(store.find_note(&b).unwrap().directory, "cs130");
        store.undo().unwrap();
        assert_eq!(store.find_note(&a).unwrap().directory, "");
        assert_eq!(store.find_note(&b).unwrap().directory, "");
    }

    #[test]
    fn rename_takes_the_new_title_and_means_the_selected_note() {
        let parsed = match parse("rename Graph traversals") {
            Parsed::Action(a) => a,
            other => panic!("{other:?}"),
        };
        assert_eq!(
            parsed,
            Action::Rename {
                note: String::new(),
                title: "Graph traversals".to_string()
            }
        );
        assert!(matches!(parse("rename"), Parsed::Usage(_)));
    }

    #[test]
    fn renaming_changes_only_the_title() {
        let (mut store, _d) = temp_store();
        let id = seed(&mut store, "Graphs", "BFS and DFS", "cs130");
        let numbering = numbering_for(&store, "cs130");
        let out = apply(
            Action::Rename {
                note: String::new(),
                title: "Graph traversals".to_string(),
            },
            &mut store,
            ctx_selected(&numbering, &id),
            &FakeAi::default(),
        )
        .unwrap();
        let note = store.find_note(&id).unwrap();
        assert_eq!(note.title, "Graph traversals");
        assert_eq!(note.body, "BFS and DFS");
        assert_eq!(note.directory, "cs130");
        assert!(out.dirty);
        // And it survives a reload, so the file on disk was rewritten too.
        let reloaded = Store::load_from(&store.notes_dir).unwrap();
        assert_eq!(reloaded.find_note(&id).unwrap().title, "Graph traversals");
    }

    /// The CLI has no selection, so an omitted note has to say so rather than
    /// guess.
    #[test]
    fn an_omitted_note_with_nothing_selected_says_so() {
        let (mut store, _d) = temp_store();
        seed(&mut store, "Graphs", "", "");
        let out = apply(
            Action::Edit {
                note: String::new(),
            },
            &mut store,
            ctx("", &[]),
            &FakeAi::default(),
        )
        .unwrap();
        assert_eq!(out.effect, Effect::None);
        assert!(out.text().contains("No note selected"), "{}", out.text());
    }

    // ── new, with a place and tags ──────────────────────────────────────────

    fn new_request(store: &mut Store, dir: &str, line: &str) -> EditRequest {
        let title = Some(line.to_string());
        match apply(
            Action::New { title },
            store,
            ctx(dir, &[]),
            &FakeAi::default(),
        )
        .unwrap()
        .effect
        {
            Effect::Edit(req) => req,
            other => panic!("expected an editor, got {other:?}"),
        }
    }

    fn target_dir(req: &EditRequest) -> &str {
        match &req.target {
            EditTarget::NewNote { dir, .. } => dir,
            other => panic!("{other:?}"),
        }
    }

    /// One line names where the note goes, its title and its tags.
    #[test]
    fn new_puts_the_note_in_a_named_directory_with_tags() {
        let (mut store, _d) = temp_store();
        store.create_dir("cs130");
        let req = new_request(&mut store, "", "cs130/Lecture 4 #exam #graphs");
        assert_eq!(target_dir(&req), "cs130");
        assert!(req.seed.contains("title: Lecture 4\n"), "{}", req.seed);
        assert!(req.seed.contains("tags: exam, graphs\n"), "{}", req.seed);
    }

    /// A slash in an ordinary title is not a directory.
    #[test]
    fn a_slash_in_a_title_stays_in_the_title() {
        let (mut store, _d) = temp_store();
        let req = new_request(&mut store, "", "TCP/IP basics");
        assert_eq!(target_dir(&req), "");
        assert!(req.seed.contains("title: TCP/IP basics\n"), "{}", req.seed);
    }

    /// A trailing slash asks for the directory even when it does not exist yet;
    /// it is made when the note is saved, not before, so cancelling leaves
    /// nothing behind.
    #[test]
    fn a_trailing_slash_makes_the_directory_on_save() {
        let (mut store, _d) = temp_store();
        let req = new_request(&mut store, "", "cs162/ Lecture 1");
        assert_eq!(target_dir(&req), "cs162");
        assert!(!store.dir_exists("cs162"), "made before the editor closed");

        apply_edit(
            &mut store,
            &req.target,
            &format!("{}notes", req.seed),
            &FakeAi::default(),
        )
        .unwrap();
        assert!(store.dir_exists("cs162"));
        assert_eq!(store.notes[0].directory, "cs162");
        assert_eq!(store.notes[0].title, "Lecture 1");
    }

    #[test]
    fn a_directory_is_relative_to_where_you_are() {
        let (mut store, _d) = temp_store();
        store.create_dir("cs130/lec");
        let req = new_request(&mut store, "cs130", "lec/Week 2");
        assert_eq!(target_dir(&req), "cs130/lec");
    }

    // ── resolution ──────────────────────────────────────────────────────────

    #[test]
    fn resolves_by_list_number_id_prefix_and_unique_title() {
        let (mut store, _d) = temp_store();
        let id = seed(&mut store, "Rust ownership", "body", "");
        let numbering = vec![id.clone()];

        assert_eq!(resolve("1", &store, &numbering), Resolved::One(id.clone()));
        assert_eq!(
            resolve(&id[..8], &store, &numbering),
            Resolved::One(id.clone())
        );
        assert_eq!(resolve("ownership", &store, &numbering), Resolved::One(id));
    }

    #[test]
    fn an_out_of_range_number_does_not_resolve() {
        let (mut store, _d) = temp_store();
        // A fixed id, so the assertions below cannot accidentally pass or fail
        // on a random UUID that happens to start with the digit under test.
        let id = "aaaaaaaa-0000-0000-0000-000000000000".to_string();
        store.notes.push(crate::notes::Note {
            id: id.clone(),
            title: "One".to_string(),
            body: "body".to_string(),
            tags: vec![],
            directory: String::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            pinned: false,
        });
        let numbering = vec![id];

        // Past the end of the numbering, and 0 which is never a valid index.
        assert_eq!(resolve("7", &store, &numbering), Resolved::None);
        assert_eq!(resolve("0", &store, &numbering), Resolved::None);
    }

    #[test]
    fn an_ambiguous_title_reports_every_candidate_with_its_number() {
        let (mut store, _d) = temp_store();
        let a = seed(&mut store, "Lecture 1 graphs", "b", "");
        let b = seed(&mut store, "Lecture 2 graphs", "b", "");
        let numbering = vec![a, b];

        let Resolved::Many(briefs) = resolve("graphs", &store, &numbering) else {
            panic!("expected an ambiguous match");
        };
        assert_eq!(briefs.len(), 2);
        assert!(briefs.iter().all(|b| b.index.is_some()));

        // And the rendered form names them without claiming a failure.
        let out = unresolved("graphs", resolve("graphs", &store, &numbering));
        assert!(out.text().contains("Multiple notes match"));
        assert!(out.text().contains("Lecture 1 graphs"));
        assert!(out.text().contains("Lecture 2 graphs"));
    }

    // ── read-only handlers ──────────────────────────────────────────────────

    #[test]
    fn list_numbers_notes_and_shows_subdirectories() {
        let (mut store, _d) = temp_store();
        store.create_dir("cs130");
        let id = seed(&mut store, "Root note", "b", "");
        seed(&mut store, "Nested", "b", "cs130");

        let out = apply(
            Action::List {
                tag: None,
                limit: 20,
            },
            &mut store,
            ctx("", &[]),
            &FakeAi::default(),
        )
        .unwrap();

        assert_eq!(out.selection, Some(vec![id]));
        assert!(out.text().contains("cs130/"), "got: {}", out.text());
        assert!(out.text().contains("Root note"));
        // A note in a subdirectory is not listed at the root.
        assert!(!out.text().contains("Nested"));
    }

    #[test]
    fn list_on_an_empty_store_clears_the_numbering() {
        let (mut store, _d) = temp_store();
        let out = apply(
            Action::List {
                tag: None,
                limit: 20,
            },
            &mut store,
            ctx("", &["stale".to_string()]),
            &FakeAi::default(),
        )
        .unwrap();
        assert_eq!(out.selection, Some(Vec::new()));
        assert!(out.text().contains("No notes yet"));
    }

    /// The CLI's search is the same search as `/`: bodies included, no flag.
    #[test]
    fn search_looks_in_bodies() {
        let (mut store, _d) = temp_store();
        seed(&mut store, "Graphs", "BFS explores level by level", "");
        let out = apply(
            Action::Search {
                query: "explores".to_string(),
            },
            &mut store,
            ctx("", &[]),
            &FakeAi::default(),
        )
        .unwrap();
        assert!(out.text().contains("Graphs"), "{}", out.text());
    }

    #[test]
    fn search_shows_the_line_it_matched_inside_a_note() {
        let (mut store, _d) = temp_store();
        seed(
            &mut store,
            "Graphs",
            "intro\nBFS explores level by level",
            "",
        );
        let out = apply(
            Action::Search {
                query: "explores".to_string(),
            },
            &mut store,
            ctx("", &[]),
            &FakeAi::default(),
        )
        .unwrap();
        assert!(
            out.text().contains("BFS explores level by level"),
            "{}",
            out.text()
        );
    }

    #[test]
    fn search_renumbers_across_directories() {
        let (mut store, _d) = temp_store();
        seed(&mut store, "Root graphs", "b", "");
        seed(&mut store, "Nested graphs", "b", "cs130");

        let out = apply(
            Action::Search {
                query: "graphs".to_string(),
            },
            &mut store,
            ctx("", &[]),
            &FakeAi::default(),
        )
        .unwrap();

        assert_eq!(out.selection.as_ref().map(|s| s.len()), Some(2));
        assert!(
            out.text().contains("cs130/"),
            "search shows the directory: {}",
            out.text()
        );
    }

    #[test]
    fn view_asks_the_shell_to_show_the_resolved_note() {
        let (mut store, _d) = temp_store();
        let id = seed(&mut store, "Note", "b", "");
        let out = apply(
            Action::View {
                note: "1".to_string(),
            },
            &mut store,
            ctx("", std::slice::from_ref(&id)),
            &FakeAi::default(),
        )
        .unwrap();
        assert_eq!(out.effect, Effect::ShowNote { id });
    }

    #[test]
    fn a_handler_given_an_unresolvable_note_reports_it_and_does_nothing() {
        let (mut store, _d) = temp_store();
        let out = apply(
            Action::View {
                note: "nope".to_string(),
            },
            &mut store,
            ctx("", &[]),
            &FakeAi::default(),
        )
        .unwrap();
        assert_eq!(out.effect, Effect::None);
        assert!(out.text().contains("No note found: nope"));
    }

    // ── mutating handlers ───────────────────────────────────────────────────

    #[test]
    fn check_toggles_a_checkbox_and_persists() {
        let (mut store, _d) = temp_store();
        let id = seed(&mut store, "Tasks", "- [ ] first\n- [ ] second", "");

        let out = apply(
            Action::Check {
                note: "1".to_string(),
                index: 1,
            },
            &mut store,
            ctx("", std::slice::from_ref(&id)),
            &FakeAi::default(),
        )
        .unwrap();
        assert!(out.dirty);
        assert!(store.find_note(&id).unwrap().body.contains("- [x] first"));

        // Reloading from disk proves it was saved, not just mutated in memory.
        let reloaded = Store::load_from(&store.notes_dir).unwrap();
        assert!(reloaded
            .find_note(&id)
            .unwrap()
            .body
            .contains("- [x] first"));
    }

    #[test]
    fn check_reports_an_out_of_range_checkbox_without_saving() {
        let (mut store, _d) = temp_store();
        let id = seed(&mut store, "Tasks", "- [ ] only one", "");
        let out = apply(
            Action::Check {
                note: "1".to_string(),
                index: 9,
            },
            &mut store,
            ctx("", &[id]),
            &FakeAi::default(),
        )
        .unwrap();
        assert!(!out.dirty);
        assert!(out.text().contains("No checkbox #9"));
    }

    #[test]
    fn mkdir_creates_relative_to_the_current_directory() {
        let (mut store, _d) = temp_store();
        store.create_dir("cs130");

        apply(
            Action::Mkdir {
                name: "lec".to_string(),
            },
            &mut store,
            ctx("cs130", &[]),
            &FakeAi::default(),
        )
        .unwrap();
        assert!(store.dir_exists("cs130/lec"));
    }

    #[test]
    fn mkdir_on_an_existing_directory_is_not_an_error() {
        let (mut store, _d) = temp_store();
        store.create_dir("cs130");
        let out = apply(
            Action::Mkdir {
                name: "cs130".to_string(),
            },
            &mut store,
            ctx("", &[]),
            &FakeAi::default(),
        )
        .unwrap();
        assert!(out.text().contains("already exists"));
        assert!(!out.dirty);
    }

    #[test]
    fn rmdir_refuses_a_non_empty_directory() {
        let (mut store, _d) = temp_store();
        store.create_dir("cs130");
        seed(&mut store, "Nested", "b", "cs130");

        let out = apply(
            Action::Rmdir {
                name: "cs130".to_string(),
                recursive: false,
            },
            &mut store,
            ctx("", &[]),
            &FakeAi::default(),
        )
        .unwrap();
        assert!(out.text().contains("not empty"), "got: {}", out.text());
        assert!(store.dir_exists("cs130"));
    }

    /// Deleting a directory with notes in it must ask first, and the prompt has
    /// to say how much is at stake — this is the only action in leo that can
    /// destroy more than one note.
    #[test]
    fn a_recursive_rmdir_asks_before_deleting_and_names_the_damage() {
        let (mut store, _d) = temp_store();
        store.create_dir("cs130");
        store.create_dir("cs130/lec");
        seed(&mut store, "One", "b", "cs130");
        seed(&mut store, "Two", "b", "cs130/lec");

        let out = apply(
            Action::Rmdir {
                name: "cs130".to_string(),
                recursive: true,
            },
            &mut store,
            ctx("", &[]),
            &FakeAi::default(),
        )
        .unwrap();

        match out.effect {
            Effect::Confirm { prompt, on_yes } => {
                assert!(prompt.contains("cs130/"), "prompt: {prompt}");
                assert!(prompt.contains("2 notes"), "prompt: {prompt}");
                assert!(prompt.contains("1 subdirectory"), "prompt: {prompt}");
                assert_eq!(
                    on_yes,
                    ConfirmedAction::DeleteDir {
                        path: "cs130".to_string()
                    }
                );
            }
            other => panic!("expected a confirmation, got {other:?}"),
        }
        // Nothing is gone yet.
        assert!(store.dir_exists("cs130"));
        assert_eq!(store.notes.len(), 2);
    }

    #[test]
    fn confirming_removes_the_directory_and_its_contents() {
        let (mut store, _d) = temp_store();
        store.create_dir("cs130");
        store.create_dir("cs130/lec");
        seed(&mut store, "One", "b", "cs130");
        seed(&mut store, "Two", "b", "cs130/lec");
        seed(&mut store, "Elsewhere", "b", "");

        let out = apply_confirmed(
            &mut store,
            &ConfirmedAction::DeleteDir {
                path: "cs130".to_string(),
            },
        )
        .unwrap();

        assert!(out.dirty);
        assert!(out.text().contains("2 notes"), "got: {}", out.text());
        assert!(!store.dir_exists("cs130"));
        assert!(!store.dir_exists("cs130/lec"));
        assert_eq!(store.notes.len(), 1, "the unrelated note survives");

        let reloaded = Store::load_from(&store.notes_dir).unwrap();
        assert_eq!(reloaded.notes.len(), 1);
    }

    /// An empty directory is not worth a prompt.
    #[test]
    fn a_recursive_rmdir_on_an_empty_directory_just_does_it() {
        let (mut store, _d) = temp_store();
        store.create_dir("empty");

        let out = apply(
            Action::Rmdir {
                name: "empty".to_string(),
                recursive: true,
            },
            &mut store,
            ctx("", &[]),
            &FakeAi::default(),
        )
        .unwrap();

        assert_eq!(out.effect, Effect::None);
        assert!(out.dirty);
        assert!(!store.dir_exists("empty"));
    }

    /// The plain form still refuses, but now says how to proceed.
    #[test]
    fn a_plain_rmdir_on_a_full_directory_points_at_the_recursive_form() {
        let (mut store, _d) = temp_store();
        store.create_dir("cs130");
        seed(&mut store, "One", "b", "cs130");

        let out = apply(
            Action::Rmdir {
                name: "cs130".to_string(),
                recursive: false,
            },
            &mut store,
            ctx("", &[]),
            &FakeAi::default(),
        )
        .unwrap();

        assert!(out.text().contains("rmdir -r cs130"), "got: {}", out.text());
        assert!(store.dir_exists("cs130"));
        assert_eq!(store.notes.len(), 1);
    }

    #[test]
    fn mv_moves_several_notes_and_says_how_many() {
        let (mut store, _d) = temp_store();
        store.create_dir("cs130");
        let a = seed(&mut store, "One", "b", "");
        let b = seed(&mut store, "Two", "b", "");

        let out = apply(
            Action::Mv {
                notes: vec!["1".to_string(), "2".to_string()],
                dir: "cs130".to_string(),
            },
            &mut store,
            ctx("", &[a.clone(), b.clone()]),
            &FakeAi::default(),
        )
        .unwrap();

        assert!(out.dirty);
        assert_eq!(store.find_note(&a).unwrap().directory, "cs130");
        assert_eq!(store.find_note(&b).unwrap().directory, "cs130");
        assert_eq!(out.text(), "Moved 2 notes to cs130");
    }

    #[test]
    fn mv_to_a_missing_directory_moves_nothing() {
        let (mut store, _d) = temp_store();
        let a = seed(&mut store, "One", "b", "");
        let out = apply(
            Action::Mv {
                notes: vec!["1".to_string()],
                dir: "ghost".to_string(),
            },
            &mut store,
            ctx("", std::slice::from_ref(&a)),
            &FakeAi::default(),
        )
        .unwrap();
        assert!(out.text().contains("No such directory"));
        assert_eq!(store.find_note(&a).unwrap().directory, "");
    }

    #[test]
    fn mv_keeps_going_when_one_reference_is_bad() {
        let (mut store, _d) = temp_store();
        store.create_dir("cs130");
        let a = seed(&mut store, "One", "b", "");

        let out = apply(
            Action::Mv {
                notes: vec!["1".to_string(), "ghost-note".to_string()],
                dir: "cs130".to_string(),
            },
            &mut store,
            ctx("", std::slice::from_ref(&a)),
            &FakeAi::default(),
        )
        .unwrap();

        assert_eq!(store.find_note(&a).unwrap().directory, "cs130");
        assert!(out.text().contains("No note found: ghost-note"));
        assert!(out.dirty, "the one note that did move must still be saved");
    }

    // ── cd ──────────────────────────────────────────────────────────────────

    #[test]
    fn cd_navigates_up_down_and_to_root() {
        let (mut store, _d) = temp_store();
        store.create_dir("cs130");
        store.create_dir("cs130/lec");
        store.create_dir("cs162");

        let cases: &[(&str, &str, &str)] = &[
            // (from, argument, expected)
            ("", "cs130", "cs130"),
            ("cs130", "lec", "cs130/lec"),
            ("cs130/lec", "..", "cs130"),
            ("cs130", "..", ""),
            ("", "..", ""),
            ("cs130/lec", "/", ""),
            ("cs130/lec", "", ""),
            ("cs130/lec", "~", ""),
            ("cs130", "/cs162", "cs162"),
            ("cs130/lec", "../../cs162", "cs162"),
        ];
        for (from, arg, expected) in cases {
            assert_eq!(
                resolve_cd(arg, &store, from).as_deref(),
                Ok(*expected),
                "cd {arg:?} from {from:?}"
            );
        }
    }

    #[test]
    fn cd_into_a_missing_directory_is_an_error_and_does_not_move() {
        let (mut store, _d) = temp_store();
        let out = apply(
            Action::Cd {
                path: "ghost".to_string(),
            },
            &mut store,
            ctx("", &[]),
            &FakeAi::default(),
        )
        .unwrap();
        assert_eq!(out.new_dir, None);
        assert!(out.text().contains("No such directory"));
    }

    // ── editor round trip ───────────────────────────────────────────────────

    #[test]
    fn new_requests_an_editor_seeded_with_the_title() {
        let (mut store, _d) = temp_store();
        let out = apply(
            Action::New {
                title: Some("My Note".to_string()),
            },
            &mut store,
            ctx("cs130", &[]),
            &FakeAi::default(),
        )
        .unwrap();

        let Effect::Edit(req) = out.effect else {
            panic!("expected an edit request, got {:?}", out.effect);
        };
        assert!(req.seed.contains("title: My Note"));
        assert_eq!(
            req.target,
            EditTarget::NewNote {
                fallback_title: "My Note".to_string(),
                dir: "cs130".to_string()
            }
        );
    }

    #[test]
    fn applying_an_edited_new_note_creates_it_in_the_right_directory() {
        let (mut store, _d) = temp_store();
        let target = EditTarget::NewNote {
            fallback_title: "Fallback".to_string(),
            dir: "cs130".to_string(),
        };
        let raw = "---\ntitle: Real Title\ntags: rust, cli\n---\nThe body.";

        let out = apply_edit(&mut store, &target, raw, &FakeAi::default()).unwrap();
        assert!(out.dirty);
        assert_eq!(store.notes.len(), 1);
        let note = &store.notes[0];
        assert_eq!(note.title, "Real Title");
        assert_eq!(note.tags, vec!["rust", "cli"]);
        assert_eq!(note.body.trim(), "The body.");
        assert_eq!(note.directory, "cs130");
    }

    #[test]
    fn an_empty_body_cancels_note_creation() {
        let (mut store, _d) = temp_store();
        let target = EditTarget::NewNote {
            fallback_title: "T".to_string(),
            dir: String::new(),
        };
        let out = apply_edit(
            &mut store,
            &target,
            "---\ntitle: T\ntags: \n---\n   \n",
            &FakeAi::default(),
        )
        .unwrap();
        assert!(out.text().contains("cancelled"));
        assert!(store.notes.is_empty());
    }

    #[test]
    fn a_deleted_title_line_falls_back_to_the_typed_title() {
        let (mut store, _d) = temp_store();
        let target = EditTarget::NewNote {
            fallback_title: "Typed Title".to_string(),
            dir: String::new(),
        };
        // No frontmatter at all: the whole buffer is the body.
        let out = apply_edit(&mut store, &target, "just a body", &FakeAi::default()).unwrap();
        assert!(out.dirty);
        assert_eq!(store.notes[0].title, "Typed Title");
        assert_eq!(store.notes[0].body, "just a body");
    }

    #[test]
    fn an_unchanged_edit_reports_no_changes_and_does_not_touch_the_note() {
        let (mut store, _d) = temp_store();
        let id = seed(&mut store, "Title", "body", "");
        let before = store.find_note(&id).unwrap().updated_at;

        let target = EditTarget::Existing {
            id: id.clone(),
            old_title: "Title".to_string(),
            old_tags: vec![],
            old_body: "body".to_string(),
        };
        let out = apply_edit(
            &mut store,
            &target,
            "---\ntitle: Title\ntags: \n---\nbody",
            &FakeAi::default(),
        )
        .unwrap();

        assert!(!out.dirty);
        assert!(out.text().contains("No changes"));
        assert_eq!(store.find_note(&id).unwrap().updated_at, before);
    }

    #[test]
    fn editing_expands_leo_prompts_before_saving() {
        let (mut store, _d) = temp_store();
        let id = seed(&mut store, "Title", "old body", "");
        let target = EditTarget::Existing {
            id: id.clone(),
            old_title: "Title".to_string(),
            old_tags: vec![],
            old_body: "old body".to_string(),
        };
        let ai = FakeAi {
            expand_to: Some("- the expanded answer".to_string()),
            ..FakeAi::default()
        };

        let out = apply_edit(
            &mut store,
            &target,
            "---\ntitle: Title\ntags: \n---\n@leo what is BFS?",
            &ai,
        )
        .unwrap();

        assert!(out.dirty);
        assert_eq!(store.find_note(&id).unwrap().body, "- the expanded answer");
        assert!(out.text().contains("Expanded 1 prompt"));
    }

    /// A failed expansion must never cost the user their edit.
    #[test]
    fn a_failed_expansion_still_saves_the_edit_and_warns() {
        let (mut store, _d) = temp_store();
        let id = seed(&mut store, "Title", "old body", "");
        let target = EditTarget::Existing {
            id: id.clone(),
            old_title: "Title".to_string(),
            old_tags: vec![],
            old_body: "old body".to_string(),
        };
        let ai = FakeAi {
            fail: true,
            ..FakeAi::default()
        };

        let out = apply_edit(
            &mut store,
            &target,
            "---\ntitle: Title\ntags: \n---\nnew text\n@leo what is BFS?",
            &ai,
        )
        .unwrap();

        assert!(
            out.dirty,
            "the edit must be saved even though expansion failed"
        );
        assert!(out.text().contains("Expansion failed"));
        let body = &store.find_note(&id).unwrap().body;
        assert!(body.contains("new text"));
        assert!(
            body.contains("@leo what is BFS?"),
            "the prompt line is kept"
        );
    }

    // ── delete confirmation ─────────────────────────────────────────────────

    #[test]
    fn delete_asks_for_confirmation_before_removing_anything() {
        let (mut store, _d) = temp_store();
        let id = seed(&mut store, "Doomed", "b", "");

        let out = apply(
            Action::Delete {
                note: "1".to_string(),
            },
            &mut store,
            ctx("", std::slice::from_ref(&id)),
            &FakeAi::default(),
        )
        .unwrap();

        assert_eq!(
            out.effect,
            Effect::Confirm {
                prompt: "Delete Doomed?".to_string(),
                on_yes: ConfirmedAction::DeleteNote {
                    id: id.clone(),
                    title: "Doomed".to_string()
                },
            }
        );
        // Nothing is gone yet.
        assert!(store.find_note(&id).is_some());
    }

    #[test]
    fn a_confirmed_delete_removes_the_note_from_disk() {
        let (mut store, _d) = temp_store();
        let id = seed(&mut store, "Doomed", "b", "");

        let out = apply_confirmed(
            &mut store,
            &ConfirmedAction::DeleteNote {
                id: id.clone(),
                title: "Doomed".to_string(),
            },
        )
        .unwrap();

        assert!(out.dirty);
        assert!(store.find_note(&id).is_none());
        let reloaded = Store::load_from(&store.notes_dir).unwrap();
        assert!(reloaded.find_note(&id).is_none());
    }

    // ── listen round trip ───────────────────────────────────────────────────

    #[test]
    fn listen_validates_the_append_target_before_recording() {
        let (mut store, _d) = temp_store();
        let out = apply(
            Action::Listen {
                title: None,
                append_to: Some("ghost".to_string()),
                screen: false,
            },
            &mut store,
            ctx("", &[]),
            &FakeAi::default(),
        )
        .unwrap();
        assert_eq!(out.effect, Effect::None, "must not start recording");
        assert!(out.text().contains("No note found: ghost"));
    }

    #[test]
    fn listen_requests_a_recording_carrying_the_current_directory() {
        let (mut store, _d) = temp_store();
        let out = apply(
            Action::Listen {
                title: Some("Lecture 3".to_string()),
                append_to: None,
                screen: true,
            },
            &mut store,
            ctx("cs130", &[]),
            &FakeAi::default(),
        )
        .unwrap();
        assert_eq!(
            out.effect,
            Effect::Listen(ListenRequest {
                screen: true,
                title: Some("Lecture 3".to_string()),
                append_to: None,
                dir: "cs130".to_string(),
            })
        );
    }

    #[test]
    fn a_transcript_becomes_a_new_note_titled_by_the_model() {
        let (mut store, _d) = temp_store();
        let req = ListenRequest {
            screen: false,
            title: None,
            append_to: None,
            dir: "cs130".to_string(),
        };
        let out = apply_transcript(&mut store, &req, "some speech", &FakeAi::default()).unwrap();

        assert!(out.dirty);
        assert_eq!(store.notes.len(), 1);
        assert_eq!(store.notes[0].title, "AI Title");
        assert_eq!(store.notes[0].directory, "cs130");
        assert_eq!(store.notes[0].tags, vec!["listen"]);
    }

    #[test]
    fn a_user_supplied_title_wins_over_the_models() {
        let (mut store, _d) = temp_store();
        let req = ListenRequest {
            screen: false,
            title: Some("My Title".to_string()),
            append_to: None,
            dir: String::new(),
        };
        apply_transcript(&mut store, &req, "some speech", &FakeAi::default()).unwrap();
        assert_eq!(store.notes[0].title, "My Title");
    }

    #[test]
    fn an_empty_transcript_creates_nothing() {
        let (mut store, _d) = temp_store();
        let req = ListenRequest {
            screen: false,
            title: None,
            append_to: None,
            dir: String::new(),
        };
        let out = apply_transcript(&mut store, &req, "   ", &FakeAi::default()).unwrap();
        assert!(out.text().contains("No speech detected"));
        assert!(store.notes.is_empty());
        assert!(!out.dirty);
    }

    #[test]
    fn appending_a_transcript_keeps_the_existing_body() {
        let (mut store, _d) = temp_store();
        let id = seed(&mut store, "Lecture", "## Existing\n- old point", "");
        let req = ListenRequest {
            screen: false,
            title: None,
            append_to: Some(id.clone()),
            dir: String::new(),
        };

        apply_transcript(&mut store, &req, "more speech", &FakeAi::default()).unwrap();
        let body = &store.find_note(&id).unwrap().body;
        assert!(body.contains("- old point"), "existing content survives");
        assert!(body.contains("- appended"));
        assert_eq!(store.notes.len(), 1, "append must not create a second note");
    }

    // ── ask ─────────────────────────────────────────────────────────────────

    #[test]
    fn ask_on_a_note_without_prompts_does_nothing() {
        let (mut store, _d) = temp_store();
        let id = seed(&mut store, "Plain", "no prompts here", "");
        let out = apply(
            Action::Ask {
                note: "1".to_string(),
            },
            &mut store,
            ctx("", &[id]),
            &FakeAi::default(),
        )
        .unwrap();
        assert!(out.text().contains("No @leo prompts"));
        assert!(!out.dirty);
    }

    #[test]
    fn ask_replaces_the_prompt_line_with_the_expansion() {
        let (mut store, _d) = temp_store();
        let id = seed(&mut store, "Graphs", "@leo what is BFS?", "");
        let ai = FakeAi {
            expand_to: Some("- BFS explores level by level".to_string()),
            ..FakeAi::default()
        };

        let out = apply(
            Action::Ask {
                note: "1".to_string(),
            },
            &mut store,
            ctx("", std::slice::from_ref(&id)),
            &ai,
        )
        .unwrap();

        assert!(out.dirty);
        assert_eq!(
            store.find_note(&id).unwrap().body,
            "- BFS explores level by level"
        );
    }

    /// A retired verb must point at its replacement. Answering "unknown
    /// command" would read like a typo and send the user hunting.
    #[test]
    fn the_retired_env_verb_names_its_replacement() {
        match parse("env") {
            Parsed::Retired {
                verb,
                replacement,
                why,
            } => {
                assert_eq!(verb, "env");
                assert_eq!(replacement, "Ctrl-S");
                assert!(why.contains("keychain"), "{why}");
            }
            other => panic!("expected Retired, got {other:?}"),
        }
    }

    /// It must also be gone from the vocabulary, so it stops appearing in
    /// completion and help.
    #[test]
    fn env_is_no_longer_a_verb() {
        assert!(
            !VERBS
                .iter()
                .any(|v| v.name == "env" || v.aliases.contains(&"env")),
            "env is still in the verb table"
        );
    }

    // ── shell-delegated actions ─────────────────────────────────────────────

    #[test]
    fn sync_model_and_config_are_delegated_to_the_shell_untouched() {
        let (mut store, _d) = temp_store();
        let ai = FakeAi::default();
        let cases = [
            (
                Action::Sync(SyncAction::Push),
                Effect::Sync(SyncAction::Push),
            ),
            (Action::Help, Effect::ShowHelp),
            (Action::Quit, Effect::Quit),
        ];
        for (action, expected) in cases {
            let out = apply(action.clone(), &mut store, ctx("", &[]), &ai).unwrap();
            assert_eq!(out.effect, expected, "for {action:?}");
        }
    }

    // ── frontmatter and prompts ─────────────────────────────────────────────

    #[test]
    fn frontmatter_parses_title_tags_and_body() {
        let (title, tags, body) =
            parse_frontmatter("---\ntitle: T\ntags: a, b\n---\nbody line\nsecond");
        assert_eq!(title, "T");
        assert_eq!(tags, vec!["a", "b"]);
        assert_eq!(body, "body line\nsecond");
    }

    #[test]
    fn absent_or_malformed_frontmatter_keeps_the_whole_buffer_as_body() {
        let raw = "no frontmatter here";
        assert_eq!(
            parse_frontmatter(raw),
            (String::new(), vec![], raw.to_string())
        );

        let unterminated = "---\ntitle: T\nbody with no closing marker";
        assert_eq!(
            parse_frontmatter(unterminated),
            (String::new(), vec![], unterminated.to_string())
        );
    }

    #[test]
    fn empty_tags_do_not_become_an_empty_tag() {
        let (_, tags, _) = parse_frontmatter("---\ntitle: T\ntags: \n---\nbody");
        assert!(tags.is_empty());
        let (_, tags, _) = parse_frontmatter("---\ntitle: T\ntags: a, , b\n---\nbody");
        assert_eq!(tags, vec!["a", "b"]);
    }

    #[test]
    fn leo_prompt_detection_is_case_insensitive_and_needs_a_question() {
        assert_eq!(is_leo_prompt("@leo what is BFS?"), Some("what is BFS?"));
        assert_eq!(is_leo_prompt("  @LEO what is BFS?  "), Some("what is BFS?"));
        assert_eq!(is_leo_prompt("@leo"), None);
        assert_eq!(is_leo_prompt("@leo   "), None);
        assert_eq!(is_leo_prompt("email me @leo later"), None);
        assert_eq!(is_leo_prompt(""), None);
        // Must not panic on a short or multi-byte line.
        assert_eq!(is_leo_prompt("@le"), None);
        assert_eq!(is_leo_prompt("héllo"), None);
    }

    #[test]
    fn numbering_reflects_the_current_directory() {
        let (mut store, _d) = temp_store();
        store.create_dir("cs130");
        let root = seed(&mut store, "Root", "b", "");
        let nested = seed(&mut store, "Nested", "b", "cs130");

        assert_eq!(numbering_for(&store, ""), vec![root]);
        assert_eq!(numbering_for(&store, "cs130"), vec![nested]);
    }

    /// Every note gets a number, not only the twenty newest: the panes list the
    /// whole directory, and `leo view 30` must work after `leo list --limit 50`.
    #[test]
    fn numbering_covers_every_note_in_the_directory() {
        let (mut store, _d) = temp_store();
        for i in 0..25 {
            store
                .create_note(format!("Note {i}"), "", vec![], "")
                .unwrap();
        }
        store.save().unwrap();
        assert_eq!(numbering_for(&store, "").len(), 25);
    }

    // ── trash ───────────────────────────────────────────────────────────────

    fn trash(store: &mut Store, what: TrashAction) -> Outcome {
        apply(Action::Trash(what), store, ctx("", &[]), &FakeAi::default()).unwrap()
    }

    fn delete(store: &mut Store, id: &str) -> Outcome {
        apply_confirmed(
            store,
            &ConfirmedAction::DeleteNote {
                id: id.to_string(),
                title: String::new(),
            },
        )
        .unwrap()
    }

    #[test]
    fn deleting_says_the_note_went_to_the_trash() {
        let (mut store, _d) = temp_store();
        let id = seed(&mut store, "Graphs", "", "");
        let out = delete(&mut store, &id);
        assert!(out.text().contains("trash"), "{}", out.text());
    }

    #[test]
    fn an_empty_trash_says_so() {
        let (mut store, _d) = temp_store();
        let out = trash(&mut store, TrashAction::List);
        assert!(out.text().contains("empty"), "{}", out.text());
    }

    /// The list numbers each note and says where it came from, and how to
    /// bring one back.
    #[test]
    fn the_trash_lists_what_was_deleted() {
        let (mut store, _d) = temp_store();
        let id = seed(&mut store, "Lecture 4", "", "cs130");
        delete(&mut store, &id);
        let out = trash(&mut store, TrashAction::List);
        let text = out.text();
        for expected in ["1", "Lecture 4", "/cs130", "trash restore", "30 days"] {
            assert!(text.contains(expected), "no {expected:?}:\n{text}");
        }
    }

    #[test]
    fn restoring_by_number_brings_the_note_back_and_selects_it() {
        let (mut store, _d) = temp_store();
        let id = seed(&mut store, "Lecture 4", "BFS", "cs130");
        delete(&mut store, &id);
        let out = trash(
            &mut store,
            TrashAction::Restore {
                which: "1".to_string(),
            },
        );
        assert!(out.dirty);
        assert_eq!(out.select.as_deref(), Some(id.as_str()));
        assert!(out.text().contains("Lecture 4"), "{}", out.text());
        let reloaded = Store::load_from(&store.notes_dir).unwrap();
        assert_eq!(reloaded.find_note(&id).unwrap().directory, "cs130");
        assert!(reloaded.trashed().is_empty());
    }

    #[test]
    fn restoring_by_title_works_and_a_miss_says_so() {
        let (mut store, _d) = temp_store();
        let id = seed(&mut store, "Lecture 4", "", "");
        delete(&mut store, &id);
        let miss = trash(
            &mut store,
            TrashAction::Restore {
                which: "9".to_string(),
            },
        );
        assert!(!miss.dirty);
        assert!(miss.text().contains("No note"), "{}", miss.text());
        trash(
            &mut store,
            TrashAction::Restore {
                which: "lecture".to_string(),
            },
        );
        assert!(store.find_note(&id).is_some());
    }

    /// Emptying destroys notes for good, so it asks first.
    #[test]
    fn emptying_the_trash_asks_first() {
        let (mut store, _d) = temp_store();
        let id = seed(&mut store, "Gone", "", "");
        delete(&mut store, &id);
        let out = trash(&mut store, TrashAction::Empty);
        match out.effect {
            Effect::Confirm {
                on_yes: ConfirmedAction::EmptyTrash,
                ..
            } => {}
            other => panic!("expected a confirmation, got {other:?}"),
        }
        assert_eq!(store.trashed().len(), 1, "emptied before asking");
        let done = apply_confirmed(&mut store, &ConfirmedAction::EmptyTrash).unwrap();
        assert!(done.text().contains("1"), "{}", done.text());
        assert!(store.trashed().is_empty());
    }

    #[test]
    fn times_read_like_a_person_would_say_them() {
        let now = Utc::now();
        assert_eq!(ago(now, now), "just now");
        assert_eq!(
            ago(now - chrono::Duration::minutes(5), now),
            "5 minutes ago"
        );
        assert_eq!(ago(now - chrono::Duration::hours(1), now), "1 hour ago");
        assert_eq!(ago(now - chrono::Duration::days(3), now), "3 days ago");
    }

    // ── pin ─────────────────────────────────────────────────────────────────

    /// `pin` toggles: the first press pins, the second unpins.
    #[test]
    fn pin_toggles_the_selected_note() {
        let (mut store, _d) = temp_store();
        let id = seed(&mut store, "Syllabus", "", "");
        let numbering = numbering_for(&store, "");
        let pin = |store: &mut Store| {
            apply(
                Action::Pin {
                    note: String::new(),
                },
                store,
                ctx_selected(&numbering, &id),
                &FakeAi::default(),
            )
            .unwrap()
        };
        let first = pin(&mut store);
        assert!(first.dirty);
        assert!(first.text().contains("Pinned"), "{}", first.text());
        assert!(
            Store::load_from(&store.notes_dir)
                .unwrap()
                .find_note(&id)
                .unwrap()
                .pinned
        );
        let second = pin(&mut store);
        assert!(second.text().contains("Unpinned"), "{}", second.text());
        assert!(!store.find_note(&id).unwrap().pinned);
    }
}
