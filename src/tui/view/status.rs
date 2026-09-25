//! The `:` line and the status line beneath it.

use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line as TuiLine, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::action::Kind;

use super::line::style_for;

use super::theme;

/// Draw the command line. When `active`, the terminal cursor is placed in it so
/// the user sees a real caret rather than a drawn one.
///
/// `ghost` is the completion hint shown ahead of the cursor; it is not part of
/// the text and is never submitted. `idle` is what the line shows when it is not
/// in use: the keys that do something where the user is.
pub fn render_command(
    frame: &mut Frame,
    area: Rect,
    active: bool,
    text: &str,
    cursor: usize,
    ghost: Option<&str>,
    idle: &[(&'static str, &'static str)],
) {
    if !active {
        let hints = super::hints::spans(idle, area.width);
        frame.render_widget(Paragraph::new(TuiLine::from(hints)), area);
        return;
    }

    let mut spans = vec![
        Span::styled(":", Style::default().fg(theme::accent())),
        Span::raw(text.to_string()),
    ];
    if let Some(ghost) = ghost {
        if !ghost.is_empty() {
            spans.push(Span::styled(
                ghost.to_string(),
                Style::default().add_modifier(Modifier::DIM),
            ));
        }
    }
    frame.render_widget(Paragraph::new(TuiLine::from(spans)), area);

    // +1 for the leading ":".
    let x = area.x + 1 + cursor as u16;
    frame.set_cursor_position(Position::new(x.min(area.x + area.width.saturating_sub(1)), area.y));
}

/// Draw the live filter on the command line, with a real caret.
///
/// A separate line from the `:` prompt on purpose: `/` is not a command, it is a
/// lens on the pane above, and showing the count keeps the effect visible while
/// typing.
pub fn render_filter(frame: &mut Frame, area: Rect, query: &str, matches: usize) {
    let mut spans = vec![
        Span::styled("/", Style::default().fg(theme::accent())),
        Span::raw(query.to_string()),
    ];
    let summary = match matches {
        0 => "  no matches".to_string(),
        1 => "  1 match".to_string(),
        n => format!("  {n} matches"),
    };
    spans.push(Span::styled(
        summary,
        Style::default().add_modifier(Modifier::DIM),
    ));
    frame.render_widget(Paragraph::new(TuiLine::from(spans)), area);

    // +1 for the leading "/".
    let x = area.x + 1 + query.chars().count() as u16;
    frame.set_cursor_position(Position::new(
        x.min(area.x + area.width.saturating_sub(1)),
        area.y,
    ));
}

/// Draw the point being typed during a recording, with a real caret. Empty,
/// it says what typing here does.
pub fn render_jot(frame: &mut Frame, area: Rect, text: &str) {
    let mut spans = vec![Span::styled("✎ ", Style::default().fg(theme::accent()))];
    if text.is_empty() {
        spans.push(Span::styled(
            "type what matters · Enter adds it · Tab raw text · Esc stops",
            Style::default().add_modifier(Modifier::DIM),
        ));
    } else {
        spans.push(Span::raw(text.to_string()));
    }
    frame.render_widget(Paragraph::new(TuiLine::from(spans)), area);
    let x = area.x + 2 + text.chars().count() as u16;
    frame.set_cursor_position(Position::new(x.min(area.x + area.width.saturating_sub(1)), area.y));
}

/// What the right-hand side of the bar reports.
///
/// Passed in rather than computed here so the view stays a view: the counts come
/// from the store, which this module has no business reaching into.
#[derive(Debug, Clone, Copy, Default)]
pub struct Counts {
    /// Notes in the current listing.
    pub notes: usize,
    /// Words in the selected note, or none when nothing is selected.
    pub words: Option<usize>,
}

