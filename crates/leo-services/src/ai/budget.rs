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

const AGENT_OUTPUT_TOKENS: u32 = 32_000;

fn kind_of(name: &str, provider: &ProviderConfig) -> Budget {
    let model = provider.model.as_deref().unwrap_or("").to_ascii_lowercase();
    match provider.kind {
        Some(ProviderKind::ClaudeCode | ProviderKind::Codex) => Budget::AGENT,
        _ if name == "ollama" || provider.base_url.as_deref().is_some_and(local) => Budget::LOCAL,
        _ if model.ends_with(":free") || model == "openrouter/free" => Budget::FREE,
        _ => Budget::CLOUD,
    }
}

pub fn budget_of(name: &str, provider: &ProviderConfig) -> Budget {
    let kind = kind_of(name, provider);
    let output = match provider.kind {
        Some(ProviderKind::ClaudeCode | ProviderKind::Codex) => AGENT_OUTPUT_TOKENS,
        _ => provider.max_tokens.unwrap_or(4_096),
    };
    let reads = note_tokens(window_of(name, provider)) as usize * 3 / 4;
    let writes = output as usize * 2;
    Budget {
        words: reads.min(writes).max(Budget::LOCAL.words.min(reads)),
        at_once: kind.at_once,
    }
}

pub const CHARS_PER_TOKEN: usize = 4;
const NOTES_SHARE: f64 = 0.45;
const PAID_NOTE_TOKENS: u32 = 50_000;
const PLAN_NOTE_TOKENS: u32 = 120_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub tokens: u32,
    pub paid: bool,
}

pub fn window_of(name: &str, provider: &ProviderConfig) -> Window {
    let model = provider.model.as_deref().unwrap_or("").to_ascii_lowercase();
    let url = provider
        .base_url
        .as_deref()
        .unwrap_or("")
        .to_ascii_lowercase();
    let (tokens, paid) = match provider.kind {
        Some(ProviderKind::ClaudeCode) => (200_000, false),
        Some(ProviderKind::Codex) => (256_000, false),
        _ if name == "ollama" || local(&url) => (8_192, false),
        _ if model.ends_with(":free") || model == "openrouter/free" => (32_000, false),
        _ if model.contains("gemini") => (1_000_000, true),
        _ if model.contains("claude") => (200_000, true),
        _ if model.contains("grok-4") => (256_000, true),
        _ if model.contains("gpt-4.1") => (1_000_000, true),
        _ if model.contains("gpt-5") || model.contains("gpt-6") => (400_000, true),
        _ => (128_000, true),
    };
    Window {
        tokens: provider.context.filter(|t| *t >= 2_048).unwrap_or(tokens),
        paid,
    }
}

pub fn note_tokens(window: Window) -> u32 {
    let share = (f64::from(window.tokens) * NOTES_SHARE) as u32;
    share.min(if window.paid {
        PAID_NOTE_TOKENS
    } else {
        PLAN_NOTE_TOKENS
    })
}

