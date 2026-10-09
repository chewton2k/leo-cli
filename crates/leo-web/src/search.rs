use std::cmp::Reverse;
use std::collections::HashSet;

use leo_core::notes::Note;
use leo_core::store::Store;

use crate::chat::flat;
use crate::graph::{Cache, Read};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", content = "name", rename_all = "lowercase")]
pub enum Why {
    Idea(String),
    Summary,
    Meaning,
}

#[derive(Debug)]
pub struct Hit<'a> {
    pub note: &'a Note,
    pub why: Option<Why>,
}

struct Seen {
    title: String,
    text: String,
    summary: String,
    ideas: Vec<(String, Vec<String>, Vec<String>)>,
    title_initials: Vec<String>,
}

fn parts(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

fn initials(words: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for size in 2..=5.min(words.len()) {
        if size == 2 && words.len() > 2 {
            continue;
        }
        for window in words.windows(size) {
            let all: String = window.iter().filter_map(|w| w.chars().next()).collect();
            out.push(all);
            let long: Vec<&String> = window.iter().filter(|w| w.chars().count() > 2).collect();
            if long.len() >= 2 && long.len() < window.len() {
                out.push(long.iter().filter_map(|w| w.chars().next()).collect());
            }
        }
    }
    out
}

fn seen(note: &Note, read: Option<&Read>) -> Seen {
    let title = flat(&note.title);
    let title_initials = initials(&parts(&title));
    Seen {
        text: format!("{title}\n{}", flat(&note.body)),
        title,
        summary: read.map(|r| flat(&r.summary)).unwrap_or_default(),
        ideas: read
            .map(|r| {
                r.concepts
                    .iter()
                    .map(|c| {
                        let words = parts(&flat(c));
                        let short = initials(&words);
                        (c.clone(), words, short)
                    })
                    .collect()
            })
            .unwrap_or_default(),
        title_initials,
    }
}

enum Found {
    Title,
    Text,
    Idea(String),
    Summary,
}

fn find_word(seen: &Seen, word: &str) -> Option<Found> {
    let short = (2..=6).contains(&word.chars().count());
    if seen.title.contains(word) || (short && seen.title_initials.iter().any(|i| i == word)) {
        return Some(Found::Title);
    }
    if seen.text.contains(word) {
        return Some(Found::Text);
    }
    for (idea, words, ideas_initials) in &seen.ideas {
        let named = words.iter().any(|w| w == word)
            || (word.chars().count() >= 4 && words.iter().any(|w| w.starts_with(word)));
        if named || (short && ideas_initials.iter().any(|i| i == word)) {
            return Some(Found::Idea(idea.clone()));
        }
    }
    seen.summary.contains(word).then_some(Found::Summary)
}

pub fn search<'a>(store: &'a Store, cache: &Cache, query: &str) -> Vec<Hit<'a>> {
    let plain = |notes: Vec<&'a Note>| {
        notes
            .into_iter()
            .map(|note| Hit { note, why: None })
            .collect()
    };
    if query.split_whitespace().any(|w| w.starts_with('#')) {
        return plain(store.find(query));
    }
    let words: Vec<String> = parts(&flat(query));
    if words.is_empty() {
        return Vec::new();
    }
    let needed = (2 * words.len()).div_ceil(3);
    let mut ranked: Vec<(u8, usize, &Note, Option<Why>)> = Vec::new();
    for note in &store.notes {
        let seen = seen(note, cache.notes.get(&note.id));
        let found: Vec<Option<Found>> = words.iter().map(|w| find_word(&seen, w)).collect();
        let hits = found.iter().flatten().count();
        if hits == 0 {
            continue;
        }
        let score: usize = found
            .iter()
            .flatten()
            .map(|f| match f {
                Found::Title => 3,
                Found::Idea(_) => 3,
                Found::Summary => 2,
                Found::Text => 1,
            })
            .sum();
        let why = found.iter().flatten().find_map(|f| match f {
            Found::Idea(idea) => Some(Why::Idea(idea.clone())),
            Found::Summary => Some(Why::Summary),
            _ => None,
        });
        let group = if found.iter().all(|f| matches!(f, Some(Found::Title))) {
            0
        } else if hits == words.len() {
            1
        } else if words.len() >= 3
            && hits >= needed
            && found
                .iter()
                .any(|f| matches!(f, Some(Found::Title | Found::Idea(_))))
        {
            2
        } else {
            continue;
        };
        ranked.push((group, score, note, why));
    }
    ranked.sort_by(|(ga, sa, a, _), (gb, sb, b, _)| {
        ga.cmp(gb)
            .then(sb.cmp(sa))
            .then(b.updated_at.cmp(&a.updated_at))
    });
    let mut taken: HashSet<&str> = ranked.iter().map(|(_, _, n, _)| n.id.as_str()).collect();
    let mut hits: Vec<Hit<'a>> = ranked
        .into_iter()
        .map(|(_, _, note, why)| Hit { note, why })
        .collect();
    let mut rest: Vec<&'a Note> = store
        .find(query)
        .into_iter()
        .filter(|n| taken.insert(n.id.as_str()))
        .collect();
    rest.sort_by_key(|n| Reverse(n.updated_at));
    hits.extend(plain(rest));
    hits
}

