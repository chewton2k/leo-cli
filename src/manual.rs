//! The built-in manual note.
//!
//! A first run creates one note containing the whole command reference, so the
//! app explains itself in the pane the user is already looking at rather than
//! in a README they have to go find.
//!
//! Installation is recorded in a marker file, not inferred from the note's
//! presence: deleting the manual is a legitimate choice and must not be undone
//! on the next launch. The marker carries a version so a future release can
//! offer an updated manual without resurrecting a deleted one.

use anyhow::Result;

use crate::store::Store;

/// Bump when the manual's content changes enough to be worth re-offering.
const MANUAL_VERSION: u32 = 1;
const MARKER: &str = ".manual-installed";
pub const MANUAL_TITLE: &str = "leo manual";

/// Where the marker lives: the data directory, one level above `notes/`.
///
/// Deliberately not inside the notes directory. That directory is what `sync`
/// pushes to the user's git remote, so a marker there gets committed and
/// travels between machines — which also means a second machine would think the
/// manual was already installed and never create it.
fn marker_path(notes_dir: &std::path::Path) -> std::path::PathBuf {
    match notes_dir.parent() {
        Some(parent) => parent.join(MARKER),
        // No parent is not a real layout, but falling back keeps this
        // infallible rather than skipping the manual entirely.
        None => notes_dir.join(MARKER),
    }
}

/// Create the manual note if this store has never had one.
///
/// Returns the new note's ID when one was created. Failure is never fatal: a
/// read-only data directory should not stop the app from starting, so the
/// caller ignores the error.
pub fn install_if_absent(store: &mut Store) -> Result<Option<String>> {
    let marker = marker_path(&store.notes_dir);

    // Earlier versions wrote the marker inside the notes directory, where it
    // ended up in the user's git history. Move it out, so the next sync stops
    // carrying it, and treat it as already installed either way.
    let legacy = store.notes_dir.join(MARKER);
    if legacy.exists() {
        let version = std::fs::read_to_string(&legacy).unwrap_or_default();
        let _ = std::fs::write(&marker, version.trim());
        let _ = std::fs::remove_file(&legacy);
    }

    if let Ok(text) = std::fs::read_to_string(&marker) {
        if text.trim().parse::<u32>().unwrap_or(0) >= MANUAL_VERSION {
            return Ok(None);
        }
    }

    let note = store.create_note(MANUAL_TITLE, manual_body(), vec!["manual".to_string()], "")?;
    let id = note.id.clone();
    store.save()?;
    if let Some(parent) = marker.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&marker, MANUAL_VERSION.to_string())?;
    Ok(Some(id))
}

