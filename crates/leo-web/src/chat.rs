use std::collections::BTreeSet;
use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use leo_core::notes::Note;
use leo_core::store::Store;

use crate::graph::Cache;

pub type Streamer = Arc<
    dyn Fn(&str, &str, u32, &mut dyn FnMut(&str), &mut dyn FnMut()) -> Result<String> + Send + Sync,
>;

const OPEN_CHARS: usize = 14_000;
const ATTACHED_CHARS: usize = 12_000;
pub const MOST_ATTACHED: usize = 8;
const NOTE_CHARS: usize = 4_000;
pub const ROOM: usize = 64_000;
pub const LEAST_ROOM: usize = 12_000;
pub const MOST_ROOM: usize = 96_000;
const NEIGHBOURS: usize = 5;
const MATCHES: usize = 8;
const EXPANDED_MATCHES: usize = 3;
const NEIGHBOURS_PER_MATCH: usize = 2;
const TURNS: usize = 14;
const DOCS_CHARS: usize = 60_000;
const TURN_CHARS: usize = 4_000;
pub const REPLY_TOKENS: u32 = 4_000;

pub const MODES: [&str; 2] = ["chat", "study"];

#[derive(Debug, Clone, Deserialize)]
pub struct Turn {
    pub role: String,
    pub text: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatBody {
    pub messages: Vec<Turn>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub refs: Vec<String>,
    #[serde(default)]
    pub chat: Option<String>,
    #[serde(default)]
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SourceRef {
    pub n: usize,
    pub id: String,
    pub title: String,
    pub folder: String,
    pub why: String,
}

struct Picked<'a> {
    note: &'a Note,
    why: String,
    most: usize,
}

pub(crate) fn studied(note: &Note) -> bool {
    !(note.title == leo_core::manual::MANUAL_TITLE && note.tags.iter().any(|t| t == "manual"))
}

pub(crate) fn clip(text: &str, most: usize) -> String {
    let mut out: String = text.chars().take(most).collect();
    if text.chars().count() > most {
        out.push_str("\n[…the rest of this note is left out]");
    }
    out
}

pub(crate) fn connected<'a>(
    store: &'a Store,
    cache: &'a Cache,
    id: &str,
) -> Vec<(&'a Note, String, String)> {
    let mut out: Vec<(u8, &Note, String, String)> = Vec::new();
    let mut seen = BTreeSet::new();
    for link in cache.all_links() {
        let other = if link.a == id {
            &link.b
        } else if link.b == id {
            &link.a
        } else {
            continue;
        };
        if !seen.insert(other.clone()) {
            continue;
        }
        if let Some(note) = store.notes.iter().find(|n| &n.id == other) {
            out.push((link.strength, note, link.kind.clone(), link.why.clone()));
        }
    }
    out.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.title.cmp(&b.1.title)));
    out.into_iter()
        .map(|(_, n, kind, why)| (n, kind, why))
        .collect()
}

pub(crate) fn attribute(text: &str) -> String {
    text.replace('"', "'").replace(['\n', '\r'], " ")
}

pub(crate) fn flat(text: &str) -> String {
    text.to_lowercase().replace(['-', '_'], " ")
}

fn match_score(note: &Note, read: Option<&crate::graph::Read>, words: &[String]) -> usize {
    let title = flat(&note.title);
    let body = flat(&note.body);
    let summary = read.map(|r| flat(&r.summary)).unwrap_or_default();
    let concepts: Vec<String> = read
        .map(|r| r.concepts.iter().map(|c| flat(c)).collect())
        .unwrap_or_default();
    words
        .iter()
        .map(|word| {
            let mut score = 0;
            if title.contains(word.as_str()) {
                score += 3;
            }
            if concepts
                .iter()
                .any(|c| c.split_whitespace().any(|part| part == word))
            {
                score += 3;
            }
            if summary.contains(word.as_str()) {
                score += 2;
            }
            if body.contains(word.as_str()) {
                score += 1;
            }
            score
        })
        .sum()
}

