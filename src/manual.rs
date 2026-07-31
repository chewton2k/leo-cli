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
const MANUAL_VERSION: u32 = 2;
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

    // On a version bump, rewrite the note the user already has rather than
    // adding a second one beside it. A user who deleted theirs is not given
    // another: the marker below records the attempt either way.
    let existing = store
        .notes
        .iter()
        .find(|n| n.title == MANUAL_TITLE && n.tags.iter().any(|t| t == "manual"))
        .map(|n| n.id.clone());

    let id = match existing {
        Some(id) => {
            if let Some(note) = store.find_note_mut(&id) {
                note.body = manual_body();
                note.updated_at = chrono::Utc::now();
            }
            id
        }
        None => store
            .create_note(MANUAL_TITLE, manual_body(), vec!["manual".to_string()], "")?
            .id
            .clone(),
    };
    store.save()?;
    if let Some(parent) = marker.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&marker, MANUAL_VERSION.to_string())?;
    Ok(Some(id))
}

/// The manual itself: one screen, then a pointer to the rest.
///
/// Deliberately not a full reference. `?` already holds every key and every
/// command, grouped and scrollable, and duplicating it here made the largest
/// note most users own — permanently sitting at the top of their list. This
/// answers "what do I do now" and says where the rest is.
pub fn manual_body() -> String {
    format!(
        r#"This note is the manual. It is an ordinary note, so you can search it,
edit it, or delete it — it will not come back.

## The one-minute version

Three panes: directories, your notes, and the selected note. `j`/`k` move,
`h`/`l` switch panes, `Enter` opens. `e` edits the note in your editor, `x`
ticks the first open checkbox, `D` deletes.

Anything that takes an argument goes on the `:` line:

```
:new Rust ownership        create a note
:search borrow             find one
:mkdir cs130               make a directory
:mv 2 cs130                move note 2 into it
```

`Tab` completes titles, directories and tags. Notes are numbered as you see
them, so `:view 2` means the second one in the pane.

## Press `?` for everything else

That help screen is the full reference — every key, every command, grouped and
scrollable. It is always one keypress away, which is why this note is short.

## Talking instead of typing

`:listen` records and turns speech into structured notes. Press `t` while it
runs to see the raw transcript, `Enter` to stop. Writing `@leo <question>` in a
note and running `:ask` replaces that line with an answer.

Both need a model. `Ctrl-S` shows which ones are set up and lets you add one;
`leo doctor` in a shell reports anything missing along with the command that
installs it.

## Your notes are just files

Plain markdown, one file per note, in `{notes_dir}`. `:sync init` then
`:sync connect <url>` backs them up to git; after that every save commits.
`:export 1 pdf` writes a copy elsewhere.

Settings live in `{config_path}`. API keys never do — `Ctrl-S` or
`leo model login` keeps those in a separate file only your account can read.
"#,
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
    /// The manual is a quickstart, not a reference. The reference is `?`, and
    /// duplicating it here made the largest note most users own.
    #[test]
    fn the_manual_fits_on_a_screen_or_two() {
        let body = manual_body();
        let lines = body.lines().count();
        assert!(lines <= 60, "the manual grew back to {lines} lines");
    }

    /// Short is only acceptable if the full reference is discoverable from it.
    #[test]
    fn the_manual_points_at_the_full_reference() {
        let body = manual_body();
        assert!(body.contains("`?`"), "never mentions the help key");
        assert!(body.contains("Ctrl-S"), "never mentions the provider screen");
        assert!(body.contains("leo doctor"), "never mentions doctor");
    }

    /// The handful of things a first-time user needs on day one must be here,
    /// even though the exhaustive list is not.
    #[test]
    fn the_manual_covers_the_day_one_commands() {
        let body = manual_body();
        for verb in ["new", "search", "mkdir", "mv", "listen", "ask", "sync", "export"] {
            assert!(body.contains(verb), "the manual never mentions `{verb}`");
        }
        // And the keys someone needs before they find the help screen.
        for key in ["j", "k", "Enter", "Tab", "e", "x", "D"] {
            assert!(body.contains(key), "the manual never mentions the {key} key");
        }
    }

    /// No stale instructions: a command the manual names must still exist.
    #[test]
    fn the_manual_names_no_retired_command() {
        let body = manual_body();
        for (alias, _) in crate::action::RETIRED {
            // Checked as a `:` command, since short aliases like `e` and `x`
            // appear as prose elsewhere.
            assert!(
                !body.contains(&format!(":{alias} ")) && !body.contains(&format!(":{alias}\n")),
                "the manual still tells the user to run `:{alias}`"
            );
        }
        assert!(!body.contains("leo env"), "the manual still mentions leo env");
    }

    #[test]
    fn the_manual_has_scrollable_structure_rather_than_one_wall_of_text() {
        let body = manual_body();
        let headings = body.lines().filter(|l| l.starts_with("## ")).count();
        assert!(headings >= 4, "only {headings} sections");
        assert!(body.contains("```"), "no examples to copy");
    }

    /// The manual must say keys are kept apart from the config, and that the
    /// place they live is private — that is the whole security story a user
    /// needs from a quickstart.
    #[test]
    fn the_manual_says_keys_are_kept_separately_and_privately() {
        let body = manual_body().to_lowercase();
        assert!(body.contains("never"), "does not say keys are never in the config");
        assert!(
            body.contains("only your account can read")
                || body.contains("only you can read"),
            "does not say the store is private"
        );
    }

    /// A version bump must rewrite the note the user already has rather than
    /// leaving two manuals side by side.
    #[test]
    fn a_new_manual_version_replaces_the_old_note() {
        let (mut store, _d) = temp_store();
        let first = install_if_absent(&mut store).unwrap().unwrap();

        // Simulate an older release's marker, and an older body.
        std::fs::write(marker_path(&store.notes_dir), "1").unwrap();
        if let Some(note) = store.find_note_mut(&first) {
            note.body = "old text".to_string();
        }
        store.save().unwrap();

        let second = install_if_absent(&mut store).unwrap().unwrap();
        assert_eq!(second, first, "a second manual was created");
        assert_eq!(store.notes.len(), 1, "two manuals now exist");
        assert!(store.find_note(&first).unwrap().body.contains("one-minute"));
    }
}
