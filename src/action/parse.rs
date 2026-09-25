//! The `:` vocabulary: the verb table, retired words, and the line parser.

use super::*;

/// All verbs and their aliases, in help order. The completion engine reads
/// this too, so a new verb becomes completable for free.
/// An alias earns its place by being something a user already types, not by
/// saving a keystroke. Three kinds survive:
///
/// * shell muscle memory — `ls`, `rm`, `mv`, `exit`, `q`;
/// * the same letter as the key that does it in the panes — `e`, `x`, `?`;
/// * nothing else.
///
/// Seventeen aliases became six. The rest were a second name to learn for no
/// gain, and some actively misled: `l` listed notes here while moving between
/// panes there, and `d` deleted a note here while dropping a provider from a
/// chain on the settings screen.
pub const VERBS: &[Verb] = &[
    v("new", &[], "new [dir/][title] [#tag...]", "a note, opening $EDITOR"),
    v("edit", &["e"], "edit [note]", "open a note in $EDITOR"),
    v("delete", &["rm"], "delete [note]", "delete a note (asks first)"),
    v("rename", &[], "rename <new title>", "retitle the selected note"),
    v("check", &["x"], "check <note> <N>", "tick or untick checkbox N"),
    v("undo", &["u"], "undo", "take back the last delete, move or tick"),
    v("listen", &[], "listen [title | add [note]] [--screen]", "record, and write notes from speech"),
    v("ask", &[], "ask [note]", "answer the note's @leo lines"),
    v("mkdir", &[], "mkdir <name>", "a directory here"),
    v("cd", &[], "cd <dir>", "enter a directory; .. up, / root"),
    v("mv", &[], "mv [note...] <dir>", "move notes, or the selected one"),
    v("sync", &[], "sync <init | connect <url> | push | pull | status>", "back up to git"),
    v("help", &["?"], "help", "every key and command"),
    v("quit", &["exit", "q"], "quit", "leave"),
];

/// One `:` command: its name, the aliases that survived the prune, how to call
/// it, and what it does. Help, the `:` menu and usage errors all read this.
#[derive(Debug)]
pub struct Verb {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub usage: &'static str,
    pub summary: &'static str,
}

pub(super) const fn v(
    name: &'static str,
    aliases: &'static [&'static str],
    usage: &'static str,
    summary: &'static str,
) -> Verb {
    Verb { name, aliases, usage, summary }
}

/// The table row for a verb or one of its aliases.
pub fn verb(word: &str) -> Option<&'static Verb> {
    VERBS.iter().find(|v| v.name == word || v.aliases.contains(&word))
}

/// Words that used to work: what to use instead, and why it changed.
///
/// Removing a word someone has in their fingers is only kind if the removal
/// explains itself. "Unknown command: d" reads like a typo and sends the user
/// hunting; naming the replacement costs one line. A replacement starting with
/// `:` is a command; anything else is a key or a place on screen.
pub const RETIRED: &[(&str, &str, &str)] = &[
    ("l", "the notes pane", LISTED),
    ("ls", "the notes pane", LISTED),
    ("list", "the notes pane", LISTED),
    ("v", "j and k", SHOWN),
    ("view", "j and k", SHOWN),
    ("d", ":delete", ONE_NAME),
    ("del", ":delete", ONE_NAME),
    ("n", "n", "it is a key now: n makes a note"),
    ("rec", "R", "it is a key now: R records"),
    ("move", "m", "it is a key now: m moves the selected note"),
    ("h", "?", ONE_NAME),
    ("find", "/", ONE_SEARCH),
    ("search", "/", ONE_SEARCH),
    ("expand", "a", "it is a key now: a asks about the selected note"),
    ("uncheck", ":check", ONE_NAME),
    ("tags", "t", "it switches the left pane to your tags, with counts"),
    ("rmdir", "D in the directories pane", "it asks, then removes the directory"),
    ("model", "Ctrl-S", PROFILE),
    ("config", "Ctrl-S", PROFILE),
    ("rem", "a note with - [ ] lines", GONE_REMIND),
    ("remind", "a note with - [ ] lines", GONE_REMIND),
    ("exp", "the .md file in your notes folder", GONE_EXPORT),
    ("export", "the .md file in your notes folder", GONE_EXPORT),
    (
        "env",
        "Ctrl-S",
        "keys live in your OS keychain now, not a plaintext file",
    ),
    ("pwd", "the status bar", "it always shows where you are"),
    ("clear", "Esc", "it closes whatever output is pinned"),
];

