pub mod chain;
pub mod chat;
pub mod error;
pub mod live;
pub mod provider;
pub mod transcribe;

use std::path::Path;

use anyhow::Result;

use crate::config::Config;

/// Token budget for full note structuring.
/// Room for a long lecture's notes, for a provider with no cap of its own.
pub const STRUCTURE_MAX_TOKENS: u32 = 8192;
/// Token budget for expanding one inline @leo prompt.
const EXPAND_MAX_TOKENS: u32 = 2000;

/// Report each degradation so a silent downgrade is never invisible.
///
/// Goes through `diag` rather than printing: these functions run on the main
/// thread while the TUI owns the screen, and a stray write there does lasting
/// damage — it desynchronizes ratatui's cell diff, after which unchanged cells
/// are never repainted and stale text stays on screen until a full redraw.
fn report(outcome: &chain::ChainOutcome<String>) {
    for f in &outcome.fallbacks {
        leo_core::diag::warn(format!(
            "{} unavailable, using {} ({})",
            f.from, f.to, f.reason
        ));
    }
}

fn context() -> (Config, Box<dyn crate::config::secret::SecretStore>) {
    (Config::load(), crate::config::secret::default_store())
}

/// Transcribe an audio file of any length through the configured chain,
/// returning which provider served it and any degradation along the way.
///
/// Callers that own a terminal print the fallbacks; the TUI turns them into
/// status-line events instead, since an `eprintln!` would smear ink across the
/// alternate screen.
/// Resolve every credential the AI chains might need, discarding the values.
///
/// Called before live transcription starts. Reading a credential is not free:
/// the OS keychain can take a long time to answer — over a hundred seconds when
/// macOS decides to ask permission — and paying that inside the rolling loop
/// stalls it completely, with nothing on screen to say why. Paying it up front
/// costs nothing the user notices, because the recorder is already running and
/// no audio is lost, and the read is cached for the rest of the process.
pub fn warm_credentials() {
    let config = crate::config::Config::load();
    let store = crate::config::secret::default_store();
    let names = config
        .transcribe
        .chain
        .iter()
        .chain(config.chat.chain.iter());
    for name in names {
        if let Some(provider) = config.provider(name) {
            let _ =
                crate::config::secret::resolve(name, provider.key_env.as_deref(), store.as_ref());
        }
    }
}

pub fn transcribe_outcome(audio_path: &Path) -> Result<chain::ChainOutcome<String>> {
    let (cfg, store) = context();
    transcribe::run(&cfg, &store, audio_path)
}

/// Transcribe, reporting chunk progress so a caller can draw a bar.
pub fn transcribe_outcome_with_progress(
    audio_path: &Path,
    progress: &(dyn Fn(usize, usize) + Send + Sync),
) -> Result<chain::ChainOutcome<String>> {
    let (cfg, store) = context();
    transcribe::run_with_progress(&cfg, &store, audio_path, progress)
}

/// One chat completion through the configured chain, without printing.
pub fn chat_outcome(prompt: chat::Prompt, max_tokens: u32) -> Result<chain::ChainOutcome<String>> {
    let (cfg, store) = context();
    chat::complete(&cfg, &store, prompt, max_tokens)
}

/// Expand every `@leo` line, reporting text as it arrives.
///
/// `on_fragment` receives the answer in pieces and `on_restart` says to discard
/// what came before, which happens when the chain falls through to another
/// provider. Returns the rewritten body and how many prompts were expanded, the
/// same as the non-streaming path, so the caller saves the note identically.
pub fn expand_prompts_streaming(
    body: &str,
    title: &str,
    on_fragment: &mut dyn FnMut(&str),
    on_restart: &mut dyn FnMut(),
) -> Result<(String, usize)> {
    let (cfg, store) = context();
    Ok(answer_each(body, |question, local_context| {
        let prompt = chat::build_expand_prompt(question, local_context, title, body);
        chat::complete_streaming(
            &cfg,
            &store,
            prompt,
            EXPAND_MAX_TOKENS,
            on_fragment,
            on_restart,
        )
        .ok()
        .map(|outcome| chat::clean_reply(&outcome.value))
    }))
}

