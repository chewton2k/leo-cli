//! What a pane says when it has nothing to show.
//!
//! An empty pane and a broken pane look identical, and leo has spent its whole
//! life looking broken to new users for exactly this reason: a blank list gives
//! no clue whether there is nothing here, the filter matched nothing, or the app
//! failed to load. Every empty pane now names the key that fills it.

use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line as TuiLine, Span};
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::Frame;

use super::theme;

/// Characters in a line, for working out how many rows it will wrap to.
fn line_width(line: &TuiLine<'_>) -> usize {
    line.spans.iter().map(|s| s.content.chars().count()).sum()
}

/// How many rows `width` characters occupy in a pane `available` wide.
fn rows_for(width: usize, available: u16) -> u16 {
    if available == 0 {
        return 1;
    }
    let available = available as usize;
    width.div_ceil(available).max(1) as u16
}

/// A message for an empty pane: what is going on, and what to press.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    /// Why the pane is empty, in the user's terms.
    pub reason: String,
    /// The key or command that changes it. Rendered in the accent.
    pub action: String,
}

impl Hint {
    pub fn new(reason: impl Into<String>, action: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            action: action.into(),
        }
    }

    /// Nothing here yet, at the top level.
    pub fn no_notes() -> Self {
        Self::new("No notes yet", ":new to write one")
    }

    /// An empty directory, which is different from an empty collection: the way
    /// out matters as much as the way forward.
    pub fn empty_directory() -> Self {
        Self::new("Nothing in this directory", ":new here · :cd .. to leave")
    }

    /// A filter that matched nothing. Quoting the query is the point — it is
    /// usually a typo, and seeing it is the fix.
    pub fn no_matches(query: &str) -> Self {
        Self::new(format!("Nothing matches \"{query}\""), "Esc to clear")
    }

    pub fn no_directories() -> Self {
        Self::new("No directories", ":mkdir name to add one")
    }

    pub fn no_selection() -> Self {
        Self::new("No note selected", "j and k to move")
    }

    pub fn no_tags() -> Self {
        Self::new("No tags yet", "add tags: to a note's frontmatter")
    }
}

/// Draw a hint centred in `area`, which is expected to be inside a pane's
/// borders.
///
/// Vertically centred rather than at the top, so it reads as a state the pane is
/// in rather than as its first row of content.
pub fn render(frame: &mut Frame, area: Rect, hint: &Hint) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    let lines = vec![
        TuiLine::from(Span::styled(
            hint.reason.clone(),
            Style::default().add_modifier(Modifier::DIM),
        )),
        TuiLine::from(Span::styled(
            hint.action.clone(),
            Style::default()
                .fg(theme::accent_muted())
                .add_modifier(Modifier::DIM),
        )),
    ];

    // Wrapped, because the dirs pane is eighteen columns wide and a truncated
    // instruction (":mkdir name to a") reads as a bug rather than a hint.
    let needed = lines
        .iter()
        .map(|line| rows_for(line_width(line), area.width))
        .sum::<u16>()
        .max(1);
    let height = needed.min(area.height);
    let top = area.y + area.height.saturating_sub(height) / 2;
    let centred = Rect::new(area.x, top, area.width, height);

    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        centred,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn drawn(hint: &Hint, width: u16, height: u16) -> String {
        let mut t = Terminal::new(TestBackend::new(width, height)).unwrap();
        t.draw(|f| render(f, f.area(), hint)).unwrap();
        t.backend().to_string()
    }

    /// Both halves must reach the screen: the reason alone leaves the user
    /// informed but stuck.
    #[test]
    fn a_hint_shows_its_reason_and_its_action() {
        let out = drawn(&Hint::no_notes(), 40, 6);
        assert!(out.contains("No notes yet"), "{out}");
        assert!(out.contains(":new"), "{out}");
    }

    #[test]
    fn every_built_in_hint_offers_a_way_forward() {
        let hints = [
            Hint::no_notes(),
            Hint::empty_directory(),
            Hint::no_matches("xyz"),
            Hint::no_directories(),
            Hint::no_selection(),
            Hint::no_tags(),
        ];
        for hint in hints {
            assert!(!hint.reason.trim().is_empty(), "{hint:?}");
            assert!(!hint.action.trim().is_empty(), "no action: {hint:?}");
        }
    }

    /// A failed search is usually a typo, so the query has to be visible.
    #[test]
    fn a_failed_search_quotes_what_was_searched_for() {
        let hint = Hint::no_matches("owenrship");
        assert!(hint.reason.contains("owenrship"), "{hint:?}");
        let out = drawn(&hint, 44, 5);
        assert!(out.contains("owenrship"), "{out}");
    }

    /// An empty directory needs the way out, not just the way forward.
    #[test]
    fn an_empty_directory_says_how_to_leave() {
        assert!(Hint::empty_directory().action.contains("cd .."));
    }

    #[test]
    fn a_pane_too_small_for_the_hint_does_not_panic() {
        for (w, h) in [(1, 1), (2, 0), (0, 2), (3, 1), (80, 1)] {
            let mut t = Terminal::new(TestBackend::new(w.max(1), h.max(1))).unwrap();
            t.draw(|f| {
                let area = Rect::new(0, 0, w, h);
                render(f, area, &Hint::no_notes())
            })
            .unwrap();
        }
    }

    /// A truncated instruction reads as a bug. The dirs pane is eighteen columns
    /// wide, so the hint has to wrap rather than run off the edge.
    #[test]
    fn a_hint_wraps_in_a_narrow_pane_rather_than_being_cut_off() {
        let hint = Hint::no_directories();
        // The inner width of an 18-column pane.
        let out = drawn(&hint, 16, 8);
        let text: String = out.chars().filter(|c| !matches!(c, '"' | '\n')).collect();

        // Every word of the action survives somewhere on screen.
        for word in [":mkdir", "name", "to", "add", "one"] {
            assert!(text.contains(word), "lost {word:?} from:\n{out}");
        }
    }

    #[test]
    fn wrapping_arithmetic_counts_rows_not_characters() {
        assert_eq!(rows_for(10, 20), 1);
        assert_eq!(rows_for(20, 20), 1);
        assert_eq!(rows_for(21, 20), 2);
        assert_eq!(rows_for(40, 20), 2);
        assert_eq!(rows_for(41, 20), 3);
        // Degenerate widths must not divide by zero.
        assert_eq!(rows_for(10, 0), 1);
        assert_eq!(rows_for(0, 20), 1);
    }

    /// Centred, so it reads as a state rather than as content.
    #[test]
    fn the_hint_sits_in_the_middle_not_the_top() {
        let out = drawn(&Hint::no_notes(), 30, 9);
        let rows: Vec<&str> = out.lines().collect();
        let first = rows
            .iter()
            .position(|r| r.contains("No notes yet"))
            .expect("hint not drawn");
        assert!(first > 1, "hint drawn at row {first}, expected lower");
    }
}