/// The status bar: where we are, what just happened, and what is here.
///
/// Drawn as a filled bar rather than text on the terminal background. The
/// difference is not decoration — a bar gives the interface a bottom edge, so the
/// panes read as a window instead of text that happens to stop.
pub fn render_status(
    frame: &mut Frame,
    area: Rect,
    dir: &str,
    message: Option<(Kind, &str)>,
    busy: Option<&str>,
    counts: Counts,
) {
    let bar = Style::default().bg(theme::bar());

    // Fill first: every cell gets the background, including the gap in the
    // middle, or the bar would appear as two disconnected patches.
    frame.render_widget(ratatui::widgets::Block::default().style(bar), area);

    let where_ = if dir.is_empty() {
        "/".to_string()
    } else {
        format!("/{dir}")
    };
    let mut left = vec![Span::styled(
        format!(" {where_} "),
        bar.fg(theme::accent()).add_modifier(Modifier::BOLD),
    )];

    if let Some(label) = busy {
        left.push(Span::styled(format!("{label} "), bar.fg(theme::warn())));
    }

    if let Some((kind, text)) = message {
        // Keep the message's intent colour, but on the bar's background.
        let style = style_for(kind).bg(theme::bar());
        left.push(Span::styled(text.to_string(), style));
    }

    let right = summary(counts);
    let left_width: usize = left.iter().map(|s| s.content.chars().count()).sum();
    let right_width = right.chars().count();

    frame.render_widget(Paragraph::new(TuiLine::from(left)).style(bar), area);

    // Only draw the counts if they fit without colliding with the message.
    if right_width > 0 && left_width + right_width + 2 <= area.width as usize {
        let x = area.x + area.width - right_width as u16 - 1;
        let right_area = Rect::new(x, area.y, right_width as u16, 1);
        frame.render_widget(
            Paragraph::new(TuiLine::from(Span::styled(
                right,
                bar.add_modifier(Modifier::DIM),
            )))
            .style(bar),
            right_area,
        );
    }
}

