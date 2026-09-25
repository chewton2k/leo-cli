//! Markdown for the preview pane.
//!
//! Not a general markdown implementation and deliberately not a dependency: the
//! preview shows notes a person typed, so what matters is that structure is
//! *visible* — a heading looks like a heading, a checked box looks done, code
//! recedes. Anything unrecognised is shown as written rather than swallowed,
//! which is the property a general renderer tends not to have.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line as TuiLine, Span};

use super::theme;

/// A checked box, and an empty one. Symbols rather than `[x]` so a list scans at
/// a glance.
pub const BOX_DONE: &str = "☑ ";
pub const BOX_OPEN: &str = "☐ ";
/// Bullets, by nesting depth.
const BULLETS: [&str; 3] = ["• ", "◦ ", "‣ "];
/// Drawn down the left of a quote.
const QUOTE_BAR: &str = "▏";

/// Render a note body into styled lines.
///
/// Stateful because fenced code blocks span lines: inside a fence, nothing is
/// interpreted, which is the whole point of a fence.
pub fn render(body: &str) -> Vec<TuiLine<'static>> {
    let mut out = Vec::new();
    let mut in_fence = false;

    for raw in body.lines() {
        let trimmed = raw.trim_start();
        let indent = raw.len() - trimmed.len();

        // Fences toggle, and the fence line itself is not shown: it is syntax,
        // not content.
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            let language = trimmed.trim_start_matches(['`', '~']).trim();
            if in_fence && !language.is_empty() {
                out.push(TuiLine::from(Span::styled(
                    format!("{}{language}", " ".repeat(indent)),
                    Style::default().fg(theme::accent_muted()),
                )));
            }
            continue;
        }

        if in_fence {
            out.push(TuiLine::from(Span::styled(
                format!("  {raw}"),
                Style::default().fg(theme::accent_muted()),
            )));
            continue;
        }

        out.push(line(trimmed, indent));
    }

    out
}

/// Render one line outside a code fence.
fn line(trimmed: &str, indent: usize) -> TuiLine<'static> {
    let pad = " ".repeat(indent);

    // A horizontal rule becomes an actual rule.
    if is_rule(trimmed) {
        return TuiLine::from(Span::styled(
            "─".repeat(48),
            Style::default().fg(theme::accent_muted()),
        ));
    }

    // Headings: the accent, bold, with the hashes dropped. Deeper levels are
    // bold but not coloured, so the hierarchy is visible without a rainbow.
    if let Some(level) = heading_level(trimmed) {
        let text = trimmed[level..].trim_start().to_string();
        let style = if level <= 2 {
            Style::default()
                .fg(theme::accent())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().add_modifier(Modifier::BOLD)
        };
        return TuiLine::from(vec![Span::raw(pad), Span::styled(text, style)]);
    }

    // A quote gets a bar and recedes.
    if let Some(rest) = trimmed.strip_prefix("> ").or(trimmed.strip_prefix(">")) {
        let mut spans = vec![
            Span::raw(pad),
            Span::styled(QUOTE_BAR.to_string(), Style::default().fg(theme::accent_muted())),
            Span::raw(" "),
        ];
        spans.extend(inline(rest, Style::default().add_modifier(Modifier::DIM)));
        return TuiLine::from(spans);
    }

    // Checkboxes, before bullets: `- [ ]` is also a bullet.
    if let Some((done, rest)) = checkbox(trimmed) {
        let (marker, body_style) = if done {
            (
                Span::styled(BOX_DONE.to_string(), Style::default().fg(theme::good())),
                Style::default().add_modifier(Modifier::DIM | Modifier::CROSSED_OUT),
            )
        } else {
            (
                Span::styled(BOX_OPEN.to_string(), Style::default().fg(theme::accent())),
                Style::default(),
            )
        };
        let mut spans = vec![Span::raw(pad), marker];
        spans.extend(inline(rest, body_style));
        return TuiLine::from(spans);
    }

    // Bullets, with the marker in the accent and the depth from the indent.
    if let Some(rest) = bullet(trimmed) {
        let depth = (indent / 2).min(BULLETS.len() - 1);
        let mut spans = vec![
            Span::raw(pad),
            Span::styled(
                BULLETS[depth].to_string(),
                Style::default().fg(theme::accent()),
            ),
        ];
        spans.extend(inline(rest, Style::default()));
        return TuiLine::from(spans);
    }

    // Numbered lists keep their numbers, which carry meaning a bullet would lose.
    if let Some((number, rest)) = ordered(trimmed) {
        let mut spans = vec![
            Span::raw(pad),
            Span::styled(
                format!("{number}. "),
                Style::default().fg(theme::accent()),
            ),
        ];
        spans.extend(inline(rest, Style::default()));
        return TuiLine::from(spans);
    }

    // A prompt waiting to be expanded, which is worth spotting.
    if trimmed.starts_with("@leo") {
        return TuiLine::from(vec![
            Span::raw(pad),
            Span::styled(
                trimmed.to_string(),
                Style::default()
                    .fg(theme::warn())
                    .add_modifier(Modifier::ITALIC),
            ),
        ]);
    }

    let mut spans = vec![Span::raw(pad)];
    spans.extend(inline(trimmed, Style::default()));
    TuiLine::from(spans)
}

