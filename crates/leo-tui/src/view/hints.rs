//! The key hints on the idle command line.
//!
//! They change with where the user is, because the question the line answers is
//! "what can I do here?", and the answer differs between the notes list, the
//! directory tree and a recording in progress.

use ratatui::style::{Modifier, Style};
use ratatui::text::Span;

use super::theme;

/// Where the keyboard is, as far as the hints are concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Notes,
    Dirs,
    Preview,
    Recording,
}

pub const MOST: usize = 6;

/// Key and what it does, most useful first. Fitting drops from the end.
pub fn for_place(place: Place) -> &'static [(&'static str, &'static str)] {
    match place {
        Place::Notes => &[
            ("Enter", "write"),
            ("n", "new"),
            ("/", "search"),
            ("F2", "actions"),
            ("D", "delete"),
            ("?", "help"),
        ],
        Place::Dirs => &[
            ("Enter", "open"),
            ("N", "new folder"),
            ("/", "search"),
            ("F2", "actions"),
            ("D", "delete"),
            ("?", "help"),
        ],
        Place::Preview => &[
            ("Esc", "done"),
            ("type", "to write"),
            ("Tab", "indent"),
            ("Ctrl-Z", "undo"),
        ],
        Place::Recording => &[
            ("type", "a point"),
            ("Enter", "add point"),
            ("Ctrl-P", "pause"),
            ("↑↓", "scroll back"),
            ("Esc", "twice stops and saves"),
        ],
    }
}

/// Lay the hints out in `width` columns. Pairs that do not fit are dropped from
/// the end, but `? help` always stays, since it is the way to everything else.
pub fn spans(hints: &[(&'static str, &'static str)], width: u16) -> Vec<Span<'static>> {
    const GAP: usize = 3;
    let cost = |(key, what): &(&str, &str)| GAP + key.chars().count() + 1 + what.chars().count();

    let help = hints.iter().find(|(k, _)| *k == "?");
    let mut budget = (width as usize).saturating_sub(help.map(cost).unwrap_or(0));
    let actions = hints
        .iter()
        .find(|(k, _)| *k == "F2")
        .filter(|hint| cost(hint) <= budget);
    budget = budget.saturating_sub(actions.map(cost).unwrap_or(0));
    let mut shown: Vec<&(&'static str, &'static str)> = Vec::new();
    for hint in hints.iter().filter(|(k, _)| *k != "?" && *k != "F2") {
        let c = cost(hint);
        if c > budget {
            break;
        }
        budget -= c;
        shown.push(hint);
    }
    shown.extend(actions);
    shown.extend(help);
    shown.sort_by_key(|hint| hints.iter().position(|entry| entry == *hint));

    let key_style = Style::default()
        .fg(theme::accent())
        .add_modifier(Modifier::BOLD);
    let what_style = Style::default().add_modifier(Modifier::DIM);
    let mut out = Vec::new();
    for (key, what) in shown {
        out.push(Span::raw(" ".repeat(GAP)));
        out.push(Span::styled(*key, key_style));
        out.push(Span::styled(format!(" {what}"), what_style));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(place: Place) -> Vec<&'static str> {
        for_place(place).iter().map(|(k, _)| *k).collect()
    }

    fn text(spans: &[Span<'_>]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn the_notes_list_offers_the_everyday_keys() {
        let k = keys(Place::Notes);
        for key in ["Enter", "n", "D", "/", "?"] {
            assert!(k.contains(&key), "notes hints lack {key}: {k:?}");
        }
    }

    #[test]
    fn no_place_offers_more_than_a_handful_of_keys() {
        for place in [Place::Notes, Place::Dirs, Place::Preview, Place::Recording] {
            assert!(for_place(place).len() <= MOST, "{place:?} offers too many");
        }
    }

    #[test]
    fn the_directory_pane_offers_directory_keys() {
        let k = keys(Place::Dirs);
        for key in ["Enter", "N", "D"] {
            assert!(k.contains(&key), "dirs hints lack {key}: {k:?}");
        }
    }

    #[test]
    fn a_recording_says_how_to_stop() {
        let hints = for_place(Place::Recording);
        assert!(hints
            .iter()
            .any(|(k, what)| *k == "Esc" && what.contains("stop")));
        assert!(hints
            .iter()
            .any(|(k, what)| *k == "Enter" && what.contains("point")));
    }

    /// A hint for a key that does nothing would be worse than no hint.
    #[test]
    fn every_hinted_key_is_documented_in_help() {
        let documented = super::super::help::all_keys().join(" ");
        for place in [Place::Notes, Place::Dirs, Place::Preview, Place::Recording] {
            for (key, _) in for_place(place) {
                if *key == "type" {
                    continue;
                }
                assert!(
                    documented.split_whitespace().any(|k| k == *key),
                    "{place:?} hints `{key}`, which help does not document"
                );
            }
        }
    }

    #[test]
    fn help_survives_a_narrow_terminal() {
        let s = spans(for_place(Place::Notes), 24);
        let t = text(&s);
        assert!(t.contains("? help"), "{t:?}");
        assert!(t.chars().count() <= 24, "{t:?} is wider than 24");
    }

    #[test]
    fn actions_remain_discoverable_when_other_shortcuts_do_not_fit() {
        for place in [Place::Notes, Place::Dirs] {
            let t = text(&spans(for_place(place), 24));
            assert!(t.contains("F2 actions"), "{t:?}");
            assert!(t.contains("? help"), "{t:?}");
            assert!(t.chars().count() <= 24, "{t:?}");
        }
    }

    #[test]
    fn a_wide_terminal_shows_them_all() {
        let t = text(&spans(for_place(Place::Notes), 200));
        for (key, what) in for_place(Place::Notes) {
            assert!(t.contains(&format!("{key} {what}")), "{t:?} lacks {key}");
        }
    }
}
