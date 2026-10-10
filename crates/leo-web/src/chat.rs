use std::collections::BTreeSet;
use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use leo_core::notes::Note;
use leo_core::store::Store;

use crate::graph::Cache;

pub type Streamer = Arc<
    dyn Fn(&str, &str, u32, &mut dyn FnMut(&str), &mut dyn FnMut()) -> Result<Reply> + Send + Sync,
>;

#[derive(Debug, Clone, PartialEq)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub schema: serde_json::Value,
}

pub struct Exchange<'a> {
    pub tail: &'a str,
    pub last: bool,
    pub max_tokens: u32,
    pub most_calls: usize,
    pub piece: &'a mut dyn FnMut(&str),
    pub restart: &'a mut dyn FnMut(),
    pub call: &'a mut dyn FnMut(&str, &serde_json::Value) -> String,
}

pub trait Conversation: Send {
    fn native(&self) -> bool;
    fn say(&mut self, text: &str, exchange: Exchange<'_>) -> Result<Reply>;
}

pub struct Instructions<'a> {
    pub native: &'a str,
    pub text: &'a str,
    pub last: &'a str,
}

pub type Converser =
    Arc<dyn Fn(&Instructions, &[ToolSpec]) -> Option<Box<dyn Conversation>> + Send + Sync>;

pub struct Restated {
    streamer: Streamer,
    system: String,
    last_system: String,
    transcript: String,
}

impl Restated {
    pub fn new(streamer: Streamer, system: String, last_system: String) -> Restated {
        Restated {
            streamer,
            system,
            last_system,
            transcript: String::new(),
        }
    }
}

impl Conversation for Restated {
    fn native(&self) -> bool {
        false
    }

