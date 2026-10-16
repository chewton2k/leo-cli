use std::io::{Read, Seek, Write};
use std::path::Path;

use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;

#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct Parts {
    #[serde(default)]
    pub uploads: bool,
    #[serde(default)]
    pub chats: bool,
    #[serde(default)]
    pub trash: bool,
}

#[derive(Debug, Default, PartialEq)]
pub struct Packed {
    pub files: usize,
    pub bytes: u64,
}

const README: &str = "Everything leo exported.

notes/    your notes as Markdown files, in their folders. Any Markdown
          editor opens them; leo reads them back if you copy them into its
          notes folder.
uploads/  the files you uploaded, in a folder per note (named by the note's id,
          which is the id: line at the top of the note).
chats/    your conversations with Felix, one JSON file each.
interviews/ interview frameworks, workspaces, attempts and practice schedules,
          included with chats.
tasks.json the tasks you added on the Calendar page (Storage can restore
          them). Tasks from notes are in the notes themselves.
trash/    notes in the trash, as Markdown.
recording-sources/ retained transcripts, original points and diagnostics,
          included with uploads. Audio recovery and calendar links
          stay on your computer.

Settings and API keys are never included.
";

const MOST_DEPTH: usize = 64;
const MOST_ENTRIES: usize = 100_000;
const MOST_BYTES: u64 = 10 * 1024 * 1024 * 1024;
const LIMIT_MESSAGE: &str = "This export is too large. Export fewer parts or use shallower folders; Leo exports up to 100,000 items, 64 folder levels and 10 GiB at a time.";

#[derive(Clone, Copy)]
struct Limits {
    depth: usize,
    entries: usize,
    bytes: u64,
}

const LIMITS: Limits = Limits {
    depth: MOST_DEPTH,
    entries: MOST_ENTRIES,
    bytes: MOST_BYTES,
};

struct Budget {
    limits: Limits,
    entries: usize,
    bytes: u64,
}

impl Budget {
    fn new(limits: Limits) -> Self {
        Self {
            limits,
            entries: 0,
            bytes: 0,
        }
    }

    fn entry(&mut self) -> Result<()> {
        ensure!(self.entries < self.limits.entries, LIMIT_MESSAGE);
        self.entries += 1;
        Ok(())
    }

    fn remaining_bytes(&self) -> u64 {
        self.limits.bytes.saturating_sub(self.bytes)
    }
}

struct BudgetWriter<'a, W> {
    out: &'a mut W,
    budget: &'a mut Budget,
}

impl<W: Write> Write for BudgetWriter<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() as u64 > self.budget.remaining_bytes() {
            return Err(std::io::Error::other(LIMIT_MESSAGE));
        }
        let written = self.out.write(bytes)?;
        self.budget.bytes += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.out.flush()
    }
}

fn checked_path(root: &Path, path: &Path) -> Result<std::path::PathBuf> {
    match std::fs::symlink_metadata(root) {
        Ok(meta) => ensure!(meta.is_dir(), "Export folders cannot use symbolic links; replace the link with a real folder and try again."),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("The export folder could not be checked; check its permissions and try again"),
    }
    let relative = path
        .strip_prefix(root)
        .context("Export files must stay inside Leo's data folder")?;
    if relative.as_os_str().is_empty() {
        return Ok(root.to_path_buf());
    }
    leo_core::paths::contained_path(root, relative)
        .context("Export folders cannot use symbolic links; replace the link with a real folder and try again")
}