/// Answer every `@leo` line in `body` with `answer`, which gets the question
/// and the five lines around it. An answered question stays in the note as a
/// bold **Q:** line with the answer under it; one that gets no answer is left
/// as written, so nothing is lost and it can be asked again. Returns the new
/// body and how many were answered.
fn answer_each(
    body: &str,
    mut answer: impl FnMut(&str, &str) -> Option<String>,
) -> (String, usize) {
    let lines: Vec<&str> = body.lines().collect();
    let mut result: Vec<String> = Vec::with_capacity(lines.len());
    let mut count = 0;

    for (i, &line) in lines.iter().enumerate() {
        let Some(question) = leo_core::action::is_leo_prompt(line) else {
            result.push(line.to_string());
            continue;
        };
        let before = lines[i.saturating_sub(5)..i].join("\n");
        let after = lines[(i + 1)..(i + 6).min(lines.len())].join("\n");
        let local_context = format!("{before}\n{after}");

        match answer(question, &local_context).filter(|a| !a.trim().is_empty()) {
            Some(text) => {
                result.push(format!("**Q:** {question}\n\n{}", text.trim()));
                count += 1;
            }
            None => result.push(line.to_string()),
        }
    }
    (result.join("\n"), count)
}

/// Transcribe an audio file of any length through the configured chain.
pub fn transcribe(audio_path: &Path) -> Result<String> {
    let outcome = transcribe_outcome(audio_path)?;
    report(&outcome);
    Ok(outcome.value)
}

/// Structure a raw transcript into organized notes. Returns (title, body).
pub fn structure_notes(transcript: &str) -> Result<(String, String)> {
    let (cfg, store) = context();
    let outcome = chat::complete(
        &cfg,
        &store,
        chat::build_structure_prompt(transcript),
        STRUCTURE_MAX_TOKENS,
    )?;
    report(&outcome);
    Ok(chat::split_title_body(&outcome.value))
}

/// Structure a new transcript as an addition to an existing note.
pub fn structure_notes_append(transcript: &str, existing_body: &str) -> Result<String> {
    let (cfg, store) = context();
    let outcome = chat::complete(
        &cfg,
        &store,
        chat::build_append_prompt(transcript, existing_body),
        STRUCTURE_MAX_TOKENS,
    )?;
    report(&outcome);
    Ok(chat::clean_reply(&outcome.value))
}

/// Expand a single `@leo` prompt in place.
pub fn expand_prompt(
    question: &str,
    local_context: &str,
    note_title: &str,
    full_body: &str,
) -> Result<String> {
    let (cfg, store) = context();
    let outcome = chat::complete(
        &cfg,
        &store,
        chat::build_expand_prompt(question, local_context, note_title, full_body),
        EXPAND_MAX_TOKENS,
    )?;
    report(&outcome);
    Ok(chat::clean_reply(&outcome.value))
}

/// The real [`leo_core::action::Ai`], running the provider chains.
pub struct RealAi;

impl leo_core::action::Ai for RealAi {
    fn expand_prompts(&self, body: &str, title: &str) -> Result<(String, usize)> {
        expand_leo_prompts(body, title)
    }
    fn structure(&self, transcript: &str) -> Result<(String, String)> {
        structure_notes(transcript)
    }
    fn structure_append(&self, transcript: &str, existing: &str) -> Result<String> {
        structure_notes_append(transcript, existing)
    }
}

/// Answer every `@leo` line in a note, giving each one the lines around it plus
/// the whole note for background.
pub fn expand_leo_prompts(body: &str, title: &str) -> Result<(String, usize)> {
    Ok(answer_each(body, |question, local_context| {
        expand_prompt(question, local_context, title, body).ok()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The question stays in the note, above its answer, so a later reader can
    /// see what was asked.
    #[test]
    fn an_answered_question_keeps_the_question() {
        let body = "## Graphs\n@leo what is BFS?\n- more notes";
        let (out, count) = answer_each(body, |question, _| {
            assert_eq!(question, "what is BFS?");
            Some("Breadth-first search visits level by level.".to_string())
        });
        assert_eq!(count, 1);
        assert_eq!(
            out,
            "## Graphs\n**Q:** what is BFS?\n\nBreadth-first search visits level by level.\n- more notes"
        );
    }

    /// An unanswered question stays exactly as written, to try again.
    #[test]
    fn an_unanswered_question_is_left_alone() {
        let body = "@leo what is BFS?";
        let (out, count) = answer_each(body, |_, _| None);
        assert_eq!(count, 0);
        assert_eq!(out, body);
    }

    /// Each question gets the five lines around it as local context.
    #[test]
    fn the_context_is_the_lines_around_the_question() {
        let body = "a\nb\nc\nd\ne\nf\n@leo q?\ng\nh";
        answer_each(body, |_, context| {
            assert!(context.contains("b\nc\nd\ne\nf"), "{context}");
            assert!(!context.contains('a'), "{context}");
            assert!(context.contains("g\nh"), "{context}");
            None
        });
    }
}
