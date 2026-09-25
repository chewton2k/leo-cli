use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::notes::Note;

// ── Frontmatter serialization ───────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct NoteFrontmatter {
    id: String,
    title: String,
    #[serde(default)]
    tags: Vec<String>,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

fn note_to_markdown(note: &Note) -> Result<String> {
    let fm = NoteFrontmatter {
        id: note.id.clone(),
        title: note.title.clone(),
        tags: note.tags.clone(),
        created_at: note.created_at,
        updated_at: note.updated_at,
    };
    let yaml = serde_yaml::to_string(&fm)?;
    Ok(format!("---\n{}---\n\n{}", yaml, note.body))
}

fn parse_note_from_markdown(content: &str, relative_path: &Path) -> Result<Note> {
    let rest = content
        .strip_prefix("---\n")
        .context("note file missing opening ---")?;
    let end = rest.find("\n---\n").context("note file missing closing ---")?;
    let yaml_str = &rest[..end];
    let body = rest[end + 5..].trim_start_matches('\n').to_string();

    let fm: NoteFrontmatter =
        serde_yaml::from_str(yaml_str).context("failed to parse frontmatter")?;

    let directory = relative_path
        .parent()
        .and_then(|p| if p == Path::new("") { None } else { p.to_str() })
        .unwrap_or("")
        .to_string();

    Ok(Note {
        id: fm.id,
        title: fm.title,
        body,
        tags: fm.tags,
        directory,
        created_at: fm.created_at,
        updated_at: fm.updated_at,
    })
}

// ── Filesystem helpers ──────────────────────────────────────────────────────

/// notes directory: ~/Library/Application Support/leo/notes  (macOS)
fn notes_dir_path() -> Result<PathBuf> {
    let base = dirs::data_dir().context("Could not determine user data directory")?;
    Ok(base.join("leo").join("notes"))
}

/// Legacy notes.json path — used only for migration detection.
fn old_data_path() -> Result<PathBuf> {
    let base = dirs::data_dir().context("Could not determine user data directory")?;
    Ok(base.join("leo").join("notes.json"))
}

fn load_directories(notes_dir: &Path) -> Result<Vec<String>> {
    let path = notes_dir.join("directories.json");
    if path.exists() {
        let raw = fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&raw)?)
    } else {
        Ok(Vec::new())
    }
}

fn save_directories(notes_dir: &Path, directories: &[String]) -> Result<()> {
    fs::write(
        notes_dir.join("directories.json"),
        serde_json::to_string_pretty(directories)?,
    )?;
    Ok(())
}

/// Collect absolute paths of all .md files under `dir`, skipping hidden dirs.
fn collect_md_paths(dir: &Path, result: &mut HashSet<PathBuf>) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !name.starts_with('.') {
                collect_md_paths(&path, result)?;
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            result.insert(path);
        }
    }
    Ok(())
}

/// Recursively parse all .md files under `dir` into `notes`.
fn collect_notes(notes_dir: &Path, dir: &Path, notes: &mut Vec<Note>) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !name.starts_with('.') {
                collect_notes(notes_dir, &path, notes)?;
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            let content = fs::read_to_string(&path)?;
            let relative = path.strip_prefix(notes_dir).context("path outside notes_dir")?;
            match parse_note_from_markdown(&content, relative) {
                Ok(note) => notes.push(note),
                Err(e) => crate::diag::warn(format!("skipping {}: {e}", path.display())),
            }
        }
    }
    Ok(())
}

// ── Store ───────────────────────────────────────────────────────────────────

/// A change that can be taken back.
///
/// Restoring whole `Note` values rather than replaying an inverse operation: a
/// deleted note must come back with its original id, timestamps and body, and an
/// inverse `create` would not do that.
#[derive(Debug, Clone)]
pub enum Undoable {
    /// Notes that were removed, and directories that went with them. Covers both
    /// a single note and a recursive directory delete, because the difference is
    /// only how many notes are in the list.
    Deleted {
        notes: Vec<Note>,
        directories: Vec<String>,
        what: String,
    },
    /// A note that moved, and where it came from.
    Moved { id: String, from: String, title: String },
    /// A checkbox that was toggled. Toggling is its own inverse.
    Toggled { id: String, n: usize, title: String },
}

impl Undoable {
    /// What to tell the user was undone.
    pub fn describe(&self) -> String {
        match self {
            Undoable::Deleted { what, .. } => format!("Restored {what}"),
            Undoable::Moved { title, from, .. } => {
                let place = if from.is_empty() { "/".to_string() } else { format!("/{from}") };
                format!("Moved \"{title}\" back to {place}")
            }
            Undoable::Toggled { title, n, .. } => {
                format!("Un-toggled box {n} in \"{title}\"")
            }
        }
    }
}

/// How many changes can be taken back.
///
/// Deep enough to cover a mistake noticed a few actions later, shallow enough
/// that the deleted notes it holds are not a memory leak in disguise.
const UNDO_DEPTH: usize = 32;

/// Persistent store backed by per-note .md files in a directory.
pub struct Store {
    pub notes: Vec<Note>,
    pub directories: Vec<String>,
    pub notes_dir: PathBuf,
    /// Most recent change last. Not persisted: undo covers a session, and a
    /// deletion that survived a restart is a decision the user has lived with.
    undo: Vec<Undoable>,
}

