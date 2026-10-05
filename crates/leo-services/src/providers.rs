//! Everything that touches AI providers from outside the chains: describing a
//! credential, storing and removing keys, testing one provider, and opening the
//! config file. The CLI and the TUI's profile screen both call these, so there
//! is one implementation of each.

use anyhow::Result;
use colored::Colorize;

use crate::ai;
use crate::config::{
    self,
    secret::{redact, resolve, SecretStore},
    Config,
};

/// What a model command asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelAction {
    Login { name: String },
    Logout { name: String },
}

/// What a config command asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigAction {
    Edit,
}

/// Build the single transcription provider named by `name`, regardless of
/// whether it appears in the `[transcribe]` chain — `leo model test <name>`
/// must work for a provider a user is still setting up.
pub fn build_one_transcriber(
    name: &str,
    pc: &config::provider::ProviderConfig,
    store: &dyn SecretStore,
) -> Option<Box<dyn ai::provider::TranscribeProvider>> {
    use config::provider::ProviderKind;
    match pc.kind {
        Some(ProviderKind::Transcriptions) => {
            Some(Box::new(ai::provider::transcriptions::Transcriptions::new(
                name.to_string(),
                pc,
                resolve(pc.account(name), store),
            )))
        }
        Some(ProviderKind::ChatAudio) => Some(Box::new(
            ai::provider::chat_audio::ChatAudioTranscribe::new(
                name.to_string(),
                pc,
                resolve(pc.account(name), store),
            ),
        )),
        Some(ProviderKind::WhisperCpp) => Some(Box::new(
            ai::provider::whisper_cpp::WhisperCppTranscribe::new(name.to_string(), pc),
        )),
        Some(ProviderKind::Parakeet) => Some(Box::new(
            ai::provider::parakeet::ParakeetTranscribe::new(name.to_string(), pc),
        )),
        _ => None,
    }
}

/// Send one minimal request to a provider and report what happened.
///
/// Returns the report rather than printing it, so the CLI can print it and the
/// provider screen can show it in its status line. Transcription providers are
/// only checked for reachability: exercising one needs audio, and a test that
/// records from the microphone is not a test anyone wants to run twice.
pub fn test_provider(name: &str) -> Result<String> {
    let cfg = Config::load();
    let store = config::secret::default_store();
    let Some(pc) = cfg.provider(name) else {
        anyhow::bail!("no provider named '{name}' in your config");
    };

    let started = std::time::Instant::now();
    if let Some(p) = ai::provider::build_one_chat(name, pc, &store) {
        if !p.available() {
            anyhow::bail!("{}", p.unavailable_reason());
        }
        let reply = p
            .complete(&ai::provider::ChatRequest {
                system: None,
                prompt: "Reply with the single word: ok".to_string(),
                temperature: 0.0,
                max_tokens: 16,
            })
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let reply = reply.trim().replace('\n', " ");
        // A reasoning model can answer at length; one line is enough here.
        let reply: String = reply.chars().take(60).collect();
        return Ok(format!(
            "{name} responded in {:?}: {reply}",
            started.elapsed()
        ));
    }
    match pc.kind {
        Some(_) => match build_one_transcriber(name, pc, &store) {
            Some(p) if p.available() => Ok(format!("{name} is reachable")),
            Some(p) => anyhow::bail!("{}", p.unavailable_reason()),
            None => anyhow::bail!("provider '{name}' could not be built"),
        },
        None => anyhow::bail!("provider '{name}' has no `kind`"),
    }
}

/// Providers in a chain that need a key and have none, in chain order.
pub fn providers_missing_keys(cfg: &Config, store: &dyn SecretStore) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for name in cfg.chat.chain.iter().chain(&cfg.transcribe.chain) {
        let Some(pc) = cfg.provider(name) else {
            continue;
        };
        if pc.key_env.is_none() {
            continue;
        }
        let account = pc.account(name).to_string();
        if !store.has(&account) && !out.contains(&account) {
            out.push(account);
        }
    }
    out
}