pub(super) const ONE_NAME: &str = "one name per command now, so there is less to learn";
pub(super) const LISTED: &str = "the notes pane always lists this directory";
pub(super) const SHOWN: &str = "the preview shows whichever note is selected";
pub(super) const PROFILE: &str = "providers and keys live on that screen; `leo model` still works in a shell";
pub(super) const GONE_REMIND: &str = "reminders were removed; a checklist note does the same";
pub(super) const GONE_EXPORT: &str = "export was removed; every note is already a Markdown file";
pub(super) const ONE_SEARCH: &str = "one search now: / looks in every note, bodies and tags included";

/// Every word that can start a command, canonical names and aliases alike.
pub fn all_verb_words() -> Vec<&'static str> {
    let mut out = Vec::new();
    for verb in VERBS {
        out.push(verb.name);
        out.extend_from_slice(verb.aliases);
    }
    out
}

/// Split on whitespace, keeping quoted runs together.
pub fn tokenize(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut quote_char = '"';

    for ch in input.chars() {
        if in_quotes {
            if ch == quote_char {
                in_quotes = false;
            } else {
                current.push(ch);
            }
        } else {
            match ch {
                '"' | '\'' => {
                    in_quotes = true;
                    quote_char = ch;
                }
                ' ' | '\t' => {
                    if !current.is_empty() {
                        tokens.push(current.clone());
                        current.clear();
                    }
                }
                _ => current.push(ch),
            }
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Strip a natural-language `hey leo` / `leo` prefix, so "hey leo remind me to
/// call mom" works. Only strips when something follows.
pub fn strip_leo_prefix(tokens: &mut Vec<String>) {
    if tokens.len() >= 2
        && tokens[0].eq_ignore_ascii_case("hey")
        && tokens[1].eq_ignore_ascii_case("leo")
    {
        tokens.drain(0..2);
    } else if tokens.len() >= 2 && tokens[0].eq_ignore_ascii_case("leo") {
        tokens.drain(0..1);
    }
}

/// Parse one command line into an [`Action`].
pub fn parse(line: &str) -> Parsed {
    let mut tokens = tokenize(line.trim());
    if tokens.is_empty() {
        return Parsed::Empty;
    }
    strip_leo_prefix(&mut tokens);
    if tokens.is_empty() {
        return Parsed::Empty;
    }

    let verb = tokens[0].to_lowercase();
    let args = &tokens[1..];
    let joined = || args.join(" ");
    let usage = |name: &str| {
        Parsed::Usage(self::verb(name).map(|v| v.usage).unwrap_or(name).to_string())
    };
    let act = |a: Action| Parsed::Action(a);

    match verb.as_str() {
        "new" => act(Action::New {
            title: if args.is_empty() { None } else { Some(joined()) },
        }),

        "edit" | "e" => act(Action::Edit { note: joined() }),
        "delete" | "rm" => act(Action::Delete { note: joined() }),

        // The checkbox number is the last token, so everything before it is the
        // note reference — a title with spaces still resolves.
        "check" | "x" => {
            if args.len() < 2 {
                return usage("check");
            }
            match args.last().unwrap().parse::<usize>() {
                Ok(index) if index >= 1 => act(Action::Check {
                    note: args[..args.len() - 1].join(" "),
                    index,
                }),
                _ => Parsed::Usage("Checkbox number must be a positive integer.".to_string()),
            }
        }

        "listen" => {
            let screen = args.iter().any(|a| a == "--screen");
            let rest: Vec<String> =
                args.iter().filter(|a| a.as_str() != "--screen").cloned().collect();

            if rest.first().map(|s| s.eq_ignore_ascii_case("add")).unwrap_or(false) {
                return act(Action::Listen {
                    title: None,
                    append_to: Some(rest[1..].join(" ")),
                    screen,
                });
            }
            act(Action::Listen {
                title: if rest.is_empty() { None } else { Some(rest.join(" ")) },
                append_to: None,
                screen,
            })
        }

        "ask" => act(Action::Ask { note: joined() }),

        "undo" | "u" => act(Action::Undo),

        // The whole line is the new title; the note is always the selected one
        // (see `fill_selected`), which is what the `r` key pre-fills this for.
        "rename" => {
            if args.is_empty() {
                usage("rename")
            } else {
                act(Action::Rename { note: String::new(), title: joined() })
            }
        }

        "mkdir" => {
            let name = joined().trim().to_string();
            if name.is_empty() {
                usage("mkdir")
            } else {
                act(Action::Mkdir { name })
            }
        }

        "cd" => act(Action::Cd { path: joined().trim().to_string() }),


        // With one argument, that is the directory and the note is the
        // selected one.
        "mv" => {
            if args.is_empty() {
                return usage("mv");
            }
            act(Action::Mv {
                notes: args[..args.len() - 1].to_vec(),
                dir: args.last().unwrap().trim_matches('/').to_string(),
            })
        }

        "sync" => match args.first().map(|s| s.to_lowercase()).as_deref() {
            Some("init") => act(Action::Sync(SyncAction::Init)),
            Some("connect") => match args.get(1) {
                Some(url) => act(Action::Sync(SyncAction::Connect { url: url.clone() })),
                None => Parsed::Usage("sync connect <url>".to_string()),
            },
            Some("push") => act(Action::Sync(SyncAction::Push)),
            Some("pull") => act(Action::Sync(SyncAction::Pull)),
            Some("status") => act(Action::Sync(SyncAction::Status)),
            _ => usage("sync"),
        },

        "help" | "?" => act(Action::Help),
        "quit" | "exit" | "q" => act(Action::Quit),

        _ => match RETIRED.iter().find(|(alias, _, _)| *alias == verb.as_str()) {
            Some((alias, replacement, why)) => Parsed::Retired {
                verb: alias,
                replacement,
                why,
            },
            None => Parsed::Unknown(verb),
        },
    }
}

#[cfg(test)]
mod parse_tests {
    use super::*;

    fn act(line: &str) -> Action {
        match parse(line) {
            Parsed::Action(a) => a,
            other => panic!("expected an action for {line:?}, got {other:?}"),
        }
    }

    fn usage(line: &str) -> String {
        match parse(line) {
            Parsed::Usage(u) => u,
            other => panic!("expected usage for {line:?}, got {other:?}"),
        }
    }

    #[test]
    fn blank_input_is_empty() {
        assert_eq!(parse(""), Parsed::Empty);
        assert_eq!(parse("   "), Parsed::Empty);
        // A bare prefix with nothing after it is not a command either.
        assert_eq!(parse("hey leo"), Parsed::Empty);
    }

    #[test]
    fn unknown_verb_is_reported_with_the_verb() {
        assert_eq!(parse("frobnicate 3"), Parsed::Unknown("frobnicate".to_string()));
    }

    /// Every live alias must parse exactly like the verb it abbreviates.
    #[test]
    fn every_alias_maps_to_the_same_action_as_its_canonical_verb() {
        let pairs = [
            ("e 1", "edit 1"),
            ("rm 1", "delete 1"),
            ("x 1 2", "check 1 2"),
            ("?", "help"),
            ("exit", "quit"),
            ("q", "quit"),
        ];
        for (alias, canonical) in pairs {
            assert_eq!(
                parse(alias),
                parse(canonical),
                "alias {alias:?} should parse like {canonical:?}"
            );
        }
    }

    /// A retired alias must name its replacement. Removing a word someone has in
    /// their fingers is only kind if the removal explains itself; "unknown
    /// command: d" reads like a typo.
    #[test]
    fn every_retired_alias_names_a_real_replacement() {
        for (alias, instead, why) in RETIRED {
            match parse(alias) {
                Parsed::Retired { verb, replacement, why: said } => {
                    assert_eq!(verb, *alias);
                    assert_eq!(replacement, *instead);
                    assert_eq!(said, *why);
                    assert!(!why.is_empty(), "{alias} retires without a reason");
                    // A `:` replacement has to be something that actually parses.
                    if let Some(command) = instead.strip_prefix(':') {
                        assert!(
                            !matches!(parse(command), Parsed::Unknown(_) | Parsed::Retired { .. }),
                            "{alias} points at {instead}, which is not a verb"
                        );
                    }
                }
                other => panic!("{alias} should be retired, got {other:?}"),
            }
        }
    }

    /// A retired alias must not also be live, or the table contradicts itself.
    #[test]
    fn no_retired_alias_is_still_in_the_verb_table() {
        for (alias, _, _) in RETIRED {
            assert!(
                !all_verb_words().contains(alias),
                "{alias} is both retired and live"
            );
        }
    }

    /// Verbs left over from the line-oriented shell, where the screen scrolled
    /// and nothing showed the directory. The panes do both now.
    #[test]
    fn pwd_and_clear_point_at_what_replaced_them() {
        match parse("pwd") {
            Parsed::Retired { replacement, .. } => {
                assert!(replacement.contains("status"), "{replacement}")
            }
            other => panic!("expected Retired, got {other:?}"),
        }
        match parse("clear") {
            Parsed::Retired { replacement, .. } => assert_eq!(replacement, "Esc"),
            other => panic!("expected Retired, got {other:?}"),
        }
    }

    /// The point of the prune: one name per command, give or take the few that
    /// come from the shell or mirror a key.
    #[test]
    fn the_vocabulary_stays_small() {
        let aliases: usize = VERBS.iter().map(|v| v.aliases.len()).sum();
        assert!(aliases <= 8, "aliases crept back up to {aliases}");
    }

    #[test]
    fn verbs_are_case_insensitive() {
        assert_eq!(act("EDIT 1"), act("edit 1"));
        assert_eq!(act("Mv 1 cs130"), act("mv 1 cs130"));
    }

    #[test]
    fn hey_leo_prefix_is_stripped() {
        assert_eq!(act("hey leo new Groceries"), act("new Groceries"));
        assert_eq!(act("leo undo"), Action::Undo);
        // "leo" alone as the whole line is not a command.
        assert_eq!(parse("leo"), Parsed::Unknown("leo".to_string()));
    }

    #[test]
    fn multi_word_note_references_are_joined() {
        assert_eq!(
            act("edit Rust ownership notes"),
            Action::Edit { note: "Rust ownership notes".to_string() }
        );
    }

    #[test]
    fn quoted_arguments_stay_together() {
        assert_eq!(
            act("new \"My Note\""),
            Action::New { title: Some("My Note".to_string()) }
        );
    }

    /// `check` takes the checkbox number as the LAST token, so a multi-word
    /// title in front of it must still resolve.
    #[test]
    fn check_takes_its_number_from_the_end() {
        assert_eq!(
            act("check Rust ownership 3"),
            Action::Check { note: "Rust ownership".to_string(), index: 3 }
        );
    }

    #[test]
    fn check_rejects_a_non_numeric_or_zero_index() {
        assert!(usage("check 1 abc").contains("positive integer"));
        assert!(usage("check 1 0").contains("positive integer"));
        assert!(usage("check 1").contains("check <note>"));
    }

    /// `mv` takes the directory last and any number of notes before it.
    #[test]
    fn mv_takes_the_directory_from_the_end() {
        assert_eq!(
            act("mv 1 2 3 cs130"),
            Action::Mv {
                notes: vec!["1".to_string(), "2".to_string(), "3".to_string()],
                dir: "cs130".to_string(),
            }
        );
        // A trailing slash on the destination is tolerated, and `/` means root.
        assert_eq!(
            act("mv 1 /"),
            Action::Mv { notes: vec!["1".to_string()], dir: String::new() }
        );
    }

    /// Verbs the panes or the profile screen already do, and the two features
    /// that went: each must still explain itself when typed.
    #[test]
    fn verbs_the_panes_replaced_are_retired() {
        for word in [
            "list", "ls", "view", "tags", "rmdir", "model", "config", "remind", "export",
        ] {
            assert!(
                matches!(parse(word), Parsed::Retired { .. }),
                "{word:?} should be retired, got {:?}",
                parse(word)
            );
        }
    }

    /// There is one search, and it is `/`.
    #[test]
    fn search_and_find_point_at_slash() {
        for word in ["search rust", "find rust"] {
            match parse(word) {
                Parsed::Retired { replacement, .. } => assert_eq!(replacement, "/"),
                other => panic!("{word:?} should be retired, got {other:?}"),
            }
        }
    }

    #[test]
    fn listen_parses_screen_flag_title_and_append_target() {
        assert_eq!(
            act("listen"),
            Action::Listen { title: None, append_to: None, screen: false }
        );
        assert_eq!(
            act("listen CS 101 Lecture"),
            Action::Listen {
                title: Some("CS 101 Lecture".to_string()),
                append_to: None,
                screen: false,
            }
        );
        assert_eq!(
            act("listen add 1"),
            Action::Listen { title: None, append_to: Some("1".to_string()), screen: false }
        );
        // --screen is positional-agnostic and never lands in the title.
        assert_eq!(
            act("listen --screen Lecture 3"),
            Action::Listen {
                title: Some("Lecture 3".to_string()),
                append_to: None,
                screen: true,
            }
        );
        assert_eq!(
            act("listen Lecture 3 --screen"),
            Action::Listen {
                title: Some("Lecture 3".to_string()),
                append_to: None,
                screen: true,
            }
        );
        // No note after `add` means the selected one; see `fill_selected`.
        assert_eq!(act("listen add"), Action::Listen { title: None, append_to: Some(String::new()), screen: false });
    }

    #[test]
    fn cd_accepts_no_argument_as_root() {
        assert_eq!(act("cd"), Action::Cd { path: String::new() });
        assert_eq!(act("cd .."), Action::Cd { path: "..".to_string() });
        assert_eq!(act("cd cs130"), Action::Cd { path: "cs130".to_string() });
    }

    #[test]
    fn sync_subcommands_parse() {
        assert_eq!(act("sync init"), Action::Sync(SyncAction::Init));
        assert_eq!(act("sync push"), Action::Sync(SyncAction::Push));
        assert_eq!(act("sync pull"), Action::Sync(SyncAction::Pull));
        assert_eq!(act("sync status"), Action::Sync(SyncAction::Status));
        assert_eq!(
            act("sync connect https://example.com/n.git"),
            Action::Sync(SyncAction::Connect { url: "https://example.com/n.git".to_string() })
        );
        assert!(usage("sync connect").contains("connect"));
        assert!(usage("sync").contains("init"));
        assert!(usage("sync bogus").contains("init"));
    }

    #[test]
    fn usage_is_returned_for_verbs_missing_a_required_argument() {
        for line in ["mkdir", "mv", "rename", "check 1"] {
            assert!(
                matches!(parse(line), Parsed::Usage(_)),
                "{line:?} should report usage"
            );
        }
    }

    /// The table is what help, the : menu and usage errors are built from, so
    /// every row has to carry both.
    #[test]
    fn every_verb_has_a_usage_and_a_summary() {
        for verb in VERBS {
            assert!(
                verb.usage.split_whitespace().next() == Some(verb.name),
                "{}: usage {:?} does not start with the verb",
                verb.name,
                verb.usage
            );
            assert!(!verb.summary.is_empty(), "{} has no summary", verb.name);
        }
    }

    /// A usage error quotes the table, so the two cannot disagree.
    #[test]
    fn usage_errors_come_from_the_table() {
        assert_eq!(usage("mkdir"), verb("mkdir").unwrap().usage);
        assert_eq!(usage("rename"), verb("rename").unwrap().usage);
    }

    #[test]
    fn every_verb_and_alias_in_the_table_parses_to_something_known() {
        for word in all_verb_words() {
            // Bare verbs may legitimately want arguments; what must never
            // happen is a verb in the table being reported as unknown.
            assert!(
                !matches!(parse(word), Parsed::Unknown(_)),
                "{word:?} is in VERBS but parse() calls it unknown"
            );
        }
    }

    #[test]
    fn tokenize_keeps_quoted_runs_and_drops_empty_gaps() {
        assert_eq!(tokenize("a  b\tc"), vec!["a", "b", "c"]);
        assert_eq!(tokenize("new \"two words\""), vec!["new", "two words"]);
        assert_eq!(tokenize("new 'single quoted'"), vec!["new", "single quoted"]);
        assert!(tokenize("   ").is_empty());
    }
}