impl Store {
    /// Load notes from the platform data directory.
    /// Automatically migrates from legacy notes.json on first run.
    pub fn load() -> Result<Self> {
        let notes_dir = notes_dir_path()?;
        let old_path = old_data_path()?;
        // Treat notes_dir as empty if it doesn't exist, or if it exists but
        // contains no .md files (handles the case where sync init ran first
        // and created notes_dir with .git/ before migration could fire).
        let notes_dir_empty = !notes_dir.exists() || {
            let mut md_paths = std::collections::HashSet::new();
            collect_md_paths(&notes_dir, &mut md_paths).unwrap_or(());
            md_paths.is_empty()
        };
        if old_path.exists() && notes_dir_empty {
            migrate_from_json(&old_path, &notes_dir)?;
        }
        Self::load_from(&notes_dir)
    }

    /// Load notes from a specific directory. Used directly in tests.
    pub fn load_from(notes_dir: &Path) -> Result<Self> {
        fs::create_dir_all(notes_dir)?;
        let directories = load_directories(notes_dir)?;
        let mut notes = Vec::new();
        collect_notes(notes_dir, notes_dir, &mut notes)?;
        Ok(Store {
            undo: Vec::new(),
            notes,
            directories,
            notes_dir: notes_dir.to_path_buf(),
        })
    }

    /// Persist notes to disk with full reconcile (writes new, deletes removed).
    /// All `.md` files in `notes_dir` are owned by the store — any file not
    /// corresponding to a current note will be deleted.
    pub fn save(&self) -> Result<()> {
        fs::create_dir_all(&self.notes_dir)?;

        // Snapshot existing .md paths before writing
        let mut old_paths: HashSet<PathBuf> = HashSet::new();
        collect_md_paths(&self.notes_dir, &mut old_paths)?;

        // Write all current notes
        let mut new_paths: HashSet<PathBuf> = HashSet::new();
        for note in &self.notes {
            let file_path = self.note_path(note);
            if let Some(parent) = file_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&file_path, note_to_markdown(note)?)?;
            new_paths.insert(file_path);
        }

        // Delete files no longer in the notes vec
        for old_path in &old_paths {
            if !new_paths.contains(old_path) {
                fs::remove_file(old_path)?;
            }
        }

        save_directories(&self.notes_dir, &self.directories)?;

        // Auto-commit if git repo is initialized (non-fatal — notes are saved regardless)
        if crate::sync::is_initialized(&self.notes_dir) {
            if let Err(e) = crate::sync::auto_commit(&self.notes_dir) {
                // Every save reaches here, including from the TUI, so this must
                // never write to the terminal directly.
                crate::diag::warn(format!("sync auto-commit failed: {e}"));
            }
        }

