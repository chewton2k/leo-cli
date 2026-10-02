//! The preview pane: the selected note's body, or the transcript stream while
//! recording.

use ratatui::layout::Rect;
use ratatui::text::Line as TuiLine;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use ratatui::style::{Modifier, Style};
use ratatui::text::Span;

use super::line::border;
use leo_core::notes::Note;

/// What the pane is currently showing.
pub enum Preview<'a> {
    Empty,
    Note(&'a Note),
    /// A recording in progress: the points typed so far, the transcript as it
    /// grows, and — while recording — the point being typed.
    Live {
        paused: bool,
        points: Vec<String>,
        transcript: &'a str,
        jot: Option<&'a str>,
        scroll: &'a super::livescroll::LiveScroll,
    },
    /// Free text: a streaming answer.
    Text {
        title: String,
        body: String,
    },
    /// Handler output, keeping each line's `Kind` styling.
    Lines {
        title: String,
        lines: &'a [leo_core::action::Line],
    },
}

/// `cursor` is the checkbox the preview's cursor is on, drawn reversed and kept
/// in view. `search` is the active search: its words are highlighted, and while
/// the user has not scrolled, the first match is brought into view.
pub fn render(
    frame: &mut Frame,
    area: Rect,
    preview: &Preview<'_>,
    scroll: u16,
    focused: bool,
    search: Option<&str>,
) {
    if matches!(preview, Preview::Live { .. }) {
        render_live(frame, area, preview, focused);
        return;
    }
    if matches!(preview, Preview::Empty) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border(focused))
            .title("preview");
        let inner = block.inner(area);
        frame.render_widget(block, area);
        super::empty::render(frame, inner, &super::empty::Hint::no_selection());
        return;
    }

    let (title, mut lines): (String, Vec<TuiLine>) = match preview {
        Preview::Empty | Preview::Live { .. } => (String::new(), Vec::new()),
        // Markdown, so a note looks the way it was written rather than like a
        // text dump: headings in the accent, checkboxes as boxes, code receding.
        Preview::Note(n) => (
            format!(
                "{}  ·  {}",
                n.title,
                super::when::long(n.updated_at, chrono::Utc::now())
            ),
            super::markdown::render(&n.body),
        ),
        Preview::Text { title, body } => (title.clone(), super::markdown::render(body)),
        Preview::Lines { title, lines } => (
            title.clone(),
            lines.iter().map(super::line::to_tui).collect(),
        ),
    };
    let line_count = lines.len();
    let untouched = scroll == 0;
    let mut scroll = clamp_scroll(scroll, line_count, area.height);
    let visible = area.height.saturating_sub(2).max(1);

    let words = search
        .map(leo_core::notes::search_words)
        .unwrap_or_default();
    if !words.is_empty() {
        let mut first = None;
        for (i, line) in lines.iter_mut().enumerate() {
            let (marked, hit) = highlight(std::mem::take(line), &words);
            *line = marked;
            if hit && first.is_none() {
                first = Some(i as u16);
            }
        }
        // Two lines of context above the match, when the user has not scrolled.
        if let (true, Some(row)) = (untouched, first) {
            if row >= visible {
                scroll = row.saturating_sub(2);
            }
        }
    }

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border(focused))
                .title(title),
        )
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));

    frame.render_widget(paragraph, area);
}

