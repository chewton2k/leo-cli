use chrono::{DateTime, Utc};
use colored::Colorize;
use serde::{Deserialize, Serialize};

/// Wrap URLs in a string with OSC 8 terminal hyperlink escape sequences.
/// Most modern terminals (iTerm2, Terminal.app, Windows Terminal, etc.)
/// render these as clickable links.
fn linkify(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(start) = rest.find("http://").or_else(|| rest.find("https://")) {
        result.push_str(&rest[..start]);

        let url_part = &rest[start..];
        // URL ends at whitespace or common trailing punctuation
        let end = url_part
            .find(|c: char| c.is_whitespace() || "<>\"'`|{}[]()".contains(c))
            .unwrap_or(url_part.len());

        // Trim trailing punctuation that's likely not part of the URL
        let mut url = &url_part[..end];
        while url.ends_with(|c: char| ".,;:!?)".contains(c)) {
            url = &url[..url.len() - 1];
        }

        // OSC 8 hyperlink: \x1b]8;;URL\x1b\\TEXT\x1b]8;;\x1b\\
        result.push_str(&format!(
            "\x1b]8;;{url}\x1b\\{styled}\x1b]8;;\x1b\\",
            url = url,
            styled = url.underline().cyan(),
        ));

        // Push any trimmed trailing chars back
        let consumed = url.len();
        rest = &url_part[consumed..];
    }

    result.push_str(rest);
    result
}

/// A single note.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    /// Unique identifier (UUID v4)
    pub id: String,

    /// Short title shown in list view
    pub title: String,

    /// Full Markdown body
    pub body: String,

    /// Creation timestamp (UTC)
    pub created_at: DateTime<Utc>,

    /// Last-modified timestamp (UTC)
    pub updated_at: DateTime<Utc>,

    /// Free-form tags for organisation
    pub tags: Vec<String>,

    /// Directory path (empty string = root)
    #[serde(default)]
    pub directory: String,

    #[serde(default)]
    pub pinned: bool,

    #[serde(skip)]
    pub extra: serde_yaml::Mapping,
}

impl Note {
    /// Create a new note with generated id and current timestamps.
    pub fn new(
        title: impl Into<String>,
        body: impl Into<String>,
        tags: Vec<String>,
        directory: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Note {
            id: uuid::Uuid::new_v4().to_string(),
            title: title.into(),
            body: body.into(),
            created_at: now,
            updated_at: now,
            tags,
            directory: directory.into(),
            pinned: false,
            extra: serde_yaml::Mapping::new(),
        }
    }

    /// One-line summary as a formatted String.
    pub fn format_summary(&self) -> String {
        let date = self.updated_at.format("%Y-%m-%d %H:%M");
        let id_short: String = self.id.chars().take(8).collect();
        let tags = if self.tags.is_empty() {
            String::new()
        } else {
            format!("  [{}]", self.tags.join(", ").dimmed())
        };
        format!(
            "{} {} {}{}",
            id_short.dimmed(),
            date.to_string().cyan(),
            self.title.bold(),
            tags,
        )
    }

    /// Full view including body with rendered checkboxes and lists.
    pub fn print_full(&self) {
        let created = self.created_at.format("%Y-%m-%d %H:%M UTC");
        let updated = self.updated_at.format("%Y-%m-%d %H:%M UTC");

        println!("{}", "─".repeat(60).dimmed());
        println!("{} {}", "Title:".bold(), self.title);
        println!("{} {}", "ID:   ".bold(), self.id.dimmed());
        if !self.directory.is_empty() {
            println!("{} {}", "Dir:  ".bold(), self.directory.cyan());
        }
        println!("{} {}", "Tags: ".bold(), self.tags.join(", "));
        println!("{} {}", "Created:".bold(), created.to_string().dimmed());
        println!("{} {}", "Updated:".bold(), updated.to_string().dimmed());
        println!("{}", "─".repeat(60).dimmed());
        println!();
        println!("{}", self.render_body());
        println!();
    }

