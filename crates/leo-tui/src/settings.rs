//! The provider screen's behavior: building its rows from config plus the
//! keychain, and applying the actions the user takes on them.
//!
//! Kept out of `tui/mod.rs` so the row-building and chain-editing logic can be
//! tested against an in-memory secret store, with no terminal and no keychain.

use anyhow::Result;

use crate::view::settings::{Credential, Row, SettingAction};
use leo_services::config::choice::{self, Local};
use leo_services::config::edit::{self, Task};
use leo_services::config::provider::ProviderKind;
use leo_services::config::secret::{redact, SecretStore};
use leo_services::config::Config;

/// Which chain a provider kind can serve. A transcription provider in the chat
/// chain would be silently dropped by the chain builder, so the screen offers
/// the right one instead.
fn task_for(kind: Option<ProviderKind>) -> Task {
    match kind {
        Some(ProviderKind::Openai) => Task::Chat,
        _ => Task::Transcribe,
    }
}

/// Something that can be done to a provider row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderOp {
    Login,
    Add,
    Test,
}

/// What Enter does on a provider row: the one thing it most needs.
pub fn primary_action(credential: &Credential, in_chain: bool) -> ProviderOp {
    match (credential, in_chain) {
        (Credential::Missing, _) => ProviderOp::Login,
        (_, false) => ProviderOp::Add,
        (_, true) => ProviderOp::Test,
    }
}

/// Describe where a provider's credential comes from, without revealing it.
///
/// Asks whether a key exists rather than reading it: this runs for every
/// provider on the screen, and on a real keychain each *read* can cost a
/// permission dialog while an existence check does not.
fn credential_for(name: &str, key_env: Option<&str>, store: &dyn SecretStore) -> Credential {
    let Some(var) = key_env else {
        return Credential::NotNeeded;
    };
    if let Ok(value) = std::env::var(var) {
        if !value.trim().is_empty() {
            return Credential::Env {
                var: var.to_string(),
                redacted: redact(&value),
            };
        }
    }
    if store.has(name) {
        Credential::Stored
    } else {
        Credential::Missing
    }
}

pub fn simple_rows(
    cfg: &Config,
    store: &dyn SecretStore,
    notes_dir: &std::path::Path,
    local: &Local,
) -> Vec<Row> {
    let mut rows = vec![Row::Section("AI".to_string())];
    let writing_key = ai_rows(&mut rows, cfg, store, local, Task::Chat, None);
    ai_rows(
        &mut rows,
        cfg,
        store,
        local,
        Task::Transcribe,
        writing_key.as_deref(),
    );
    rows.extend(appearance_rows(cfg));
    rows.extend(backup_rows(notes_dir, cfg));
    rows.extend(storage_rows(notes_dir));
    rows.push(Row::Section("advanced".to_string()));
    rows.push(Row::Setting {
        label: "AI providers".to_string(),
        value: "every provider, its key, and the order they are tried in".to_string(),
        action: SettingAction::ShowProviders,
    });
    rows
}

fn ai_rows(
    rows: &mut Vec<Row>,
    cfg: &Config,
    store: &dyn SecretStore,
    local: &Local,
    task: Task,
    shown_key: Option<&str>,
) -> Option<String> {
    let what = match task {
        Task::Chat => "writing",
        Task::Transcribe => "speech",
    };
    let Some(sel) = choice::selection(cfg, store, task) else {
        rows.push(Row::Setting {
            label: what.to_string(),
            value: "○ nothing chosen".to_string(),
            action: SettingAction::ChooseProvider(task),
        });
        return None;
    };
    let mark = if sel.ready { "●" } else { "○" };
    let Some(chosen) = sel.choice else {
        rows.push(Row::Setting {
            label: what.to_string(),
            value: format!("{mark} {} (from config.toml)", sel.provider),
            action: SettingAction::ChooseProvider(task),
        });
        return None;
    };
    rows.push(Row::Setting {
        label: what.to_string(),
        value: format!("{mark} {}", chosen.label()),
        action: SettingAction::ChooseProvider(task),
    });
    let model_label = format!("{what} model");
    if chosen.local() {
        local_rows(rows, cfg, local, task, &sel, model_label);
        return None;
    }
    rows.push(Row::Setting {
        label: model_label,
        value: sel.model.clone().unwrap_or_else(|| "(default)".to_string()),
        action: SettingAction::ChooseModel(task),
    });
    let pc = cfg.provider(&sel.provider)?;
    let account = pc.account(&sel.provider).to_string();
    if shown_key == Some(account.as_str()) {
        return Some(account);
    }
    let value = match credential_for(&account, pc.key_env.as_deref(), store) {
        Credential::Stored => "stored".to_string(),
        Credential::Env { var, .. } => format!("from ${var}"),
        Credential::NotNeeded => "not needed".to_string(),
        Credential::Missing => "none — Enter to add one".to_string(),
    };
    rows.push(Row::Setting {
        label: format!("{} key", chosen.name),
        value,
        action: SettingAction::StoreKey {
            name: account.clone(),
        },
    });
    Some(account)
}

