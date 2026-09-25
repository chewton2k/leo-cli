//! Context-aware fuzzy completion for the `:` line.
//!
//! What can be completed depends on the verb and on the token's position
//! *relative to the end of the line*, not just its index: `mv 1 2 cs130` and
//! `check 1 3` both put a fixed slot last and let the note references occupy
//! everything before it. Candidates are always leo's own data — verbs,
//! directories, note titles, tags — so no filesystem completion is needed.
//!
//! The engine is a pure function of (line, cursor, sources), which makes the
//! whole position table table-testable.

use nucleo::pattern::{CaseMatching, Normalization, Pattern};
use nucleo::Matcher;

use crate::action::all_verb_words;

const SYNC_SUBS: &[&str] = &["init", "connect", "push", "pull", "status"];

/// A note as the completer sees it: the number the `:` line accepts, and the
/// title the user actually remembers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteChoice {
    pub number: usize,
    pub title: String,
}

/// Everything the completer can draw from. Built by the App from the store and
/// config; owned by the caller so this module needs neither.
#[derive(Debug, Default, Clone)]
pub struct Sources {
    /// Directories reachable from the current one, as `cd` would accept them.
    pub dirs: Vec<String>,
    /// Notes in the current numbering.
    pub notes: Vec<NoteChoice>,
    /// Tag names, without the `#`.
    pub tags: Vec<String>,
}

/// One completion result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    /// Character index where the replaced token starts.
    pub start: usize,
    /// Character index where it ends, always the cursor.
    pub end: usize,
    /// Matches, best first.
    pub matches: Vec<String>,
}

impl Completion {
    pub fn best(&self) -> Option<&String> {
        self.matches.first()
    }

    /// The part of the top match that has not been typed yet, for the inline
    /// ghost hint.
    pub fn ghost(&self, typed: &str) -> Option<String> {
        let best = self.best()?;
        // Only a prefix match can be shown ahead of the cursor; a fuzzy match
        // that reorders characters would render as nonsense.
        let rest = best.strip_prefix(typed)?;
        (!rest.is_empty()).then(|| rest.to_string())
    }
}

/// Split the line into tokens, keeping each token's character span so a
/// completion can replace exactly what was typed.
fn spans(line: &str) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    let mut start = None;
    let mut current = String::new();

    for (i, c) in line.chars().enumerate() {
        if c.is_whitespace() {
            if let Some(s) = start.take() {
                out.push((s, i, std::mem::take(&mut current)));
            }
        } else {
            if start.is_none() {
                start = Some(i);
            }
            current.push(c);
        }
    }
    if let Some(s) = start {
        out.push((s, line.chars().count(), current));
    }
    out
}

/// Which candidate list applies at the cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Source {
    Verbs,
    Dirs,
    Notes,
    Tags,
    Words(&'static [&'static str]),
    /// A note reference, but the same slot could also be the trailing argument
    /// — `export 1 md` while the cursor is on `1`. Notes rank first.
    NotesThen(Box<Source>),
    None,
}