/// The recording view. The transcript is wrapped here rather than by the
/// widget, so the newest words can be kept at the bottom of the pane as it
/// grows; the typing box sits under it.
fn render_live(frame: &mut Frame, area: Rect, preview: &Preview<'_>, focused: bool) {
    use ratatui::layout::{Constraint, Layout, Position};

    let Preview::Live {
        paused,
        points,
        transcript,
        jot,
        scroll,
    } = preview
    else {
        return;
    };
    let (paused, jot) = (*paused, *jot);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border(focused))
        .title(match (paused, scroll.is_following()) {
            (true, _) => "live transcript — paused (Ctrl-P resumes)",
            (false, true) => "live transcript",
            (false, false) => "live transcript — scrolled back (End returns, or it does in 10 s)",
        });
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let field_width = inner.width.saturating_sub(2) as usize;
    let jot_rows = jot.map(|j| typing_rows(j, field_width)).unwrap_or_default();
    let box_height = match jot {
        Some(_) => jot_rows.len().clamp(1, JOT_MOST_ROWS) as u16 + 2,
        None => 0,
    };
    let points_height = if points.is_empty() {
        0
    } else {
        (points.len() as u16 + 2).min(inner.height / 3)
    };
    let [points_area, text_area, box_area] = Layout::vertical([
        Constraint::Length(points_height),
        Constraint::Min(0),
        Constraint::Length(box_height),
    ])
    .areas(inner);

    if points_height > 0 {
        let accent = Style::default()
            .fg(super::theme::accent())
            .add_modifier(Modifier::BOLD);
        let mut lines = vec![TuiLine::from(Span::styled("Your points", accent))];
        for p in points
            .iter()
            .rev()
            .take((points_height as usize).saturating_sub(2))
            .rev()
        {
            lines.push(TuiLine::from(vec![
                Span::raw("• "),
                Span::styled(p.clone(), Style::default().add_modifier(Modifier::BOLD)),
            ]));
        }
        frame.render_widget(Paragraph::new(lines), points_area);
    }

    let text = if transcript.trim().is_empty() {
        vec![TuiLine::from(Span::styled(
            "recording...",
            Style::default().add_modifier(Modifier::DIM),
        ))]
    } else {
        let (shown, trimmed) = recent_words(transcript, LIVE_WORDS);
        let mut rows = wrap(shown, text_area.width as usize);
        if trimmed {
            rows.insert(0, EARLIER.to_string());
        }
        let top = scroll.visible_top(
            rows.len(),
            text_area.height as usize,
            std::time::Instant::now(),
        );
        rows.into_iter()
            .skip(top)
            .take(text_area.height as usize)
            .map(TuiLine::from)
            .collect()
    };
    frame.render_widget(Paragraph::new(text), text_area);

    if let Some(jot) = jot {
        let jot_box = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(super::theme::accent()))
            .title(" your point · Enter adds it · Ctrl-P pauses · Esc twice stops ");
        let field = jot_box.inner(box_area);
        frame.render_widget(jot_box, box_area);
        if jot.is_empty() {
            let hint = Span::styled(
                "type what matters — it is woven into the note",
                Style::default().add_modifier(Modifier::DIM),
            );
            frame.render_widget(Paragraph::new(TuiLine::from(hint)), field);
            frame.set_cursor_position(Position::new(field.x, field.y));
            return;
        }
        let visible = (field.height as usize).max(1);
        let skip = jot_rows.len().saturating_sub(visible);
        let shown: Vec<TuiLine> = jot_rows
            .iter()
            .skip(skip)
            .map(|row| TuiLine::from(row.clone()))
            .collect();
        let last = jot_rows.last().map(|r| r.chars().count()).unwrap_or(0) as u16;
        let row = (jot_rows.len() - skip).saturating_sub(1) as u16;
        frame.render_widget(Paragraph::new(shown), field);
        frame.set_cursor_position(Position::new(
            (field.x + last).min(field.x + field.width.saturating_sub(1)),
            field.y + row.min(field.height.saturating_sub(1)),
        ));
    }
}

const JOT_MOST_ROWS: usize = 6;

pub fn typing_rows(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut rows = vec![String::new()];
    let mut used = 0;
    for piece in text.split_inclusive(' ') {
        let word = piece.trim_end_matches(' ').chars().count();
        if used > 0 && used + word > width {
            rows.push(String::new());
            used = 0;
        }
        for c in piece.chars() {
            if c != ' ' && used >= width {
                rows.push(String::new());
                used = 0;
            }
            if let Some(row) = rows.last_mut() {
                row.push(c);
            }
            used += 1;
        }
    }
    rows
}

/// Greedy word wrap to `width` columns; a word longer than a row is split.
const LIVE_WORDS: usize = 20_000;
const EARLIER: &str = "… earlier parts are saved and will all be in the note";

fn recent_words(text: &str, most: usize) -> (&str, bool) {
    let mut words = 0;
    let mut in_word = false;
    for (i, c) in text.char_indices().rev() {
        if c.is_whitespace() {
            if in_word {
                words += 1;
                if words == most {
                    return (&text[i + c.len_utf8()..], true);
                }
            }
            in_word = false;
        } else {
            in_word = true;
        }
    }
    (text, false)
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut rows = Vec::new();
    let mut row = String::new();
    for word in text.split_whitespace() {
        let mut word: Vec<char> = word.chars().collect();
        while word.len() > width {
            if !row.is_empty() {
                rows.push(std::mem::take(&mut row));
            }
            rows.push(word.drain(..width).collect());
        }
        let word: String = word.into_iter().collect();
        if row.is_empty() {
            row = word;
        } else if row.chars().count() + 1 + word.chars().count() <= width {
            row.push(' ');
            row.push_str(&word);
        } else {
            rows.push(std::mem::replace(&mut row, word));
        }
    }
    if !row.is_empty() {
        rows.push(row);
    }
    rows
}

