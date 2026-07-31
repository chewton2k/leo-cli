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
}

pub fn rows(notes: &[&Note]) -> Vec<NoteRow> {
    notes
        .iter()
        .enumerate()
        .map(|(i, n)| NoteRow {
            number: i + 1,
            id: n.id.clone(),
            title: n.title.clone(),
            tags: n.tags.clone(),
        })
        .collect()
}

fn item(row: &NoteRow) -> ListItem<'static> {
    let mut spans = vec![
        Span::styled(
            format!("{:>3} ", row.number),
            Style::default().add_modifier(Modifier::DIM),
        ),
        Span::raw(row.title.clone()),
    ];
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
) {
    let title = if rows.is_empty() {
        "notes".to_string()
    } else {
        format!("notes ({})", rows.len())
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

    let list = List::new(rows.iter().map(item).collect::<Vec<_>>())
        .block(block)
        .highlight_style(selection(focused));

    let mut state = ListState::default();
    state.select(Some(selected.min(rows.len() - 1)));
    frame.render_stateful_widget(list, area, &mut state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    /// An empty pane must say what to do. A blank one is indistinguishable from
    /// a broken one, which is how leo read to new users.
    #[test]
    fn an_empty_pane_shows_the_hint_it_was_given() {
        let mut t = Terminal::new(TestBackend::new(46, 8)).unwrap();
        let hint = crate::tui::view::empty::Hint::empty_directory();
        t.draw(|f| render(f, f.area(), &[], 0, true, &hint)).unwrap();
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
        let r = rows(&[&a, &b]);
        assert_eq!(r[0].number, 1);
        assert_eq!(r[1].number, 2);
        assert_eq!(r[1].tags, vec!["rust"]);
    }

    #[test]
    fn renders_numbers_titles_and_tags() {
        let a = note("Rust ownership", &["rust", "learning"]);
        let r = rows(&[&a]);
        let mut terminal = Terminal::new(TestBackend::new(50, 5)).unwrap();
        terminal.draw(|f| render(f, f.area(), &r, 0, true, &crate::tui::view::empty::Hint::no_notes())).unwrap();

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
        let r = rows(&[&a]);
        let mut terminal = Terminal::new(TestBackend::new(20, 4)).unwrap();
        terminal.draw(|f| render(f, f.area(), &r, 0, true, &crate::tui::view::empty::Hint::no_notes())).unwrap();
    }
}
