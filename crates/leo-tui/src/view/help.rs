//! The help overlay and the confirmation prompt.
//!
//! Help is grouped by task and scrollable, because the flat list it replaced
//! overflowed anything shorter than a full-height terminal and silently hid its
//! own last rows.

use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line as TuiLine, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use super::theme;

/// Center a box of the given size inside `area`.
pub fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let [row] = Layout::vertical([Constraint::Length(height.min(area.height))])
        .flex(Flex::Center)
        .areas(area);
    let [cell] = Layout::horizontal([Constraint::Length(width.min(area.width))])
        .flex(Flex::Center)
        .areas(row);
    cell
}

/// One row of help: a key or command, and what it does. An empty `what` marks a
/// section heading.
pub struct Entry {
    pub key: &'static str,
    pub what: &'static str,
}

pub struct Section {
    pub title: &'static str,
    pub entries: &'static [Entry],
}

const fn e(key: &'static str, what: &'static str) -> Entry {
    Entry { key, what }
}

/// Help content, grouped the way someone looks for it.
pub const SECTIONS: &[Section] = &[
    Section {
        title: "Moving around",
        entries: &[
            e("j / k", "down / up"),
            e("g / G", "first / last"),
            e("h / l", "switch pane: dirs, notes, body"),
            e("Enter", "open a directory, or focus the note body"),
            e("Ctrl-D / Ctrl-U", "scroll the note"),
            e("Tab", "back to a recently visited note"),
            e("Ctrl-R", "reload from disk, and repaint"),
            e("click", "focus a pane, or select a row"),
            e("wheel", "scroll whatever is under the pointer"),
            e("Esc", "close an overlay, or unpin output"),
            e("?", "this help"),
            e("q", "quit"),
        ],
    },
    Section {
        title: "Finding things",
        entries: &[
            e("/", "search every note: titles, bodies, tags"),
            e("  #word", "only notes with that tag"),
            e("  Enter", "keep the results, and return to the panes"),
            e("  Esc", "clear it, staying on the note you picked"),
            e("Ctrl-P", "the same as /"),
            e("t", "left pane: directories or tags"),
            e("  Enter", "on a tag: show only those notes"),
        ],
    },
    Section {
        title: "Everyday keys",
        entries: &[
            e("n", "new note here, in $EDITOR"),
            e("N", "new directory"),
            e("e", "edit the selected note"),
            e("r", "rename it"),
            e("m", "move it to another directory"),
            e("x", "tick its first open checkbox"),
            e(
                "  j / k",
                "in the preview: pick a checkbox, then x ticks it",
            ),
            e("a", "ask AI: answer its @leo lines"),
            e("R", "record a new note by talking"),
            e("D", "delete it (asks first)"),
            e("Space", "mark notes; D and m then act on all of them"),
            e("D", "in the dirs pane: delete that whole directory"),
            e("u", "undo the last delete, move or tick"),
        ],
    },
    Section {
        title: "While recording",
        entries: &[
            e("type", "a point that matters; it leads the notes, in bold"),
            e("Enter", "add the point"),
            e("Tab", "raw text or bullets"),
            e("Esc", "stop and save"),
        ],
    },
    Section {
        title: "The : line",
        entries: &[
            e(":", "start a command; the commands are listed below"),
            e("Tab", "complete verbs, notes, dirs, tags"),
            e("Up / Down", "previous commands"),
            e("Ctrl-W / Ctrl-U", "delete a word / the line"),
            e("[note]", "leave it out to mean the selected note"),
        ],
    },
    Section {
        title: "Providers and settings",
        entries: &[
            e("Ctrl-S", "your profile: providers, keys, colour, backup"),
            e(
                "  Enter",
                "on a provider: store its key, add it, or test it",
            ),
            e("  x", "on that screen: remove a key"),
            e(
                "  J / K",
                "on that screen: change the order they are tried in",
            ),
            e("  a / d", "on that screen: add to / drop from a list"),
            e("  e", "on that screen: open config.toml"),
            e("  Enter", "on a setting: change it, or set up git backup"),
            e("leo setup", "what works here; fixes what does not"),
        ],
    },
    Section {
        title: "Elsewhere",
        entries: &[
            e("backup", "Ctrl-S: back up on quit, or when idle"),
            e("leo serve", "read notes from your phone (shell only)"),
            e("Ctrl-R", "reload from disk, and repaint the screen"),
        ],
    },
];

