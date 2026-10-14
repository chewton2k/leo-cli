use std::path::Path;

use leo_core::notes::Note;

pub const MOST_CHARS: usize = 400_000;

fn part(note: &Note, body: &str) -> String {
    let title = note.title.trim();
    let title = if title.is_empty() { "Untitled" } else { title };
    let body = body.trim();
    if body.is_empty() {
        format!("## {title}\n")
    } else {
        format!("## {title}\n\n{body}\n")
    }
}

pub fn joined(notes_dir: &Path, into: &Note, from: &Note) -> String {
    let from_body = moved_pictures(notes_dir, &from.directory, &into.directory, &from.body);
    let mut parts = [(into, into.body.clone()), (from, from_body)];
    parts.sort_by_key(|(note, _)| note.created_at);
    format!(
        "{}\n---\n\n{}",
        part(parts[0].0, &parts[0].1),
        part(parts[1].0, &parts[1].1)
    )
}

fn moved_pictures(notes_dir: &Path, from_dir: &str, into_dir: &str, body: &str) -> String {
    if from_dir == into_dir {
        return body.to_string();
    }
    body.lines()
        .map(|line| {
            let mut out = line.to_string();
            for (_, shown) in leo_core::attachments::pictures_in(line).into_iter().rev() {
                if leo_core::attachments::resolve(notes_dir, into_dir, &shown.target).is_some() {
                    continue;
                }
                let Some(path) = leo_core::attachments::resolve(notes_dir, from_dir, &shown.target)
                else {
                    continue;
                };
                let Ok(top) = path.strip_prefix(notes_dir) else {
                    continue;
                };
                let top = top.to_string_lossy().replace('\\', "/");
                out = out.replace(&format!("]({})", shown.target), &format!("]({top})"));
            }
            out
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(title: &str, dir: &str, body: &str, minutes_ago: i64) -> Note {
        let mut store =
            leo_core::store::Store::load_from(&tempfile::tempdir().unwrap().keep()).unwrap();
        let mut n = store.create_note(title, body, vec![], "").unwrap().clone();
        n.directory = dir.into();
        n.created_at = chrono::Utc::now() - chrono::Duration::minutes(minutes_ago);
        n
    }

    #[test]
    fn both_notes_are_kept_whole_under_their_titles_oldest_first() {
        let older = note("BFS", "", "BFS uses a queue.\n\n- [x] read chapter 3", 60);
        let newer = note("Graphs", "", "  A graph has nodes and edges.  ", 5);
        let body = joined(Path::new("/nowhere"), &newer, &older);
        assert_eq!(
            body,
            "## BFS\n\nBFS uses a queue.\n\n- [x] read chapter 3\n\n---\n\n## Graphs\n\nA graph has nodes and edges.\n"
        );
        assert_eq!(
            joined(Path::new("/nowhere"), &older, &newer),
            body,
            "the drop direction does not change the order"
        );
        let empty = note("", "", "", 1);
        assert!(joined(Path::new("/nowhere"), &older, &empty).ends_with("## Untitled\n"));
    }

    #[test]
    fn a_picture_beside_the_other_note_still_shows_from_the_combined_one() {
        let dir = tempfile::tempdir().unwrap();
        let notes = dir.path();
        std::fs::create_dir_all(notes.join("cs130")).unwrap();
        std::fs::write(notes.join("cs130/tree.png"), b"png").unwrap();
        let body = "Look: ![Tree](tree.png)";
        assert_eq!(
            moved_pictures(notes, "cs130", "", body),
            "Look: ![Tree](cs130/tree.png)"
        );
        assert_eq!(moved_pictures(notes, "cs130", "cs130", body), body);
    }
}
