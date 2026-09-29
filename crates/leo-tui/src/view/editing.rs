use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line as TuiLine, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use super::line::border;
use super::markdown;
use super::theme;
use crate::editor::{breaks, cursor_xy, Editor, Row};

fn split_spans(line: TuiLine<'static>, starts: &[usize]) -> Vec<TuiLine<'static>> {
    let mut rows: Vec<Vec<Span<'static>>> = vec![Vec::new(); starts.len()];
    let mut at = 0;
    let mut current = 0;
    for span in line.spans {
        let mut piece = String::new();
        for c in span.content.chars() {
            while current + 1 < starts.len() && at >= starts[current + 1] {
                if !piece.is_empty() {
                    rows[current].push(Span::styled(std::mem::take(&mut piece), span.style));
                }
                current += 1;
            }
            piece.push(c);
            at += 1;
        }
        if !piece.is_empty() {
            rows[current].push(Span::styled(piece, span.style));
        }
    }
    rows.into_iter().map(TuiLine::from).collect()
}

fn text_of(line: &TuiLine) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

pub fn render(frame: &mut Frame, area: Rect, title: &str, editor: &Editor, focused: bool) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border(focused))
        .title(title.to_string());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let width = (inner.width as usize).saturating_sub(1).max(1);
    editor.width.set(width);
    editor.height.set(inner.height as usize);
    editor.origin.set((inner.x, inner.y));

    let code = Style::default().fg(theme::accent_muted());
    let mut rows: Vec<TuiLine<'static>> = Vec::new();
    let mut layout: Vec<Row> = Vec::new();
    let mut cursor = (0, 0);
    let mut in_fence = false;
    for (i, raw) in editor.lines.iter().enumerate() {
        let fence = markdown::is_fence(raw);
        let is_cursor = i == editor.row;
        let line = if is_cursor {
            TuiLine::from(Span::raw(raw.clone()))
        } else if fence {
            TuiLine::from(Span::styled(raw.clone(), code.add_modifier(Modifier::DIM)))
        } else if in_fence {
            TuiLine::from(Span::styled(raw.clone(), code))
        } else {
            markdown::render_line(raw)
        };
        if fence {
            in_fence = !in_fence;
        }
        let starts = breaks(&text_of(&line), width);
        if is_cursor {
            let (x, y) = cursor_xy(raw, editor.col, width);
            cursor = (x, rows.len() + y);
        }
        for &start in &starts {
            layout.push(Row {
                line: i,
                start,
                raw: is_cursor || fence || in_fence,
            });
        }
        rows.extend(split_spans(line, &starts));
    }

    let visible = inner.height as usize;
    let mut scroll = editor.scroll.get().min(rows.len().saturating_sub(1));
    if editor.follow.get() {
        if cursor.1 < scroll {
            scroll = cursor.1;
        } else if cursor.1 >= scroll + visible {
            scroll = cursor.1 + 1 - visible;
        }
        editor.follow.set(false);
    }
    editor.scroll.set(scroll);
    *editor.layout.borrow_mut() = layout;

    let shown: Vec<TuiLine<'static>> = rows.into_iter().skip(scroll).take(visible).collect();
    frame.render_widget(Paragraph::new(shown), inner);
    if focused && cursor.1 >= scroll && cursor.1 < scroll + visible {
        frame.set_cursor_position(Position::new(
            inner.x + cursor.0 as u16,
            inner.y + (cursor.1 - scroll) as u16,
        ));
    }
}

pub enum Hit {
    Place { line: usize, col: usize },
    Box { line: usize },
}