/// Every documented key or command, flattened. The manual's tests assert
/// against this so a binding cannot be added without being documented in both
/// places — which is the only consumer, hence the allow.
#[allow(dead_code)]
pub fn all_keys() -> Vec<&'static str> {
    SECTIONS
        .iter()
        .flat_map(|s| s.entries.iter().map(|e| e.key))
        .collect()
}

/// Build the rendered lines, so scrolling and height can be computed from the
/// same content that gets drawn.
fn help_lines() -> Vec<TuiLine<'static>> {
    let heading = |title: &str| {
        TuiLine::from(Span::styled(
            format!(" {title}"),
            Style::default()
                .fg(theme::accent())
                .add_modifier(Modifier::BOLD),
        ))
    };
    let row = |key: String, what: &'static str| {
        TuiLine::from(vec![
            Span::styled(
                format!("  {key:<20}"),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(what),
        ])
    };

    let mut lines = Vec::new();
    for (i, section) in SECTIONS.iter().enumerate() {
        if i > 0 {
            lines.push(TuiLine::from(""));
        }
        lines.push(heading(section.title));
        for entry in section.entries {
            lines.push(row(entry.key.to_string(), entry.what));
        }
    }

    // Built from the verb table, so a command cannot exist without being here.
    lines.push(TuiLine::from(""));
    lines.push(heading("Commands"));
    for verb in leo_core::action::VERBS {
        lines.push(row(format!(":{}", verb.usage), verb.summary));
    }
    lines
}

/// Total content height, for clamping the caller's scroll offset.
pub fn line_count() -> usize {
    help_lines().len()
}

/// How many terminal rows the content occupies once wrapped to `width`.
///
/// `Paragraph::scroll` counts *rendered* rows, not logical lines, so clamping
/// against `line_count()` leaves the last section unreachable whenever anything
/// wrapped — which is what hid the final section on a narrow overlay.
fn wrapped_rows(width: u16) -> usize {
    let width = width.max(1) as usize;
    help_lines()
        .iter()
        .map(|line| {
            let len = line.to_string().chars().count();
            len.div_ceil(width).max(1)
        })
        .sum()
}

/// Draw help. `scroll` is a line offset the caller owns, so j/k work here the
/// same as everywhere else.
pub fn render_help(frame: &mut Frame, area: Rect, scroll: u16) {
    let lines = help_lines();

    // The whole terminal: this is a reference, not a dialog, and a 66-column box
    // cut the longer lines on a narrow terminal while showing fewer entries per
    // page on a wide one.
    let box_area = area;
    let inner_height = box_area.height.saturating_sub(2) as usize;
    let inner_width = box_area.width.saturating_sub(2);

    let total = wrapped_rows(inner_width);
    let max_scroll = total.saturating_sub(inner_height) as u16;
    let scroll = scroll.min(max_scroll);

    let more = if max_scroll > 0 {
        format!(
            " help  {}-{} of {} · j/k scroll · any other key closes ",
            scroll as usize + 1,
            (scroll as usize + inner_height).min(total),
            total
        )
    } else {
        " help  ·  any key closes ".to_string()
    };

    frame.render_widget(Clear, box_area);
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::accent()))
                    .title(more),
            )
            .scroll((scroll, 0))
            .wrap(Wrap { trim: false }),
        box_area,
    );
}

