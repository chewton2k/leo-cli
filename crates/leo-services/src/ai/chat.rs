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

/// The instructions and the list that make typed points lead the notes.
/// Empty when nothing was typed, so the prompt is unchanged.
fn points_section(points: &[Jotted], length_secs: u64) -> String {
    if points.is_empty() {
        return String::new();
    }
    let list: String = points
        .iter()
        .map(|p| format!("- ({}) {}\n", clock(p.at_secs), p.text))
        .collect();
    format!(
        "The listener typed these points while recording, each with how far into the \
         {} recording they typed it. They are what the listener found most important:\n\
         {list}\n\
         Rules for the typed points:\n\
         - Open the body with a \"## Key points\" section: every typed point, in bold, in \
         the listener's words, each followed by the detail the transcript gives about it \
         (look at what was said around its time)\n\
         - Keep a typed point even if the transcript never mentions it\n\
         - Everything else from the transcript comes after, as usual\n\n",
        clock(length_secs)
    )
}

/// Typed points with no speech to go with them, as a note body.
pub fn points_as_markdown(points: &[Jotted]) -> String {
    let mut body = "## Key points\n".to_string();
    for p in points {
        body.push_str(&format!("- **{}** ({})\n", p.text, clock(p.at_secs)));
    }
    body
}

pub fn build_structure_prompt(transcript: &str) -> String {
    build_structure_prompt_with(transcript, &[], 0)
}

pub fn build_append_prompt(transcript: &str, existing_body: &str) -> String {
    build_append_prompt_with(transcript, existing_body, &[], 0)
}

/// The structure prompt, led by whatever the listener typed while recording.
pub fn build_structure_prompt_with(transcript: &str, points: &[Jotted], length_secs: u64) -> String {
    let points = points_section(points, length_secs);
    format!(
        "You are a note-taking assistant. Given the following transcript from a lecture or meeting, \
         create well-structured notes in Markdown format.\n\n\
         Rules:\n\
         - The FIRST line must be ONLY a concise title (no # prefix, no formatting, just plain text)\n\
         - Follow it with a blank line, then the structured body\n\
         - Use bullet points (- ) for key points\n\
         - Use checkboxes (- [ ] ) for action items or to-dos mentioned\n\
         - Make tables when grouping like ideas\n\
         - Group related points under ## headings\n\
         - There will sometimes be noise in the transcription so make sure to filter out any extraneous information not related to the main topic \n\
         - Interweave your own notes with the structured output where you deem helpful \n\
         - Don't lose important details and capture notes that are meaningful\n\n\
         {points}Transcript:\n{transcript}"
    )
}

/// The append prompt, led by whatever the listener typed while recording.
pub fn build_append_prompt_with(
    transcript: &str,
    existing_body: &str,
    points: &[Jotted],
    length_secs: u64,
) -> String {
    let points = points_section(points, length_secs);
    format!(
        "You are a note-taking assistant. You are adding to an EXISTING note. \
         Given the existing notes and a new transcript, create well-structured notes \
         for ONLY the new content in Markdown format.\n\n\
         Rules:\n\
         - Do NOT include a title — this will be appended to an existing note\n\
         - Use bullet points (- ) for key points\n\
         - Use checkboxes (- [ ] ) for action items or to-dos mentioned\n\
         - Group related points under ## headings\n\
         - Filter out noise from transcription\n\
         - Keep it concise but don't lose important details\n\
         - Avoid duplicating information already in the existing notes\n\
         - Use the same style and structure as the existing notes\n\n\
         Existing notes:\n{existing_body}\n\n\
         {points}New transcript:\n{transcript}"
    )
}

pub fn build_expand_prompt(
    question: &str,
    local_context: &str,
    note_title: &str,
    full_body: &str,
) -> String {
    format!(
        "You are a note-taking assistant helping expand a specific section of lecture notes.\n\n\
         Topic: {note_title}\n\n\
         Full lecture notes (for background):\n{full_body}\n\n\
         Local context around the question:\n{local_context}\n\n\
         Question to expand on:\n{question}\n\n\
         Rules:\n\
         - Answer concisely in markdown (bullet points, short paragraphs)\n\
         - Tie your answer back to the lecture context where relevant\n\
         - Do not repeat what's already in the notes\n\
         - Return only the expanded content, no preamble"
    )
}

