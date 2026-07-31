//! The notes you were just looking at.
//!
//! A small, ordered set of recently opened notes, like an editor's list of open
//! tabs. The point is the return trip: writing notes means going back and forth
//! between two or three of them, and finding the same note again through the
//! pane or the finder every time is the friction this removes.

use std::path::{Path, PathBuf};

/// How many notes to remember. Small on purpose: a list long enough to need
/// searching is a worse version of the finder, which already exists.
pub const CAPACITY: usize = 5;

/// Recently opened note IDs, most recent first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recent {
    ids: Vec<String>,
}

impl Recent {
    /// Record a visit. Moves an already-known note to the front rather than
    /// duplicating it, so the list is an order of use, not a log.
    pub fn touch(&mut self, id: &str) {
        if id.is_empty() {
            return;
        }
        self.ids.retain(|existing| existing != id);
        self.ids.insert(0, id.to_string());
        self.ids.truncate(CAPACITY);
    }

    /// Most recent first.
    pub fn ids(&self) -> &[String] {
        &self.ids
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// Drop anything that no longer exists.
    ///
    /// Notes get deleted, and a list of dead ids would offer the user rows that
    /// do nothing when chosen.
    pub fn retain_existing(&mut self, exists: impl Fn(&str) -> bool) {
        self.ids.retain(|id| exists(id));
    }

    /// The nth entry, for jumping straight to it.
    pub fn nth(&self, index: usize) -> Option<&String> {
        self.ids.get(index)
    }

    /// Where the list is kept: beside the config, not inside the notes
    /// directory, which is what `sync` pushes to a git remote. Which notes this
    /// machine visited is not something to publish or to carry between machines.
    fn path() -> Option<PathBuf> {
        crate::config::Config::config_path()
            .ok()
            .map(|p| p.with_file_name("recent.json"))
    }

    /// Read the list, or an empty one. Never fails: a missing or corrupt file
    /// means no history, which is a fine state to be in.
    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        Self::load_from(&path)
    }

    pub fn load_from(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        match serde_json::from_str::<Vec<String>>(&text) {
            Ok(mut ids) => {
                ids.truncate(CAPACITY);
                Self { ids }
            }
            Err(_) => Self::default(),
        }
    }

    /// Persist, quietly. A failure here must never interrupt note-taking: this
    /// is a convenience, and the cost of losing it is one extra keypress.
    pub fn save(&self) {
        if let Some(path) = Self::path() {
            self.save_to(&path);
        }
    }

    pub fn save_to(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string(&self.ids) {
            let _ = std::fs::write(path, json);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_most_recent_note_is_first() {
        let mut recent = Recent::default();
        recent.touch("a");
        recent.touch("b");
        recent.touch("c");
        assert_eq!(recent.ids(), ["c", "b", "a"]);
    }

    /// Revisiting must reorder, not duplicate: this is an order of use, not a log.
    #[test]
    fn revisiting_a_note_moves_it_to_the_front() {
        let mut recent = Recent::default();
        recent.touch("a");
        recent.touch("b");
        recent.touch("a");
        assert_eq!(recent.ids(), ["a", "b"]);
    }

    #[test]
    fn the_list_is_capped_and_forgets_the_oldest() {
        let mut recent = Recent::default();
        for i in 0..CAPACITY + 3 {
            recent.touch(&format!("note{i}"));
        }
        assert_eq!(recent.ids().len(), CAPACITY);
        assert_eq!(recent.nth(0).unwrap(), &format!("note{}", CAPACITY + 2));
        // The earliest visits are gone.
        assert!(!recent.ids().contains(&"note0".to_string()));
    }

    #[test]
    fn an_empty_id_is_ignored() {
        let mut recent = Recent::default();
        recent.touch("");
        assert!(recent.is_empty());
    }

    /// A deleted note must leave the list, or it offers a row that does nothing.
    #[test]
    fn entries_that_no_longer_exist_are_dropped() {
        let mut recent = Recent::default();
        recent.touch("gone");
        recent.touch("kept");
        recent.retain_existing(|id| id == "kept");
        assert_eq!(recent.ids(), ["kept"]);
    }

    #[test]
    fn the_list_survives_a_round_trip_to_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("recent.json");

        let mut recent = Recent::default();
        recent.touch("a");
        recent.touch("b");
        recent.save_to(&path);

        assert_eq!(Recent::load_from(&path), recent);
    }

    #[test]
    fn a_missing_file_means_no_history_rather_than_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Recent::load_from(&dir.path().join("nope.json")).is_empty());
    }

    #[test]
    fn a_corrupt_file_means_no_history_rather_than_a_crash() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("recent.json");
        std::fs::write(&path, "{not json").unwrap();
        assert!(Recent::load_from(&path).is_empty());
    }

    /// A file written by a future version with a longer list must not blow past
    /// the cap on load.
    #[test]
    fn an_over_long_file_is_truncated_on_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("recent.json");
        let many: Vec<String> = (0..50).map(|i| format!("n{i}")).collect();
        std::fs::write(&path, serde_json::to_string(&many).unwrap()).unwrap();

        assert_eq!(Recent::load_from(&path).ids().len(), CAPACITY);
    }

    /// Saving must not create anything inside the notes directory, which is what
    /// `sync` pushes.
    #[test]
    fn the_list_lives_beside_the_config_not_with_the_notes() {
        if let Some(path) = Recent::path() {
            assert_eq!(path.file_name().unwrap(), "recent.json");
            assert!(
                !path.to_string_lossy().contains("notes"),
                "recent.json would be committed by sync: {}",
                path.display()
            );
        }
    }
}