pub fn hit(editor: &Editor, column: u16, row: u16) -> Option<Hit> {
    let (x0, y0) = editor.origin.get();
    if column < x0 || row < y0 {
        return None;
    }
    let visual = (row - y0) as usize + editor.scroll.get();
    let layout = editor.layout.borrow();
    let target = *layout.get(visual)?;
    let raw = editor.lines.get(target.line)?;
    let x = (column - x0) as usize;
    if target.raw {
        let width = editor.width.get();
        let y = layout[..visual]
            .iter()
            .rev()
            .take_while(|r| r.line == target.line)
            .count();
        return Some(Hit::Place {
            line: target.line,
            col: crate::editor::col_at(raw, width, y, x),
        });
    }
    let shown = target.start + x;
    if target.start == 0 && markdown::on_box(raw, shown) {
        return Some(Hit::Box { line: target.line });
    }
    Some(Hit::Place {
        line: target.line,
        col: markdown::raw_col(raw, shown),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn draw(editor: &Editor, width: u16, height: u16) -> String {
        let mut t = Terminal::new(TestBackend::new(width, height)).unwrap();
        t.draw(|f| render(f, f.area(), "Groceries", editor, true))
            .unwrap();
        t.backend().to_string()
    }

    #[test]
    fn only_the_line_with_the_cursor_shows_its_markdown() {
        let mut ed = Editor::open("id", "## Week\n- [ ] milk\n- [x] eggs");
        ed.place(1, 0);
        let out = draw(&ed, 40, 8);
        assert!(out.contains("- [ ] milk"), "{out}");
        assert!(!out.contains("## Week"), "{out}");
        assert!(out.contains("Week"), "{out}");
        assert!(!out.contains("- [x] eggs"), "{out}");
        assert!(out.contains("eggs"), "{out}");
    }

    #[test]
    fn a_click_on_a_formatted_line_moves_the_cursor_there() {
        let mut ed = Editor::open("id", "- [ ] milk\nsome **bold** text");
        ed.place(0, 0);
        draw(&ed, 40, 8);
        match hit(&ed, 1 + 7, 2) {
            Some(Hit::Place { line, col }) => assert_eq!((line, col), (1, 9)),
            _ => panic!("expected a place"),
        }
    }

    #[test]
    fn a_click_on_a_box_is_a_tick() {
        let mut ed = Editor::open("id", "text\n- [ ] milk");
        ed.place(0, 0);
        draw(&ed, 40, 8);
        assert!(matches!(hit(&ed, 1, 2), Some(Hit::Box { line: 1 })));
    }

    #[test]
    fn the_view_scrolls_to_keep_the_cursor_on_screen() {
        let body: Vec<String> = (0..50).map(|i| format!("line {i}")).collect();
        let ed = Editor::open("id", &body.join("\n"));
        let out = draw(&ed, 30, 8);
        assert!(out.contains("line 49"), "{out}");
        assert!(!out.contains("line 0 "), "{out}");
    }

    #[test]
    fn thousands_of_random_keys_never_panic_or_lose_the_cursor() {
        let alphabet: Vec<char> = "ab -[]x*#>1.세계é🎉\t".chars().collect();
        let mut seed: u64 = 0x5eed;
        let mut next = |n: usize| {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) as usize) % n
        };
        for round in 0..40 {
            let mut ed = Editor::open("id", "- [ ] 세계\n## 제목\n```\ncode 🎉\n```\n> quote");
            let width = 3 + next(40) as u16;
            let height = 3 + next(12) as u16;
            let mut t = Terminal::new(TestBackend::new(width, height)).unwrap();
            for _ in 0..300 {
                match next(16) {
                    0 => ed.enter(),
                    1 => ed.backspace(),
                    2 => ed.delete(),
                    3 => ed.left(),
                    4 => ed.right(),
                    5 => ed.up(),
                    6 => ed.down(),
                    7 => ed.indent(),
                    8 => ed.outdent(),
                    9 => {
                        ed.undo();
                    }
                    10 => ed.paste("x\n세\n- [ ] 🎉"),
                    11 => {
                        ed.toggle_box(next(ed.lines.len() + 2));
                    }
                    12 => ed.page(next(2) == 0),
                    13 => {
                        let (x, y) = (
                            next(width as usize + 3) as u16,
                            next(height as usize + 3) as u16,
                        );
                        match hit(&ed, x, y) {
                            Some(Hit::Place { line, col }) => ed.place(line, col),
                            Some(Hit::Box { line }) => {
                                ed.toggle_box(line);
                            }
                            None => {}
                        }
                    }
                    _ => ed.insert(alphabet[next(alphabet.len())]),
                }
                assert!(ed.row < ed.lines.len(), "round {round}");
                assert!(ed.col <= ed.lines[ed.row].chars().count(), "round {round}");
                t.draw(|f| render(f, f.area(), "t", &ed, true)).unwrap();
            }
        }
    }

    #[test]
    fn long_lines_wrap_without_losing_text() {
        let ed = Editor::open("id", "alpha beta gamma delta epsilon zeta");
        let out = draw(&ed, 16, 8);
        for word in ["alpha", "gamma", "epsilon", "zeta"] {
            assert!(out.contains(word), "{word}: {out}");
        }
    }
}