/// The manual itself. Organized by task, because the reader is looking for "how
/// do I move a note", not for an alphabetical list of verbs.
pub fn manual_body() -> String {
    format!(
        r#"Welcome. This note is the manual — it lives in your notes, so you can
search it, scroll it, and delete it when you no longer need it.

Everything below works in two places: on the `:` line in this interface, and as
a `leo <command>` subcommand in your shell.

## Getting around

The screen has three panes: directories, notes, and the selected note.

| Key | What it does |
|-----|--------------|
| `j` / `k` | Move down / up |
| `g` / `G` | Jump to first / last |
| `h` / `l` | Move between panes |
| `Enter` | Open a directory, or focus the note body |
| `Ctrl-D` / `Ctrl-U` | Scroll this note |
| `Ctrl-P` | Fuzzy-find any note, in any directory |
| `Ctrl-S` | Providers and settings |
| `Ctrl-R` | Reload from disk |
| `Esc` | Close an overlay, or clear pinned output |
| `?` | Help |
| `q` | Quit |

Notes are numbered in the middle pane. Those numbers are what commands take, so
`:view 2` opens the second note in the list.

## The `:` line

Press `:` to type a command. Press `Tab` to complete: it knows verbs, note
titles, directory names, tags, provider names, and export formats, and it
matches loosely — `:view grtrv` finds "Graph traversals".

When you complete a note by title, leo substitutes its number for you.

Press `/` as a shortcut for `:search `. `Up` and `Down` walk back through
commands you have already run, and `Ctrl-W` deletes a word, `Ctrl-U` the line.

## Writing notes

| Command | What it does |
|---------|--------------|
| `:new [title]` | Create a note. Opens your `$EDITOR` |
| `:edit <note>` | Edit a note in `$EDITOR` |
| `:view <note>` | Show a note |
| `:delete <note>` | Delete a note. Asks first |
| `:list [#tag] [N]` | List notes, optionally by tag or capped at N |
| `:search <query>` | Search titles |
| `:search -f <query>` | Search titles and bodies |
| `:tags` | Every tag, with counts |

Shortcuts: `n`, `e`, `v`, `rm`, `ls`, `find`. `D` deletes the selected note
without typing anything.

`<note>` accepts a list number (`2`), an ID prefix (`3f2a`), or a distinctive
part of the title (`ownership`).

Notes are plain Markdown files with a small YAML header. Nothing is locked in —
you can edit them with any editor, and `sync` pushes them to GitHub as-is.

## Checklists

Write checkboxes as `- [ ] thing`. Then:

- `x` toggles the first unchecked box in the selected note
- `:check <note> <N>` toggles box number N

- [ ] Try toggling this box with `x`
- [x] This one is already done

## Directories

| Command | What it does |
|---------|--------------|
| `:mkdir <name>` | Create a directory |
| `:cd <dir>` | Enter it. `..` goes up, `/` goes to the root |
| `:pwd` | Where am I |
| `:mv <note>... <dir>` | Move one or more notes |
| `:rmdir <name>` | Remove an empty directory |

Selecting a directory in the left pane and pressing `Enter` is the same as `cd`.

`:list` shows only the current directory. Search and `Ctrl-P` always look
everywhere.

## Reminders

```
:remind me to buy milk
```

Reminders collect as checkboxes in one note tagged `#reminder`. Toggle them like
any other checkbox.

## Recording a lecture

```
:listen                    start recording
:listen Lecture 4          ...with a title you choose
:listen add 2              ...appending to note 2 instead of creating one
:listen --screen           capture system audio instead of the microphone
```

While recording, the right pane fills with short bullets summarizing what has
been said so far. Press `t` to switch to the raw transcript, and `Enter` to
stop.

Stopping is not cancelling. The finished recording is transcribed in one pass and
saved as a note, so the result does not depend on what the live view managed to
catch.

Needs SoX (`brew install sox`) and one working transcription provider.

## Asking questions inside a note

Write a line starting with `@leo` anywhere in a note:

```
@leo what is the difference between Box and Rc?
```

Then run `:ask <note>`. Each `@leo` line is replaced by the answer, in place.
Saving a note you edited with `e` does this automatically.

## Exporting

```
:export <note> md
```

Formats: `txt`, `md`, `html`, `docx`, `pdf`, `rtf`, `odt`. The last four need
Pandoc (`brew install pandoc`). Files land on your Desktop.

## Backing up to GitHub

```
:sync init                 make the notes directory a git repo
:sync connect <url>        point it at a remote
:sync push                 send notes up
:sync pull                 bring notes down
:sync status               what changed
```

Once initialized, every save commits automatically.

## AI providers

Press `Ctrl-S` for the provider screen. It lists both chains — one for chat, one
for transcription — with each provider's model and whether it has a key, and
below them everything else that is configured but unused.

| Key | On the provider screen |
|-----|------------------------|
| `j` / `k` | Move between providers |
| `l` | Store an API key (typing is hidden) |
| `x` | Remove a stored key |
| `t` | Send one small request to check it works |
| `J` / `K` | Change priority within a chain |
| `a` | Add the selected provider to its chain |
| `d` | Drop it from the chain (it stays configured) |
| `e` | Open the config file in `$EDITOR` |
| `Esc` | Close |

A filled dot means leo would use that provider right now. A hollow one means it
is configured but not usable yet — usually a missing key, a binary that is not
installed, or a local server that is not running.

Providers are tried in order, and unavailable ones are skipped silently, so it
is fine to list more than you have. That means a laptop with Ollama installed
uses it for free and falls back to a cloud provider only when Ollama is not
running.

```
:model list                what is configured, and what has a key
:model login <provider>    store an API key in your OS keychain
:model test <provider>     one small request, to check it works
:config edit               open the config file in $EDITOR
```

Keys go in your operating system's keychain, never in a file in this directory.

Nothing here requires an API key if you run models locally:

```
brew install ollama whisper-cpp
ollama pull qwen3:8b
```

## Reading notes from your phone

```
leo serve --port 3131
```

Opens a small web page with a QR code, on your local network. It has no
password, so only do this on a network you trust.

## Where things live

- Notes: `{notes_dir}`
- Settings: `{config_path}`
- Keys: your OS keychain, under the service name `leo`

---

That is everything. Delete this note whenever you like — it will not come back.
Press `?` for the short version at any time."#,
        notes_dir = "<data dir>/leo/notes/",
        config_path = "<config dir>/leo/config.toml",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::load_from(&dir.path().join("notes")).unwrap();
        (store, dir)
    }

    #[test]
    fn a_first_run_creates_the_manual() {
        let (mut store, _d) = temp_store();
        let id = install_if_absent(&mut store).unwrap();

        assert!(id.is_some());
        assert_eq!(store.notes.len(), 1);
        let note = &store.notes[0];
        assert_eq!(note.title, MANUAL_TITLE);
        assert_eq!(note.tags, vec!["manual"]);
        assert!(note.directory.is_empty(), "the manual belongs at the root");

        // It is a real note on disk, editable and searchable like any other.
        let reloaded = Store::load_from(&store.notes_dir).unwrap();
        assert_eq!(reloaded.notes.len(), 1);
    }

    #[test]
    fn a_second_run_does_not_create_another() {
        let (mut store, _d) = temp_store();
        install_if_absent(&mut store).unwrap();
        assert_eq!(install_if_absent(&mut store).unwrap(), None);
        assert_eq!(store.notes.len(), 1);
    }

    /// Deleting the manual is a choice, and it must stick.
    #[test]
    fn a_deleted_manual_is_not_resurrected() {
        let (mut store, _d) = temp_store();
        let id = install_if_absent(&mut store).unwrap().unwrap();

        assert!(store.delete_note(&id));
        store.save().unwrap();

        assert_eq!(install_if_absent(&mut store).unwrap(), None);
        assert!(store.notes.is_empty());
    }

    /// A store that already has notes is not a first run, but it has never had a
    /// manual either — the marker, not the note count, is what decides.
    #[test]
    fn an_existing_store_still_gets_a_manual_once() {
        let (mut store, _d) = temp_store();
        store.create_note("Existing", "body", vec![], "").unwrap();
        store.save().unwrap();

        assert!(install_if_absent(&mut store).unwrap().is_some());
        assert_eq!(store.notes.len(), 2);
        assert_eq!(install_if_absent(&mut store).unwrap(), None);
    }

    #[test]
    fn a_stale_marker_version_offers_the_manual_again() {
        let (mut store, _d) = temp_store();
        install_if_absent(&mut store).unwrap();

        // Simulate a marker written by an older release.
        std::fs::write(marker_path(&store.notes_dir), "0").unwrap();
        assert!(install_if_absent(&mut store).unwrap().is_some());
    }

    #[test]
    fn a_corrupt_marker_is_treated_as_absent_rather_than_crashing() {
        let (mut store, _d) = temp_store();
        std::fs::create_dir_all(&store.notes_dir).unwrap();
        std::fs::write(marker_path(&store.notes_dir), "not a number").unwrap();
        assert!(install_if_absent(&mut store).unwrap().is_some());
    }

    /// The marker must not sit in the directory `sync` pushes: it is leo's
    /// bookkeeping, not a note, and committing it also makes a second machine
    /// think the manual is already installed.
    #[test]
    fn the_marker_lives_outside_the_synced_notes_directory() {
        let (mut store, _d) = temp_store();
        install_if_absent(&mut store).unwrap();

        assert!(
            !store.notes_dir.join(MARKER).exists(),
            "the marker is inside the synced notes directory"
        );
        assert!(marker_path(&store.notes_dir).exists());
        assert_eq!(marker_path(&store.notes_dir).parent(), store.notes_dir.parent());
    }

    /// An install that already has the old marker keeps working, and the stray
    /// file is cleaned out of the notes directory.
    #[test]
    fn a_legacy_marker_is_migrated_out_of_the_notes_directory() {
        let (mut store, _d) = temp_store();
        std::fs::create_dir_all(&store.notes_dir).unwrap();
        std::fs::write(store.notes_dir.join(MARKER), MANUAL_VERSION.to_string()).unwrap();

        // Already installed, so no second manual...
        assert_eq!(install_if_absent(&mut store).unwrap(), None);
        assert!(store.notes.is_empty());
        // ...and the stray file is gone, with the version preserved.
        assert!(!store.notes_dir.join(MARKER).exists());
        let moved = std::fs::read_to_string(marker_path(&store.notes_dir)).unwrap();
        assert_eq!(moved.trim(), MANUAL_VERSION.to_string());
    }

    /// The manual is the primary documentation, so every command surface has to
    /// appear in it. A new verb that never gets documented is a real bug.
    #[test]
    fn the_manual_documents_every_command_verb() {
        let body = manual_body();
        for (canonical, _aliases) in crate::action::VERBS {
            // `env` is legacy and deliberately undocumented; `clear` and `help`
            // are UI affordances covered by the key table.
            if matches!(*canonical, "env" | "clear" | "help" | "quit") {
                continue;
            }
            assert!(
                body.contains(canonical),
                "the manual never mentions `{canonical}`"
            );
        }
    }

    #[test]
    fn the_manual_documents_every_key_in_the_help_table() {
        let body = manual_body();
        for key in crate::tui::view::help::all_keys() {
            // Keys are written with backticks in the manual's tables, and
            // commands appear as `:verb`, so compare on the first token.
            let first = key.split(' ').next().unwrap_or(key);
            assert!(
                body.contains(first),
                "the manual never mentions the `{key}` key"
            );
        }
    }

    #[test]
    fn the_manual_has_scrollable_structure_rather_than_one_wall_of_text() {
        let body = manual_body();
        let headings = body.lines().filter(|l| l.starts_with("## ")).count();
        assert!(headings >= 8, "only {headings} sections");
        // Tables and fenced examples are what make it skimmable.
        assert!(body.contains("| Key | What it does |"));
        assert!(body.contains("```"));
    }

    #[test]
    fn the_manual_mentions_where_keys_are_stored_and_that_it_is_not_a_file() {
        let body = manual_body().to_lowercase();
        assert!(body.contains("keychain"));
        assert!(body.contains("never in a file"));
    }
}