fn local_rows(
    rows: &mut Vec<Row>,
    cfg: &Config,
    local: &Local,
    task: Task,
    sel: &choice::Selection,
    model_label: String,
) {
    match task {
        Task::Chat => {
            if !local.ollama_running {
                let value = if leo_services::health::on_path("ollama") {
                    "Ollama is not running: open the Ollama app"
                } else {
                    "Ollama is not installed: brew install ollama"
                };
                rows.push(Row::Fact {
                    label: model_label,
                    value: value.to_string(),
                });
                return;
            }
            if local.ollama.is_empty() {
                rows.push(Row::Setting {
                    label: model_label,
                    value: format!("none yet — Enter downloads {}", choice::OLLAMA_STARTER),
                    action: SettingAction::GetLocalModel(task),
                });
                return;
            }
            let current = sel.model.clone().unwrap_or_default();
            let value = if local.ollama.contains(&current) {
                current
            } else {
                format!("{current} is not downloaded — Enter picks one you have")
            };
            rows.push(Row::Setting {
                label: model_label,
                value,
                action: SettingAction::ChooseModel(task),
            });
        }
        Task::Transcribe => {
            let bin = cfg
                .provider(&sel.provider)
                .and_then(|p| p.bin.clone())
                .unwrap_or_else(|| "whisper-cli".to_string());
            if !leo_services::health::on_path(&bin) {
                rows.push(Row::Fact {
                    label: "whisper.cpp".to_string(),
                    value: "not installed: brew install whisper-cpp".to_string(),
                });
            }
            if local.whisper.is_empty() {
                rows.push(Row::Setting {
                    label: model_label,
                    value: "none yet — Enter downloads base.en (142 MB)".to_string(),
                    action: SettingAction::GetLocalModel(task),
                });
                return;
            }
            let current = sel.model.clone().unwrap_or_default();
            let installed = local
                .whisper
                .iter()
                .any(|p| *p == leo_services::ai::provider::whisper_cpp::expand_tilde(&current));
            let value = if installed {
                choice::whisper_label(&current)
            } else {
                format!(
                    "{} is missing — Enter picks one you have",
                    choice::whisper_label(&current)
                )
            };
            rows.push(Row::Setting {
                label: model_label,
                value,
                action: SettingAction::ChooseModel(task),
            });
        }
    }
}

/// Build the screen: both chains in order, then everything else that is
/// configured.
pub fn rows(cfg: &Config, store: &dyn SecretStore, notes_dir: &std::path::Path) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut in_a_chain: Vec<&String> = Vec::new();

    for task in [Task::Chat, Task::Transcribe] {
        let chain = match task {
            Task::Chat => &cfg.chat.chain,
            Task::Transcribe => &cfg.transcribe.chain,
        };
        rows.push(Row::Header(task));

        // Availability comes from the real provider implementations, so the
        // marker means "the chain runner would use this now", not a guess.
        let ready: Vec<bool> = match task {
            Task::Chat => leo_services::ai::provider::build_chat_chain(cfg, store)
                .iter()
                .map(|p| p.available())
                .collect(),
            Task::Transcribe => leo_services::ai::provider::build_transcribe_chain(cfg, store)
                .iter()
                .map(|p| p.available())
                .collect(),
        };
        // The builder drops entries it cannot construct, so line readiness up by
        // name rather than by index.
        let ready_names: Vec<String> = match task {
            Task::Chat => leo_services::ai::provider::build_chat_chain(cfg, store)
                .iter()
                .map(|p| p.name().to_string())
                .collect(),
            Task::Transcribe => leo_services::ai::provider::build_transcribe_chain(cfg, store)
                .iter()
                .map(|p| p.name().to_string())
                .collect(),
        };

        for (i, name) in chain.iter().enumerate() {
            in_a_chain.push(name);
            let (model, credential) = match cfg.provider(name) {
                Some(pc) => (
                    pc.model.clone().unwrap_or_else(|| "(default)".to_string()),
                    credential_for(pc.account(name), pc.key_env.as_deref(), store),
                ),
                // Named in a chain but never defined: worth showing rather than
                // hiding, since it is a config typo the user should see.
                None => ("(not configured)".to_string(), Credential::Missing),
            };
            let is_ready = ready_names
                .iter()
                .position(|n| n == name)
                .and_then(|p| ready.get(p).copied())
                .unwrap_or(false);

            rows.push(Row::Member {
                task,
                position: i + 1,
                name: name.clone(),
                model,
                credential,
                ready: is_ready,
            });
        }
    }

    let unused: Vec<(&String, &leo_services::config::provider::ProviderConfig)> = cfg
        .providers
        .iter()
        .filter(|(name, _)| !in_a_chain.contains(name))
        .collect();

    if !unused.is_empty() {
        rows.push(Row::AvailableHeader);
        for (name, pc) in unused {
            rows.push(Row::Unused {
                name: name.clone(),
                model: pc.model.clone().unwrap_or_else(|| "(default)".to_string()),
                credential: credential_for(pc.account(name), pc.key_env.as_deref(), store),
                task: task_for(pc.kind),
            });
        }
    }

    rows.extend(appearance_rows(cfg));
    rows.extend(backup_rows(notes_dir, cfg));
    rows.extend(storage_rows(notes_dir));
    rows
}