/// The model is asked to put a bare title on line one; everything after the
/// blank line is the body.
pub fn split_title_body(content: &str) -> (String, String) {
    let mut lines = content.lines();
    let title = lines
        .next()
        .unwrap_or("")
        .trim_start_matches('#')
        .trim()
        .to_string();
    let title = if title.is_empty() {
        "Untitled Notes".to_string()
    } else {
        title
    };
    let body = lines.collect::<Vec<_>>().join("\n").trim().to_string();
    (title, body)
}

/// Run one prompt through the configured chat chain.
pub fn complete(
    cfg: &Config,
    store: &dyn SecretStore,
    prompt: String,
    max_tokens: u32,
) -> Result<ChainOutcome<String>> {
    let providers = build_chat_chain(cfg, store);
    let req = ChatRequest {
        prompt,
        temperature: TEMPERATURE,
        max_tokens,
    };
    run_chat_chain(providers, &req)
}

/// The same completion, delivered as it arrives.
pub fn complete_streaming(
    cfg: &Config,
    store: &dyn SecretStore,
    prompt: String,
    max_tokens: u32,
    on_fragment: &mut dyn FnMut(&str),
    on_restart: &mut dyn FnMut(),
) -> Result<ChainOutcome<String>> {
    let providers = build_chat_chain(cfg, store);
    let req = ChatRequest {
        prompt,
        temperature: TEMPERATURE,
        max_tokens,
    };
    crate::ai::chain::run_chat_chain_streaming(providers, &req, on_fragment, on_restart)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structure_prompt_embeds_the_transcript() {
        let p = build_structure_prompt("the mitochondria is the powerhouse");
        assert!(p.contains("the mitochondria is the powerhouse"));
        assert!(p.contains("FIRST line"));
    }

    fn points() -> Vec<Jotted> {
        vec![
            Jotted { at_secs: 134, text: "BFS uses a queue".to_string() },
            Jotted { at_secs: 610, text: "exam: know Dijkstra".to_string() },
        ]
    }

    /// Without typed points the prompt is exactly what it always was.
    #[test]
    fn no_typed_points_leaves_the_prompt_unchanged() {
        assert_eq!(
            build_structure_prompt_with("t", &[], 0),
            build_structure_prompt("t")
        );
        assert_eq!(build_append_prompt_with("t", "e", &[], 0), build_append_prompt("t", "e"));
    }

    /// What the listener typed is what they found important, so the notes are
    /// built around it and it stands out.
    #[test]
    fn typed_points_lead_and_are_emphasized() {
        let p = build_structure_prompt_with("a long lecture", &points(), 900);
        assert!(p.contains("BFS uses a queue"), "{p}");
        assert!(p.contains("exam: know Dijkstra"), "{p}");
        assert!(p.contains("02:14"), "no time for the first point: {p}");
        assert!(p.contains("15:00"), "no recording length: {p}");
        assert!(p.contains("## Key points"), "{p}");
        assert!(p.to_lowercase().contains("bold"), "{p}");
        assert!(p.contains("a long lecture"));
    }

    #[test]
    fn typed_points_reach_an_append_too() {
        let p = build_append_prompt_with("more", "## Existing", &points(), 900);
        assert!(p.contains("BFS uses a queue"), "{p}");
        assert!(p.contains("## Existing"));
    }

    /// With no speech, the typed points are still a note.
    #[test]
    fn points_alone_make_a_note_body() {
        let body = points_as_markdown(&points());
        assert!(body.starts_with("## Key points\n"), "{body}");
        assert!(body.contains("- **BFS uses a queue** (02:14)"), "{body}");
    }

    #[test]
    fn append_prompt_embeds_both_transcript_and_existing_body() {
        let p = build_append_prompt("new stuff", "## Existing\n- old point");
        assert!(p.contains("new stuff"));
        assert!(p.contains("- old point"));
        assert!(p.contains("Do NOT include a title"));
    }

    #[test]
    fn expand_prompt_embeds_all_four_inputs() {
        let p = build_expand_prompt("what is BFS?", "local ctx", "Graphs", "full body here");
        assert!(p.contains("what is BFS?"));
        assert!(p.contains("local ctx"));
        assert!(p.contains("Graphs"));
        assert!(p.contains("full body here"));
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
}
