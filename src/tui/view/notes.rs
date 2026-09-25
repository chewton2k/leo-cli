//! The notes pane: the numbered list the user drives most.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line as TuiLine, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use ratatui::Frame;

use super::line::{border, selection};
use crate::notes::Note;

/// One row: the same 1-based number the `:` line accepts, plus a summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteRow {
    pub number: usize,
    pub id: String,
    pub title: String,
    pub tags: Vec<String>,
    /// The note's directory, when it is not the one being shown — a search
    /// lists notes from everywhere, and a title alone would hide where they are.
    pub elsewhere: Option<String>,
    /// Marked with Space, so D and m will include it.
    pub marked: bool,
}

pub fn rows(notes: &[&Note], current_dir: &str) -> Vec<NoteRow> {
    notes
        .iter()
        .enumerate()
        .map(|(i, n)| NoteRow {
            number: i + 1,
            id: n.id.clone(),
            title: n.title.clone(),
            tags: n.tags.clone(),
            elsewhere: (n.directory != current_dir).then(|| n.directory.clone()),
            marked: false,
        })
        .collect()
}

fn item(row: &NoteRow) -> ListItem<'static> {
    let mut spans = vec![
        Span::styled(format!("{:>3}", row.number), Style::default().add_modifier(Modifier::DIM)),
        Span::styled(
            if row.marked { "●" } else { " " },
            Style::default().fg(super::theme::accent()),
        ),
    ];
    if let Some(dir) = &row.elsewhere {
        let shown = if dir.is_empty() { "/".to_string() } else { format!("{dir}/") };
        spans.push(Span::styled(shown, Style::default().add_modifier(Modifier::DIM)));
    }
    spans.push(Span::raw(row.title.clone()));
    if !row.tags.is_empty() {
        spans.push(Span::styled(
            format!("  [{}]", row.tags.join(", ")),
            Style::default().add_modifier(Modifier::DIM),
        ));
    }
    ListItem::new(TuiLine::from(spans))
}

/// `empty` is what to say when there is nothing to list. The caller knows why the
/// pane is empty — no notes, an empty directory, a filter that matched nothing —
/// and only it can say the useful thing.
pub fn render(
    frame: &mut Frame,
    area: Rect,
    rows: &[NoteRow],
    selected: usize,
    focused: bool,
    empty: &super::empty::Hint,
    filter: Option<&str>,
) {
    // Naming the filter in the title is what stops a narrowed pane from looking
    // like a pane that lost its notes.
    let title = match filter {
        Some(query) if !query.trim().is_empty() => {
            format!("notes matching \"{query}\" ({})", rows.len())
        }
        _ if rows.is_empty() => "notes".to_string(),
        _ => format!("notes ({})", rows.len()),
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border(focused))
        .title(title);

    if rows.is_empty() {
        let inner = block.inner(area);
        frame.render_widget(block, area);
        super::empty::render(frame, inner, empty);
        return;
    }

    let inner = block.inner(area);
    let list = List::new(rows.iter().map(item).collect::<Vec<_>>())
        .block(block)
        .highlight_style(selection(focused));

    let mut state = ListState::default();
    state.select(Some(selected.min(rows.len() - 1)));
    // Set the offset explicitly rather than letting the widget derive one, so
    // that a click can be mapped back to a row with the same arithmetic.
    *state.offset_mut() = first_visible(selected, rows.len(), inner.height);
    frame.render_stateful_widget(list, area, &mut state);
}

/// Index of the topmost visible row.
///
/// Shared by rendering and hit-testing: if a click used different arithmetic
/// from the paint, clicking a scrolled list would select the wrong note.
pub fn first_visible(selected: usize, total: usize, height: u16) -> usize {
    let height = height as usize;
    if height == 0 || total <= height {
        return 0;
    }
    let last_possible = total - height;
    // Keep the selection on screen, scrolling no further than the end.
    selected.saturating_sub(height - 1).min(last_possible)
}

