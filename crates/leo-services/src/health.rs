//! What works on this machine, and what to run if it doesn't.
//!
//! greeting, and the pre-flight check before recording all agree — and so a
//! missing dependency is reported with the command that installs it rather than
//! as a failure after the user has already tried to use it.

use crate::config::provider::ProviderConfig;
use crate::config::secret::SecretStore;
use crate::config::Config;

/// Whether a capability is usable, and what to do when it is not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// Works right now.
    Ready,
    /// Not usable, with the one command that fixes it.
    Missing { fix: String },
    /// Usable, but worth mentioning.
    Warn { note: String },
}

impl State {
    pub fn is_ready(&self) -> bool {
        matches!(self, State::Ready)
    }
}

/// One checked thing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    /// What it is, in the user's words rather than the binary's name.
    pub what: String,
    /// What it is needed for, so the user can judge whether to care.
    pub needed_for: String,
    pub state: State,
    /// Extra detail: a version, a path, a model name.
    pub detail: Option<String>,
}

impl Check {
    pub(crate) fn ready(what: &str, needed_for: &str, detail: Option<String>) -> Self {
        Self {
            what: what.to_string(),
            needed_for: needed_for.to_string(),
            state: State::Ready,
            detail,
        }
    }

    pub(crate) fn missing(what: &str, needed_for: &str, fix: &str) -> Self {
        Self {
            what: what.to_string(),
            needed_for: needed_for.to_string(),
            state: State::Missing {
                fix: fix.to_string(),
            },
            detail: None,
        }
    }
}

pub use leo_core::paths::on_path;

/// Whether a local HTTP server is listening, used for Ollama and friends.
///
/// Resolves the address itself rather than going through `to_socket_addrs`,
/// which performs a DNS lookup with no timeout — a bogus hostname there can
/// block for minutes, and this runs on startup.
pub(crate) fn port_open(base_url: &str) -> bool {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
    use std::time::Duration;

    let trimmed = base_url
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    let authority = trimmed.split('/').next().unwrap_or(trimmed);

    // Split host and port, keeping bracketed IPv6 literals intact.
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) if !h.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => {
            (h, p.parse::<u16>().unwrap_or(0))
        }
        _ => (
            authority,
            if base_url.starts_with("https://") {
                443
            } else {
                80
            },
        ),
    };
    if port == 0 {
        return false;
    }

    let host = host.trim_start_matches('[').trim_end_matches(']');
    let ip: IpAddr = match host {
        "localhost" => IpAddr::V4(Ipv4Addr::LOCALHOST),
        other => match other.parse::<Ipv4Addr>() {
            Ok(v4) => IpAddr::V4(v4),
            Err(_) => match other.parse::<Ipv6Addr>() {
                Ok(v6) => IpAddr::V6(v6),
                // Not a literal address. Only loopback reaches this function,
                // so anything else is not something to probe.
                Err(_) => return false,
            },
        },
    };

    TcpStream::connect_timeout(&SocketAddr::new(ip, port), Duration::from_millis(300)).is_ok()
}

/// Whether the microphone is actually heard, by recording a fraction of a
/// second and looking at the samples.
///
/// Worth the half second because on macOS a denied microphone permission is not
/// an error: `rec` succeeds and every sample is zero. Nothing downstream can
/// tell that from a quiet room, so a user gets a transcript of invented text
/// instead of a reason. Probing also makes macOS raise its permission prompt,
/// which is the fix.
pub fn microphone_device() -> Check {
    match crate::session::mic::microphone_name() {
        Some(name) => Check::ready("microphone device", "recording audio", Some(name)),
        None => Check::missing(
            "microphone device",
            "recording audio",
            "plug in a microphone, or pick an input in your sound settings",
        ),
    }
}

