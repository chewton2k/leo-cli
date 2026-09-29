const MAX_CHARS: usize = 100;
const MAX_BYTES: usize = 200;
const FORBIDDEN: &str = "/\\:*?\"<>|";

fn is_edge(c: char) -> bool {
    c == ' ' || c == '.'
}

pub fn file_name(title: &str) -> String {
    let mut cleaned = String::with_capacity(title.len());
    let mut after_space = true;
    for c in title.chars() {
        if c == ':' {
            if !after_space {
                cleaned.push(' ');
            }
            cleaned.push_str("- ");
            after_space = true;
            continue;
        }
        if c.is_whitespace() {
            if !after_space {
                cleaned.push(' ');
                after_space = true;
            }
            continue;
        }
        cleaned.push(if c.is_control() || FORBIDDEN.contains(c) {
            '-'
        } else {
            c
        });
        after_space = false;
    }

    let mut cut = String::new();
    for (chars, c) in cleaned.trim_matches(is_edge).chars().enumerate() {
        if chars == MAX_CHARS || cut.len() + c.len_utf8() > MAX_BYTES {
            break;
        }
        cut.push(c);
    }
    let name = cut.trim_matches(is_edge);
    if name.is_empty() {
        "Untitled".to_string()
    } else {
        name.to_string()
    }
}

pub fn numbered(stem: &str, n: usize) -> String {
    if n <= 1 {
        stem.to_string()
    } else {
        format!("{stem} ({n})")
    }
}

fn strip_number(stem: &str) -> Option<&str> {
    let open = stem.rfind(" (")?;
    let digits = stem[open + 2..].strip_suffix(')')?;
    (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())).then_some(&stem[..open])
}

pub fn matches_title(stem: &str, title: &str) -> bool {
    let base = file_name(title);
    stem == base || strip_number(stem) == Some(base.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_title_is_its_own_file_name() {
        assert_eq!(file_name("Lecture 4"), "Lecture 4");
        assert_eq!(file_name("Graph search — BFS"), "Graph search — BFS");
    }

    #[test]
    fn characters_no_file_system_allows_become_dashes() {
        assert_eq!(file_name("a/b\\c*e?f\"g<h>i|j"), "a-b-c-e-f-g-h-i-j");
        assert_eq!(file_name("tab\there"), "tab here");
        assert_eq!(file_name("bell\u{7}x"), "bell-x");
    }

    #[test]
    fn a_colon_reads_as_a_dash_between_words() {
        assert_eq!(
            file_name("Lecture 4: Graph search"),
            "Lecture 4 - Graph search"
        );
        assert_eq!(file_name("Q:A"), "Q - A");
        assert_eq!(file_name(": leading"), "- leading");
    }

    #[test]
    fn edges_are_trimmed_and_spaces_collapse() {
        assert_eq!(file_name("  many   spaces  "), "many spaces");
        assert_eq!(file_name(".hidden"), "hidden");
        assert_eq!(file_name("v1.2."), "v1.2");
        assert_eq!(file_name("..."), "Untitled");
        assert_eq!(file_name(""), "Untitled");
        assert_eq!(file_name("   "), "Untitled");
    }

    #[test]
    fn a_long_title_is_cut_by_characters() {
        let title = "x".repeat(300);
        assert_eq!(file_name(&title).chars().count(), 100);
    }

    #[test]
    fn a_long_title_in_wide_characters_is_cut_by_bytes_on_a_character_boundary() {
        let title = "笔".repeat(300);
        let name = file_name(&title);
        assert!(name.len() <= 200, "{}", name.len());
        assert!(name.chars().all(|c| c == '笔'));
        assert_eq!(name.chars().count(), 66);
    }

    #[test]
    fn a_trailing_space_left_by_the_cut_is_removed() {
        let title = format!("{} tail", "x".repeat(99));
        assert_eq!(file_name(&title), "x".repeat(99));
    }

    #[test]
    fn numbering_starts_at_two() {
        assert_eq!(numbered("Lecture", 1), "Lecture");
        assert_eq!(numbered("Lecture", 2), "Lecture (2)");
        assert_eq!(numbered("Lecture", 10), "Lecture (10)");
    }

    #[test]
    fn a_stem_matches_its_title_with_or_without_a_number() {
        assert!(matches_title("Lecture 4", "Lecture 4"));
        assert!(matches_title("Lecture 4 (2)", "Lecture 4"));
        assert!(matches_title("a-b", "a/b"));
        assert!(!matches_title("Lecture 5", "Lecture 4"));
        assert!(!matches_title("Lecture 4 (two)", "Lecture 4"));
        assert!(!matches_title("Lecture 4 ()", "Lecture 4"));
    }
}