/// Which row a click at `row` lands on, given the pane's area.
///
/// `None` when the click was on a border or past the last row, so a stray click
/// moves nothing rather than jumping to the end.
pub fn row_at(area: Rect, row: u16, selected: usize, total: usize) -> Option<usize> {
    let inner_top = area.y + 1;
    let inner_height = area.height.saturating_sub(2);
    if row < inner_top || row >= inner_top + inner_height {
        return None;
    }
    let index = first_visible(selected, total, inner_height) + (row - inner_top) as usize;
    (index < total).then_some(index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    /// A narrowed pane must say it is narrowed, or it looks like a pane that lost
    /// its notes.
    #[test]
    fn a_filtered_pane_names_the_filter_in_its_title() {
        let a = note("Rust ownership", &[]);
        let r = rows(&[&a], "");
        let mut t = Terminal::new(TestBackend::new(48, 6)).unwrap();
        t.draw(|f| {
            render(
                f,
                f.area(),
                &r,
                0,
                true,
                &crate::tui::view::empty::Hint::no_notes(),
                Some("own"),
            )
        })
        .unwrap();
        let out = t.backend().to_string();
        assert!(out.contains("own"), "{out}");
        assert!(out.contains("(1)"), "{out}");
    }

    // ── hit testing ─────────────────────────────────────────────────────────

    /// A short list never scrolls, so the first row is always the first item.
    #[test]
    fn a_list_that_fits_starts_at_the_top() {
        assert_eq!(first_visible(0, 3, 10), 0);
        assert_eq!(first_visible(2, 3, 10), 0);
        assert_eq!(first_visible(0, 10, 10), 0);
    }

    /// A long list scrolls just enough to keep the selection visible, and never
    /// past the end — a window showing blank rows below the last item is the bug
    /// this prevents.
    #[test]
    fn a_long_list_scrolls_to_hold_the_selection_without_overshooting() {
        // 100 items, 10 rows.
        assert_eq!(first_visible(0, 100, 10), 0);
        assert_eq!(first_visible(9, 100, 10), 0, "the tenth item still fits");
        assert_eq!(first_visible(10, 100, 10), 1);
        assert_eq!(first_visible(99, 100, 10), 90, "the last item sits on the last row");
        // Beyond the end cannot scroll further.
        assert_eq!(first_visible(500, 100, 10), 90);
    }

    #[test]
    fn a_zero_height_pane_has_no_offset_and_does_not_panic() {
        assert_eq!(first_visible(5, 100, 0), 0);
    }

    /// The click-to-row mapping must agree with the paint, or clicking a scrolled
    /// list selects the wrong note — silently, which is the worst kind.
    #[test]
    fn a_click_maps_to_the_row_that_was_drawn() {
        // A pane 12 rows tall has 10 usable rows between its borders.
        let area = Rect::new(0, 0, 30, 12);

        // Unscrolled: the first inner row is item 0.
        assert_eq!(row_at(area, 1, 0, 100), Some(0));
        assert_eq!(row_at(area, 5, 0, 100), Some(4));
        assert_eq!(row_at(area, 10, 0, 100), Some(9));

        // Scrolled: selection 50 puts item 41 on the first row.
        let offset = first_visible(50, 100, 10);
        assert_eq!(offset, 41);
        assert_eq!(row_at(area, 1, 50, 100), Some(41));
        assert_eq!(row_at(area, 10, 50, 100), Some(50), "the selected row");
    }

    /// A click on a border or past the last item must move nothing rather than
    /// jumping somewhere arbitrary.
    #[test]
    fn a_click_outside_the_rows_selects_nothing() {
        let area = Rect::new(0, 0, 30, 12);
        assert_eq!(row_at(area, 0, 0, 100), None, "top border");
        assert_eq!(row_at(area, 11, 0, 100), None, "bottom border");
        assert_eq!(row_at(area, 50, 0, 100), None, "outside the pane");
        // Three items in a ten-row pane: rows four onwards are empty.
        assert_eq!(row_at(area, 3, 0, 3), Some(2));
        assert_eq!(row_at(area, 4, 0, 3), None, "past the last item");
        assert_eq!(row_at(area, 1, 0, 0), None, "empty list");
    }

    /// An empty pane must say what to do. A blank one is indistinguishable from
    /// a broken one, which is how leo read to new users.
    #[test]
    fn an_empty_pane_shows_the_hint_it_was_given() {
        let mut t = Terminal::new(TestBackend::new(46, 8)).unwrap();
        let hint = crate::tui::view::empty::Hint::empty_directory();
        t.draw(|f| render(f, f.area(), &[], 0, true, &hint, None)).unwrap();
        let out = t.backend().to_string();

        assert!(out.contains("Nothing in this directory"), "{out}");
        assert!(out.contains("cd .."), "{out}");
        // The count is dropped rather than reading "notes (0)".
        assert!(!out.contains("(0)"), "{out}");
    }

    fn note(title: &str, tags: &[&str]) -> Note {
        Note::new(
            title,
            "body",
            tags.iter().map(|t| t.to_string()).collect(),
            "",
        )
    }

    #[test]
    fn rows_are_numbered_from_one() {
        let a = note("First", &[]);
        let b = note("Second", &["rust"]);
        let r = rows(&[&a, &b], "");
        assert_eq!(r[0].number, 1);
        assert_eq!(r[1].number, 2);
        assert_eq!(r[1].tags, vec!["rust"]);
    }

    #[test]
    fn renders_numbers_titles_and_tags() {
        let a = note("Rust ownership", &["rust", "learning"]);
        let r = rows(&[&a], "");
        let mut terminal = Terminal::new(TestBackend::new(50, 5)).unwrap();
        terminal.draw(|f| render(f, f.area(), &r, 0, true, &crate::tui::view::empty::Hint::no_notes(), None)).unwrap();

        let out = terminal.backend().to_string();
        assert!(out.contains("Rust ownership"), "{out}");
        assert!(out.contains("rust, learning"), "{out}");
        assert!(out.contains("1"), "{out}");
        assert!(out.contains("notes (1)"), "{out}");
    }

    /// The title used to read "notes (empty)". The hint in the pane says it
    /// better, so the title stays clean and the advice goes where the eye is.
    #[test]
    fn an_empty_list_offers_advice_rather_than_labelling_the_title() {
        let mut terminal = Terminal::new(TestBackend::new(34, 6)).unwrap();
        terminal
            .draw(|f| {
                render(
                    f,
                    f.area(),
                    &[],
                    0,
                    false,
                    &crate::tui::view::empty::Hint::no_notes(),
                    None,
                )
            })
            .unwrap();
        let out = terminal.backend().to_string();
        assert!(out.contains("No notes yet"), "{out}");
        assert!(!out.contains("(empty)"), "{out}");
    }

    #[test]
    fn a_title_longer_than_the_pane_does_not_panic() {
        let a = note(&"x".repeat(500), &[]);
        let r = rows(&[&a], "");
        let mut terminal = Terminal::new(TestBackend::new(20, 4)).unwrap();
        terminal.draw(|f| render(f, f.area(), &r, 0, true, &crate::tui::view::empty::Hint::no_notes(), None)).unwrap();
    }
}
