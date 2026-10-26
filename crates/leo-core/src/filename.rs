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
    } else if reserved_on_windows(name) {
        let (stem, rest) = name.split_at(name.find('.').unwrap_or(name.len()));
        format!("{stem}-{rest}")
    } else {
        name.to_string()
    }
}

pub fn reserved_on_windows(name: &str) -> bool {
    let stem = name
        .split('.')
        .next()
        .unwrap_or(name)
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    if matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) {
        return true;
    }
    let number = stem
        .strip_prefix("COM")
        .or_else(|| stem.strip_prefix("LPT"));
    matches!(
        number,
        Some("0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³")
    )
}

pub fn portable_folder(name: &str) -> bool {
    !name.ends_with(['.', ' '])
        && !name.contains(['*', '?', '"', '<', '>', '|'])
        && !reserved_on_windows(name)
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

const GENERIC: [&str; 31] = [
    "img",
    "image",
    "dsc",
    "dscn",
    "pxl",
    "photo",
    "pic",
    "screenshot",
    "screen",
    "scan",
    "scanned",
    "document",
    "doc",
    "untitled",
    "download",
    "file",
    "upload",
    "new",
    "copy",
    "whatsapp",
    "signal",
    "telegram",
    "camscanner",
    "recording",
    "audio",
    "video",
    "vid",
    "mov",
    "export",
    "attachment",
    "tmp",
];
const DATE_WORDS: [&str; 16] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "sept", "oct", "nov", "dec",
    "am", "pm", "at",
];

fn squashed(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

pub fn tracking_label(text: &str) -> Option<String> {
    let label = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let words: Vec<String> = label
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    if words.is_empty() || words.len() > 8 || label.chars().count() > 60 {
        return None;
    }
    let lettered: Vec<&String> = words
        .iter()
        .filter(|w| w.chars().any(char::is_alphabetic))
        .collect();
    let numbered = words.iter().any(|w| w.chars().any(|c| c.is_ascii_digit()));
    let hashed = words.iter().any(|w| {
        w.len() >= 12
            && w.chars().all(|c| c.is_ascii_hexdigit())
            && w.chars().any(|c| c.is_ascii_digit())
    });
    let generic = GENERIC.iter().any(|g| {
        words[0] == *g
            || words[0]
                .strip_prefix(g)
                .is_some_and(|rest| rest.chars().all(|c| c.is_ascii_digit()))
    });
    let dated = lettered.iter().all(|w| {
        DATE_WORDS
            .iter()
            .any(|d| w.trim_end_matches(|c: char| c.is_ascii_digit()) == *d)
    });
    (numbered && !lettered.is_empty() && !hashed && !generic && !dated).then_some(label)
}

pub fn file_label(name: &str) -> Option<String> {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name).trim();
    let stem = match base.rsplit_once('.') {
        Some((stem, ext))
            if !stem.trim().is_empty()
                && ext.len() <= 5
                && ext.chars().all(|c| c.is_ascii_alphanumeric())
                && ext.chars().any(|c| c.is_ascii_alphabetic()) =>
        {
            stem
        }
        _ => base,
    };
    tracking_label(stem)
}

const SERIES: [&str; 22] = [
    "lecture",
    "lec",
    "week",
    "chapter",
    "unit",
    "module",
    "lab",
    "homework",
    "hw",
    "assignment",
    "pset",
    "set",
    "quiz",
    "exam",
    "midterm",
    "discussion",
    "tutorial",
    "recitation",
    "worksheet",
    "project",
    "lesson",
    "session",
];
const SERIES_LOOK: usize = 2_000;

fn numberish(word: &str) -> bool {
    let digits = word.trim_end_matches(|c: char| c.is_ascii_alphabetic());
    !digits.is_empty()
        && digits.len() <= 3
        && word.len() - digits.len() <= 1
        && digits.chars().all(|c| c.is_ascii_digit())
}

fn course_code(text: &str, words: &[(usize, usize, &str)], at: usize) -> Option<usize> {
    let code = |w: &str| {
        let letters =
            w.trim_end_matches(|c: char| c.is_ascii_alphanumeric() && !c.is_ascii_uppercase());
        let rest = &w[letters.len()..];
        (2..=5).contains(&letters.len())
            && letters.chars().all(|c| c.is_ascii_uppercase())
            && numberish(rest)
    };
    let shout = |w: &str| (2..=5).contains(&w.len()) && w.chars().all(|c| c.is_ascii_uppercase());
    let before = at.checked_sub(1)?;
    if text[words[before].1..words[at].0].chars().count() > 3 {
        return None;
    }
    if code(words[before].2) {
        return Some(words[before].0);
    }
    let first = before.checked_sub(1)?;
    (numberish(words[before].2) && shout(words[first].2)).then_some(words[first].0)
}

pub fn series_label(text: &str) -> Option<String> {
    let end = text
        .char_indices()
        .nth(SERIES_LOOK)
        .map_or(text.len(), |(i, _)| i);
    let text = &text[..end];
    let mut words = Vec::new();
    let mut start = None;
    for (i, c) in text
        .char_indices()
        .chain(std::iter::once((text.len(), ' ')))
    {
        match (c.is_alphanumeric(), start) {
            (true, None) => start = Some(i),
            (false, Some(from)) => {
                words.push((from, i, &text[from..i]));
                start = None;
            }
            _ => {}
        }
    }
    for (at, &(from, to, word)) in words.iter().enumerate() {
        let lower = word.to_lowercase();
        let glued = SERIES.iter().find_map(|k| {
            lower
                .strip_prefix(k)
                .filter(|rest| numberish(rest))
                .map(|_| to)
        });
        let next = words.get(at + 1).filter(|(n_from, _, n)| {
            SERIES.contains(&lower.as_str())
                && numberish(n)
                && text[to..*n_from]
                    .chars()
                    .all(|c| c == ' ' || c == '#' || c == '.' || c == ':')
                && text[to..*n_from].chars().count() <= 3
        });
        let Some(until) = glued.or(next.map(|n| n.1)) else {
            continue;
        };
        if lower == "set"
            && !words[..at]
                .last()
                .is_some_and(|w| w.2.eq_ignore_ascii_case("problem"))
        {
            continue;
        }
        let begin = if lower == "set" {
            words[at - 1].0
        } else {
            from
        };
        let begin =
            course_code(text, &words, if lower == "set" { at - 1 } else { at }).unwrap_or(begin);
        return Some(
            text[begin..until]
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
        );
    }
    None
}