pub fn model(command: ModelAction) -> Result<()> {
    let cfg = Config::load();
    let store = config::secret::default_store();

    match command {
        ModelAction::Login { name } => {
            let name = account_for(&cfg, &name);
            if cfg.provider(&name).is_none() {
                println!(
                    "  {} no [providers.{name}] block in your config — storing the key anyway.",
                    "note".yellow()
                );
                println!("  Type :settings in leo to add it, or run `leo doctor` to see what is configured.");
            }
            let key_env = cfg.provider(&name).and_then(|p| p.key_env.clone());

            // Offer to import an existing .env value rather than making the
            // user paste it again.
            if let Some(var) = key_env.as_deref() {
                if let Ok(existing) = std::env::var(var) {
                    if !existing.trim().is_empty() {
                        println!("  Found {var} in your environment ({}).", redact(&existing));
                        print!("  Store it for you? [Y/n] ");
                        use std::io::Write;
                        std::io::stdout().flush().ok();
                        let mut answer = String::new();
                        std::io::stdin().read_line(&mut answer)?;
                        if !answer.trim().eq_ignore_ascii_case("n") {
                            store.set(&name, existing.trim())?;
                            println!("  {} stored for {name}.", "ok".green());
                            println!(
                                "  You can now remove this line from your .env:\n    {var}=..."
                            );
                            return Ok(());
                        }
                    }
                }
            }

            // Read with echo disabled: never on screen, never in shell history,
            // never an argv value visible to other processes.
            let secret = rpassword::prompt_password(format!("  API key for {name}: "))?;
            if secret.trim().is_empty() {
                anyhow::bail!("no key entered");
            }
            store.set(&name, secret.trim())?;
            println!("  {} stored for {name}.", "ok".green());
            Ok(())
        }

        ModelAction::Logout { name } => {
            store.delete(&account_for(&cfg, &name))?;
            Ok(())
        }
    }
}

