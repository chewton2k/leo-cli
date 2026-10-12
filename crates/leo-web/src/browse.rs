use std::collections::HashMap;

use axum::http::StatusCode;
use leo_core::notes::Note;

const MOST_RECENT: usize = 100;
const MOST_RECENT_BYTES: usize = MOST_RECENT * 65;
const MOST_SCAN: usize = 32_000;
const MOST_PREVIEW: usize = 1_600;
const MOST_SUMMARY: usize = 220;
const MOST_LINES: usize = 800;

pub(crate) fn order(
    notes: &mut [&Note],
    sort: Option<&str>,
    recent: Option<&str>,
) -> Result<(), StatusCode> {
    let sort = sort.unwrap_or("updated");
    if !matches!(sort, "updated" | "opened" | "folder" | "title") {
        return Err(StatusCode::BAD_REQUEST);
    }
    let recent = recent.unwrap_or_default();
    if recent.len() > MOST_RECENT_BYTES {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut opened = HashMap::new();
    for (i, id) in recent.split(',').filter(|id| !id.is_empty()).enumerate() {
        if i >= MOST_RECENT || !crate::chats::valid_id(id) {
            return Err(StatusCode::BAD_REQUEST);
        }
        opened.entry(id).or_insert(i);
    }
    notes.sort_by_cached_key(|note| {
        let rank = if sort == "opened" {
            *opened.get(note.id.as_str()).unwrap_or(&usize::MAX)
        } else {
            0
        };
        (
            !note.pinned,
            rank,
            if sort == "folder" {
                note.directory.to_lowercase()
            } else {
                String::new()
            },
            if matches!(sort, "title" | "folder") {
                note.title.to_lowercase()
            } else {
                String::new()
            },
            std::cmp::Reverse(note.updated_at),
            note.id.clone(),
        )
    });
    Ok(())
}

pub(crate) fn preview(body: &str) -> String {
    let scan: String = body.chars().take(MOST_SCAN).collect();
    let lines: Vec<&str> = scan.lines().take(MOST_LINES).collect();
    let mut fence: Option<(usize, char, usize)> = None;
    let mut visual = None;
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if let Some((start, mark, width)) = fence {
            if trimmed.chars().take_while(|c| *c == mark).count() >= width {
                let part = lines[start..=i].join("\n");
                if part.chars().count() <= MOST_PREVIEW && visual == Some(start) {
                    return part;
                }
                fence = None;
                visual = None;
            }
            continue;
        }
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            let mark = trimmed.chars().next().unwrap_or('`');
            let width = trimmed.chars().take_while(|c| *c == mark).count();
            let kind = trimmed.trim_start_matches(mark).trim();
            fence = Some((i, mark, width));
            if matches!(kind, "mermaid" | "svg") {
                visual = Some(i);
            }
        } else if (trimmed.starts_with("![") || trimmed.starts_with("![["))
            && trimmed.chars().count() <= MOST_PREVIEW
        {
            return trimmed.to_string();
        } else if trimmed == "$$" {
            if let Some(end) = lines
                .iter()
                .skip(i + 1)
                .position(|line| line.trim() == "$$")
            {
                let part = lines[i..=i + end + 1].join("\n");
                if part.chars().count() <= MOST_PREVIEW {
                    return part;
                }
            }
        }
    }
    scan.chars().take(MOST_PREVIEW).collect()
}

pub(crate) fn summary(body: &str) -> String {
    let scan: String = body.chars().take(MOST_SCAN).collect();
    let mut fenced = false;
    let mut headings = Vec::new();
    for line in scan.lines().take(MOST_LINES) {
        let line = line.trim();
        if line.starts_with("```") || line.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced || line.is_empty() {
            continue;
        }
        if line.starts_with('#') {
            let heading = line.trim_start_matches('#').trim();
            if headings.len() < 4 && !heading.is_empty() {
                headings.push(heading);
            }
            continue;
        }
        if line.starts_with(['>', '|', '$', '!', '<']) || line.starts_with("From ") {
            continue;
        }
        let text = line.trim_start_matches(['-', '*', '+', ' ']);
        if text.starts_with('[') || text.chars().count() < 20 {
            continue;
        }
        let sentence = text
            .split_inclusive(['.', '。', '!', '?'])
            .next()
            .unwrap_or(text);
        return sentence.chars().take(MOST_SUMMARY).collect();
    }
    headings.join(" · ").chars().take(MOST_SUMMARY).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_visual_preview_is_complete_bounded_and_unicode_safe() {
        let body =
            "# Lecture\nAn opening paragraph.\n```mermaid\ngraph LR\n A --> B\n```\nMore text";
        assert_eq!(preview(body), "```mermaid\ngraph LR\n A --> B\n```");
        assert_eq!(
            preview("# Photos\n![A figure](picture.png)"),
            "![A figure](picture.png)"
        );
        assert_eq!(preview("# Math\n$$\nx^2\n$$"), "$$\nx^2\n$$");
        assert_eq!(preview(&"é🦊".repeat(2000)).chars().count(), MOST_PREVIEW);
        let oversized = format!("```svg\n{}\n```\n![Small](small.png)", "x".repeat(2000));
        assert_eq!(preview(&oversized), "![Small](small.png)");
    }

    #[test]
    fn summaries_skip_drawing_code_and_tasks_then_use_content_or_headings() {
        assert_eq!(
            summary(
                "# Lecture\n```svg\n<svg>hidden internals</svg>\n```\n- [ ] Submit homework tomorrow\nBreadth first search visits each layer. Another sentence."
            ),
            "Breadth first search visits each layer."
        );
        assert_eq!(
            summary("# Queues\n## Breadth first search\n- [ ] Finish"),
            "Queues · Breadth first search"
        );
        assert!(summary(&"🦊".repeat(1000)).chars().count() <= MOST_SUMMARY);
    }
}
