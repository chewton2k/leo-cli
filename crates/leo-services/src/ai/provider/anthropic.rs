use std::io::BufRead;

use serde_json::{json, Value};

use crate::ai::error::{
    classify_reqwest, classify_status, classify_status_with_key, scrub_secret, ProviderError,
    ProviderResult,
};
use crate::ai::provider::{ChatProvider, ChatRequest, Image, Sink, Spent};
use crate::config::provider::ProviderConfig;
use crate::config::secret::Secret;

const DEFAULT_MAX_TOKENS: u32 = 4096;
const VERSION: &str = "2023-06-01";
pub const EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

pub fn is_anthropic(base_url: &str) -> bool {
    base_url.contains("api.anthropic.com")
}

pub struct Anthropic {
    name: String,
    base_url: String,
    model: String,
    key: Option<Secret>,
    max_tokens: u32,
    effort: Option<String>,
    spent: std::sync::Mutex<Option<Spent>>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

impl Usage {
    pub fn plus(self, more: Usage) -> Usage {
        Usage {
            input: self.input + more.input,
            output: self.output + more.output,
            cache_read: self.cache_read + more.cache_read,
            cache_write: self.cache_write + more.cache_write,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Round {
    pub blocks: Vec<Value>,
    pub text: String,
    pub calls: Vec<(String, String, Value)>,
    pub stop: String,
    pub usage: Usage,
}

impl Anthropic {
    pub fn new(name: String, cfg: &ProviderConfig, key: Option<Secret>) -> Anthropic {
        let effort = crate::config::choice::effort_for(
            &name,
            cfg.model.as_deref().unwrap_or(""),
            cfg.effort.as_deref(),
        );
        Anthropic {
            name,
            base_url: cfg
                .base_url
                .clone()
                .unwrap_or_else(|| "https://api.anthropic.com/v1".to_string()),
            model: cfg.model.clone().unwrap_or_default(),
            key,
            max_tokens: cfg.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
            effort,
            spent: std::sync::Mutex::new(None),
        }
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn effort(&self) -> Option<&str> {
        self.effort.as_deref()
    }

    pub fn body(
        &self,
        system: &str,
        messages: &[Value],
        tools: &[Value],
        max_tokens: u32,
        no_tools: bool,
    ) -> Value {
        let mut body = json!({
            "model": self.model,
            "max_tokens": max_tokens,
            "stream": true,
            "cache_control": { "type": "ephemeral" },
            "messages": messages,
        });
        if !system.trim().is_empty() {
            body["system"] = json!([{
                "type": "text",
                "text": system,
                "cache_control": { "type": "ephemeral" },
            }]);
        }
        if !tools.is_empty() {
            let mut tools = tools.to_vec();
            if let Some(last) = tools.last_mut() {
                last["cache_control"] = json!({ "type": "ephemeral" });
            }
            body["tools"] = Value::Array(tools);
            if no_tools {
                body["tool_choice"] = json!({ "type": "none" });
            }
        }
        if let Some(effort) = &self.effort {
            body["output_config"] = json!({ "effort": effort });
        }
        body
    }

    pub fn stream(&self, body: &Value, sink: Sink<'_>) -> ProviderResult<Round> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(600))
            .build()
            .map_err(|e| ProviderError::Fatal(format!("{}: {e}", self.name)))?;
        let mut request = client
            .post(format!("{}/messages", self.base_url.trim_end_matches('/')))
            .header("content-type", "application/json")
            .header("anthropic-version", VERSION);
        if let Some(key) = &self.key {
            request = request.header("x-api-key", key.as_str());
        }
        let resp = request
            .json(body)
            .send()
            .map_err(|e| classify_reqwest(&self.name, &e))?;
        let status = resp.status().as_u16();
        if !resp.status().is_success() {
            let text = resp.text().unwrap_or_default();
            let text = scrub_secret(&text, self.key.as_ref().map(Secret::as_str));
            return Err(match self.key {
                Some(_) => classify_status_with_key(status, &self.name, &text),
                None => classify_status(status, &self.name, &text),
            });
        }
        read_events(std::io::BufReader::new(resp), &self.name, sink)
    }

    fn answer(&self, req: &ChatRequest, content: Value, sink: Sink<'_>) -> ProviderResult<String> {
        let messages = [json!({ "role": "user", "content": content })];
        let body = self.body(
            req.system.as_deref().unwrap_or(""),
            &messages,
            &[],
            req.max_tokens,
            false,
        );
        let round = self.stream(&body, sink)?;
        if round.stop == "max_tokens" {
            leo_core::diag::warn(format!(
                "{}: the answer hit its {}-token limit and is cut off; raise max_tokens for {} in config.toml (:settings, then e)",
                self.name, req.max_tokens, self.name
            ));
        }
        if round.text.trim().is_empty() {
            return Err(ProviderError::Retryable(format!(
                "{}: returned no content",
                self.name
            )));
        }
        self.note_spent(round.usage, req, &round.text);
        Ok(round.text)
    }

    fn note_spent(&self, usage: Usage, req: &ChatRequest, answer: &str) {
        let spent = if usage == Usage::default() {
            let mut guessed = Spent::guessed(Some(self.model.clone()), req, answer);
            guessed.effort = self.effort.clone();
            guessed
        } else {
            spent_of(usage, &self.model, self.effort.as_deref())
        };
        if let Ok(mut slot) = self.spent.lock() {
            *slot = Some(spent);
        }
    }
}

pub fn spent_of(usage: Usage, model: &str, effort: Option<&str>) -> Spent {
    Spent {
        model: Some(model.to_string()),
        effort: effort.map(str::to_string),
        input: usage.input + usage.cache_read + usage.cache_write,
        output: usage.output,
        estimated: false,
        cached: usage.cache_read,
        cache_written: usage.cache_write,
    }
}

pub fn tool_of(name: &str, description: &str, schema: &Value) -> Value {
    json!({ "name": name, "description": description, "input_schema": schema })
}

fn stream_failed(name: &str, error: &Value) -> ProviderError {
    let kind = error["type"].as_str().unwrap_or("");
    let message = error["message"].as_str().unwrap_or("the stream failed");
    let said = format!("{name}: {message}");
    match kind {
        "overloaded_error" | "api_error" | "rate_limit_error" | "timeout_error" => {
            ProviderError::Retryable(said)
        }
        _ => ProviderError::Fatal(said),
    }
}

pub fn read_events(reader: impl BufRead, name: &str, sink: Sink<'_>) -> ProviderResult<Round> {
    let mut round = Round::default();
    let mut partial: Vec<String> = Vec::new();
    for line in reader.lines() {
        let line =
            line.map_err(|e| ProviderError::Retryable(format!("{name}: stream ended early: {e}")))?;
        let Some(payload) = line.strip_prefix("data:") else {
            continue;
        };
        let Ok(event) = serde_json::from_str::<Value>(payload.trim()) else {
            continue;
        };
        let at = event["index"].as_u64().unwrap_or(0) as usize;
        match event["type"].as_str().unwrap_or("") {
            "message_start" => {
                let usage = &event["message"]["usage"];
                round.usage.input = usage["input_tokens"].as_u64().unwrap_or(0);
                round.usage.cache_read = usage["cache_read_input_tokens"].as_u64().unwrap_or(0);
                round.usage.cache_write =
                    usage["cache_creation_input_tokens"].as_u64().unwrap_or(0);
                round.usage.output = usage["output_tokens"].as_u64().unwrap_or(0);
            }
            "content_block_start" => {
                while round.blocks.len() <= at {
                    round.blocks.push(Value::Null);
                    partial.push(String::new());
                }
                round.blocks[at] = event["content_block"].clone();
            }
            "content_block_delta" => {
                let Some(block) = round.blocks.get_mut(at) else {
                    continue;
                };
                let delta = &event["delta"];
                match delta["type"].as_str().unwrap_or("") {
                    "text_delta" => {
                        let text = delta["text"].as_str().unwrap_or("");
                        let whole = format!("{}{text}", block["text"].as_str().unwrap_or(""));
                        block["text"] = Value::String(whole);
                        round.text.push_str(text);
                        sink(text);
                    }
                    "input_json_delta" => {
                        partial[at].push_str(delta["partial_json"].as_str().unwrap_or(""));
                    }
                    "thinking_delta" => {
                        let more = delta["thinking"].as_str().unwrap_or("");
                        let whole = format!("{}{more}", block["thinking"].as_str().unwrap_or(""));
                        block["thinking"] = Value::String(whole);
                    }
                    "signature_delta" => {
                        let more = delta["signature"].as_str().unwrap_or("");
                        let whole = format!("{}{more}", block["signature"].as_str().unwrap_or(""));
                        block["signature"] = Value::String(whole);
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                let Some(block) = round.blocks.get_mut(at) else {
                    continue;
                };
                if block["type"] == "tool_use" {
                    let given = partial[at].trim();
                    let input = if given.is_empty() {
                        json!({})
                    } else {
                        serde_json::from_str(given)
                            .unwrap_or_else(|_| json!({ "__unreadable": given }))
                    };
                    block["input"] = input.clone();
                    round.calls.push((
                        block["id"].as_str().unwrap_or("").to_string(),
                        block["name"].as_str().unwrap_or("").to_string(),
                        input,
                    ));
                }
            }
            "message_delta" => {
                if let Some(stop) = event["delta"]["stop_reason"].as_str() {
                    round.stop = stop.to_string();
                }
                if let Some(output) = event["usage"]["output_tokens"].as_u64() {
                    round.usage.output = output;
                }
            }
            "error" => return Err(stream_failed(name, &event["error"])),
            _ => {}
        }
    }
    round.blocks.retain(|b| !b.is_null());
    Ok(round)
}

impl ChatProvider for Anthropic {
    fn complete(&self, req: &ChatRequest) -> ProviderResult<String> {
        self.answer(req, json!(req.prompt), &mut |_| {})
    }

    fn complete_streaming(&self, req: &ChatRequest, sink: Sink<'_>) -> ProviderResult<String> {
        self.answer(req, json!(req.prompt), sink)
    }

    fn complete_with_images(&self, req: &ChatRequest, images: &[Image]) -> ProviderResult<String> {
        use base64::Engine;
        let mut content: Vec<Value> = images
            .iter()
            .map(|image| {
                json!({
                    "type": "image",
                    "source": {
                        "type": "base64",
                        "media_type": image.mime,
                        "data": base64::engine::general_purpose::STANDARD.encode(&image.bytes),
                    }
                })
            })
            .collect();
        content.push(json!({ "type": "text", "text": req.prompt }));
        self.answer(req, Value::Array(content), &mut |_| {})
    }

    fn available(&self) -> bool {
        self.key.is_some()
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn spent(&self) -> Option<Spent> {
        self.spent.lock().ok().and_then(|slot| slot.clone())
    }

    fn unavailable_reason(&self) -> String {
        format!(
            "{}: no API key (run `leo doctor`, or type :settings in leo)",
            self.name
        )
    }

    fn max_tokens(&self) -> Option<u32> {
        Some(self.max_tokens)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STREAM: &str = r#"event: message_start
data: {"type":"message_start","message":{"usage":{"input_tokens":20,"cache_read_input_tokens":3000,"cache_creation_input_tokens":500,"output_tokens":1}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}

data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Look it up."}}

data: {"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig=="}}

data: {"type":"content_block_stop","index":0}

data: {"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}

data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Let me "}}

data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"check."}}

data: {"type":"content_block_stop","index":1}

data: {"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"toolu_1","name":"search_notes","input":{}}}

data: {"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"query\": \"he"}}

data: {"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"ap\"}"}}

data: {"type":"content_block_stop","index":2}

data: {"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":42}}

data: {"type":"message_stop"}
"#;

    #[test]
    fn a_stream_gives_its_text_tool_calls_thinking_and_cache_use() {
        let mut shown = String::new();
        let round =
            read_events(STREAM.as_bytes(), "Anthropic", &mut |t| shown.push_str(t)).unwrap();
        assert_eq!(shown, "Let me check.");
        assert_eq!(round.text, "Let me check.");
        assert_eq!(round.stop, "tool_use");
        assert_eq!(
            round.calls,
            [(
                "toolu_1".to_string(),
                "search_notes".to_string(),
                json!({ "query": "heap" })
            )]
        );
        assert_eq!(round.blocks.len(), 3);
        assert_eq!(round.blocks[0]["thinking"], "Look it up.");
        assert_eq!(
            round.blocks[0]["signature"], "sig==",
            "thinking goes back exactly as it came"
        );
        assert_eq!(round.blocks[2]["input"], json!({ "query": "heap" }));
        assert_eq!(
            round.usage,
            Usage {
                input: 20,
                output: 42,
                cache_read: 3000,
                cache_write: 500
            }
        );
        let spent = spent_of(round.usage, "claude-sonnet-5-5", Some("high"));
        assert_eq!(
            (spent.input, spent.cached, spent.cache_written),
            (3520, 3000, 500)
        );
    }

    #[test]
    fn an_error_in_the_stream_is_retried_only_when_it_is_passing() {
        let busy = "data: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n";
        assert!(matches!(
            read_events(busy.as_bytes(), "Anthropic", &mut |_| {}),
            Err(ProviderError::Retryable(m)) if m.contains("Overloaded")
        ));
        let bad = "data: {\"type\":\"error\",\"error\":{\"type\":\"invalid_request_error\",\"message\":\"bad\"}}\n";
        assert!(matches!(
            read_events(bad.as_bytes(), "Anthropic", &mut |_| {}),
            Err(ProviderError::Fatal(_))
        ));
    }

    #[test]
    fn the_request_caches_instructions_tools_and_the_conversation_and_sets_effort() {
        let cfg = ProviderConfig {
            model: Some("claude-opus-5-5".into()),
            effort: Some("XHigh".into()),
            ..ProviderConfig::default()
        };
        let api = Anthropic::new("anthropic".into(), &cfg, None);
        let tools = [
            tool_of("search_notes", "Search.", &json!({"type": "object"})),
            tool_of("open_note", "Open.", &json!({"type": "object"})),
        ];
        let body = api.body(
            "Be Felix.",
            &[json!({"role": "user", "content": "hi"})],
            &tools,
            16000,
            false,
        );
        assert_eq!(
            body["cache_control"]["type"], "ephemeral",
            "the growing conversation is cached"
        );
        assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
        assert!(body["tools"][0].get("cache_control").is_none());
        assert_eq!(body["tools"][1]["cache_control"]["type"], "ephemeral");
        assert_eq!(body["output_config"]["effort"], "xhigh");
        assert!(body.get("tool_choice").is_none());
        let last = api.body("", &[], &tools, 100, true);
        assert_eq!(last["tool_choice"]["type"], "none");
        assert!(last.get("system").is_none());
        let unknown = Anthropic::new(
            "anthropic".into(),
            &ProviderConfig {
                effort: Some("turbo".into()),
                ..ProviderConfig::default()
            },
            None,
        );
        assert!(unknown
            .body("x", &[], &[], 1, false)
            .get("output_config")
            .is_none());
    }
}