#[allow(clippy::too_many_arguments)]
fn add_dir<W: Write + Seek>(
    zip: &mut zip::ZipWriter<W>,
    root: &Path,
    from: &Path,
    under: &str,
    skip_hidden: bool,
    depth: usize,
    budget: &mut Budget,
    packed: &mut Packed,
) -> Result<()> {
    ensure!(depth <= budget.limits.depth, LIMIT_MESSAGE);
    let from = checked_path(root, from)?;
    let entries = match std::fs::read_dir(&from) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error).context(
                "The export folder could not be read; check its permissions and try again",
            )
        }
    };
    let mut sorted = Vec::new();
    for entry in entries {
        budget.entry()?;
        sorted.push(
            entry.context(
                "An export item could not be read; check folder permissions and try again",
            )?,
        );
    }
    sorted.sort_by_key(|entry| entry.file_name());
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for entry in sorted {
        let name = entry.file_name().to_string_lossy().to_string();
        if skip_hidden && name.starts_with('.') {
            continue;
        }
        ensure!(
            !name.contains(['/', '\\']) && !name.chars().any(char::is_control),
            "An export filename contains unsupported characters; rename it and try again"
        );
        let kind = entry
            .file_type()
            .context("An export item could not be checked; check its permissions and try again")?;
        let inside = format!("{under}/{name}");
        if kind.is_dir() {
            add_dir(
                zip,
                root,
                &entry.path(),
                &inside,
                skip_hidden,
                depth + 1,
                budget,
                packed,
            )?;
        } else if kind.is_file() {
            let path = checked_path(root, &entry.path())?;
            let mut open = std::fs::OpenOptions::new();
            open.read(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                open.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
            }
            let file = open.open(path).context(
                "An export file could not be opened safely; check its permissions and try again",
            )?;
            let meta = file.metadata()?;
            ensure!(
                meta.is_file(),
                "Only regular files can be exported; remove the special file and try again"
            );
            ensure!(meta.len() <= budget.remaining_bytes(), LIMIT_MESSAGE);
            zip.start_file(&inside, options)?;
            let mut input = file.take(budget.remaining_bytes() + 1);
            let before = budget.bytes;
            std::io::copy(
                &mut input,
                &mut BudgetWriter {
                    out: &mut *zip,
                    budget: &mut *budget,
                },
            )?;
            packed.bytes += budget.bytes - before;
            packed.files += 1;
        }
    }
    Ok(())
}

fn add_json<W: Write + Seek>(
    zip: &mut zip::ZipWriter<W>,
    inside: &str,
    value: &impl Serialize,
    budget: &mut Budget,
    packed: &mut Packed,
) -> Result<()> {
    budget.entry()?;
    zip.start_file(inside, SimpleFileOptions::default())?;
    let before = budget.bytes;
    serde_json::to_writer_pretty(
        BudgetWriter {
            out: &mut *zip,
            budget: &mut *budget,
        },
        value,
    )?;
    packed.bytes += budget.bytes - before;
    packed.files += 1;
    Ok(())
}

fn chat_ids(chats: &Path, budget: &Budget) -> Result<Vec<String>> {
    let remaining = budget.limits.entries.saturating_sub(budget.entries);
    let ids: Vec<String> = crate::chats::db_of(chats).with(|connection| {
        let mut query =
            connection.prepare("SELECT id FROM chats ORDER BY updated_at DESC, id LIMIT ?1")?;
        let rows = query.query_map([(remaining + 1) as i64], |row| row.get(0))?;
        rows.collect()
    })?;
    ensure!(ids.len() <= remaining, LIMIT_MESSAGE);
    Ok(ids)
}

fn documents(chats: &Path, chat: &str, budget: &Budget) -> Result<Vec<String>> {
    let remaining = budget.limits.entries.saturating_sub(budget.entries);
    let ids: Vec<String> = crate::chats::db_of(chats).with(|connection| {
        let mut query = connection
            .prepare("SELECT id FROM chat_docs WHERE chat = ?1 ORDER BY added_at, id LIMIT ?2")?;
        let rows = query.query_map(rusqlite::params![chat, (remaining + 1) as i64], |row| {
            row.get(0)
        })?;
        rows.collect()
    })?;
    ensure!(ids.len() <= remaining, LIMIT_MESSAGE);
    Ok(ids)
}

pub fn write_zip<W: Write + Seek>(
    out: W,
    notes_dir: &Path,
    chats_dir: &Path,
    parts: Parts,
) -> Result<Packed> {
    write_zip_with_limits(out, notes_dir, chats_dir, parts, LIMITS)
}