/// Decide what to complete: the verb, the argument position, and how many
/// arguments already precede the cursor.
fn source_for(line: &str, cursor: usize) -> (Source, usize, usize) {
    let all = spans(line);
    // The token being typed is the one the cursor sits inside or just after.
    let current = all
        .iter()
        .enumerate()
        .find(|(_, (s, e, _))| cursor >= *s && cursor <= *e);

    let (index, (start, end, text)) = match current {
        Some((i, span)) => (i, span.clone()),
        // The cursor is in whitespace: a brand new token at the cursor.
        None => (all.len(), (cursor, cursor, String::new())),
    };

    // A `#tag` anywhere means tags, whatever the verb is.
    if text.starts_with('#') {
        return (Source::Tags, start + 1, end);
    }

    if index == 0 {
        return (Source::Verbs, start, end);
    }

    let verb = all[0].2.to_lowercase();
    // 1-based position among the arguments.
    let arg = index;

    let source = match verb.as_str() {
        "cd" | "mkdir" => Source::Dirs,

        "edit" | "e" | "delete" | "rm" | "ask" => Source::Notes,

        // The trailing slot is a checkbox number, which nothing can usefully
        // complete, so every position offers notes.
        "check" | "x" => Source::Notes,

        // Directory last, notes before it. While typing the first argument the
        // user is naming a note; later arguments could be either.
        "mv" => {
            if arg == 1 {
                Source::Notes
            } else {
                Source::NotesThen(Box::new(Source::Dirs))
            }
        }

        "sync" => {
            if arg == 1 {
                Source::Words(SYNC_SUBS)
            } else {
                Source::None
            }
        }

        "listen" => {
            // `listen add <note>` takes a note; a bare title takes nothing.
            if arg >= 2 && all[1].2.eq_ignore_ascii_case("add") {
                Source::Notes
            } else if arg == 1 {
                Source::Words(&["add", "--screen"])
            } else {
                Source::None
            }
        }

        _ => Source::None,
    };

    (source, start, end)
}

/// Expand a source into its candidate strings.
fn candidates(source: &Source, sources: &Sources) -> Vec<String> {
    match source {
        Source::Verbs => all_verb_words().iter().map(|s| s.to_string()).collect(),
        Source::Dirs => {
            let mut out = sources.dirs.clone();
            // Navigation targets that are not directories in the store.
            out.push("..".to_string());
            out.push("/".to_string());
            out
        }
        Source::Notes => sources
            .notes
            .iter()
            // The number is what gets submitted, but matching on the title is
            // what the user can actually remember, so offer "1 Rust ownership"
            // and strip the title when it is accepted.
            .map(|n| format!("{} {}", n.number, n.title))
            .collect(),
        Source::Tags => sources.tags.clone(),
        Source::Words(words) => words.iter().map(|s| s.to_string()).collect(),
        Source::NotesThen(other) => {
            let mut out = candidates(&Source::Notes, sources);
            out.extend(candidates(other, sources));
            out
        }
        Source::None => Vec::new(),
    }
}

/// Rank candidates against what has been typed. An empty pattern keeps the
/// natural order, which is the numbering for notes and help order for verbs.
fn rank(typed: &str, pool: Vec<String>) -> Vec<String> {
    if typed.is_empty() {
        return pool;
    }
    let mut matcher = Matcher::new(nucleo::Config::DEFAULT);
    let pattern = Pattern::parse(typed, CaseMatching::Ignore, Normalization::Smart);
    pattern
        .match_list(pool, &mut matcher)
        .into_iter()
        .map(|(item, _score)| item)
        .collect()
}

/// Complete the token at `cursor`.
pub fn complete(line: &str, cursor: usize, sources: &Sources) -> Completion {
    let (source, start, end) = source_for(line, cursor);
    let typed: String = line.chars().skip(start).take(end.saturating_sub(start)).collect();
    let matches = rank(&typed, candidates(&source, sources));
    Completion { start, end, matches }
}

/// Apply a chosen match to the line, returning the new line and cursor.
///
/// A note candidate carries its title for matching but only its number is a
/// valid argument, so the title is dropped on the way in.
pub fn apply(line: &str, completion: &Completion, choice: &str) -> (String, usize) {
    let choice = strip_note_title(choice);
    let prefix: String = line.chars().take(completion.start).collect();
    let suffix: String = line.chars().skip(completion.end).collect();
    let new_line = format!("{prefix}{choice}{suffix}");
    let cursor = completion.start + choice.chars().count();
    (new_line, cursor)
}

