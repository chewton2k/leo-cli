use serde::Serialize;

use leo_core::notes::Note;
use leo_core::store::Store;

use crate::chat::{attribute, clip, connected, studied, SourceRef};
use crate::graph::Cache;
use crate::routes::notes::excerpt;

pub const MOST_STEPS: usize = 12;
pub const MOST_PROPOSALS: usize = 8;
const FOUND: usize = 8;
const OPEN_CHARS: usize = 20_000;
const CONNECTED: usize = 10;
const DECIDE_AFTER: usize = 48;

pub struct Param {
    pub name: &'static str,
    pub required: bool,
    pub about: &'static str,
}

#[derive(Clone, Copy)]
pub struct Spec {
    pub name: &'static str,
    pub purpose: &'static str,
    pub params: &'static [Param],
    pub returns: &'static str,
    pub example: &'static str,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Access {
    #[default]
    Ask,
    Auto,
    Read,
}

impl Access {
    pub fn named(name: &str) -> Access {
        match name {
            "auto" => Access::Auto,
            "read" => Access::Read,
            _ => Access::Ask,
        }
    }

    pub fn changes(self) -> bool {
        self != Access::Read
    }
}

pub fn linked(text: &str, sources: &[SourceRef], own: Option<&str>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("[n") {
        let (before, from) = rest.split_at(at);
        let cited = from.find(']').and_then(|end| {
            if from[end + 1..].starts_with('(') || before.ends_with('[') {
                return None;
            }
            let numbers: Option<Vec<usize>> = from[1..end]
                .split(',')
                .map(|part| part.trim().strip_prefix('n')?.parse().ok())
                .collect();
            numbers.map(|numbers| (end, numbers))
        });
        let Some((end, numbers)) = cited else {
            out.push_str(before);
            out.push_str("[n");
            rest = &from[2..];
            continue;
        };
        let mut titles: Vec<String> = Vec::new();
        for n in numbers {
            let Some(source) = sources.iter().find(|s| s.n == n) else {
                continue;
            };
            let title: String = source
                .title
                .chars()
                .filter(|c| !matches!(c, '[' | ']' | '|' | '#'))
                .collect();
            let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
            if Some(source.id.as_str()) != own && !title.is_empty() && !titles.contains(&title) {
                titles.push(title);
            }
        }
        if titles.is_empty() {
            out.push_str(before.trim_end_matches([' ', '\t']));
        } else {
            out.push_str(before);
            out.push_str(
                &titles
                    .iter()
                    .map(|t| format!("[[{t}]]"))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
        }
        rest = &from[end + 1..];
    }
    out.push_str(rest);
    out
}

pub fn changes_notes(name: &str) -> bool {
    matches!(name, "edit_note" | "create_note")
}

const NOTE: Param = Param {
    name: "note",
    required: true,
    about: "which note: an id like n3 from a <note> tag or an earlier result, or the note's exact title",
};

pub const SPECS: [Spec; 10] = [
    Spec {
        name: "search_notes",
        purpose: "Find the user's notes that match words, abbreviations (BFS finds breadth-first search) or ideas in their knowledge graph.",
        params: &[Param {
            name: "query",
            required: true,
            about: "the words to look for, like \"dijkstra priority queue\"",
        }],
        returns: "Up to 8 lines, best first, each: [n7] \"Title\" in folder (through the idea \"...\" when the knowledge graph found it): an excerpt. Then \"...and N more\" when there are more. Or \"No note matches ...\".",
        example: r#"<tool>{"name": "search_notes", "query": "breadth-first search"}</tool>"#,
    },
    Spec {
        name: "open_note",
        purpose: "Read the whole text of one note.",
        params: &[NOTE],
        returns: "<note id=\"n3\" title=\"...\" class=\"folder\">the full Markdown text</note>",
        example: r#"<tool>{"name": "open_note", "note": "n3"}</tool>"#,
    },
    Spec {
        name: "connected_notes",
        purpose: "List the notes linked to a note in the user's knowledge graph, with how and why they connect.",
        params: &[NOTE],
        returns: "Up to 10 lines, each: [n5] \"Title\" (kind of link): why they connect. Or a line saying it has no connections yet.",
        example: r#"<tool>{"name": "connected_notes", "note": "Heaps"}</tool>"#,
    },
    Spec {
        name: "calculate",
        purpose: "Work out numbers exactly instead of estimating them: function values, iterations of a method, tables, statistics, unit conversions. Never write a number you did not calculate here or read in the notes.",
        params: &[Param {
            name: "steps",
            required: true,
            about: "one line per step: define functions like f(x) = x^2 + 4*cos(x), set values like a = 1 + 0.381966*(2 - 1), or write an expression to evaluate like f(a). Values carry over between lines. Has + - * / ^, comparisons, if(condition, then, otherwise), sin cos tan asin acos atan exp ln log log10 sqrt abs min max floor ceil round sum mean, pi and e. Angles in radians",
        }],
        returns: "Each line with its result to 10 decimal places, or why that line did not work.",
        example: r#"<tool>{"name": "calculate", "steps": "f(x) = x^2 + 4*cos(x)\na = 1.381966\nb = 1.618034\nf(a)\nf(b)\nL = if(f(a) > f(b), a, 1)"}</tool>"#,
    },
    Spec {
        name: "ask_user",
        purpose: "Ask the user a short question when you need their answer before going on (what they mean, which note, how deep to go). Your answer stops there and waits for them.",
        params: &[
            Param {
                name: "question",
                required: true,
                about: "the question, in one sentence",
            },
            Param {
                name: "options",
                required: false,
                about: "up to 5 short answers to tap, separated by | ; leave it out for a free answer",
            },
        ],
        returns: "\"Asked.\" Then end your reply in one short sentence; the user's answer comes as their next message. Write math in the question and choices as $...$.",
        example: r#"<tool>{"name": "ask_user", "question": "Should I cover BFS or DFS first?", "options": "BFS | DFS | Both"}</tool>"#,
    },
    Spec {
        name: "quiz",
        purpose: "Give the user a practice question they answer in the chat: multiple choice, fill in the blank, or free response. Use it to check understanding, especially in study.",
        params: &[
            Param {
                name: "kind",
                required: true,
                about: "multiple_choice, fill_blank or free_response",
            },
            Param {
                name: "question",
                required: true,
                about: "the question; for fill_blank write ___ where the missing words go",
            },
            Param {
                name: "options",
                required: false,
                about: "for multiple_choice: 2 to 6 choices separated by |",
            },
            Param {
                name: "answer",
                required: true,
                about: "the correct choice exactly as written in options, the missing words (other accepted answers separated by |), or a model answer for free_response",
            },
            Param {
                name: "explain",
                required: false,
                about: "one or two sentences shown after they answer, saying why",
            },
        ],
        returns: "\"Asked.\" Then end your reply in one short sentence; how the user did comes as their next message. Write math in the question, choices and answer as $...$, like $f(x)=x^2+4\\cos x$.",
        example: r#"<tool>{"name": "quiz", "kind": "multiple_choice", "question": "What does BFS use to pick the next node?", "options": "a stack | a queue | a heap", "answer": "a queue", "explain": "BFS explores the oldest node found first."}</tool>"#,
    },
    Spec {
        name: "look_at_picture",
        purpose: "Look at the pictures in a note (diagrams, photos of slides or boards, charts) when the question needs what they show. Notes already carry a short [Picture: ...] description where one was made.",
        params: &[
            NOTE,
            Param {
                name: "question",
                required: false,
                about: "what to look for in the pictures",
            },
        ],
        returns: "Each picture's description, or \"That did not work: ...\" when the note has no pictures or no AI that can see is set up.",
        example: r#"<tool>{"name": "look_at_picture", "note": "n2", "question": "what does the graph on the slide show?"}</tool>"#,
    },
    Spec {
        name: "read_document",
        purpose: "Read a document the user gave in this chat, part by part. Use it when the question needs a part of the document you were not shown.",
        params: &[
            Param {
                name: "document",
                required: true,
                about: "the document's id like d1, or its file name",
            },
            Param {
                name: "part",
                required: false,
                about: "which part to read, starting at 1; leave it out for part 1",
            },
        ],
        returns: "<document name=\"...\" part=\"2\" of=\"5\">that part's text</document>, or \"That did not work: ...\" when there is no such document or part.",
        example: r#"<tool>{"name": "read_document", "document": "d1", "part": "2"}</tool>"#,
    },
    Spec {
        name: "edit_note",
        purpose: "Suggest a change to a note. Nothing changes until the user presses Apply.",
        params: &[
            NOTE,
            Param {
                name: "find",
                required: false,
                about: "text copied exactly from the note, which must appear in it exactly once; leave it out or empty to add at the end",
            },
            Param {
                name: "replace",
                required: true,
                about: "the Markdown that goes in place of \"find\", or that is added at the end; write it as note text, with no citations like [n2] (to point to another note, write [[Its title]])",
            },
            Param {
                name: "why",
                required: false,
                about: "one sentence for the user on why the change helps",
            },
        ],
        returns: "\"Suggested.\" when the user can now see the change, or \"That did not work: ...\" when \"find\" is not in the note exactly once.",
        example: r#"<tool>{"name": "edit_note", "note": "n1", "find": "uses a stack", "replace": "uses a queue", "why": "BFS takes the oldest vertex first"}</tool>"#,
    },
    Spec {
        name: "create_note",
        purpose: "Suggest a new note. Nothing is made until the user presses Create.",
        params: &[
            Param {
                name: "title",
                required: true,
                about: "the new note's title",
            },
            Param {
                name: "body",
                required: true,
                about: "the note in Markdown, using \\n for new lines, with no citations like [n2] (to point to another note, write [[Its title]])",
            },
            Param {
                name: "folder",
                required: false,
                about: "an existing or new folder like cs130; leave it out for the top level",
            },
        ],
        returns: "\"Suggested.\" when the user can now see the new note.",
        example: r###"<tool>{"name": "create_note", "title": "BFS and DFS", "body": "## BFS\n- uses a queue", "folder": "cs130"}</tool>"###,
    },
];

pub const WEB_SPECS: [Spec; 2] = [
    Spec {
        name: "web_search",
        purpose: "Search the web for outside or current facts that the user's notes do not have.",
        params: &[Param {
            name: "query",
            required: true,
            about: "what to search for, like \"Dijkstra Turing Award year\"",
        }],
        returns: "Up to 6 results, each: [w2] Title, its address, then a short snippet. Or \"No results\".",
        example: r#"<tool>{"name": "web_search", "query": "Dijkstra Turing Award year"}</tool>"#,
    },
    Spec {
        name: "open_page",
        purpose: "Read the text of a page that web_search returned in this answer.",
        params: &[Param {
            name: "page",
            required: true,
            about: "a result id like w2, or that result's exact address; other addresses are refused",
        }],
        returns: "<page address=\"...\">the page's text, shortened when long</page>",
        example: r#"<tool>{"name": "open_page", "page": "w2"}</tool>"#,
    },
];

pub fn spec_of(name: &str) -> Option<&'static Spec> {
    SPECS
        .iter()
        .chain(WEB_SPECS.iter())
        .find(|s| s.name == name)
}

