use anyhow::Result;

use crate::ai::provider::agent_cli::{Agent, AgentCli, ClaudeSession};
use crate::ai::provider::codex_app::{tool_specs, CodexThread, CodexTurn};
use crate::ai::provider::{ChatProvider, Sink};
use crate::ai::spend::{answered, Answered};
use crate::config::provider::ProviderKind;
use crate::config::secret::SecretStore;
use crate::config::Config;

#[derive(Debug, Clone, PartialEq)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub schema: serde_json::Value,
}

pub struct Turn<'a> {
    pub tail: &'a str,
    pub max_tokens: u32,
    pub most_calls: usize,
    pub sink: Sink<'a>,
    pub restart: &'a mut dyn FnMut(),
    pub call: &'a mut dyn FnMut(&str, &serde_json::Value) -> String,
}

pub trait Session: Send {
    fn native(&self) -> bool;
    fn say(&mut self, text: &str, turn: Turn<'_>) -> Result<(String, Option<Answered>)>;
}

pub struct Systems<'a> {
    pub native: &'a str,
    pub text: &'a str,
}

struct ClaudeTalk {
    session: ClaudeSession,
    provider: String,
    config: Config,
}

impl Session for ClaudeTalk {
    fn native(&self) -> bool {
        false
    }

    fn say(&mut self, text: &str, turn: Turn<'_>) -> Result<(String, Option<Answered>)> {
        let message = if turn.tail.is_empty() {
            text.to_string()
        } else {
            format!("{text}\n\n{}", turn.tail)
        };
        let (answer, spent) = self
            .session
            .say(&message, turn.sink)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        Ok((answer, Some(answered(&self.config, &self.provider, spent))))
    }
}

struct CodexTalk {
    thread: CodexThread,
    provider: String,
    config: Config,
}

impl Session for CodexTalk {
    fn native(&self) -> bool {
        true
    }

    fn say(&mut self, text: &str, turn: Turn<'_>) -> Result<(String, Option<Answered>)> {
        let (answer, spent) = self
            .thread
            .say(
                text,
                CodexTurn {
                    sink: turn.sink,
                    restart: turn.restart,
                    call: turn.call,
                },
            )
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        Ok((answer, Some(answered(&self.config, &self.provider, spent))))
    }
}

pub fn open(
    config: &Config,
    store: &dyn SecretStore,
    systems: &Systems,
    tools: &[ToolDef],
) -> Option<Box<dyn Session>> {
    for name in &config.chat.chain {
        let Some(provider) = config
            .provider(name)
            .cloned()
            .or_else(|| Config::built_in_provider(name))
        else {
            continue;
        };
        match provider.kind {
            Some(ProviderKind::ClaudeCode) => {
                let agent = AgentCli::new(name.clone(), Agent::ClaudeCode, &provider);
                if !agent.available() {
                    continue;
                }
                let session = agent.claude_session(systems.text).ok()?;
                return Some(Box::new(ClaudeTalk {
                    session,
                    provider: name.clone(),
                    config: config.clone(),
                }));
            }
            Some(ProviderKind::Codex) => {
                let agent = AgentCli::new(name.clone(), Agent::Codex, &provider);
                if !agent.available() {
                    continue;
                }
                let specs: Vec<(String, String, serde_json::Value)> = tools
                    .iter()
                    .map(|t| (t.name.clone(), t.description.clone(), t.schema.clone()))
                    .collect();
                let thread =
                    CodexThread::start(&agent, systems.native, &tool_specs(&specs)).ok()?;
                return Some(Box::new(CodexTalk {
                    thread,
                    provider: name.clone(),
                    config: config.clone(),
                }));
            }
            _ => {
                let usable = crate::ai::provider::build_one_chat(name, &provider, store)
                    .is_some_and(|p| p.available());
                if usable {
                    return None;
                }
            }
        }
    }
    None
}