/// `"3 Rust ownership"` -> `"3"`. Anything not shaped like a numbered note is
/// returned unchanged.
fn strip_note_title(choice: &str) -> &str {
    match choice.split_once(' ') {
        Some((head, _)) if head.chars().all(|c| c.is_ascii_digit()) && !head.is_empty() => head,
        _ => choice,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sources() -> Sources {
        Sources {
            dirs: vec!["cs130".to_string(), "cs162".to_string(), "chem".to_string()],
            notes: vec![
                NoteChoice { number: 1, title: "Rust ownership".to_string() },
                NoteChoice { number: 2, title: "Graph traversals".to_string() },
                NoteChoice { number: 3, title: "Midterm plan".to_string() },
            ],
            tags: vec!["rust".to_string(), "reminder".to_string(), "learning".to_string()],
        }
    }

    /// Complete at the end of the line, which is where Tab is pressed.
    fn at_end(line: &str) -> Completion {
        complete(line, line.chars().count(), &sources())
    }

    fn matches(line: &str) -> Vec<String> {
        at_end(line).matches
    }

    // ── first word: verbs ───────────────────────────────────────────────────

    #[test]
    fn the_first_word_completes_verbs_and_aliases() {
        let m = matches("ren");
        assert_eq!(m.first().map(String::as_str), Some("rename"));

        // Aliases are completable too, since they are real input.
        assert!(matches("r").contains(&"rm".to_string()));
        // An empty line offers everything, in help order.
        assert_eq!(matches("").first().map(String::as_str), Some("new"));
    }

    #[test]
    fn a_verb_prefix_matching_nothing_yields_no_matches() {
        assert!(matches("zzzz").is_empty());
    }

    // ── directories ─────────────────────────────────────────────────────────

    #[test]
    fn cd_and_mkdir_complete_directories() {
        for verb in ["cd", "mkdir"] {
            let m = matches(&format!("{verb} cs1"));
            assert!(m.contains(&"cs130".to_string()), "{verb}: {m:?}");
            assert!(m.contains(&"cs162".to_string()), "{verb}: {m:?}");
            assert!(!m.contains(&"chem".to_string()), "{verb} matched chem: {m:?}");
        }
    }

    #[test]
    fn cd_offers_the_parent_and_root_shortcuts() {
        let m = matches("cd ");
        assert!(m.contains(&"..".to_string()), "{m:?}");
        assert!(m.contains(&"/".to_string()), "{m:?}");
    }

    // ── notes ───────────────────────────────────────────────────────────────

    #[test]
    fn note_verbs_complete_notes_by_title() {
        for verb in ["edit", "delete", "ask", "e", "rm"] {
            let m = matches(&format!("{verb} owner"));
            assert_eq!(
                m.first().map(String::as_str),
                Some("1 Rust ownership"),
                "{verb}: {m:?}"
            );
        }
    }

    #[test]
    fn fuzzy_matching_finds_a_title_from_scattered_letters() {
        // The spec's example shape: non-contiguous characters still match.
        let m = matches("edit grtrv");
        assert_eq!(m.first().map(String::as_str), Some("2 Graph traversals"));
    }

    #[test]
    fn accepting_a_note_leaves_only_its_number() {
        let c = at_end("edit owner");
        let (line, cursor) = apply("edit owner", &c, c.best().unwrap());
        assert_eq!(line, "edit 1");
        assert_eq!(cursor, 6);
    }

    // ── trailing-slot verbs ─────────────────────────────────────────────────

    #[test]
    fn check_completes_notes_in_every_argument_position() {
        assert!(!matches("check own").is_empty());
        // The trailing checkbox number is not completable, but the note
        // reference before it still is.
        let m = matches("check 1 ");
        assert!(m.iter().any(|c| c.contains("Rust ownership")), "{m:?}");
    }

    #[test]
    fn mv_completes_notes_first_then_directories() {
        let first = matches("mv own");
        assert_eq!(first.first().map(String::as_str), Some("1 Rust ownership"));

        // In a later slot a directory is the likely target, and must be offered.
        let later = matches("mv 1 cs1");
        assert!(later.contains(&"cs130".to_string()), "{later:?}");
    }

    // ── tags ────────────────────────────────────────────────────────────────

    #[test]
    fn a_hash_completes_tags_anywhere_in_the_line() {
        let m = matches("new Lecture #ru");
        assert_eq!(m.first().map(String::as_str), Some("rust"));
        assert!(!matches("mv #remin").is_empty());
    }

    #[test]
    fn accepting_a_tag_keeps_the_hash() {
        let line = "new Lecture #ru";
        let c = at_end(line);
        let (new_line, _) = apply(line, &c, c.best().unwrap());
        assert_eq!(new_line, "new Lecture #rust");
    }

    // ── flags and subcommands ───────────────────────────────────────────────

    #[test]
    fn sync_completes_its_subcommands() {
        assert!(matches("sync ").contains(&"status".to_string()));
        assert_eq!(matches("sync pu").len(), 2, "push and pull both fuzzy match");
    }

    #[test]
    fn listen_completes_add_and_then_a_note() {
        assert!(matches("listen ").contains(&"add".to_string()));
        assert!(matches("listen ").contains(&"--screen".to_string()));
        let m = matches("listen add own");
        assert_eq!(m.first().map(String::as_str), Some("1 Rust ownership"));
    }

    #[test]
    fn free_text_arguments_offer_nothing() {
        // A note title being typed after `new` is the user's own text.
        assert!(matches("new My New Note").is_empty());
        assert!(matches("rename Graph traversals").is_empty());
    }

    // ── mechanics ───────────────────────────────────────────────────────────

    #[test]
    fn completing_mid_line_replaces_only_the_token_under_the_cursor() {
        let line = "mv ow cs130";
        // Cursor just after "ow".
        let c = complete(line, 5, &sources());
        assert_eq!(c.start, 3);
        assert_eq!(c.end, 5);
        let (new_line, cursor) = apply(line, &c, "1 Rust ownership");
        assert_eq!(new_line, "mv 1 cs130");
        assert_eq!(cursor, 4);
    }

    #[test]
    fn the_ghost_hint_is_only_the_untyped_remainder() {
        let c = at_end("ren");
        assert_eq!(c.ghost("ren").as_deref(), Some("ame"));
        // A fuzzy match that is not a prefix cannot be shown ahead of the
        // cursor, since the characters do not line up.
        let scattered = at_end("edit grtrv");
        assert_eq!(scattered.ghost("grtrv"), None);
        // Nothing left to hint once the word is complete.
        assert_eq!(at_end("edit").ghost("edit"), None);
    }

    #[test]
    fn an_empty_line_completes_from_position_zero() {
        let c = complete("", 0, &sources());
        assert_eq!(c.start, 0);
        assert_eq!(c.end, 0);
        assert!(!c.matches.is_empty());
    }

    #[test]
    fn a_trailing_space_starts_a_new_token_rather_than_extending_the_last() {
        let c = at_end("cd ");
        assert_eq!(c.start, 3, "the completion replaces nothing typed so far");
        assert_eq!(c.end, 3);
    }

    #[test]
    fn multibyte_input_spans_are_measured_in_characters() {
        let line = "view héllo";
        let c = complete(line, 10, &sources());
        assert_eq!(c.start, 5, "character index, not byte index");
        assert_eq!(c.end, 10);
    }

    #[test]
    fn empty_sources_never_panic() {
        let empty = Sources::default();
        // Store-derived candidates have nothing to offer...
        for line in ["cd x", "edit x", "new #x"] {
            let c = complete(line, line.chars().count(), &empty);
            assert!(c.matches.is_empty(), "{line}: {:?}", c.matches);
        }
        // ...but the static lists are independent of the store, so a fresh
        // install can still complete a subcommand.
        let c = complete("sync pu", 7, &empty);
        assert!(!c.matches.is_empty());
    }

    #[test]
    fn stripping_a_note_title_leaves_other_candidates_alone() {
        assert_eq!(strip_note_title("3 Midterm plan"), "3");
        assert_eq!(strip_note_title("cs130"), "cs130");
        assert_eq!(strip_note_title("md"), "md");
        assert_eq!(strip_note_title("-f"), "-f");
        // A title that starts with a number but is not a numbered candidate.
        assert_eq!(strip_note_title("2024 review"), "2024");
    }
}