pub fn is_interaction(name: &str) -> bool {
    matches!(name, "ask_user" | "quiz")
}

pub const ASKED: &str = "Asked. It appears as a card just below your reply, and the user answers in it. End your reply here with one short sentence; do not answer it for them and do not call more tools.";

pub const ALREADY_ASKED: &str = "That did not work: you already asked the user something in this reply. End your reply now with one short sentence and wait for their answer.";

pub fn asked_from(call: &Call) -> Result<Asked, String> {
    let question = call.text("question").trim().to_string();
    if question.is_empty() {
        return Err("\"question\" is empty".into());
    }
    Ok(Asked {
        question,
        options: choices(&call.text("options"), 5),
    })
}

pub fn is_web(name: &str) -> bool {
    WEB_SPECS.iter().any(|s| s.name == name)
}

pub fn describe(spec: &Spec) -> String {
    let params: Vec<String> = spec
        .params
        .iter()
        .map(|p| {
            format!(
                "  - {} (text, {}): {}",
                p.name,
                if p.required { "required" } else { "optional" },
                p.about
            )
        })
        .collect();
    format!(
        "### {}\n{}\nParameters:\n{}\nReturns: {}\nExample: {}",
        spec.name,
        spec.purpose,
        params.join("\n"),
        spec.returns,
        spec.example
    )
}

pub fn manual() -> String {
    manual_with(false)
}

pub fn manual_with(web: bool) -> String {
    manual_for(web, Access::Ask)
}

fn as_access(spec: &Spec, access: Access) -> Spec {
    match (access, spec.name) {
        (Access::Auto, "edit_note") => Spec {
            purpose: "Change a note. leo applies it at once, and the user can undo it.",
            returns: "\"Changed.\" when leo applied it, or \"That did not work: ...\" when \"find\" is not in the note exactly once.",
            ..*spec
        },
        (Access::Auto, "create_note") => Spec {
            purpose: "Make a new note. leo makes it at once, and the user can undo it.",
            returns: "\"Made.\" when leo made the note.",
            ..*spec
        },
        _ => *spec,
    }
}

