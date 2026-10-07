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
const LINK_CONCEPTS: usize = 250;
const MOST_LINKS: usize = 60;
const CONCEPT_CHARS: usize = 48;
const WHY_CHARS: usize = 120;
const READ_TOKENS: u32 = 4_000;
const LINK_TOKENS: u32 = 6_000;

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
    pub links: Links,
    #[serde(default)]
    pub built_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Read {
    pub hash: String,
    pub concepts: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Links {
    pub key: String,
    pub items: Vec<Link>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Link {
    pub a: String,
    pub b: String,
    pub why: String,
}

fn fnv(text: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn hash_of(source: &Source) -> String {
    fnv(&format!("{}\u{0}{}", source.title, source.body))
}

fn key(label: &str) -> String {
    label.to_lowercase()
}

pub fn concept_label(raw: &str) -> Option<String> {
    let words: Vec<&str> = raw.split_whitespace().collect();
    let joined = words.join(" ");
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

fn folder_name(directory: &str) -> &str {
    if directory.is_empty() {
        "top level"
    } else {
        directory
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
    folders: BTreeSet<String>,
}

fn concepts(sources: &[Source], cache: &Cache) -> BTreeMap<String, Concept> {
    let mut out: BTreeMap<String, Concept> = BTreeMap::new();
    for source in sources {
        let Some(read) = cache.notes.get(&source.id) else {
            continue;
        };
        for label in &read.concepts {
            let entry = out.entry(key(label)).or_insert_with(|| Concept {
                label: label.clone(),
                notes: BTreeSet::new(),
                folders: BTreeSet::new(),
            });
            entry.notes.insert(source.id.clone());
            entry
                .folders
                .insert(folder_name(&source.directory).to_string());
        }
    }
    out
}

fn by_use(table: &BTreeMap<String, Concept>, most: usize) -> Vec<(&String, &Concept)> {
    let mut list: Vec<(&String, &Concept)> = table.iter().collect();
    list.sort_by(|a, b| b.1.notes.len().cmp(&a.1.notes.len()).then(a.0.cmp(b.0)));
    list.truncate(most);
    list
}

const READ_RULES: &str = "\
You read a student's notes and name the ideas each one teaches, so that notes about the same idea can be linked.

For each note, list 3 to 8 key concepts: specific ideas, methods, terms, people or results, as short noun phrases of 1 to 4 words. Leave out generic words such as \"introduction\", \"overview\", \"lecture\", \"notes\" or \"example\".
When a concept is already in the vocabulary, use exactly that name, so the same idea always has the same name. Name a new concept the way a textbook would.

Reply with JSON only, no other text, in this shape:
{\"notes\": [{\"id\": \"n1\", \"concepts\": [\"breadth-first search\", \"queue\"]}]}";

pub fn read_prompt(batch: &[&Source], vocabulary: &[String]) -> (String, String) {
    let mut user = String::from("<vocabulary>\n");
    for word in vocabulary {
        user.push_str(word);
        user.push('\n');
    }
    user.push_str("</vocabulary>\n\n");
    for (i, source) in batch.iter().enumerate() {
        user.push_str(&format!(
            "<note id=\"n{}\" folder=\"{}\">\n# {}\n{}\n</note>\n\n",
            i + 1,
            folder_name(&source.directory),
            source.title,
            clip(&source.body, NOTE_CHARS)
        ));
    }
    user.push_str("List the key concepts of each note, as JSON.");
    (READ_RULES.to_string(), user)
}

pub fn parse_read(reply: &str, count: usize) -> Option<Vec<Vec<String>>> {
    let value = json_in(reply)?;
    let notes = value.get("notes")?.as_array()?;
    let mut out = vec![Vec::new(); count];
    for note in notes {
        let Some(index) = note
            .get("id")
            .and_then(Value::as_str)
            .and_then(|id| id.trim().trim_start_matches('n').parse::<usize>().ok())
            .filter(|i| (1..=count).contains(i))
        else {
            continue;
        };
        let mut seen = BTreeSet::new();
        for raw in note
            .get("concepts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if let Some(label) = concept_label(raw) {
                if seen.insert(key(&label)) {
                    out[index - 1].push(label);
                }
            }
        }
    }
    Some(out)
}

const LINK_RULES: &str = "\
You find how the ideas in a student's notes connect, so they can study them together.

You get concepts, each with the folders (subjects) it appears in. List the most useful connections between pairs of them: one is a prerequisite of the other, one applies or generalises the other, they are the same idea under different names, they contrast, or one is an example of the other. Prefer connections between different folders, since those are the ones a student misses, and skip two concepts that only ever appear in the same note, since that note already ties them together. Use the exact names from the list, and connect two concepts only when the connection is real.

Reply with JSON only, no other text, in this shape:
{\"links\": [{\"a\": \"breadth-first search\", \"b\": \"queue\", \"why\": \"BFS keeps the nodes it will visit next in a queue\"}]}
At most 60 links; each \"why\" is one short sentence of at most 12 words.";

pub fn link_prompt(lines: &str) -> (String, String) {
    (
        LINK_RULES.to_string(),
        format!("<concepts>\n{lines}</concepts>\n\nList the connections between these concepts, as JSON."),
    )
}

pub fn parse_links(reply: &str, known: &HashMap<String, String>) -> Option<Vec<Link>> {
    let value = json_in(reply)?;
    let links = value.get("links")?.as_array()?;
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for link in links {
        let side = |name: &str| {
            link.get(name)
                .and_then(Value::as_str)
                .and_then(concept_label)
                .and_then(|label| known.get(&key(&label)).cloned())
        };
        let (Some(a), Some(b)) = (side("a"), side("b")) else {
            continue;
        };
        let (ka, kb) = (key(&a), key(&b));
        if ka == kb {
            continue;
        }
        let pair = if ka < kb { (ka, kb) } else { (kb, ka) };
        if !seen.insert(pair) {
            continue;
        }
        let why = link
            .get("why")
            .and_then(Value::as_str)
            .map(|w| w.split_whitespace().collect::<Vec<_>>().join(" "))
            .unwrap_or_default();
        out.push(Link {
            a,
            b,
            why: why.chars().take(WHY_CHARS).collect(),
        });
        if out.len() >= MOST_LINKS {
            break;
        }
    }
    Some(out)
}

fn adds_something(table: &BTreeMap<String, Concept>, a: &str, b: &str) -> bool {
    match (table.get(&key(a)), table.get(&key(b))) {
        (Some(a), Some(b)) => a.notes != b.notes,
        _ => false,
    }
}

fn link_lines(table: &BTreeMap<String, Concept>) -> (String, HashMap<String, String>) {
    let mut lines = String::new();
    let mut known = HashMap::new();
    let mut chosen = by_use(table, LINK_CONCEPTS);
    chosen.sort_by(|a, b| a.0.cmp(b.0));
    for (k, concept) in chosen {
        let folders: Vec<&str> = concept.folders.iter().map(String::as_str).collect();
        lines.push_str(&format!("- {} [{}]\n", concept.label, folders.join(", ")));
        known.insert(k.clone(), concept.label.clone());
    }
    (lines, known)
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
    if stale.is_empty() && cache.built_at.is_some() {
        return 0;
    }
    plan(&stale).len() + 1
}

pub fn build(
    sources: &[Source],
    cache: &mut Cache,
    write: &dyn Fn(&str, &str, u32) -> Result<String>,
    progress: &mut dyn FnMut(usize, usize),
) -> Vec<String> {
    let present: BTreeSet<&str> = sources.iter().map(|s| s.id.as_str()).collect();
    cache.notes.retain(|id, _| present.contains(id.as_str()));
    let stale = stale(sources, cache);
    let batches = plan(&stale);
    let total = batches.len() + 1;
    let mut problems = Vec::new();
    let mut worked = batches.is_empty();
    progress(0, total);

    for (done, batch) in batches.iter().enumerate() {
        let notes: Vec<&Source> = batch.iter().map(|&i| stale[i]).collect();
        let table = concepts(sources, cache);
        let vocabulary: Vec<String> = by_use(&table, VOCABULARY)
            .into_iter()
            .map(|(_, c)| c.label.clone())
            .collect();
        let (system, user) = read_prompt(&notes, &vocabulary);
        let read = write(&system, &user, READ_TOKENS)
            .map_err(|e| e.to_string())
            .and_then(|reply| {
                parse_read(&reply, notes.len())
                    .ok_or_else(|| "the AI did not answer in the expected form".to_string())
            });
        match read {
            Ok(lists) => {
                worked = true;
                for (source, concepts) in notes.iter().zip(lists) {
                    cache.notes.insert(
                        source.id.clone(),
                        Read {
                            hash: hash_of(source),
                            concepts,
                        },
                    );
                }
            }
            Err(e) => problems.push(e),
        }
        progress(done + 1, total);
    }

    let table = concepts(sources, cache);
    if table.is_empty() {
        cache.links = Links::default();
    } else {
        let (lines, known) = link_lines(&table);
        let wanted = fnv(&lines);
        if cache.links.key != wanted {
            let (system, user) = link_prompt(&lines);
            let linked = write(&system, &user, LINK_TOKENS)
                .map_err(|e| e.to_string())
                .and_then(|reply| {
                    parse_links(&reply, &known)
                        .ok_or_else(|| "the AI did not answer in the expected form".to_string())
                });
            match linked {
                Ok(mut items) => {
                    items.retain(|l| adds_something(&table, &l.a, &l.b));
                    cache.links = Links { key: wanted, items };
                }
                Err(e) => problems.push(e),
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
    pub count: usize,
    pub concepts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Edge {
    pub a: String,
    pub b: String,
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
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

pub fn assemble(sources: &[Source], cache: &Cache) -> Graph {
    let table = concepts(sources, cache);
    let linked: Vec<(String, String, String)> = cache
        .links
        .items
        .iter()
        .map(|l| (key(&l.a), key(&l.b), l.why.clone()))
        .filter(|(a, b, _)| a != b && adds_something(&table, a, b))
        .collect();
    let in_links: BTreeSet<&String> = linked.iter().flat_map(|(a, b, _)| [a, b]).collect();
    let mut shown: BTreeSet<String> = table
        .iter()
        .filter(|(k, c)| c.notes.len() >= 2 || in_links.contains(k))
        .map(|(k, _)| k.clone())
        .collect();
    if shown.is_empty() {
        shown = table.keys().cloned().collect();
    }

    let mut graph = Graph::default();
    let mut titles: HashMap<String, String> = HashMap::new();
    for source in sources {
        titles
            .entry(source.title.to_lowercase())
            .or_insert_with(|| source.id.clone());
    }
    for source in sources {
        let concepts = cache
            .notes
            .get(&source.id)
            .map(|r| r.concepts.clone())
            .unwrap_or_default();
        let count = concepts.iter().filter(|c| shown.contains(&key(c))).count();
        graph.nodes.push(Node {
            id: format!("n:{}", source.id),
            kind: "note",
            label: if source.title.trim().is_empty() {
                "Untitled".to_string()
            } else {
                source.title.clone()
            },
            folder: source.directory.clone(),
            count,
            concepts,
        });
    }
    for k in &shown {
        let concept = &table[k];
        graph.nodes.push(Node {
            id: format!("c:{k}"),
            kind: "concept",
            label: concept.label.clone(),
            folder: String::new(),
            count: concept.notes.len(),
            concepts: Vec::new(),
        });
        for note in &concept.notes {
            graph.edges.push(Edge {
                a: format!("n:{note}"),
                b: format!("c:{k}"),
                kind: "covers",
                why: None,
            });
        }
    }
    for (a, b, why) in linked {
        graph.edges.push(Edge {
            a: format!("c:{a}"),
            b: format!("c:{b}"),
            kind: "related",
            why: (!why.is_empty()).then_some(why),
        });
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
            let pair = if source.id < *other {
                (source.id.clone(), other.clone())
            } else {
                (other.clone(), source.id.clone())
            };
            if wired.insert(pair.clone()) {
                graph.edges.push(Edge {
                    a: format!("n:{}", pair.0),
                    b: format!("n:{}", pair.1),
                    kind: "link",
                    why: None,
                });
            }
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

    fn fake<'a>(
        reads: &'a AtomicUsize,
        links: &'a AtomicUsize,
    ) -> impl Fn(&str, &str, u32) -> Result<String> + 'a {
        move |system: &str, user: &str, _| {
            if system.starts_with("You read") {
                reads.fetch_add(1, Ordering::SeqCst);
                let mut notes = Vec::new();
                for (i, part) in user.split("<note id=").skip(1).enumerate() {
                    let concepts = if part.contains("BFS") {
                        r#"["Breadth-First Search", "queue"]"#
                    } else if part.contains("LIFO") {
                        r#"["stack", "depth-first search"]"#
                    } else {
                        r#"["Queue", "round robin"]"#
                    };
                    notes.push(format!(r#"{{"id": "n{}", "concepts": {concepts}}}"#, i + 1));
                }
                Ok(format!(
                    "```json\n{{\"notes\": [{}]}}\n```",
                    notes.join(",")
                ))
            } else {
                if links.fetch_add(1, Ordering::SeqCst) == 0 {
                    assert!(user.contains("- queue [cs130, cs162]"), "{user}");
                }
                Ok(r#"Here you go: {"links": [
                    {"a": "queue", "b": "Round Robin", "why": "round robin takes the next process from a queue"},
                    {"a": "breadth-first search", "b": "depth-first search", "why": "two ways to walk a graph"},
                    {"a": "stack", "b": "depth-first search", "why": "only ever together in Stacks"},
                    {"a": "queue", "b": "made up", "why": "x"},
                    {"a": "queue", "b": "queue", "why": "x"}
                ]}"#
                .to_string())
            }
        }
    }

    #[test]
    fn notes_are_read_once_and_concepts_tie_folders_together() {
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
        assert_eq!(cache.notes["a"].concepts, ["Breadth-First Search", "queue"]);
        assert_eq!(cache.links.items.len(), 2, "{:?}", cache.links.items);

        let graph = assemble(&sources, &cache);
        let has = |a: &str, b: &str, kind: &str| {
            graph
                .edges
                .iter()
                .any(|e| e.kind == kind && ((e.a == a && e.b == b) || (e.a == b && e.b == a)))
        };
        assert!(has("n:a", "c:queue", "covers"));
        assert!(
            has("n:c", "c:queue", "covers"),
            "Queue and queue are one concept"
        );
        assert!(has("c:queue", "c:round robin", "related"));
        assert!(
            has("n:a", "n:b", "link"),
            "the [[Stacks]] link joins the notes"
        );
        assert!(
            graph.nodes.iter().any(|n| n.id == "c:breadth-first search"),
            "linked concepts show"
        );
        assert!(
            !graph.nodes.iter().any(|n| n.id == "c:stack"),
            "a concept in one note with no link stays in its note"
        );
        assert_eq!(
            graph.nodes.iter().find(|n| n.id == "n:b").unwrap().concepts,
            ["stack", "depth-first search"]
        );

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
            "the same concepts need no new links"
        );

        changed.remove(2);
        build(&changed, &mut cache, &write, &mut |_, _| {});
        assert!(
            !cache.notes.contains_key("c"),
            "a deleted note leaves the map"
        );
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
                Ok(r#"{"notes": [{"id": "n1", "concepts": ["queue"]}, {"id": "n2", "concepts": ["stack"]}, {"id": "n3", "concepts": ["queue"]}]}"#.into())
            } else {
                Ok(r#"{"links": [{"a": "queue", "b": "stack", "why": "both hold items waiting their turn"}]}"#.into())
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
            && e.why.as_deref() == Some("both hold items waiting their turn")));

        let without = Arc::new(Graphs::for_notes(&dir.path().join("other/notes"), None));
        without.start(sources.clone());
        assert_eq!(without.status(&sources).state, "failed");
    }
}
