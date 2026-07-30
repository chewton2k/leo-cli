//! Editing `config.toml` in place, without throwing away the user's file.
//!
//! The shipped config is mostly comments — which provider is free, how to add
//! your own, where to get a key. Serializing the parsed `Config` back out would
//! silently delete all of it, so edits go through `toml_edit`, which preserves
//! comments, ordering, and formatting and touches only the value being changed.

use anyhow::{Context, Result};
use toml_edit::{Array, DocumentMut, Item, Value};

/// Which chain an edit applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Task {
    Chat,
    Transcribe,
}

impl Task {
    pub fn table(self) -> &'static str {
        match self {
            Task::Chat => "chat",
            Task::Transcribe => "transcribe",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Task::Chat => "chat",
            Task::Transcribe => "transcribe",
        }
    }
}

/// Read the chain as it appears in the file.
pub fn read_chain(doc: &DocumentMut, task: Task) -> Vec<String> {
    doc.get(task.table())
        .and_then(|t| t.get("chain"))
        .and_then(|c| c.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Replace one chain, creating the table if the user deleted it.
pub fn write_chain(doc: &mut DocumentMut, task: Task, chain: &[String]) {
    let mut array = Array::new();
    for name in chain {
        array.push(name.as_str());
    }
    // Keep it on one line, the way the shipped file has it.
    array.set_trailing_comma(false);

    let table = doc
        .entry(task.table())
        .or_insert(Item::Table(toml_edit::Table::new()));
    if let Some(table) = table.as_table_mut() {
        table.insert("chain", Item::Value(Value::Array(array)));
    }
}

/// Move the entry at `index` one step toward the front, returning the new index.
/// Order is priority, so this is how a user promotes a provider.
pub fn move_up(chain: &mut [String], index: usize) -> usize {
    if index == 0 || index >= chain.len() {
        return index;
    }
    chain.swap(index - 1, index);
    index - 1
}

/// Move the entry at `index` one step toward the back.
pub fn move_down(chain: &mut [String], index: usize) -> usize {
    if index + 1 >= chain.len() {
        return index;
    }
    chain.swap(index, index + 1);
    index + 1
}

/// Add `name` to the end of a chain, or do nothing if it is already there.
pub fn add(chain: &mut Vec<String>, name: &str) -> bool {
    if chain.iter().any(|n| n == name) {
        return false;
    }
    chain.push(name.to_string());
    true
}

/// Remove `name` from a chain.
pub fn remove(chain: &mut Vec<String>, name: &str) -> bool {
    let before = chain.len();
    chain.retain(|n| n != name);
    chain.len() != before
}

/// Load the config file for editing, creating it from the defaults first if it
/// does not exist yet — a user who never ran `config edit` still gets a file
/// with all the explanatory comments rather than a bare two lines.
pub fn load_document() -> Result<(std::path::PathBuf, DocumentMut)> {
    let (path, _created) = crate::config::Config::ensure_exists()?;
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("could not read {}", path.display()))?;
    let doc = text
        .parse::<DocumentMut>()
        .with_context(|| format!("{} is not valid TOML", path.display()))?;
    Ok((path, doc))
}

/// Write a document back, atomically enough that a crash mid-write cannot leave
/// a truncated config: the new text lands in a sibling file first, then
/// replaces the original in one rename.
pub fn save_document(path: &std::path::Path, doc: &DocumentMut) -> Result<()> {
    let tmp = path.with_extension("toml.new");
    std::fs::write(&tmp, doc.to_string())
        .with_context(|| format!("could not write {}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("could not replace {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> DocumentMut {
        crate::config::Config::default_toml().parse::<DocumentMut>().unwrap()
    }

    #[test]
    fn the_shipped_config_is_valid_toml_for_editing() {
        let d = doc();
        assert_eq!(read_chain(&d, Task::Chat), vec!["ollama", "openrouter"]);
        assert_eq!(
            read_chain(&d, Task::Transcribe),
            vec!["whisper_cpp", "groq", "hf"]
        );
    }

    /// The whole reason for `toml_edit`: an edit must not cost the user the
    /// comments that explain the file.
    #[test]
    fn rewriting_a_chain_preserves_comments_and_other_providers() {
        let mut d = doc();
        let before = d.to_string();
        assert!(before.contains("# Local, free, private"));

        write_chain(&mut d, Task::Chat, &["groq_chat".to_string()]);
        let after = d.to_string();

        assert!(after.contains("chain = [\"groq_chat\"]"), "{after}");
        // Comments survive.
        assert!(after.contains("# Local, free, private"));
        assert!(after.contains("Keys do NOT belong in this file"));
        // Every provider table survives.
        for provider in ["ollama", "openrouter", "whisper_cpp", "xai", "gemini"] {
            assert!(
                after.contains(&format!("[providers.{provider}]")),
                "lost [providers.{provider}]"
            );
        }
        // And the transcribe chain is untouched.
        assert_eq!(
            read_chain(&d, Task::Transcribe),
            vec!["whisper_cpp", "groq", "hf"]
        );
    }

    #[test]
    fn a_rewritten_chain_reads_back_and_still_parses_as_config() {
        let mut d = doc();
        write_chain(&mut d, Task::Chat, &["openrouter".to_string(), "ollama".to_string()]);
        assert_eq!(read_chain(&d, Task::Chat), vec!["openrouter", "ollama"]);

        let cfg = crate::config::Config::parse(&d.to_string()).unwrap();
        assert_eq!(cfg.chat.chain, vec!["openrouter", "ollama"]);
    }

    #[test]
    fn an_empty_chain_is_writable_so_a_user_can_disable_a_task() {
        let mut d = doc();
        write_chain(&mut d, Task::Chat, &[]);
        assert!(read_chain(&d, Task::Chat).is_empty());
        assert!(crate::config::Config::parse(&d.to_string()).is_ok());
    }

    #[test]
    fn a_missing_table_is_created_rather_than_erroring() {
        let mut d = "# just a comment\n".parse::<DocumentMut>().unwrap();
        assert!(read_chain(&d, Task::Chat).is_empty());
        write_chain(&mut d, Task::Chat, &["ollama".to_string()]);
        assert_eq!(read_chain(&d, Task::Chat), vec!["ollama"]);
        assert!(d.to_string().contains("# just a comment"));
    }

    #[test]
    fn reordering_moves_one_step_and_saturates() {
        let mut chain = vec!["a".to_string(), "b".to_string(), "c".to_string()];

        assert_eq!(move_up(&mut chain, 0), 0, "already first");
        assert_eq!(chain, ["a", "b", "c"]);

        assert_eq!(move_up(&mut chain, 2), 1);
        assert_eq!(chain, ["a", "c", "b"]);

        assert_eq!(move_down(&mut chain, 2), 2, "already last");
        assert_eq!(chain, ["a", "c", "b"]);

        assert_eq!(move_down(&mut chain, 0), 1);
        assert_eq!(chain, ["c", "a", "b"]);
    }

    #[test]
    fn reordering_an_out_of_range_index_is_a_no_op() {
        let mut chain = vec!["a".to_string()];
        assert_eq!(move_up(&mut chain, 9), 9);
        assert_eq!(move_down(&mut chain, 9), 9);
        assert_eq!(chain, ["a"]);

        let mut empty: Vec<String> = Vec::new();
        assert_eq!(move_up(&mut empty, 0), 0);
        assert_eq!(move_down(&mut empty, 0), 0);
    }

    #[test]
    fn adding_appends_once_and_removing_takes_it_out() {
        let mut chain = vec!["ollama".to_string()];

        assert!(add(&mut chain, "openrouter"));
        assert_eq!(chain, ["ollama", "openrouter"]);

        // A provider is a chain member or it is not; adding twice is not an
        // error but must not duplicate it, since order is priority.
        assert!(!add(&mut chain, "openrouter"));
        assert_eq!(chain, ["ollama", "openrouter"]);

        assert!(remove(&mut chain, "ollama"));
        assert_eq!(chain, ["openrouter"]);
        assert!(!remove(&mut chain, "ollama"), "already gone");
    }

    #[test]
    fn saving_replaces_the_file_without_leaving_the_temp_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[chat]\nchain = [\"old\"]\n").unwrap();

        let mut d = std::fs::read_to_string(&path)
            .unwrap()
            .parse::<DocumentMut>()
            .unwrap();
        write_chain(&mut d, Task::Chat, &["new".to_string()]);
        save_document(&path, &d).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"new\""));
        assert!(!text.contains("\"old\""));
        assert!(!path.with_extension("toml.new").exists(), "temp file left behind");
    }
}