const WHEN_ASK: &str = "When to use them:
- The user asks you to fix, correct, update, add to or rewrite a note, or you find a mistake they asked you to fix: use edit_note, once per change, with \"find\" copied exactly from the note text you were given or opened. This is how you change notes here, so never say you cannot edit or change notes.
- The user asks for a new note: use create_note.
- The question is about notes you were not given, or you need a note's full text or its connections: use search_notes, open_note or connected_notes.
- The question is about a document the user gave and needs a part you were not shown: use read_document.
- The question is about a picture in a note and its description is not enough: use look_at_picture.
- You cannot tell what the user means and a wrong guess would waste their time: use ask_user, once.
- You want to check what the user understands, or they ask to be quizzed: use quiz, one question at a time.
- The answer needs numbers (evaluating a function, iterating a method, a table, any arithmetic beyond the trivial): use calculate, as often as needed, and report only numbers it gave you.
- Otherwise answer straight away without tools.
After suggesting a change or a note, tell the user what you suggested and that they can apply it; never claim it is already done.";

const WHEN_AUTO: &str = "When to use them:
- The user asks you to fix, correct, update, add to or rewrite a note, or you find a mistake they asked you to fix: use edit_note, once per change, with \"find\" copied exactly from the note text you were given or opened. The user chose Auto, so leo applies your changes at once and they can undo them; make only the changes they asked for.
- The user asks for a new note: use create_note.
- The question is about notes you were not given, or you need a note's full text or its connections: use search_notes, open_note or connected_notes.
- The question is about a document the user gave and needs a part you were not shown: use read_document.
- The question is about a picture in a note and its description is not enough: use look_at_picture.
- You cannot tell what the user means and a wrong guess would waste their time: use ask_user, once.
- You want to check what the user understands, or they ask to be quizzed: use quiz, one question at a time.
- The answer needs numbers (evaluating a function, iterating a method, a table, any arithmetic beyond the trivial): use calculate, as often as needed, and report only numbers it gave you.
- Otherwise answer straight away without tools.
After a change or a new note, tell the user plainly what you changed or made.";

const WHEN_READ: &str = "When to use them:
- The question is about notes you were not given, or you need a note's full text or its connections: use search_notes, open_note or connected_notes.
- The question is about a document the user gave and needs a part you were not shown: use read_document.
- The question is about a picture in a note and its description is not enough: use look_at_picture.
- You cannot tell what the user means and a wrong guess would waste their time: use ask_user, once.
- You want to check what the user understands, or they ask to be quizzed: use quiz, one question at a time.
- The answer needs numbers (evaluating a function, iterating a method, a table, any arithmetic beyond the trivial): use calculate, as often as needed, and report only numbers it gave you.
- Otherwise answer straight away without tools.
The user chose Read only for this chat, so you cannot change or make notes. When they ask for a change, say exactly what you would change and where, and tell them they can switch Felix to Ask or Auto, beside the message box, to let you make it.";

