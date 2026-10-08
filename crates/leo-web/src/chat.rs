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
const NOTE_CHARS: usize = 4_000;
const TOTAL_CHARS: usize = 48_000;
const NEIGHBOURS: usize = 5;
const MATCHES: usize = 8;
const TURNS: usize = 14;
const TURN_CHARS: usize = 4_000;
pub const REPLY_TOKENS: u32 = 4_000;

pub const MODES: [&str; 5] = ["ask", "coach", "quiz", "explain", "meeting"];

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

fn studied(note: &Note) -> bool {
    !(note.title == leo_core::manual::MANUAL_TITLE && note.tags.iter().any(|t| t == "manual"))
}

fn clip(text: &str, most: usize) -> String {
    let mut out: String = text.chars().take(most).collect();
    if text.chars().count() > most {
        out.push_str("\n[…the rest of this note is left out]");
    }
    out
}

fn connected<'a>(store: &'a Store, cache: &'a Cache, id: &str) -> Vec<(&'a Note, String, String)> {
    let mut out: Vec<(u8, &Note, String, String)> = Vec::new();
    let mut seen = BTreeSet::new();
    for link in cache.pairs.values().flat_map(|p| &p.links) {
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
    question: &str,
) -> (Vec<SourceRef>, String) {
    let mut picked: Vec<Picked> = Vec::new();
    let mut have = BTreeSet::new();
    let open_note = open.and_then(|id| store.notes.iter().find(|n| n.id == id && studied(n)));
    if let Some(note) = open_note {
        have.insert(note.id.clone());
        picked.push(Picked {
            note,
            why: "open".into(),
            most: OPEN_CHARS,
        });
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
                    most: NOTE_CHARS,
                });
            }
        }
    }
    for note in store.relevant(question, MATCHES * 2) {
        if picked.len() > MATCHES + NEIGHBOURS {
            break;
        }
        if studied(note) && have.insert(note.id.clone()) {
            picked.push(Picked {
                note,
                why: "matches the question".into(),
                most: NOTE_CHARS,
            });
        }
    }

    let mut sources = Vec::new();
    let mut text = String::new();
    for (i, p) in picked.iter().enumerate() {
        if text.chars().count() >= TOTAL_CHARS {
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
            "<note id=\"n{n}\" title=\"{}\" class=\"{class}\" included=\"{}\">\n",
            p.note.title, p.why
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
You are Felix, the friendly study buddy built into leo, the user's notes app. You work from the user's own notes, given in <note> tags with ids like n1. Each note says why it was included: the note the user has open, notes connected to it in their knowledge graph (with the reason), or notes that match the question.

- Ground what you say in the notes and cite them with their id in square brackets right after the sentence, like [n2]. Cite only notes you actually used.
- When the notes do not cover something, say so in one short sentence, then answer from general knowledge under the words \"Beyond your notes:\". Never present general knowledge as if it came from the notes.
- Point out connections between notes, especially across different classes, when they help.
- Write in Markdown: short paragraphs, bullet lists, bold key terms, fenced code blocks for code and formulas. Be concise and start with the answer, with no preamble.";

fn style(mode: &str) -> &'static str {
    match mode {
        "coach" => "\
Mode: study coach. Help the user learn the material, not just read it, using techniques that work: retrieval practice (ask them to recall before you tell), elaboration (ask why and how), connecting ideas across notes and classes, worked examples, and suggesting what to review again and when. Ask one question at a time and wait for the answer. When you judge an answer the user gave, begin your reply with [[correct]] if it was right or [[incorrect]] if it was wrong or incomplete, then say plainly what was right and what was not, give a hint before the full solution, and keep each turn short. If they ask for a study plan, base it on the notes and spread review over days.",
        "quiz" => "\
Mode: quiz. Quiz the user on the notes, starting with the open note if there is one. Ask one question at a time, mixing recall, application and questions that connect two notes. Wait for the answer. When you judge it, begin your reply with [[correct]] or [[incorrect]], then explain briefly with a citation, and ask the next question. Keep a running score at the end of each reply, like (Score: 3/4). Do not reveal answers before the user tries.",
        "explain" => "\
Mode: explain simply. Explain the topic the way the Feynman technique does: plain words first, an everyday analogy, one small worked example, then the precise version with the correct terms. Point out the most common misunderstanding. End with one short question that checks understanding.",
        "meeting" => "\
Mode: meeting and work notes. Treat the notes as meeting or work notes. When asked to review, give: a two-sentence summary, decisions made, action items as a checklist (- [ ]) with the owner and due date only when the notes state them, open questions, and risks. When asked, draft a short follow-up message. Never invent names, owners, dates or numbers.",
        _ => "\
Mode: ask. Answer the user's question directly from the notes.",
    }
}

pub fn prompt(mode: &str, notes: &str, messages: &[Turn]) -> (String, String) {
    let system = format!("{BASE}\n\n{}", style(mode));
    let mut user = String::new();
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
    requested
        .and_then(|m| MODES.iter().find(|known| **known == m))
        .copied()
        .unwrap_or("ask")
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
            "what about priority queue heaps?",
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
    fn without_an_open_note_the_question_picks_the_notes() {
        let (store, _d, ids) = store();
        let (sources, _) = gather(&store, &cache(&ids), None, "who sends the forecast");
        assert_eq!(
            sources.first().map(|s| s.title.as_str()),
            Some("Budget meeting")
        );
        let (none, text) = gather(&store, &Cache::default(), None, "zzz");
        assert!(none.is_empty());
        assert!(text.is_empty());
    }

    #[test]
    fn long_notes_and_conversations_are_clipped() {
        let (mut store, _d, ids) = store();
        store.find_note_mut(&ids[0]).unwrap().body = "x".repeat(100_000);
        let (_, text) = gather(&store, &Cache::default(), Some(&ids[0]), "x");
        assert!(text.chars().count() < OPEN_CHARS + 2_000);
        let many: Vec<Turn> = (0..40)
            .map(|i| Turn {
                role: if i % 2 == 0 { "user" } else { "assistant" }.into(),
                text: format!("turn {i}"),
            })
            .collect();
        let (_, user) = prompt("ask", &text, &many);
        assert!(!user.contains("turn 0\n"), "old turns are dropped");
        assert!(user.contains("User: turn 38"));
        assert!(user.contains("Felix: turn 39"));
    }

    #[test]
    fn each_mode_has_its_own_instructions_and_unknown_modes_ask() {
        assert_eq!(mode_of(Some("quiz")), "quiz");
        assert_eq!(mode_of(Some("delete everything")), "ask");
        assert_eq!(mode_of(None), "ask");
        let turns = vec![Turn {
            role: "user".into(),
            text: "go".into(),
        }];
        let systems: BTreeSet<String> = MODES.iter().map(|m| prompt(m, "", &turns).0).collect();
        assert_eq!(systems.len(), MODES.len());
        let (system, user) = prompt("quiz", "", &turns);
        assert!(system.contains("Score: 3/4"));
        assert!(system.contains("[[correct]]"));
        assert!(system.contains("[n2]"));
        assert!(user.contains("No notes matched"));
        assert!(prompt("meeting", "", &turns)
            .0
            .contains("Never invent names"));
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
}
