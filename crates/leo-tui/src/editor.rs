use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};

use unicode_width::UnicodeWidthChar;

const GROUP: Duration = Duration::from_millis(800);
const HISTORY: usize = 200;
pub const AUTOSAVE: Duration = Duration::from_millis(1500);

#[derive(Clone)]
struct Snapshot {
    lines: Vec<String>,
    row: usize,
    col: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    pub line: usize,
    pub start: usize,
    pub raw: bool,
}

pub struct Editor {
    pub id: String,
    pub lines: Vec<String>,
    pub row: usize,
    pub col: usize,
    pub dirty: bool,
    pub last_edit: Option<Instant>,
    pub scroll: Cell<usize>,
    pub width: Cell<usize>,
    pub height: Cell<usize>,
    pub follow: Cell<bool>,
    pub layout: RefCell<Vec<Row>>,
    pub origin: Cell<(u16, u16)>,
    want_x: Option<usize>,
    history: Vec<Snapshot>,
    grouped_at: Option<Instant>,
}

#[derive(Debug, PartialEq, Eq)]
enum Next {
    Prefix(String),
    Exit,
    Plain,
}

pub fn chars(s: &str) -> usize {
    s.chars().count()
}

fn byte_at(s: &str, col: usize) -> usize {
    s.char_indices().nth(col).map_or(s.len(), |(i, _)| i)
}

fn width_of(c: char) -> usize {
    c.width().unwrap_or(0)
}

pub fn breaks(text: &str, width: usize) -> Vec<usize> {
    let width = width.max(1);
    let all: Vec<char> = text.chars().collect();
    let mut starts = vec![0];
    let mut row_start = 0;
    let mut used = 0;
    let mut last_space: Option<usize> = None;
    for (i, &c) in all.iter().enumerate() {
        let w = width_of(c);
        if used + w > width && i > row_start {
            let brk = match last_space {
                Some(sp) if sp + 1 > row_start && sp < i => sp + 1,
                _ => i,
            };
            starts.push(brk);
            row_start = brk;
            used = all[brk..i].iter().map(|&c| width_of(c)).sum();
            last_space = all[brk..i].iter().rposition(|&c| c == ' ').map(|p| p + brk);
        }
        used += w;
        if c == ' ' {
            last_space = Some(i);
        }
    }
    starts
}

pub fn cursor_xy(text: &str, col: usize, width: usize) -> (usize, usize) {
    let starts = breaks(text, width);
    let y = starts.iter().rposition(|&s| s <= col).unwrap_or(0);
    let x = text
        .chars()
        .skip(starts[y])
        .take(col.saturating_sub(starts[y]))
        .map(width_of)
        .sum();
    (x, y)
}

pub fn col_at(text: &str, width: usize, y: usize, x: usize) -> usize {
    let starts = breaks(text, width);
    let y = y.min(starts.len() - 1);
    let total = chars(text);
    let end = if y + 1 < starts.len() {
        starts[y + 1].saturating_sub(1).max(starts[y])
    } else {
        total
    };
    let mut used = 0;
    for (i, c) in text.chars().enumerate().skip(starts[y]) {
        if i >= end {
            return end;
        }
        let w = width_of(c);
        if used + w > x {
            return i;
        }
        used += w;
    }
    end
}

fn continuation(before: &str) -> Next {
    let indent: String = before
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect();
    let rest = &before[indent.len()..];
    for marker in ['-', '*', '+'] {
        if let Some(after) = rest.strip_prefix(marker) {
            let Some(content) = after.strip_prefix(' ') else {
                continue;
            };
            let boxed = content.trim_start();
            let inner = boxed
                .strip_prefix("[ ]")
                .or_else(|| boxed.strip_prefix("[x]"))
                .or_else(|| boxed.strip_prefix("[X]"));
            if let Some(inner) = inner {
                return if inner.trim().is_empty() {
                    Next::Exit
                } else {
                    Next::Prefix(format!("{indent}{marker} [ ] "))
                };
            }
            return if content.trim().is_empty() {
                Next::Exit
            } else {
                Next::Prefix(format!("{indent}{marker} "))
            };
        }
    }
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if !digits.is_empty() {
        let after = &rest[digits.len()..];
        if let Some(dot) = after.chars().next().filter(|c| *c == '.' || *c == ')') {
            if let Some(content) = after[1..].strip_prefix(' ') {
                let next = digits.parse::<u64>().unwrap_or(0) + 1;
                return if content.trim().is_empty() {
                    Next::Exit
                } else {
                    Next::Prefix(format!("{indent}{next}{dot} "))
                };
            }
        }
    }
    if let Some(after) = rest.strip_prefix('>') {
        let content = after.strip_prefix(' ').unwrap_or(after);
        return if content.trim().is_empty() {
            Next::Exit
        } else {
            Next::Prefix(format!("{indent}> "))
        };
    }
    Next::Plain
}