/// Split each span of `line` around case-insensitive matches of `words`,
/// reversing the matched text. Returns the line and whether anything matched.
fn highlight(line: TuiLine<'static>, words: &[String]) -> (TuiLine<'static>, bool) {
    let mut hit = false;
    let mut spans = Vec::new();
    for span in line.spans {
        let text = span.content.to_string();
        let lower = text.to_lowercase();
        // Byte offsets only line up when lowercasing kept every length.
        if lower.len() != text.len() {
            spans.push(span);
            continue;
        }
        let mut ranges: Vec<(usize, usize)> = Vec::new();
        for w in words {
            let mut from = 0;
            while let Some(i) = lower[from..].find(w.as_str()) {
                ranges.push((from + i, from + i + w.len()));
                from += i + w.len().max(1);
            }
        }
        if ranges.is_empty() {
            spans.push(span);
            continue;
        }
        hit = true;
        ranges.sort();
        let mut at = 0;
        for (start, end) in ranges {
            if start < at || !text.is_char_boundary(start) || !text.is_char_boundary(end) {
                continue;
            }
            if start > at {
                spans.push(Span::styled(text[at..start].to_string(), span.style));
            }
            spans.push(Span::styled(
                text[start..end].to_string(),
                span.style.add_modifier(Modifier::REVERSED),
            ));
            at = end;
        }
        if at < text.len() {
            spans.push(Span::styled(text[at..].to_string(), span.style));
        }
    }
    (TuiLine { spans, ..line }, hit)
}