pub fn microphone() -> Check {
    match crate::session::mic::listen_for(0.4).map(|s| crate::session::wav::peak(&s)) {
        Ok(peak) if crate::ai::live::is_silent(peak) => Check {
            what: "microphone".to_string(),
            needed_for: "recording audio".to_string(),
            state: State::Missing {
                fix: "System Settings > Privacy & Security > Microphone — allow \
                      Leo, then try recording again"
                    .to_string(),
            },
            detail: Some("recorded silence; the mic is not being heard".to_string()),
        },
        Ok(peak) => Check::ready(
            "microphone",
            "recording audio",
            Some(format!("hearing input (peak {peak:.3})")),
        ),
        Err(e) => Check {
            what: "microphone".to_string(),
            needed_for: "recording audio".to_string(),
            state: State::Warn {
                note: "could not be tested".to_string(),
            },
            detail: Some(e.to_string()),
        },
    }
}

/// Everything needed to record speech into a note.
///
/// Separate from the full report so `listen` can check it before recording
/// rather than failing partway through.
/// `microphone` is only meaningful when the audio comes from a microphone:
/// `record screen` captures system output through a loopback device, and the replay
/// hook reads a file, so probing the mic for either would block a recording
/// that would have worked.
pub fn recording(config: &Config, store: &dyn SecretStore, uses_microphone: bool) -> Vec<Check> {
    let mut checks = Vec::new();

    if uses_microphone {
        let device = microphone_device();
        let found = device.state.is_ready();
        checks.push(device);
        if found {
            checks.push(microphone());
        }
    }
    checks.push(chain_check(config, Chain::Transcribe, store));
    checks
}

/// Which chain a check refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chain {
    Chat,
    Transcribe,
}

impl Chain {
    fn label(self) -> &'static str {
        match self {
            Chain::Chat => "a chat model",
            Chain::Transcribe => "a transcription model",
        }
    }

    fn needed_for(self) -> &'static str {
        match self {
            Chain::Chat => "ask, and structuring recordings into notes",
            Chain::Transcribe => "turning speech into text",
        }
    }
}

