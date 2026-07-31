//! The strip of recently visited notes, along the top.
//!
//! An editor's row of tabs, for the same reason editors have one: writing means
//! moving between two or three notes, and a visible list makes the return trip a
//! glance and a keypress rather than a search.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line as TuiLine, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use super::theme;

/// How much of a title a tab shows before it is cut short.
const MAX_TITLE: usize = 18;

/// One tab: a note's title, and whether it is the one on screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab {
    pub title: String,
    pub current: bool,
}

/// Draw the strip. Nothing is drawn when there are no tabs, so an unused feature
/// costs no space.
pub fn render(frame: &mut Frame, area: Rect, tabs: &[Tab]) {
    if tabs.is_empty() || area.height == 0 {
        return;
    }

    let mut spans = Vec::new();
    let mut used = 0usize;

    for tab in tabs {
        let label = shorten(&tab.title);
        // Two spaces of padding, plus a separator between tabs.
        let width = label.chars().count() + 3;
        if used + width > area.width as usize {
            break;
        }
        used += width;

        // The current note is the accent on the bar; the rest recede. Without
        // that contrast the strip reads as a sentence rather than a set of tabs.
        let style = if tab.current {
            Style::default()
                .bg(theme::bar())
                .fg(theme::accent())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().add_modifier(Modifier::DIM)
        };
        spans.push(Span::styled(format!(" {label} "), style));
        spans.push(Span::styled(
            "·",
            Style::default().add_modifier(Modifier::DIM),
        ));
    }

    // Drop the trailing separator: it separates nothing.
    spans.pop();
    frame.render_widget(Paragraph::new(TuiLine::from(spans)), area);
}

/// Which tab is at `column`, if any.
///
/// Shares [`shorten`] and the same padding as [`render`], so a click lands on the
/// tab the user actually sees rather than a neighbour.
pub fn tab_at(tabs: &[Tab], column: u16) -> Option<usize> {
    let mut x = 0usize;
    for (index, tab) in tabs.iter().enumerate() {
        let label = shorten(&tab.title);
        // " label " plus the separator that follows it.
        let width = label.chars().count() + 2;
        if (column as usize) >= x && (column as usize) < x + width {
            return Some(index);
        }
        x += width + 1;
    }
    None
}

/// Cut a long title with an ellipsis, counting characters rather than bytes so a
/// multi-byte title cannot be split mid-character.
fn shorten(title: &str) -> String {
    if title.chars().count() <= MAX_TITLE {
        return title.to_string();
    }
    let kept: String = title.chars().take(MAX_TITLE.saturating_sub(1)).collect();
    format!("{kept}…")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn tab(title: &str, current: bool) -> Tab {
        Tab {
            title: title.to_string(),
            current,
        }
    }

    fn drawn(tabs: &[Tab], width: u16) -> String {
        let mut t = Terminal::new(TestBackend::new(width, 1)).unwrap();
        t.draw(|f| render(f, f.area(), tabs)).unwrap();
        t.backend().to_string()
    }

    #[test]
    fn every_tab_that_fits_is_shown() {
        let out = drawn(&[tab("Alpha", true), tab("Beta", false)], 40);
        assert!(out.contains("Alpha"), "{out}");
        assert!(out.contains("Beta"), "{out}");
    }

    /// The current note has to stand out, or the strip reads as a sentence.
    #[test]
    fn the_current_tab_is_distinguished_from_the_rest() {
        let mut t = Terminal::new(TestBackend::new(40, 1)).unwrap();
        let tabs = [tab("Alpha", true), tab("Beta", false)];
        t.draw(|f| render(f, f.area(), &tabs)).unwrap();

        let buffer = t.backend().buffer().clone();
        let accented = (0..40).any(|x| buffer[(x, 0)].style().fg == Some(theme::accent()));
        assert!(accented, "the current tab is not highlighted");
    }

    /// A long title is cut, not wrapped, and never mid-character.
    #[test]
    fn a_long_title_is_shortened_with_an_ellipsis() {
        let long = "A title far longer than any tab should be";
        let shortened = shorten(long);
        assert!(shortened.chars().count() <= MAX_TITLE);
        assert!(shortened.ends_with('…'), "{shortened}");

        // Multi-byte titles must not be split into invalid text.
        let unicode = "日本語のタイトルがとても長い場合はどうなるか";
        let shortened = shorten(unicode);
        assert!(shortened.chars().count() <= MAX_TITLE);
    }

    #[test]
    fn a_short_title_is_left_alone() {
        assert_eq!(shorten("Alpha"), "Alpha");
    }

    /// Tabs that do not fit are dropped rather than overflowing the row.
    #[test]
    fn tabs_stop_at_the_edge_of_the_screen() {
        let tabs = [
            tab("First", false),
            tab("Second", false),
            tab("Third", false),
            tab("Fourth", false),
        ];
        let out = drawn(&tabs, 20);
        assert!(out.contains("First"), "{out}");
        assert!(!out.contains("Fourth"), "the strip overflowed: {out}");
    }

    /// An unused feature must cost no space.
    #[test]
    fn no_tabs_draws_nothing() {
        // `TestBackend` quotes each row, so compare the contents rather than the
        // whole string.
        let out = drawn(&[], 30);
        let contents: String = out.chars().filter(|c| !matches!(c, '"' | '\n')).collect();
        assert!(contents.trim().is_empty(), "{out:?}");
    }

    /// A click has to land on the tab under the pointer, using the same widths
    /// the paint used.
    #[test]
    fn a_click_finds_the_tab_under_it() {
        let tabs = [tab("Alpha", true), tab("Beta", false), tab("Gamma", false)];
        // " Alpha " occupies columns 0..7, then a separator at 7.
        assert_eq!(tab_at(&tabs, 0), Some(0));
        assert_eq!(tab_at(&tabs, 3), Some(0));
        assert_eq!(tab_at(&tabs, 6), Some(0));
        assert_eq!(tab_at(&tabs, 7), None, "the separator belongs to no tab");
        assert_eq!(tab_at(&tabs, 8), Some(1));
        assert_eq!(tab_at(&tabs, 13), Some(1));
        assert_eq!(tab_at(&tabs, 15), Some(2));
        assert_eq!(tab_at(&tabs, 200), None, "past the last tab");
    }

    #[test]
    fn a_click_on_an_empty_strip_finds_nothing() {
        assert_eq!(tab_at(&[], 0), None);
    }

    #[test]
    fn a_zero_width_or_height_strip_does_not_panic() {
        let mut t = Terminal::new(TestBackend::new(10, 1)).unwrap();
        t.draw(|f| render(f, Rect::new(0, 0, 0, 0), &[tab("A", true)]))
            .unwrap();
        t.draw(|f| render(f, Rect::new(0, 0, 10, 0), &[tab("A", true)]))
            .unwrap();
    }
}
