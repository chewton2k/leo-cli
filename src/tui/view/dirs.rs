//! The directory pane.

use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use ratatui::Frame;

use super::line::{border, selection};

/// One row of the pane. `..` and the current directory's children.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirRow {
    pub label: String,
    /// The path `cd` should receive when this row is opened.
    pub target: String,
}

/// Build the rows for a tag listing: every tag with how many notes carry it.
///
/// Sorted by count, because "which tags do I actually use" is the question this
/// pane answers, and alphabetical order buries it.
pub fn tag_rows(tags: &[(String, usize)]) -> Vec<DirRow> {
    let mut sorted = tags.to_vec();
    sorted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    sorted
        .into_iter()
        .map(|(tag, count)| DirRow {
            label: format!("#{tag}  {count}"),
            target: tag,
        })
        .collect()
}

/// Build the rows for a directory listing: an "up" entry when not at the root,
/// then each child.
pub fn rows(current_dir: &str, children: &[String]) -> Vec<DirRow> {
    let mut out = Vec::new();
    if !current_dir.is_empty() {
        out.push(DirRow { label: "..".to_string(), target: "..".to_string() });
    }
    for child in children {
        out.push(DirRow { label: format!("{child}/"), target: child.clone() });
    }
    out
}

/// `title` and `empty` differ between the directory and tag listings, which are
/// otherwise the same pane.
pub fn render(
    frame: &mut Frame,
    area: Rect,
    rows: &[DirRow],
    selected: usize,
    focused: bool,
    title: &str,
    empty: &super::empty::Hint,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border(focused))
        .title(title.to_string());

    if rows.is_empty() {
        let inner = block.inner(area);
        frame.render_widget(block, area);
        super::empty::render(frame, inner, empty);
        return;
    }

    let items: Vec<ListItem> = rows.iter().map(|r| ListItem::new(r.label.clone())).collect();
    let list = List::new(items)
        .block(block)
        .highlight_style(selection(focused));

    let mut state = ListState::default();
    state.select(Some(selected.min(rows.len() - 1)));
    frame.render_stateful_widget(list, area, &mut state);
}

#[cfg(test)]
mod tests {
    /// Tags are listed by how much they are used, since "which tags do I
    /// actually use" is what this pane is for.
    #[test]
    fn tags_are_listed_by_count_then_alphabetically() {
        let tags = vec![
            ("rust".to_string(), 2),
            ("cs130".to_string(), 5),
            ("apple".to_string(), 2),
        ];
        let rows = super::tag_rows(&tags);
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels, vec!["#cs130  5", "#apple  2", "#rust  2"]);
        // The target is the bare tag, which is what the filter needs.
        assert_eq!(rows[0].target, "cs130");
    }

    #[test]
    fn no_tags_is_an_empty_listing_rather_than_a_panic() {
        assert!(super::tag_rows(&[]).is_empty());
    }

    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn the_root_has_no_up_entry() {
        let r = rows("", &["cs130".to_string(), "cs162".to_string()]);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].label, "cs130/");
        assert_eq!(r[0].target, "cs130");
    }

    #[test]
    fn a_subdirectory_gets_an_up_entry_first() {
        let r = rows("cs130", &["lec".to_string()]);
        assert_eq!(r[0].label, "..");
        assert_eq!(r[0].target, "..");
        assert_eq!(r[1].label, "lec/");
    }

    #[test]
    fn renders_the_directory_names() {
        let mut terminal = Terminal::new(TestBackend::new(20, 6)).unwrap();
        let r = rows("cs130", &["lec".to_string()]);
        terminal
            .draw(|f| render(f, f.area(), &r, 0, true, "dirs", &crate::tui::view::empty::Hint::no_directories()))
            .unwrap();

        let rendered = terminal.backend().to_string();
        assert!(rendered.contains("dirs"), "{rendered}");
        assert!(rendered.contains(".."), "{rendered}");
        assert!(rendered.contains("lec/"), "{rendered}");
    }

    /// An out-of-range selection must clamp rather than panic.
    #[test]
    fn an_empty_or_overflowing_selection_does_not_panic() {
        let mut terminal = Terminal::new(TestBackend::new(20, 6)).unwrap();
        terminal.draw(|f| render(f, f.area(), &[], 3, false, "dirs", &crate::tui::view::empty::Hint::no_directories())).unwrap();
        let r = rows("", &["a".to_string()]);
        terminal.draw(|f| render(f, f.area(), &r, 99, true, "dirs", &crate::tui::view::empty::Hint::no_directories())).unwrap();
    }
}