    /// Render the body with pretty checkboxes and bullet lists.
    /// Checkboxes are numbered so they can be toggled with `check`.
    pub fn render_body(&self) -> String {
        let mut checkbox_num = 0u32;
        self.body
            .lines()
            .map(|line| {
                let trimmed = line.trim_start();
                if let Some(b) = checkbox_line(line) {
                    checkbox_num += 1;
                    if b.ticked {
                        format!(
                            "  {} {} {}",
                            format!("[{checkbox_num}]").dimmed(),
                            "☑".green(),
                            linkify(b.text).dimmed(),
                        )
                    } else {
                        format!(
                            "  {} {} {}",
                            format!("[{checkbox_num}]").dimmed(),
                            "☐".white(),
                            linkify(b.text),
                        )
                    }
                } else if let Some(rest) = trimmed.strip_prefix("- ") {
                    format!("  • {}", linkify(rest))
                } else {
                    linkify(line)
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Toggle the Nth checkbox (1-based). Returns the new state text, or None
    /// if no such checkbox exists.
    /// The first body line holding any word of a search query, trimmed. Words
    /// starting with `#` are tags, not text, so they never match here.
    pub fn matching_line(&self, query: &str) -> Option<String> {
        let words = search_words(query);
        if words.is_empty() {
            return None;
        }
        self.body
            .lines()
            .find(|line| {
                let line = line.to_lowercase();
                words.iter().any(|w| line.contains(w.as_str()))
            })
            .map(|line| line.trim().to_string())
    }

    /// Each checkbox in the body, in order: true when ticked. The same lines
    /// [`Note::toggle_checkbox`] counts, so an index here is an index there.
    pub fn checkboxes(&self) -> Vec<bool> {
        self.body
            .lines()
            .filter_map(|l| checkbox_line(l).map(|b| b.ticked))
            .collect()
    }

    pub fn toggle_checkbox(&mut self, n: usize) -> Option<String> {
        let mut seen = 0usize;
        let mut out = String::with_capacity(self.body.len());
        let mut result = None;
        for piece in self.body.split_inclusive('\n') {
            let line = piece.trim_end_matches(['\n', '\r']);
            let found = result.is_none();
            let hit = checkbox_line(line).filter(|_| {
                seen += 1;
                found && seen == n
            });
            match hit {
                Some(b) => {
                    let flipped = if b.ticked { ' ' } else { 'x' };
                    out.push_str(&line[..b.state]);
                    out.push(flipped);
                    out.push_str(&piece[b.state + 1..]);
                    result = Some(if b.ticked {
                        format!("{} {}", "☐".white(), b.text)
                    } else {
                        format!("{} {}", "☑".green(), b.text)
                    });
                }
                None => out.push_str(piece),
            }
        }
        if result.is_some() {
            self.body = out;
            self.updated_at = Utc::now();
        }
        result
    }
}

pub struct Checkbox<'a> {
    pub ticked: bool,
    pub state: usize,
    pub text: &'a str,
}

pub fn checkbox_line(line: &str) -> Option<Checkbox<'_>> {
    let trimmed = line.trim_start();
    let indent = line.len() - trimmed.len();
    let marker = match trimmed.chars().next()? {
        '-' | '*' | '+' => 1,
        c if c.is_ascii_digit() => {
            let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
            match trimmed[digits..].chars().next() {
                Some('.' | ')') => digits + 1,
                _ => return None,
            }
        }
        _ => return None,
    };
    let rest = trimmed[marker..].strip_prefix(" [")?;
    let mut chars = rest.chars();
    let ticked = match chars.next()? {
        ' ' => false,
        'x' | 'X' => true,
        _ => return None,
    };
    let after = chars.as_str().strip_prefix(']')?;
    let text = if after.is_empty() {
        ""
    } else {
        after.strip_prefix(' ')?
    };
    Some(Checkbox {
        ticked,
        state: indent + marker + 2,
        text,
    })
}

/// The plain words of a search query, lowercased: what can match text. Words
/// starting with `#` name tags and are left out.
pub fn search_words(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .filter(|w| !w.starts_with('#'))
        .map(str::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_list_style_of_checkbox_counts_and_toggles() {
        let mut note = Note::new(
            "T",
            "- [ ] dash\n* [ ] star\n+ [x] plus\n1. [ ] one\n2) [X] two\n  - [ ]\n-[ ] not\n- [y] not\ncode [ ] not\n",
            vec![],
            "",
        );
        assert_eq!(
            note.checkboxes(),
            vec![false, false, true, false, true, false]
        );
        for n in 1..=6 {
            assert!(note.toggle_checkbox(n).is_some(), "box {n}");
        }
        assert_eq!(
            note.body,
            "- [x] dash\n* [x] star\n+ [ ] plus\n1. [x] one\n2) [ ] two\n  - [x]\n-[ ] not\n- [y] not\ncode [ ] not\n"
        );
        assert!(note.toggle_checkbox(7).is_none());
    }

    /// The line a search matched inside the note, so results can show where.
    #[test]
    fn matching_line_finds_the_first_body_line_with_a_search_word() {
        let note = Note::new(
            "Graphs",
            "intro\n\n  BFS explores level by level\nDFS goes deep",
            vec![],
            "",
        );
        assert_eq!(
            note.matching_line("bfs").as_deref(),
            Some("BFS explores level by level")
        );
        // Any word of the query will do, and #tags are not body text.
        assert_eq!(
            note.matching_line("#exam deep").as_deref(),
            Some("DFS goes deep")
        );
        assert_eq!(note.matching_line("#exam"), None);
        assert_eq!(note.matching_line("nowhere"), None);
        assert_eq!(note.matching_line(""), None);
    }

    #[test]
    fn checkboxes_lists_each_box_in_order() {
        let note = Note::new(
            "T",
            "- [ ] a\ntext\n  - [x] b\n- [X] c\n- [ ]\n",
            vec![],
            "",
        );
        assert_eq!(note.checkboxes(), vec![false, true, true, false]);
    }

    #[test]
    fn a_summary_of_a_note_with_a_short_or_wide_id_does_not_panic() {
        for id in ["abc", "1", "", "세계세계세계세계"] {
            let mut note = Note::new("Title", "body", vec![], "");
            note.id = id.to_string();
            let summary = note.format_summary();
            assert!(summary.contains("Title"), "{id:?}: {summary}");
        }
    }
}