/// Clamp a scroll offset to something that still shows content.
pub fn clamp_scroll(scroll: u16, line_count: usize, viewport_height: u16) -> u16 {
    let visible = viewport_height.saturating_sub(2); // borders
    let max = (line_count as u16).saturating_sub(visible);
    scroll.min(max)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_long_point_wraps_between_words_and_keeps_its_spaces() {
        use super::typing_rows;
        assert_eq!(typing_rows("", 10), [""]);
        assert_eq!(
            typing_rows("bfs uses a queue", 10),
            ["bfs uses a ", "queue"]
        );
        assert_eq!(typing_rows("bfs uses ", 8), ["bfs uses "]);
        assert_eq!(
            typing_rows("abcdefghijklmnop xyz", 6),
            ["abcdef", "ghijkl", "mnop ", "xyz"]
        );
        let text = "a point long enough to need three rows here";
        assert_eq!(typing_rows(text, 12).concat(), text);
        assert!(typing_rows(text, 12)
            .iter()
            .all(|r| r.trim_end().chars().count() <= 12));
    }

    #[test]
    fn the_point_box_grows_to_show_a_long_point() {
        use ratatui::{backend::TestBackend, Terminal};
        let scroll = crate::view::livescroll::LiveScroll::new();
        let jot = "first idea about graphs then a second thought on queues and stacks";
        let preview = super::Preview::Live {
            paused: false,
            points: Vec::new(),
            transcript: "we talked about graphs",
            jot: Some(jot),
            scroll: &scroll,
        };
        let mut terminal = Terminal::new(TestBackend::new(30, 14)).unwrap();
        terminal
            .draw(|f| super::render(f, f.area(), &preview, 0, true, None))
            .unwrap();
        let screen: Vec<String> = (0..14)
            .map(|y| {
                (0..30)
                    .map(|x| terminal.backend().buffer()[(x, y)].symbol().to_string())
                    .collect()
            })
            .collect();
        let text = screen.join("\n");
        for word in ["first", "graphs", "queues", "stacks"] {
            assert!(text.contains(word), "{word} is not on screen:\n{text}");
        }
    }

    #[test]
    fn a_huge_live_transcript_shows_only_its_recent_words() {
        let text: String = (0..50_000).map(|i| format!("w{i} ")).collect();
        let (shown, trimmed) = super::recent_words(&text, super::LIVE_WORDS);
        assert!(trimmed);
        assert_eq!(shown.split_whitespace().count(), super::LIVE_WORDS);
        assert!(shown.trim_end().ends_with("w49999"));
        let (all, cut) = super::recent_words("세계 short text", 10);
        assert_eq!((all, cut), ("세계 short text", false));
    }

    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn renders_a_notes_title_and_body() {
        let note = Note::new("Graphs", "- BFS\n- DFS", vec![], "");
        let mut terminal = Terminal::new(TestBackend::new(30, 6)).unwrap();
        terminal
            .draw(|f| render(f, f.area(), &Preview::Note(&note), 0, true, None))
            .unwrap();

        let out = terminal.backend().to_string();
        assert!(out.contains("Graphs"), "{out}");
        assert!(out.contains("BFS"), "{out}");
    }

    #[test]
    fn an_empty_preview_renders_the_placeholder_title() {
        let mut terminal = Terminal::new(TestBackend::new(20, 4)).unwrap();
        terminal
            .draw(|f| render(f, f.area(), &Preview::Empty, 0, false, None))
            .unwrap();
        assert!(terminal.backend().to_string().contains("preview"));
    }

    #[test]
    fn scrolling_past_the_end_is_clamped_not_a_panic() {
        let note = Note::new("T", "line\n".repeat(3), vec![], "");
        let mut terminal = Terminal::new(TestBackend::new(20, 6)).unwrap();
        terminal
            .draw(|f| render(f, f.area(), &Preview::Note(&note), 9999, true, None))
            .unwrap();

        assert_eq!(clamp_scroll(9999, 3, 6), 0, "3 lines fit in 4 rows");
        assert_eq!(clamp_scroll(9999, 100, 6), 96);
        assert_eq!(clamp_scroll(2, 100, 6), 2);
    }

    fn modifiers_on(buf: &ratatui::buffer::Buffer, word: &str) -> Vec<Modifier> {
        let mut out = Vec::new();
        for y in 0..buf.area.height {
            let line: String = (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect();
            if let Some(i) = line.find(word) {
                let x = line[..i].chars().count() as u16;
                out.push(buf[(x, y)].modifier);
            }
        }
        out
    }

    /// The words a search matched stand out in the note, so it is obvious
    /// why it was found.
    #[test]
    fn search_words_are_highlighted() {
        let note = Note::new("T", "intro\nBFS explores level by level\n", vec![], "");
        let mut t = Terminal::new(TestBackend::new(40, 8)).unwrap();
        t.draw(|f| render(f, f.area(), &Preview::Note(&note), 0, true, Some("bfs")))
            .unwrap();
        let buf = t.backend().buffer().clone();
        assert!(
            modifiers_on(&buf, "BFS")
                .iter()
                .any(|m| m.contains(Modifier::REVERSED)),
            "the match is not highlighted"
        );
        assert!(
            !modifiers_on(&buf, "explores")
                .iter()
                .any(|m| m.contains(Modifier::REVERSED)),
            "more than the match is highlighted"
        );
    }

    /// A match far down a long note is scrolled into view.
    #[test]
    fn a_match_below_the_fold_is_brought_into_view() {
        let body = format!("{}needle here\n", "filler\n".repeat(40));
        let note = Note::new("T", body, vec![], "");
        let mut t = Terminal::new(TestBackend::new(40, 10)).unwrap();
        t.draw(|f| render(f, f.area(), &Preview::Note(&note), 0, true, Some("needle")))
            .unwrap();
        assert!(t.backend().to_string().contains("needle here"));
    }

    /// The wiring, not the rendering: markdown details are tested next door, but
    /// something has to catch the preview drawing raw text again.
    #[test]
    fn a_note_body_goes_through_the_markdown_renderer() {
        let note = Note::new("T", "## Heading\n- [x] done\n", vec![], "");
        let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
        terminal
            .draw(|f| render(f, f.area(), &Preview::Note(&note), 0, true, None))
            .unwrap();
        let out = terminal.backend().to_string();

        assert!(!out.contains("##"), "hashes reached the screen: {out}");
        assert!(out.contains('☑'), "no rendered checkbox: {out}");
    }

    #[test]
    fn highlighting_text_whose_lowercase_shifts_characters_does_not_panic() {
        let line = TuiLine::from("ẞİ");
        let (out, _) = highlight(line, &["i".to_string()]);
        let text: String = out.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "ẞİ");
    }
}