        Ok(())
    }

    /// Returns the expected .md file path for a note.
    /// Assumes `note.directory` is a clean relative path with no `..` components.
    fn note_path(&self, note: &Note) -> PathBuf {
        if note.directory.is_empty() {
            self.notes_dir.join(format!("{}.md", note.id))
        } else {
            self.notes_dir
                .join(&note.directory)
                .join(format!("{}.md", note.id))
        }
    }

    /// Create and store a new note, returning a reference to it.
    pub fn create_note(
        &mut self,
        title: impl Into<String>,
        body: impl Into<String>,
        tags: Vec<String>,
        directory: &str,
    ) -> Result<&Note> {
        let note = Note::new(title, body, tags, directory);
        self.notes.push(note);
        Ok(self.notes.last().unwrap())
    }

    /// Return notes sorted newest-first, optionally filtered by tag.
    /// Searches across ALL directories.
    pub fn list_notes(&self, tag: Option<&str>, limit: usize) -> Vec<&Note> {
        let mut notes: Vec<&Note> = self
            .notes
            .iter()
            .filter(|n| match tag {
                Some(t) => n.tags.iter().any(|tag| tag == t),
                None => true,
            })
            .collect();
        notes.sort_by_key(|n| std::cmp::Reverse(n.updated_at));
        notes.truncate(limit);
        notes
    }

    /// Return notes in a specific directory, sorted newest-first.
    pub fn list_notes_in_dir(&self, dir: &str, tag: Option<&str>, limit: usize) -> Vec<&Note> {
        let mut notes: Vec<&Note> = self
            .notes
            .iter()
            .filter(|n| n.directory == dir)
            .filter(|n| match tag {
                Some(t) => n.tags.iter().any(|tag| tag == t),
                None => true,
            })
            .collect();
        notes.sort_by_key(|n| std::cmp::Reverse(n.updated_at));
        notes.truncate(limit);
        notes
    }

    /// Find a note by numeric index, full ID, unique prefix, or title.
    pub fn find_by_index_or_prefix(&self, input: &str) -> Option<&Note> {
        if let Ok(n) = input.parse::<usize>() {
            let list = self.list_notes(None, 20);
            if n >= 1 && n <= list.len() {
                return Some(list[n - 1]);
            }
        }
        if let Some(note) = self.find_note(input) {
            return Some(note);
        }
        let title_matches = self.find_by_title(input);
        if title_matches.len() == 1 {
            return Some(title_matches[0]);
        }
        None
    }

    /// Mutable version of find_by_index_or_prefix.
    pub fn find_by_index_or_prefix_mut(&mut self, input: &str) -> Option<&mut Note> {
        let id = self.find_by_index_or_prefix(input)?.id.clone();
        self.notes.iter_mut().find(|note| note.id == id)
    }

    /// Find notes whose title contains the query (case-insensitive), newest first.
    pub fn find_by_title(&self, query: &str) -> Vec<&Note> {
        let q = query.to_lowercase();
        let mut matches: Vec<&Note> = self
            .notes
            .iter()
            .filter(|n| n.title.to_lowercase().contains(&q))
            .collect();
        matches.sort_by_key(|n| std::cmp::Reverse(n.updated_at));
        matches
    }

    /// Find a note by full ID or unique prefix.
    pub fn find_note(&self, id_prefix: &str) -> Option<&Note> {
        let matches: Vec<&Note> = self
            .notes
            .iter()
            .filter(|n| n.id.starts_with(id_prefix))
            .collect();
        if matches.len() == 1 { Some(matches[0]) } else { None }
    }

    /// Find a mutable note by full ID or unique prefix.
    pub fn find_note_mut(&mut self, id_prefix: &str) -> Option<&mut Note> {
        let mut found = self.notes.iter_mut().filter(|n| n.id.starts_with(id_prefix));
        let first = found.next()?;
        if found.next().is_none() { Some(first) } else { None }
    }

    /// Delete a note by ID prefix; returns true if removed.
    pub fn delete_note(&mut self, id_prefix: &str) -> bool {
        let (removed, kept): (Vec<Note>, Vec<Note>) = std::mem::take(&mut self.notes)
            .into_iter()
            .partition(|n| n.id.starts_with(id_prefix));
        self.notes = kept;

        if removed.is_empty() {
            return false;
        }
        let what = match removed.as_slice() {
            [one] => format!("\"{}\"", one.title),
            many => format!("{} notes", many.len()),
        };
        self.remember(Undoable::Deleted {
            notes: removed,
            directories: Vec::new(),
            what,
        });
        true
    }

    /// Push a change onto the undo stack, discarding the oldest when full.
    fn remember(&mut self, change: Undoable) {
        if self.undo.len() == UNDO_DEPTH {
            self.undo.remove(0);
        }
        self.undo.push(change);
    }

    /// Whether there is anything to take back.
    ///
    /// Used by tests to assert that a no-op records nothing; the handler asks
    /// [`Store::undo`] directly, since it needs the description anyway.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Take back the most recent change, returning what was done.
    ///
    /// `None` when there is nothing to undo, so the caller can say so rather
    /// than silently doing nothing.
    pub fn undo(&mut self) -> Option<String> {
        let change = self.undo.pop()?;
        let described = change.describe();

        match change {
            Undoable::Deleted { notes, directories, .. } => {
                for dir in directories {
                    if !self.directories.contains(&dir) {
                        self.directories.push(dir);
                    }
                }
                self.notes.extend(notes);
                // Restored notes go back in the order the store keeps.
                self.notes.sort_by_key(|n| std::cmp::Reverse(n.created_at));
            }
            Undoable::Moved { id, from, .. } => {
                if let Some(note) = self.find_note_mut(&id) {
                    note.directory = from;
                    note.updated_at = Utc::now();
                }
            }
            Undoable::Toggled { id, n, .. } => {
                // A toggle is its own inverse, so this must not record itself.
                if let Some(note) = self.find_note_mut(&id) {
                    note.toggle_checkbox(n);
                }
            }
        }

        Some(described)
    }

    /// The one search: every directory, titles, bodies and tags.
    ///
    /// Every word must appear somewhere in the note; a `#word` must be the start
    /// of one of its tags, so a partly typed tag already narrows. Notes whose
    /// title holds all the words come first, then other matches, then notes
    /// whose title only fuzzy-matches (`grtrv` for "Graph traversals"). Newest
    /// first within each group. Case never matters.
    pub fn find(&self, query: &str) -> Vec<&Note> {
        let query = query.to_lowercase();
        let (tags, words): (Vec<&str>, Vec<&str>) =
            query.split_whitespace().partition(|w| w.starts_with('#'));
        let tags: Vec<&str> = tags.iter().map(|t| &t[1..]).filter(|t| !t.is_empty()).collect();

        let mut matcher = nucleo::Matcher::new(nucleo::Config::DEFAULT);
        let fuzzy = nucleo::pattern::Pattern::parse(
            &words.join(" "),
            nucleo::pattern::CaseMatching::Ignore,
            nucleo::pattern::Normalization::Smart,
        );

        let mut ranked: Vec<(u8, &Note)> = Vec::new();
        for note in &self.notes {
            let note_tags: Vec<String> = note.tags.iter().map(|t| t.to_lowercase()).collect();
            if !tags.iter().all(|t| note_tags.iter().any(|nt| nt.starts_with(t))) {
                continue;
            }
            let title = note.title.to_lowercase();
            let body = note.body.to_lowercase();
            let in_title = words.iter().all(|w| title.contains(w));
            let anywhere = words
                .iter()
                .all(|w| title.contains(w) || body.contains(w) || note_tags.iter().any(|t| t.contains(w)));
            let group = if in_title {
                0
            } else if anywhere {
                1
            } else {
                let mut buf = Vec::new();
                let haystack = nucleo::Utf32Str::new(&note.title, &mut buf);
                if fuzzy.score(haystack, &mut matcher).is_none() {
                    continue;
                }
                2
            };
            ranked.push((group, note));
        }
        ranked.sort_by(|(ga, a), (gb, b)| ga.cmp(gb).then(b.updated_at.cmp(&a.updated_at)));
        ranked.into_iter().map(|(_, n)| n).collect()
    }

    /// Return all tags with usage counts, sorted most-used first.
    pub fn tags(&self) -> Vec<(String, usize)> {
        let mut counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for note in &self.notes {
            for tag in &note.tags {
                *counts.entry(tag.clone()).or_insert(0) += 1;
            }
        }
        let mut tags: Vec<(String, usize)> = counts.into_iter().collect();
        tags.sort_by_key(|t| std::cmp::Reverse(t.1));
        tags
    }

    /// Toggle the Nth checkbox in a note. Returns the new state text.
    pub fn toggle_checkbox(&mut self, id_prefix: &str, n: usize) -> Option<String> {
        let note = self.find_note_mut(id_prefix)?;
        let (id, title) = (note.id.clone(), note.title.clone());
        let label = note.toggle_checkbox(n)?;
        self.remember(Undoable::Toggled { id, n, title });
        Some(label)
    }

    // ── Directory operations ───────────────────────────────────────────────

    pub fn create_dir(&mut self, path: &str) -> bool {
        let path = path.trim_matches('/');
        if path.is_empty() { return false; }
        let mut created = false;
        let parts: Vec<&str> = path.split('/').collect();
        for i in 0..parts.len() {
            let dir = parts[..=i].join("/");
            if !self.directories.contains(&dir) {
                self.directories.push(dir);
                created = true;
            }
        }
        created
    }

    pub fn dir_exists(&self, path: &str) -> bool {
        if path.is_empty() { return true; }
        self.directories.contains(&path.to_string())
    }

    pub fn subdirs(&self, parent: &str) -> Vec<String> {
        let prefix = if parent.is_empty() {
            String::new()
        } else {
            format!("{parent}/")
        };
        let mut dirs: Vec<String> = self
            .directories
            .iter()
            .filter_map(|d| {
                if parent.is_empty() {
                    if !d.contains('/') { Some(d.clone()) } else { None }
                } else if let Some(rest) = d.strip_prefix(&prefix) {
                    if !rest.is_empty() && !rest.contains('/') {
                        Some(rest.to_string())
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();
        dirs.sort();
        dirs.dedup();
        dirs
    }

    pub fn delete_dir(&mut self, path: &str) -> bool {
        let path = path.trim_matches('/');
        let prefix = format!("{path}/");
        let has_notes = self
            .notes
            .iter()
            .any(|n| n.directory == path || n.directory.starts_with(&prefix));
        let has_subdirs = self.directories.iter().any(|d| d.starts_with(&prefix));
        if has_notes || has_subdirs { return false; }
        let before = self.directories.len();
        self.directories.retain(|d| d != path);
        self.directories.len() < before
    }

    /// What a recursive delete of `path` would remove: notes and directories,
    /// counting everything nested inside it.
    ///
    /// Exists so the confirmation prompt can state the blast radius before the
    /// user commits to it, rather than after.
    pub fn dir_contents(&self, path: &str) -> (usize, usize) {
        let path = path.trim_matches('/');
        if path.is_empty() {
            return (0, 0);
        }
        let prefix = format!("{path}/");
        let notes = self
            .notes
            .iter()
            .filter(|n| n.directory == path || n.directory.starts_with(&prefix))
            .count();
        // The directory itself plus anything below it.
        let dirs = self
            .directories
            .iter()
            .filter(|d| *d == path || d.starts_with(&prefix))
            .count();
        (notes, dirs)
    }

    /// Delete a directory and everything in it. Returns the counts removed.
    ///
    /// Separate from [`Store::delete_dir`], which refuses a non-empty directory:
    /// removing a tree of notes should take a deliberately different call, not a
    /// flag on the safe one.
    pub fn delete_dir_recursive(&mut self, path: &str) -> (usize, usize) {
        let path = path.trim_matches('/');
        if path.is_empty() {
            // Refuse the root. "Delete everything" is not what any single
            // keypress should mean, undo or no undo.
            return (0, 0);
        }
        let prefix = format!("{path}/");

        let (removed_notes, kept): (Vec<Note>, Vec<Note>) = std::mem::take(&mut self.notes)
            .into_iter()
            .partition(|n| n.directory == path || n.directory.starts_with(&prefix));
        self.notes = kept;

        let (removed_dirs, kept_dirs): (Vec<String>, Vec<String>) =
            std::mem::take(&mut self.directories)
                .into_iter()
                .partition(|d| d == path || d.starts_with(&prefix));
        self.directories = kept_dirs;

        let counts = (removed_notes.len(), removed_dirs.len());
        if counts != (0, 0) {
            self.remember(Undoable::Deleted {
                notes: removed_notes,
                directories: removed_dirs,
                what: format!("/{path}"),
            });
        }
        counts
    }

    pub fn move_note(&mut self, id_prefix: &str, new_dir: &str) -> Option<String> {
        let note = self.find_note_mut(id_prefix)?;
        let from = note.directory.clone();
        let (id, title) = (note.id.clone(), note.title.clone());
        note.directory = new_dir.to_string();
        note.updated_at = Utc::now();
        self.remember(Undoable::Moved {
            id,
            from,
            title: title.clone(),
        });
        Some(title)
    }
}

// ── Migration ───────────────────────────────────────────────────────────────

fn migrate_from_json(old_path: &Path, notes_dir: &Path) -> Result<()> {
    let raw = fs::read_to_string(old_path)
        .with_context(|| format!("failed to read {}", old_path.display()))?;

    #[derive(serde::Deserialize)]
    struct LegacyStore {
        notes: Vec<Note>,
        #[serde(default)]
        directories: Vec<String>,
    }

    let (notes, directories): (Vec<Note>, Vec<String>) = if raw.trim_start().starts_with('[') {
        let notes: Vec<Note> = serde_json::from_str(&raw)?;
        (notes, Vec::new())
    } else {
        let data: LegacyStore = serde_json::from_str(&raw)?;
        (data.notes, data.directories)
    };

    let count = notes.len();
    fs::create_dir_all(notes_dir)?;

    for note in &notes {
        let file_path = if note.directory.is_empty() {
            notes_dir.join(format!("{}.md", note.id))
        } else {
            let dir = notes_dir.join(&note.directory);
            fs::create_dir_all(&dir)?;
            dir.join(format!("{}.md", note.id))
        };
        fs::write(file_path, note_to_markdown(note)?)?;
    }

    save_directories(notes_dir, &directories)?;

    fs::rename(old_path, old_path.with_extension("json.bak"))?;

    crate::diag::warn(format!("migrated {count} notes to {}", notes_dir.display()));
    Ok(())
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {

    use super::*;
    use crate::notes::Note;

    fn make_note() -> Note {
        Note {
            id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            title: "Test Note".to_string(),
            body: "Hello **world**".to_string(),
            tags: vec!["rust".to_string(), "test".to_string()],
            directory: "".to_string(),
            created_at: chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
                .unwrap()
                .with_timezone(&chrono::Utc),
            updated_at: chrono::DateTime::parse_from_rfc3339("2026-01-02T00:00:00Z")
                .unwrap()
                .with_timezone(&chrono::Utc),
        }
    }

    fn store_with_tree() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::load_from(&dir.path().join("notes")).unwrap();
        store.create_dir("cs130");
        store.create_dir("cs130/lec");
        store.create_dir("cs162");
        store.create_note("Root note", "b", vec![], "").unwrap();
        store.create_note("In cs130", "b", vec![], "cs130").unwrap();
        store.create_note("In lec 1", "b", vec![], "cs130/lec").unwrap();
        store.create_note("In lec 2", "b", vec![], "cs130/lec").unwrap();
        store.create_note("In cs162", "b", vec![], "cs162").unwrap();
        store.save().unwrap();
        (store, dir)
    }

    #[test]
    fn dir_contents_counts_everything_nested() {
        let (store, _d) = store_with_tree();
        // cs130 itself, cs130/lec, and the three notes between them.
        assert_eq!(store.dir_contents("cs130"), (3, 2));
        assert_eq!(store.dir_contents("cs130/lec"), (2, 1));
        assert_eq!(store.dir_contents("cs162"), (1, 1));
        // An unknown directory has nothing in it.
        assert_eq!(store.dir_contents("nope"), (0, 0));
        // The root is never reported as deletable.
        assert_eq!(store.dir_contents(""), (0, 0));
    }

    /// A sibling directory whose name merely starts with the same letters must
    /// not be swept up: "cs13" is not a parent of "cs130".
    #[test]
    fn dir_contents_matches_path_segments_not_string_prefixes() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::load_from(&dir.path().join("notes")).unwrap();
        store.create_dir("cs13");
        store.create_dir("cs130");
        store.create_note("In cs130", "b", vec![], "cs130").unwrap();

        assert_eq!(store.dir_contents("cs13"), (0, 1), "cs130 is not inside cs13");
        assert_eq!(store.dir_contents("cs130"), (1, 1));
    }

    #[test]
    fn deleting_a_directory_recursively_removes_its_notes_and_subdirectories() {
        let (mut store, _d) = store_with_tree();

        let (notes, dirs) = store.delete_dir_recursive("cs130");
        assert_eq!((notes, dirs), (3, 2));

        assert!(!store.dir_exists("cs130"));
        assert!(!store.dir_exists("cs130/lec"));
        // Untouched neighbours.
        assert!(store.dir_exists("cs162"));
        assert_eq!(store.notes.len(), 2);
        assert!(store.find_by_title("Root note").len() == 1);
        assert!(store.find_by_title("In cs162").len() == 1);

        // And it survives a reload: the files are gone from disk.
        store.save().unwrap();
        let reloaded = Store::load_from(&store.notes_dir).unwrap();
        assert_eq!(reloaded.notes.len(), 2);
        assert!(!reloaded.dir_exists("cs130"));
    }

    #[test]
    fn deleting_a_leaf_directory_leaves_its_parent() {
        let (mut store, _d) = store_with_tree();
        assert_eq!(store.delete_dir_recursive("cs130/lec"), (2, 1));
        assert!(store.dir_exists("cs130"), "the parent must survive");
        assert_eq!(store.find_by_title("In cs130").len(), 1);
    }

    /// "Delete everything" is not something any keypress should be able to mean.
    #[test]
    fn the_root_cannot_be_deleted_recursively() {
        let (mut store, _d) = store_with_tree();
        assert_eq!(store.delete_dir_recursive(""), (0, 0));
        assert_eq!(store.delete_dir_recursive("/"), (0, 0));
        assert_eq!(store.notes.len(), 5, "nothing was removed");
        assert!(store.dir_exists("cs130"));
    }

    #[test]
    fn deleting_an_unknown_directory_removes_nothing() {
        let (mut store, _d) = store_with_tree();
        assert_eq!(store.delete_dir_recursive("ghost"), (0, 0));
        assert_eq!(store.notes.len(), 5);
    }

    /// The safe delete still refuses a non-empty directory, so the recursive one
    /// is the only way to lose notes.
    #[test]
    fn the_non_recursive_delete_still_refuses_a_non_empty_directory() {
        let (mut store, _d) = store_with_tree();
        assert!(!store.delete_dir("cs130"));
        assert!(store.dir_exists("cs130"));
        assert_eq!(store.notes.len(), 5);
    }

    #[test]
    fn test_note_roundtrip() {
        let note = make_note();
        let md = note_to_markdown(&note).unwrap();
        let parsed =
            parse_note_from_markdown(&md, std::path::Path::new("550e8400.md")).unwrap();
        assert_eq!(parsed.id, note.id);
        assert_eq!(parsed.title, note.title);
        assert_eq!(parsed.body, note.body);
        assert_eq!(parsed.tags, note.tags);
        assert_eq!(parsed.directory, "");
    }

    #[test]
    fn test_directory_derived_from_path() {
        let note = make_note();
        let md = note_to_markdown(&note).unwrap();
        let parsed = parse_note_from_markdown(
            &md,
            std::path::Path::new("cs162/lec/550e8400.md"),
        )
        .unwrap();
        assert_eq!(parsed.directory, "cs162/lec");
    }

    #[test]
    fn test_note_path_root() {
        let tmp = tempfile::TempDir::new().unwrap();
        let notes_dir = tmp.path().join("notes");
        let store = Store { notes: vec![], directories: vec![], notes_dir: notes_dir.clone(), undo: Vec::new() };
        let note = make_note();
        assert_eq!(
            store.note_path(&note),
            notes_dir.join("550e8400-e29b-41d4-a716-446655440000.md")
        );
    }

    #[test]
    fn test_note_path_subdir() {
        let tmp = tempfile::TempDir::new().unwrap();
        let notes_dir = tmp.path().join("notes");
        let store = Store { notes: vec![], directories: vec![], notes_dir: notes_dir.clone(), undo: Vec::new() };
        let mut note = make_note();
        note.directory = "cs162/lec".to_string();
        assert_eq!(
            store.note_path(&note),
            notes_dir.join("cs162/lec/550e8400-e29b-41d4-a716-446655440000.md")
        );
    }

    #[test]
    fn test_load_from_reads_md_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let notes_dir = tmp.path().join("notes");
        std::fs::create_dir_all(&notes_dir).unwrap();

        let note = make_note();
        std::fs::write(
            notes_dir.join("550e8400-e29b-41d4-a716-446655440000.md"),
            note_to_markdown(&note).unwrap(),
        )
        .unwrap();

        let mut note2 = make_note();
        note2.id = "aaaabbbb-0000-0000-0000-000000000000".to_string();
        note2.title = "Subdir Note".to_string();
        note2.directory = "cs162".to_string();
        std::fs::create_dir_all(notes_dir.join("cs162")).unwrap();
        std::fs::write(
            notes_dir.join("cs162/aaaabbbb-0000-0000-0000-000000000000.md"),
            note_to_markdown(&note2).unwrap(),
        )
        .unwrap();

        let store = Store::load_from(&notes_dir).unwrap();
        assert_eq!(store.notes.len(), 2);
        let loaded = store.notes.iter().find(|n| n.id == note.id).unwrap();
        assert_eq!(loaded.directory, "");
        assert_eq!(loaded.title, "Test Note");
        assert_eq!(loaded.body, "Hello **world**");
        let loaded2 = store.notes.iter().find(|n| n.id == note2.id).unwrap();
        assert_eq!(loaded2.directory, "cs162");
        assert_eq!(loaded2.title, "Subdir Note");
    }

    #[test]
    fn test_save_writes_md_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let notes_dir = tmp.path().join("notes");
        std::fs::create_dir_all(&notes_dir).unwrap();
        let store = Store {
            undo: Vec::new(),
            notes: vec![make_note()],
            directories: vec![],
            notes_dir: notes_dir.clone(),
        };
        store.save().unwrap();
        assert!(notes_dir.join("550e8400-e29b-41d4-a716-446655440000.md").exists());
    }

    #[test]
    fn test_save_deletes_orphaned_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let notes_dir = tmp.path().join("notes");
        std::fs::create_dir_all(&notes_dir).unwrap();

        let orphan = notes_dir.join("deadbeef-0000-0000-0000-000000000000.md");
        std::fs::write(&orphan, "---\nid: deadbeef-0000-0000-0000-000000000000\ntitle: Old\ntags: []\ncreated_at: '2026-01-01T00:00:00Z'\nupdated_at: '2026-01-01T00:00:00Z'\n---\n\nbody").unwrap();

        let store = Store {
            undo: Vec::new(),
            notes: vec![make_note()],
            directories: vec![],
            notes_dir: notes_dir.clone(),
        };
        store.save().unwrap();

        assert!(!orphan.exists(), "orphaned file should be deleted");
        assert!(notes_dir.join("550e8400-e29b-41d4-a716-446655440000.md").exists());
    }

    #[test]
    fn test_migrate_from_json() {
        let tmp = tempfile::TempDir::new().unwrap();
        let old_path = tmp.path().join("notes.json");
        let json = serde_json::json!({
            "notes": [{
                "id": "550e8400-e29b-41d4-a716-446655440000",
                "title": "Migrated Note",
                "body": "content",
                "tags": ["rust"],
                "directory": "",
                "created_at": "2026-01-01T00:00:00Z",
                "updated_at": "2026-01-02T00:00:00Z"
            }],
            "directories": []
        });
        std::fs::write(&old_path, serde_json::to_string(&json).unwrap()).unwrap();

        let notes_dir = tmp.path().join("notes");
        assert!(!notes_dir.exists());

        migrate_from_json(&old_path, &notes_dir).unwrap();

        assert!(notes_dir.exists());
        assert!(notes_dir.join("550e8400-e29b-41d4-a716-446655440000.md").exists());
        assert!(!old_path.exists(), "notes.json should be renamed");
        assert!(tmp.path().join("notes.json.bak").exists());
    }

    #[test]
    fn test_save_handles_directory_move() {
        let tmp = tempfile::TempDir::new().unwrap();
        let notes_dir = tmp.path().join("notes");
        std::fs::create_dir_all(&notes_dir).unwrap();

        let note = make_note();
        let store = Store {
            undo: Vec::new(),
            notes: vec![note.clone()],
            directories: vec![],
            notes_dir: notes_dir.clone(),
        };
        store.save().unwrap();
        assert!(notes_dir.join("550e8400-e29b-41d4-a716-446655440000.md").exists());

        let mut moved = note.clone();
        moved.directory = "ideas".to_string();
        let store2 = Store {
            undo: Vec::new(),
            notes: vec![moved],
            directories: vec!["ideas".to_string()],
            notes_dir: notes_dir.clone(),
        };
        store2.save().unwrap();

        assert!(notes_dir.join("ideas/550e8400-e29b-41d4-a716-446655440000.md").exists());
        assert!(
            !notes_dir.join("550e8400-e29b-41d4-a716-446655440000.md").exists(),
            "old location should be removed after move"
        );
    }

    #[test]
    fn test_save_auto_commits_when_initialized() {
        let tmp = tempfile::TempDir::new().unwrap();
        let notes_dir = tmp.path().join("notes");
        std::fs::create_dir_all(&notes_dir).unwrap();

        crate::sync::init(&notes_dir).unwrap();

        let store = Store {
            undo: Vec::new(),
            notes: vec![make_note()],
            directories: vec![],
            notes_dir: notes_dir.clone(),
        };
        store.save().unwrap();

        let log = std::process::Command::new("git")
            .args(["log", "--oneline"])
            .current_dir(&notes_dir)
            .output()
            .unwrap();
        let log_str = String::from_utf8(log.stdout).unwrap();
        assert!(
            log_str.contains("update notes"),
            "expected auto-commit, got: {log_str}"
        );
    }

    // ── undo ────────────────────────────────────────────────────────────────

    /// An empty store in a temporary directory.
    fn temp_store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::load_from(&dir.path().join("notes")).unwrap();
        (store, dir)
    }


    // ── find ────────────────────────────────────────────────────────────────

    fn titles(found: Vec<&Note>) -> Vec<String> {
        found.iter().map(|n| n.title.clone()).collect()
    }

    fn store_for_find() -> (Store, tempfile::TempDir) {
        let (mut store, d) = temp_store();
        store.create_dir("cs130");
        store.create_note("Rust ownership", "borrow checker rules", vec!["rust".into()], "").unwrap();
        store.create_note("Graph traversals", "BFS explores level by level", vec![], "cs130").unwrap();
        store.create_note("Lecture 4", "graphs: BFS, then DFS", vec!["exam".into()], "cs130").unwrap();
        (store, d)
    }

    /// One search, everywhere: bodies and other directories included.
    #[test]
    fn find_looks_in_bodies_in_every_directory() {
        let (store, _d) = store_for_find();
        assert_eq!(titles(store.find("borrow")), vec!["Rust ownership"]);
        let bfs = titles(store.find("bfs"));
        assert_eq!(bfs.len(), 2, "{bfs:?}");
    }

    #[test]
    fn a_title_match_ranks_ahead_of_a_body_match() {
        let (store, _d) = store_for_find();
        let found = titles(store.find("graph"));
        assert_eq!(found, vec!["Graph traversals", "Lecture 4"]);
    }

    #[test]
    fn every_word_has_to_match_somewhere() {
        let (store, _d) = store_for_find();
        assert_eq!(titles(store.find("bfs dfs")), vec!["Lecture 4"]);
    }

    #[test]
    fn a_hash_word_means_a_tag() {
        let (store, _d) = store_for_find();
        assert_eq!(titles(store.find("#exam")), vec!["Lecture 4"]);
        // Narrowed further by ordinary words.
        assert!(store.find("#exam borrow").is_empty());
        // A partly typed tag already narrows, since this runs on every keystroke.
        assert_eq!(titles(store.find("#ex")), vec!["Lecture 4"]);
    }

    /// Fuzzy title matching, which the Ctrl-P finder used to offer, still finds
    /// a note from a few letters of its title.
    #[test]
    fn a_few_letters_of_a_title_still_find_it() {
        let (store, _d) = store_for_find();
        assert_eq!(titles(store.find("grtrv")), vec!["Graph traversals"]);
    }

    #[test]
    fn find_ignores_case() {
        let (store, _d) = store_for_find();
        assert_eq!(titles(store.find("RUST")), vec!["Rust ownership"]);
    }

    /// The whole point: a deleted note comes back as it was, not as a copy.
    #[test]
    fn undoing_a_delete_restores_the_note_exactly() {
        let (mut store, _d) = temp_store();
        let note = store.create_note("Keep me", "body text", vec!["tag".into()], "").unwrap();
        let (id, created, updated) = (note.id.clone(), note.created_at, note.updated_at);

        assert!(store.delete_note(&id));
        assert!(store.find_note(&id).is_none());

        let described = store.undo().expect("something to undo");
        assert!(described.contains("Keep me"), "{described}");

        let back = store.find_note(&id).expect("the note is back");
        assert_eq!(back.title, "Keep me");
        assert_eq!(back.body, "body text");
        assert_eq!(back.tags, vec!["tag"]);
        // Same identity and timestamps: a restored note is the original, not a
        // new note that happens to look similar.
        assert_eq!(back.id, id);
        assert_eq!(back.created_at, created);
        assert_eq!(back.updated_at, updated);
    }

    /// The scariest action must be the most reversible.
    #[test]
    fn undoing_a_recursive_directory_delete_restores_everything() {
        let (mut store, _d) = temp_store();
        store.create_dir("cs130");
        store.create_dir("cs130/week1");
        store.create_note("A", "a", vec![], "cs130").unwrap();
        store.create_note("B", "b", vec![], "cs130/week1").unwrap();
        store.create_note("Elsewhere", "e", vec![], "").unwrap();

        let (notes, dirs) = store.delete_dir_recursive("cs130");
        assert_eq!((notes, dirs), (2, 2));
        assert_eq!(store.notes.len(), 1, "only the outside note should remain");

        let described = store.undo().expect("something to undo");
        assert!(described.contains("/cs130"), "{described}");
        assert_eq!(store.notes.len(), 3);
        assert!(store.dir_exists("cs130"));
        assert!(store.dir_exists("cs130/week1"));
        // And each note is back where it lived.
        let a = store.find_by_title("A").first().map(|n| n.directory.clone());
        assert_eq!(a.as_deref(), Some("cs130"));
    }

    #[test]
    fn undoing_a_move_puts_a_note_back() {
        let (mut store, _d) = temp_store();
        store.create_dir("cs130");
        let id = store.create_note("Wanderer", "b", vec![], "").unwrap().id.clone();

        store.move_note(&id, "cs130");
        assert_eq!(store.find_note(&id).unwrap().directory, "cs130");

        let described = store.undo().unwrap();
        assert!(described.contains("Wanderer"), "{described}");
        assert_eq!(store.find_note(&id).unwrap().directory, "");
    }

    #[test]
    fn undoing_a_checkbox_toggle_unticks_it() {
        let (mut store, _d) = temp_store();
        let id = store
            .create_note("Tasks", "- [ ] one\n- [ ] two", vec![], "")
            .unwrap()
            .id
            .clone();

        store.toggle_checkbox(&id, 1);
        assert!(store.find_note(&id).unwrap().body.contains("- [x] one"));

        store.undo().expect("something to undo");
        assert!(
            store.find_note(&id).unwrap().body.contains("- [ ] one"),
            "{}",
            store.find_note(&id).unwrap().body
        );
    }

    /// Undo must not undo itself: one press, one step back.
    #[test]
    fn undoing_does_not_stack_its_own_inverse() {
        let (mut store, _d) = temp_store();
        let id = store.create_note("Tasks", "- [ ] one", vec![], "").unwrap().id.clone();

        store.toggle_checkbox(&id, 1);
        assert!(store.can_undo());
        store.undo();
        assert!(!store.can_undo(), "undo pushed its own inverse onto the stack");
    }

    #[test]
    fn undo_steps_back_through_several_changes_newest_first() {
        let (mut store, _d) = temp_store();
        let a = store.create_note("A", "a", vec![], "").unwrap().id.clone();
        let b = store.create_note("B", "b", vec![], "").unwrap().id.clone();

        store.delete_note(&a);
        store.delete_note(&b);
        assert!(store.notes.is_empty());

        store.undo();
        assert!(store.find_note(&b).is_some(), "B was deleted last, so it returns first");
        assert!(store.find_note(&a).is_none());

        store.undo();
        assert!(store.find_note(&a).is_some());
        assert!(!store.can_undo());
    }

    #[test]
    fn undoing_nothing_says_so_rather_than_doing_nothing_silently() {
        let (mut store, _d) = temp_store();
        assert!(!store.can_undo());
        assert!(store.undo().is_none());
    }

    /// A failed delete must not leave a no-op on the stack, or `u` would appear
    /// to do nothing.
    #[test]
    fn a_delete_that_matched_nothing_records_nothing() {
        let (mut store, _d) = temp_store();
        store.create_note("A", "a", vec![], "").unwrap();
        assert!(!store.delete_note("does-not-exist"));
        assert!(!store.can_undo());
    }

    #[test]
    fn refusing_to_delete_the_root_records_nothing() {
        let (mut store, _d) = temp_store();
        store.create_note("A", "a", vec![], "").unwrap();
        assert_eq!(store.delete_dir_recursive(""), (0, 0));
        assert!(!store.can_undo());
    }

    /// The stack is bounded, or a long session holds every note ever deleted.
    #[test]
    fn the_undo_stack_is_bounded_and_keeps_the_newest() {
        let (mut store, _d) = temp_store();
        let mut ids = Vec::new();
        for i in 0..UNDO_DEPTH + 5 {
            let id = store
                .create_note(format!("N{i}"), "b", vec![], "")
                .unwrap()
                .id
                .clone();
            ids.push(id);
        }
        for id in &ids {
            store.delete_note(id);
        }

        let mut undone = 0;
        while store.undo().is_some() {
            undone += 1;
        }
        assert_eq!(undone, UNDO_DEPTH, "the stack grew past its bound");
        // The newest deletions are the ones that survived.
        assert!(store.find_note(ids.last().unwrap()).is_some());
        assert!(store.find_note(ids.first().unwrap()).is_none());
    }

    /// Restoring must survive a round trip to disk, or undo is a lie the moment
    /// the store is saved.
    #[test]
    fn a_restored_note_is_written_back_to_disk() {
        let (mut store, _d) = temp_store();
        let id = store.create_note("Persisted", "body", vec![], "").unwrap().id.clone();
        store.save().unwrap();

        store.delete_note(&id);
        store.save().unwrap();
        assert!(Store::load_from(&store.notes_dir).unwrap().find_note(&id).is_none());

        store.undo();
        store.save().unwrap();
        let reloaded = Store::load_from(&store.notes_dir).unwrap();
        let back = reloaded.find_note(&id).expect("restored note missing from disk");
        assert_eq!(back.title, "Persisted");
        assert_eq!(back.body, "body");
    }
}
