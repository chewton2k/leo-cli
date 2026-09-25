//! The preview pane: the selected note's body, or the transcript stream while
//! recording.

use ratatui::layout::Rect;
use ratatui::text::Line as TuiLine;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use ratatui::style::{Modifier, Style};
use ratatui::text::Span;

use super::line::border;
use super::markdown::{BOX_DONE, BOX_OPEN};
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
    cursor: Option<usize>,
    search: Option<&str>,
) {
    if let Preview::Live {
        paused,
        points,
        transcript,
        jot,
    } = preview
    {
        render_live(frame, area, *paused, points, transcript, *jot, focused);
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
        Preview::Note(n) => (n.title.clone(), super::markdown::render(&n.body)),
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

    if let Some(n) = cursor {
        let is_box = |l: &TuiLine| {
            l.spans
                .iter()
                .any(|s| s.content == BOX_OPEN || s.content == BOX_DONE)
        };
        if let Some(row) = (0..lines.len()).filter(|&i| is_box(&lines[i])).nth(n) {
            lines[row] = lines[row]
                .clone()
                .patch_style(Style::default().add_modifier(Modifier::REVERSED));
            let row = row as u16;
            if row < scroll {
                scroll = row;
            } else if row >= scroll + visible {
                scroll = row + 1 - visible;
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
fn render_live(
    frame: &mut Frame,
    area: Rect,
    paused: bool,
    points: &[String],
    transcript: &str,
    jot: Option<&str>,
    focused: bool,
) {
    use ratatui::layout::{Constraint, Layout, Position};

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border(focused))
        .title(if paused {
            "live transcript — paused (Ctrl-P resumes)"
        } else {
            "live transcript"
        });
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let box_height = if jot.is_some() { 3 } else { 0 };
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
        for p in points.iter().rev().take(points_height as usize - 2).rev() {
            lines.push(TuiLine::from(vec![
                Span::raw("• "),
                Span::styled(p.clone(), Style::default().add_modifier(Modifier::BOLD)),
            ]));
        }
        frame.render_widget(Paragraph::new(lines), points_area);
    }

    let text = if transcript.trim().is_empty() {
        vec![TuiLine::from(Span::styled(
            "listening...",
            Style::default().add_modifier(Modifier::DIM),
        ))]
    } else {
        let rows = wrap(transcript, text_area.width as usize);
        let skip = rows.len().saturating_sub(text_area.height as usize);
        rows.into_iter().skip(skip).map(TuiLine::from).collect()
    };
    frame.render_widget(Paragraph::new(text), text_area);

    if let Some(jot) = jot {
        let jot_box = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(super::theme::accent()))
            .title(" your point · Enter adds it · Ctrl-P pauses · Esc stops ");
        let field = jot_box.inner(box_area);
        frame.render_widget(jot_box, box_area);
        let shown = if jot.is_empty() {
            Span::styled(
                "type what matters — it leads the note, in bold",
                Style::default().add_modifier(Modifier::DIM),
            )
        } else {
            Span::raw(jot.to_string())
        };
        frame.render_widget(Paragraph::new(TuiLine::from(shown)), field);
        let x = field.x + jot.chars().count() as u16;
        frame.set_cursor_position(Position::new(
            x.min(field.x + field.width.saturating_sub(1)),
            field.y,
        ));
    }
}

/// Greedy word wrap to `width` columns; a word longer than a row is split.
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
            if start < at {
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
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn renders_a_notes_title_and_body() {
        let note = Note::new("Graphs", "- BFS\n- DFS", vec![], "");
        let mut terminal = Terminal::new(TestBackend::new(30, 6)).unwrap();
        terminal
            .draw(|f| render(f, f.area(), &Preview::Note(&note), 0, true, None, None))
            .unwrap();

        let out = terminal.backend().to_string();
        assert!(out.contains("Graphs"), "{out}");
        assert!(out.contains("BFS"), "{out}");
    }

    #[test]
    fn an_empty_preview_renders_the_placeholder_title() {
        let mut terminal = Terminal::new(TestBackend::new(20, 4)).unwrap();
        terminal
            .draw(|f| render(f, f.area(), &Preview::Empty, 0, false, None, None))
            .unwrap();
        assert!(terminal.backend().to_string().contains("preview"));
    }

    #[test]
    fn scrolling_past_the_end_is_clamped_not_a_panic() {
        let note = Note::new("T", "line\n".repeat(3), vec![], "");
        let mut terminal = Terminal::new(TestBackend::new(20, 6)).unwrap();
        terminal
            .draw(|f| render(f, f.area(), &Preview::Note(&note), 9999, true, None, None))
            .unwrap();

        assert_eq!(clamp_scroll(9999, 3, 6), 0, "3 lines fit in 4 rows");
        assert_eq!(clamp_scroll(9999, 100, 6), 96);
        assert_eq!(clamp_scroll(2, 100, 6), 2);
    }

    /// The checkbox cursor is drawn reversed, so `x` visibly means that line.
    #[test]
    fn the_checkbox_under_the_cursor_is_highlighted() {
        let note = Note::new("T", "- [ ] read\n- [x] done\n", vec![], "");
        let mut terminal = Terminal::new(TestBackend::new(30, 6)).unwrap();
        terminal
            .draw(|f| render(f, f.area(), &Preview::Note(&note), 0, true, Some(1), None))
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let row_of = |word: &str| {
            (0..buf.area.height)
                .find(|y| {
                    let line: String = (0..buf.area.width)
                        .map(|x| buf[(x, *y)].symbol().to_string())
                        .collect();
                    line.contains(word)
                })
                .unwrap()
        };
        let reversed = |y: u16| {
            (1..buf.area.width - 1).any(|x| {
                buf[(x, y)]
                    .modifier
                    .contains(ratatui::style::Modifier::REVERSED)
            })
        };
        assert!(
            reversed(row_of("done")),
            "the cursor line is not highlighted"
        );
        assert!(
            !reversed(row_of("read")),
            "the other box is highlighted too"
        );
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
        t.draw(|f| {
            render(
                f,
                f.area(),
                &Preview::Note(&note),
                0,
                true,
                None,
                Some("bfs"),
            )
        })
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
        t.draw(|f| {
            render(
                f,
                f.area(),
                &Preview::Note(&note),
                0,
                true,
                None,
                Some("needle"),
            )
        })
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
            .draw(|f| render(f, f.area(), &Preview::Note(&note), 0, true, None, None))
            .unwrap();
        let out = terminal.backend().to_string();

        assert!(!out.contains("##"), "hashes reached the screen: {out}");
        assert!(out.contains('☑'), "no rendered checkbox: {out}");
    }
}