pub fn matches<'a>(store: &'a Store, cache: &Cache, question: &str, limit: usize) -> Vec<&'a Note> {
    let words: Vec<String> = leo_core::notes::question_words(&flat(question));
    if words.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(usize, &Note)> = store
        .notes
        .iter()
        .filter(|note| studied(note))
        .filter_map(|note| {
            let score = match_score(note, cache.notes.get(&note.id), &words);
            (score > 0).then_some((score, note))
        })
        .collect();
    scored.sort_by(|(sa, a), (sb, b)| sb.cmp(sa).then(b.updated_at.cmp(&a.updated_at)));
    scored.into_iter().take(limit).map(|(_, n)| n).collect()
}

pub fn question_of(messages: &[Turn]) -> String {
    messages
        .iter()
        .rev()
        .filter(|t| t.role == "user")
        .take(2)
        .map(|t| t.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn gather(
    store: &Store,
    cache: &Cache,
    open: Option<&str>,
    attached: &[String],
    question: &str,
    room: usize,
) -> (Vec<SourceRef>, String) {
    let room = room.clamp(LEAST_ROOM, MOST_ROOM);
    let share = |at_default: usize| at_default * room / ROOM;
    let (open_chars, attached_chars, note_chars) =
        (share(OPEN_CHARS), share(ATTACHED_CHARS), share(NOTE_CHARS));
    let mut picked: Vec<Picked> = Vec::new();
    let mut have = BTreeSet::new();
    for id in attached.iter().take(MOST_ATTACHED) {
        if let Some(note) = store.notes.iter().find(|n| &n.id == id) {
            if have.insert(note.id.clone()) {
                picked.push(Picked {
                    note,
                    why: "attached by the user".into(),
                    most: attached_chars,
                });
            }
        }
    }
    let open_note = open.and_then(|id| store.notes.iter().find(|n| n.id == id && studied(n)));
    if let Some(note) = open_note {
        if have.insert(note.id.clone()) {
            picked.push(Picked {
                note,
                why: "open".into(),
                most: open_chars,
            });
        }
        for (other, kind, why) in connected(store, cache, &note.id)
            .into_iter()
            .take(NEIGHBOURS)
        {
            if studied(other) && have.insert(other.id.clone()) {
                let reason = if why.is_empty() {
                    format!("connected ({kind})")
                } else {
                    format!("connected ({kind}): {why}")
                };
                picked.push(Picked {
                    note: other,
                    why: reason,
                    most: note_chars,
                });
            }
        }
    }
    let mut found: Vec<&Note> = Vec::new();
    for note in matches(store, cache, question, MATCHES * 2) {
        if found.len() >= MATCHES {
            break;
        }
        if have.insert(note.id.clone()) {
            found.push(note);
            picked.push(Picked {
                note,
                why: "matches the question".into(),
                most: note_chars,
            });
        }
    }
    for note in found.iter().take(EXPANDED_MATCHES) {
        for (other, kind, why) in connected(store, cache, &note.id)
            .into_iter()
            .filter(|(other, _, _)| studied(other))
            .take(NEIGHBOURS_PER_MATCH)
        {
            if have.insert(other.id.clone()) {
                let reason = if why.is_empty() {
                    format!("connected to {} ({kind})", note.title)
                } else {
                    format!("connected to {} ({kind}): {why}", note.title)
                };
                picked.push(Picked {
                    note: other,
                    why: reason,
                    most: note_chars,
                });
            }
        }
    }

    let mut sources = Vec::new();
    let mut text = String::new();
    for (i, p) in picked.iter().enumerate() {
        if text.chars().count() >= room {
            break;
        }
        let n = i + 1;
        let summary = cache
            .notes
            .get(&p.note.id)
            .map(|r| r.summary.as_str())
            .filter(|s| !s.is_empty());
        let links: Vec<String> = connected(store, cache, &p.note.id)
            .into_iter()
            .take(4)
            .map(|(other, kind, _)| format!("{kind} \"{}\"", other.title))
            .collect();
        let class = if p.note.directory.is_empty() {
            "unfiled".to_string()
        } else {
            p.note.directory.clone()
        };
        text.push_str(&format!(
            "<note id=\"n{n}\" title=\"{}\" class=\"{}\" included=\"{}\">\n",
            attribute(&p.note.title),
            attribute(&class),
            attribute(&p.why)
        ));
        if let Some(summary) = summary {
            text.push_str(&format!("Summary: {summary}\n"));
        }
        if !links.is_empty() {
            text.push_str(&format!("Connections: {}\n", links.join("; ")));
        }
        text.push_str(&clip(&p.note.body, p.most));
        text.push_str("\n</note>\n\n");
        sources.push(SourceRef {
            n,
            id: p.note.id.clone(),
            title: p.note.title.clone(),
            folder: p.note.directory.clone(),
            why: p.why.clone(),
        });
    }
    (sources, text)
}

const BASE: &str = "\
You are Felix, the friendly study buddy built into leo, the user's notes app. You work from the user's own notes, given in <note> tags with ids like n1. Each note says why it was included: notes the user attached to the conversation (treat these as what they are asking about), the note the user has open, notes that match the question, or notes connected to one of those in their knowledge graph (with the reason). Use those connections to relate ideas across notes and classes.

- Ground what you say in the notes and cite them with their id in square brackets right after the sentence, like [n2]. Cite only notes you actually used.
- When the notes do not cover something, say so in one short sentence, then answer from general knowledge under the words \"Beyond your notes:\". Never present general knowledge as if it came from the notes.
- The user may also give you documents, in <document> tags with ids like d1; they are files from their device, not notes. Use them when the question is about them and name the document when you use it, like (slides.pdf). Do not cite documents with square brackets.
- Use interpretable language: plain words someone new to the subject can follow, with each technical term explained the first time it appears.
- Point out connections between notes, especially across different classes, when they help.
- Write in Markdown: short paragraphs, bullet lists, bold key terms, fenced code blocks for code and formulas. Be concise and start with the answer, with no preamble.";

fn style(mode: &str) -> &'static str {
    match mode {
        "study" => "\
Mode: study. Help the user learn the material, not just read it. Use retrieval practice: ask them to recall before you tell, one question at a time, mixing recall, application and questions that connect two notes (start with the attached or open notes when there are any), and wait for the answer. When you judge an answer, begin your reply with [[correct]] if it was right or [[incorrect]] if it was wrong or incomplete, then say plainly what was right and what was not, give a hint before the full solution, cite the note, and ask the next question. When you are quizzing, keep a running score at the end of each reply, like (Score: 3/4). Do not reveal answers before the user tries. If they ask for a study plan, base it on the notes and spread review over days. Keep each turn short.",
        _ => "\
Mode: chat. Talk with the user the way a helpful assistant would: answer any question, help with writing, planning or thinking something through, and carry the conversation naturally. Use the notes whenever they are relevant, and always use the ones the user attached.
- When asked to explain something, use plain words first, an everyday analogy, one small worked example, then the precise version with the correct terms, and point out the most common misunderstanding.
- When the notes are meeting or work notes and the user asks for a review, give a two-sentence summary, decisions made, action items as a checklist (- [ ]) with the owner and due date only when the notes state them, open questions and risks, and draft a short follow-up message when asked. Never invent names, owners, dates or numbers.",
    }
}

pub fn documents_block(docs: &[(String, String)]) -> String {
    if docs.is_empty() {
        return String::new();
    }
    let share = DOCS_CHARS / docs.len();
    let mut out = String::from("<documents>\n");
    for (i, (name, text)) in docs.iter().enumerate() {
        out.push_str(&format!(
            "<document id=\"d{}\" name=\"{}\">\n{}\n</document>\n",
            i + 1,
            name.replace('"', "'"),
            clip(text.trim(), share)
        ));
    }
    out.push_str("</documents>\n\n");
    out
}

pub fn prompt(
    mode: &str,
    notes: &str,
    documents: &[(String, String)],
    messages: &[Turn],
) -> (String, String) {
    let system = format!("{BASE}\n\n{}", style(mode));
    let mut user = documents_block(documents);
    if notes.trim().is_empty() {
        user.push_str("<notes>\nNo notes matched this conversation.\n</notes>\n\n");
    } else {
        user.push_str("<notes>\n");
        user.push_str(notes);
        user.push_str("</notes>\n\n");
    }
    user.push_str("<conversation>\n");
    let start = messages.len().saturating_sub(TURNS);
    for turn in &messages[start..] {
        let who = if turn.role == "user" { "User" } else { "Felix" };
        user.push_str(&format!(
            "{who}: {}\n\n",
            clip(turn.text.trim(), TURN_CHARS)
        ));
    }
    user.push_str("</conversation>\n\nReply as Felix to the user's last message.");
    (system, user)
}

pub fn mode_of(requested: Option<&str>) -> &'static str {
    match requested {
        Some("study" | "quiz" | "coach") => "study",
        _ => "chat",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{NoteLink, Pair, Read};

    fn store() -> (Store, tempfile::TempDir, Vec<String>) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::load_from(&dir.path().join("notes")).unwrap();
        let mut ids = Vec::new();
        for (title, body, folder) in [
            (
                "Graph traversals",
                "BFS uses a queue and visits level by level.",
                "cs130",
            ),
            (
                "Scheduling",
                "Round robin takes the next process from the ready queue.",
                "cs162",
            ),
            ("Heaps", "A binary heap backs a priority queue.", "cs130"),
            (
                "Budget meeting",
                "Decided to cut travel. Sam sends the forecast Friday.",
                "work",
            ),
        ] {
            ids.push(
                store
                    .create_note(title, body, vec![], folder)
                    .unwrap()
                    .id
                    .clone(),
            );
        }
        store
            .create_note(
                leo_core::manual::MANUAL_TITLE,
                "queue queue queue",
                vec!["manual".into()],
                "",
            )
            .unwrap();
        (store, dir, ids)
    }

    fn cache(ids: &[String]) -> Cache {
        let mut cache = Cache::default();
        cache.notes.insert(
            ids[0].clone(),
            Read {
                hash: String::new(),
                summary: "How BFS explores a graph".into(),
                concepts: vec![],
                linked: None,
            },
        );
        cache.pairs.insert(
            "1:0-0".into(),
            Pair {
                hash: String::new(),
                links: vec![NoteLink {
                    a: ids[1].clone(),
                    b: ids[0].clone(),
                    kind: "same method".into(),
                    strength: 3,
                    why: "Both take the next item from a queue".into(),
                }],
            },
        );
        cache
    }

    #[test]
    fn the_open_note_comes_first_then_its_connections_then_matches() {
        let (store, _d, ids) = store();
        let cache = cache(&ids);
        let (sources, text) = gather(
            &store,
            &cache,
            Some(&ids[0]),
            &[],
            "what about priority queue heaps?",
            ROOM,
        );
        let titles: Vec<&str> = sources.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(titles[..3], ["Graph traversals", "Scheduling", "Heaps"]);
        assert_eq!(sources[0].why, "open");
        assert!(sources[1]
            .why
            .starts_with("connected (same method): Both take"));
        assert_eq!(sources[2].why, "matches the question");
        assert!(
            !titles.contains(&leo_core::manual::MANUAL_TITLE),
            "the manual is never study material"
        );
        assert!(text.contains(
            "<note id=\"n1\" title=\"Graph traversals\" class=\"cs130\" included=\"open\">"
        ));
        assert!(text.contains("Summary: How BFS explores a graph"));
        assert!(text.contains("Connections: same method \"Scheduling\""));
    }

    #[test]
    fn notes_the_user_attached_come_first_and_only_once() {
        let (store, _d, ids) = store();
        let attached = vec![
            ids[2].clone(),
            ids[0].clone(),
            "missing".to_string(),
            ids[2].clone(),
        ];
        let (sources, text) = gather(&store, &cache(&ids), Some(&ids[0]), &attached, "zzz", ROOM);
        let picked: Vec<&str> = sources.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(picked[..2], [ids[2].as_str(), ids[0].as_str()]);
        assert_eq!(sources[0].why, "attached by the user");
        assert_eq!(sources[1].why, "attached by the user");
        assert_eq!(picked.iter().filter(|id| **id == ids[0]).count(), 1);
        assert!(text.contains("included=\"attached by the user\""));
    }

    #[test]
    fn a_question_finds_notes_by_the_ideas_on_the_map_even_in_other_words() {
        let (store, _d, ids) = store();
        let mut cache = cache(&ids);
        cache.notes.get_mut(&ids[0]).unwrap().concepts =
            vec!["breadth-first search".into(), "queue".into()];
        assert!(
            store.relevant("explain breadth first search", 8).is_empty(),
            "the note's own words never say it"
        );
        let found = matches(&store, &cache, "explain breadth first search", 8);
        assert_eq!(found.first().map(|n| n.id.as_str()), Some(ids[0].as_str()));
        let summary = matches(&store, &cache, "how does something explore a graph?", 8);
        assert_eq!(
            summary.first().map(|n| n.id.as_str()),
            Some(ids[0].as_str())
        );
        assert!(matches(&store, &cache, "", 8).is_empty());
        assert!(
            !matches(&store, &cache, "queue", 8)
                .iter()
                .any(|n| n.title == leo_core::manual::MANUAL_TITLE),
            "the manual is never study material"
        );
    }

    #[test]
    fn a_title_with_quotes_cannot_break_the_note_tags() {
        assert_eq!(attribute("The \"Big O\"\nnotes"), "The 'Big O' notes");
    }

    #[test]
    fn notes_that_match_bring_their_connections_on_the_map() {
        let (store, _d, ids) = store();
        let cache = cache(&ids);
        let (sources, text) = gather(&store, &cache, None, &[], "round robin", ROOM);
        assert_eq!(sources[0].id, ids[1], "Scheduling matches the question");
        let joined = sources
            .iter()
            .find(|s| s.id == ids[0])
            .expect("its neighbour on the map comes along");
        assert_eq!(
            joined.why,
            "connected to Scheduling (same method): Both take the next item from a queue"
        );
        assert!(text.contains("included=\"connected to Scheduling (same method)"));
        assert_eq!(
            sources.iter().filter(|s| s.id == ids[0]).count(),
            1,
            "a note is never sent twice"
        );
    }

    #[test]
    fn without_an_open_note_the_question_picks_the_notes() {
        let (store, _d, ids) = store();
        let (sources, _) = gather(
            &store,
            &cache(&ids),
            None,
            &[],
            "who sends the forecast",
            ROOM,
        );
        assert_eq!(
            sources.first().map(|s| s.title.as_str()),
            Some("Budget meeting")
        );
        let (none, text) = gather(&store, &Cache::default(), None, &[], "zzz", ROOM);
        assert!(none.is_empty());
        assert!(text.is_empty());
    }

    #[test]
    fn long_notes_and_conversations_are_clipped() {
        let (mut store, _d, ids) = store();
        store.find_note_mut(&ids[0]).unwrap().body = "x".repeat(100_000);
        let (_, text) = gather(&store, &Cache::default(), Some(&ids[0]), &[], "x", ROOM);
        assert!(text.chars().count() < OPEN_CHARS + 2_000);
        let many: Vec<Turn> = (0..40)
            .map(|i| Turn {
                role: if i % 2 == 0 { "user" } else { "assistant" }.into(),
                text: format!("turn {i}"),
            })
            .collect();
        let (_, user) = prompt("ask", &text, &[], &many);
        assert!(!user.contains("turn 0\n"), "old turns are dropped");
        assert!(user.contains("User: turn 38"));
        assert!(user.contains("Felix: turn 39"));
    }

    #[test]
    fn there_are_two_styles_and_the_old_ones_land_in_them() {
        assert_eq!(mode_of(Some("study")), "study");
        assert_eq!(mode_of(Some("quiz")), "study");
        assert_eq!(mode_of(Some("coach")), "study");
        assert_eq!(mode_of(Some("explain")), "chat");
        assert_eq!(mode_of(Some("meeting")), "chat");
        assert_eq!(mode_of(Some("delete everything")), "chat");
        assert_eq!(mode_of(None), "chat");
        let turns = vec![Turn {
            role: "user".into(),
            text: "go".into(),
        }];
        let systems: BTreeSet<String> =
            MODES.iter().map(|m| prompt(m, "", &[], &turns).0).collect();
        assert_eq!(systems.len(), MODES.len());
        let (system, user) = prompt("study", "", &[], &turns);
        assert!(system.contains("Score: 3/4"));
        assert!(system.contains("[[correct]]"));
        assert!(system.contains("[n2]"));
        assert!(user.contains("No notes matched"));
        let chat = prompt("chat", "", &[], &turns).0;
        assert!(chat.contains("Never invent names"));
        assert!(chat.contains("everyday analogy"));
        assert!(!chat.contains("[[correct]] if"));
    }

    #[test]
    fn felix_is_told_to_use_interpretable_language_in_every_style() {
        for mode in MODES {
            assert!(prompt(mode, "", &[], &[])
                .0
                .contains("Use interpretable language"));
        }
    }

    #[test]
    fn documents_go_before_the_notes_and_share_a_budget() {
        let docs = vec![
            (
                "slides.pdf".to_string(),
                "Heaps keep the minimum at the root.".to_string(),
            ),
            ("big \"one\".txt".to_string(), "x".repeat(DOCS_CHARS)),
        ];
        let turns = vec![Turn {
            role: "user".into(),
            text: "what do the slides say?".into(),
        }];
        let (system, user) = prompt("chat", "<note id=\"n1\">", &docs, &turns);
        assert!(system.contains("<document> tags"));
        assert!(user.starts_with("<documents>\n<document id=\"d1\" name=\"slides.pdf\">"));
        assert!(user.contains("name=\"big 'one'.txt\""));
        assert!(user.find("</documents>").unwrap() < user.find("<notes>").unwrap());
        assert!(
            user.chars().count() < DOCS_CHARS + 2_000,
            "two documents share the budget"
        );
        assert!(documents_block(&[]).is_empty());
    }

    #[test]
    fn the_question_is_the_last_things_the_user_said() {
        let turns = vec![
            Turn {
                role: "user".into(),
                text: "tell me about heaps".into(),
            },
            Turn {
                role: "assistant".into(),
                text: "Heaps are trees".into(),
            },
            Turn {
                role: "user".into(),
                text: "and their running time?".into(),
            },
        ];
        assert_eq!(
            question_of(&turns),
            "and their running time? tell me about heaps"
        );
    }

    #[test]
    fn a_small_model_gets_fewer_notes_and_a_big_one_more() {
        let (mut store, _d, ids) = store();
        for i in 0..30 {
            store
                .create_note(
                    format!("Queue drill {i}"),
                    "queue ".repeat(2_000),
                    vec![],
                    "",
                )
                .unwrap();
        }
        let sizes: Vec<usize> = [LEAST_ROOM, ROOM, MOST_ROOM, 1]
            .iter()
            .map(|room| {
                gather(
                    &store,
                    &Cache::default(),
                    Some(&ids[0]),
                    &[],
                    "queue",
                    *room,
                )
                .1
                .chars()
                .count()
            })
            .collect();
        assert!(sizes[0] < sizes[1] && sizes[1] < sizes[2], "{sizes:?}");
        assert!(sizes[0] <= LEAST_ROOM + NOTE_CHARS + 600, "{sizes:?}");
        assert!(sizes[2] > 40_000, "{sizes:?}");
        assert_eq!(
            sizes[3], sizes[0],
            "a tiny room is raised to the least that is useful"
        );
    }
}