pub fn with_label(title: &str, label: Option<&str>) -> String {
    let title = title.trim();
    let Some(label) = label.map(str::trim).filter(|l| !l.is_empty()) else {
        return title.to_string();
    };
    let (have, want) = (squashed(title), squashed(label));
    if have.contains(&want) {
        title.to_string()
    } else if have.is_empty() || want.contains(&have) {
        label.to_string()
    } else {
        format!("{label} – {title}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_windows_keeps_for_devices_get_a_dash() {
        assert_eq!(file_name("CON"), "CON-");
        assert_eq!(file_name("aux"), "aux-");
        assert_eq!(file_name("nul.backup"), "nul-.backup");
        assert_eq!(file_name("COM1"), "COM1-");
        assert_eq!(file_name("lpt²"), "lpt²-");
        assert_eq!(file_name("Console"), "Console");
        assert_eq!(file_name("COM10"), "COM10");
        assert!(matches_title("CON-", "CON"));
        assert!(matches_title("CON- (2)", "CON"));
    }

    #[test]
    fn folders_must_open_on_windows_too() {
        assert!(portable_folder("Week 1"));
        assert!(!portable_folder("aux"));
        assert!(!portable_folder("notes."));
        assert!(!portable_folder("notes "));
        assert!(!portable_folder("why?"));
    }

    #[test]
    fn course_and_assignment_names_are_kept_and_camera_or_date_names_are_not() {
        for (name, label) in [
            ("ma161_hw3.pdf", "ma161_hw3"),
            ("CS 61B Project 2 Spec.docx", "CS 61B Project 2 Spec"),
            ("Lecture 5 - Heaps.pptx", "Lecture 5 - Heaps"),
            ("week3-notes.md", "week3-notes"),
            (
                "folder/econ101 midterm review.pdf",
                "econ101 midterm review",
            ),
            ("Lec 3.2", "Lec 3.2"),
        ] {
            assert_eq!(file_label(name).as_deref(), Some(label), "{name}");
        }
        for name in [
            "IMG_2034.jpg",
            "Screenshot 2026-10-20 at 3.14.15 PM.png",
            "PXL_20261020_141522.jpg",
            "20261020_141522.jpg",
            "Scan 3.pdf",
            "Document1.docx",
            "WhatsApp Image 2026-10-20 at 09.12.33.jpeg",
            "Oct 20 2026.pdf",
            "syllabus.pdf",
            "3f2a9c41d8e07b6a5c4d.pdf",
            "the complete set of answers to every question in the twelfth chapter 12.pdf",
            ".pdf",
        ] {
            assert_eq!(file_label(name), None, "{name}");
        }
        assert_eq!(
            tracking_label("MA161  Lecture 12").as_deref(),
            Some("MA161 Lecture 12")
        );
        assert_eq!(tracking_label("Data/Algos Interview"), None);
    }

    #[test]
    fn lecture_week_and_homework_numbers_are_found_with_their_course_code() {
        for (text, label) in [
            ("Lecture 1\nIntroduction to proofs", "Lecture 1"),
            ("# CS 61B Lecture 3: Linked lists", "CS 61B Lecture 3"),
            ("MA161 Homework #4\nDue Friday", "MA161 Homework #4"),
            ("Welcome to week 2 of the course", "week 2"),
            ("Problem Set 5", "Problem Set 5"),
            ("ECON 101 — Chapter 7: Elasticity", "ECON 101 — Chapter 7"),
            ("CS 61B notes. Chapter 2", "Chapter 2"),
            ("hw3 solutions", "hw3"),
            ("Lab 2a: circuits", "Lab 2a"),
        ] {
            assert_eq!(series_label(text).as_deref(), Some(label), "{text}");
        }
        for text in [
            "Introduction to proofs",
            "We set 3 goals",
            "The lecture covered 2024 results",
            "",
        ] {
            assert_eq!(series_label(text), None, "{text}");
        }
        let late = format!("{} Lecture 9", "word ".repeat(1_000));
        assert_eq!(
            series_label(&late),
            None,
            "only the start of the material counts"
        );
    }

    #[test]
    fn a_title_keeps_its_label_once() {
        let label = Some("ma161_hw3");
        assert_eq!(
            with_label("Eigenvalues and diagonalisation", label),
            "ma161_hw3 – Eigenvalues and diagonalisation"
        );
        assert_eq!(
            with_label("MA161 HW3: Eigenvalues", label),
            "MA161 HW3: Eigenvalues"
        );
        assert_eq!(
            with_label("ma161_hw3 – Eigenvalues", label),
            "ma161_hw3 – Eigenvalues"
        );
        assert_eq!(with_label("", label), "ma161_hw3");
        assert_eq!(with_label("  ", label), "ma161_hw3");
        assert_eq!(
            with_label("Heaps", Some("Lecture 5 - Heaps")),
            "Lecture 5 - Heaps"
        );
        assert_eq!(with_label(" Heaps ", None), "Heaps");
    }

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
