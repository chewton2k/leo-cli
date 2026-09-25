//! Note reference resolution: list number, then ID prefix, then unique title.

use super::*;

/// A note rendered just enough to disambiguate it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteBrief {
    /// 1-based position in the caller's current numbering, when it has one.
    pub index: Option<usize>,
    pub id: String,
    pub title: String,
}

/// The result of resolving a user-typed note reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    One(String),
    /// A title substring matched more than one note.
    Many(Vec<NoteBrief>),
    None,
}

/// Resolve a reference the same way everywhere: list number, then ID prefix,
/// then unique title substring. Returns structured data rather than printing,
/// so both shells can render disambiguation their own way.
///
/// Always yields a note's full ID, never the prefix the user typed. Downstream
/// `Store` lookups accept prefixes, but an `Outcome` or `ConfirmedAction` may
/// outlive the store state it was built from, and a prefix that is unique today
/// can become ambiguous after the next `sync pull`.
pub fn resolve(input: &str, store: &Store, numbering: &[String]) -> Resolved {
    if let Ok(n) = input.parse::<usize>() {
        if n >= 1 && n <= numbering.len() {
            let id = &numbering[n - 1];
            if let Some(note) = store.find_note(id) {
                return Resolved::One(note.id.clone());
            }
        }
    }
    if let Some(note) = store.find_note(input) {
        return Resolved::One(note.id.clone());
    }
    let matches = store.find_by_title(input);
    match matches.len() {
        0 => Resolved::None,
        1 => Resolved::One(matches[0].id.clone()),
        _ => Resolved::Many(
            matches
                .iter()
                .map(|note| NoteBrief {
                    index: numbering.iter().position(|id| id == &note.id).map(|p| p + 1),
                    id: note.id.clone(),
                    title: note.title.clone(),
                })
                .collect(),
        ),
    }
}

/// Render a failed resolution as output lines.
pub(super) fn unresolved(input: &str, resolved: Resolved) -> Outcome {
    match resolved {
        Resolved::Many(briefs) => {
            let mut lines = vec![Line::warn(format!("Multiple notes match \"{input}\":"))];
            for b in briefs {
                let idx = match b.index {
                    Some(i) => format!("{i:>3}"),
                    None => "   ".to_string(),
                };
                let short = &b.id[..std::cmp::min(8, b.id.len())];
                lines.push(Line::plain(format!("{idx} {short} {}", b.title)));
            }
            lines.push(Line::dim("Use a number or ID prefix to pick one."));
            Outcome::lines(lines)
        }
        _ => Outcome::line(Line::bad(format!("No note found: {input}"))),
    }
}

/// Resolve or return the rendered failure, so handlers stay one line each.
macro_rules! resolve_or_return {
    ($input:expr, $store:expr, $numbering:expr) => {
        match resolve($input, $store, $numbering) {
            Resolved::One(id) => id,
            other => return Ok(unresolved($input, other)),
        }
    };
}

pub(super) use resolve_or_return;