/// How many leading `#` a heading has, or `None`.
fn heading_level(trimmed: &str) -> Option<usize> {
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    // `#tag` is not a heading; a heading has a space after the hashes.
    let followed_by_space = trimmed[hashes..].starts_with(' ');
    (1..=6).contains(&hashes).then_some(hashes).filter(|_| followed_by_space)
}

/// `---`, `***`, `___`, three or more, spaced or not.
///
/// Also catches the `---` that opens and closes a note's frontmatter, which is
/// the right thing to draw for it anyway.
fn is_rule(trimmed: &str) -> bool {
    let cleaned: String = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
    cleaned.len() >= 3
        && ['*', '_', '-']
            .iter()
            .any(|marker| cleaned.chars().all(|c| c == *marker))
}

/// A task item: whether it is done, and the text after the box.
fn checkbox(trimmed: &str) -> Option<(bool, &str)> {
    for marker in ["- ", "* ", "+ "] {
        if let Some(rest) = trimmed.strip_prefix(marker) {
            if let Some(inner) = rest.strip_prefix("[ ]") {
                return Some((false, inner.trim_start()));
            }
            for done in ["[x]", "[X]"] {
                if let Some(inner) = rest.strip_prefix(done) {
                    return Some((true, inner.trim_start()));
                }
            }
        }
    }
    None
}

/// An unordered list item's text.
fn bullet(trimmed: &str) -> Option<&str> {
    for marker in ["- ", "* ", "+ "] {
        if let Some(rest) = trimmed.strip_prefix(marker) {
            return Some(rest);
        }
    }
    None
}

/// An ordered list item's number and text.
fn ordered(trimmed: &str) -> Option<(&str, &str)> {
    let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let rest = &trimmed[digits..];
    let rest = rest.strip_prefix(". ").or(rest.strip_prefix(") "))?;
    Some((&trimmed[..digits], rest))
}

/// Split inline markup into spans: `**bold**`, `*italic*`, `_italic_`, `` `code` ``.
///
/// The delimiters are dropped, so the text reads as it was meant to. Unmatched
/// delimiters are left as typed rather than eating the rest of the line, which is
/// what makes this safe on prose containing a stray asterisk.
fn inline(text: &str, base: Style) -> Vec<Span<'static>> {
    /// Markers in match order: `**` before `*`, or bold would parse as two
    /// empty italics.
    const MARKERS: [(&str, Emphasis); 4] = [
        ("**", Emphasis::Bold),
        ("`", Emphasis::Code),
        ("*", Emphasis::Italic),
        ("_", Emphasis::Italic),
    ];

    let mut spans = Vec::new();
    let mut plain = String::new();
    let mut rest = text;
    // Whether the previous character was part of a word. `_` inside a word is
    // an identifier, not emphasis — `snake_case_name` must survive intact.
    let mut prev_alnum = false;

    while !rest.is_empty() {
        let matched = MARKERS.iter().find_map(|(marker, kind)| {
            if *marker == "_" && prev_alnum {
                return None;
            }
            delimited(rest, marker).and_then(|(body, used)| {
                if *marker == "_" && rest[used..].starts_with(|c: char| c.is_alphanumeric()) {
                    return None;
                }
                Some((body, used, *kind))
            })
        });

        match matched {
            Some((body, used, kind)) => {
                flush(&mut spans, &mut plain, base);
                spans.push(Span::styled(body, kind.style(base)));
                rest = &rest[used..];
                prev_alnum = false;
            }
            None => {
                // Advance by one character, not one byte: a multi-byte character
                // would otherwise be split and panic.
                let next = rest.chars().next();
                let step = next.map(char::len_utf8).unwrap_or(1);
                plain.push_str(&rest[..step]);
                rest = &rest[step..];
                prev_alnum = next.is_some_and(char::is_alphanumeric);
            }
        }
    }

    flush(&mut spans, &mut plain, base);
    if spans.is_empty() {
        spans.push(Span::styled(String::new(), base));
    }
    spans
}

/// The kinds of inline emphasis, and how each looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Emphasis {
    Bold,
    Italic,
    Code,
}

impl Emphasis {
    fn style(self, base: Style) -> Style {
        match self {
            Emphasis::Bold => base.add_modifier(Modifier::BOLD),
            Emphasis::Italic => base.add_modifier(Modifier::ITALIC),
            // Code borrows the warn colour: it is the one palette entry that
            // reads as "literal" without competing with the accent.
            Emphasis::Code => base.fg(theme::warn()).add_modifier(Modifier::DIM),
        }
    }
}

