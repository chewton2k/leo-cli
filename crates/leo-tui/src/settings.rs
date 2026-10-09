//! The provider screen's behavior: building its rows from config plus the
//! keychain, and applying the actions the user takes on them.
//!
//! Kept out of `tui/mod.rs` so the row-building and chain-editing logic can be
//! tested against an in-memory secret store, with no terminal and no keychain.

use anyhow::Result;

use crate::view::settings::{Credential, Row, SettingAction};
use leo_services::config::choice::{self, Local};
use leo_services::config::edit::{self, Task};
use leo_services::config::secret::SecretStore;
use leo_services::config::Config;

/// Describe where a provider's credential comes from, without revealing it.
///
/// Asks whether a key exists rather than reading it: this runs for every
/// provider on the screen, and on a real keychain each *read* can cost a
/// permission dialog while an existence check does not.
fn credential_for(name: &str, key_env: Option<&str>, store: &dyn SecretStore) -> Credential {
    let Some(var) = key_env else {
        return Credential::NotNeeded;
    };
    if store.has(name) {
        return Credential::Stored;
    }
    if std::env::var(var).is_ok_and(|v| !v.trim().is_empty()) {
        return Credential::Ignored {
            var: var.to_string(),
        };
    }
    Credential::Missing
}

pub fn simple_rows(
    cfg: &Config,
    store: &dyn SecretStore,
    notes_dir: &std::path::Path,
    local: &Local,
    usage: &leo_services::usage::Seen,
) -> Vec<Row> {
    let mut rows = vec![Row::Section("AI".to_string())];
    let writing_key = ai_rows(&mut rows, cfg, store, local, usage, Task::Chat, None);
    ai_rows(
        &mut rows,
        cfg,
        store,
        local,
        usage,
        Task::Transcribe,
        writing_key.as_deref(),
    );
    rows.extend(appearance_rows(cfg));
    rows.extend(backup_rows(notes_dir, cfg));
    rows.extend(storage_rows(notes_dir));
    rows
}

fn ai_rows(
    rows: &mut Vec<Row>,
    cfg: &Config,
    store: &dyn SecretStore,
    local: &Local,
    usage: &leo_services::usage::Seen,
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
    let limits = cfg
        .provider(&sel.provider)
        .and_then(leo_services::ai::provider::agent_cli::Agent::of)
        .and_then(|_| usage.get(&sel.provider))
        .map(|seen| {
            format!(
                " ({})",
                leo_services::usage::label(seen, chrono::Utc::now())
            )
        })
        .unwrap_or_default();
    rows.push(Row::Setting {
        label: what.to_string(),
        value: format!("{mark} {}{limits}", chosen.label()),
        action: SettingAction::ChooseProvider(task),
    });
    let model_label = format!("{what} model");
    if chosen.local() {
        local_rows(rows, local, task, &sel, model_label);
        return None;
    }
    rows.push(Row::Setting {
        label: model_label,
        value: sel
            .model
            .as_deref()
            .map(|m| choice::priced(task, &sel.provider, m))
            .unwrap_or_else(|| "(default)".to_string()),
        action: SettingAction::ChooseModel(task),
    });
    let pc = cfg.provider(&sel.provider)?;
    if task == Task::Chat
        && !choice::efforts(&sel.provider, sel.model.as_deref().unwrap_or("")).is_empty()
    {
        rows.push(Row::Setting {
            label: format!("{what} effort"),
            value: pc.effort.clone().unwrap_or_else(|| "default".to_string()),
            action: SettingAction::ChooseEffort,
        });
    }
    if let Some(agent) = leo_services::ai::provider::agent_cli::Agent::of(pc) {
        let bin = pc.bin.as_deref().unwrap_or(agent.program());
        let value = if leo_services::ai::provider::agent_cli::locate(bin).is_some() {
            format!(
                "uses {}, no key; if asked, {}",
                agent.plan(),
                agent.sign_in()
            )
        } else {
            format!("`{bin}` is not installed: {}", agent.install())
        };
        rows.push(Row::Fact {
            label: format!("{} sign-in", chosen.name),
            value,
        });
        return None;
    }
    let account = pc.account(&sel.provider).to_string();
    if shown_key == Some(account.as_str()) {
        return Some(account);
    }
    let value = match credential_for(&account, pc.key_env.as_deref(), store) {
        Credential::Stored => "stored".to_string(),
        Credential::Ignored { var } => {
            format!("none — Enter to add one (leo does not read ${var})")
        }
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
                format!("{current} (free)")
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
            if !local.speech_ready {
                rows.push(Row::Setting {
                    label: model_label,
                    value: format!(
                        "none yet — Enter downloads it ({} MB, once)",
                        leo_services::ai::provider::parakeet::MODEL_MB
                    ),
                    action: SettingAction::GetLocalModel(task),
                });
            }
        }
    }
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
    let Some(next) = choice::step(&options, sel.model.as_deref(), delta) else {
        return Ok(Changed::No);
    };
    let (path, mut doc) = edit::load_document()?;
    choice::write_model(&mut doc, &sel.provider, next);
    edit::save_document(&path, &doc)?;
    Ok(Changed::Yes(format!(
        "Model set to {}.",
        choice::priced(task, &sel.provider, next)
    )))
}