pub fn manual_for(web: bool, access: Access) -> String {
    let tools: Vec<String> = SPECS
        .iter()
        .filter(|spec| access.changes() || !changes_notes(spec.name))
        .chain(WEB_SPECS.iter().filter(|_| web))
        .map(|spec| describe(&as_access(spec, access)))
        .collect();
    let limits = if access.changes() {
        format!("At most {MOST_STEPS} calls per answer, and at most {MOST_PROPOSALS} changes (edit_note or create_note).")
    } else {
        format!("At most {MOST_STEPS} calls per answer.")
    };
    let when = match access {
        Access::Ask => WHEN_ASK,
        Access::Auto => WHEN_AUTO,
        Access::Read => WHEN_READ,
    };
    let outside = if web {
        "For outside or current facts, use web_search and open_page below, and say which parts came from the web and from where. For anything in the user's notes, use the note tools."
    } else {
        "Your own web search, if you have it, is fine for outside or current facts: use it, and say which parts came from the web. For anything in the user's notes, use the tools below."
    };
    format!(
        "## Your tools
You have a tool layer for the user's notes. It belongs to leo, not to your own tool system, and it is always available, even when your own tools are switched off.
{outside}

How to call a tool:
- Reply with exactly one line and nothing before or after it: <tool>{{\"name\": \"<tool>\", \"<parameter>\": \"<value>\"}}</tool>
- The part inside the tags is one JSON object: \"name\" is the tool, every other key is a parameter, and every value is a string in double quotes. Write new lines inside values as \\n.
- One tool per reply. leo runs it and sends back <tool_result name=\"<tool>\">the result</tool_result>, then you continue: call another tool or answer the user.
- A result that starts with \"That did not work:\" says what was wrong; correct the call and try again.
- {limits} Notes that tools find get ids like n7; cite them like [n7].

The tools:

{}

{when}",
        tools.join("\n\n"),
        outside = outside
    )
}

fn offered(web: bool, access: Access) -> impl Iterator<Item = Spec> {
    SPECS
        .iter()
        .filter(move |spec| access.changes() || !changes_notes(spec.name))
        .chain(WEB_SPECS.iter().filter(move |_| web))
        .map(move |spec| as_access(spec, access))
}

pub fn native_specs(web: bool, access: Access) -> Vec<crate::chat::ToolSpec> {
    offered(web, access)
        .map(|spec| {
            let properties: serde_json::Map<String, serde_json::Value> = spec
                .params
                .iter()
                .map(|p| {
                    (
                        p.name.to_string(),
                        serde_json::json!({ "type": "string", "description": p.about }),
                    )
                })
                .collect();
            let required: Vec<&str> = spec
                .params
                .iter()
                .filter(|p| p.required)
                .map(|p| p.name)
                .collect();
            crate::chat::ToolSpec {
                name: spec.name.to_string(),
                description: format!("{} Returns: {}", spec.purpose, spec.returns),
                schema: serde_json::json!({
                    "type": "object",
                    "properties": properties,
                    "required": required,
                    "additionalProperties": false,
                }),
            }
        })
        .collect()
}

pub fn guidance_for(web: bool, access: Access) -> String {
    let limits = if access.changes() {
        format!("At most {MOST_STEPS} tool calls per answer, and at most {MOST_PROPOSALS} changes (edit_note or create_note).")
    } else {
        format!("At most {MOST_STEPS} tool calls per answer.")
    };
    let when = match access {
        Access::Ask => WHEN_ASK,
        Access::Auto => WHEN_AUTO,
        Access::Read => WHEN_READ,
    };
    let outside = if web {
        "For outside or current facts, use web_search and open_page, and say which parts came from the web and from where."
    } else {
        "Your own web search, if you have it, is fine for outside or current facts: say which parts came from the web."
    };
    format!(
        "## Your tools\nleo gives you tools for the user's notes; call them as tools, several at once when they do not depend on each other. {outside} A result that starts with \"That did not work:\" says what was wrong; correct the call and try again. {limits} Notes that tools find get ids like n7; cite them like [n7].\n\n{when}"
    )
}

pub fn result_message(name: &str, result: &str) -> String {
    format!("<tool_result name=\"{name}\">\n{result}\n</tool_result>\n\nContinue: use another tool, or reply to the user.")
}

pub fn check(call: &Call) -> Result<(), String> {
    let Some(spec) = spec_of(&call.name) else {
        let names: Vec<&str> = SPECS.iter().map(|s| s.name).collect();
        return Err(format!(
            "there is no tool called \"{}\"; the tools are {}",
            call.name,
            names.join(", ")
        ));
    };
    let Some(args) = call.args.as_object() else {
        return Err(format!(
            "the parameters must be a JSON object. How to call it:\n{}",
            describe(spec)
        ));
    };
    for param in spec.params {
        match args.get(param.name) {
            Some(serde_json::Value::String(text))
                if param.required && param.name != "body" && text.trim().is_empty() =>
            {
                return Err(format!(
                    "\"{}\" is empty. How to call it:\n{}",
                    param.name,
                    describe(spec)
                ));
            }
            Some(serde_json::Value::String(_)) => {}
            None | Some(serde_json::Value::Null) if !param.required => {}
            None | Some(serde_json::Value::Null) => {
                return Err(format!(
                    "\"{}\" is missing. How to call it:\n{}",
                    param.name,
                    describe(spec)
                ));
            }
            Some(_) => {
                return Err(format!(
                    "\"{}\" must be text in double quotes. How to call it:\n{}",
                    param.name,
                    describe(spec)
                ));
            }
        }
    }
    Ok(())
}

pub const REMINDER: &str = "Remember your tools: if the user wants a note fixed, corrected, changed, added to or made, or you need a note you were not given, your whole reply is a single <tool>{...}</tool> line instead of an answer. When they asked you to fix something and you found what is wrong, suggest the fix with edit_note before you answer; do not only explain it. You can open and change notes this way, so do not ask the user to do it. Otherwise reply to the user.";

pub const READ_REMINDER: &str = "Remember your tools: if you need a note you were not given, your whole reply is a single <tool>{...}</tool> line instead of an answer. This chat is read only, so do not try to change notes. Otherwise reply to the user.";

pub fn reminder(access: Access) -> &'static str {
    if access.changes() {
        REMINDER
    } else {
        READ_REMINDER
    }
}

pub const NUDGE: &str = "The user asked for a change to their notes, but your reply suggested none. If a note should be fixed, changed, added to or made, reply now with only the edit_note or create_note line; the tools are available. If nothing needs changing, reply to the user again.";

const CHANGE_WORDS: [&str; 15] = [
    "fix", "correct", "change", "update", "edit", "rewrite", "add", "append", "insert", "remove",
    "create", "make", "write", "improve", "expand",
];

pub const UNSTUCK: &str = "Your tools are available in this chat: you use one by replying with only its <tool>{...}</tool> line, as the manual shows. If a tool would help, reply now with that line; otherwise answer the user without saying the tools are unavailable.";

pub fn claims_no_tools(reply: &str) -> bool {
    let lower = reply.to_lowercase().replace('’', "'");
    let refusal = [
        "can't",
        "cannot",
        "can not",
        "unable to",
        "don't have",
        "do not have",
        "no access",
        "isn't available",
        "is not available",
        "aren't available",
        "are not available",
        "not available",
    ]
    .iter()
    .any(|w| lower.contains(w));
    let about_tools = [
        "tool",
        "web_search",
        "open_page",
        "search_notes",
        "open_note",
        "edit_note",
        "create_note",
        "connected_notes",
        "search the web",
        "browse",
        "edit your note",
        "change your note",
        "access your notes",
        "look up",
        "look it up",
        "the web",
        "internet",
        "online",
    ]
    .iter()
    .any(|w| lower.contains(w));
    refusal && about_tools
}

pub fn wants_change(message: &str) -> bool {
    message
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .any(|word| CHANGE_WORDS.contains(&word))
}

const WIDE_WORDS: [&str; 9] = [
    "all",
    "every",
    "each",
    "whole",
    "entire",
    "across",
    "throughout",
    "everything",
    "reorganize",
];

pub fn wants_plan(message: &str) -> bool {
    let lower = message.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let changes = words.iter().filter(|w| CHANGE_WORDS.contains(w)).count();
    let wide = words.iter().any(|w| WIDE_WORDS.contains(w));
    let steps = message
        .lines()
        .filter(|l| {
            let l = l.trim_start();
            l.starts_with("- ")
                || l.starts_with("* ")
                || l.chars().next().is_some_and(|c| c.is_ascii_digit()) && l.contains(". ")
        })
        .count();
    (changes > 0 && wide) || changes >= 3 || steps >= 3 || words.len() > 120
}

pub const PLAN: &str = "## This is a big request
Before you start, make a short plan for yourself: what you need to read or find, what you will change or write, and in what order. Then work through it step by step with your tools, checking each part against the notes. Finish with a short list of what you did and anything you left for the user to decide.";

pub const NO_MORE_TOOLS: &str =
    "You have used all the tools you can for this answer. Do not ask for another; answer the user now with what you have.";

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

const LATEX_WORDS: [&str; 74] = [
    "frac",
    "dfrac",
    "tfrac",
    "sqrt",
    "sum",
    "prod",
    "int",
    "iint",
    "oint",
    "lim",
    "infty",
    "partial",
    "nabla",
    "cdot",
    "cdots",
    "ldots",
    "dots",
    "times",
    "div",
    "pm",
    "mp",
    "le",
    "leq",
    "ge",
    "geq",
    "ne",
    "neq",
    "approx",
    "equiv",
    "sim",
    "to",
    "rightarrow",
    "Rightarrow",
    "leftarrow",
    "iff",
    "implies",
    "in",
    "notin",
    "subset",
    "cup",
    "cap",
    "forall",
    "exists",
    "neg",
    "alpha",
    "beta",
    "gamma",
    "delta",
    "epsilon",
    "varepsilon",
    "theta",
    "lambda",
    "mu",
    "nu",
    "pi",
    "rho",
    "sigma",
    "tau",
    "phi",
    "varphi",
    "omega",
    "Delta",
    "Sigma",
    "Omega",
    "sin",
    "cos",
    "tan",
    "log",
    "ln",
    "exp",
    "boxed",
    "text",
    "mathbb",
    "mathrm",
];

const LATEX_MORE: [&str; 14] = [
    "left",
    "right",
    "begin",
    "end",
    "quad",
    "qquad",
    "hat",
    "bar",
    "vec",
    "overline",
    "operatorname",
    "max",
    "min",
    "arg",
];

fn latex_kept(json: &str) -> String {
    let chars: Vec<char> = json.chars().collect();
    let mut out = String::with_capacity(json.len() + 16);
    let mut in_string = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if !in_string || c != '\\' {
            if c == '"' {
                in_string = !in_string;
            }
            out.push(c);
            i += 1;
            continue;
        }
        let next = chars.get(i + 1).copied().unwrap_or(' ');
        let word: String = chars[i + 1..]
            .iter()
            .take_while(|c| c.is_ascii_alphabetic())
            .collect();
        let hex = chars
            .get(i + 2..i + 6)
            .is_some_and(|h| h.iter().all(char::is_ascii_hexdigit));
        let command = LATEX_WORDS
            .iter()
            .chain(LATEX_MORE.iter())
            .any(|w| *w == word);
        let escape = matches!(next, '"' | '\\' | '/')
            || (next == 'u' && hex && !command)
            || (matches!(next, 'b' | 'f' | 'n' | 'r' | 't') && !command);
        if escape {
            out.push(c);
            out.push(next);
            i += 2;
        } else {
            out.push_str("\\\\");
            i += 1;
        }
    }
    out
}