    fn say(&mut self, text: &str, exchange: Exchange<'_>) -> Result<Reply> {
        if !self.transcript.is_empty() {
            self.transcript.push_str("\n\n");
        }
        self.transcript.push_str(text);
        let asked = if exchange.tail.is_empty() {
            self.transcript.clone()
        } else {
            format!("{}\n\n{}", self.transcript, exchange.tail)
        };
        let system = if exchange.last {
            &self.last_system
        } else {
            &self.system
        };
        let reply = (self.streamer)(
            system,
            &asked,
            exchange.max_tokens,
            exchange.piece,
            exchange.restart,
        )?;
        self.transcript
            .push_str(&format!("\n\nFelix replied: {}", reply.text));
        Ok(reply)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Reply {
    pub text: String,
    pub spent: Option<Spent>,
}

impl From<String> for Reply {
    fn from(text: String) -> Reply {
        Reply { text, spent: None }
    }
}

impl From<&str> for Reply {
    fn from(text: &str) -> Reply {
        Reply::from(text.to_string())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Spent {
    pub by: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub input: u64,
    pub output: u64,
    pub estimated: bool,
    pub cost: Option<f64>,
    pub plan: bool,
    pub local: bool,
    #[serde(default)]
    pub cached: u64,
    pub steps: u32,
}

impl Spent {
    pub fn plus(self, more: Spent) -> Spent {
        let cost = match (self.cost, more.cost) {
            (Some(a), Some(b)) => Some(a + b),
            _ if self.by != more.by || self.model != more.model => None,
            (a, b) => a.or(b),
        };
        Spent {
            input: self.input + more.input,
            output: self.output + more.output,
            cached: self.cached + more.cached,
            estimated: self.estimated || more.estimated,
            cost,
            steps: self.steps + more.steps.max(1),
            ..more
        }
    }
}

const OPEN_CHARS: usize = 14_000;
const ATTACHED_CHARS: usize = 12_000;
pub const MOST_ATTACHED: usize = 8;
const NOTE_CHARS: usize = 4_000;
pub const ROOM: usize = 64_000;
pub const LEAST_ROOM: usize = 12_000;
pub const MOST_ROOM: usize = 480_000;
const NEIGHBOURS: usize = 5;
const MATCHES: usize = 8;
const EXPANDED_MATCHES: usize = 3;
const NEIGHBOURS_PER_MATCH: usize = 2;
const TURNS: usize = 14;
const DOCS_CHARS: usize = 60_000;
const TURN_CHARS: usize = 4_000;
pub const REPLY_TOKENS: u32 = 16_000;

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
    pub recent: Vec<String>,
    #[serde(default)]
    pub chat: Option<String>,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub access: Option<String>,
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

pub const MOST_RECENT: usize = 6;

pub fn gather(
    store: &Store,
    cache: &Cache,
    open: Option<&str>,
    attached: &[String],
    question: &str,
    room: usize,
) -> (Vec<SourceRef>, String) {
    gather_with(store, cache, open, attached, &[], question, room)
}

pub fn gather_with(
    store: &Store,
    cache: &Cache,
    open: Option<&str>,
    attached: &[String],
    recent: &[String],
    question: &str,
    room: usize,
) -> (Vec<SourceRef>, String) {
    gather_seeing(
        store,
        cache,
        open,
        attached,
        recent,
        question,
        room,
        None,
        &[],
    )
}

#[allow(clippy::too_many_arguments)]
pub fn gather_seeing(
    store: &Store,
    cache: &Cache,
    open: Option<&str>,
    attached: &[String],
    recent: &[String],
    question: &str,
    room: usize,
    captions: Option<&crate::captions::Captions>,
    close: &[(String, f32)],
) -> (Vec<SourceRef>, String) {
    let room = room.clamp(LEAST_ROOM, MOST_ROOM);
    let (open_chars, attached_chars, note_chars) = (
        scaled(OPEN_CHARS, room),
        scaled(ATTACHED_CHARS, room),
        scaled(NOTE_CHARS, room),
    );
    let (neighbours, matches_wanted, expanded, per_match) = (
        widened(NEIGHBOURS, room, 15),
        widened(MATCHES, room, 32),
        widened(EXPANDED_MATCHES, room, 8),
        widened(NEIGHBOURS_PER_MATCH, room, 4),
    );
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
    for id in recent.iter().take(MOST_RECENT) {
        if let Some(note) = store.notes.iter().find(|n| &n.id == id && studied(n)) {
            if have.insert(note.id.clone()) {
                picked.push(Picked {
                    note,
                    why: "used earlier in this chat".into(),
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
            .take(neighbours)
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
    for note in matches(store, cache, question, matches_wanted * 2) {
        if found.len() >= matches_wanted {
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
    let by_meaning = matches_wanted / 2 + 2;
    for (id, _) in close.iter().take(by_meaning * 2) {
        if found.len() >= matches_wanted + by_meaning {
            break;
        }
        if let Some(note) = store.notes.iter().find(|n| &n.id == id && studied(n)) {
            if have.insert(note.id.clone()) {
                found.push(note);
                picked.push(Picked {
                    note,
                    why: "close in meaning to the question".into(),
                    most: note_chars,
                });
            }
        }
    }
    for note in found.iter().take(expanded) {
        for (other, kind, why) in connected(store, cache, &note.id)
            .into_iter()
            .filter(|(other, _, _)| studied(other))
            .take(per_match)
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
        let body = match captions {
            Some(captions) => crate::captions::captioned(
                &store.notes_dir,
                &p.note.directory,
                &p.note.body,
                captions,
            ),
            None => p.note.body.clone(),
        };
        text.push_str(&clip(&body, p.most));
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
- When the user asks about their notes and the notes do not cover it, say so in one short sentence, then answer from general knowledge under the words \"Beyond your notes:\". Never present general knowledge as if it came from the notes. When the question is about a document they gave you, or is general (homework, a concept, a problem to solve), just answer it: do not announce that no notes matched.
- The user may also give you documents, in <document> tags with ids like d1; they are files from their device, not notes. Use them when the question is about them and name the document when you use it, like (slides.pdf). Do not cite documents with square brackets.
- Use interpretable language: plain words someone new to the subject can follow, with each technical term explained the first time it appears.
- The user can switch AI models and styles in the middle of a chat, so earlier Felix replies may have been written by another model. Treat the whole conversation as yours: stay consistent with it and build on it. Lines in square brackets at the start of a reply, like [What Felix did for this answer: …], [Felix asked: …] or [Practice question …], record what happened then; use them instead of looking the same things up again, and open a note again only when you need its full text.
- Point out connections between notes, especially across different classes, when they help.
- Write in Markdown: short paragraphs, bullet lists, bold key terms, fenced code blocks for code, and math in LaTeX: $...$ inside a sentence and $$...$$ on lines of their own.
- Match the length to the task. A quick question gets a short answer that starts with the answer, with no preamble. A problem, derivation, proof or homework question gets complete, teachable working: what is asked, the method and why it fits, each step with its formulas, the numbers (work every one out with the calculate tool; never estimate a value in your head), tables where a method iterates, the result clearly marked (for example $$\\boxed{x^* \\in [1.854, 2]}$$), and a short check or comment on whether the result makes sense.
- Homework and problem sets: read the whole assignment first so you know every question and part. Follow the order and scope the user asks for: \"start with question 1\" means do question 1, every part, thoroughly, then offer the next one. If you stop before the end, say exactly which questions or parts are left. Never skip a part silently, and never trade correctness for speed.
- Diagrams: when a picture makes something easier to understand (a process or algorithm, a cycle, a hierarchy, how ideas connect, a timeline, amounts to compare), or the user asks for a diagram, chart, graph, map or visualization, draw it as Mermaid in a ```mermaid code block, after one sentence saying what it shows; leo draws it. Use flowchart LR or TD for steps and how things connect, mindmap for a topic and its parts, sequenceDiagram for who talks to whom, stateDiagram-v2 for states, timeline for dates, classDiagram or erDiagram for structures, pie for shares, and xychart-beta for numbers. Keep labels short and put a label with punctuation in double quotes, like A[\"BFS (queue)\"]. One diagram per idea, never one just for decoration. A diagram can go into a note the same way, through edit_note or create_note, when the user wants it there.";

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
    documents_within(docs, DOCS_CHARS)
}

pub fn scaled(at_default: usize, room: usize) -> usize {
    at_default * room.clamp(LEAST_ROOM, MOST_ROOM) / ROOM
}

fn widened(count: usize, room: usize, most: usize) -> usize {
    scaled(count, room).clamp(count, most)
}

fn documents_within(docs: &[(String, String)], budget: usize) -> String {
    if docs.is_empty() {
        return String::new();
    }
    let share = budget / docs.len();
    let mut out = String::from("<documents>\n");
    for (i, (name, text)) in docs.iter().enumerate() {
        let text = text.trim();
        let more = if text.chars().count() > share {
            "\n[The document goes on. Use read_document to read any part of it.]"
        } else {
            ""
        };
        out.push_str(&format!(
            "<document id=\"d{}\" name=\"{}\">\n{}{more}\n</document>\n",
            i + 1,
            name.replace('"', "'"),
            clip(text, share)
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
    prompt_within(mode, notes, documents, messages, ROOM, None)
}

pub const MOST_TURNS: usize = 40;
pub const MEMORY_EVERY: usize = 6;
const EARLIER_CHARS: usize = 6_000;
const MEMORY_WORDS: u32 = 400;

pub fn turns_for(room: usize) -> usize {
    (TURNS * room.clamp(LEAST_ROOM, MOST_ROOM) / ROOM).clamp(TURNS, MOST_TURNS)
}

pub fn first_kept(messages: &[Turn], room: usize) -> usize {
    messages.len().saturating_sub(turns_for(room))
}

pub fn hash_of(messages: &[Turn]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for turn in messages {
        for byte in turn
            .role
            .bytes()
            .chain([0])
            .chain(turn.text.bytes())
            .chain([0])
        {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    }
    format!("{hash:016x}")
}

pub fn memory_fits(memory: &crate::chats::Memory, messages: &[Turn], start: usize) -> bool {
    memory.upto <= start
        && memory.upto <= messages.len()
        && hash_of(&messages[..memory.upto]) == memory.hash
}

fn line_of(turn: &Turn, most: usize) -> String {
    let who = if turn.role == "user" { "User" } else { "Felix" };
    let flat = turn.text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut text: String = flat.chars().take(most).collect();
    if flat.chars().count() > most {
        text.push('…');
    }
    format!("{who}: {text}")
}

pub fn earlier_of(
    messages: &[Turn],
    start: usize,
    memory: Option<&crate::chats::Memory>,
) -> String {
    let memory = memory.filter(|m| memory_fits(m, messages, start));
    let from = memory.map_or(0, |m| m.upto);
    let mut lines: Vec<String> = Vec::new();
    let mut used = 0;
    for turn in messages[from..start].iter().rev() {
        let line = line_of(turn, if turn.role == "user" { 400 } else { 240 });
        used += line.len();
        if used > EARLIER_CHARS {
            lines.push("…".into());
            break;
        }
        lines.push(line);
    }
    lines.reverse();
    let mut out = String::new();
    if let Some(memory) = memory {
        out.push_str(&format!(
            "What the chat covered before (a summary):\n{}\n",
            memory.text.trim()
        ));
    }
    if !lines.is_empty() {
        out.push_str(&format!(
            "Earlier messages, shortened:\n{}\n",
            lines.join("\n")
        ));
    }
    out
}

pub const MEMORY_RULES: &str = "You keep the memory of a long study chat between a student and Felix, their study buddy, so Felix can keep helping once older messages are out of view. Write a summary of at most 300 words in plain sentences and short lists: what the student is working on and why, what they asked and what Felix explained or changed in their notes, facts they gave about themselves, their course or deadlines, what they understood well and what they got wrong, decisions made, and questions still open. Keep names, numbers, note titles and terms exactly. Use interpretable language. Reply with the summary only.";

pub fn memory_prompt(old: Option<&crate::chats::Memory>, more: &[Turn]) -> (String, String, u32) {
    let mut user = String::new();
    if let Some(old) = old {
        user.push_str(&format!(
            "<summary_so_far>\n{}\n</summary_so_far>\n\n",
            old.text.trim()
        ));
    }
    user.push_str("<more_of_the_chat>\n");
    for turn in more {
        user.push_str(&line_of(turn, TURN_CHARS));
        user.push_str("\n\n");
    }
    user.push_str("</more_of_the_chat>\n\nWrite the updated summary: everything that matters from the summary so far and from these messages.");
    (MEMORY_RULES.to_string(), user, MEMORY_WORDS * 2)
}

pub fn prompt_within(
    mode: &str,
    notes: &str,
    documents: &[(String, String)],
    messages: &[Turn],
    room: usize,
    memory: Option<&crate::chats::Memory>,
) -> (String, String) {
    let system = format!("{BASE}\n\n{}", style(mode));
    let mut user = documents_within(documents, scaled(DOCS_CHARS, room));
    if notes.trim().is_empty() {
        user.push_str("<notes>\nNo notes matched this conversation.\n</notes>\n\n");
    } else {
        user.push_str("<notes>\n");
        user.push_str(notes);
        user.push_str("</notes>\n\n");
    }
    let start = first_kept(messages, room);
    let earlier = earlier_of(messages, start, memory);
    if !earlier.is_empty() {
        user.push_str(&format!(
            "<earlier_in_this_chat>\n{earlier}</earlier_in_this_chat>\n\n"
        ));
    }
    user.push_str("<conversation>\n");
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

    fn turns(n: usize) -> Vec<Turn> {
        (0..n)
            .map(|i| Turn {
                role: if i % 2 == 0 { "user" } else { "assistant" }.into(),
                text: format!("message {i}"),
            })
            .collect()
    }

    #[test]
    fn bigger_models_see_more_of_the_chat_word_for_word() {
        assert_eq!(turns_for(LEAST_ROOM), TURNS);
        assert_eq!(turns_for(ROOM), TURNS);
        assert_eq!(turns_for(ROOM * 2), TURNS * 2);
        assert_eq!(turns_for(MOST_ROOM), MOST_TURNS);
        assert_eq!(first_kept(&turns(10), ROOM), 0);
        assert_eq!(first_kept(&turns(20), ROOM), 6);
    }

    #[test]
    fn a_summary_is_used_only_while_it_matches_the_chat_it_was_made_from() {
        let messages = turns(20);
        let memory = crate::chats::Memory {
            upto: 4,
            hash: hash_of(&messages[..4]),
            text: "Summary of the start.".into(),
        };
        let with = earlier_of(&messages, 6, Some(&memory));
        assert!(
            with.starts_with("What the chat covered before (a summary):\nSummary of the start.")
        );
        assert!(with.contains("User: message 4\nFelix: message 5"));
        assert!(!with.contains("message 3"));
        let changed = crate::chats::Memory {
            hash: "0".into(),
            ..memory.clone()
        };
        let without = earlier_of(&messages, 6, Some(&changed));
        assert!(!without.contains("Summary of the start."));
        assert!(without.contains("User: message 0"));
        assert_eq!(earlier_of(&messages, 0, None), "");
        let long: Vec<Turn> = (0..200)
            .map(|i| Turn {
                role: "user".into(),
                text: format!("{i} {}", "word ".repeat(200)),
            })
            .collect();
        let clipped = earlier_of(&long, 190, None);
        assert!(clipped.len() < EARLIER_CHARS + 600);
        assert!(clipped.contains("User: 189 "), "the most recent are kept");
        assert!(!clipped.contains("User: 0 "));
    }

    #[test]
    fn notes_felix_used_earlier_in_the_chat_come_along_with_the_next_question() {
        let mut store =
            Store::load_from(&tempfile::tempdir().unwrap().keep().join("notes")).unwrap();
        let paper = store
            .create_note(
                "Neuro-Symbolic Drive",
                "The method pairs planner traces with trajectories.",
                vec![],
                "research",
            )
            .unwrap()
            .id
            .clone();
        let other = store
            .create_note("Heaps", "Minimum at the root.", vec![], "")
            .unwrap()
            .id
            .clone();
        let (sources, text) = gather_with(
            &store,
            &Cache::default(),
            None,
            &[],
            &[paper.clone(), "missing".into()],
            "what are the weaknesses?",
            ROOM,
        );
        assert_eq!(sources[0].id, paper);
        assert_eq!(sources[0].why, "used earlier in this chat");
        assert!(text.contains("planner traces"));
        assert!(!sources.iter().any(|s| s.id == other));
    }
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
        let conversation = user.split("<conversation>").nth(1).unwrap();
        assert!(
            !conversation.contains("turn 0\n"),
            "old turns leave the conversation"
        );
        assert!(
            user.contains("<earlier_in_this_chat>\nEarlier messages, shortened:\nUser: turn 0\n"),
            "and come along shortened"
        );
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
    fn felix_is_told_how_to_draw_diagrams_leo_can_show() {
        assert!(BASE.contains("```mermaid"));
        for kind in [
            "flowchart",
            "mindmap",
            "sequenceDiagram",
            "timeline",
            "pie",
            "xychart-beta",
        ] {
            assert!(BASE.contains(kind), "{kind}");
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
