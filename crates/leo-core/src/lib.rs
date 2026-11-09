//! The domain: notes, the store that keeps them on disk, and the retention
//! choices in `keep`. Nothing here draws to a terminal or makes a network
//! request, which is what keeps it testable on its own.

pub const NO_WRITING_AI: &str = "leo can't write yet: ";

pub fn writing_said(text: &str) -> String {
    let line = text.lines().next().unwrap_or("").trim();
    let line = line.strip_prefix(NO_WRITING_AI).unwrap_or(line);
    let line: String = line.chars().take(240).collect();
    match line.trim().trim_end_matches('.') {
        "" => format!("{NO_WRITING_AI}the writing AI didn't answer."),
        line => format!("{NO_WRITING_AI}{line}."),
    }
}

pub mod attachments;
pub mod children;
pub mod diag;
pub mod filename;
pub mod keep;
pub mod notes;
pub mod open;
pub mod paths;
pub mod recording;
pub mod store;
pub mod tasks;
pub mod watch;

pub mod activity;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_writer_failure_is_one_short_sentence_after_the_prefix() {
        let said = writing_said("every chat provider failed. Last error: ollama: down\nmore");
        assert_eq!(
            said,
            "leo can't write yet: every chat provider failed. Last error: ollama: down."
        );
        assert_eq!(writing_said(&said), said);
        assert_eq!(
            writing_said(""),
            "leo can't write yet: the writing AI didn't answer."
        );
    }
}
