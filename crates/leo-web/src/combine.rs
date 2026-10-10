use std::collections::HashSet;
use std::path::Path;

use leo_core::notes::Note;

pub const MOST_CHARS: usize = 120_000;
pub const KEPT_ENOUGH: f32 = 0.9;
const LEAST_TOKENS: u32 = 4_000;
const MOST_TOKENS: u32 = 32_000;
const ALSO: &str = "## Also in the original notes";

pub const SYSTEM: &str = "You combine two of the user's notes into one note. Use interpretable language.

Rules:
- Keep every piece of information from both notes: every fact, definition, explanation, example, step, number, date, name, formula, table, code block, link, picture, quote, question and checkbox (with its ticked or unticked state). Nothing unique may be lost.
- When both notes say the same thing, say it once, keeping the clearer wording and every detail that either version adds.
- Organise the result under clear headings in a sensible order, joining sections that cover the same topic.
- Do not summarise, shorten, add new facts, add opinions or comment on the merge.
- Copy Markdown, LaTeX math, Mermaid blocks, callouts, [[links]], web links and picture links exactly as they are written.
- Reply with the body of the combined note only: no title line, no preamble, no remarks after it, and do not wrap it in a code block.";

pub fn prompt(into: &Note, from: &Note) -> String {
    format!(
        "<note title=\"{}\">\n{}\n</note>\n\n<note title=\"{}\">\n{}\n</note>\n\nCombine these two notes into one. Keep every piece of information from both; say what repeats only once. Reply with the combined note's body only.",
        attribute(&into.title),
        into.body.trim(),
        attribute(&from.title),
        from.body.trim()
    )
}

pub fn reply_tokens(into: &Note, from: &Note) -> u32 {
    let chars = into.body.chars().count() + from.body.chars().count();
    ((chars / 3) as u32 * 13 / 10).clamp(LEAST_TOKENS, MOST_TOKENS)
}

fn attribute(text: &str) -> String {
    text.replace(['"', '\n'], " ")
}

pub struct Combined {
    pub body: String,
    pub added: Vec<String>,
    pub kept: f32,
}

pub fn settle(notes_dir: &Path, into: &Note, from: &Note, reply: &str) -> Combined {
    let body = moved_pictures(
        notes_dir,
        &from.directory,
        &into.directory,
        &unwrapped(reply),
    );
    let sources = [into.body.as_str(), from.body.as_str()];
    let added = missing(&sources, &body);
    let body = if added.is_empty() {
        body
    } else {
        format!("{}\n\n{ALSO}\n\n{}\n", body.trim_end(), added.join("\n\n"))
    };
    let kept = kept_words(&sources, &body);
    Combined { body, added, kept }
}

fn unwrapped(reply: &str) -> String {
    let text = reply.trim();
    let Some(rest) = text.strip_prefix("```") else {
        return text.to_string();
    };
    let Some(open) = rest.find('\n') else {
        return text.to_string();
    };
    let tag = rest[..open].trim();
    if !matches!(tag, "" | "markdown" | "md") {
        return text.to_string();
    }
    match rest[open + 1..].trim_end().strip_suffix("```") {
        Some(inner) if !tag.is_empty() || !inner.contains("\n```") => inner.trim().to_string(),
        _ => text.to_string(),
    }
}

fn flat(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn pieces(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let lines: Vec<&str> = body.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_start();
        let fence = ["```", "~~~"].into_iter().find(|f| trimmed.starts_with(f));
        if let Some(fence) = fence {
            let end = (i + 1..lines.len())
                .find(|&j| lines[j].trim_start().starts_with(fence))
                .unwrap_or(lines.len() - 1);
            out.push(lines[i..=end].join("\n"));
            i = end + 1;
            continue;
        }
        if trimmed.starts_with("$$") && !(trimmed.len() > 4 && trimmed.trim_end().ends_with("$$")) {
            let end = (i + 1..lines.len())
                .find(|&j| lines[j].trim_end().ends_with("$$"))
                .unwrap_or(lines.len() - 1);
            out.push(lines[i..=end].join("\n"));
            i = end + 1;
            continue;
        }
        if leo_core::notes::checkbox_line(line).is_some() {
            out.push(line.trim().to_string());
        }
        for (range, _) in leo_core::attachments::pictures_in(line) {
            out.push(line[range].to_string());
        }
        out.extend(wiki_links(line));
        out.extend(web_links(line));
        if trimmed.starts_with("$$") {
            out.push(trimmed.trim_end().to_string());
        }
        i += 1;
    }
    let mut seen = HashSet::new();
    out.retain(|p| !p.trim().is_empty() && seen.insert(flat(p)));
    out
}

fn wiki_links(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(start) = line[from..].find("[[").map(|i| from + i) {
        let Some(end) = line[start + 2..].find("]]").map(|i| start + 2 + i + 2) else {
            break;
        };
        if !line[..start].ends_with('!') {
            out.push(line[start..end].to_string());
        }
        from = end;
    }
    out
}