/// The theme section: what colour the interface is, and how to change it.
fn appearance_rows(cfg: &Config) -> Vec<Row> {
    let palette = cfg.theme.palette();
    let named = leo_services::config::theme::presets()
        .into_iter()
        .find(|(_, rgb)| *rgb == palette.accent)
        .map(|(name, _)| name.to_string());

    vec![
        Row::Section("appearance".to_string()),
        Row::Setting {
            label: "colour".to_string(),
            // The hex is what the config holds, so showing it makes the row and
            // the file legible to each other.
            value: match named {
                Some(name) => format!("{name}  {}", palette.accent.to_hex()),
                None => palette.accent.to_hex(),
            },
            action: SettingAction::NextTheme,
        },
    ]
}

/// The GitHub backup section: whether notes are backed up, where to, and how far
/// behind.
///
/// Sync was previously only reachable by typing `:sync init`, then
/// `:sync connect <url>`, which meant the feature was invisible to anyone who had
/// not read the README.
fn backup_rows(notes_dir: &std::path::Path, cfg: &Config) -> Vec<Row> {
    let mut rows = vec![Row::Section("backup to github".to_string())];

    if !leo_core::sync::is_initialized(notes_dir) {
        rows.push(Row::Setting {
            label: "git backup".to_string(),
            value: "not set up".to_string(),
            action: SettingAction::SyncInit,
        });
        return rows;
    }

    match leo_core::sync::remote_url(notes_dir) {
        Some(url) => {
            // A setting, not a fact: a remote that cannot be changed from the
            // page that shows it is a dead end, and moving a repository is an
            // ordinary thing to do.
            rows.push(Row::Setting {
                label: "remote".to_string(),
                value: url.clone(),
                action: SettingAction::SyncConnect { current: Some(url) },
            });
            let waiting = match leo_core::sync::unpushed(notes_dir) {
                Some(0) => "everything is pushed".to_string(),
                Some(1) => "1 commit to push".to_string(),
                Some(n) => format!("{n} commits to push"),
                None => "no upstream branch yet".to_string(),
            };
            rows.push(Row::Setting {
                label: "push".to_string(),
                value: waiting,
                action: SettingAction::SyncPush,
            });
            rows.push(Row::Setting {
                label: "pull".to_string(),
                value: "fetch and reload".to_string(),
                action: SettingAction::SyncPull,
            });
            // Only offered once there is somewhere to push to: the setting means
            // nothing without a remote.
            rows.push(Row::Setting {
                label: "automatically".to_string(),
                value: cfg.sync.auto_push.label().to_string(),
                action: SettingAction::NextAutoPush,
            });
        }
        None => rows.push(Row::Setting {
            label: "remote".to_string(),
            value: "none — connect one".to_string(),
            action: SettingAction::SyncConnect { current: None },
        }),
    }

    rows
}

/// Where things are on disk. Facts rather than settings: leo decides these, and
/// the user only needs to be able to find them.
fn storage_rows(notes_dir: &std::path::Path) -> Vec<Row> {
    let mut rows = vec![Row::Section("where things live".to_string())];
    rows.push(Row::Fact {
        label: "notes".to_string(),
        value: notes_dir.display().to_string(),
    });
    if let Ok(path) = Config::config_path() {
        rows.push(Row::Setting {
            label: "settings".to_string(),
            value: path.display().to_string(),
            action: SettingAction::EditConfig,
        });
    }
    if let Ok(file) = leo_services::config::file_store::FileStore::new() {
        rows.push(Row::Fact {
            label: "keys".to_string(),
            value: format!("{} (only you can read it)", file.path().display()),
        });
    }
    rows
}

/// What the App should do after an action that changed the config file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Changed {
    /// Nothing was written.
    No,
    /// The file changed; reload config and rebuild the rows. Carries a message.
    Yes(String),
}

/// Switch to the next colour preset and write it to the config.
///
/// Cycling rather than offering a list: there are six, and pressing a key until
/// it looks right is faster than reading names. Writes `preset` and clears any
/// explicit `accent`, so the choice on screen and the file agree.
pub fn cycle_theme() -> Result<Changed> {
    let (path, mut doc) = edit::load_document()?;

    let presets: Vec<&str> = leo_services::config::theme::presets()
        .keys()
        .copied()
        .collect();
    let current = doc
        .get("theme")
        .and_then(|t| t.get("preset"))
        .and_then(|p| p.as_str())
        .unwrap_or("orange");
    let next = presets
        .iter()
        .position(|p| *p == current)
        .map(|i| presets[(i + 1) % presets.len()])
        .unwrap_or(presets[0]);

    let theme = doc
        .entry("theme")
        .or_insert(toml_edit::Item::Table(toml_edit::Table::new()));
    if let Some(table) = theme.as_table_mut() {
        table.insert("preset", toml_edit::value(next));
        // An explicit accent would win over the preset, which would make this
        // key appear to do nothing.
        table.remove("accent");
    }

    edit::save_document(&path, &doc)?;
    Ok(Changed::Yes(format!(
        "Colour set to {next}. Restart leo to see it."
    )))
}

