use anyhow::Result;

use serde_json::{json, Value};

use crate::ai::provider::agent_cli::{Agent, AgentCli, ClaudeSession};
use crate::ai::provider::anthropic::{self, Anthropic};
use crate::ai::provider::codex_app::{tool_specs, CodexThread, CodexTurn};
use crate::ai::provider::openai::{self, OpenAiChat};
use crate::ai::provider::Spent;
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

enum Wire {
    OpenAi(OpenAiChat),
    Anthropic(Anthropic),
}

struct ApiTalk {
    wire: Wire,
    provider: String,
    config: Config,
    system: String,
    tools: Vec<Value>,
    history: Vec<Value>,
    max_tokens: u32,
}

fn unreadable(args: &str) -> Value {
    serde_json::from_str::<Value>(args)
        .ok()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({ "__unreadable": args }))
}

fn result_for(call: &mut dyn FnMut(&str, &Value) -> String, name: &str, args: &Value) -> String {
    match args.get("__unreadable") {
        Some(given) => format!(
            "That did not work: the arguments were not a JSON object ({}). Call {name} again with valid JSON.",
            given.as_str().unwrap_or("")
        ),
        None => call(name, args),
    }
}

impl ApiTalk {
    fn new(
        wire: Wire,
        provider: &str,
        config: &Config,
        system: &str,
        tools: &[ToolDef],
    ) -> ApiTalk {
        let tools = tools
            .iter()
            .map(|t| match wire {
                Wire::OpenAi(_) => openai::function_of(&t.name, &t.description, &t.schema),
                Wire::Anthropic(_) => anthropic::tool_of(&t.name, &t.description, &t.schema),
            })
            .collect();
        let history = match wire {
            Wire::OpenAi(_) => vec![json!({ "role": "system", "content": system })],
            Wire::Anthropic(_) => Vec::new(),
        };
        let max_tokens = match &wire {
            Wire::OpenAi(chat) => ChatProvider::max_tokens(chat),
            Wire::Anthropic(api) => ChatProvider::max_tokens(api),
        }
        .unwrap_or(0);
        ApiTalk {
            wire,
            provider: provider.to_string(),
            config: config.clone(),
            system: system.to_string(),
            tools,
            history,
            max_tokens,
        }
    }

    fn model_and_effort(&self) -> (String, Option<String>) {
        match &self.wire {
            Wire::OpenAi(chat) => (chat.model().to_string(), chat.effort().map(str::to_string)),
            Wire::Anthropic(api) => (api.model().to_string(), api.effort().map(str::to_string)),
        }
    }
}

impl Session for ApiTalk {
    fn native(&self) -> bool {
        true
    }