fn web_links(line: &str) -> Vec<String> {
    line.split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | '<' | '>' | '"' | '\''))
        .filter(|w| w.starts_with("http://") || w.starts_with("https://"))
        .map(|w| {
            w.trim_end_matches(['.', ',', ';', ':', '!', '?', ']'])
                .to_string()
        })
        .collect()
}

pub fn missing(sources: &[&str], merged: &str) -> Vec<String> {
    let merged = flat(merged);
    let mut seen = HashSet::new();
    sources
        .iter()
        .flat_map(|s| pieces(s))
        .filter(|p| seen.insert(flat(p)))
        .filter(|p| !merged.contains(&flat(p)) && !checkbox_kept(p, &merged))
        .collect()
}

fn checkbox_kept(piece: &str, merged: &str) -> bool {
    let Some(rest) = piece
        .strip_prefix("- ")
        .or_else(|| piece.strip_prefix("* "))
        .or_else(|| piece.strip_prefix("+ "))
    else {
        return false;
    };
    ["- ", "* ", "+ "]
        .iter()
        .any(|bullet| merged.contains(&flat(&format!("{bullet}{rest}"))))
}

fn words(text: &str) -> HashSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 4)
        .map(str::to_string)
        .collect()
}

pub fn kept_words(sources: &[&str], merged: &str) -> f32 {
    let wanted: HashSet<String> = sources.iter().flat_map(|s| words(s)).collect();
    if wanted.is_empty() {
        return 1.0;
    }
    let have = words(merged);
    wanted.iter().filter(|w| have.contains(*w)).count() as f32 / wanted.len() as f32
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

    fn note(title: &str, dir: &str, body: &str) -> Note {
        let mut store =
            leo_core::store::Store::load_from(&tempfile::tempdir().unwrap().keep()).unwrap();
        let mut n = store.create_note(title, body, vec![], "").unwrap().clone();
        n.directory = dir.into();
        n
    }

    #[test]
    fn everything_that_must_survive_is_found_and_put_back_if_the_ai_drops_it() {
        let a = note(
            "BFS",
            "",
            "BFS uses a queue.\n\n```python\nq = deque([s])\n```\n\n- [ ] practise BFS\n- [x] read chapter 3\n\nSee [[Graph search]] and https://example.com/bfs.\n\n![Tree](tree.png)\n\n$$\nO(V + E)\n$$",
        );
        let b = note(
            "Graphs",
            "",
            "A graph has nodes and edges. BFS uses a queue.",
        );
        let dropped =
            "## Graphs\nA graph has nodes and edges. BFS uses a queue.\n\n- [ ] practise BFS";
        let combined = settle(Path::new("/nowhere"), &b, &a, dropped);
        assert_eq!(combined.added.len(), 6, "{:?}", combined.added);
        for piece in [
            "q = deque([s])",
            "- [x] read chapter 3",
            "[[Graph search]]",
            "https://example.com/bfs",
            "![Tree](tree.png)",
            "O(V + E)",
        ] {
            assert!(combined.body.contains(piece), "{piece}");
        }
        assert!(combined.body.contains(ALSO));
        assert!(missing(&[&a.body, &b.body], &combined.body).is_empty());

        let whole = format!("{}\n\nA graph has nodes and edges.", a.body);
        let kept = settle(
            Path::new("/nowhere"),
            &b,
            &a,
            &format!("```markdown\n{whole}\n```"),
        );
        assert!(kept.added.is_empty(), "{:?}", kept.added);
        assert!(!kept.body.starts_with("```"));
        assert!(kept.kept > 0.99);
    }

    #[test]
    fn a_checkbox_keeps_its_state_and_a_bullet_style_change_is_fine() {
        let a = "- [x] read chapter 3";
        assert!(missing(&[a], "* [x] read chapter 3").is_empty());
        assert_eq!(missing(&[a], "- [ ] read chapter 3"), [a]);
    }

    #[test]
    fn lost_wording_is_measured_so_the_preview_can_warn() {
        let a = "Dijkstra finds shortest paths with a priority queue and relaxes every edge once.";
        assert!(kept_words(&[a], a) > 0.99);
        assert!(kept_words(&[a], "Dijkstra finds paths.") < KEPT_ENOUGH);
    }

    #[test]
    fn the_prompt_holds_both_notes_and_asks_for_nothing_to_be_lost() {
        let a = note("BFS \"intro\"", "", "queue");
        let b = note("Graphs", "", "edges");
        let user = prompt(&b, &a);
        assert!(user.contains("<note title=\"Graphs\">\nedges\n</note>"));
        assert!(user.contains("<note title=\"BFS  intro \">\nqueue\n</note>"));
        assert!(SYSTEM.contains("Use interpretable language"));
        assert!(SYSTEM.contains("Nothing unique may be lost"));
        assert_eq!(reply_tokens(&a, &b), LEAST_TOKENS);
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