/// Switch when leo pushes on its own, and write it to the config.
pub fn cycle_auto_push() -> Result<Changed> {
    let (path, mut doc) = edit::load_document()?;

    let current = doc
        .get("sync")
        .and_then(|t| t.get("auto_push"))
        .and_then(|v| v.as_str())
        .and_then(|s| match s {
            "off" => Some(leo_services::config::sync::AutoPush::Off),
            "on_quit" => Some(leo_services::config::sync::AutoPush::OnQuit),
            "when_idle" => Some(leo_services::config::sync::AutoPush::WhenIdle),
            _ => None,
        })
        .unwrap_or_default();
    let next = current.next();

    let table = doc
        .entry("sync")
        .or_insert(toml_edit::Item::Table(toml_edit::Table::new()));
    if let Some(table) = table.as_table_mut() {
        table.insert("auto_push", toml_edit::value(next.as_str()));
    }

    edit::save_document(&path, &doc)?;
    Ok(Changed::Yes(format!("Backing up {}.", next.label())))
}

pub fn step_provider(task: Task, delta: isize) -> Result<Changed> {
    let (path, mut doc) = edit::load_document()?;
    let cfg = Config::load();
    let store = leo_services::config::secret::default_store();
    let current = choice::selection(&cfg, store.as_ref(), task).map(|s| s.provider);
    let next = choice::step_choice(task, current.as_deref(), delta);
    choice::write_choice(&mut doc, task, next.provider);
    edit::save_document(&path, &doc)?;
    let what = match task {
        Task::Chat => "Writing",
        Task::Transcribe => "Speech",
    };
    Ok(Changed::Yes(format!("{what} now uses {}.", next.label())))
}

pub fn step_model(task: Task, delta: isize, local: &Local) -> Result<Changed> {
    let cfg = Config::load();
    let store = leo_services::config::secret::default_store();
    let Some(sel) = choice::selection(&cfg, store.as_ref(), task) else {
        return Ok(Changed::No);
    };
    let options = choice::model_options(task, &sel.provider, local);
    let current = sel.model.as_deref().map(|m| {
        leo_services::ai::provider::whisper_cpp::expand_tilde(m)
            .display()
            .to_string()
    });
    let Some(next) = choice::step(&options, current.as_deref(), delta) else {
        return Ok(Changed::No);
    };
    let (path, mut doc) = edit::load_document()?;
    choice::write_model(&mut doc, &sel.provider, next);
    edit::save_document(&path, &doc)?;
    let shown = if task == Task::Transcribe && sel.choice.is_some_and(|c| c.local()) {
        choice::whisper_label(next)
    } else {
        next.clone()
    };
    Ok(Changed::Yes(format!("Model set to {shown}.")))
}

pub fn use_model(task: Task, model: &str) -> Result<Changed> {
    let provider = match task {
        Task::Chat => "ollama",
        Task::Transcribe => "whisper_cpp",
    };
    let (path, mut doc) = edit::load_document()?;
    choice::write_model(&mut doc, provider, model);
    edit::save_document(&path, &doc)?;
    Ok(Changed::Yes(format!(
        "Downloaded. {} uses it now.",
        match task {
            Task::Chat => "Writing",
            Task::Transcribe => "Speech",
        }
    )))
}

/// Move a chain member up or down and persist it. `delta` is -1 or 1.
pub fn reorder(task: Task, name: &str, delta: isize) -> Result<Changed> {
    let (path, mut doc) = edit::load_document()?;
    let mut chain = edit::read_chain(&doc, task);

    let Some(index) = chain.iter().position(|n| n == name) else {
        return Ok(Changed::No);
    };
    let moved = if delta < 0 {
        edit::move_up(&mut chain, index)
    } else {
        edit::move_down(&mut chain, index)
    };
    if moved == index {
        return Ok(Changed::No);
    }

    edit::write_chain(&mut doc, task, &chain);
    edit::save_document(&path, &doc)?;
    Ok(Changed::Yes(format!(
        "{name} is now {} in the {} chain",
        moved + 1,
        task.label()
    )))
}

/// Add a provider to the chain its kind can serve.
pub fn add_to_chain(task: Task, name: &str) -> Result<Changed> {
    let (path, mut doc) = edit::load_document()?;
    let mut chain = edit::read_chain(&doc, task);
    if !edit::add(&mut chain, name) {
        return Ok(Changed::No);
    }
    edit::write_chain(&mut doc, task, &chain);
    edit::save_document(&path, &doc)?;
    Ok(Changed::Yes(format!(
        "{name} added to the {} chain",
        task.label()
    )))
}