fn write_zip_with_limits<W: Write + Seek>(
    out: W,
    notes_dir: &Path,
    chats_dir: &Path,
    parts: Parts,
    limits: Limits,
) -> Result<Packed> {
    let mut zip = zip::ZipWriter::new(out);
    let mut packed = Packed::default();
    let mut budget = Budget::new(limits);
    budget.entry()?;
    zip.start_file("leo/README.txt", SimpleFileOptions::default())?;
    BudgetWriter {
        out: &mut zip,
        budget: &mut budget,
    }
    .write_all(README.as_bytes())?;
    let data = crate::storage::data_dir(notes_dir);
    add_dir(
        &mut zip,
        &data,
        notes_dir,
        "leo/notes",
        true,
        0,
        &mut budget,
        &mut packed,
    )?;
    #[derive(Serialize)]
    struct TaskExport<'a> {
        version: u8,
        tasks: &'a [crate::tasks::Task],
    }
    let tasks = crate::tasks::load_all(chats_dir);
    if !tasks.is_empty() {
        add_json(
            &mut zip,
            "leo/tasks.json",
            &TaskExport {
                version: 1,
                tasks: &tasks,
            },
            &mut budget,
            &mut packed,
        )?;
    }
    if parts.trash {
        add_dir(
            &mut zip,
            &data,
            &notes_dir.join(".trash"),
            "leo/trash",
            false,
            0,
            &mut budget,
            &mut packed,
        )?;
    }
    if parts.uploads {
        let uploads = data.join("attachments");
        add_dir(
            &mut zip,
            &data,
            &uploads,
            "leo/uploads",
            true,
            0,
            &mut budget,
            &mut packed,
        )?;
        add_dir(
            &mut zip,
            &data,
            &leo_core::recording::root(notes_dir),
            "leo/recording-sources",
            true,
            0,
            &mut budget,
            &mut packed,
        )?;
    }
    if parts.chats {
        for summary in
            crate::interview::list(chats_dir).map_err(|error| anyhow::anyhow!(error.message()))?
        {
            let Some(id) = summary["id"].as_str() else {
                continue;
            };
            ensure!(
                crate::chats::valid_id(id),
                "A saved practice has an invalid id; remove it in Storage and try again"
            );
            let item = crate::interview::load(chats_dir, id)
                .map_err(|error| anyhow::anyhow!(error.message()))?;
            add_json(
                &mut zip,
                &format!("leo/interviews/{id}.json"),
                &item,
                &mut budget,
                &mut packed,
            )?;
        }
        for id in chat_ids(chats_dir, &budget)? {
            let Some(chat) = crate::chats::load(chats_dir, &id) else {
                continue;
            };
            add_json(
                &mut zip,
                &format!("leo/chats/{}.json", chat.id),
                &chat,
                &mut budget,
                &mut packed,
            )?;
            for doc in documents(chats_dir, &chat.id, &budget)? {
                ensure!(
                    doc.len() == 32 && doc.chars().all(|c| c.is_ascii_hexdigit()),
                    "A saved document has an invalid id; remove it in Storage and try again"
                );
                let body: String = crate::chats::db_of(chats_dir).with(|connection| {
                    connection.query_row(
                        "SELECT text FROM chat_docs WHERE chat = ?1 AND id = ?2",
                        rusqlite::params![chat.id, doc],
                        |row| row.get(0),
                    )
                })?;
                budget.entry()?;
                zip.start_file(
                    format!("leo/chats/{}.files/{doc}.txt", chat.id),
                    SimpleFileOptions::default(),
                )?;
                BudgetWriter {
                    out: &mut zip,
                    budget: &mut budget,
                }
                .write_all(body.as_bytes())?;
                packed.files += 1;
                packed.bytes += body.len() as u64;
            }
        }
    }
    zip.finish()?;
    Ok(packed)
}

pub fn write_folder<W: Write + Seek>(out: W, folder: &Path, under: &str) -> Result<Packed> {
    write_folder_from(out, folder.parent().unwrap_or(folder), folder, under)
}