pub fn felix_room(config: &Config) -> usize {
    config
        .chat
        .chain
        .iter()
        .filter_map(|name| {
            let provider = config
                .provider(name)
                .cloned()
                .or_else(|| Config::built_in_provider(name))?;
            Some(note_tokens(window_of(name, &provider)) as usize * CHARS_PER_TOKEN)
        })
        .min()
        .unwrap_or(Budget::FREE.chars())
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
    fn parts_are_as_big_as_the_model_reads_and_its_notes_can_be_written() {
        let mut claude = provider(ProviderKind::ClaudeCode, None, Some("claude-sonnet-5-5"));
        assert_eq!(
            budget_of("claude_code", &claude),
            Budget {
                words: 64_000,
                at_once: 3
            }
        );
        claude.kind = Some(ProviderKind::Codex);
        assert_eq!(budget_of("codex", &claude).words, 64_000);
        let mut paid = provider(
            ProviderKind::Openai,
            Some("https://api.anthropic.com/v1"),
            Some("claude-haiku-5-5"),
        );
        paid.max_tokens = Some(32_000);
        assert_eq!(
            budget_of("anthropic", &paid),
            Budget {
                words: 37_500,
                at_once: 3
            }
        );
        paid.max_tokens = Some(4_096);
        assert_eq!(budget_of("anthropic", &paid).words, 8_192);
        let mut free = provider(
            ProviderKind::Openai,
            Some("https://openrouter.ai/api/v1"),
            Some("qwen/qwen3.8-27b:free"),
        );
        free.max_tokens = Some(8_192);
        assert_eq!(
            budget_of("openrouter", &free),
            Budget {
                words: 10_800,
                at_once: 2
            }
        );
        let home = provider(
            ProviderKind::Openai,
            Some("http://127.0.0.1:11434/v1"),
            Some("qwen3:8b"),
        );
        assert_eq!(
            budget_of("lmstudio", &home),
            Budget {
                words: 2_764,
                at_once: 1
            }
        );
    }

    #[test]
    fn felix_reads_as_much_as_the_model_holds_but_paid_answers_stay_affordable() {
        let room = |kind, url: Option<&str>, model: Option<&str>| {
            let p = provider(kind, url, model);
            note_tokens(window_of("x", &p)) as usize * CHARS_PER_TOKEN
        };
        assert_eq!(room(ProviderKind::ClaudeCode, None, None), 90_000 * 4);
        assert_eq!(
            room(ProviderKind::Codex, None, Some("gpt-6.1-sol")),
            115_200 * 4
        );
        assert_eq!(
            room(
                ProviderKind::Openai,
                Some("https://api.anthropic.com/v1"),
                Some("claude-sonnet-5-5")
            ),
            50_000 * 4
        );
        assert_eq!(
            room(
                ProviderKind::Openai,
                Some("https://generativelanguage.googleapis.com/v1beta/openai"),
                Some("gemini-3.1-pro-preview")
            ),
            50_000 * 4
        );
        assert_eq!(
            room(
                ProviderKind::Openai,
                Some("https://openrouter.ai/api/v1"),
                Some("qwen/qwen3.8-27b:free")
            ),
            14_400 * 4
        );
        assert_eq!(
            room(
                ProviderKind::Openai,
                Some("http://127.0.0.1:11434/v1"),
                Some("qwen3:8b")
            ),
            3_686 * 4
        );
        let mut told = provider(
            ProviderKind::Openai,
            Some("http://127.0.0.1:11434/v1"),
            Some("qwen3:8b"),
        );
        told.context = Some(32_768);
        assert_eq!(window_of("ollama", &told).tokens, 32_768);
        told.context = Some(10);
        assert_eq!(window_of("ollama", &told).tokens, 8_192);
    }

    #[test]
    fn felix_reads_what_the_weakest_model_in_the_chain_can_hold() {
        let mut config: Config = toml::from_str("").unwrap();
        config.chat.chain = vec!["claude_code".into()];
        assert_eq!(felix_room(&config), 360_000);
        config.chat.chain = vec!["claude_code".into(), "ollama".into()];
        assert_eq!(felix_room(&config), 3_686 * 4);
        config.chat.chain = vec![];
        assert_eq!(felix_room(&config), Budget::FREE.chars());
    }

    #[test]
    fn a_chain_writes_in_parts_its_weakest_member_can_take() {
        let mut config: Config = toml::from_str("").unwrap();
        config.chat.chain = vec!["codex".into()];
        assert_eq!(writing_budget(&config).words, 64_000);
        config.chat.chain = vec!["codex".into(), "ollama".into()];
        assert_eq!(
            writing_budget(&config),
            Budget {
                words: 2_764,
                at_once: 1
            }
        );
        config.chat.chain = vec!["nothing-like-this".into()];
        assert_eq!(writing_budget(&config), Budget::FREE);
        config.chat.chain = vec![];
        assert_eq!(writing_budget(&config), Budget::FREE);
    }
}