pub fn toggle_box_line(line: &str) -> Option<String> {
    let indent = line.len() - line.trim_start().len();
    let rest = &line[indent..];
    let mark = rest.strip_prefix("- [")?;
    let state = mark.chars().next()?;
    let tail = mark[state.len_utf8()..].strip_prefix("] ")?;
    let flipped = match state {
        ' ' => 'x',
        'x' | 'X' => ' ',
        _ => return None,
    };
    Some(format!("{}- [{flipped}] {tail}", &line[..indent]))
}

impl Editor {
    pub fn open(id: &str, body: &str) -> Editor {
        let lines: Vec<String> = body
            .replace("\r\n", "\n")
            .split('\n')
            .map(str::to_string)
            .collect();
        let row = lines.len() - 1;
        let col = chars(&lines[row]);
        Editor {
            id: id.to_string(),
            lines,
            row,
            col,
            dirty: false,
            last_edit: None,
            scroll: Cell::new(0),
            width: Cell::new(80),
            height: Cell::new(20),
            follow: Cell::new(true),
            layout: RefCell::new(Vec::new()),
            origin: Cell::new((0, 0)),
            want_x: None,
            history: Vec::new(),
            grouped_at: None,
        }
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    fn line(&self) -> &str {
        &self.lines[self.row]
    }

    fn snapshot(&mut self, fresh: bool) {
        let now = Instant::now();
        let new_group = fresh
            || self
                .grouped_at
                .is_none_or(|t| now.duration_since(t) > GROUP);
        if new_group {
            self.history.push(Snapshot {
                lines: self.lines.clone(),
                row: self.row,
                col: self.col,
            });
            if self.history.len() > HISTORY {
                self.history.remove(0);
            }
        }
        self.grouped_at = if fresh { None } else { Some(now) };
    }

    fn touched(&mut self) {
        self.dirty = true;
        self.last_edit = Some(Instant::now());
        self.want_x = None;
        self.follow.set(true);
    }

    fn moved(&mut self) {
        self.grouped_at = None;
        self.follow.set(true);
    }

    pub fn insert(&mut self, c: char) {
        self.snapshot(c == ' ');
        let at = byte_at(self.line(), self.col);
        self.lines[self.row].insert(at, c);
        self.col += 1;
        self.touched();
    }

    pub fn paste(&mut self, text: &str) {
        self.snapshot(true);
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        let at = byte_at(self.line(), self.col);
        let tail = self.lines[self.row].split_off(at);
        let mut parts = text.split('\n');
        let first = parts.next().unwrap_or("");
        self.lines[self.row].push_str(first);
        self.col += chars(first);
        for part in parts {
            self.row += 1;
            self.lines.insert(self.row, part.to_string());
            self.col = chars(part);
        }
        self.lines[self.row].push_str(&tail);
        self.touched();
    }

    pub fn enter(&mut self) {
        self.snapshot(true);
        let at = byte_at(self.line(), self.col);
        let after = self.lines[self.row].split_off(at);
        let before = self.lines[self.row].clone();
        match continuation(&before) {
            Next::Exit if after.trim().is_empty() => {
                self.lines[self.row] = String::new();
                self.col = 0;
            }
            Next::Prefix(prefix) => {
                self.col = chars(&prefix);
                self.lines.insert(self.row + 1, prefix + &after);
                self.row += 1;
            }
            _ => {
                self.lines.insert(self.row + 1, after);
                self.row += 1;
                self.col = 0;
            }
        }
        self.touched();
    }

    pub fn backspace(&mut self) {
        if self.col > 0 {
            self.snapshot(false);
            let start = byte_at(self.line(), self.col - 1);
            let end = byte_at(self.line(), self.col);
            self.lines[self.row].replace_range(start..end, "");
            self.col -= 1;
            self.touched();
        } else if self.row > 0 {
            self.snapshot(true);
            let line = self.lines.remove(self.row);
            self.row -= 1;
            self.col = chars(&self.lines[self.row]);
            self.lines[self.row].push_str(&line);
            self.touched();
        }
    }

    pub fn delete(&mut self) {
        if self.col < chars(self.line()) {
            self.snapshot(false);
            let start = byte_at(self.line(), self.col);
            let end = byte_at(self.line(), self.col + 1);
            self.lines[self.row].replace_range(start..end, "");
            self.touched();
        } else if self.row + 1 < self.lines.len() {
            self.snapshot(true);
            let next = self.lines.remove(self.row + 1);
            self.lines[self.row].push_str(&next);
            self.touched();
        }
    }

    pub fn left(&mut self) {
        if self.col > 0 {
            self.col -= 1;
        } else if self.row > 0 {
            self.row -= 1;
            self.col = chars(self.line());
        }
        self.want_x = None;
        self.moved();
    }

    pub fn right(&mut self) {
        if self.col < chars(self.line()) {
            self.col += 1;
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = 0;
        }
        self.want_x = None;
        self.moved();
    }

    pub fn home(&mut self) {
        self.col = 0;
        self.want_x = None;
        self.moved();
    }

    pub fn end(&mut self) {
        self.col = chars(self.line());
        self.want_x = None;
        self.moved();
    }

    pub fn up(&mut self) {
        let width = self.width.get();
        let (x, y) = cursor_xy(self.line(), self.col, width);
        let want = *self.want_x.get_or_insert(x);
        if y > 0 {
            self.col = col_at(self.line(), width, y - 1, want);
        } else if self.row > 0 {
            self.row -= 1;
            let last = breaks(self.line(), width).len() - 1;
            self.col = col_at(self.line(), width, last, want);
        } else {
            self.col = 0;
        }
        self.moved();
    }

    pub fn down(&mut self) {
        let width = self.width.get();
        let (x, y) = cursor_xy(self.line(), self.col, width);
        let want = *self.want_x.get_or_insert(x);
        let rows = breaks(self.line(), width).len();
        if y + 1 < rows {
            self.col = col_at(self.line(), width, y + 1, want);
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = col_at(self.line(), width, 0, want);
        } else {
            self.col = chars(self.line());
        }
        self.moved();
    }

    pub fn page(&mut self, down: bool) {
        for _ in 0..self.height.get().saturating_sub(1).max(1) {
            if down {
                self.down();
            } else {
                self.up();
            }
        }
    }

    pub fn indent(&mut self) {
        self.snapshot(true);
        self.lines[self.row].insert_str(0, "  ");
        self.col += 2;
        self.touched();
    }

    pub fn outdent(&mut self) {
        let spaces = self
            .line()
            .chars()
            .take(2)
            .take_while(|c| *c == ' ')
            .count();
        if spaces == 0 {
            return;
        }
        self.snapshot(true);
        self.lines[self.row].replace_range(..spaces, "");
        self.col = self.col.saturating_sub(spaces);
        self.touched();
    }

    pub fn undo(&mut self) -> bool {
        let Some(last) = self.history.pop() else {
            return false;
        };
        self.lines = last.lines;
        self.row = last.row.min(self.lines.len() - 1);
        self.col = last.col.min(chars(self.line()));
        self.grouped_at = None;
        self.touched();
        true
    }

    pub fn toggle_box(&mut self, line: usize) -> bool {
        let Some(flipped) = self.lines.get(line).and_then(|l| toggle_box_line(l)) else {
            return false;
        };
        self.snapshot(true);
        self.lines[line] = flipped;
        self.touched();
        true
    }

    pub fn place(&mut self, row: usize, col: usize) {
        self.row = row.min(self.lines.len() - 1);
        self.col = col.min(chars(self.line()));
        self.want_x = None;
        self.moved();
    }

    pub fn saved(&mut self) {
        self.dirty = false;
    }

    pub fn wants_saving(&self) -> bool {
        self.dirty && self.last_edit.is_some_and(|t| t.elapsed() >= AUTOSAVE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at_end(body: &str) -> Editor {
        Editor::open("id", body)
    }

    fn typed(ed: &mut Editor, text: &str) {
        for c in text.chars() {
            ed.insert(c);
        }
    }

    #[test]
    fn opening_puts_the_cursor_at_the_end() {
        let ed = at_end("one\ntwo");
        assert_eq!((ed.row, ed.col), (1, 3));
        let empty = at_end("");
        assert_eq!(empty.lines, vec![""]);
        assert_eq!((empty.row, empty.col), (0, 0));
    }

    #[test]
    fn typing_inserts_at_the_cursor_and_marks_it_dirty() {
        let mut ed = at_end("ab");
        ed.left();
        typed(&mut ed, "X");
        assert_eq!(ed.text(), "aXb");
        assert!(ed.dirty);
    }

    #[test]
    fn enter_continues_lists_checklists_numbers_and_quotes() {
        for (line, next) in [
            ("- milk", "- "),
            ("  * nested", "  * "),
            ("- [x] eggs", "- [ ] "),
            ("- [ ] bread", "- [ ] "),
            ("9. ninth", "10. "),
            ("1) first", "2) "),
            ("> quoted", "> "),
        ] {
            let mut ed = at_end(line);
            ed.enter();
            assert_eq!(ed.lines, vec![line.to_string(), next.to_string()], "{line}");
            assert_eq!((ed.row, ed.col), (1, chars(next)), "{line}");
        }
    }

    #[test]
    fn enter_on_an_empty_item_ends_the_list() {
        for line in ["- ", "- [ ] ", "3. ", "> "] {
            let mut ed = at_end(&format!("- a\n{line}"));
            ed.enter();
            assert_eq!(ed.lines, vec!["- a".to_string(), String::new()], "{line:?}");
            assert_eq!((ed.row, ed.col), (1, 0));
        }
    }

    #[test]
    fn enter_in_plain_text_splits_the_line() {
        let mut ed = at_end("hello world");
        ed.place(0, 5);
        ed.enter();
        assert_eq!(ed.lines, vec!["hello", " world"]);
        assert_eq!((ed.row, ed.col), (1, 0));
        for line in ["**bold**", "---", "-dash"] {
            let mut ed = at_end(line);
            ed.enter();
            assert_eq!(ed.lines[1], "", "{line}");
        }
    }

    #[test]
    fn backspace_at_the_start_of_a_line_joins_it_to_the_one_above() {
        let mut ed = at_end("one\ntwo");
        ed.home();
        ed.backspace();
        assert_eq!(ed.lines, vec!["onetwo"]);
        assert_eq!((ed.row, ed.col), (0, 3));
    }

    #[test]
    fn delete_at_the_end_joins_the_next_line() {
        let mut ed = at_end("one\ntwo");
        ed.place(0, 3);
        ed.delete();
        assert_eq!(ed.lines, vec!["onetwo"]);
    }

    #[test]
    fn editing_is_safe_with_wide_and_multibyte_characters() {
        let mut ed = at_end("세계 é");
        ed.backspace();
        ed.left();
        ed.backspace();
        typed(&mut ed, "界");
        assert_eq!(ed.text(), "세界 ");
    }

    #[test]
    fn tab_indents_and_shift_tab_outdents() {
        let mut ed = at_end("- item");
        ed.indent();
        assert_eq!(ed.text(), "  - item");
        assert_eq!(ed.col, 8);
        ed.outdent();
        assert_eq!(ed.text(), "- item");
        assert_eq!(ed.col, 6);
        ed.outdent();
        assert_eq!(ed.text(), "- item");
    }

    #[test]
    fn undo_takes_back_a_burst_of_typing_at_once() {
        let mut ed = at_end("");
        typed(&mut ed, "hello");
        ed.enter();
        typed(&mut ed, "world");
        assert!(ed.undo());
        assert_eq!(ed.text(), "hello\n");
        assert!(ed.undo());
        assert_eq!(ed.text(), "hello");
        assert!(ed.undo());
        assert_eq!(ed.text(), "");
        assert!(!ed.undo());
    }

    #[test]
    fn pasting_several_lines_keeps_them_as_they_are() {
        let mut ed = at_end("a|b");
        ed.place(0, 2);
        ed.paste("- one\r\n- two");
        assert_eq!(ed.lines, vec!["a|- one", "- twob"]);
        assert_eq!((ed.row, ed.col), (1, 5));
    }

    #[test]
    fn a_box_toggles_both_ways_and_other_lines_are_left_alone() {
        assert_eq!(toggle_box_line("- [ ] milk").as_deref(), Some("- [x] milk"));
        assert_eq!(
            toggle_box_line("  - [X] eggs").as_deref(),
            Some("  - [ ] eggs")
        );
        assert_eq!(toggle_box_line("- milk"), None);
        let mut ed = at_end("- [ ] a\ntext");
        assert!(ed.toggle_box(0));
        assert!(!ed.toggle_box(1));
        assert_eq!(ed.text(), "- [x] a\ntext");
    }

    #[test]
    fn long_lines_wrap_at_spaces() {
        assert_eq!(breaks("hello big world", 10), vec![0, 10]);
        assert_eq!(breaks("abcdefghijkl", 5), vec![0, 5, 10]);
        assert_eq!(breaks("", 5), vec![0]);
        assert_eq!(breaks("세계세계", 4), vec![0, 2]);
    }

    #[test]
    fn up_and_down_move_between_wrapped_rows_and_keep_the_column() {
        let mut ed = at_end("short\nhello big world");
        ed.width.set(10);
        ed.place(1, 12);
        assert_eq!(cursor_xy(ed.line(), ed.col, 10), (2, 1));
        ed.up();
        assert_eq!((ed.row, ed.col), (1, 2));
        ed.up();
        assert_eq!((ed.row, ed.col), (0, 2));
        ed.down();
        ed.down();
        assert_eq!((ed.row, ed.col), (1, 12));
        ed.down();
        assert_eq!((ed.row, ed.col), (1, 15));
    }

    #[test]
    fn it_wants_saving_only_after_a_pause() {
        let mut ed = at_end("");
        assert!(!ed.wants_saving());
        ed.insert('a');
        assert!(!ed.wants_saving());
        ed.last_edit = Some(Instant::now() - AUTOSAVE);
        assert!(ed.wants_saving());
        ed.saved();
        assert!(!ed.wants_saving());
    }
}