/// Whether at least one provider in a chain is usable, and which one leo would
/// reach for.
pub(crate) fn chain_check(config: &Config, chain: Chain, store: &dyn SecretStore) -> Check {
    let names = match chain {
        Chain::Chat => &config.chat.chain,
        Chain::Transcribe => &config.transcribe.chain,
    };

    let (usable, fix) = match chain {
        Chain::Chat => {
            let writing = writing(config, store, None);
            let ready = writing.state == WritingState::Ready;
            (
                writing.provider.filter(|_| ready),
                writing.fix.unwrap_or_default(),
            )
        }
        Chain::Transcribe => (
            names
                .iter()
                .find(|name| provider_usable(config, name, store))
                .cloned(),
            "leo update   (downloads the speech model; free, private)\n\
             or: Settings in leo, choose OpenAI, Gemini or xAI, add its key"
                .to_string(),
        ),
    };

    match usable {
        Some(first) => Check::ready(
            chain.label(),
            chain.needed_for(),
            Some(format!("using {first}")),
        ),
        None => Check {
            what: chain.label().to_string(),
            needed_for: chain.needed_for().to_string(),
            state: State::Missing { fix },
            detail: if names.is_empty() {
                Some("no providers configured".to_string())
            } else {
                Some(format!("tried {}", names.join(", ")))
            },
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WritingState {
    Ready,
    NotInstalled,
    NotRunning,
    NoModel,
    NotChosen,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Writing {
    pub provider: Option<String>,
    pub state: WritingState,
    pub fix: Option<String>,
}

pub fn writing(
    config: &Config,
    store: &dyn SecretStore,
    models: Option<&crate::config::choice::Local>,
) -> Writing {
    let names = &config.chat.chain;
    let usable = names
        .iter()
        .find(|name| provider_usable(config, name, store));
    let provider = usable.or(names.first()).cloned();
    let pc = provider.as_deref().and_then(|name| config.provider(name));
    let agent = pc.and_then(crate::ai::provider::agent_cli::Agent::of);
    let state = match (pc, usable) {
        (None, _) => WritingState::NotChosen,
        (Some(_), Some(name)) => match models.filter(|_| name == "ollama") {
            Some(local) if !local.ollama_running => WritingState::NotRunning,
            Some(local) if local.ollama.is_empty() => WritingState::NoModel,
            _ => WritingState::Ready,
        },
        (Some(_), None) if agent.is_some() => WritingState::NotInstalled,
        (Some(pc), None)
            if pc.key_env.is_none() && pc.base_url.as_deref().is_some_and(is_loopback) =>
        {
            WritingState::NotRunning
        }
        (Some(_), None) => WritingState::NotChosen,
    };
    let other = "or: Settings in leo, choose another way to write";
    let fix = match state {
        WritingState::Ready => None,
        WritingState::NotInstalled => agent.map(|agent| format!("{}\n{other}", agent.install())),
        WritingState::NotRunning => Some(format!(
            "open the Ollama app, or run: ollama serve\n{other}"
        )),
        WritingState::NoModel => Some(format!("ollama pull qwen3:8b\n{other}")),
        WritingState::NotChosen => Some(
            "brew install ollama && ollama pull qwen3:8b   (free, private)\n\
             or: Settings in leo, choose Claude Code or Codex to use the plan you have"
                .to_string(),
        ),
    };
    Writing {
        provider,
        state,
        fix,
    }
}

/// Whether one named provider could serve a request right now.
///
/// Deliberately cheap and offline: a key check, a PATH check, or a port check.
/// Never a real request, so this stays fast enough to run on startup.
pub fn provider_usable(config: &Config, name: &str, store: &dyn SecretStore) -> bool {
    let Some(provider) = config.provider(name) else {
        return false;
    };
    let _: &ProviderConfig = provider;

    if provider.kind == Some(crate::config::provider::ProviderKind::Parakeet) {
        return crate::speech::library::library().is_ok_and(|l| l.selected().is_ok());
    }
    if let Some(agent) = crate::ai::provider::agent_cli::Agent::of(provider) {
        let bin = provider.bin.as_deref().unwrap_or(agent.program());
        return crate::ai::provider::agent_cli::locate(bin).is_some();
    }
    // A key-based provider: only the key leo stored, and `has` rather than a
    // read so this cannot cost a permission prompt.
    if provider.key_env.is_some() {
        return store.has(provider.account(name));
    }

    // A local server with no key: is anything listening?
    match &provider.base_url {
        Some(url) if is_loopback(url) => port_open(url),
        // Remote and keyless: assume reachable; a request will say otherwise.
        Some(_) => true,
        None => false,
    }
}

fn is_loopback(url: &str) -> bool {
    url.contains("localhost") || url.contains("127.0.0.1") || url.contains("[::1]")
}

/// Where credentials are kept, and whether the file is readable by anyone else.
///
/// Worth reporting because the choice of a file over the keychain trades
/// encryption at rest for filesystem permissions — so those permissions are the
/// protection, and an unchecked assumption is not one.
pub(crate) fn credentials_check() -> Check {
    use crate::config::file_store::FileStore;

    if std::env::var("LEO_USE_KEYCHAIN").is_ok_and(|v| v != "0" && !v.is_empty()) {
        return Check::ready(
            "credentials",
            "storing API keys",
            Some("OS keychain (LEO_USE_KEYCHAIN is set)".to_string()),
        );
    }

    match FileStore::new() {
        // Tighten it rather than only complaining: leo created this file, and a
        // loose mode is a bug in an older leo, not a decision the user made.
        Ok(file) if !file.is_private() && !file.make_private() => Check {
            what: "credentials".to_string(),
            needed_for: "storing API keys".to_string(),
            state: State::Warn {
                note: format!(
                    "{} is readable by other accounts on this machine",
                    file.path().display()
                ),
            },
            detail: Some(format!("chmod 600 {}", file.path().display())),
        },
        Ok(file) => Check::ready(
            "credentials",
            "storing API keys",
            Some(format!("{} — only you can read it", file.path().display())),
        ),
        Err(e) => Check {
            what: "credentials".to_string(),
            needed_for: "storing API keys".to_string(),
            state: State::Warn {
                note: format!("no location for a credentials file: {e}"),
            },
            detail: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::provider::ProviderConfig;
    use crate::config::secret::MemoryStore;

    /// Never the real keychain: an unsigned test binary reading it can block on
    /// a permission dialog with nobody to answer it.
    fn store() -> MemoryStore {
        MemoryStore::default()
    }

    fn config_with(providers: Vec<(&str, ProviderConfig)>, chat: Vec<&str>) -> Config {
        Config {
            providers: providers
                .into_iter()
                .map(|(n, p)| (n.to_string(), p))
                .collect(),
            chat: crate::config::provider::TaskChain {
                chain: chat.into_iter().map(String::from).collect(),
            },
            transcribe: crate::config::provider::TaskChain { chain: vec![] },
        }
    }

    #[test]
    fn a_binary_on_path_is_found_and_a_missing_one_is_not() {
        // `sh` exists on every unix box this targets.
        #[cfg(unix)]
        assert!(on_path("sh"));
        assert!(!on_path("leo-definitely-not-a-real-binary"));
    }

    fn cli(bin: &str) -> ProviderConfig {
        ProviderConfig {
            kind: Some(crate::config::provider::ProviderKind::ClaudeCode),
            bin: Some(bin.to_string()),
            ..Default::default()
        }
    }

    fn installed() -> ProviderConfig {
        cli(if cfg!(windows) { "cmd.exe" } else { "sh" })
    }

    fn missing() -> ProviderConfig {
        cli("leo-definitely-not-a-real-binary")
    }

    #[test]
    fn a_named_program_must_be_installed() {
        let config = config_with(
            vec![("present", installed()), ("absent", missing())],
            vec!["present"],
        );
        assert!(provider_usable(&config, "present", &store()));
        assert!(!provider_usable(&config, "absent", &store()));
    }

    #[test]
    fn an_unknown_provider_is_never_usable() {
        let config = config_with(vec![], vec![]);
        assert!(!provider_usable(&config, "nope", &store()));
    }

    /// An empty chain is a real gap, not a pass.
    #[test]
    fn a_chain_with_no_usable_provider_is_missing() {
        let config = config_with(vec![], vec![]);
        let check = chain_check(&config, Chain::Chat, &store());
        assert!(matches!(check.state, State::Missing { .. }));
        assert_eq!(check.detail.as_deref(), Some("no providers configured"));
    }

    /// The chain reports which provider would actually serve the request, since
    /// "chat works" is less useful than "chat works, using ollama".
    #[test]
    fn a_usable_chain_names_the_provider_it_would_use() {
        let config = config_with(vec![("localbin", installed())], vec!["localbin"]);
        let check = chain_check(&config, Chain::Chat, &store());
        assert!(check.state.is_ready());
        assert_eq!(check.detail.as_deref(), Some("using localbin"));
    }

    /// A chain skips ahead: an unusable first provider must not mask a working
    /// second one, because that is exactly how the fallback behaves at runtime.
    #[test]
    fn a_chain_falls_past_an_unusable_provider() {
        let config = config_with(
            vec![("broken", missing()), ("working", installed())],
            vec!["broken", "working"],
        );
        let check = chain_check(&config, Chain::Chat, &store());
        assert!(check.state.is_ready());
        assert_eq!(check.detail.as_deref(), Some("using working"));
    }

    fn server(url: &str) -> ProviderConfig {
        ProviderConfig {
            kind: Some(crate::config::provider::ProviderKind::Openai),
            base_url: Some(url.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn writing_says_what_is_missing_for_each_kind_of_provider() {
        for (providers, chain, state) in [
            (vec![], vec![], WritingState::NotChosen),
            (vec![], vec!["gone"], WritingState::NotChosen),
            (
                vec![("cli", missing())],
                vec!["cli"],
                WritingState::NotInstalled,
            ),
            (vec![("cli", installed())], vec!["cli"], WritingState::Ready),
            (
                vec![("local", server("http://127.0.0.1:1/v1"))],
                vec!["local"],
                WritingState::NotRunning,
            ),
            (
                vec![("remote", server("https://example.invalid/v1"))],
                vec!["remote"],
                WritingState::Ready,
            ),
        ] {
            let config = config_with(providers, chain);
            let local = crate::config::choice::Local::default();
            for models in [None, Some(&local)] {
                let writing = writing(&config, &store(), models);
                assert_eq!(writing.state, state, "{writing:?}");
                assert_eq!(writing.fix.is_none(), state == WritingState::Ready);
                assert_eq!(
                    chain_check(&config, Chain::Chat, &store()).state.is_ready(),
                    state == WritingState::Ready
                );
            }
        }
        assert_eq!(serde_json::json!(WritingState::NoModel), "no_model");
    }

    #[test]
    fn ollama_without_models_is_no_model_only_when_its_models_were_listed() {
        use std::io::Read;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "http://127.0.0.1:{}/v1",
            listener.local_addr().unwrap().port()
        );
        let config = config_with(vec![("ollama", server(&url))], vec!["ollama"]);
        let local = |running: bool, models: &[&str]| crate::config::choice::Local {
            ollama_running: running,
            ollama: models.iter().map(|m| m.to_string()).collect(),
            speech_ready: false,
        };

        assert_eq!(writing(&config, &store(), None).state, WritingState::Ready);
        listener.set_nonblocking(true).unwrap();
        while let Ok((mut stream, _)) = listener.accept() {
            stream.set_nonblocking(false).unwrap();
            let mut sent = Vec::new();
            stream.read_to_end(&mut sent).unwrap();
            assert!(sent.is_empty(), "the cheap path sent a request");
        }

        for (models, state) in [
            (local(true, &[]), WritingState::NoModel),
            (local(false, &[]), WritingState::NotRunning),
            (local(true, &["qwen3:8b"]), WritingState::Ready),
        ] {
            assert_eq!(writing(&config, &store(), Some(&models)).state, state);
        }
    }

    /// Screen capture does not go through the microphone, so a silent mic must
    /// not block it. Nor must a replayed file.
    #[test]
    fn screen_capture_is_not_blocked_by_the_microphone() {
        let config = config_with(vec![], vec![]);
        let without = recording(&config, &store(), false);
        assert!(
            !without.iter().any(|c| c.what == "microphone"),
            "screen capture probed the microphone"
        );
    }

    /// A port check must not hang, panic, or resolve DNS on nonsense input.
    /// It ran for fifteen minutes before it stopped going through
    /// `to_socket_addrs`, which does an unbounded lookup.
    #[test]
    fn a_closed_or_malformed_url_is_simply_not_open() {
        let started = std::time::Instant::now();
        assert!(!port_open("http://127.0.0.1:1"));
        assert!(!port_open("not a url at all"));
        assert!(!port_open(
            "http://a-hostname-that-does-not-resolve.invalid:80"
        ));
        assert!(!port_open(""));
        assert!(!port_open("http://127.0.0.1:0"));
        // Generous, but far below what a DNS timeout costs.
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "took {:?} — is it resolving DNS?",
            started.elapsed()
        );
    }

    #[test]
    fn a_loopback_port_check_parses_host_and_port() {
        // Nothing is listening on port 1, but the address must parse and the
        // attempt must be made rather than short-circuiting on a parse failure.
        assert!(!port_open("http://localhost:1"));
        assert!(!port_open("http://[::1]:1/v1"));
        assert!(!port_open("http://127.0.0.1:1/v1/chat"));
    }

    #[test]
    fn loopback_urls_are_recognized() {
        assert!(is_loopback("http://localhost:11434/v1"));
        assert!(is_loopback("http://127.0.0.1:1234/v1"));
        assert!(!is_loopback("https://openrouter.ai/api/v1"));
    }

    /// The recording pre-flight must cover everything a recording needs, so the
    /// user learns about all of it before speaking rather than one gap at a time.
    #[test]
    fn the_recording_preflight_needs_audio_and_speech_but_not_writing() {
        let config = config_with(vec![], vec![]);
        let checks = recording(&config, &store(), false);
        let subjects: Vec<&str> = checks.iter().map(|c| c.what.as_str()).collect();
        assert!(!subjects.contains(&"sox"), "{subjects:?}");
        assert!(subjects.contains(&"a transcription model"), "{subjects:?}");
        assert!(!subjects.contains(&"a chat model"), "{subjects:?}");
    }
}
