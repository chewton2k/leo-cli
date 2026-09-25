//! Shared styling, so `Kind` means the same thing in every pane.

use ratatui::style::{Modifier, Style};

use super::theme;
use ratatui::text::{Line as TuiLine, Span};

use leo_core::action::{Kind, Line};

pub fn style_for(kind: Kind) -> Style {
    match kind {
        Kind::Plain | Kind::Blank => Style::default(),
        Kind::Dim => Style::default().add_modifier(Modifier::DIM),
        Kind::Good => Style::default().fg(theme::good()),
        Kind::Warn => Style::default().fg(theme::warn()),
        Kind::Bad => Style::default().fg(theme::bad()),
        Kind::Dir => Style::default().fg(theme::accent()).add_modifier(Modifier::BOLD),
    }
}

/// Convert one handler output line into a renderable line.
pub fn to_tui(line: &Line) -> TuiLine<'static> {
    TuiLine::from(Span::styled(line.text.clone(), style_for(line.kind)))
}

/// The border style for a pane: the accent when focused, a muted form of the
/// same hue otherwise, so the frame reads as one palette rather than one lit
/// pane and two grey ones.
pub fn border(focused: bool) -> Style {
    if focused {
        Style::default().fg(theme::accent())
    } else {
        Style::default().fg(theme::accent_muted())
    }
}

/// The style for the selected row of a focused or unfocused list. An unfocused
/// pane keeps a dimmer marker so the user can still see where they left it.
pub fn selection(focused: bool) -> Style {
    if focused {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_gets_a_distinct_intent() {
        assert_eq!(style_for(Kind::Good).fg, Some(theme::good()));
        assert_eq!(style_for(Kind::Bad).fg, Some(theme::bad()));
        assert_eq!(style_for(Kind::Warn).fg, Some(theme::warn()));
        assert_eq!(style_for(Kind::Dir).fg, Some(theme::accent()));
        assert!(style_for(Kind::Dim).add_modifier.contains(Modifier::DIM));
        assert_eq!(style_for(Kind::Plain), Style::default());
    }

    #[test]
    fn a_focused_pane_is_visually_distinct() {
        assert_ne!(border(true), border(false));
        assert_ne!(selection(true), selection(false));
        // Both borders use the accent hue, so the frame reads as one palette.
        assert_eq!(border(true).fg, Some(theme::accent()));
        assert_eq!(border(false).fg, Some(theme::accent_muted()));
    }

    #[test]
    fn output_lines_keep_their_text() {
        let line = Line::good("Created abc12345");
        assert_eq!(to_tui(&line).to_string(), "Created abc12345");
    }
}