fn call_from(json: &str) -> Result<Call, String> {
    let json = latex_kept(json);
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
            if spec_of(&call.name).is_some() {
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
pub struct Asked {
    pub question: String,
    pub options: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Quiz {
    pub kind: String,
    pub question: String,
    pub options: Vec<String>,
    pub answer: String,
    pub explain: String,
}

pub const QUIZ_KINDS: [&str; 3] = ["multiple_choice", "fill_blank", "free_response"];

pub fn choices(text: &str, most: usize) -> Vec<String> {
    text.split('|')
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .take(most)
        .collect()
}

pub fn quiz_from(call: &Call) -> Result<Quiz, String> {
    let kind = call
        .text("kind")
        .trim()
        .to_lowercase()
        .replace([' ', '-'], "_");
    if !QUIZ_KINDS.contains(&kind.as_str()) {
        return Err(format!("\"kind\" must be one of {}", QUIZ_KINDS.join(", ")));
    }
    let question = call.text("question").trim().to_string();
    let answer = call.text("answer").trim().to_string();
    let options = choices(&call.text("options"), 6);
    if kind == "multiple_choice" {
        if options.len() < 2 {
            return Err("a multiple_choice quiz needs 2 to 6 options separated by |".into());
        }
        if !options.iter().any(|o| o.eq_ignore_ascii_case(&answer)) {
            return Err("the answer must be one of the options, written the same way".into());
        }
    }
    if kind == "fill_blank" && !question.contains("___") {
        return Err("a fill_blank question needs ___ where the missing words go".into());
    }
    Ok(Quiz {
        options: if kind == "multiple_choice" {
            options
        } else {
            Vec::new()
        },
        kind,
        question,
        answer,
        explain: call.text("explain").trim().to_string(),
    })
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
    web: Option<crate::Web>,
    pages: Vec<String>,
    access: Access,
    documents: Vec<(String, String)>,
    captions: Option<std::sync::Arc<crate::captions::Captions>>,
    pub asked: usize,
    steer: Option<crate::steer::Steer>,
    meaning: Option<(
        crate::vectors::Meaning,
        std::sync::Arc<crate::vectors::Vectors>,
    )>,
}

const WEB_RESULTS: usize = 6;
const PAGE_CHARS: usize = 12_000;

impl Desk {
    pub fn new(sources: Vec<SourceRef>, room: usize) -> Desk {
        Desk {
            sources,
            proposals: 0,
            room,
            web: None,
            pages: Vec::new(),
            access: Access::Ask,
            documents: Vec::new(),
            captions: None,
            asked: 0,
            steer: None,
            meaning: None,
        }
    }

    pub fn with_meaning(
        mut self,
        meaning: Option<crate::vectors::Meaning>,
        vectors: std::sync::Arc<crate::vectors::Vectors>,
    ) -> Desk {
        self.meaning = meaning.map(|m| (m, vectors));
        self
    }

    pub fn with_steer(mut self, steer: crate::steer::Steer) -> Desk {
        self.steer = Some(steer);
        self
    }

    pub fn take_steering(&self) -> Vec<String> {
        self.steer
            .as_ref()
            .map(crate::steer::Steer::take)
            .unwrap_or_default()
    }

    pub fn with_captions(mut self, captions: std::sync::Arc<crate::captions::Captions>) -> Desk {
        self.captions = Some(captions);
        self
    }

    fn shown(&self, store: &Store, note: &Note) -> String {
        match &self.captions {
            Some(captions) => {
                crate::captions::captioned(&store.notes_dir, &note.directory, &note.body, captions)
            }
            None => note.body.clone(),
        }
    }

    pub fn pictures_for(
        &mut self,
        store: &Store,
        wanted: &str,
    ) -> Result<(String, Vec<(String, std::path::PathBuf)>), String> {
        let note = self.resolve(store, wanted)?;
        self.tag(note, "its pictures looked at by Felix");
        Ok((
            note.title.clone(),
            crate::captions::pictures_of(&store.notes_dir, &note.directory, &note.body),
        ))
    }

    pub fn with_documents(mut self, documents: Vec<(String, String)>) -> Desk {
        self.documents = documents;
        self
    }

    pub fn part_chars(&self) -> usize {
        (self.room / 3).clamp(12_000, 120_000)
    }

    fn read_document(&self, call: &Call) -> Done {
        let wanted = call.text("document").trim().to_string();
        let fail = |why: String| Done {
            step: format!("Looked for the document “{}”", clip(&wanted, 60)),
            result: format!("That did not work: {why}."),
            proposal: None,
            found: Vec::new(),
        };
        let index = wanted
            .strip_prefix('d')
            .and_then(|n| n.parse::<usize>().ok())
            .and_then(|n| n.checked_sub(1))
            .filter(|i| *i < self.documents.len())
            .or_else(|| {
                self.documents
                    .iter()
                    .position(|(name, _)| name.eq_ignore_ascii_case(&wanted))
            });
        let Some(index) = index else {
            let names: Vec<String> = self
                .documents
                .iter()
                .enumerate()
                .map(|(i, (name, _))| format!("d{} {name}", i + 1))
                .collect();
            return fail(if names.is_empty() {
                "no documents were given in this chat".into()
            } else {
                format!(
                    "there is no document called \"{wanted}\"; the documents are {}",
                    names.join(", ")
                )
            });
        };
        let (name, text) = &self.documents[index];
        let chars: Vec<char> = text.chars().collect();
        let size = self.part_chars();
        let parts = chars.len().div_ceil(size).max(1);
        let part = call
            .text("part")
            .trim()
            .parse::<usize>()
            .unwrap_or(1)
            .max(1);
        if part > parts {
            return fail(format!(
                "{name} has {parts} part{}",
                if parts == 1 { "" } else { "s" }
            ));
        }
        let piece: String = chars[(part - 1) * size..(part * size).min(chars.len())]
            .iter()
            .collect();
        Done {
            step: format!("Read part {part} of {parts} of {name}"),
            result: format!(
                "<document name=\"{}\" part=\"{part}\" of=\"{parts}\">\n{piece}\n</document>",
                attribute(name)
            ),
            proposal: None,
            found: Vec::new(),
        }
    }

    pub fn with_access(mut self, access: Access) -> Desk {
        self.access = access;
        self
    }

    pub fn with_web(mut self, web: Option<crate::Web>) -> Desk {
        self.web = web;
        self
    }

    pub fn run_web(&mut self, call: &Call) -> Done {
        let fail = |step: String, why: String| Done {
            step,
            result: format!("That did not work: {why}."),
            proposal: None,
            found: Vec::new(),
        };
        if let Err(problem) = check(call) {
            return fail(format!("Tried {} with a mistake", call.name), problem);
        }
        let Some(web) = self.web.clone() else {
            return fail(
                format!("Tried {}", call.name),
                format!(
                    "{} is not available here; use your own web search if you have one",
                    call.name
                ),
            );
        };
        if call.name == "web_search" {
            let query = call.text("query");
            let step = format!("Searched the web for “{}”", clip(query.trim(), 60));
            let hits = match (web.search)(query.trim()) {
                Ok(hits) => hits,
                Err(e) => return fail(step, format!("the search did not work ({e})")),
            };
            if hits.is_empty() {
                return Done {
                    step,
                    result: format!("No results for \"{query}\"."),
                    proposal: None,
                    found: Vec::new(),
                };
            }
            let mut lines = Vec::new();
            let mut found = Vec::new();
            for hit in hits.into_iter().take(WEB_RESULTS) {
                let n = match self.pages.iter().position(|p| *p == hit.url) {
                    Some(i) => i + 1,
                    None => {
                        self.pages.push(hit.url.clone());
                        self.pages.len()
                    }
                };
                found.push(hit.title.clone());
                lines.push(format!(
                    "[w{n}] {}\n{}\n{}",
                    hit.title,
                    hit.url,
                    clip(&hit.snippet, 300)
                ));
            }
            return Done {
                step,
                result: lines.join("\n\n"),
                proposal: None,
                found,
            };
        }
        let wanted = call.text("page");
        let wanted = wanted.trim().trim_start_matches('[').trim_end_matches(']');
        let url = wanted
            .strip_prefix('w')
            .and_then(|n| n.parse::<usize>().ok())
            .and_then(|n| n.checked_sub(1))
            .and_then(|i| self.pages.get(i).cloned())
            .or_else(|| self.pages.iter().find(|p| p.as_str() == wanted).cloned());
        let Some(url) = url else {
            return fail(
                "Tried to open a page".into(),
                "only pages that web_search returned in this answer can be opened; search first and use a result id like w2".into(),
            );
        };
        let site = url
            .split("//")
            .nth(1)
            .unwrap_or(&url)
            .split('/')
            .next()
            .unwrap_or("")
            .to_string();
        let step = format!("Read a page on {site}");
        match (web.page)(&url) {
            Ok(text) => Done {
                step,
                result: format!(
                    "<page address=\"{}\">\n{}\n</page>",
                    attribute(&url),
                    clip(&text, PAGE_CHARS)
                ),
                proposal: None,
                found: vec![url],
            },
            Err(e) => fail(step, format!("the page could not be read ({e})")),
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
        if !self.access.changes() && changes_notes(&call.name) {
            return Done {
                step: "Wanted to change a note (read only)".into(),
                result: "That did not work: this chat is read only, so notes cannot be changed or made. Tell the user what you would change and that they can switch Felix to Ask or Auto.".into(),
                proposal: None,
                found: Vec::new(),
            };
        }
        let fail = |step: &str, why: String| Done {
            step: step.to_string(),
            result: format!("That did not work: {why}."),
            proposal: None,
            found: Vec::new(),
        };
        if is_web(&call.name) {
            return self.run_web(call);
        }
        if let Err(problem) = check(call) {
            let step = match spec_of(&call.name) {
                Some(spec) => format!("Tried {} with a mistake", spec.name),
                None => format!("Tried a tool called “{}”", clip(&call.name, 40)),
            };
            return Done {
                step,
                result: format!("That did not work: {problem}"),
                proposal: None,
                found: Vec::new(),
            };
        }
        match call.name.as_str() {
            "search_notes" => {
                let query = call.text("query");
                let step = format!("Searched your notes for “{}”", clip(query.trim(), 60));
                let words: Vec<String> =
                    query.split_whitespace().map(|w| w.to_lowercase()).collect();
                let close = self
                    .meaning
                    .as_ref()
                    .map(|(meaning, vectors)| {
                        crate::vectors::close_to(Some(meaning), vectors, &query, FOUND)
                    })
                    .unwrap_or_default();
                let hits = crate::search::with_meaning(
                    store,
                    &query,
                    crate::search::search(store, cache, &query),
                    &close,
                );
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
                            " (through its summary in the knowledge graph)".into()
                        }
                        Some(crate::search::Why::Meaning) => {
                            " (close in meaning, not in words)".into()
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
                        clip(
                            &self.shown(store, note),
                            crate::chat::scaled(OPEN_CHARS, self.room).min(self.room / 3)
                        )
                    ),
                    proposal: None,
                    found: vec![note.title.clone()],
                }
            }
            "read_document" => self.read_document(call),
            "connected_notes" => {
                let wanted = call.text("note");
                let note = match self.resolve(store, &wanted) {
                    Ok(note) => note,
                    Err(why) => return fail(&format!("Looked for “{}”", clip(&wanted, 60)), why),
                };
                let step = format!("Followed the knowledge graph from “{}”", note.title);
                let links: Vec<(&Note, String, String)> = connected(store, cache, &note.id)
                    .into_iter()
                    .filter(|(other, _, _)| studied(other))
                    .take(CONNECTED)
                    .collect();
                if links.is_empty() {
                    return Done {
                        step,
                        result: format!(
                            "\"{}\" has no connections in the knowledge graph yet.",
                            note.title
                        ),
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
                let find = call.text("find");
                let replace = linked(&call.text("replace"), &self.sources, Some(&note.id));
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
                let body = linked(&call.text("body"), &self.sources, None);
                let folder = call.text("folder").trim().trim_matches('/').to_string();
                let step = format!("Suggested a new note, “{}”", clip(&title, 60));
                if self.proposals >= MOST_PROPOSALS {
                    return fail(
                        &step,
                        format!("at most {MOST_PROPOSALS} suggestions fit in one answer"),
                    );
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
                format!("there is no tool called \"{other}\""),
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
    fn latex_in_a_tool_call_keeps_its_backslashes() {
        let call = find_call(r#"<tool>{"name": "quiz", "kind": "fill_blank", "question": "Solve \(f'(x)=0\) where $f(x)=x^2+4\cos x$ and $\frac{1}{2}\theta \neq \nabla g$: ___", "answer": "x \approx 1.8955"}</tool>"#)
            .unwrap()
            .unwrap();
        assert_eq!(
            call.text("question"),
            r"Solve \(f'(x)=0\) where $f(x)=x^2+4\cos x$ and $\frac{1}{2}\theta \neq \nabla g$: ___"
        );
        assert_eq!(call.text("answer"), r"x \approx 1.8955");
        let plain = find_call(r#"<tool>{"name": "search_notes", "query": "line one\nline two\ttab \"quoted\" é a\\b"}</tool>"#)
            .unwrap()
            .unwrap();
        assert_eq!(
            plain.text("query"),
            "line one\nline two\ttab \"quoted\" é a\\b",
            "real escapes still mean what they say"
        );
    }

    #[test]
    fn a_big_request_is_planned_first_and_a_small_one_is_not() {
        assert!(wants_plan("fix every typo in all my cs130 notes"));
        assert!(wants_plan(
            "rewrite the intro, add an example and fix the formula"
        ));
        assert!(wants_plan(
            "Please:\n1. read the paper\n2. list its claims\n3. compare with my notes"
        ));
        assert!(!wants_plan("what is a heap?"));
        assert!(!wants_plan("fix the typo in this note"));
        assert!(
            !wants_plan("explain all of it"),
            "wide but nothing to change"
        );
    }

    #[test]
    fn a_long_document_is_read_part_by_part_by_id_or_name() {
        let text = format!("{}{}", "a".repeat(12_000), "b".repeat(5_000));
        let desk = Desk::new(Vec::new(), 36_000).with_documents(vec![("paper.pdf".into(), text)]);
        assert_eq!(desk.part_chars(), 12_000);
        let read = |args: serde_json::Value| {
            desk.read_document(&Call {
                name: "read_document".into(),
                args,
            })
        };
        let first = read(serde_json::json!({ "document": "d1" }));
        assert!(first
            .result
            .starts_with("<document name=\"paper.pdf\" part=\"1\" of=\"2\">\naaa"));
        assert_eq!(first.step, "Read part 1 of 2 of paper.pdf");
        let second = read(serde_json::json!({ "document": "PAPER.pdf", "part": "2" }));
        assert!(
            second.result.contains("part=\"2\" of=\"2\">\nbbbbb") && !second.result.contains("aaa")
        );
        assert!(read(serde_json::json!({ "document": "d1", "part": "3" }))
            .result
            .contains("paper.pdf has 2 parts"));
        assert!(read(serde_json::json!({ "document": "d9" }))
            .result
            .contains("the documents are d1 paper.pdf"));
        let none = Desk::new(Vec::new(), 36_000);
        assert!(none
            .read_document(&Call {
                name: "read_document".into(),
                args: serde_json::json!({ "document": "d1" })
            })
            .result
            .contains("no documents were given"));
    }

    fn source(n: usize, id: &str, title: &str) -> SourceRef {
        SourceRef {
            n,
            id: id.into(),
            title: title.into(),
            folder: String::new(),
            why: String::new(),
        }
    }

    #[test]
    fn citations_written_into_a_note_become_links_or_go() {
        let sources = [
            source(1, "a", "Neuro-Symbolic Drive"),
            source(2, "b", "Driving [rules] | v2"),
            source(3, "c", "Heaps"),
        ];
        assert_eq!(
            linked("Reduces miss rate. [n2]", &sources, Some("a")),
            "Reduces miss rate. [[Driving rules v2]]"
        );
        assert_eq!(linked("It helps [n1].", &sources, Some("a")), "It helps.");
        assert_eq!(
            linked("Both [n1, n3] and [n3][n9] here", &sources, None),
            "Both [[Neuro-Symbolic Drive]], [[Heaps]] and [[Heaps]] here"
        );
        assert_eq!(linked("Gone [n9] now", &sources, None), "Gone now");
        assert_eq!(
            linked(
                "keep [n1](https://x.y) and [[n1]] and [note] and [n]",
                &sources,
                None
            ),
            "keep [n1](https://x.y) and [[n1]] and [note] and [n]"
        );
    }

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
    fn the_manual_explains_every_tool_its_parameters_results_and_errors() {
        let text = manual();
        for spec in &SPECS {
            assert!(
                text.contains(&format!("### {}", spec.name)),
                "{}",
                spec.name
            );
            assert!(text.contains(spec.example), "{}", spec.name);
            for param in spec.params {
                let kind = if param.required {
                    "required"
                } else {
                    "optional"
                };
                assert!(
                    text.contains(&format!("  - {} (text, {kind}): ", param.name)),
                    "{}.{}",
                    spec.name,
                    param.name
                );
            }
            let example = find_call(spec.example).unwrap().unwrap();
            assert_eq!(example.name, spec.name);
            assert_eq!(
                check(&example),
                Ok(()),
                "the example for {} is itself a valid call",
                spec.name
            );
        }
        for promise in [
            "<tool_result name=",
            "That did not work:",
            "One tool per reply",
            "always available",
            &format!("at most {MOST_STEPS} calls"),
            "web search",
        ] {
            assert!(
                text.to_lowercase().contains(&promise.to_lowercase()),
                "{promise}"
            );
        }
    }

    #[test]
    fn a_call_that_breaks_the_spec_is_answered_with_how_to_call_it() {
        let wrong = |json: serde_json::Value| {
            check(&Call {
                name: json["name"].as_str().unwrap().to_string(),
                args: json,
            })
            .unwrap_err()
        };
        let missing = wrong(serde_json::json!({"name": "open_note"}));
        assert!(
            missing.starts_with("\"note\" is missing. How to call it:\n### open_note"),
            "{missing}"
        );
        let empty = wrong(serde_json::json!({"name": "search_notes", "query": "  "}));
        assert!(empty.starts_with("\"query\" is empty"));
        let number = wrong(serde_json::json!({"name": "open_note", "note": 3}));
        assert!(number.contains("must be text in double quotes"));
        let unknown = wrong(serde_json::json!({"name": "delete_note", "note": "n1"}));
        assert!(unknown.contains(
            "the tools are search_notes, open_note, connected_notes, calculate, ask_user, quiz, look_at_picture, read_document, edit_note, create_note"
        ));
        assert_eq!(
            check(&Call {
                name: "edit_note".into(),
                args: serde_json::json!({"name": "edit_note", "note": "n1", "replace": "x"})
            }),
            Ok(()),
            "optional parameters may be left out"
        );
        assert_eq!(
            check(&Call {
                name: "create_note".into(),
                args: serde_json::json!({"name": "create_note", "title": "t", "body": ""})
            }),
            Ok(()),
            "an empty body is allowed"
        );

        let dir = tempfile::tempdir().unwrap();
        let store = Store::load_from(&dir.path().join("notes")).unwrap();
        let done = Desk::new(vec![], crate::chat::ROOM).run(
            &store,
            &Cache::default(),
            &Call {
                name: "open_note".into(),
                args: serde_json::json!({"name": "open_note"}),
            },
        );
        assert_eq!(done.step, "Tried open_note with a mistake");
        assert!(done
            .result
            .starts_with("That did not work: \"note\" is missing. How to call it:"));
    }

    fn fake_web(fail_search: bool) -> crate::Web {
        crate::Web {
            search: std::sync::Arc::new(move |query: &str| {
                if fail_search {
                    anyhow::bail!("offline")
                }
                Ok(vec![
                    crate::WebHit {
                        title: format!("About {query}"),
                        url: "https://example.org/a".into(),
                        snippet: "First.".into(),
                    },
                    crate::WebHit {
                        title: "Second".into(),
                        url: "https://example.org/b".into(),
                        snippet: "Second.".into(),
                    },
                ])
            }),
            page: std::sync::Arc::new(|address: &str| Ok(format!("Text of {address}"))),
            needed: std::sync::Arc::new(|| true),
        }
    }

    #[test]
    fn web_tools_are_described_only_when_leo_provides_them() {
        let with = manual_with(true);
        let without = manual();
        for spec in &WEB_SPECS {
            assert!(with.contains(&format!("### {}", spec.name)));
            assert!(!without.contains(&format!("### {}", spec.name)));
            assert_eq!(check(&find_call(spec.example).unwrap().unwrap()), Ok(()));
        }
        assert!(with.contains("use web_search and open_page"));
        assert!(without.contains("Your own web search"));
    }

    #[test]
    fn web_pages_can_only_be_opened_from_this_answers_results() {
        let call = |json: serde_json::Value| Call {
            name: json["name"].as_str().unwrap().to_string(),
            args: json,
        };
        let mut desk = Desk::new(vec![], crate::chat::ROOM).with_web(Some(fake_web(false)));
        let found = desk.run_web(&call(
            serde_json::json!({"name": "web_search", "query": "heaps"}),
        ));
        assert_eq!(found.step, "Searched the web for “heaps”");
        assert_eq!(found.found, ["About heaps", "Second"]);
        assert!(
            found
                .result
                .starts_with("[w1] About heaps\nhttps://example.org/a\nFirst."),
            "{}",
            found.result
        );
        let read = desk.run_web(&call(
            serde_json::json!({"name": "open_page", "page": "w2"}),
        ));
        assert_eq!(read.step, "Read a page on example.org");
        assert!(read
            .result
            .contains("<page address=\"https://example.org/b\">\nText of https://example.org/b"));
        let by_address = desk.run_web(&call(
            serde_json::json!({"name": "open_page", "page": "https://example.org/a"}),
        ));
        assert!(by_address.result.contains("Text of https://example.org/a"));
        for wanted in [
            "https://evil.example/steal?notes=1",
            "w9",
            "http://127.0.0.1/",
        ] {
            let refused = desk.run_web(&call(
                serde_json::json!({"name": "open_page", "page": wanted}),
            ));
            assert!(
                refused
                    .result
                    .starts_with("That did not work: only pages that web_search returned"),
                "{wanted}"
            );
        }
        let through_run = desk.run(
            &Store::load_from(&tempfile::tempdir().unwrap().path().join("n")).unwrap(),
            &Cache::default(),
            &call(serde_json::json!({"name": "web_search", "query": "x"})),
        );
        assert_eq!(through_run.step, "Searched the web for “x”");

        let mut offline = Desk::new(vec![], crate::chat::ROOM).with_web(Some(fake_web(true)));
        let failed = offline.run_web(&call(
            serde_json::json!({"name": "web_search", "query": "x"}),
        ));
        assert!(failed
            .result
            .starts_with("That did not work: the search did not work (offline)"));
        let mut none = Desk::new(vec![], crate::chat::ROOM);
        let missing = none.run_web(&call(
            serde_json::json!({"name": "web_search", "query": "x"}),
        ));
        assert!(missing.result.contains("is not available here"));
    }

    #[test]
    fn a_reply_that_claims_the_tools_are_missing_is_recognised() {
        assert!(claims_no_tools(
            "I can’t access the Leo `web_search` or `open_page` tools in this chat."
        ));
        assert!(claims_no_tools(
            "I don't have a way to edit your note here."
        ));
        assert!(claims_no_tools(
            "The note editing tool isn't available in this chat."
        ));
        assert!(claims_no_tools("I can’t look up a current figure here."));
        assert!(claims_no_tools(
            "I don't have internet access, so from memory: about 670,000."
        ));
        assert!(!claims_no_tools("BFS uses a queue, not a stack."));
        assert!(!claims_no_tools("I can't be sure, but the heap is a tree."));
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
        while desk.proposals < MOST_PROPOSALS {
            desk.run(
                &store,
                &cache,
                &call(serde_json::json!({"name": "create_note", "title": "More", "body": ""})),
            );
        }
        let beyond = desk.run(
            &store,
            &cache,
            &call(serde_json::json!({"name": "create_note", "title": "Beyond", "body": ""})),
        );
        assert!(
            beyond.proposal.is_none()
                && beyond.result.contains(&format!("at most {MOST_PROPOSALS}"))
        );
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