pub fn with_meaning<'a>(
    store: &'a Store,
    query: &str,
    mut hits: Vec<Hit<'a>>,
    close: &[(String, f32)],
) -> Vec<Hit<'a>> {
    if query.split_whitespace().any(|w| w.starts_with('#')) {
        return hits;
    }
    let mut taken: HashSet<String> = hits.iter().map(|h| h.note.id.clone()).collect();
    for (id, _) in close {
        if let Some(note) = store.notes.iter().find(|n| &n.id == id) {
            if taken.insert(note.id.clone()) {
                hits.push(Hit {
                    note,
                    why: Some(Why::Meaning),
                });
            }
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library(notes: &[(&str, &str)]) -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::load_from(&dir.path().join("notes")).unwrap();
        for (title, body) in notes {
            let id = store
                .create_note(*title, "", vec![], "")
                .unwrap()
                .id
                .clone();
            store.find_note_mut(&id).unwrap().body = body.to_string();
        }
        (dir, store)
    }

    fn mapped(store: &Store, ideas: &[(&str, &[&str], &str)]) -> Cache {
        let mut cache = Cache::default();
        for (title, concepts, summary) in ideas {
            let note = store.notes.iter().find(|n| n.title == *title).unwrap();
            cache.notes.insert(
                note.id.clone(),
                Read {
                    hash: String::new(),
                    summary: summary.to_string(),
                    concepts: concepts.iter().map(|c| c.to_string()).collect(),
                    linked: None,
                },
            );
        }
        cache
    }

    fn titles(hits: &[Hit]) -> Vec<String> {
        hits.iter().map(|h| h.note.title.clone()).collect()
    }

    #[test]
    fn an_abbreviation_finds_the_idea_the_map_names() {
        let (_d, store) = library(&[
            ("Graph traversal", "Visit every vertex level by level."),
            ("Sorting", "Merge sort splits the list."),
        ]);
        let cache = mapped(
            &store,
            &[("Graph traversal", &["breadth-first search", "queues"], "")],
        );
        let hits = search(&store, &cache, "BFS");
        assert_eq!(titles(&hits), ["Graph traversal"]);
        assert_eq!(hits[0].why, Some(Why::Idea("breadth-first search".into())));
    }

    #[test]
    fn an_abbreviation_of_a_title_and_hyphenated_words_are_found_without_a_map() {
        let (_d, store) = library(&[
            ("Breadth-First Search", ""),
            ("Week 3", "We covered depth-first search today."),
        ]);
        let cache = Cache::default();
        assert_eq!(
            titles(&search(&store, &cache, "bfs")),
            ["Breadth-First Search"]
        );
        let hits = search(&store, &cache, "depth first");
        assert_eq!(titles(&hits), ["Week 3"]);
        assert_eq!(hits[0].why, None, "found in the text itself");
    }

    #[test]
    fn titles_come_first_then_text_then_the_map() {
        let (_d, store) = library(&[
            ("Lecture 4", "Heaps keep the minimum at the root."),
            ("Heaps", "A tree."),
            ("Priority queues", "Insert and pop."),
        ]);
        let cache = mapped(&store, &[("Priority queues", &["heaps"], "")]);
        assert_eq!(
            titles(&search(&store, &cache, "heaps")),
            ["Heaps", "Priority queues", "Lecture 4"]
        );
    }

    #[test]
    fn a_summary_counts_and_says_so() {
        let (_d, store) = library(&[("Lecture 9", "Board photos only.")]);
        let cache = mapped(
            &store,
            &[("Lecture 9", &[], "Covers Dijkstra's shortest paths.")],
        );
        let hits = search(&store, &cache, "dijkstra");
        assert_eq!(titles(&hits), ["Lecture 9"]);
        assert_eq!(hits[0].why, Some(Why::Summary));
        assert_eq!(
            serde_json::to_value(Why::Idea("queues".into())).unwrap(),
            serde_json::json!({"kind": "idea", "name": "queues"})
        );
        assert_eq!(
            serde_json::to_value(Why::Summary).unwrap(),
            serde_json::json!({"kind": "summary"})
        );
    }

    #[test]
    fn several_words_need_most_of_them_and_a_strong_place() {
        let (_d, store) = library(&[
            ("Shortest paths", "Dijkstra's algorithm."),
            ("Exam logistics", "Room 101."),
            ("Unrelated", "Nothing here."),
            ("Breadth-First Search", ""),
        ]);
        let cache = Cache::default();
        assert_eq!(
            titles(&search(&store, &cache, "shortest paths exam")),
            ["Shortest paths"]
        );
        assert!(search(&store, &cache, "nothing matches zebra").is_empty());
        assert!(
            search(&store, &cache, "depth first").is_empty(),
            "one word of two is not a match"
        );
    }

    #[test]
    fn a_tag_search_and_a_misspelt_title_still_work() {
        let (_d, mut store) = library(&[("Algorithms", ""), ("Tagged", "")]);
        let tagged = store
            .notes
            .iter()
            .find(|n| n.title == "Tagged")
            .unwrap()
            .id
            .clone();
        store.find_note_mut(&tagged).unwrap().tags = vec!["exam".into()];
        let cache = Cache::default();
        assert_eq!(titles(&search(&store, &cache, "#exam")), ["Tagged"]);
        assert_eq!(titles(&search(&store, &cache, "algrthms")), ["Algorithms"]);
        assert!(search(&store, &cache, "  ").is_empty());
    }
}