/// The right-hand summary: note count, and the selected note's length.
///
/// Pluralised, because "1 notes" is the kind of detail that makes software feel
/// unfinished.
fn summary(counts: Counts) -> String {
    let notes = match counts.notes {
        1 => "1 note".to_string(),
        n => format!("{n} notes"),
    };
    match counts.words {
        Some(1) => format!("{notes} · 1 word"),
        Some(w) => format!("{notes} · {w} words"),
        None => notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn counts(notes: usize, words: Option<usize>) -> Counts {
        Counts { notes, words }
    }

    /// The bar must be a bar: every cell carries the background, or it looks like
    /// two disconnected patches of colour.
    #[test]
    fn the_status_bar_is_filled_all_the_way_across() {
        let mut t = Terminal::new(TestBackend::new(40, 1)).unwrap();
        t.draw(|f| render_status(f, f.area(), "cs130", None, None, counts(3, None)))
            .unwrap();

        let buffer = t.backend().buffer().clone();
        for x in 0..40 {
            assert_eq!(
                buffer[(x, 0)].style().bg,
                Some(theme::bar()),
                "cell {x} has no bar background"
            );
        }
    }

    #[test]
    fn the_bar_shows_where_we_are_and_what_is_here() {
        let mut t = Terminal::new(TestBackend::new(50, 1)).unwrap();
        t.draw(|f| render_status(f, f.area(), "cs130", None, None, counts(3, Some(120))))
            .unwrap();
        let out = t.backend().to_string();
        assert!(out.contains("/cs130"), "{out}");
        assert!(out.contains("3 notes"), "{out}");
        assert!(out.contains("120 words"), "{out}");
    }

    #[test]
    fn the_root_directory_reads_as_a_slash() {
        let mut t = Terminal::new(TestBackend::new(30, 1)).unwrap();
        t.draw(|f| render_status(f, f.area(), "", None, None, counts(0, None)))
            .unwrap();
        assert!(t.backend().to_string().contains('/'));
    }

    #[test]
    fn counts_are_pluralised() {
        assert_eq!(summary(counts(1, None)), "1 note");
        assert_eq!(summary(counts(2, None)), "2 notes");
        assert_eq!(summary(counts(0, None)), "0 notes");
        assert_eq!(summary(counts(1, Some(1))), "1 note · 1 word");
        assert_eq!(summary(counts(2, Some(9))), "2 notes · 9 words");
    }

    /// A long message must not be overwritten by the counts, nor overflow.
    #[test]
    fn the_counts_give_way_to_a_long_message() {
        let mut t = Terminal::new(TestBackend::new(30, 1)).unwrap();
        let long = "a message long enough to fill the whole bar and then some";
        t.draw(|f| {
            render_status(
                f,
                f.area(),
                "somewhere",
                Some((Kind::Good, long)),
                None,
                counts(42, Some(999)),
            )
        })
        .unwrap();
        let out = t.backend().to_string();
        assert!(!out.contains("42 notes"), "counts collided with the message: {out}");
    }

    #[test]
    fn a_message_keeps_its_intent_colour_on_the_bar() {
        let mut t = Terminal::new(TestBackend::new(40, 1)).unwrap();
        t.draw(|f| {
            render_status(f, f.area(), "", Some((Kind::Bad, "failed")), None, counts(1, None))
        })
        .unwrap();
        let buffer = t.backend().buffer().clone();
        let has_bad = (0..40).any(|x| buffer[(x, 0)].style().fg == Some(theme::bad()));
        assert!(has_bad, "an error message lost its colour on the bar");
    }

    #[test]
    fn a_narrow_terminal_does_not_panic() {
        for width in [1, 2, 3, 8] {
            let mut t = Terminal::new(TestBackend::new(width, 1)).unwrap();
            t.draw(|f| {
                render_status(f, f.area(), "deep/directory", None, Some("Working"), counts(9, Some(9)))
            })
            .unwrap();
        }
    }

    #[test]
    fn the_filter_line_shows_the_query_and_how_many_matched() {
        let mut t = Terminal::new(TestBackend::new(50, 1)).unwrap();
        t.draw(|f| render_filter(f, f.area(), "owner", 3)).unwrap();
        let out = t.backend().to_string();
        assert!(out.contains("/owner"), "{out}");
        assert!(out.contains("3 matches"), "{out}");
    }

    #[test]
    fn the_filter_line_pluralises_and_says_when_nothing_matched() {
        let mut t = Terminal::new(TestBackend::new(50, 1)).unwrap();
        t.draw(|f| render_filter(f, f.area(), "zzz", 0)).unwrap();
        assert!(t.backend().to_string().contains("no matches"));

        let mut t = Terminal::new(TestBackend::new(50, 1)).unwrap();
        t.draw(|f| render_filter(f, f.area(), "a", 1)).unwrap();
        assert!(t.backend().to_string().contains("1 match"));
    }

    #[test]
    fn a_long_filter_query_does_not_panic() {
        let mut t = Terminal::new(TestBackend::new(12, 1)).unwrap();
        t.draw(|f| render_filter(f, f.area(), &"x".repeat(80), 0)).unwrap();
    }

    #[test]
    fn an_inactive_command_line_shows_the_key_hints() {
        let mut t = Terminal::new(TestBackend::new(60, 1)).unwrap();
        t.draw(|f| render_command(f, f.area(), false, "", 0, None, super::super::hints::for_place(super::super::hints::Place::Notes))).unwrap();
        let out = t.backend().to_string();
        assert!(out.contains("n new"), "{out}");
        assert!(out.contains("? help"), "{out}");
    }

    #[test]
    fn an_active_command_line_shows_a_colon_and_the_text() {
        let mut t = Terminal::new(TestBackend::new(30, 1)).unwrap();
        t.draw(|f| render_command(f, f.area(), true, "list", 4, None, &[])).unwrap();
        assert!(t.backend().to_string().contains(":list"));
    }

    #[test]
    fn the_ghost_hint_follows_the_typed_text() {
        let mut t = Terminal::new(TestBackend::new(30, 1)).unwrap();
        t.draw(|f| render_command(f, f.area(), true, "vie", 3, Some("w"), &[])).unwrap();
        assert!(t.backend().to_string().contains(":view"), "{}", t.backend().to_string());
    }

    /// A cursor beyond the pane must be clamped, not passed through.
    #[test]
    fn a_cursor_past_the_edge_is_clamped() {
        let mut t = Terminal::new(TestBackend::new(10, 1)).unwrap();
        t.draw(|f| render_command(f, f.area(), true, &"x".repeat(50), 50, None, &[]))
            .unwrap();
    }

    #[test]
    fn the_status_line_shows_the_directory_and_the_last_message() {
        let mut t = Terminal::new(TestBackend::new(60, 1)).unwrap();
        t.draw(|f| {
            render_status(f, f.area(), "cs130/lec", Some((Kind::Good, "Created abc")), None, Counts::default())
        })
        .unwrap();
        let out = t.backend().to_string();
        assert!(out.contains("/cs130/lec"), "{out}");
        assert!(out.contains("Created abc"), "{out}");
    }

    #[test]
    fn the_root_directory_shows_as_a_slash() {
        let mut t = Terminal::new(TestBackend::new(20, 1)).unwrap();
        t.draw(|f| render_status(f, f.area(), "", None, None, Counts::default())).unwrap();
        assert!(t.backend().to_string().contains(" / "), "{}", t.backend().to_string());
    }

    #[test]
    fn a_busy_label_is_visible_alongside_the_message() {
        let mut t = Terminal::new(TestBackend::new(60, 1)).unwrap();
        t.draw(|f| render_status(f, f.area(), "", None, Some("Recording 00:12"), Counts::default())).unwrap();
        assert!(t.backend().to_string().contains("Recording 00:12"));
    }
}
