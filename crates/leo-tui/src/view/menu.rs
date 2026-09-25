//! The menu above an open `:` line: what can be typed at the cursor.
//!
//! Tab completion already knew the candidates; showing them turns "remember a
//! verb" into "pick one", which is the difference for someone who has not used
//! leo in a month.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line as TuiLine, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;

use super::theme;

/// Rows shown at once. Enough to see the whole verb list.
pub const MAX_ROWS: usize = 14;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub label: String,
    /// What a verb does. Only verbs have one.
    pub detail: Option<&'static str>,
}

/// Draw `items` in a box sitting directly on top of `line`, the command line.
///
/// `selected` is the candidate Tab has put on the line, if any. When there are
/// more items than fit, the window follows it.
pub fn render(frame: &mut Frame, screen: Rect, line: Rect, items: &[Item], selected: Option<usize>) {
    let room = line.y.saturating_sub(screen.y) as usize;
    let rows = items.len().min(MAX_ROWS).min(room);
    if rows == 0 {
        return;
    }
    let first = match selected {
        Some(i) if i >= rows => i + 1 - rows,
        _ => 0,
    };
    let shown = &items[first..first + rows];

    let label_width = shown.iter().map(|i| i.label.chars().count()).max().unwrap_or(0);
    let lines: Vec<TuiLine> = shown
        .iter()
        .enumerate()
        .map(|(n, item)| {
            let picked = selected == Some(first + n);
            let style = if picked {
                Style::default().fg(theme::accent()).add_modifier(Modifier::REVERSED)
            } else {
                Style::default().fg(theme::accent())
            };
            let mut spans = vec![Span::styled(format!(" {:<label_width$} ", item.label), style)];
            if let Some(detail) = item.detail {
                spans.push(Span::styled(
                    format!(" {detail} "),
                    Style::default().add_modifier(Modifier::DIM),
                ));
            }
            TuiLine::from(spans)
        })
        .collect();

    let width = lines
        .iter()
        .map(|l| l.width())
        .max()
        .unwrap_or(0)
        .min(line.width as usize) as u16;
    let area = Rect {
        x: line.x,
        y: line.y - rows as u16,
        width,
        height: rows as u16,
    };
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().style(Style::default().bg(theme::bar()))),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn item(label: &str, detail: Option<&'static str>) -> Item {
        Item { label: label.to_string(), detail }
    }

    fn draw(height: u16, items: &[Item], selected: Option<usize>) -> String {
        let mut t = Terminal::new(TestBackend::new(60, height)).unwrap();
        t.draw(|f| {
            let screen = f.area();
            let line = Rect { x: 0, y: screen.height - 1, width: 60, height: 1 };
            render(f, screen, line, items, selected);
        })
        .unwrap();
        t.backend().to_string()
    }

    #[test]
    fn a_verb_shows_what_it_does() {
        let out = draw(6, &[item("rename", Some("retitle the selected note"))], None);
        assert!(out.contains("rename"), "{out}");
        assert!(out.contains("retitle the selected note"), "{out}");
    }

    #[test]
    fn no_room_above_draws_nothing_and_does_not_panic() {
        let out = draw(1, &[item("rename", None)], None);
        assert!(!out.contains("rename"), "{out}");
    }

    #[test]
    fn the_window_follows_the_selection() {
        let items: Vec<Item> = (0..30).map(|i| item(&format!("item{i:02}"), None)).collect();
        let out = draw(40, &items, Some(25));
        assert!(out.contains("item25"), "{out}");
        assert!(!out.contains("item00"), "{out}");
    }
}
