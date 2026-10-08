use std::io::{Seek, Write};
use std::path::Path;

use anyhow::Result;
use serde::Deserialize;
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

notes/    your notes as Markdown files, in their folders. Obsidian and any
          editor open them; leo reads them back if you copy them into its
          notes folder.
uploads/  the files you uploaded, in a folder per note (named by the note's id,
          which is the id: line at the top of the note).
chats/    your conversations with Felix, one JSON file each.
trash/    notes in the trash, as Markdown.

Settings and API keys are never included.
";

fn add_dir<W: Write + Seek>(
    zip: &mut zip::ZipWriter<W>,
    from: &Path,
    under: &str,
    skip_hidden: bool,
    packed: &mut Packed,
) -> Result<()> {
    let Ok(entries) = std::fs::read_dir(from) else {
        return Ok(());
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        if skip_hidden && name.starts_with('.') {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let inside = format!("{under}/{name}");
        if kind.is_dir() {
            add_dir(zip, &entry.path(), &inside, skip_hidden, packed)?;
        } else if kind.is_file() {
            zip.start_file(&inside, options)?;
            let mut file = std::fs::File::open(entry.path())?;
            packed.bytes += std::io::copy(&mut file, zip)?;
            packed.files += 1;
        }
    }
    Ok(())
}

pub fn write_zip<W: Write + Seek>(
    out: W,
    notes_dir: &Path,
    chats_dir: &Path,
    parts: Parts,
) -> Result<Packed> {
    let mut zip = zip::ZipWriter::new(out);
    let mut packed = Packed::default();
    zip.start_file("leo/README.txt", SimpleFileOptions::default())?;
    zip.write_all(README.as_bytes())?;
    add_dir(&mut zip, notes_dir, "leo/notes", true, &mut packed)?;
    if parts.trash {
        add_dir(
            &mut zip,
            &notes_dir.join(".trash"),
            "leo/trash",
            false,
            &mut packed,
        )?;
    }
    if parts.uploads {
        let uploads = crate::storage::data_dir(notes_dir).join("attachments");
        add_dir(&mut zip, &uploads, "leo/uploads", true, &mut packed)?;
    }
    if parts.chats {
        add_dir(&mut zip, chats_dir, "leo/chats", true, &mut packed)?;
    }
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
        std::fs::create_dir_all(&chats).unwrap();
        std::fs::write(chats.join("chat-1234.json"), "{}").unwrap();

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
        ] {
            assert!(listed.iter().any(|n| n == want), "{want} in {listed:?}");
        }
        assert!(!listed.iter().any(|n| n.contains(".git")), "{listed:?}");
    }
}
