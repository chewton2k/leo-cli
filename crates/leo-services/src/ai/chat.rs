use anyhow::Result;

use crate::ai::chain::{run_chat_chain, ChainOutcome};
use crate::ai::provider::{build_chat_chain, ChatRequest};
use crate::config::secret::SecretStore;
use crate::config::Config;

/// Chat calls run at a low temperature: these are structuring tasks, not
/// creative ones.
const TEMPERATURE: f32 = 0.3;

/// A point the listener typed while recording, and how far in they typed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Jotted {
    pub at_secs: u64,
    pub text: String,
}

/// `m:ss` as `mm:ss`, or `h:mm:ss` past an hour.
pub fn clock(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

/// Typed points with no speech to go with them, as a note body.
pub fn points_as_markdown(points: &[Jotted]) -> String {
    let mut body = "## Key points\n".to_string();
    for p in points {
        body.push_str(&format!("- **{}** ({})\n", p.text, clock(p.at_secs)));
    }
    body
}

/// A request in two parts: standing instructions, sent as the system message,
/// and the material to work on, sent as the user's message. Models follow rules
/// given this way more reliably than rules mixed into the material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub system: String,
    pub user: String,
}

/// How the speech-recognition text is to be read. Shared by the new-note and
/// append prompts so they cannot disagree about it.
const READING_A_TRANSCRIPT: &str = "\
The transcript comes from speech recognition, so expect mistakes:
- Fix words that were clearly misheard, using the subject for context (\"breath first search\" is \"breadth-first search\"). Do not guess beyond that.
- Leave out filler, repetition, and anything unrelated to the topic.

Where the recording is patchy or the speaker was vague, fill the gap with accurate explanation from your own knowledge, so the notes make sense on their own. Add what helps someone understand the topic, not tangents.";

/// Formatting rules both note prompts share.
const FORMATTING: &str = "\
- Bold a term where it is defined. Put formulas and code in code blocks.
- Use a table only to compare two or more things across the same attributes.
- Put tasks in a final \"## Action items\" section as checkboxes (- [ ] ), and only if the speaker assigned or mentioned some; otherwise leave the section out.";

/// What to do with points the listener typed while recording.
fn points_rule(opening: &str) -> String {
    format!(
        "The listener typed points while recording; they mark what mattered most to them. \
{opening} a \"## Key points\" section: each typed point in bold, in the listener's words, \
followed by what the transcript says about it. The [~mm:ss] markers in the transcript show \
roughly how far into the recording each part was said; use the time beside a typed point to \
find the part it refers to. Keep every typed point, even one the transcript never mentions."
    )
}

/// The typed points as the user message lists them.
fn points_block(points: &[Jotted]) -> String {
    if points.is_empty() {
        return String::new();
    }
    let list: String = points
        .iter()
        .map(|p| format!("- ({}) {}\n", clock(p.at_secs), p.text))
        .collect();
    format!("<typed_points>\n{list}</typed_points>\n\n")
}

/// Mark roughly where each minute falls in a transcript that has no timestamps,
/// by spreading the recording's length evenly over its words. Speech is not
/// that even, but it is close enough to find the stretch a typed point belongs
/// to, which is all the markers are for.
pub fn with_time_markers(transcript: &str, length_secs: u64) -> String {
    const EVERY_SECS: u64 = 60;
    let words: Vec<&str> = transcript.split_whitespace().collect();
    if length_secs == 0 || words.is_empty() {
        return transcript.to_string();
    }
    let mut out = String::with_capacity(transcript.len() + words.len() / 4);
    let mut next = EVERY_SECS;
    for (i, word) in words.iter().enumerate() {
        let at = i as u64 * length_secs / words.len() as u64;
        if at >= next {
            out.push_str(&format!("[~{}] ", clock(next)));
            next += EVERY_SECS;
        }
        out.push_str(word);
        out.push(' ');
    }
    out.trim_end().to_string()
}

pub fn build_structure_prompt(transcript: &str) -> Prompt {
    build_structure_prompt_with(transcript, &[], 0)
}

pub fn build_append_prompt(transcript: &str, existing_body: &str) -> Prompt {
    build_append_prompt_with(transcript, existing_body, &[], 0)
}

/// The prompt that turns a recording into a new note. Typed points, when there
/// are any, lead the note and the transcript gets time markers to match them.
pub fn build_structure_prompt_with(
    transcript: &str,
    points: &[Jotted],
    length_secs: u64,
) -> Prompt {
    let key_points = if points.is_empty() {
        String::new()
    } else {
        format!("\n\n{}", points_rule("Right after the summary, add"))
    };
    let system = format!(
        "You turn lecture and meeting transcripts into study notes in Markdown.

{READING_A_TRANSCRIPT}

Shape of the reply:
1. The first line is the title, as plain text: no \"Title:\", no #, no quotes, no bold.
2. A blank line, then a 2-3 sentence summary of the whole recording.
3. ## sections for the topics, in the order they came up, with bullet points (- ).
{FORMATTING}{key_points}

Reply with the note only: no preamble before the title, no remarks after the note, and do not wrap it in a code block."
    );
    let transcript = if points.is_empty() {
        transcript.to_string()
    } else {
        with_time_markers(transcript, length_secs)
    };
    let user = format!(
        "{}<transcript>\n{transcript}\n</transcript>\n\n\
         Write the notes for this transcript: the title alone on the first line, then the \
         summary, then the sections.",
        points_block(points)
    );
    Prompt { system, user }
}

/// The prompt that turns a recording into an addition to an existing note.
pub fn build_append_prompt_with(
    transcript: &str,
    existing_body: &str,
    points: &[Jotted],
    length_secs: u64,
) -> Prompt {
    let key_points = if points.is_empty() {
        String::new()
    } else {
        format!("\n\n{}", points_rule("Start the addition with"))
    };
    let system = format!(
        "You add to an existing set of notes in Markdown, from a new transcript.

{READING_A_TRANSCRIPT}

Rules for the addition:
- Write only the new material. No title, no summary of the existing notes.
- Do not repeat anything the existing notes already cover.
- Match the existing notes' style; start each new topic with a ## heading and use bullet points (- ).
{FORMATTING}{key_points}

Reply with the addition only: no preamble, no remarks after it, and do not wrap it in a code block."
    );
    let transcript = if points.is_empty() {
        transcript.to_string()
    } else {
        with_time_markers(transcript, length_secs)
    };
    let user = format!(
        "<existing_notes>\n{existing_body}\n</existing_notes>\n\n{}<transcript>\n{transcript}\n</transcript>\n\n\
         Write only the new notes to add for this transcript, with no title.",
        points_block(points)
    );
    Prompt { system, user }
}

/// The prompt that answers a question from a set of the user's notes, given as
/// (title, directory, body).
pub fn build_notes_question_prompt(question: &str, notes: &[(&str, &str, &str)]) -> Prompt {
    // Enough of each note to answer from, while a handful still fits.
    const PER_NOTE_CHARS: usize = 2000;
    let system = "\
You answer a question using only the user's own notes, given below.
- Answer directly and concisely in Markdown.
- After each fact, name the note it came from in square brackets, like [Graph traversals].
- If the notes do not cover the question, say so plainly first; you may then add a short general answer, clearly marked as not from their notes.
- Reply with the answer only: no preamble, no remarks after it, and do not wrap it in a code block."
        .to_string();
    let mut user = String::new();
    for (title, dir, body) in notes {
        let body: String = body.chars().take(PER_NOTE_CHARS).collect();
        user.push_str(&format!(
            "<note title=\"{title}\" directory=\"{dir}\">\n{body}\n</note>\n\n"
        ));
    }
    user.push_str(&format!("<question>\n{question}\n</question>"));
    Prompt { system, user }
}

/// The prompt that answers an `@leo` question written inside a note.
pub fn build_expand_prompt(
    question: &str,
    local_context: &str,
    note_title: &str,
    full_body: &str,
) -> Prompt {
    let system = "\
You answer a question the user wrote inside their own notes. Your answer is placed in the note directly under the question.
- Answer directly and concisely in Markdown: short paragraphs or bullets.
- Use the note for context and tie the answer back to it where that helps.
- Do not repeat what the note already says.
- Reply with the answer only: no preamble, do not restate the question, no remarks after it, and do not wrap it in a code block."
        .to_string();
    let user = format!(
        "<note title=\"{note_title}\">\n{full_body}\n</note>\n\n\
         <around_the_question>\n{local_context}\n</around_the_question>\n\n\
         <question>\n{question}\n</question>"
    );
    Prompt { system, user }
}

/// Strip what models wrap around an answer despite being asked not to: a code
/// block around the whole reply, a preamble line ("Here are your notes:"), and
/// a sign-off ("Let me know if…").
pub fn clean_reply(reply: &str) -> String {
    let mut text = reply.trim();

    // A fence around the whole reply, not one inside it.
    if let Some(rest) = text.strip_prefix("```") {
        if let Some(inner) = rest.strip_suffix("```") {
            if !inner.contains("```") {
                text = inner.split_once('\n').map_or("", |(_, body)| body).trim();
            }
        }
    }

    let mut lines: Vec<&str> = text.lines().collect();
    while lines
        .first()
        .is_some_and(|l| l.trim().is_empty() || is_preamble(l))
    {
        lines.remove(0);
    }
    while lines
        .last()
        .is_some_and(|l| l.trim().is_empty() || is_sign_off(l))
    {
        lines.pop();
    }
    lines.join("\n").trim().to_string()
}

/// A chatty opener, not a title: it starts like one and ends like a sentence,
/// so "Sure-fire study tricks" stays a title.
fn is_preamble(line: &str) -> bool {
    let l = line.trim().to_lowercase();
    if !l.ends_with([':', '.', '!']) {
        return false;
    }
    const OPENERS: &[&str] = &[
        "here are",
        "here is",
        "here's",
        "sure",
        "certainly",
        "of course",
        "below are",
        "below is",
    ];
    OPENERS.iter().any(|o| l.starts_with(o))
}

fn is_sign_off(line: &str) -> bool {
    let l = line.trim().to_lowercase();
    const CLOSERS: &[&str] = &[
        "let me know",
        "i hope this",
        "hope this helps",
        "feel free to",
    ];
    CLOSERS.iter().any(|c| l.starts_with(c))
}

/// Split a reply into (title, body): the first line is the title, cleaned of
/// the "Title:", `#`, bold and quotes models add, and the rest is the body.
pub fn split_title_body(content: &str) -> (String, String) {
    let cleaned = clean_reply(content);
    let mut lines = cleaned.lines();
    let mut title = lines.next().unwrap_or("").trim();
    loop {
        let before = title;
        title = title.trim_start_matches('#').trim();
        title = title.trim_matches('*').trim();
        for label in ["Title:", "title:", "TITLE:"] {
            if let Some(rest) = title.strip_prefix(label) {
                title = rest.trim();
            }
        }
        title = title
            .trim_matches(|c| c == '"' || c == '\'' || c == '“' || c == '”')
            .trim();
        if title == before {
            break;
        }
    }
    let title = if title.is_empty() {
        "Untitled Notes".to_string()
    } else {
        title.to_string()
    };
    let body = lines.collect::<Vec<_>>().join("\n").trim().to_string();
    (title, body)
}

/// Run one prompt through the configured chat chain.
pub fn complete(
    cfg: &Config,
    store: &dyn SecretStore,
    prompt: Prompt,
    max_tokens: u32,
) -> Result<ChainOutcome<String>> {
    let providers = build_chat_chain(cfg, store);
    let req = ChatRequest {
        system: Some(prompt.system),
        prompt: prompt.user,
        temperature: TEMPERATURE,
        max_tokens,
    };
    run_chat_chain(providers, &req)
}

/// The same completion, delivered as it arrives.
pub fn complete_streaming(
    cfg: &Config,
    store: &dyn SecretStore,
    prompt: Prompt,
    max_tokens: u32,
    on_fragment: &mut dyn FnMut(&str),
    on_restart: &mut dyn FnMut(),
) -> Result<ChainOutcome<String>> {
    let providers = build_chat_chain(cfg, store);
    let req = ChatRequest {
        system: Some(prompt.system),
        prompt: prompt.user,
        temperature: TEMPERATURE,
        max_tokens,
    };
    crate::ai::chain::run_chat_chain_streaming(providers, &req, on_fragment, on_restart)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn points() -> Vec<Jotted> {
        vec![
            Jotted {
                at_secs: 134,
                text: "BFS uses a queue".to_string(),
            },
            Jotted {
                at_secs: 610,
                text: "exam: know Dijkstra".to_string(),
            },
        ]
    }

    /// With no speech, the typed points are still a note.
    #[test]
    fn points_alone_make_a_note_body() {
        let body = points_as_markdown(&points());
        assert!(body.starts_with("## Key points\n"), "{body}");
        assert!(body.contains("- **BFS uses a queue** (02:14)"), "{body}");
    }

    #[test]
    fn split_title_takes_the_first_line() {
        let (title, body) = split_title_body("Lecture 4: Graphs\n\n- BFS\n- DFS");
        assert_eq!(title, "Lecture 4: Graphs");
        assert_eq!(body, "- BFS\n- DFS");
    }

    #[test]
    fn split_title_strips_a_markdown_heading_marker() {
        let (title, _) = split_title_body("## Lecture 4\n\nbody");
        assert_eq!(title, "Lecture 4");
    }

    #[test]
    fn split_title_handles_a_single_line_response() {
        let (title, body) = split_title_body("Just A Title");
        assert_eq!(title, "Just A Title");
        assert_eq!(body, "");
    }

    #[test]
    fn split_title_falls_back_when_empty() {
        let (title, _) = split_title_body("");
        assert_eq!(title, "Untitled Notes");
    }

    // ── reading the reply ───────────────────────────────────────────────────

    /// The ways free models dress up a title, each of which used to become the
    /// note's title verbatim.
    #[test]
    fn a_dressed_up_title_is_cleaned() {
        for reply in [
            "Lecture 4: Graphs\n\n- BFS",
            "# Lecture 4: Graphs\n\n- BFS",
            "Title: Lecture 4: Graphs\n\n- BFS",
            "**Lecture 4: Graphs**\n\n- BFS",
            "\"Lecture 4: Graphs\"\n\n- BFS",
            "**Title:** Lecture 4: Graphs\n\n- BFS",
        ] {
            let (title, body) = split_title_body(reply);
            assert_eq!(title, "Lecture 4: Graphs", "from {reply:?}");
            assert_eq!(body, "- BFS", "from {reply:?}");
        }
    }

    #[test]
    fn a_preamble_before_the_title_is_skipped() {
        for reply in [
            "Here are your notes:\n\nLecture 4: Graphs\n\n- BFS",
            "Sure! Here are the structured notes.\nLecture 4: Graphs\n\n- BFS",
            "Certainly, here is the note:\n\n# Lecture 4: Graphs\n\n- BFS",
        ] {
            let (title, body) = split_title_body(reply);
            assert_eq!(title, "Lecture 4: Graphs", "from {reply:?}");
            assert_eq!(body, "- BFS", "from {reply:?}");
        }
    }

    #[test]
    fn a_title_that_merely_starts_like_a_preamble_is_kept() {
        assert_eq!(
            split_title_body("Sure-fire study tricks\n\n- sleep").0,
            "Sure-fire study tricks"
        );
    }

    /// A reply wrapped in a code block would otherwise save the fences into
    /// the note.
    #[test]
    fn a_reply_wrapped_in_a_code_block_is_unwrapped() {
        let (title, body) = split_title_body("```markdown\nLecture 4\n\n- BFS\n```");
        assert_eq!(title, "Lecture 4");
        assert_eq!(body, "- BFS");
        assert_eq!(clean_reply("```\n- added point\n```"), "- added point");
    }

    /// A code block inside the note is content, not wrapping.
    #[test]
    fn a_code_block_inside_the_note_is_kept() {
        let reply = "Lecture 4\n\n```python\nprint(1)\n```\n\n- BFS";
        let (_, body) = split_title_body(reply);
        assert!(body.contains("```python\nprint(1)\n```"), "{body}");
    }

    #[test]
    fn a_sign_off_after_the_note_is_dropped() {
        let reply = "Lecture 4\n\n- BFS\n\nLet me know if you want more detail!";
        assert_eq!(split_title_body(reply).1, "- BFS");
        assert_eq!(clean_reply("- BFS\n\nI hope this helps."), "- BFS");
    }

    // ── the structure prompt ────────────────────────────────────────────────

    #[test]
    fn rules_go_in_the_system_message_and_the_transcript_in_the_user_message() {
        let p = build_structure_prompt("the mitochondria is the powerhouse");
        assert!(p.system.contains("first line is the title"), "{}", p.system);
        assert!(!p.system.contains("mitochondria"));
        assert!(
            p.user
                .contains("<transcript>\nthe mitochondria is the powerhouse\n</transcript>"),
            "{}",
            p.user
        );
    }

    /// Long transcripts push early instructions out of a model's attention, so
    /// the essentials are repeated after the material.
    #[test]
    fn the_rules_are_restated_after_the_transcript() {
        let p = build_structure_prompt("some speech");
        let after = &p.user[p.user.find("</transcript>").unwrap()..];
        assert!(after.contains("title"), "{}", p.user);
    }

    #[test]
    fn the_structure_prompt_asks_for_a_clean_useful_shape() {
        let s = build_structure_prompt("x").system;
        for wanted in [
            "misheard",      // fix speech-recognition errors
            "own knowledge", // fill gaps the recording or speaker left
            "summary",       // a short summary under the title
            "order",         // sections follow the lecture
            "Bold",          // defined terms stand out
            "code block",    // formulas and code
            "compare",       // tables only for real comparisons
            "Action items",  // tasks only when there were some
            "no preamble",   // nothing before the title
        ] {
            assert!(
                s.contains(wanted),
                "structure prompt lacks {wanted:?}:\n{s}"
            );
        }
    }

    #[test]
    fn without_typed_points_there_is_no_key_points_section_or_time_markers() {
        let p = build_structure_prompt("some speech");
        assert!(!p.system.contains("Key points"));
        assert!(!p.user.contains("[~"));
        assert!(!p.user.contains("<typed_points>"));
    }

    /// What the listener typed is what they found important, so the notes are
    /// built around it and it stands out; time markers let the model find the
    /// part of the transcript each point was typed during.
    #[test]
    fn typed_points_lead_and_line_up_with_the_transcript() {
        let transcript = vec!["word"; 900].join(" ");
        let p = build_structure_prompt_with(&transcript, &points(), 900);
        assert!(p.system.contains("## Key points"), "{}", p.system);
        assert!(p.system.to_lowercase().contains("bold"));
        assert!(p.user.contains("<typed_points>"), "{}", p.user);
        assert!(p.user.contains("(02:14) BFS uses a queue"));
        assert!(
            p.user.contains("[~02:00]"),
            "no time markers in the transcript"
        );
    }

    #[test]
    fn typed_points_reach_an_append_too() {
        let p = build_append_prompt_with("more", "## Existing", &points(), 900);
        assert!(p.user.contains("BFS uses a queue"), "{}", p.user);
        assert!(
            p.user
                .contains("<existing_notes>\n## Existing\n</existing_notes>"),
            "{}",
            p.user
        );
        assert!(p.system.contains("Key points"));
    }

    #[test]
    fn an_append_writes_only_the_new_material() {
        let s = build_append_prompt("new stuff", "## Existing").system;
        assert!(s.contains("No title"), "{s}");
        assert!(s.contains("Do not repeat"), "{s}");
        assert!(s.contains("misheard"), "{s}");
    }

    // ── time markers ────────────────────────────────────────────────────────

    #[test]
    fn markers_are_spread_through_the_transcript_by_position() {
        let transcript = (0..600)
            .map(|i| format!("w{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        let marked = with_time_markers(&transcript, 600);
        // Ten minutes, one word a second: a marker every minute, at the word
        // spoken then.
        assert!(marked.contains("[~01:00] w60"), "{marked}");
        assert!(marked.contains("[~09:00] w540"), "{marked}");
        assert!(!marked.contains("[~00:00]"));
        assert_eq!(marked.matches("[~").count(), 9);
    }

    #[test]
    fn no_length_means_no_markers() {
        assert_eq!(with_time_markers("a b c", 0), "a b c");
        assert_eq!(with_time_markers("", 600), "");
    }

    // ── a question across the notes ─────────────────────────────────────────

    #[test]
    fn a_question_across_notes_carries_each_note_and_asks_for_sources() {
        let notes = [
            ("Graph traversals", "cs130", "BFS uses a queue."),
            ("Lecture 4", "", "Dijkstra finds shortest paths."),
        ];
        let p = build_notes_question_prompt("what is BFS?", &notes);
        assert!(
            p.user.contains("<question>\nwhat is BFS?\n</question>"),
            "{}",
            p.user
        );
        assert!(
            p.user
                .contains("<note title=\"Graph traversals\" directory=\"cs130\">"),
            "{}",
            p.user
        );
        assert!(p.user.contains("Dijkstra"));
        assert!(p.system.contains("only"), "{}", p.system);
        assert!(
            p.system.contains("[Graph traversals]") || p.system.contains("brackets"),
            "{}",
            p.system
        );
        assert!(p.system.to_lowercase().contains("say so"), "{}", p.system);
    }

    /// A long note is trimmed so a handful of notes fits in one request.
    #[test]
    fn a_long_note_is_trimmed_in_the_question_prompt() {
        let body = "word ".repeat(5000);
        let notes = [("Long", "", body.as_str())];
        let p = build_notes_question_prompt("q", &notes);
        assert!(p.user.len() < 12_000, "{}", p.user.len());
    }

    // ── the @leo prompt ─────────────────────────────────────────────────────

    #[test]
    fn a_question_prompt_carries_the_note_and_is_not_about_lectures_only() {
        let p = build_expand_prompt("what is BFS?", "local ctx", "Groceries", "full body here");
        assert!(!p.system.to_lowercase().contains("lecture"), "{}", p.system);
        assert!(p.system.contains("no preamble"), "{}", p.system);
        for part in ["what is BFS?", "local ctx", "Groceries", "full body here"] {
            assert!(p.user.contains(part), "{part} missing from {}", p.user);
        }
    }
}
