//! The preview pane: the selected note's body, or the transcript stream while
//! recording.

use ratatui::layout::Rect;
use ratatui::text::Line as TuiLine;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use super::line::border;
use crate::notes::Note;

/// What the pane is currently showing.
pub enum Preview<'a> {
    Empty,
    Note(&'a Note),
    /// Free text, used by the live transcription stream.
    Text { title: String, body: String },
    /// Handler output, keeping each line's `Kind` styling.
    Lines { title: String, lines: &'a [crate::action::Line] },
}

pub fn render(frame: &mut Frame, area: Rect, preview: &Preview<'_>, scroll: u16, focused: bool) {
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

    let (title, lines): (String, Vec<TuiLine>) = match preview {
        Preview::Empty => (String::new(), Vec::new()),
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
    let scroll = clamp_scroll(scroll, line_count, area.height);

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
            .draw(|f| render(f, f.area(), &Preview::Note(&note), 0, true))
            .unwrap();

        let out = terminal.backend().to_string();
        assert!(out.contains("Graphs"), "{out}");
        assert!(out.contains("BFS"), "{out}");
    }

    #[test]
    fn an_empty_preview_renders_the_placeholder_title() {
        let mut terminal = Terminal::new(TestBackend::new(20, 4)).unwrap();
        terminal
            .draw(|f| render(f, f.area(), &Preview::Empty, 0, false))
            .unwrap();
        assert!(terminal.backend().to_string().contains("preview"));
    }

    #[test]
    fn scrolling_past_the_end_is_clamped_not_a_panic() {
        let note = Note::new("T", "line\n".repeat(3), vec![], "");
        let mut terminal = Terminal::new(TestBackend::new(20, 6)).unwrap();
        terminal
            .draw(|f| render(f, f.area(), &Preview::Note(&note), 9999, true))
            .unwrap();

        assert_eq!(clamp_scroll(9999, 3, 6), 0, "3 lines fit in 4 rows");
        assert_eq!(clamp_scroll(9999, 100, 6), 96);
        assert_eq!(clamp_scroll(2, 100, 6), 2);
    }

    /// The wiring, not the rendering: markdown details are tested next door, but
    /// something has to catch the preview drawing raw text again.
    #[test]
    fn a_note_body_goes_through_the_markdown_renderer() {
        let note = Note::new("T", "## Heading\n- [x] done\n", vec![], "");
        let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
        terminal
            .draw(|f| render(f, f.area(), &Preview::Note(&note), 0, true))
            .unwrap();
        let out = terminal.backend().to_string();

        assert!(!out.contains("##"), "hashes reached the screen: {out}");
        assert!(out.contains('☑'), "no rendered checkbox: {out}");
    }
}