pub fn step_effort(delta: isize) -> Result<Changed> {
    let cfg = Config::load();
    let store = leo_services::config::secret::default_store();
    let Some(sel) = choice::selection(&cfg, store.as_ref(), Task::Chat) else {
        return Ok(Changed::No);
    };
    let model = sel.model.clone().unwrap_or_default();
    let current = cfg.provider(&sel.provider).and_then(|p| p.effort.clone());
    let Some(next) = choice::step_effort(&sel.provider, &model, current.as_deref(), delta) else {
        return Ok(Changed::No);
    };
    let (path, mut doc) = edit::load_document()?;
    choice::write_effort(&mut doc, &sel.provider, next);
    edit::save_document(&path, &doc)?;
    Ok(Changed::Yes(match next {
        Some(effort) => format!("Effort set to {effort}."),
        None => "Effort set to the model's default.".to_string(),
    }))
}

pub fn use_model(task: Task, model: &str) -> Result<Changed> {
    if task == Task::Transcribe {
        return Ok(Changed::Yes("Downloaded. Speech uses it now.".to_string()));
    }
    let provider = "ollama";
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

#[cfg(test)]
mod tests {
    use super::*;
    use leo_services::config::secret::MemoryStore;

    fn simple_page(
        config: &str,
        store: &dyn SecretStore,
        local: &Local,
    ) -> Vec<(String, String, String)> {
        simple_page_with(config, store, local, &Default::default())
    }

    fn simple_page_with(
        config: &str,
        store: &dyn SecretStore,
        local: &Local,
        usage: &leo_services::usage::Seen,
    ) -> Vec<(String, String, String)> {
        use super::*;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(&path, config).unwrap();
        let cfg = Config::load_from(&path);
        simple_rows(&cfg, store, tmp.path(), local, usage)
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
    fn writing_with_codex_or_claude_code_offers_an_effort_and_ollama_does_not() {
        use super::*;
        let store = MemoryStore::default();
        let rows = simple_page(
            "[chat]\nchain = [\"codex\"]\n[providers.codex]\neffort = \"high\"\n[transcribe]\nchain = []\n",
            &store,
            &Local::default(),
        );
        let effort = row(&rows, "writing effort").unwrap();
        assert_eq!(
            (effort.1.as_str(), effort.2.as_str()),
            ("high", "ChooseEffort")
        );
        let rows = simple_page(
            "[chat]\nchain = [\"claude_code\"]\n[transcribe]\nchain = []\n",
            &store,
            &Local::default(),
        );
        assert_eq!(row(&rows, "writing effort").unwrap().1, "default");
        let rows = simple_page(
            "[chat]\nchain = [\"ollama\"]\n[transcribe]\nchain = []\n",
            &store,
            &Local::default(),
        );
        assert!(row(&rows, "writing effort").is_none());
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
        assert_eq!(model.1, "claude-sonnet-5-5 ($2 in, $10 out per 1M tokens)");
        assert_eq!(model.2, "ChooseModel(Chat)");
        let key = row(&rows, "Anthropic key").unwrap();
        assert_eq!(key.2, "StoreKey { name: \"anthropic\" }");
        assert!(key.1.starts_with("none — Enter to add one"), "{}", key.1);
        assert_eq!(row(&rows, "speech").unwrap().1, "● OpenAI");
        assert_eq!(
            row(&rows, "speech model").unwrap().1,
            "gpt-transcribe ($0.27 per hour)"
        );
        let openai = row(&rows, "OpenAI key").unwrap();
        assert_eq!(openai.1, "stored");
        assert!(!rows.iter().any(|(l, _, _)| l.contains("chain")));
    }

    #[test]
    fn a_coding_plan_writes_without_a_key() {
        use super::*;
        for (provider, name, model, missing) in [
            (
                "claude_code",
                "Claude Code",
                "claude-sonnet-5-5 (included in your Claude plan)",
                "leo-no-such-claude",
            ),
            (
                "codex",
                "Codex",
                "gpt-6-luna (included in your ChatGPT plan)",
                "leo-no-such-codex",
            ),
        ] {
            let rows = simple_page(
                &format!(
                    "[chat]\nchain = [\"{provider}\"]\n[transcribe]\nchain = []\n[providers.{provider}]\nbin = \"{missing}\"\n"
                ),
                &MemoryStore::default(),
                &Local::default(),
            );
            assert_eq!(row(&rows, "writing").unwrap().1, format!("○ {name}"));
            let chosen = row(&rows, "writing model").unwrap();
            assert_eq!(chosen.1, model);
            assert_eq!(chosen.2, "ChooseModel(Chat)");
            let sign_in = row(&rows, &format!("{name} sign-in")).unwrap();
            assert_eq!(sign_in.2, "Fact");
            assert!(sign_in.1.contains("is not installed"), "{}", sign_in.1);
            assert!(
                !rows.iter().any(|(l, _, _)| l.ends_with(" key")),
                "{rows:?}"
            );
        }
    }

    #[test]
    fn a_coding_plan_shows_its_latest_limits_next_to_its_name() {
        use super::*;
        let seen = |five, week| leo_services::usage::Usage {
            five_hour: Some(leo_services::usage::Window {
                used: five,
                resets_at: None,
            }),
            seven_day: Some(leo_services::usage::Window {
                used: week,
                resets_at: None,
            }),
            seen_at: chrono::Utc::now(),
        };
        let mut usage = leo_services::usage::Seen::new();
        usage.insert("claude_code".to_string(), seen(0.45, 0.9));
        usage.insert("codex".to_string(), seen(0.02, 0.47));
        usage.insert("openai".to_string(), seen(0.5, 0.5));
        let page = |chain: &str| {
            simple_page_with(
                &format!("[chat]\nchain = [\"{chain}\"]\n[transcribe]\nchain = []\n"),
                &MemoryStore::default(),
                &Local::default(),
                &usage,
            )
        };
        let rows = page("claude_code");
        assert!(row(&rows, "writing")
            .unwrap()
            .1
            .ends_with("Claude Code (5h: 45%, 7d: 90%)"));
        let rows = page("codex");
        assert!(row(&rows, "writing")
            .unwrap()
            .1
            .ends_with("Codex (5h: 2%, 7d: 47%)"));
        let rows = page("openai");
        assert!(row(&rows, "writing").unwrap().1.ends_with("OpenAI"));
    }

    #[test]
    fn a_key_in_the_environment_is_never_used_and_the_row_says_so() {
        use super::*;
        let config = "[chat]\nchain = [\"openrouter\"]\n[transcribe]\nchain = []\n";
        std::env::set_var("OPENROUTER_API_KEY", "sk-from-a-shell-profile");
        let rows = simple_page(config, &MemoryStore::default(), &Local::default());
        let store = MemoryStore::default();
        store.set("openrouter", "k").unwrap();
        let stored = simple_page(config, &store, &Local::default());
        std::env::remove_var("OPENROUTER_API_KEY");
        let key = row(&rows, "OpenRouter key").unwrap();
        assert_eq!(
            key.1,
            "none — Enter to add one (leo does not read $OPENROUTER_API_KEY)"
        );
        assert!(row(&rows, "writing").unwrap().1.starts_with('○'));
        assert_eq!(row(&stored, "OpenRouter key").unwrap().1, "stored");
        assert!(row(&stored, "writing").unwrap().1.starts_with('●'));
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
        let no_model = tempfile::tempdir().unwrap();
        let config = &format!(
            "[chat]\nchain = [\"ollama\"]\n[transcribe]\nchain = [\"parakeet\"]\n[providers.parakeet]\nmodel_path = '{}'\n",
            no_model.path().display()
        );
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

        assert!(speech_model.1.contains("670 MB"), "{speech_model:?}");

        let running = Local {
            ollama_running: true,
            ollama: Vec::new(),
            speech_ready: false,
        };
        let rows = simple_page(config, &MemoryStore::default(), &running);
        assert_eq!(
            row(&rows, "writing model").unwrap().2,
            "GetLocalModel(Chat)"
        );
        assert_eq!(
            row(&rows, "speech").unwrap().1,
            format!("○ {} (Parakeet)", choice::this_computer())
        );

        let ready = Local {
            ollama_running: false,
            ollama: Vec::new(),
            speech_ready: true,
        };
        let rows = simple_page(config, &MemoryStore::default(), &ready);
        assert!(
            row(&rows, "speech model").is_none(),
            "a speech model that is there needs no row: {rows:?}"
        );

        let pulled = Local {
            ollama_running: true,
            ollama: vec!["qwen3:8b".to_string(), "gemma3:4b".to_string()],
            speech_ready: true,
        };
        let rows = simple_page(config, &MemoryStore::default(), &pulled);
        let writing_model = row(&rows, "writing model").unwrap();
        assert_eq!(writing_model.1, "qwen3:8b (free)");
        assert_eq!(writing_model.2, "ChooseModel(Chat)");
    }

    #[test]
    fn a_chain_set_by_hand_is_shown_by_name_and_left_alone() {
        use super::*;
        let rows = simple_page(
            "[chat]\nchain = [\"mine\"]\n[transcribe]\nchain = []\n[providers.mine]\nkind = \"openai\"\nbase_url = \"https://example.com/v1\"\nkey_env = \"LEO_TEST_MINE_API_KEY\"\n",
            &MemoryStore::default(),
            &Local::default(),
        );
        assert_eq!(
            row(&rows, "writing").unwrap().1,
            "○ mine (from config.toml)"
        );
        assert!(row(&rows, "writing model").is_none());
        assert_eq!(row(&rows, "speech").unwrap().1, "○ nothing chosen");
    }

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
}