/// The text between a matched pair of `marker`, and how many chars were consumed.
fn delimited(rest: &str, marker: &str) -> Option<(String, usize)> {
    let after = rest.strip_prefix(marker)?;
    // An empty pair (`****`) is not emphasis; treat it as literal text.
    let end = after.find(marker).filter(|e| *e > 0)?;
    let body = &after[..end];
    // Emphasis does not span a line break, and `a * b * c` is arithmetic.
    if body.starts_with(' ') || body.ends_with(' ') {
        return None;
    }
    // Byte length, because the caller slices by bytes.
    Some((body.to_string(), marker.len() * 2 + body.len()))
}

fn flush(spans: &mut Vec<Span<'static>>, plain: &mut String, base: Style) {
    if !plain.is_empty() {
        spans.push(Span::styled(std::mem::take(plain), base));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The visible text of a rendered line, with styling discarded.
    fn text(line: &TuiLine) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn one(input: &str) -> TuiLine<'static> {
        let mut lines = render(input);
        assert_eq!(lines.len(), 1, "expected one line from {input:?}");
        lines.remove(0)
    }

    #[test]
    fn a_heading_loses_its_hashes_and_gains_the_accent() {
        let line = one("## Ownership");
        assert_eq!(text(&line), "Ownership");
        let styled = line.spans.last().unwrap();
        assert_eq!(styled.style.fg, Some(theme::accent()));
        assert!(styled.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn deep_headings_are_bold_but_not_coloured() {
        let line = one("#### Details");
        assert_eq!(text(&line), "Details");
        let styled = line.spans.last().unwrap();
        assert_eq!(styled.style.fg, None);
        assert!(styled.style.add_modifier.contains(Modifier::BOLD));
    }

    /// A tag is not a heading. This is the case a naive `starts_with('#')` gets
    /// wrong, and notes are full of tags.
    #[test]
    fn a_tag_is_not_treated_as_a_heading() {
        let line = one("#reminder buy coffee");
        assert_eq!(text(&line), "#reminder buy coffee");
    }

    #[test]
    fn seven_hashes_are_not_a_heading() {
        assert_eq!(text(&one("####### too deep")), "####### too deep");
    }

    #[test]
    fn checkboxes_become_symbols_and_done_items_recede() {
        let open = one("- [ ] write tests");
        assert!(text(&open).starts_with(BOX_OPEN), "{:?}", text(&open));
        assert!(text(&open).contains("write tests"));

        let done = one("- [x] write tests");
        assert!(text(&done).starts_with(BOX_DONE));
        let body = done.spans.last().unwrap();
        assert!(body.style.add_modifier.contains(Modifier::CROSSED_OUT));
        assert!(body.style.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn an_uppercase_x_also_counts_as_done() {
        assert!(text(&one("- [X] done")).starts_with(BOX_DONE));
    }

    #[test]
    fn bullets_become_dots_and_nest_by_indent() {
        assert!(text(&one("- top")).contains(BULLETS[0]));
        assert!(text(&one("  - nested")).contains(BULLETS[1]));
        assert!(text(&one("    - deeper")).contains(BULLETS[2]));
        // Beyond the deepest marker it stays put rather than panicking.
        assert!(text(&one("            - very deep")).contains(BULLETS[2]));
    }

    #[test]
    fn asterisk_and_plus_are_bullets_too() {
        assert!(text(&one("* starred")).contains(BULLETS[0]));
        assert!(text(&one("+ plussed")).contains(BULLETS[0]));
    }

    #[test]
    fn numbered_lists_keep_their_numbers() {
        assert_eq!(text(&one("1. first")), "1. first");
        assert_eq!(text(&one("12) twelfth")), "12. twelfth");
    }

    #[test]
    fn a_code_fence_is_hidden_and_its_contents_are_left_alone() {
        let lines = render("```rust\nlet x = **not bold**;\n```");
        // The language label, then the code. The fences themselves are gone.
        assert_eq!(lines.len(), 2, "{:?}", lines.iter().map(text).collect::<Vec<_>>());
        assert_eq!(text(&lines[0]), "rust");
        assert!(
            text(&lines[1]).contains("**not bold**"),
            "markup inside a fence was interpreted: {:?}",
            text(&lines[1])
        );
    }

    #[test]
    fn an_unlabelled_fence_shows_only_its_contents() {
        let lines = render("```\nplain\n```");
        assert_eq!(lines.len(), 1);
        assert!(text(&lines[0]).contains("plain"));
    }

    /// An unclosed fence must not swallow the rest of the note.
    #[test]
    fn an_unclosed_fence_still_shows_its_contents() {
        let lines = render("```\none\ntwo");
        assert_eq!(lines.len(), 2);
        assert!(text(&lines[0]).contains("one"));
        assert!(text(&lines[1]).contains("two"));
    }

    #[test]
    fn emphasis_drops_its_delimiters() {
        assert_eq!(text(&one("**bold** here")), "bold here");
        assert_eq!(text(&one("*italic* here")), "italic here");
        assert_eq!(text(&one("_italic_ here")), "italic here");
        assert_eq!(text(&one("use `code` now")), "use code now");
    }

    #[test]
    fn emphasis_carries_the_right_style() {
        let line = one("**bold**");
        assert!(line.spans[1].style.add_modifier.contains(Modifier::BOLD));
        let line = one("`code`");
        assert_eq!(line.spans[1].style.fg, Some(theme::warn()));
    }

    /// The property that makes this safe on prose: a stray delimiter is shown as
    /// typed rather than eating everything after it.
    #[test]
    fn unmatched_delimiters_are_left_as_written() {
        assert_eq!(text(&one("2 * 3 * 4")), "2 * 3 * 4");
        assert_eq!(text(&one("a lone * asterisk")), "a lone * asterisk");
        assert_eq!(text(&one("unclosed **bold")), "unclosed **bold");
        assert_eq!(text(&one("snake_case_name")), "snake_case_name");
        assert_eq!(text(&one("_leading underscore")), "_leading underscore");
        // Two asterisks alone are not a pair. Four *are* a rule, tested below.
        assert_eq!(text(&one("**")), "**");
    }

    #[test]
    fn a_quote_gets_a_bar_and_recedes() {
        let line = one("> quoted");
        assert!(text(&line).contains(QUOTE_BAR));
        assert!(text(&line).contains("quoted"));
        assert!(line
            .spans
            .last()
            .unwrap()
            .style
            .add_modifier
            .contains(Modifier::DIM));
    }

    #[test]
    fn rules_become_rules() {
        for rule in ["---", "***", "___", "- - -", "****", "-----"] {
            let line = one(rule);
            assert!(
                text(&line).starts_with('─'),
                "{rule:?} did not become a rule: {:?}",
                text(&line)
            );
        }
    }

    #[test]
    fn an_unexpanded_prompt_stands_out() {
        let line = one("@leo what is a monad?");
        assert_eq!(text(&line), "@leo what is a monad?");
        let span = line.spans.last().unwrap();
        assert_eq!(span.style.fg, Some(theme::warn()));
        assert!(span.style.add_modifier.contains(Modifier::ITALIC));
    }

    /// Nothing may be silently dropped: a note is the user's own writing.
    #[test]
    fn plain_text_survives_unchanged() {
        for input in [
            "just a sentence",
            "trailing spaces   ",
            "unicode: café — naïve 日本語",
            "punctuation!? (parens) [brackets] {braces}",
            "a#hash mid-word",
        ] {
            assert_eq!(text(&one(input)), input.trim_end_matches('\n'));
        }
    }

    #[test]
    fn indentation_is_preserved() {
        let line = one("    indented text");
        assert!(text(&line).starts_with("    "), "{:?}", text(&line));
    }

    #[test]
    fn an_empty_body_renders_nothing_rather_than_panicking() {
        assert!(render("").is_empty());
    }

    #[test]
    fn a_blank_line_stays_blank() {
        let lines = render("a\n\nb");
        assert_eq!(lines.len(), 3);
        assert_eq!(text(&lines[1]), "");
    }

    /// A realistic note, end to end: every line accounted for, nothing lost.
    #[test]
    fn a_whole_note_renders_line_for_line() {
        let body = "\
# Graph traversal

BFS explores **level by level**.

- [x] read the chapter
- [ ] write notes
  - nested point

```python
def bfs(g): pass
```

> worth remembering

1. queue for BFS
2. stack for DFS

@leo compare with Dijkstra";
        let lines = render(body);
        let rendered: Vec<String> = lines.iter().map(text).collect();

        // Two fence lines removed, one language label added: same count.
        assert_eq!(lines.len(), body.lines().count() - 1, "{rendered:#?}");
        assert!(rendered.iter().any(|l| l == "Graph traversal"));
        assert!(rendered.iter().any(|l| l.contains("level by level")));
        assert!(rendered.iter().any(|l| l.starts_with(BOX_DONE)));
        assert!(rendered.iter().any(|l| l.starts_with(BOX_OPEN)));
        assert!(rendered.iter().any(|l| l.contains("def bfs")));
        assert!(rendered.iter().any(|l| l.contains(QUOTE_BAR)));
        assert!(rendered.iter().any(|l| l == "1. queue for BFS"));
        assert!(rendered.iter().any(|l| l.contains("@leo")));
    }
}