    fn say(&mut self, text: &str, turn: Turn<'_>) -> Result<(String, Option<Answered>)> {
        let Turn {
            tail,
            max_tokens,
            most_calls,
            sink,
            restart,
            call,
        } = turn;
        let message = if tail.is_empty() {
            text.to_string()
        } else {
            format!("{text}\n\n{tail}")
        };
        self.history
            .push(json!({ "role": "user", "content": message }));
        let max_tokens = max_tokens.max(self.max_tokens);
        let (model, effort) = self.model_and_effort();
        let mut spent = Spent {
            model: Some(model),
            effort,
            ..Spent::default()
        };
        let mut sent_chars = self.system.len()
            + self
                .history
                .iter()
                .map(|m| m.to_string().len())
                .sum::<usize>();
        let mut calls = 0usize;
        loop {
            let no_tools = calls >= most_calls;
            let (answer, asked) = match &self.wire {
                Wire::OpenAi(chat) => {
                    let body = chat.tool_body(&self.history, &self.tools, max_tokens, no_tools);
                    let round = chat
                        .stream_round(&body, sink)
                        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                    match round.usage {
                        Some((input, output, cached)) => {
                            spent.input += input;
                            spent.output += output;
                            spent.cached += cached;
                        }
                        None => {
                            spent.estimated = true;
                            spent.input += sent_chars.div_ceil(4) as u64;
                            spent.output += (round.text.len()
                                + round.calls.iter().map(|c| c.2.len()).sum::<usize>())
                            .div_ceil(4) as u64;
                        }
                    }
                    if round.calls.is_empty() {
                        self.history
                            .push(json!({ "role": "assistant", "content": round.text }));
                        (round.text, Vec::new())
                    } else {
                        let listed: Vec<Value> = round
                            .calls
                            .iter()
                            .map(|(id, name, args)| {
                                json!({ "id": id, "type": "function", "function": { "name": name, "arguments": args } })
                            })
                            .collect();
                        self.history.push(json!({
                            "role": "assistant",
                            "content": if round.text.is_empty() { Value::Null } else { Value::String(round.text.clone()) },
                            "tool_calls": listed,
                        }));
                        let asked: Vec<(String, String, Value)> = round
                            .calls
                            .into_iter()
                            .map(|(id, name, args)| (id, name, unreadable(&args)))
                            .collect();
                        (round.text, asked)
                    }
                }
                Wire::Anthropic(api) => {
                    let body = api.body(
                        &self.system,
                        &self.history,
                        &self.tools,
                        max_tokens,
                        no_tools,
                    );
                    let round = api
                        .stream(&body, sink)
                        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                    spent.input +=
                        round.usage.input + round.usage.cache_read + round.usage.cache_write;
                    spent.output += round.usage.output;
                    spent.cached += round.usage.cache_read;
                    spent.cache_written += round.usage.cache_write;
                    self.history
                        .push(json!({ "role": "assistant", "content": round.blocks }));
                    (round.text, round.calls)
                }
            };
            if asked.is_empty() {
                if answer.trim().is_empty() {
                    anyhow::bail!("{} returned no answer", self.provider);
                }
                return Ok((answer, Some(answered(&self.config, &self.provider, spent))));
            }
            if !answer.is_empty() {
                restart();
            }
            let mut results = Vec::new();
            for (id, name, args) in &asked {
                calls += 1;
                let result = result_for(call, name, args);
                sent_chars += result.len();
                results.push((id.clone(), result));
            }
            match &self.wire {
                Wire::OpenAi(_) => {
                    for (id, result) in results {
                        self.history
                            .push(json!({ "role": "tool", "tool_call_id": id, "content": result }));
                    }
                }
                Wire::Anthropic(_) => {
                    let blocks: Vec<Value> = results
                        .into_iter()
                        .map(|(id, result)| json!({ "type": "tool_result", "tool_use_id": id, "content": result }))
                        .collect();
                    self.history
                        .push(json!({ "role": "user", "content": blocks }));
                }
            }
        }
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
            Some(ProviderKind::Openai) => {
                let key = match provider.key_env.as_deref() {
                    Some(_) => crate::config::secret::resolve(provider.account(name), store),
                    None => None,
                };
                let wire = if provider
                    .base_url
                    .as_deref()
                    .is_some_and(anthropic::is_anthropic)
                {
                    let api = Anthropic::new(name.clone(), &provider, key);
                    if !api.available() {
                        continue;
                    }
                    Wire::Anthropic(api)
                } else {
                    let chat = OpenAiChat::new(name.clone(), &provider, key);
                    if !chat.available() {
                        continue;
                    }
                    Wire::OpenAi(chat)
                };
                return Some(Box::new(ApiTalk::new(
                    wire,
                    name,
                    config,
                    systems.native,
                    tools,
                )));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::provider::ProviderConfig;
    use crate::config::secret::{MemoryStore, SecretStore};
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};

    type Heard = Arc<Mutex<Vec<(String, Value)>>>;

    fn serve(answers: Vec<String>) -> (String, Heard) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let heard = Arc::clone(&seen);
        std::thread::spawn(move || {
            for answer in answers {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let mut raw = Vec::new();
                let mut buffer = [0u8; 65536];
                let body_at = loop {
                    let n = stream.read(&mut buffer).unwrap_or(0);
                    if n == 0 {
                        break None;
                    }
                    raw.extend_from_slice(&buffer[..n]);
                    if let Some(at) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                        break Some(at + 4);
                    }
                };
                let Some(body_at) = body_at else { return };
                let head = String::from_utf8_lossy(&raw[..body_at]).to_string();
                let length: usize = head
                    .lines()
                    .find_map(|l| {
                        l.to_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse().unwrap_or(0))
                    })
                    .unwrap_or(0);
                while raw.len() < body_at + length {
                    let n = stream.read(&mut buffer).unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    raw.extend_from_slice(&buffer[..n]);
                }
                let body =
                    serde_json::from_slice(&raw[body_at..body_at + length]).unwrap_or(Value::Null);
                heard.lock().unwrap().push((head, body));
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                    answer.len()
                );
            }
        });
        (format!("http://127.0.0.1:{port}/v1"), seen)
    }

    fn tools() -> Vec<ToolDef> {
        vec![ToolDef {
            name: "search_notes".into(),
            description: "Search the notes.".into(),
            schema: json!({ "type": "object", "properties": { "query": { "type": "string" } } }),
        }]
    }

    struct Ran {
        answer: String,
        answered: Answered,
        shown: String,
        restarts: usize,
        calls: Vec<(String, Value)>,
    }

    fn run(talk: &mut dyn Session, most_calls: usize) -> Ran {
        let mut shown = String::new();
        let mut restarts = 0;
        let mut calls = Vec::new();
        let (answer, answered) = talk
            .say(
                "What is a heap?",
                Turn {
                    tail: "",
                    max_tokens: 16000,
                    most_calls,
                    sink: &mut |t| shown.push_str(t),
                    restart: &mut || restarts += 1,
                    call: &mut |name, args| {
                        calls.push((name.to_string(), args.clone()));
                        "[n1] \"Heaps\": the minimum sits at the root.".to_string()
                    },
                },
            )
            .unwrap();
        Ran {
            answer,
            answered: answered.unwrap(),
            shown,
            restarts,
            calls,
        }
    }

    fn sse(events: &[&str]) -> String {
        events.iter().map(|e| format!("data: {e}\n\n")).collect()
    }

    #[test]
    fn an_openai_compatible_model_calls_leos_tools_natively_and_answers() {
        let (url, seen) = serve(vec![
            sse(&[
                r#"{"choices":[{"delta":{"content":"Checking."}}]}"#,
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"search_notes","arguments":"{\"query\":\"heap\"}"}},{"index":1,"id":"c2","function":{"name":"search_notes","arguments":"not json"}}]}}]}"#,
                r#"{"choices":[],"usage":{"prompt_tokens":3000,"completion_tokens":20,"prompt_tokens_details":{"cached_tokens":2048}}}"#,
                "[DONE]",
            ]),
            sse(&[
                r#"{"choices":[{"delta":{"content":"The minimum is at the root [n1]."}}]}"#,
                r#"{"choices":[],"usage":{"prompt_tokens":3200,"completion_tokens":15,"prompt_tokens_details":{"cached_tokens":2944}}}"#,
                "[DONE]",
            ]),
        ]);
        let cfg = ProviderConfig {
            kind: Some(ProviderKind::Openai),
            base_url: Some(url.replace("/v1", "/api.openai.com/v1")),
            model: Some("gpt-5-mini".into()),
            ..ProviderConfig::default()
        };
        let chat = OpenAiChat::new("openai".into(), &cfg, None);
        let mut talk = ApiTalk::new(
            Wire::OpenAi(chat),
            "openai",
            &Config::default(),
            "Be Felix.",
            &tools(),
        );
        let ran = run(&mut talk, 12);
        assert_eq!(ran.answer, "The minimum is at the root [n1].");
        assert_eq!(ran.shown, "Checking.The minimum is at the root [n1].");
        assert_eq!(ran.restarts, 1, "words before a tool call are taken back");
        assert_eq!(
            ran.calls,
            [("search_notes".to_string(), json!({ "query": "heap" }))]
        );
        assert_eq!(
            (ran.answered.input, ran.answered.output, ran.answered.cached),
            (6200, 35, 4992)
        );
        let seen = seen.lock().unwrap();
        let first = &seen[0].1;
        assert_eq!(
            first["messages"][0],
            json!({ "role": "system", "content": "Be Felix." })
        );
        assert_eq!(first["tools"][0]["function"]["name"], "search_notes");
        let second = &seen[1].1["messages"];
        assert_eq!(second[2]["tool_calls"][0]["id"], "c1");
        assert_eq!(second[3]["tool_call_id"], "c1");
        assert!(second[3]["content"]
            .as_str()
            .unwrap()
            .contains("minimum sits at the root"));
        assert_eq!(second[4]["tool_call_id"], "c2");
        assert!(second[4]["content"]
            .as_str()
            .unwrap()
            .starts_with("That did not work: the arguments were not a JSON object"));
    }

    #[test]
    fn claude_through_the_messages_api_caches_and_keeps_its_thinking_between_tool_calls() {
        let (url, seen) = serve(vec![
            sse(&[
                r#"{"type":"message_start","message":{"usage":{"input_tokens":50,"cache_creation_input_tokens":4000,"cache_read_input_tokens":0,"output_tokens":1}}}"#,
                r#"{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}"#,
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Search first."}}"#,
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"s1"}}"#,
                r#"{"type":"content_block_stop","index":0}"#,
                r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"t1","name":"search_notes","input":{}}}"#,
                r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"query\":\"heap\"}"}}"#,
                r#"{"type":"content_block_stop","index":1}"#,
                r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":30}}"#,
            ]),
            sse(&[
                r#"{"type":"message_start","message":{"usage":{"input_tokens":80,"cache_creation_input_tokens":0,"cache_read_input_tokens":4000,"output_tokens":1}}}"#,
                r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"At the root [n1]."}}"#,
                r#"{"type":"content_block_stop","index":0}"#,
                r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":12}}"#,
            ]),
        ]);
        let store = MemoryStore::default();
        store.set("anthropic", "sk-ant-test").unwrap();
        let cfg = ProviderConfig {
            kind: Some(ProviderKind::Openai),
            base_url: Some(url),
            model: Some("claude-sonnet-5-5".into()),
            effort: Some("high".into()),
            key_env: Some("ANTHROPIC_API_KEY".into()),
            ..ProviderConfig::default()
        };
        let key = crate::config::secret::resolve("anthropic", &store);
        let api = Anthropic::new("anthropic".into(), &cfg, key);
        let mut talk = ApiTalk::new(
            Wire::Anthropic(api),
            "anthropic",
            &Config::default(),
            "Be Felix.",
            &tools(),
        );
        let ran = run(&mut talk, 12);
        assert_eq!(ran.answer, "At the root [n1].");
        assert_eq!(ran.restarts, 0);
        assert_eq!(
            ran.calls,
            [("search_notes".to_string(), json!({ "query": "heap" }))]
        );
        assert_eq!(ran.answered.cached, 4000);
        assert_eq!(ran.answered.input, 50 + 4000 + 80 + 4000);
        assert_eq!(ran.answered.effort.as_deref(), Some("high"));
        let seen = seen.lock().unwrap();
        assert!(seen[0].0.to_lowercase().contains("x-api-key: sk-ant-test"));
        assert!(seen[0]
            .0
            .to_lowercase()
            .contains("anthropic-version: 2023-06-01"));
        let first = &seen[0].1;
        assert_eq!(first["system"][0]["text"], "Be Felix.");
        assert_eq!(first["output_config"]["effort"], "high");
        assert_eq!(first["tools"][0]["input_schema"]["type"], "object");
        let second = &seen[1].1["messages"];
        assert_eq!(
            second[1]["content"][0]["signature"], "s1",
            "thinking is sent back as it came"
        );
        assert_eq!(second[1]["content"][1]["input"], json!({ "query": "heap" }));
        assert_eq!(second[2]["content"][0]["type"], "tool_result");
        assert_eq!(second[2]["content"][0]["tool_use_id"], "t1");
    }

    #[test]
    fn once_the_calls_run_out_the_model_is_told_to_answer_without_tools() {
        let (url, seen) = serve(vec![
            sse(&[
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"search_notes","arguments":"{}"}}]}}]}"#,
            ]),
            sse(&[r#"{"choices":[{"delta":{"content":"Done."}}]}"#]),
        ]);
        let cfg = ProviderConfig {
            base_url: Some(url),
            ..ProviderConfig::default()
        };
        let mut talk = ApiTalk::new(
            Wire::OpenAi(OpenAiChat::new("ollama".into(), &cfg, None)),
            "ollama",
            &Config::default(),
            "",
            &tools(),
        );
        let ran = run(&mut talk, 1);
        assert_eq!(ran.answer, "Done.");
        assert!(
            ran.answered.local || ran.answered.cost.is_none() || ran.answered.cost == Some(0.0)
        );
        let seen = seen.lock().unwrap();
        assert!(seen[0].1.get("tool_choice").is_none());
        assert_eq!(seen[1].1["tool_choice"], "none");
    }

    #[test]
    fn api_providers_get_native_tool_sessions_and_claude_gets_the_messages_api() {
        let store = MemoryStore::default();
        store.set("anthropic", "sk-ant").unwrap();
        let mut config = Config::default();
        config.chat.chain = vec!["openai".into(), "anthropic".into()];
        let systems = Systems {
            native: "n",
            text: "t",
        };
        let talk =
            open(&config, &store, &systems, &tools()).expect("a keyed API provider gets a session");
        assert!(talk.native());
        config.chat.chain = vec!["openai".into()];
        assert!(
            open(&config, &store, &systems, &tools()).is_none(),
            "no key, no session"
        );
    }
}