pub(crate) fn write_folder_from<W: Write + Seek>(
    out: W,
    root: &Path,
    folder: &Path,
    under: &str,
) -> Result<Packed> {
    let mut zip = zip::ZipWriter::new(out);
    let mut packed = Packed::default();
    add_dir(
        &mut zip,
        root,
        folder,
        under,
        true,
        0,
        &mut Budget::new(LIMITS),
        &mut packed,
    )?;
    zip.finish()?;
    Ok(packed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(bytes: Vec<u8>) -> Vec<String> {
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect()
    }

    fn folder_with_limits(folder: &Path, limits: Limits) -> Result<Packed> {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let mut packed = Packed::default();
        add_dir(
            &mut zip,
            folder,
            folder,
            "files",
            true,
            0,
            &mut Budget::new(limits),
            &mut packed,
        )?;
        zip.finish()?;
        Ok(packed)
    }

    #[test]
    fn export_limits_bound_depth_entries_and_total_file_bytes() {
        let deep = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(deep.path().join("a/b/c")).unwrap();
        std::fs::write(deep.path().join("a/b/c/example.md"), "example").unwrap();
        assert!(
            folder_with_limits(deep.path(), Limits { depth: 1, ..LIMITS })
                .unwrap_err()
                .to_string()
                .contains("too large")
        );
        let wide = tempfile::tempdir().unwrap();
        for name in [".a", ".b", ".c"] {
            std::fs::write(wide.path().join(name), "").unwrap();
        }
        assert!(folder_with_limits(
            wide.path(),
            Limits {
                entries: 2,
                ..LIMITS
            }
        )
        .is_err());
        let bytes = tempfile::tempdir().unwrap();
        for name in ["a", "b"] {
            std::fs::write(bytes.path().join(name), "ab").unwrap();
        }
        assert!(folder_with_limits(bytes.path(), Limits { bytes: 3, ..LIMITS }).is_err());
        assert_eq!(
            folder_with_limits(bytes.path(), Limits { bytes: 4, ..LIMITS }).unwrap(),
            Packed { files: 2, bytes: 4 }
        );
    }

    #[test]
    fn serialized_tasks_practices_chats_and_documents_share_the_export_budget() {
        for kind in ["tasks", "practice", "chat", "document"] {
            let home = tempfile::tempdir().unwrap();
            let notes = home.path().join("notes");
            let chats = home.path().join("chats");
            std::fs::create_dir_all(&notes).unwrap();
            let now = chrono::Utc::now();
            match kind {
                "tasks" => {
                    let task = serde_json::from_value(serde_json::json!({"id":"texport-1234", "title":"Example task", "created_at":now, "updated_at":now})).unwrap();
                    crate::tasks::put(&chats, &task).unwrap();
                }
                "practice" => {
                    let framework = serde_json::from_value(serde_json::json!({"title":"Interview practice","track":"coding","company":"","role":"Engineer","language":"Python","style":"coach","date":"","weekly_minutes":120,"skills":["Approach"],"wants":"Hints first"})).unwrap();
                    crate::interview::configure(&chats, "interview-export-01", 0, framework)
                        .unwrap();
                }
                _ => {
                    crate::chats::save(&chats, "chat-export-1234", serde_json::from_value(serde_json::json!({"mode":"chat", "messages":[{"role":"user", "text":"Example question"}]})).unwrap(), now).unwrap();
                    if kind == "document" {
                        crate::chat_files::add(
                            &chats,
                            "chat-export-1234",
                            "example.txt",
                            "Example document",
                            now,
                        )
                        .unwrap();
                    }
                }
            }
            let parts = Parts {
                chats: true,
                ..Parts::default()
            };
            let packed =
                write_zip(std::io::Cursor::new(Vec::new()), &notes, &chats, parts).unwrap();
            assert!(packed.files > 0, "{kind}");
            let limits = Limits {
                bytes: README.len() as u64 + packed.bytes - 1,
                ..LIMITS
            };
            assert!(
                write_zip_with_limits(
                    std::io::Cursor::new(Vec::new()),
                    &notes,
                    &chats,
                    parts,
                    limits
                )
                .is_err(),
                "{kind}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn export_roots_refuse_symlinks_and_nested_links_are_omitted() {
        for root in ["notes", "notes/.trash", "attachments", "recording-sources"] {
            let home = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            std::fs::write(
                outside.path().join("example.txt"),
                "EXAMPLE PRIVATE CONTENT",
            )
            .unwrap();
            if root != "notes" {
                std::fs::create_dir(home.path().join("notes")).unwrap();
            }
            std::os::unix::fs::symlink(outside.path(), home.path().join(root)).unwrap();
            let result = write_zip(
                std::io::Cursor::new(Vec::new()),
                &home.path().join("notes"),
                &home.path().join("chats"),
                Parts {
                    uploads: true,
                    trash: true,
                    chats: false,
                },
            );
            assert!(result.is_err(), "{root}");
        }
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(
            outside.path().join("example.txt"),
            "EXAMPLE PRIVATE CONTENT",
        )
        .unwrap();
        std::os::unix::fs::symlink(outside.path(), home.path().join("linked-folder")).unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("example.txt"),
            home.path().join("linked-file"),
        )
        .unwrap();
        let mut out = std::io::Cursor::new(Vec::new());
        let packed = write_folder(&mut out, home.path(), "files").unwrap();
        assert_eq!(packed, Packed::default());
        assert!(names(out.into_inner()).is_empty());
        assert!(write_folder_from(
            std::io::Cursor::new(Vec::new()),
            home.path(),
            &home.path().join("linked-folder/nested"),
            "files"
        )
        .is_err());
    }

    #[test]
    fn the_zip_holds_the_notes_in_their_folders_and_only_the_parts_asked_for() {
        let tmp = tempfile::tempdir().unwrap();
        let notes = tmp.path().join("notes");
        std::fs::create_dir_all(notes.join("cs130")).unwrap();
        std::fs::create_dir_all(notes.join(".git")).unwrap();
        std::fs::create_dir_all(notes.join(".trash")).unwrap();
        std::fs::write(notes.join("Top.md"), "top").unwrap();
        std::fs::write(notes.join("cs130/Graphs.md"), "graphs").unwrap();
        std::fs::write(notes.join(".git/config"), "secret-ish").unwrap();
        std::fs::write(notes.join(".trash/Old.md"), "old").unwrap();
        std::fs::create_dir_all(tmp.path().join("attachments/abc")).unwrap();
        std::fs::write(tmp.path().join("attachments/abc/slides.pdf"), "pdf").unwrap();
        let chats = tmp.path().join("chats");
        crate::chats::save(
            &chats,
            "chat-1234",
            serde_json::from_value(serde_json::json!({ "mode": "chat", "messages": [{ "role": "user", "text": "hi" }] })).unwrap(),
            chrono::Utc::now(),
        )
        .unwrap();
        crate::chat_files::add(&chats, "chat-1234", "w.pdf", "week 3", chrono::Utc::now()).unwrap();

        let framework = serde_json::from_value(serde_json::json!({"title":"Interview practice","track":"coding","company":"","role":"Engineer","language":"Python","style":"coach","date":"","weekly_minutes":120,"skills":["Approach"],"wants":"Hints first"})).unwrap();
        crate::interview::configure(&chats, "interview-export-01", 0, framework).unwrap();

        let mut only_notes = std::io::Cursor::new(Vec::new());
        let packed = write_zip(&mut only_notes, &notes, &chats, Parts::default()).unwrap();
        assert_eq!(packed, Packed { files: 2, bytes: 9 });
        assert_eq!(
            names(only_notes.into_inner()),
            [
                "leo/README.txt",
                "leo/notes/Top.md",
                "leo/notes/cs130/Graphs.md"
            ]
        );

        let mut all = std::io::Cursor::new(Vec::new());
        let parts = Parts {
            uploads: true,
            chats: true,
            trash: true,
        };
        write_zip(&mut all, &notes, &chats, parts).unwrap();
        let listed = names(all.into_inner());
        for want in [
            "leo/trash/Old.md",
            "leo/uploads/abc/slides.pdf",
            "leo/chats/chat-1234.json",
            "leo/interviews/interview-export-01.json",
        ] {
            assert!(listed.iter().any(|n| n == want), "{want} in {listed:?}");
        }
        assert!(!listed.iter().any(|n| n.contains(".git")), "{listed:?}");
        assert!(
            listed
                .iter()
                .any(|n| n.starts_with("leo/chats/chat-1234.files/") && n.ends_with(".txt")),
            "{listed:?}"
        );
    }

    #[test]
    fn a_notes_uploads_are_zipped_together_in_one_folder() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("slides.pdf"), "pdf").unwrap();
        std::fs::write(tmp.path().join("board.jpg"), "jpg").unwrap();
        let mut out = std::io::Cursor::new(Vec::new());
        let packed = write_folder(&mut out, tmp.path(), "Lecture 4").unwrap();
        assert_eq!(packed.files, 2);
        assert_eq!(
            names(out.into_inner()),
            ["Lecture 4/board.jpg", "Lecture 4/slides.pdf"]
        );
    }
}
