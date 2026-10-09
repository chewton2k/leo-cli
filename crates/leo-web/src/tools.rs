use serde::Serialize;

use leo_core::notes::Note;
use leo_core::store::Store;

use crate::chat::{attribute, clip, connected, studied, SourceRef};
use crate::graph::Cache;
use crate::routes::notes::excerpt;

pub const MOST_STEPS: usize = 6;
pub const MOST_PROPOSALS: usize = 3;
const FOUND: usize = 8;
const OPEN_CHARS: usize = 20_000;
const CONNECTED: usize = 10;
const DECIDE_AFTER: usize = 48;

pub const TOOLS: &str = "\
Tools: you can look through and work on the user's notes yourself. To use a tool, reply with only one line, nothing before or after it:
<tool>{\"name\": \"search_notes\", \"query\": \"breadth-first search\"}</tool>
leo runs it and gives you the result in <tool_result>; then use another tool or answer the user. These are not functions in your own tool system: you use one by writing that line as your reply, and they are always available here, even when your own tools are switched off. The tools:
- search_notes {\"query\"}: notes that match words, abbreviations or ideas on the map, with an excerpt of each.
- open_note {\"note\"}: the whole text of one note. \"note\" is an id like n3, or a title.
- connected_notes {\"note\"}: the notes linked to it on the user's map, with how and why.
- edit_note {\"note\", \"find\", \"replace\", \"why\"}: suggest changing a note: \"find\" is text copied exactly from the note (open it first), \"replace\" is what goes there instead. Leave \"find\" empty to add \"replace\" at the end. The user sees the change and decides.
- create_note {\"title\", \"body\", \"folder\"}: suggest a new note in Markdown; \"folder\" is optional. The user sees it and decides.
When to use them:
- The user asks you to fix, correct, update, add to or rewrite a note, or you find a mistake they asked you to fix: use edit_note, once per change, with \"find\" copied exactly from the note text you were given or opened. This is how you change notes here, so never say you cannot edit or change notes.
- The user asks for a new note: use create_note.
- The question is about notes you were not given, or you need a note's full text or its connections: use search_notes, open_note or connected_notes.
- Otherwise answer straight away without tools.
Use at most 6 per answer. Cite notes you used with their ids, like [n4]. After suggesting a change or a note, tell the user what you suggested and that they can apply it; never claim it is already done.";

pub const REMINDER: &str = "Remember your tools: if the user wants a note fixed, corrected, changed, added to or made, or you need a note you were not given, your whole reply is a single <tool>{...}</tool> line instead of an answer. When they asked you to fix something and you found what is wrong, suggest the fix with edit_note before you answer; do not only explain it. You can open and change notes this way, so do not ask the user to do it. Otherwise reply to the user.";

pub const NUDGE: &str = "The user asked for a change to their notes, but your reply suggested none. If a note should be fixed, changed, added to or made, reply now with only the edit_note or create_note line; the tools are available. If nothing needs changing, reply to the user again.";

const CHANGE_WORDS: [&str; 15] = [
    "fix", "correct", "change", "update", "edit", "rewrite", "add", "append", "insert", "remove",
    "create", "make", "write", "improve", "expand",
];

pub fn wants_change(message: &str) -> bool {
    message
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .any(|word| CHANGE_WORDS.contains(&word))
}

pub const NO_MORE_TOOLS: &str =
    "You have used all the tools you can for this answer. Do not ask for another; answer the user now with what you have.";

const NAMES: [&str; 5] = [
    "search_notes",
    "open_note",
    "connected_notes",
    "edit_note",
    "create_note",
];

#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub name: String,
    pub args: serde_json::Value,
}

impl Call {
    pub fn text(&self, key: &str) -> String {
        self.args
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    }
}

fn strip_fence(text: &str) -> &str {
    let t = text.trim();
    let Some(rest) = t.strip_prefix("```") else {
        return t;
    };
    let rest = rest.split_once('\n').map_or("", |(_, r)| r);
    rest.trim_end().strip_suffix("```").unwrap_or(rest).trim()
}

fn call_from(json: &str) -> Result<Call, String> {
    let value: serde_json::Value = serde_json::from_str(json.trim())
        .map_err(|e| format!("that tool call is not valid JSON ({e})"))?;
    let object = value.as_object().ok_or(
        "a tool call is one JSON object, like {\"name\": \"open_note\", \"note\": \"n2\"}",
    )?;
    let name = object
        .get("name")
        .or_else(|| object.get("tool"))
        .and_then(|n| n.as_str())
        .ok_or("the tool call has no \"name\"")?
        .to_string();
    let mut args = object
        .get("args")
        .or_else(|| object.get("arguments"))
        .or_else(|| object.get("input"))
        .cloned()
        .unwrap_or_else(|| value.clone());
    if let Some(text) = args.as_str() {
        args = serde_json::from_str(text).unwrap_or(serde_json::Value::Null);
    }
    Ok(Call { name, args })
}

pub fn find_call(reply: &str) -> Option<Result<Call, String>> {
    if let Some(start) = reply.find("<tool>") {
        let rest = &reply[start + "<tool>".len()..];
        let inner = rest.find("</tool>").map_or(rest, |end| &rest[..end]);
        return Some(call_from(strip_fence(inner)));
    }
    let bare = strip_fence(reply);
    if bare.starts_with('{') && bare.ends_with('}') {
        if let Ok(call) = call_from(bare) {
            if NAMES.contains(&call.name.as_str()) {
                return Some(Ok(call));
            }
        }
    }
    None
}

pub fn without_calls(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("<tool>") {
        out.push_str(&rest[..start]);
        rest = rest[start..]
            .find("</tool>")
            .map_or("", |end| &rest[start + end + "</tool>".len()..]);
    }
    out.push_str(rest);
    out.trim().to_string()
}

#[derive(Debug, Default)]
pub struct Gate {
    held: String,
    streaming: bool,
    stopped: bool,
    pub shown: bool,
}

impl Gate {
    fn emit(&mut self, text: &str, show: &mut dyn FnMut(&str)) {
        if !text.is_empty() {
            show(text);
            self.shown = true;
        }
    }

    pub fn push(&mut self, piece: &str, show: &mut dyn FnMut(&str)) {
        self.held.push_str(piece);
        if self.stopped {
            return;
        }
        if !self.streaming {
            let start = self.held.trim_start();
            if start.starts_with("<tool>") {
                self.stopped = true;
                return;
            }
            let maybe = start.is_empty()
                || start.starts_with('<')
                || start.starts_with('{')
                || start.starts_with('`');
            let long = start.len() >= DECIDE_AFTER && !start.starts_with('{');
            if maybe && !long {
                return;
            }
            self.streaming = true;
        }
        loop {
            let Some(at) = self.held.find('<') else {
                let all = std::mem::take(&mut self.held);
                self.emit(&all, show);
                return;
            };
            let before: String = self.held.drain(..at).collect();
            self.emit(&before, show);
            if self.held.starts_with("<tool>") {
                self.stopped = true;
                return;
            }
            if "<tool>".starts_with(self.held.as_str()) {
                return;
            }
            self.held.drain(..1);
            self.emit("<", show);
        }
    }

    pub fn finish(&mut self, show: &mut dyn FnMut(&str)) {
        let rest = without_calls(&std::mem::take(&mut self.held));
        if self.stopped && self.streaming {
            return;
        }
        self.emit(&rest, show);
    }

    pub fn reset(&mut self) {
        *self = Gate::default();
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Proposal {
    Edit {
        note: String,
        title: String,
        find: String,
        replace: String,
        why: String,
    },
    Create {
        title: String,
        body: String,
        folder: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Done {
    pub step: String,
    pub result: String,
    pub proposal: Option<Proposal>,
    pub found: Vec<String>,
}

pub struct Desk {
    pub sources: Vec<SourceRef>,
    pub proposals: usize,
    room: usize,
}

impl Desk {
    pub fn new(sources: Vec<SourceRef>, room: usize) -> Desk {
        Desk {
            sources,
            proposals: 0,
            room,
        }
    }

    fn tag(&mut self, note: &Note, why: &str) -> String {
        if let Some(s) = self.sources.iter().find(|s| s.id == note.id) {
            return format!("n{}", s.n);
        }
        let n = self.sources.iter().map(|s| s.n).max().unwrap_or(0) + 1;
        self.sources.push(SourceRef {
            n,
            id: note.id.clone(),
            title: note.title.clone(),
            folder: note.directory.clone(),
            why: why.to_string(),
        });
        format!("n{n}")
    }

    fn resolve<'a>(&self, store: &'a Store, wanted: &str) -> Result<&'a Note, String> {
        let wanted = wanted.trim().trim_start_matches('[').trim_end_matches(']');
        if wanted.is_empty() {
            return Err("say which note, with its id like n3 or its title".into());
        }
        if let Some(n) = wanted
            .strip_prefix('n')
            .and_then(|n| n.parse::<usize>().ok())
        {
            if let Some(source) = self.sources.iter().find(|s| s.n == n) {
                if let Some(note) = store.notes.iter().find(|x| x.id == source.id) {
                    return Ok(note);
                }
            }
        }
        if let Some(note) = store.notes.iter().find(|x| x.id == wanted) {
            return Ok(note);
        }
        let lower = wanted.to_lowercase();
        if let Some(note) = store.notes.iter().find(|x| x.title.to_lowercase() == lower) {
            return Ok(note);
        }
        let near: Vec<&Note> = store
            .notes
            .iter()
            .filter(|x| x.title.to_lowercase().contains(&lower))
            .collect();
        match near.as_slice() {
            [one] => Ok(one),
            [] => Err(format!(
                "no note is called \"{wanted}\"; search for it first"
            )),
            many => Err(format!(
                "several notes match \"{wanted}\": {}; use one of their exact titles",
                many.iter()
                    .take(5)
                    .map(|n| format!("\"{}\"", n.title))
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }

    pub fn run(&mut self, store: &Store, cache: &Cache, call: &Call) -> Done {
        let fail = |step: &str, why: String| Done {
            step: step.to_string(),
            result: format!("That did not work: {why}."),
            proposal: None,
            found: Vec::new(),
        };
        match call.name.as_str() {
            "search_notes" => {
                let query = call.text("query");
                let step = format!("Searched your notes for “{}”", clip(query.trim(), 60));
                if query.trim().is_empty() {
                    return fail(&step, "the search needs a \"query\"".into());
                }
                let words: Vec<String> =
                    query.split_whitespace().map(|w| w.to_lowercase()).collect();
                let hits = crate::search::search(store, cache, &query);
                if hits.is_empty() {
                    return Done {
                        step,
                        result: format!("No note matches \"{query}\"."),
                        proposal: None,
                        found: Vec::new(),
                    };
                }
                let mut lines = Vec::new();
                let mut found = Vec::new();
                for hit in hits.iter().filter(|h| studied(h.note)).take(FOUND) {
                    found.push(hit.note.title.clone());
                    let tag = self.tag(hit.note, &format!("found by searching for \"{query}\""));
                    let via = match &hit.why {
                        Some(crate::search::Why::Idea(idea)) => {
                            format!(" (through the idea \"{idea}\")")
                        }
                        Some(crate::search::Why::Summary) => {
                            " (through its summary on the map)".into()
                        }
                        None => String::new(),
                    };
                    let folder = if hit.note.directory.is_empty() {
                        "unfiled"
                    } else {
                        &hit.note.directory
                    };
                    let text = excerpt(&hit.note.body, &words).replace('\n', " ");
                    lines.push(format!(
                        "[{tag}] \"{}\" in {folder}{via}: {}",
                        attribute(&hit.note.title),
                        clip(&text, 300)
                    ));
                }
                let more = hits.len().saturating_sub(lines.len());
                if more > 0 {
                    lines.push(format!(
                        "…and {more} more; search with more specific words to narrow it."
                    ));
                }
                Done {
                    step,
                    result: lines.join("\n"),
                    proposal: None,
                    found,
                }
            }
            "open_note" => {
                let wanted = call.text("note");
                let note = match self.resolve(store, &wanted) {
                    Ok(note) => note,
                    Err(why) => return fail(&format!("Looked for “{}”", clip(&wanted, 60)), why),
                };
                let tag = self.tag(note, "opened by Felix");
                let folder = if note.directory.is_empty() {
                    "unfiled"
                } else {
                    &note.directory
                };
                Done {
                    step: format!("Opened “{}”", note.title),
                    result: format!(
                        "<note id=\"{tag}\" title=\"{}\" class=\"{}\">\n{}\n</note>",
                        attribute(&note.title),
                        attribute(folder),
                        clip(&note.body, OPEN_CHARS.min(self.room / 3))
                    ),
                    proposal: None,
                    found: vec![note.title.clone()],
                }
            }
            "connected_notes" => {
                let wanted = call.text("note");
                let note = match self.resolve(store, &wanted) {
                    Ok(note) => note,
                    Err(why) => return fail(&format!("Looked for “{}”", clip(&wanted, 60)), why),
                };
                let step = format!("Followed the map from “{}”", note.title);
                let links: Vec<(&Note, String, String)> = connected(store, cache, &note.id)
                    .into_iter()
                    .filter(|(other, _, _)| studied(other))
                    .take(CONNECTED)
                    .collect();
                if links.is_empty() {
                    return Done {
                        step,
                        result: format!("\"{}\" has no connections on the map yet.", note.title),
                        proposal: None,
                        found: Vec::new(),
                    };
                }
                let from = note.title.clone();
                let found: Vec<String> = links
                    .iter()
                    .map(|(other, _, _)| other.title.clone())
                    .collect();
                let lines: Vec<String> = links
                    .into_iter()
                    .map(|(other, kind, why)| {
                        let tag = self.tag(other, &format!("connected to {from} ({kind})"));
                        let why = if why.is_empty() {
                            String::new()
                        } else {
                            format!(": {why}")
                        };
                        format!("[{tag}] \"{}\" ({kind}){why}", attribute(&other.title))
                    })
                    .collect();
                Done {
                    step,
                    result: lines.join("\n"),
                    proposal: None,
                    found,
                }
            }
            "edit_note" => {
                let wanted = call.text("note");
                let note = match self.resolve(store, &wanted) {
                    Ok(note) => note,
                    Err(why) => return fail(&format!("Looked for “{}”", clip(&wanted, 60)), why),
                };
                let step = format!("Suggested a change to “{}”", note.title);
                if self.proposals >= MOST_PROPOSALS {
                    return fail(
                        &step,
                        format!("at most {MOST_PROPOSALS} suggestions fit in one answer"),
                    );
                }
                let (find, replace) = (call.text("find"), call.text("replace"));
                if find.is_empty() && replace.trim().is_empty() {
                    return fail(&step, "the change is empty".into());
                }
                let times = if find.is_empty() {
                    1
                } else {
                    note.body.matches(find.as_str()).count()
                };
                if times != 1 {
                    return fail(
                        &step,
                        format!("the text in \"find\" appears {times} times in the note, and it has to appear exactly once; open the note and copy a longer piece exactly"),
                    );
                }
                self.proposals += 1;
                self.tag(note, "changed by Felix");
                Done {
                    step,
                    result: "Suggested. The user sees the change and decides whether to apply it; it is not applied yet.".into(),
                    proposal: Some(Proposal::Edit {
                        note: note.id.clone(),
                        title: note.title.clone(),
                        find,
                        replace,
                        why: call.text("why"),
                    }),
                    found: Vec::new(),
                }
            }
            "create_note" => {
                let title = call.text("title").trim().to_string();
                let body = call.text("body");
                let folder = call.text("folder").trim().trim_matches('/').to_string();
                let step = format!("Suggested a new note, “{}”", clip(&title, 60));
                if self.proposals >= MOST_PROPOSALS {
                    return fail(
                        &step,
                        format!("at most {MOST_PROPOSALS} suggestions fit in one answer"),
                    );
                }
                if title.is_empty() {
                    return fail(&step, "a new note needs a \"title\"".into());
                }
                if store.validate_directory(&folder).is_err() {
                    return fail(
                        &step,
                        format!("\"{folder}\" is not a folder name leo can use"),
                    );
                }
                self.proposals += 1;
                Done {
                    step,
                    result: "Suggested. The user sees the new note and decides whether to make it; it is not made yet.".into(),
                    proposal: Some(Proposal::Create { title, body, folder }),
                    found: Vec::new(),
                }
            }
            other => fail(
                &format!("Tried a tool called “{}”", clip(other, 40)),
                format!(
                    "there is no tool called \"{other}\"; the tools are {}",
                    NAMES.join(", ")
                ),
            ),
        }
    }
}

pub fn continued(conversation: &str, call: &Call, done: &Done) -> String {
    let mut asked = call.args.clone();
    if let Some(object) = asked.as_object_mut() {
        object.insert("name".into(), serde_json::Value::String(call.name.clone()));
    }
    format!(
        "{conversation}\n\nFelix used a tool: <tool>{asked}</tool>\n<tool_result name=\"{}\">\n{}\n</tool_result>\n\nContinue: use another tool, or reply to the user.",
        call.name, done.result
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tool_call_is_read_however_a_model_writes_it() {
        let want = Call {
            name: "open_note".into(),
            args: serde_json::json!({"name": "open_note", "note": "n2"}),
        };
        assert_eq!(
            find_call("<tool>{\"name\": \"open_note\", \"note\": \"n2\"}</tool>"),
            Some(Ok(want.clone()))
        );
        assert_eq!(
            find_call("Let me look.\n<tool>{\"name\": \"open_note\", \"note\": \"n2\"}</tool>"),
            Some(Ok(want.clone()))
        );
        assert_eq!(
            find_call("```json\n{\"name\": \"open_note\", \"note\": \"n2\"}\n```"),
            Some(Ok(want.clone()))
        );
        assert_eq!(
            find_call("<tool>\n```json\n{\"name\": \"open_note\", \"note\": \"n2\"}\n```\n</tool>"),
            Some(Ok(want))
        );
        let nested = find_call(
            "<tool>{\"tool\": \"search_notes\", \"args\": {\"query\": \"heaps\"}}</tool>",
        )
        .unwrap()
        .unwrap();
        assert_eq!(nested.name, "search_notes");
        assert_eq!(nested.text("query"), "heaps");
        let stringly = find_call("<tool>{\"name\": \"search_notes\", \"arguments\": \"{\\\"query\\\": \\\"bfs\\\"}\"}</tool>").unwrap().unwrap();
        assert_eq!(stringly.text("query"), "bfs");
        assert!(find_call("<tool>{not json</tool>").unwrap().is_err());
        assert_eq!(find_call("A plain answer about {braces} in code."), None);
        assert_eq!(
            find_call("{\"answer\": 42}"),
            None,
            "JSON that names no tool is an answer"
        );
        assert_eq!(
            without_calls("Before <tool>{}</tool> after"),
            "Before  after"
        );
    }

    #[test]
    fn a_request_to_change_notes_is_recognised() {
        assert!(wants_change("Can you fix my BFS note?"));
        assert!(wants_change("Make me a note comparing BFS and DFS"));
        assert!(wants_change("add an example to my induction note"));
        assert!(!wants_change("What does a min-heap keep at its root?"));
        assert!(!wants_change("prefix sums and suffixes"));
    }

    #[test]
    fn the_gate_streams_answers_at_once_and_holds_back_tool_calls() {
        let mut shown = String::new();
        let mut gate = Gate::default();
        for piece in ["Heaps ", "keep the minimum ", "at the root."] {
            gate.push(piece, &mut |t| shown.push_str(t));
        }
        gate.finish(&mut |t| shown.push_str(t));
        assert_eq!(shown, "Heaps keep the minimum at the root.");

        let mut shown = String::new();
        let mut gate = Gate::default();
        for piece in [
            "<to",
            "ol>{\"name\": \"search_notes\", \"query\": \"a long query that goes past the limit\"}",
            "</tool>",
        ] {
            gate.push(piece, &mut |t| shown.push_str(t));
        }
        assert!(
            shown.is_empty() && !gate.shown,
            "a tool call is never shown"
        );

        let mut shown = String::new();
        let mut gate = Gate::default();
        gate.push(
            "```python\nprint('a code answer that is long enough to decide')\n```",
            &mut |t| shown.push_str(t),
        );
        assert!(gate.shown && shown.starts_with("```python"));

        let mut shown = String::new();
        let mut gate = Gate::default();
        for piece in [
            "Let me check.",
            " a <b>bold</b> word <t",
            "ool>{\"name\": \"open_note\"}</tool>",
        ] {
            gate.push(piece, &mut |t| shown.push_str(t));
        }
        assert_eq!(shown, "Let me check. a <b>bold</b> word ");

        let mut shown = String::new();
        let mut gate = Gate::default();
        for piece in ["Use a min-heap: x <", " y and <to", "day"] {
            gate.push(piece, &mut |t| shown.push_str(t));
        }
        gate.finish(&mut |t| shown.push_str(t));
        assert_eq!(shown, "Use a min-heap: x < y and <today");
    }

    fn store_with(notes: &[(&str, &str, &str)]) -> (tempfile::TempDir, Store, Vec<String>) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::load_from(&dir.path().join("notes")).unwrap();
        let ids = notes
            .iter()
            .map(|(title, body, folder)| {
                let id = store
                    .create_note(*title, "", vec![], folder)
                    .unwrap()
                    .id
                    .clone();
                store.find_note_mut(&id).unwrap().body = body.to_string();
                id
            })
            .collect();
        (dir, store, ids)
    }

    fn call(json: serde_json::Value) -> Call {
        Call {
            name: json["name"].as_str().unwrap().to_string(),
            args: json,
        }
    }

    #[test]
    fn searching_and_opening_give_notes_ids_felix_can_cite() {
        let (_d, store, ids) = store_with(&[
            (
                "Graph traversals",
                "BFS takes the oldest vertex from a queue.",
                "cs130",
            ),
            ("Heaps", "The minimum sits at the root.", "cs130"),
        ]);
        let cache = Cache::default();
        let mut desk = Desk::new(
            vec![SourceRef {
                n: 1,
                id: ids[1].clone(),
                title: "Heaps".into(),
                folder: "cs130".into(),
                why: "open".into(),
            }],
            crate::chat::ROOM,
        );
        let found = desk.run(
            &store,
            &cache,
            &call(serde_json::json!({"name": "search_notes", "query": "queue"})),
        );
        assert_eq!(found.step, "Searched your notes for “queue”");
        assert!(
            found
                .result
                .starts_with("[n2] \"Graph traversals\" in cs130: BFS takes"),
            "{}",
            found.result
        );
        let opened = desk.run(
            &store,
            &cache,
            &call(serde_json::json!({"name": "open_note", "note": "n2"})),
        );
        assert!(opened
            .result
            .contains("<note id=\"n2\" title=\"Graph traversals\""));
        assert_eq!(desk.sources.len(), 2, "a note found twice keeps one id");
        let by_title = desk.run(
            &store,
            &cache,
            &call(serde_json::json!({"name": "open_note", "note": "heaps"})),
        );
        assert!(by_title.result.contains("id=\"n1\""));
        let missing = desk.run(
            &store,
            &cache,
            &call(serde_json::json!({"name": "open_note", "note": "Calculus"})),
        );
        assert!(missing
            .result
            .starts_with("That did not work: no note is called"));
        let unknown = desk.run(
            &store,
            &cache,
            &call(serde_json::json!({"name": "delete_everything"})),
        );
        assert!(unknown.result.contains("the tools are search_notes"));
    }

    #[test]
    fn changes_and_new_notes_are_only_suggested_and_must_fit_the_note() {
        let (_d, store, ids) = store_with(&[(
            "Heaps",
            "The minimum sits at the root.\nThe root is first.",
            "",
        )]);
        let cache = Cache::default();
        let mut desk = Desk::new(vec![], crate::chat::ROOM);
        let edit = desk.run(&store, &cache, &call(serde_json::json!({
            "name": "edit_note", "note": "Heaps", "find": "The minimum sits", "replace": "In a min-heap the minimum sits", "why": "say which heap"
        })));
        assert_eq!(
            edit.proposal,
            Some(Proposal::Edit {
                note: ids[0].clone(),
                title: "Heaps".into(),
                find: "The minimum sits".into(),
                replace: "In a min-heap the minimum sits".into(),
                why: "say which heap".into(),
            })
        );
        assert!(edit.result.contains("not applied yet"));
        assert_eq!(
            store.notes[0].body, "The minimum sits at the root.\nThe root is first.",
            "nothing changes until the user applies it"
        );
        let twice = desk.run(&store, &cache, &call(serde_json::json!({"name": "edit_note", "note": "Heaps", "find": "root", "replace": "top"})));
        assert!(twice.proposal.is_none() && twice.result.contains("appears 2 times"));
        let made = desk.run(&store, &cache, &call(serde_json::json!({"name": "create_note", "title": "Heap sort", "body": "- build a heap", "folder": "cs130"})));
        assert_eq!(
            made.proposal,
            Some(Proposal::Create {
                title: "Heap sort".into(),
                body: "- build a heap".into(),
                folder: "cs130".into()
            })
        );
        let outside = desk.run(&store, &cache, &call(serde_json::json!({"name": "create_note", "title": "x", "body": "", "folder": "../escape"})));
        assert!(outside.proposal.is_none());
        desk.run(
            &store,
            &cache,
            &call(serde_json::json!({"name": "create_note", "title": "Third", "body": ""})),
        );
        let fourth = desk.run(
            &store,
            &cache,
            &call(serde_json::json!({"name": "create_note", "title": "Fourth", "body": ""})),
        );
        assert!(fourth.proposal.is_none() && fourth.result.contains("at most 3"));
        assert_eq!(
            serde_json::to_value(Proposal::Create {
                title: "t".into(),
                body: "b".into(),
                folder: String::new()
            })
            .unwrap()["kind"],
            "create"
        );
    }
}
