use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use leo_core::notes::Note;

pub type Writer = Arc<dyn Fn(&str, &str, u32) -> Result<String> + Send + Sync>;

const BATCH_NOTES: usize = 8;
const BATCH_CHARS: usize = 18_000;
const NOTE_CHARS: usize = 2_500;
const VOCABULARY: usize = 150;
const GROUP_NOTES: usize = 90;
const LINKS_PER_NOTE: usize = 4;
const CONCEPT_CHARS: usize = 48;
const SUMMARY_CHARS: usize = 200;
const WHY_CHARS: usize = 140;
const READ_TOKENS: u32 = 6_000;
const LINK_TOKENS: u32 = 12_000;

pub const KINDS: [&str; 5] = [
    "same idea",
    "same method",
    "builds on",
    "applies",
    "contrasts",
];

#[derive(Debug, Clone)]
pub struct Source {
    pub id: String,
    pub title: String,
    pub body: String,
    pub directory: String,
}

pub fn sources(notes: &[Note]) -> Vec<Source> {
    notes
        .iter()
        .filter(|n| {
            !(n.title == leo_core::manual::MANUAL_TITLE && n.tags.iter().any(|t| t == "manual"))
        })
        .map(|n| Source {
            id: n.id.clone(),
            title: n.title.clone(),
            body: n.body.clone(),
            directory: n.directory.clone(),
        })
        .collect()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Cache {
    #[serde(default)]
    pub notes: BTreeMap<String, Read>,
    #[serde(default)]
    pub links: Vec<NoteLink>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub pairs: BTreeMap<String, Pair>,
    #[serde(default)]
    pub built_at: Option<String>,
}

impl Cache {
    pub fn all_links(&self) -> impl Iterator<Item = &NoteLink> {
        self.links
            .iter()
            .chain(self.pairs.values().flat_map(|p| &p.links))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Read {
    pub hash: String,
    #[serde(default)]
    pub summary: String,
    pub concepts: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linked: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Pair {
    pub hash: String,
    pub links: Vec<NoteLink>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NoteLink {
    pub a: String,
    pub b: String,
    pub kind: String,
    pub strength: u8,
    pub why: String,
}

fn fnv(text: &str) -> String {
    format!("{:016x}", fnv64(text))
}

fn fnv64(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn hash_of(source: &Source) -> String {
    fnv(&format!("{}\u{0}{}", source.title, source.body))
}

fn key(label: &str) -> String {
    label.to_lowercase()
}

fn tidy(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn concept_label(raw: &str) -> Option<String> {
    let joined = tidy(raw);
    let trimmed = joined.trim_matches(|c: char| {
        matches!(
            c,
            '.' | ',' | ';' | ':' | '"' | '\'' | '`' | '*' | '-' | '#'
        )
    });
    let label: String = trimmed.chars().take(CONCEPT_CHARS).collect();
    let label = label.trim().to_string();
    (!label.is_empty()).then_some(label)
}

fn clip(text: &str, most: usize) -> String {
    let mut out: String = text.chars().take(most).collect();
    if text.chars().count() > most {
        out.push_str("\n…");
    }
    out
}

fn class_name(directory: &str) -> &str {
    if directory.is_empty() {
        "unfiled"
    } else {
        directory.split('/').next().unwrap_or(directory)
    }
}

pub fn plan(stale: &[&Source]) -> Vec<Vec<usize>> {
    let mut batches: Vec<Vec<usize>> = Vec::new();
    let mut chars = 0;
    for (i, source) in stale.iter().enumerate() {
        let size = source.title.chars().count() + source.body.chars().count().min(NOTE_CHARS);
        let full = batches
            .last()
            .is_none_or(|b| b.len() >= BATCH_NOTES || chars + size > BATCH_CHARS);
        if full {
            batches.push(Vec::new());
            chars = 0;
        }
        if let Some(batch) = batches.last_mut() {
            batch.push(i);
        }
        chars += size;
    }
    batches
}

pub fn json_in(reply: &str) -> Option<Value> {
    let start = reply.find('{')?;
    let end = reply.rfind('}')?;
    serde_json::from_str(reply.get(start..=end)?).ok()
}

struct Concept {
    label: String,
    notes: BTreeSet<String>,
}

fn concepts(sources: &[Source], cache: &Cache) -> BTreeMap<String, Concept> {
    let mut out: BTreeMap<String, Concept> = BTreeMap::new();
    for source in sources {
        let Some(read) = cache.notes.get(&source.id) else {
            continue;
        };
        for label in &read.concepts {
            out.entry(key(label))
                .or_insert_with(|| Concept {
                    label: label.clone(),
                    notes: BTreeSet::new(),
                })
                .notes
                .insert(source.id.clone());
        }
    }
    out
}

fn vocabulary(table: &BTreeMap<String, Concept>) -> Vec<String> {
    let mut list: Vec<(&String, &Concept)> = table.iter().collect();
    list.sort_by(|a, b| b.1.notes.len().cmp(&a.1.notes.len()).then(a.0.cmp(b.0)));
    list.into_iter()
        .take(VOCABULARY)
        .map(|(_, c)| c.label.clone())
        .collect()
}

const READ_RULES: &str = "\
You read a student's notes so they can be connected into a knowledge graph across classes.

For each note, give:
- summary: one sentence of at most 25 words saying what the note teaches. Use interpretable language: plain words someone new to the subject can follow.
- concepts: 3 to 8 key ideas and methods, as short noun phrases of 1 to 4 words: specific concepts, techniques, methods, algorithms, theorems, people or results. Include the methods used, not only the topic (for example \"dynamic programming\", \"proof by induction\", \"Fourier transform\"). Leave out generic words such as \"introduction\", \"overview\", \"lecture\", \"notes\" or \"example\".
When a concept is already in the vocabulary, use exactly that name, so the same idea always has the same name. Name a new concept the way a textbook would.

Reply with JSON only, no other text, in this shape:
{\"notes\": [{\"id\": \"n1\", \"summary\": \"How breadth-first search explores a graph level by level\", \"concepts\": [\"breadth-first search\", \"queue\"]}]}";

pub fn read_prompt(batch: &[&Source], vocabulary: &[String]) -> (String, String) {
    let mut user = String::from("<vocabulary>\n");
    for word in vocabulary {
        user.push_str(word);
        user.push('\n');
    }
    user.push_str("</vocabulary>\n\n");
    for (i, source) in batch.iter().enumerate() {
        user.push_str(&format!(
            "<note id=\"n{}\" class=\"{}\">\n# {}\n{}\n</note>\n\n",
            i + 1,
            class_name(&source.directory),
            source.title,
            clip(&source.body, NOTE_CHARS)
        ));
    }
    user.push_str("Give the summary and key concepts of each note, as JSON.");
    (READ_RULES.to_string(), user)
}

fn short_id(value: Option<&Value>, count: usize) -> Option<usize> {
    value
        .and_then(Value::as_str)
        .and_then(|id| id.trim().trim_start_matches('n').parse::<usize>().ok())
        .filter(|i| (1..=count).contains(i))
}

pub fn parse_read(reply: &str, count: usize) -> Option<Vec<(String, Vec<String>)>> {
    let value = json_in(reply)?;
    let notes = value.get("notes")?.as_array()?;
    let mut out = vec![(String::new(), Vec::new()); count];
    for note in notes {
        let Some(index) = short_id(note.get("id"), count) else {
            continue;
        };
        let summary = note
            .get("summary")
            .and_then(Value::as_str)
            .map(tidy)
            .unwrap_or_default();
        let mut seen = BTreeSet::new();
        let mut list = Vec::new();
        for raw in note
            .get("concepts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if let Some(label) = concept_label(raw) {
                if seen.insert(key(&label)) {
                    list.push(label);
                }
            }
        }
        out[index - 1] = (summary.chars().take(SUMMARY_CHARS).collect(), list);
    }
    Some(out)
}

const LINK_RULES: &str = "\
You connect a student's notes into a knowledge graph, so they can study related material together, especially across different classes.

Each note has an id, its class, a one-line summary and its key ideas and methods. Connect two notes when studying them together helps: they use the same method or technique, cover the same idea, one builds on the other, one applies the other, or they contrast two approaches to the same problem. Prefer connections between different classes, since those are the ones a student misses. Connect notes in the same class only when the connection is specific, never just because they share a class or a broad subject.

Reply with JSON only, no other text, in this shape:
{\"links\": [{\"a\": \"n1\", \"b\": \"n7\", \"kind\": \"same method\", \"strength\": 2, \"why\": \"Both break the problem into overlapping subproblems with dynamic programming\"}]}
kind is one of: same idea, same method, builds on, applies, contrasts. For builds on and applies, a is the note that builds on or applies b. strength is 1 (loosely related), 2 (clearly related) or 3 (worth studying together). why is one sentence of at most 15 words, specific to the two notes. Give each note at most 4 connections, its most useful ones, and leave a note unconnected rather than invent a link.";

fn note_line(n: usize, source: &Source, read: Option<&Read>) -> String {
    let summary = read.map(|r| r.summary.as_str()).unwrap_or("");
    let ideas = read.map(|r| r.concepts.join(", ")).unwrap_or_default();
    format!(
        "n{n} [{}] {} | {} | ideas: {}\n",
        class_name(&source.directory),
        tidy(&source.title),
        summary,
        ideas
    )
}

pub fn link_prompt(notes: &[&Source], cache: &Cache) -> (String, String) {
    let mut user = String::from("<notes>\n");
    for (i, source) in notes.iter().enumerate() {
        user.push_str(&note_line(i + 1, source, cache.notes.get(&source.id)));
    }
    user.push_str("</notes>\n\nConnect the notes worth studying together, as JSON.");
    (LINK_RULES.to_string(), user)
}

pub fn parse_links(reply: &str, notes: &[&Source]) -> Option<Vec<NoteLink>> {
    let value = json_in(reply)?;
    let links = value.get("links")?.as_array()?;
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for link in links {
        let (Some(a), Some(b)) = (
            short_id(link.get("a"), notes.len()),
            short_id(link.get("b"), notes.len()),
        ) else {
            continue;
        };
        if a == b {
            continue;
        }
        let (ida, idb) = (notes[a - 1].id.clone(), notes[b - 1].id.clone());
        if !seen.insert(ordered(&ida, &idb)) {
            continue;
        }
        let kind = link
            .get("kind")
            .and_then(Value::as_str)
            .map(|k| tidy(k).to_lowercase())
            .filter(|k| KINDS.contains(&k.as_str()))
            .unwrap_or_else(|| "same idea".to_string());
        let strength = link
            .get("strength")
            .and_then(Value::as_u64)
            .unwrap_or(2)
            .clamp(1, 3) as u8;
        let why: String = link
            .get("why")
            .and_then(Value::as_str)
            .map(tidy)
            .unwrap_or_default()
            .chars()
            .take(WHY_CHARS)
            .collect();
        out.push(NoteLink {
            a: ida,
            b: idb,
            kind,
            strength,
            why,
        });
        if out.len() >= notes.len() * LINKS_PER_NOTE {
            break;
        }
    }
    Some(out)
}

pub fn group_count(notes: usize) -> usize {
    notes.div_ceil(GROUP_NOTES).max(1)
}

fn link_key(source: &Source, read: &Read) -> String {
    fnv(&note_line(0, source, Some(read)))
}

pub struct Work<'a> {
    pub notes: Vec<&'a Source>,
    left: Option<BTreeSet<String>>,
    pub fresh: BTreeSet<String>,
}

fn chunks<'a>(list: &[&'a Source]) -> Vec<Vec<&'a Source>> {
    let groups = group_count(list.len());
    let size = list.len().div_ceil(groups).max(1);
    list.chunks(size).map(|c| c.to_vec()).collect()
}

pub fn link_work<'a>(sources: &'a [Source], cache: &Cache) -> Vec<Work<'a>> {
    let mut fresh: Vec<&Source> = Vec::new();
    let mut settled: Vec<&Source> = Vec::new();
    for source in sources {
        let Some(read) = cache
            .notes
            .get(&source.id)
            .filter(|r| r.hash == hash_of(source))
        else {
            continue;
        };
        if read.linked.as_deref() == Some(link_key(source, read).as_str()) {
            settled.push(source);
        } else {
            fresh.push(source);
        }
    }
    fresh.sort_by(|a, b| a.id.cmp(&b.id));
    settled.sort_by(|a, b| a.id.cmp(&b.id));
    let new_groups = chunks(&fresh);
    let old_groups = if settled.is_empty() {
        Vec::new()
    } else {
        chunks(&settled)
    };
    let ids = |list: &[&Source]| list.iter().map(|s| s.id.clone()).collect::<BTreeSet<_>>();
    let mut out = Vec::new();
    for (i, group) in new_groups.iter().enumerate() {
        for other in &new_groups[i..] {
            let same = std::ptr::eq(group, other);
            let mut notes = group.clone();
            if !same {
                notes.extend(other.iter().copied());
            }
            if notes.len() < 2 {
                continue;
            }
            let mut touched = ids(group);
            touched.extend(ids(other));
            out.push(Work {
                notes,
                left: (!same).then(|| ids(group)),
                fresh: touched,
            });
        }
        for old in &old_groups {
            let mut notes = group.clone();
            notes.extend(old.iter().copied());
            out.push(Work {
                notes,
                left: Some(ids(group)),
                fresh: ids(group),
            });
        }
    }
    out
}

pub fn stale<'a>(sources: &'a [Source], cache: &Cache) -> Vec<&'a Source> {
    sources
        .iter()
        .filter(|s| {
            cache
                .notes
                .get(&s.id)
                .is_none_or(|read| read.hash != hash_of(s))
        })
        .collect()
}

pub fn requests_needed(sources: &[Source], cache: &Cache) -> usize {
    let stale = stale(sources, cache);
    let reads = plan(&stale).len();
    if reads == 0 {
        return link_work(sources, cache).len();
    }
    let unread: BTreeSet<&str> = stale.iter().map(|s| s.id.as_str()).collect();
    let mut guess = cache.clone();
    for source in sources.iter().filter(|s| unread.contains(s.id.as_str())) {
        guess.notes.insert(
            source.id.clone(),
            Read {
                hash: hash_of(source),
                ..Read::default()
            },
        );
    }
    reads + link_work(sources, &guess).len()
}

fn settle_old_pairs(sources: &[Source], cache: &mut Cache) {
    if cache.pairs.is_empty() {
        return;
    }
    let old: Vec<NoteLink> = std::mem::take(&mut cache.pairs)
        .into_values()
        .flat_map(|p| p.links)
        .collect();
    cache.links.extend(old);
    for source in sources {
        if let Some(read) = cache.notes.get_mut(&source.id) {
            if read.hash == hash_of(source) && read.linked.is_none() {
                read.linked = Some(link_key(source, read));
            }
        }
    }
}

pub fn build(
    sources: &[Source],
    cache: &mut Cache,
    write: &dyn Fn(&str, &str, u32) -> Result<String>,
    progress: &mut dyn FnMut(usize, usize),
) -> Vec<String> {
    let present: BTreeSet<&str> = sources.iter().map(|s| s.id.as_str()).collect();
    cache.notes.retain(|id, _| present.contains(id.as_str()));
    settle_old_pairs(sources, cache);
    cache
        .links
        .retain(|l| present.contains(l.a.as_str()) && present.contains(l.b.as_str()));
    let stale = stale(sources, cache);
    let batches = plan(&stale);
    let mut total = requests_needed(sources, cache);
    let mut problems = Vec::new();
    let mut worked = batches.is_empty();
    progress(0, total);

    for (done, batch) in batches.iter().enumerate() {
        let notes: Vec<&Source> = batch.iter().map(|&i| stale[i]).collect();
        let (system, user) = read_prompt(&notes, &vocabulary(&concepts(sources, cache)));
        let read = write(&system, &user, READ_TOKENS)
            .map_err(|e| e.to_string())
            .and_then(|reply| {
                parse_read(&reply, notes.len())
                    .ok_or_else(|| "the AI did not answer in the expected form".to_string())
            });
        match read {
            Ok(list) => {
                worked = true;
                for (source, (summary, concepts)) in notes.iter().zip(list) {
                    let mut read = Read {
                        hash: hash_of(source),
                        summary,
                        concepts,
                        linked: None,
                    };
                    let key = link_key(source, &read);
                    let before = cache.notes.get(&source.id).and_then(|r| r.linked.clone());
                    read.linked = before.filter(|k| *k == key);
                    cache.notes.insert(source.id.clone(), read);
                }
            }
            Err(e) => problems.push(e),
        }
        progress(done + 1, total);
    }

    let work = link_work(sources, cache);
    let redo: BTreeSet<String> = work.iter().flat_map(|w| w.fresh.iter().cloned()).collect();
    cache
        .links
        .retain(|l| !redo.contains(&l.a) && !redo.contains(&l.b));
    total = batches.len() + work.len();
    progress(batches.len(), total);
    let mut failed: BTreeSet<String> = BTreeSet::new();
    for (done, job) in work.iter().enumerate() {
        let (system, user) = link_prompt(&job.notes, cache);
        let linked = write(&system, &user, LINK_TOKENS)
            .map_err(|e| e.to_string())
            .and_then(|reply| {
                parse_links(&reply, &job.notes)
                    .ok_or_else(|| "the AI did not answer in the expected form".to_string())
            });
        match linked {
            Ok(mut links) => {
                worked = true;
                if let Some(left) = &job.left {
                    links.retain(|l| left.contains(&l.a) != left.contains(&l.b));
                }
                cache.links.extend(links);
            }
            Err(e) => {
                failed.extend(job.fresh.iter().cloned());
                problems.push(e);
            }
        }
        progress(batches.len() + done + 1, total);
    }
    for source in sources {
        if failed.contains(&source.id) {
            continue;
        }
        if let Some(read) = cache.notes.get_mut(&source.id) {
            if read.hash == hash_of(source) {
                read.linked = Some(link_key(source, read));
            }
        }
    }
    progress(total, total);
    if worked {
        cache.built_at = Some(chrono::Utc::now().to_rfc3339());
    }
    problems
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Node {
    pub id: String,
    pub kind: &'static str,
    pub label: String,
    pub folder: String,
    pub summary: String,
    pub concepts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Edge {
    pub a: String,
    pub b: String,
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    pub strength: u8,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

pub fn wiki_targets(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(start) = rest.find("[[") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("]]") else {
            break;
        };
        let inner = &after[..end];
        let name = inner.split(['|', '#']).next().unwrap_or("").trim();
        if !name.is_empty() && !name.contains('\n') {
            out.push(name.to_lowercase());
        }
        rest = &after[end + 2..];
    }
    out
}

fn ordered(a: &str, b: &str) -> (String, String) {
    if a < b {
        (a.to_string(), b.to_string())
    } else {
        (b.to_string(), a.to_string())
    }
}

pub fn assemble(sources: &[Source], cache: &Cache) -> Graph {
    let present: BTreeSet<&str> = sources.iter().map(|s| s.id.as_str()).collect();
    let mut graph = Graph::default();
    for source in sources {
        let read = cache.notes.get(&source.id);
        graph.nodes.push(Node {
            id: format!("n:{}", source.id),
            kind: "note",
            label: if source.title.trim().is_empty() {
                "Untitled".to_string()
            } else {
                source.title.clone()
            },
            folder: source.directory.clone(),
            summary: read.map(|r| r.summary.clone()).unwrap_or_default(),
            concepts: read.map(|r| r.concepts.clone()).unwrap_or_default(),
        });
    }

    let mut best: BTreeMap<(String, String), &NoteLink> = BTreeMap::new();
    for link in cache.all_links() {
        if !present.contains(link.a.as_str()) || !present.contains(link.b.as_str()) {
            continue;
        }
        let pair = ordered(&link.a, &link.b);
        if best
            .get(&pair)
            .is_none_or(|old| link.strength > old.strength)
        {
            best.insert(pair, link);
        }
    }
    for link in best.values() {
        graph.edges.push(Edge {
            a: format!("n:{}", link.a),
            b: format!("n:{}", link.b),
            kind: "related",
            relation: Some(link.kind.clone()),
            why: (!link.why.is_empty()).then(|| link.why.clone()),
            strength: link.strength,
        });
    }

    let mut titles: HashMap<String, String> = HashMap::new();
    for source in sources {
        titles
            .entry(source.title.to_lowercase())
            .or_insert_with(|| source.id.clone());
    }
    let mut wired = BTreeSet::new();
    for source in sources {
        for target in wiki_targets(&source.body) {
            let Some(other) = titles.get(&target) else {
                continue;
            };
            if *other == source.id {
                continue;
            }
            let pair = ordered(&source.id, other);
            if wired.insert(pair.clone()) {
                graph.edges.push(Edge {
                    a: format!("n:{}", pair.0),
                    b: format!("n:{}", pair.1),
                    kind: "link",
                    relation: None,
                    why: None,
                    strength: 2,
                });
            }
        }
    }

    for (k, concept) in concepts(sources, cache) {
        if concept.notes.len() < 2 {
            continue;
        }
        graph.nodes.push(Node {
            id: format!("c:{k}"),
            kind: "concept",
            label: concept.label.clone(),
            folder: String::new(),
            summary: String::new(),
            concepts: Vec::new(),
        });
        for note in &concept.notes {
            graph.edges.push(Edge {
                a: format!("n:{note}"),
                b: format!("c:{k}"),
                kind: "covers",
                relation: None,
                why: None,
                strength: 1,
            });
        }
    }
    graph
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Status {
    pub state: &'static str,
    pub done: usize,
    pub total: usize,
    pub message: Option<String>,
    pub notes: usize,
    pub read: usize,
    pub stale: usize,
    pub requests: usize,
    pub rebuild_requests: usize,
    pub built_at: Option<String>,
}

#[derive(Debug, Clone)]
struct Job {
    state: &'static str,
    done: usize,
    total: usize,
    message: Option<String>,
}

pub struct Graphs {
    path: PathBuf,
    writer: Option<Writer>,
    job: Mutex<Job>,
}

impl Graphs {
    pub fn new(path: PathBuf, writer: Option<Writer>) -> Graphs {
        Graphs {
            path,
            writer,
            job: Mutex::new(Job {
                state: "idle",
                done: 0,
                total: 0,
                message: None,
            }),
        }
    }

    pub fn for_notes(notes_dir: &Path, writer: Option<Writer>) -> Graphs {
        let base = notes_dir.parent().unwrap_or(notes_dir);
        Graphs::new(base.join("graph.json"), writer)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn clear(&self) -> Result<bool> {
        let mut job = self.job.lock().unwrap_or_else(|e| e.into_inner());
        if job.state == "building" {
            return Ok(false);
        }
        match std::fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        *job = Job {
            state: "idle",
            done: 0,
            total: 0,
            message: None,
        };
        Ok(true)
    }

    pub fn load(&self) -> Cache {
        std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    fn save(&self, cache: &Cache) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let temp = self.path.with_extension("json.new");
        std::fs::write(&temp, serde_json::to_string(cache)?)?;
        std::fs::rename(&temp, &self.path)?;
        Ok(())
    }

    fn job(&self) -> Job {
        self.job
            .lock()
            .map(|j| j.clone())
            .unwrap_or_else(|e| e.into_inner().clone())
    }

    pub fn building(&self) -> Option<(usize, usize)> {
        let job = self.job();
        (job.state == "building").then_some((job.done, job.total))
    }

    fn set(&self, change: impl FnOnce(&mut Job)) {
        let mut job = self.job.lock().unwrap_or_else(|e| e.into_inner());
        change(&mut job);
    }

    pub fn status(&self, sources: &[Source]) -> Status {
        let cache = self.load();
        let job = self.job();
        let read = sources
            .iter()
            .filter(|s| cache.notes.contains_key(&s.id))
            .count();
        Status {
            state: job.state,
            done: job.done,
            total: job.total,
            message: job.message,
            notes: sources.len(),
            read,
            stale: stale(sources, &cache).len(),
            requests: requests_needed(sources, &cache),
            rebuild_requests: requests_needed(sources, &Cache::default()),
            built_at: cache.built_at,
        }
    }

    pub fn start(self: &Arc<Self>, sources: Vec<Source>) {
        {
            let mut job = self.job.lock().unwrap_or_else(|e| e.into_inner());
            if job.state == "building" {
                return;
            }
            let Some(_) = self.writer else {
                job.state = "failed";
                job.message = Some("leo serve was started without AI.".to_string());
                return;
            };
            *job = Job {
                state: "building",
                done: 0,
                total: 0,
                message: None,
            };
        }
        let graphs = Arc::clone(self);
        std::thread::spawn(move || graphs.run(&sources));
    }

    fn run(&self, sources: &[Source]) {
        let Some(writer) = self.writer.clone() else {
            return;
        };
        let mut cache = self.load();
        let problems = build(
            sources,
            &mut cache,
            &|system: &str, user: &str, most: u32| writer(system, user, most),
            &mut |done, total| {
                self.set(|job| {
                    job.done = done;
                    job.total = total;
                })
            },
        );
        let saved = self.save(&cache);
        self.set(|job| {
            let total = job.total.max(1);
            match (&saved, problems.first()) {
                (Err(e), _) => {
                    job.state = "failed";
                    job.message = Some(format!("Could not save the map: {e}"));
                }
                (Ok(()), Some(first)) if cache.built_at.is_none() || problems.len() >= total => {
                    job.state = "failed";
                    job.message = Some(first.clone());
                }
                (Ok(()), Some(first)) => {
                    job.state = "done";
                    job.message = Some(format!(
                        "Some notes could not be read this time ({first}); Update tries them again."
                    ));
                }
                (Ok(()), None) => {
                    job.state = "done";
                    job.message = None;
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn source(id: &str, title: &str, body: &str, directory: &str) -> Source {
        Source {
            id: id.into(),
            title: title.into(),
            body: body.into(),
            directory: directory.into(),
        }
    }

    fn library() -> Vec<Source> {
        vec![
            source(
                "a",
                "Graph traversals",
                "BFS uses a queue. See [[Stacks]].",
                "cs130",
            ),
            source("b", "Stacks", "LIFO. Used by DFS.", "cs130"),
            source("c", "Scheduling", "Round robin keeps a run queue.", "cs162"),
        ]
    }

    fn short_ids(user: &str) -> Vec<(String, String)> {
        user.lines()
            .filter_map(|line| {
                let (id, rest) = line.split_once(' ')?;
                (id.starts_with('n') && id[1..].parse::<usize>().is_ok())
                    .then(|| (id.to_string(), rest.to_string()))
            })
            .collect()
    }

    fn fake<'a>(
        reads: &'a AtomicUsize,
        links: &'a AtomicUsize,
    ) -> impl Fn(&str, &str, u32) -> Result<String> + 'a {
        move |system: &str, user: &str, _| {
            if system.starts_with("You read") {
                reads.fetch_add(1, Ordering::SeqCst);
                let mut notes = Vec::new();
                for (i, part) in user.split("<note id=").skip(1).enumerate() {
                    let (summary, concepts) = if part.contains("BFS") {
                        (
                            "Breadth-first search",
                            r#"["Breadth-First Search", "queue"]"#,
                        )
                    } else if part.contains("LIFO") {
                        ("Stacks and DFS", r#"["stack", "depth-first search"]"#)
                    } else {
                        ("Round robin scheduling", r#"["Queue", "round robin"]"#)
                    };
                    notes.push(format!(
                        r#"{{"id": "n{}", "summary": "{summary}", "concepts": {concepts}}}"#,
                        i + 1
                    ));
                }
                Ok(format!(
                    "```json\n{{\"notes\": [{}]}}\n```",
                    notes.join(",")
                ))
            } else {
                links.fetch_add(1, Ordering::SeqCst);
                let ids = short_ids(user);
                let find = |title: &str| {
                    ids.iter()
                        .find(|(_, rest)| rest.contains(title))
                        .map(|(id, _)| id.clone())
                        .unwrap_or_else(|| "n99".into())
                };
                let (a, b, c) = (find("Graph traversals"), find("Stacks"), find("Scheduling"));
                Ok(format!(
                    r#"Here: {{"links": [
                        {{"a": "{a}", "b": "{c}", "kind": "Same Idea", "strength": 3, "why": "both  keep work waiting in a queue"}},
                        {{"a": "{a}", "b": "{b}", "kind": "builds on", "strength": 9, "why": "DFS needs a stack"}},
                        {{"a": "{c}", "b": "{a}", "kind": "same idea", "strength": 1, "why": "duplicate pair"}},
                        {{"a": "{a}", "b": "{a}", "kind": "same idea", "why": "self"}},
                        {{"a": "{a}", "b": "n99", "kind": "same idea", "why": "unknown"}},
                        {{"a": "{b}", "b": "{c}", "kind": "made up", "why": "odd kind"}}
                    ]}}"#
                ))
            }
        }
    }

    #[test]
    fn notes_are_read_once_and_linked_to_each_other_with_reasons() {
        let (reads, links) = (AtomicUsize::new(0), AtomicUsize::new(0));
        let write = fake(&reads, &links);
        let sources = library();
        let mut cache = Cache::default();
        let mut steps = Vec::new();
        let problems = build(&sources, &mut cache, &write, &mut |d, t| steps.push((d, t)));
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(
            (reads.load(Ordering::SeqCst), links.load(Ordering::SeqCst)),
            (1, 1)
        );
        assert_eq!(steps.last(), Some(&(2, 2)));
        assert_eq!(cache.notes["a"].summary, "Breadth-first search");
        assert_eq!(cache.notes["a"].concepts, ["Breadth-First Search", "queue"]);

        let graph = assemble(&sources, &cache);
        let edge = |a: &str, b: &str, kind: &str| {
            graph
                .edges
                .iter()
                .find(|e| e.kind == kind && ((e.a == a && e.b == b) || (e.a == b && e.b == a)))
        };
        let across = edge("n:a", "n:c", "related").expect("the cross-class link");
        assert_eq!(across.relation.as_deref(), Some("same idea"));
        assert_eq!(across.strength, 3, "the first of a repeated pair is kept");
        assert_eq!(
            across.why.as_deref(),
            Some("both keep work waiting in a queue")
        );
        assert_eq!(
            edge("n:a", "n:b", "related").unwrap().strength,
            3,
            "strength is clamped"
        );
        assert_eq!(
            edge("n:b", "n:c", "related").unwrap().relation.as_deref(),
            Some("same idea")
        );
        assert_eq!(
            graph.edges.iter().filter(|e| e.kind == "related").count(),
            3
        );
        assert!(
            edge("n:a", "n:b", "link").is_some(),
            "the [[Stacks]] link joins the notes"
        );
        assert!(edge("n:a", "c:queue", "covers").is_some());
        assert!(
            edge("n:c", "c:queue", "covers").is_some(),
            "Queue and queue are one idea"
        );
        assert!(
            !graph.nodes.iter().any(|n| n.id == "c:stack"),
            "an idea in one note is not drawn"
        );
        let note = graph.nodes.iter().find(|n| n.id == "n:c").unwrap();
        assert_eq!(note.summary, "Round robin scheduling");

        let again = build(&sources, &mut cache, &write, &mut |_, _| {});
        assert!(again.is_empty());
        assert_eq!(
            (reads.load(Ordering::SeqCst), links.load(Ordering::SeqCst)),
            (1, 1),
            "nothing changed, nothing asked"
        );
        assert_eq!(requests_needed(&sources, &cache), 0);

        let mut changed = sources.clone();
        changed[1].body = "LIFO. Used by DFS and recursion.".into();
        assert_eq!(requests_needed(&changed, &cache), 2);
        build(&changed, &mut cache, &write, &mut |_, _| {});
        assert_eq!(
            reads.load(Ordering::SeqCst),
            2,
            "only the edited note is read again"
        );
        assert_eq!(
            links.load(Ordering::SeqCst),
            1,
            "the same summaries and ideas need no new links"
        );

        changed.remove(2);
        build(&changed, &mut cache, &write, &mut |_, _| {});
        assert!(
            !cache.notes.contains_key("c"),
            "a deleted note leaves the map"
        );
        let graph = assemble(&changed, &cache);
        assert!(graph.edges.iter().all(|e| e.a != "n:c" && e.b != "n:c"));
    }

    #[test]
    fn a_large_library_is_linked_in_groups_covering_every_pair_once() {
        let many: Vec<Source> = (0..200)
            .map(|i| source(&format!("{i:03}"), &format!("Note {i}"), "x", ""))
            .collect();
        let mut cache = Cache::default();
        for s in &many {
            cache.notes.insert(
                s.id.clone(),
                Read {
                    hash: hash_of(s),
                    summary: "s".into(),
                    concepts: vec![],
                    linked: None,
                },
            );
        }
        let work = link_work(&many, &cache);
        assert_eq!(group_count(200), 3);
        assert_eq!(work.len(), 6);
        let mut covered = BTreeSet::new();
        for w in &work {
            let ids: Vec<&str> = w.notes.iter().map(|s| s.id.as_str()).collect();
            for (i, a) in ids.iter().enumerate() {
                for b in &ids[i + 1..] {
                    covered.insert(ordered(a, b));
                }
            }
        }
        assert_eq!(
            covered.len(),
            200 * 199 / 2,
            "every pair of notes is seen by some request"
        );

        let cross = work.iter().find(|w| w.left.is_some()).unwrap();
        let left = cross.left.clone().unwrap();
        let at = |pick: &dyn Fn(&&Source) -> bool, nth: usize| {
            cross
                .notes
                .iter()
                .enumerate()
                .filter(|(_, s)| pick(s))
                .nth(nth)
                .unwrap()
                .0
                + 1
        };
        let l1 = at(&|s: &&Source| left.contains(&s.id), 0);
        let l2 = at(&|s: &&Source| left.contains(&s.id), 1);
        let r1 = at(&|s: &&Source| !left.contains(&s.id), 0);
        let reply = format!(
            r#"{{"links": [{{"a": "n{l1}", "b": "n{r1}", "kind": "same idea", "why": "across"}}, {{"a": "n{l1}", "b": "n{l2}", "kind": "same idea", "why": "inside"}}]}}"#
        );
        let first = cross.notes[0].id.clone();
        let size = cross.notes.len();
        let write = |_: &str, user: &str, _: u32| -> Result<String> {
            let lines = short_ids(user);
            if lines.len() == size
                && lines[0].1.starts_with(&format!(
                    "[unfiled] Note {}",
                    first.trim_start_matches('0').parse::<usize>().unwrap_or(0)
                ))
            {
                Ok(reply.clone())
            } else {
                Ok(r#"{"links": []}"#.into())
            }
        };
        build(&many, &mut cache, &write, &mut |_, _| {});
        let kept = &cache.links;
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert_eq!(
            kept[0].why, "across",
            "a pair inside one group is left to that group's own request"
        );
        assert!(
            link_work(&many, &cache).is_empty(),
            "every note is linked now"
        );
    }

    fn linked_library(count: usize) -> (Vec<Source>, Cache) {
        let many: Vec<Source> = (0..count)
            .map(|i| source(&format!("{i:03}"), &format!("Note {i}"), "x", ""))
            .collect();
        let mut cache = Cache::default();
        for s in &many {
            let mut read = Read {
                hash: hash_of(s),
                summary: format!("about {}", s.id),
                concepts: vec![],
                linked: None,
            };
            read.linked = Some(link_key(s, &read));
            cache.notes.insert(s.id.clone(), read);
        }
        cache.links.push(NoteLink {
            a: "000".into(),
            b: "001".into(),
            kind: "same idea".into(),
            strength: 2,
            why: "kept".into(),
        });
        (many, cache)
    }

    #[test]
    fn a_new_note_is_linked_to_the_others_without_redoing_their_links() {
        let (mut many, mut cache) = linked_library(200);
        many.push(source("new", "Fresh note", "brand new", ""));
        let calls = AtomicUsize::new(0);
        let seen = Mutex::new(Vec::new());
        let write = |_: &str, user: &str, _: u32| -> Result<String> {
            calls.fetch_add(1, Ordering::SeqCst);
            if user.contains("<note id=") {
                return Ok(
                    r#"{"notes": [{"id": "n1", "summary": "fresh", "concepts": []}]}"#.into(),
                );
            }
            let lines = short_ids(user);
            seen.lock().unwrap().push(lines.len());
            let new = lines
                .iter()
                .find(|(_, l)| l.contains("Fresh note"))
                .unwrap()
                .0
                .clone();
            let other = lines
                .iter()
                .find(|(_, l)| !l.contains("Fresh note"))
                .unwrap()
                .0
                .clone();
            let third = lines
                .iter()
                .filter(|(_, l)| !l.contains("Fresh note"))
                .nth(1)
                .unwrap()
                .0
                .clone();
            Ok(format!(
                r#"{{"links": [{{"a": "{new}", "b": "{other}", "kind": "same idea", "why": "new one"}}, {{"a": "{other}", "b": "{third}", "kind": "same idea", "why": "old pair"}}]}}"#
            ))
        };
        assert_eq!(requests_needed(&many, &cache), 1 + 3);
        let problems = build(&many, &mut cache, &write, &mut |_, _| {});
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1 + 3,
            "one read, then the new note against each group"
        );
        assert!(
            seen.lock().unwrap().iter().all(|n| *n <= 68),
            "{:?}",
            seen.lock().unwrap()
        );
        assert!(
            cache.links.iter().any(|l| l.why == "kept"),
            "old links stay"
        );
        assert_eq!(cache.links.iter().filter(|l| l.why == "new one").count(), 3);
        assert!(
            !cache.links.iter().any(|l| l.why == "old pair"),
            "links between two old notes are not redone"
        );
        assert_eq!(requests_needed(&many, &cache), 0);
    }

    #[test]
    fn a_new_note_whose_links_failed_is_tried_again() {
        let (mut many, mut cache) = linked_library(5);
        many.push(source("new", "Fresh note", "brand new", ""));
        let read_only = |_: &str, user: &str, _: u32| -> Result<String> {
            if user.contains("<note id=") {
                Ok(r#"{"notes": [{"id": "n1", "summary": "fresh", "concepts": []}]}"#.into())
            } else {
                anyhow::bail!("rate limited")
            }
        };
        let problems = build(&many, &mut cache, &read_only, &mut |_, _| {});
        assert_eq!(problems, ["rate limited"]);
        assert_eq!(cache.notes["new"].linked, None);
        assert_eq!(requests_needed(&many, &cache), 1);
        assert!(cache.links.iter().any(|l| l.why == "kept"));
    }

    #[test]
    fn a_map_saved_by_an_older_leo_keeps_its_links_and_asks_for_nothing() {
        let sources = library();
        let mut cache = Cache::default();
        for s in &sources {
            cache.notes.insert(
                s.id.clone(),
                Read {
                    hash: hash_of(s),
                    summary: "s".into(),
                    concepts: vec![],
                    linked: None,
                },
            );
        }
        cache.pairs.insert(
            "1:0-0".into(),
            Pair {
                hash: "old".into(),
                links: vec![NoteLink {
                    a: "a".into(),
                    b: "c".into(),
                    kind: "same idea".into(),
                    strength: 3,
                    why: "from before".into(),
                }],
            },
        );
        let asked = AtomicUsize::new(0);
        let write = |_: &str, _: &str, _: u32| -> Result<String> {
            asked.fetch_add(1, Ordering::SeqCst);
            Ok(r#"{"links": []}"#.into())
        };
        build(&sources, &mut cache, &write, &mut |_, _| {});
        assert_eq!(asked.load(Ordering::SeqCst), 0);
        assert!(cache.pairs.is_empty());
        assert_eq!(cache.links.len(), 1);
        let graph = assemble(&sources, &cache);
        assert!(graph
            .edges
            .iter()
            .any(|e| e.why.as_deref() == Some("from before")));
    }

    #[test]
    fn a_failed_reply_leaves_the_notes_to_try_again() {
        let sources = library();
        let mut cache = Cache::default();
        let write = |_: &str, _: &str, _: u32| -> Result<String> {
            Ok("Sorry, I can't help with that.".into())
        };
        let problems = build(&sources, &mut cache, &write, &mut |_, _| {});
        assert!(!problems.is_empty());
        assert!(cache.notes.is_empty());
        assert!(cache.built_at.is_none());
        assert_eq!(stale(&sources, &cache).len(), 3);
        let down = |_: &str, _: &str, _: u32| -> Result<String> {
            anyhow::bail!("no AI for writing is chosen")
        };
        let problems = build(&sources, &mut cache, &down, &mut |_, _| {});
        assert_eq!(problems[0], "no AI for writing is chosen");
    }

    #[test]
    fn batches_respect_note_and_size_limits() {
        let many: Vec<Source> = (0..20)
            .map(|i| {
                source(
                    &i.to_string(),
                    "t",
                    &"x".repeat(if i == 3 { 30_000 } else { 100 }),
                    "",
                )
            })
            .collect();
        let refs: Vec<&Source> = many.iter().collect();
        let batches = plan(&refs);
        assert!(batches
            .iter()
            .all(|b| !b.is_empty() && b.len() <= BATCH_NOTES));
        assert_eq!(batches.iter().map(Vec::len).sum::<usize>(), 20);
        let (_, user) = read_prompt(&[&many[3]], &[]);
        assert!(
            user.chars().count() < NOTE_CHARS + 400,
            "a long note is clipped"
        );
    }

    #[test]
    fn concept_names_are_tidied() {
        assert_eq!(
            concept_label("  **Big-O   notation.** "),
            Some("Big-O notation".into())
        );
        assert_eq!(concept_label("- queue"), Some("queue".into()));
        assert_eq!(concept_label("  ... "), None);
        assert_eq!(
            concept_label(&"x".repeat(200)).unwrap().chars().count(),
            CONCEPT_CHARS
        );
    }

    #[test]
    fn wiki_links_name_their_target() {
        assert_eq!(
            wiki_targets(
                "see [[Stacks]], [[Heaps|a heap]] and [[Trees#AVL]] but not [[ ]] or [[open"
            ),
            ["stacks", "heaps", "trees"]
        );
    }

    #[test]
    fn the_manual_is_not_study_material() {
        let mut store =
            leo_core::store::Store::load_from(&tempfile::tempdir().unwrap().path().join("notes"))
                .unwrap();
        store
            .create_note(
                leo_core::manual::MANUAL_TITLE,
                "help",
                vec!["manual".into()],
                "",
            )
            .unwrap();
        store.create_note("Graphs", "BFS", vec![], "cs130").unwrap();
        let list = sources(&store.notes);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "Graphs");
    }

    #[test]
    fn a_build_runs_in_the_background_and_is_saved() {
        let dir = tempfile::tempdir().unwrap();
        let writer: Writer = Arc::new(|system: &str, _: &str, _| {
            if system.starts_with("You read") {
                Ok(r#"{"notes": [{"id": "n1", "summary": "one", "concepts": ["queue"]}, {"id": "n2", "summary": "two", "concepts": ["stack"]}, {"id": "n3", "summary": "three", "concepts": ["queue"]}]}"#.into())
            } else {
                Ok(r#"{"links": [{"a": "n1", "b": "n2", "kind": "contrasts", "strength": 2, "why": "a queue and a stack order work oppositely"}]}"#.into())
            }
        });
        let graphs = Arc::new(Graphs::for_notes(&dir.path().join("notes"), Some(writer)));
        let sources = library();
        assert_eq!(graphs.status(&sources).requests, 2);
        graphs.start(sources.clone());
        let started = std::time::Instant::now();
        while graphs.status(&sources).state == "building" {
            assert!(started.elapsed().as_secs() < 10, "the build never finished");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let status = graphs.status(&sources);
        assert_eq!(status.state, "done", "{status:?}");
        assert_eq!((status.read, status.stale, status.requests), (3, 0, 0));
        assert!(dir.path().join("graph.json").is_file());
        let graph = assemble(&sources, &graphs.load());
        assert!(graph.edges.iter().any(|e| e.kind == "related"
            && e.relation.as_deref() == Some("contrasts")
            && e.why.as_deref() == Some("a queue and a stack order work oppositely")));

        let without = Arc::new(Graphs::for_notes(&dir.path().join("other/notes"), None));
        without.start(sources.clone());
        assert_eq!(without.status(&sources).state, "failed");
    }
}