pub fn get_local_model(task: crate::config::edit::Task) -> Result<String> {
    use crate::config::choice;
    use crate::config::edit::Task;
    match task {
        Task::Chat => {
            let status = std::process::Command::new("ollama")
                .args(["pull", choice::OLLAMA_STARTER])
                .status()
                .map_err(|e| anyhow::anyhow!("could not run ollama: {e}"))?;
            if !status.success() {
                anyhow::bail!("ollama pull {} failed", choice::OLLAMA_STARTER);
            }
            Ok(choice::OLLAMA_STARTER.to_string())
        }
        Task::Transcribe => Ok(download_speech_model()?.display().to_string()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelState {
    Ready,
    Missing,
    Damaged,
}

pub fn speech_model_dir() -> std::path::PathBuf {
    crate::ai::provider::parakeet::default_dir()
}

pub fn model_state(path: &std::path::Path, sha256: &str) -> ModelState {
    if !path.is_file() {
        return ModelState::Missing;
    }
    match sha256_of(path) {
        Ok(actual) if actual.eq_ignore_ascii_case(sha256) => ModelState::Ready,
        _ => ModelState::Damaged,
    }
}

pub fn files_state(dir: &std::path::Path, manifest: &[(String, String)]) -> ModelState {
    let states: Vec<ModelState> = manifest
        .iter()
        .map(|(name, sha)| model_state(&dir.join(name), sha))
        .collect();
    if states.contains(&ModelState::Damaged) {
        ModelState::Damaged
    } else if states.contains(&ModelState::Missing) {
        ModelState::Missing
    } else {
        ModelState::Ready
    }
}

pub fn speech_model_state() -> ModelState {
    files_state(
        &speech_model_dir(),
        &crate::ai::provider::parakeet::manifest(),
    )
}

pub fn sha256_of(path: &std::path::Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

pub fn download_speech_model() -> Result<std::path::PathBuf> {
    fetch_speech_model(false)
}

pub fn download_speech_model_quietly() -> Result<std::path::PathBuf> {
    fetch_speech_model(true)
}

fn fetch_speech_model(quiet: bool) -> Result<std::path::PathBuf> {
    let base = std::env::var("LEO_INSTALL_MODEL_URL")
        .unwrap_or_else(|_| crate::ai::provider::parakeet::MODEL_URL.to_string());
    let dir = speech_model_dir();
    remove_old_models();
    for (name, sha) in crate::ai::provider::parakeet::manifest() {
        if model_state(&dir.join(&name), &sha) == ModelState::Ready {
            continue;
        }
        if !quiet {
            println!("  {name}");
        }
        download_verified(&format!("{base}/{name}"), &sha, &dir, &name, quiet)?;
    }
    Ok(dir)
}

pub fn speech_model_wanted(cfg: &Config) -> bool {
    if std::env::var_os("LEO_INSTALL_NO_MODEL").is_some() {
        return false;
    }
    let uses_it = cfg.transcribe.chain.iter().any(|name| {
        cfg.provider(name).and_then(|p| p.kind)
            == Some(crate::config::provider::ProviderKind::Parakeet)
    });
    uses_it
        && !crate::ai::provider::parakeet::present(&crate::ai::provider::parakeet::default_dir())
}

pub fn remove_old_models() {
    let _ = std::fs::remove_file(crate::config::choice::models_dir().join("ggml-base.en.bin"));
}

pub fn download_verified(
    url: &str,
    sha256: &str,
    dir: &std::path::Path,
    name: &str,
    quiet: bool,
) -> Result<std::path::PathBuf> {
    std::fs::create_dir_all(dir)?;
    let target = dir.join(name);
    let partial = dir.join(format!("{name}.part"));
    let mut curl = std::process::Command::new("curl");
    curl.args(["-L", "--fail", "-o"]).arg(&partial).arg(url);
    if quiet {
        curl.arg("-sS")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
    } else {
        curl.arg("--progress-bar");
    }
    let status = curl
        .status()
        .map_err(|e| anyhow::anyhow!("could not run curl: {e}"))?;
    if !status.success() {
        let _ = std::fs::remove_file(&partial);
        anyhow::bail!("the download failed; try again when online");
    }
    let actual = sha256_of(&partial)?;
    if !actual.eq_ignore_ascii_case(sha256) {
        let _ = std::fs::remove_file(&partial);
        anyhow::bail!("the download was damaged (checksum mismatch); try again");
    }
    std::fs::rename(&partial, &target)?;
    Ok(target)
}

pub fn account_for(cfg: &Config, name: &str) -> String {
    cfg.provider(name)
        .map(|pc| pc.account(name).to_string())
        .unwrap_or_else(|| name.to_string())
}

pub fn config_file(command: ConfigAction) -> Result<()> {
    match command {
        ConfigAction::Edit => {
            let (path, created) = Config::ensure_exists()?;
            if created {
                println!("  Created {}", path.display());
            }
            leo_core::editor::open(&path)?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file_url(path: &std::path::Path) -> String {
        format!("file://{}", path.display())
    }

    #[test]
    fn a_download_is_kept_only_when_its_checksum_matches() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.bin");
        std::fs::write(&source, b"pretend model").unwrap();
        let good = sha256_of(&source).unwrap();
        assert_eq!(good.len(), 64);

        let models = dir.path().join("models");
        let saved =
            download_verified(&file_url(&source), &good, &models, "ggml-x.bin", true).unwrap();
        assert_eq!(std::fs::read(&saved).unwrap(), b"pretend model");
        assert!(!models.join("ggml-x.bin.part").exists());

        let err = download_verified(
            &file_url(&source),
            &"0".repeat(64),
            &models,
            "ggml-y.bin",
            true,
        )
        .unwrap_err();
        assert!(err.to_string().contains("damaged"), "{err}");
        assert!(!models.join("ggml-y.bin").exists());
        assert!(!models.join("ggml-y.bin.part").exists());

        let err = download_verified(
            &file_url(&dir.path().join("missing.bin")),
            &good,
            &models,
            "ggml-z.bin",
            true,
        )
        .unwrap_err();
        assert!(err.to_string().contains("failed"), "{err}");
        assert!(!models.join("ggml-z.bin.part").exists());
    }

    #[test]
    fn a_model_is_ready_only_when_its_checksum_matches() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ggml-base.en.bin");
        assert_eq!(model_state(&path, "00"), ModelState::Missing);
        std::fs::write(&path, b"whole model").unwrap();
        let good = sha256_of(&path).unwrap();
        assert_eq!(model_state(&path, &good), ModelState::Ready);
        assert_eq!(model_state(&path, &good.to_uppercase()), ModelState::Ready);
        std::fs::write(&path, b"whole mo").unwrap();
        assert_eq!(model_state(&path, &good), ModelState::Damaged);
    }

    #[test]
    fn a_model_of_several_files_is_ready_only_when_every_one_is() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.onnx");
        let b = dir.path().join("b.txt");
        std::fs::write(&a, b"aaa").unwrap();
        std::fs::write(&b, b"bbb").unwrap();
        let manifest = vec![
            ("a.onnx".to_string(), sha256_of(&a).unwrap()),
            ("b.txt".to_string(), sha256_of(&b).unwrap()),
        ];
        assert_eq!(files_state(dir.path(), &manifest), ModelState::Ready);
        std::fs::remove_file(&b).unwrap();
        assert_eq!(files_state(dir.path(), &manifest), ModelState::Missing);
        std::fs::write(&a, b"aa").unwrap();
        assert_eq!(files_state(dir.path(), &manifest), ModelState::Damaged);
    }
    use crate::config::secret::MemoryStore;
    use std::sync::Mutex;

    /// Env vars are process-global; serialize the tests that mutate them.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// need one, and have none.
    #[test]
    fn doctor_offers_keys_only_where_one_is_missing() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("LEO_TEST_SETUP_A");
        std::env::remove_var("LEO_TEST_SETUP_B");
        let cfg = Config::parse(
            r#"
[chat]
chain = ["local", "cloud_a", "cloud_b"]
[transcribe]
chain = []
[providers.local]
kind = "openai"
base_url = "http://localhost:11434/v1"
[providers.cloud_a]
kind = "openai"
base_url = "https://a.example/v1"
key_env = "LEO_TEST_SETUP_A"
[providers.cloud_b]
kind = "openai"
base_url = "https://b.example/v1"
key_env = "LEO_TEST_SETUP_B"
[providers.unused]
kind = "openai"
base_url = "https://c.example/v1"
key_env = "LEO_TEST_SETUP_C"
"#,
        )
        .unwrap();
        let store = MemoryStore::default();
        store.set("cloud_b", "k").unwrap();
        assert_eq!(
            providers_missing_keys(&cfg, &store),
            vec!["cloud_a".to_string()]
        );
    }
}