/// Remove a provider from a chain. Its definition stays in the file, so this is
/// reversible with one keypress.
pub fn remove_from_chain(task: Task, name: &str) -> Result<Changed> {
    let (path, mut doc) = edit::load_document()?;
    let mut chain = edit::read_chain(&doc, task);
    if !edit::remove(&mut chain, name) {
        return Ok(Changed::No);
    }
    edit::write_chain(&mut doc, task, &chain);
    edit::save_document(&path, &doc)?;
    Ok(Changed::Yes(format!(
        "{name} removed from the {} chain (still configured)",
        task.label()
    )))
}

#[cfg(test)]
mod tests {
    fn simple_page(
        config: &str,
        store: &dyn SecretStore,
        local: &Local,
    ) -> Vec<(String, String, String)> {
        use super::*;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(&path, config).unwrap();
        let cfg = Config::load_from(&path);
        simple_rows(&cfg, store, tmp.path(), local)
            .into_iter()
            .filter_map(|r| match r {
                Row::Setting {
                    label,
                    value,
                    action,
                } => Some((label, value, format!("{action:?}"))),
                Row::Fact { label, value } => Some((label, value, "Fact".to_string())),
                _ => None,
            })
            .collect()
    }

    fn row<'a>(
        rows: &'a [(String, String, String)],
        label: &str,
    ) -> Option<&'a (String, String, String)> {
        rows.iter().find(|(l, _, _)| l == label)
    }

    #[test]
    fn the_simple_page_offers_a_provider_a_model_and_a_key_for_each_task() {
        use super::*;
        let store = MemoryStore::default();
        store.set("openai", "k").unwrap();
        let rows = simple_page(
            "[chat]\nchain = [\"anthropic\"]\n[transcribe]\nchain = [\"openai_whisper\"]\n",
            &store,
            &Local::default(),
        );
        let writing = row(&rows, "writing").unwrap();
        assert_eq!(writing.1, "○ Anthropic");
        assert_eq!(writing.2, "ChooseProvider(Chat)");
        let model = row(&rows, "writing model").unwrap();
        assert_eq!(model.1, "claude-sonnet-5-5");
        assert_eq!(model.2, "ChooseModel(Chat)");
        let key = row(&rows, "Anthropic key").unwrap();
        assert_eq!(key.2, "StoreKey { name: \"anthropic\" }");
        if std::env::var("ANTHROPIC_API_KEY").is_err() {
            assert_eq!(key.1, "none — Enter to add one");
        }
        assert_eq!(row(&rows, "speech").unwrap().1, "● OpenAI");
        assert_eq!(row(&rows, "speech model").unwrap().1, "gpt-transcribe");
        let openai = row(&rows, "OpenAI key").unwrap();
        if std::env::var("OPENAI_API_KEY").is_err() {
            assert_eq!(openai.1, "stored");
        }
        assert!(row(&rows, "AI providers").is_some());
        assert!(!rows.iter().any(|(l, _, _)| l.contains("chain")));
    }

    #[test]
    fn one_cloud_for_both_tasks_asks_for_its_key_once() {
        use super::*;
        let rows = simple_page(
            "[chat]\nchain = [\"gemini\"]\n[transcribe]\nchain = [\"gemini_speech\"]\n",
            &MemoryStore::default(),
            &Local::default(),
        );
        let keys: Vec<_> = rows
            .iter()
            .filter(|(l, _, _)| l.ends_with(" key"))
            .collect();
        assert_eq!(keys.len(), 1, "{rows:?}");
        assert_eq!(keys[0].2, "StoreKey { name: \"gemini\" }");
    }

    #[test]
    fn a_local_choice_says_what_is_missing_and_offers_the_download() {
        use super::*;
        let config = "[chat]\nchain = [\"ollama\"]\n[transcribe]\nchain = [\"whisper_cpp\"]\n";
        let rows = simple_page(config, &MemoryStore::default(), &Local::default());
        let writing_model = row(&rows, "writing model").unwrap();
        assert_eq!(writing_model.2, "Fact");
        assert!(
            writing_model.1.contains("Ollama is not"),
            "{writing_model:?}"
        );
        let speech_model = row(&rows, "speech model").unwrap();
        assert_eq!(speech_model.2, "GetLocalModel(Transcribe)");
        assert!(!rows.iter().any(|(l, _, _)| l.ends_with(" key")));

        let running = Local {
            ollama_running: true,
            ollama: Vec::new(),
            whisper: vec![std::path::PathBuf::from("/m/ggml-small.bin")],
        };
        let rows = simple_page(config, &MemoryStore::default(), &running);
        assert_eq!(
            row(&rows, "writing model").unwrap().2,
            "GetLocalModel(Chat)"
        );
        let speech_model = row(&rows, "speech model").unwrap();
        assert_eq!(speech_model.2, "ChooseModel(Transcribe)");
        assert!(
            speech_model.1.starts_with("base.en is missing"),
            "{speech_model:?}"
        );

        let pulled = Local {
            ollama_running: true,
            ollama: vec!["qwen3:8b".to_string(), "gemma3:4b".to_string()],
            whisper: Vec::new(),
        };
        let rows = simple_page(config, &MemoryStore::default(), &pulled);
        let writing_model = row(&rows, "writing model").unwrap();
        assert_eq!(writing_model.1, "qwen3:8b");
        assert_eq!(writing_model.2, "ChooseModel(Chat)");
    }

    #[test]
    fn a_chain_set_by_hand_is_shown_by_name_and_left_alone() {
        use super::*;
        let rows = simple_page(
            "[chat]\nchain = [\"openrouter\"]\n[transcribe]\nchain = []\n",
            &MemoryStore::default(),
            &Local::default(),
        );
        assert_eq!(
            row(&rows, "writing").unwrap().1,
            "○ openrouter (from config.toml)"
        );
        assert!(row(&rows, "writing model").is_none());
        assert_eq!(row(&rows, "speech").unwrap().1, "○ nothing chosen");
    }

    /// Enter does the one thing a provider row most needs: a key when it has
    /// none, joining a list when it is unused, otherwise a test.
    #[test]
    fn enter_on_a_provider_does_what_it_most_needs() {
        use crate::view::settings::Credential;
        assert_eq!(
            primary_action(&Credential::Missing, true),
            ProviderOp::Login
        );
        assert_eq!(
            primary_action(&Credential::Missing, false),
            ProviderOp::Login
        );
        assert_eq!(primary_action(&Credential::Stored, false), ProviderOp::Add);
        assert_eq!(
            primary_action(&Credential::NotNeeded, true),
            ProviderOp::Test
        );
    }

    use super::*;
    use leo_services::config::secret::MemoryStore;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn small_config() -> Config {
        Config::parse(
            r#"
[chat]
chain = ["ollama", "openrouter"]

[transcribe]
chain = ["groq"]

[providers.ollama]
kind = "openai"
base_url = "http://localhost:11434/v1"
model = "qwen3:8b"

[providers.openrouter]
kind = "openai"
base_url = "https://openrouter.ai/api/v1"
model = "openrouter/free"
key_env = "LEO_TEST_SETTINGS_OR"

[providers.groq]
kind = "groq"
model = "whisper-large-v3-turbo"
key_env = "LEO_TEST_SETTINGS_GROQ"

[providers.cerebras]
kind = "openai"
base_url = "https://api.cerebras.ai/v1"
model = "llama-3.3-70b"
key_env = "LEO_TEST_SETTINGS_CB"
"#,
        )
        .unwrap()
    }

    #[test]
    fn rows_list_both_chains_in_order_then_the_rest() {
        let _guard = ENV_LOCK.lock().unwrap();
        for v in [
            "LEO_TEST_SETTINGS_OR",
            "LEO_TEST_SETTINGS_GROQ",
            "LEO_TEST_SETTINGS_CB",
        ] {
            std::env::remove_var(v);
        }
        let rows = rows(
            &small_config(),
            &MemoryStore::default(),
            std::path::Path::new("/tmp/leo-test-notes"),
        );

        assert_eq!(rows[0], Row::Header(Task::Chat));
        assert_eq!(rows[1].provider_name(), Some("ollama"));
        assert_eq!(rows[2].provider_name(), Some("openrouter"));
        assert_eq!(rows[3], Row::Header(Task::Transcribe));
        assert_eq!(rows[4].provider_name(), Some("groq"));
        assert_eq!(rows[5], Row::AvailableHeader);
        // Everything configured but unchained shows up, so a user can see what
        // is available without opening the file.
        assert_eq!(rows[6].provider_name(), Some("cerebras"));
        // The provider part ends where the rest of the page begins.
        assert!(matches!(rows[7], Row::Section(_)), "{:?}", rows[7]);
    }

    #[test]
    fn a_chain_position_is_shown_as_its_priority() {
        let _guard = ENV_LOCK.lock().unwrap();
        let rows = rows(
            &small_config(),
            &MemoryStore::default(),
            std::path::Path::new("/tmp/leo-test-notes"),
        );
        match &rows[2] {
            Row::Member { position, name, .. } => {
                assert_eq!(*position, 2);
                assert_eq!(name, "openrouter");
            }
            other => panic!("expected a member, got {other:?}"),
        }
    }

    #[test]
    fn a_keyless_local_provider_needs_no_credential() {
        let _guard = ENV_LOCK.lock().unwrap();
        let rows = rows(
            &small_config(),
            &MemoryStore::default(),
            std::path::Path::new("/tmp/leo-test-notes"),
        );
        match &rows[1] {
            Row::Member { credential, .. } => assert_eq!(*credential, Credential::NotNeeded),
            other => panic!("expected a member, got {other:?}"),
        }
    }

    #[test]
    fn a_stored_key_shows_as_stored_and_a_missing_one_says_so() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("LEO_TEST_SETTINGS_OR");
        std::env::remove_var("LEO_TEST_SETTINGS_GROQ");

        let store = MemoryStore::default();
        store.set("openrouter", "sk-or-v1-secret9999").unwrap();
        let rows = rows(
            &small_config(),
            &store,
            std::path::Path::new("/tmp/leo-test-notes"),
        );

        match &rows[2] {
            Row::Member { credential, .. } => {
                assert_eq!(*credential, Credential::Stored);
            }
            other => panic!("expected a member, got {other:?}"),
        }
        match &rows[4] {
            Row::Member { credential, .. } => assert_eq!(*credential, Credential::Missing),
            other => panic!("expected a member, got {other:?}"),
        }
    }

    /// Env wins over the keychain, and the screen has to say so — otherwise a
    /// user who logs in and sees no change has no way to understand why.
    #[test]
    fn an_env_var_is_reported_as_env_even_when_the_keychain_has_one() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("LEO_TEST_SETTINGS_OR", "from-env-1234");
        let store = MemoryStore::default();
        store.set("openrouter", "from-keychain-9999").unwrap();

        let rows = rows(
            &small_config(),
            &store,
            std::path::Path::new("/tmp/leo-test-notes"),
        );
        std::env::remove_var("LEO_TEST_SETTINGS_OR");

        match &rows[2] {
            Row::Member { credential, .. } => match credential {
                Credential::Env { var, redacted } => {
                    assert_eq!(var, "LEO_TEST_SETTINGS_OR");
                    assert_eq!(redacted, "…1234");
                }
                other => panic!("expected env, got {other:?}"),
            },
            other => panic!("expected a member, got {other:?}"),
        }
    }

    #[test]
    fn a_chain_entry_with_no_provider_block_is_shown_rather_than_hidden() {
        let _guard = ENV_LOCK.lock().unwrap();
        let cfg = Config::parse("[chat]\nchain = [\"ghost\"]\n").unwrap();
        let rows = rows(
            &cfg,
            &MemoryStore::default(),
            std::path::Path::new("/tmp/leo-test-notes"),
        );

        match &rows[1] {
            Row::Member {
                name, model, ready, ..
            } => {
                assert_eq!(name, "ghost");
                assert_eq!(model, "(not configured)");
                assert!(!ready);
            }
            other => panic!("expected a member, got {other:?}"),
        }
    }

    #[test]
    fn an_unused_provider_is_offered_to_the_chain_its_kind_can_serve() {
        let _guard = ENV_LOCK.lock().unwrap();
        let cfg = Config::parse(
            r#"
[providers.some_chat]
kind = "openai"
base_url = "https://x/v1"
model = "m"
key_env = "A_API_KEY"

[providers.some_whisper]
kind = "groq"
model = "w"
key_env = "B_API_KEY"

[providers.local_binary]
kind = "whisper_cpp"
bin = "whisper-cli"
model_path = "/nope"
"#,
        )
        .unwrap();
        let rows = rows(
            &cfg,
            &MemoryStore::default(),
            std::path::Path::new("/tmp/leo-test-notes"),
        );

        let task_of = |name: &str| {
            rows.iter()
                .find(|r| r.provider_name() == Some(name))
                .and_then(|r| r.task())
                .unwrap()
        };
        assert_eq!(task_of("some_chat"), Task::Chat);
        assert_eq!(task_of("some_whisper"), Task::Transcribe);
        assert_eq!(task_of("local_binary"), Task::Transcribe);
    }

    #[test]
    fn an_empty_config_produces_only_headers_and_the_rest_of_the_page() {
        let _guard = ENV_LOCK.lock().unwrap();
        let cfg = Config::parse("").unwrap();
        let rows = rows(
            &cfg,
            &MemoryStore::default(),
            std::path::Path::new("/tmp/leo-test-notes"),
        );

        // No providers, but both chain headers still say so.
        let providers: Vec<&Row> = rows
            .iter()
            .filter(|r| r.provider_name().is_some())
            .collect();
        assert!(providers.is_empty(), "{providers:?}");
        assert_eq!(rows[0], Row::Header(Task::Chat));
        assert_eq!(rows[1], Row::Header(Task::Transcribe));

        // And the page is more than providers: appearance, backup, storage.
        let sections: Vec<&str> = rows
            .iter()
            .filter_map(|r| match r {
                Row::Section(title) => Some(title.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            sections,
            ["appearance", "backup to github", "where things live"]
        );
    }

    /// Everything on the page must either do something or be worth reading, and
    /// j/k must only stop on the rows that do something.
    #[test]
    fn only_actionable_rows_are_selectable() {
        let _guard = ENV_LOCK.lock().unwrap();
        let rows = rows(
            &small_config(),
            &MemoryStore::default(),
            std::path::Path::new("/tmp/leo-test-notes"),
        );

        for row in &rows {
            match row {
                Row::Header(_) | Row::AvailableHeader | Row::Section(_) | Row::Fact { .. } => {
                    assert!(!row.selectable(), "{row:?} should not be selectable")
                }
                Row::Member { .. } | Row::Unused { .. } | Row::Setting { .. } => {
                    assert!(row.selectable(), "{row:?} should be selectable")
                }
            }
        }
        // And every setting says what choosing it will do.
        for row in rows.iter().filter(|r| matches!(r, Row::Setting { .. })) {
            let action = row.action().expect("a setting with no action");
            assert!(!action.describe().is_empty());
        }
    }

    /// The colour row must name the preset when the accent matches one, since
    /// "orange" is more use than a hex string.
    #[test]
    fn the_appearance_row_names_the_current_colour() {
        let cfg = Config::parse("").unwrap();
        let rows = appearance_rows(&cfg);
        let Row::Setting {
            label,
            value,
            action,
        } = &rows[1]
        else {
            panic!("expected a setting, got {:?}", rows[1]);
        };
        assert_eq!(label, "colour");
        assert!(value.contains("orange"), "{value}");
        assert!(value.contains("#d97757"), "{value}");
        assert_eq!(*action, SettingAction::NextTheme);
    }

    /// Sync was previously invisible unless the user had read the README.
    #[test]
    fn backup_offers_setup_when_there_is_no_repo() {
        let dir = tempfile::tempdir().unwrap();
        let rows = backup_rows(dir.path(), &Config::default());
        assert_eq!(rows[0], Row::Section("backup to github".to_string()));
        let Row::Setting { value, action, .. } = &rows[1] else {
            panic!("expected a setting, got {:?}", rows[1]);
        };
        assert_eq!(value, "not set up");
        assert_eq!(*action, SettingAction::SyncInit);
    }

    /// The gap this closes: a configured remote was shown as a fact, so the page
    /// that displayed where notes were backed up gave no way to change it.
    #[test]
    fn a_configured_remote_can_be_changed_from_the_page() {
        let dir = tempfile::tempdir().unwrap();
        let notes = dir.path().join("notes");
        std::fs::create_dir_all(&notes).unwrap();
        if leo_core::sync::init(&notes).is_err() {
            return; // no git on this machine
        }
        let url = "https://github.com/example/notes.git";
        if leo_core::sync::connect(&notes, url).is_err() {
            return;
        }

        let rows = backup_rows(&notes, &Config::default());
        let remote = rows
            .iter()
            .find(|r| matches!(r, Row::Setting { label, .. } if label == "remote"))
            .expect("the remote row is not a setting, so it cannot be changed");

        let Row::Setting { value, action, .. } = remote else {
            unreachable!()
        };
        assert_eq!(value, url);
        // And it carries the current URL, so the prompt can prefill it rather
        // than making the user retype a URL to change one character.
        assert_eq!(
            *action,
            SettingAction::SyncConnect {
                current: Some(url.to_string())
            }
        );
        assert!(remote.selectable(), "the remote row cannot be selected");
    }

    /// The setting is only meaningful once there is a remote, and it has to be
    /// changeable from the page that shows it.
    #[test]
    fn automatic_backup_is_offered_once_a_remote_exists() {
        let dir = tempfile::tempdir().unwrap();
        let notes = dir.path().join("notes");
        std::fs::create_dir_all(&notes).unwrap();
        if leo_core::sync::init(&notes).is_err() {
            return;
        }

        // No remote yet: nothing to automate.
        let rows = backup_rows(&notes, &Config::default());
        assert!(
            !rows.iter().any(|r| matches!(r, Row::Setting { action, .. }
                if *action == SettingAction::NextAutoPush)),
            "offered automatic backup with nowhere to push"
        );

        if leo_core::sync::connect(&notes, "https://github.com/example/n.git").is_err() {
            return;
        }
        let rows = backup_rows(&notes, &Config::default());
        let row = rows
            .iter()
            .find(|r| {
                matches!(r, Row::Setting { action, .. }
                if *action == SettingAction::NextAutoPush)
            })
            .expect("no automatic backup row");
        let Row::Setting { value, .. } = row else {
            unreachable!()
        };
        // The default is stated rather than left blank.
        assert_eq!(
            value,
            leo_services::config::sync::AutoPush::default().label()
        );
    }

    #[test]
    fn storage_shows_where_notes_and_keys_live() {
        let rows = storage_rows(std::path::Path::new("/tmp/leo-test-notes"));
        let values: String = rows
            .iter()
            .map(|r| match r {
                Row::Fact { label, value } | Row::Setting { label, value, .. } => {
                    format!("{label}={value} ")
                }
                _ => String::new(),
            })
            .collect();
        assert!(values.contains("notes=/tmp/leo-test-notes"), "{values}");
        assert!(values.contains("config.toml"), "{values}");
        assert!(values.contains("credentials.json"), "{values}");
    }

    #[test]
    fn readiness_reflects_whether_the_chain_runner_would_use_it() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("LEO_TEST_SETTINGS_OR");
        let store = MemoryStore::default();

        // Without a key, openrouter is not ready.
        let rows_before = rows(
            &small_config(),
            &store,
            std::path::Path::new("/tmp/leo-test-notes"),
        );
        let ready_before = matches!(&rows_before[2], Row::Member { ready: true, .. });
        assert!(!ready_before);

        // With one, it is.
        store.set("openrouter", "a-key").unwrap();
        let rows_after = rows(
            &small_config(),
            &store,
            std::path::Path::new("/tmp/leo-test-notes"),
        );
        assert!(matches!(&rows_after[2], Row::Member { ready: true, .. }));
    }
}
