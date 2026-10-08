use crate::config::provider::{ProviderConfig, ProviderKind};
use crate::config::Config;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    pub words: usize,
    pub at_once: usize,
}

impl Budget {
    pub const LOCAL: Budget = Budget {
        words: 3_000,
        at_once: 1,
    };
    pub const FREE: Budget = Budget {
        words: 4_000,
        at_once: 2,
    };
    pub const CLOUD: Budget = Budget {
        words: 10_000,
        at_once: 3,
    };
    pub const AGENT: Budget = Budget {
        words: 16_000,
        at_once: 3,
    };

    pub fn tighter(self, other: Budget) -> Budget {
        Budget {
            words: self.words.min(other.words),
            at_once: self.at_once.min(other.at_once),
        }
    }

    pub fn chars(self) -> usize {
        self.words * 6
    }
}

fn local(url: &str) -> bool {
    let url = url.to_ascii_lowercase();
    ["localhost", "127.0.0.1", "[::1]", "0.0.0.0"]
        .iter()
        .any(|host| url.contains(host))
}

pub fn budget_of(name: &str, provider: &ProviderConfig) -> Budget {
    let model = provider.model.as_deref().unwrap_or("").to_ascii_lowercase();
    match provider.kind {
        Some(ProviderKind::ClaudeCode | ProviderKind::Codex) => Budget::AGENT,
        _ if name == "ollama" || provider.base_url.as_deref().is_some_and(local) => Budget::LOCAL,
        _ if model.ends_with(":free") || model == "openrouter/free" => Budget::FREE,
        _ => Budget::CLOUD,
    }
}

pub fn writing_budget(config: &Config) -> Budget {
    config
        .chat
        .chain
        .iter()
        .filter_map(|name| {
            let provider = config
                .provider(name)
                .cloned()
                .or_else(|| Config::built_in_provider(name))?;
            Some(budget_of(name, &provider))
        })
        .reduce(Budget::tighter)
        .unwrap_or(Budget::FREE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(kind: ProviderKind, url: Option<&str>, model: Option<&str>) -> ProviderConfig {
        ProviderConfig {
            kind: Some(kind),
            base_url: url.map(str::to_string),
            model: model.map(str::to_string),
            ..ProviderConfig::default()
        }
    }

    #[test]
    fn signed_in_agents_and_paid_models_take_big_parts_and_local_ones_small() {
        let codex = provider(ProviderKind::Codex, None, Some("gpt-6.1-sol"));
        let paid = provider(
            ProviderKind::Openai,
            Some("https://api.anthropic.com/v1"),
            Some("claude-haiku-5-5"),
        );
        let free = provider(
            ProviderKind::Openai,
            Some("https://openrouter.ai/api/v1"),
            Some("qwen/qwen3.8-27b:free"),
        );
        let home = provider(
            ProviderKind::Openai,
            Some("http://127.0.0.1:11434/v1"),
            Some("qwen3:8b"),
        );
        assert_eq!(budget_of("codex", &codex), Budget::AGENT);
        assert_eq!(
            budget_of(
                "claude_code",
                &provider(ProviderKind::ClaudeCode, None, None)
            ),
            Budget::AGENT
        );
        assert_eq!(budget_of("anthropic", &paid), Budget::CLOUD);
        assert_eq!(budget_of("openrouter", &free), Budget::FREE);
        assert_eq!(budget_of("lmstudio", &home), Budget::LOCAL);
        assert_eq!(
            budget_of("ollama", &provider(ProviderKind::Openai, None, None)),
            Budget::LOCAL
        );
    }

    #[test]
    fn a_chain_writes_in_parts_its_weakest_member_can_take() {
        let mut config: Config = toml::from_str("").unwrap();
        config.chat.chain = vec!["codex".into()];
        assert_eq!(writing_budget(&config), Budget::AGENT);
        config.chat.chain = vec!["codex".into(), "ollama".into()];
        assert_eq!(writing_budget(&config), Budget::LOCAL);
        config.chat.chain = vec!["nothing-like-this".into()];
        assert_eq!(writing_budget(&config), Budget::FREE);
        config.chat.chain = vec![];
        assert_eq!(writing_budget(&config), Budget::FREE);
    }
}