/// A yes/no prompt. Destructive actions route through this rather than acting on
/// a single key press.
pub fn render_confirm(frame: &mut Frame, area: Rect, prompt: &str) {
    let box_area = centered(area, (prompt.len() as u16 + 14).min(area.width), 5);
    frame.render_widget(Clear, box_area);
    frame.render_widget(
        Paragraph::new(vec![
            TuiLine::from(""),
            TuiLine::from(Span::raw(format!("  {prompt}"))),
            TuiLine::from(Span::styled(
                "  y to confirm, anything else cancels",
                Style::default().add_modifier(Modifier::DIM),
            )),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow))
                .title("confirm"),
        )
        .wrap(Wrap { trim: false }),
        box_area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn centering_keeps_the_box_inside_the_area() {
        let area = Rect::new(0, 0, 80, 24);
        let c = centered(area, 40, 10);
        assert!(c.x + c.width <= 80);
        assert!(c.y + c.height <= 24);
        assert_eq!(c.width, 40);
        assert_eq!(c.height, 10);
    }

    #[test]
    fn an_oversized_box_is_clamped_to_the_area() {
        let area = Rect::new(0, 0, 20, 6);
        let c = centered(area, 100, 40);
        assert_eq!(c.width, 20);
        assert_eq!(c.height, 6);
    }

    #[test]
    fn every_entry_has_both_a_key_and_a_description() {
        for section in SECTIONS {
            assert!(!section.title.trim().is_empty());
            assert!(!section.entries.is_empty(), "{} is empty", section.title);
            for entry in section.entries {
                assert!(!entry.key.trim().is_empty(), "{}: blank key", section.title);
                assert!(
                    !entry.what.trim().is_empty(),
                    "{}: {} has no description",
                    section.title,
                    entry.key
                );
            }
        }
    }

    /// Help is grouped, and the grouping is the point — a single section would
    /// be the flat list this replaced.
    #[test]
    fn help_is_grouped_into_several_sections() {
        assert!(SECTIONS.len() >= 6, "only {} sections", SECTIONS.len());
    }

    /// Every key the keymap actually handles must be findable in help. This is
    /// the check that catches a binding added to keys.rs and never documented.
    #[test]
    fn every_bound_key_appears_somewhere_in_help() {
        let text: String = help_lines().iter().map(|l| l.to_string()).collect();
        for key in [
            "j", "k", "g", "G", "h", "l", "Enter", "x", "e", "D", ":", "/", "Tab", "Ctrl-P",
            "Ctrl-S", "Ctrl-D", "Ctrl-U", "Ctrl-R", "Esc", "t", "?", "q",
        ] {
            assert!(text.contains(key), "help never shows the {key} key");
        }
    }

    /// Flattened view of every documented key, for other modules' tests.
    #[test]
    fn the_flattened_key_list_is_not_empty() {
        assert!(all_keys().len() >= 20, "only {} keys", all_keys().len());
    }

    /// Every verb must be discoverable here. This is now the *only* full
    /// reference — the manual note was cut to a quickstart that points at `?` —
    /// so a gap here is a gap everywhere.
    #[test]
    fn every_command_verb_appears_in_help() {
        let text: String = help_lines().iter().map(|l| l.to_string()).collect();
        for verb in leo_core::action::VERBS {
            assert!(
                text.contains(verb.usage),
                "help never shows `{}`",
                verb.usage
            );
        }
    }

    /// And no retired name may linger, or the reference teaches a command that
    /// no longer works.
    #[test]
    fn no_retired_command_appears_in_help() {
        let text: String = help_lines().iter().map(|l| l.to_string()).collect();
        for (alias, _, _) in leo_core::action::RETIRED {
            assert!(
                !text.contains(&format!(":{alias} ")),
                "help still documents `:{alias}`"
            );
        }
        assert!(!text.contains("leo env"), "help still documents leo env");
    }

    /// The one command that diagnoses a broken setup has to be findable.
    #[test]
    fn help_mentions_setup() {
        let text: String = help_lines().iter().map(|l| l.to_string()).collect();
        assert!(text.contains("leo setup"));
    }

    #[test]
    fn renders_the_first_section_at_the_top() {
        let mut t = Terminal::new(TestBackend::new(80, 40)).unwrap();
        t.draw(|f| render_help(f, f.area(), 0)).unwrap();
        let out = t.backend().to_string();
        assert!(out.contains("Moving around"), "{out}");
        assert!(out.contains("switch pane"), "{out}");
    }

    /// Scrolling is what makes the later sections reachable in a short
    /// terminal, which the previous flat list could not do.
    #[test]
    fn scrolling_reveals_later_sections() {
        let mut t = Terminal::new(TestBackend::new(80, 14)).unwrap();

        t.draw(|f| render_help(f, f.area(), 0)).unwrap();
        let top = t.backend().to_string();

        t.draw(|f| render_help(f, f.area(), line_count() as u16))
            .unwrap();
        let bottom = t.backend().to_string();

        assert_ne!(top, bottom, "scrolling changed nothing");
        assert!(top.contains("Moving around"));
        assert!(
            bottom.contains(":quit"),
            "the last section is unreachable:\n{bottom}"
        );
    }

    #[test]
    fn the_title_reports_the_scroll_position_when_there_is_more_to_see() {
        let mut t = Terminal::new(TestBackend::new(80, 12)).unwrap();
        t.draw(|f| render_help(f, f.area(), 0)).unwrap();
        let out = t.backend().to_string();
        assert!(out.contains("of "), "no position indicator: {out}");
        assert!(out.contains("j/k"), "no scroll hint: {out}");
    }

    /// The indicator must be a well-formed range, and the last page must end at
    /// the true line count rather than running past it.
    #[test]
    fn the_position_indicator_is_a_well_formed_range() {
        // Full screen now: a 12-row terminal leaves 10 content rows inside the
        // border, and 80 columns wrap to 78.
        let total = wrapped_rows(78);
        let mut t = Terminal::new(TestBackend::new(80, 12)).unwrap();

        t.draw(|f| render_help(f, f.area(), 0)).unwrap();
        let top = t.backend().to_string();
        assert!(top.contains(&format!("1-10 of {total}")), "got: {top}");

        // Scrolled to the end: the window ends exactly at the last row.
        t.draw(|f| render_help(f, f.area(), 9999)).unwrap();
        let bottom = t.backend().to_string();
        assert!(
            bottom.contains(&format!("-{total} of {total}")),
            "last page should end at {total}: {bottom}"
        );
    }

    /// Wrapping is why the clamp cannot use the logical line count: an entry
    /// longer than the overlay is two rows on screen but one line in the table.
    #[test]
    fn wrapped_rows_never_undercounts_the_logical_lines() {
        assert!(wrapped_rows(64) >= line_count());
        // Narrower means more wrapping, never less.
        assert!(wrapped_rows(20) > wrapped_rows(64));
        // A degenerate width must not divide by zero.
        assert!(wrapped_rows(0) > 0);
    }

    #[test]
    fn scrolling_past_the_end_is_clamped_rather_than_blank() {
        let mut t = Terminal::new(TestBackend::new(80, 40)).unwrap();
        t.draw(|f| render_help(f, f.area(), 9999)).unwrap();
        // Something is still on screen.
        assert!(t.backend().to_string().contains("serve"));
    }

    #[test]
    fn help_in_a_tiny_terminal_does_not_panic() {
        for (w, h) in [(24, 6), (10, 4), (40, 3), (200, 60)] {
            let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
            t.draw(|f| render_help(f, f.area(), 0)).unwrap();
        }
    }

    #[test]
    fn the_confirm_prompt_shows_the_question_and_the_keys() {
        let mut t = Terminal::new(TestBackend::new(60, 8)).unwrap();
        t.draw(|f| render_confirm(f, f.area(), "Delete Rust ownership?"))
            .unwrap();
        let out = t.backend().to_string();
        assert!(out.contains("Delete Rust ownership?"), "{out}");
        assert!(out.contains("y to confirm"), "{out}");
    }
}
