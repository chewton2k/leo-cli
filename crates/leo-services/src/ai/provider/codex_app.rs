use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::ai::error::{ProviderError, ProviderResult};
use crate::ai::provider::agent_cli::{codex_disables, locate, stop_all, AgentCli, TOTAL_LIMIT};
use crate::ai::provider::{Sink, Spent};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tokens {
    pub input: u64,
    pub cached: u64,
    pub output: u64,
}

pub struct CodexThread {
    name: String,
    child: std::process::Child,
    stdin: Option<std::process::ChildStdin>,
    messages: Receiver<Value>,
    thread: String,
    next: u64,
    quiet: Duration,
    model: Option<String>,
    effort: Option<String>,
    total: Tokens,
    _room: tempfile::TempDir,
}

pub struct CodexTurn<'a> {
    pub sink: Sink<'a>,
    pub restart: &'a mut dyn FnMut(),
    pub call: &'a mut dyn FnMut(&str, &Value) -> String,
}

pub fn tool_specs(tools: &[(String, String, Value)]) -> Vec<Value> {
    tools
        .iter()
        .map(|(name, description, schema)| {
            json!({ "type": "function", "name": name, "description": description, "inputSchema": schema })
        })
        .collect()
}

pub fn tokens_of(usage: &Value) -> Tokens {
    let total = &usage["total"];
    Tokens {
        input: total["inputTokens"].as_u64().unwrap_or(0),
        cached: total["cachedInputTokens"].as_u64().unwrap_or(0),
        output: total["outputTokens"].as_u64().unwrap_or(0)
            + total["reasoningOutputTokens"].as_u64().unwrap_or(0),
    }
}

pub fn declined(method: &str) -> Value {
    match method {
        "execCommandApproval" | "applyPatchApproval" => json!({ "decision": "denied" }),
        "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
            json!({ "decision": "decline" })
        }
        "item/tool/requestUserInput" => json!({ "answers": {} }),
        _ => json!({}),
    }
}

impl CodexThread {
    pub fn start(agent: &AgentCli, system: &str, tools: &[Value]) -> ProviderResult<CodexThread> {
        let name = agent.provider_name().to_string();
        let fail = |what: String| ProviderError::Retryable(format!("{name}: {what}"));
        let program =
            locate(agent.bin()).ok_or_else(|| fail(format!("{} is not installed", agent.bin())))?;
        let room = agent.room()?;
        let mut command = Command::new(&program);
        command.arg("app-server");
        for feature in codex_disables(&program) {
            command.args(["-c", &format!("features.{feature}=false")]);
        }
        command
            .current_dir(room.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        let mut child = command
            .spawn()
            .map_err(|e| fail(format!("could not run {}: {e}", agent.bin())))?;
        let stdin = child.stdin.take();
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| fail("could not read codex".into()))?;
        let (tx, messages) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Ok(message) = serde_json::from_str::<Value>(&line) {
                    if tx.send(message).is_err() {
                        break;
                    }
                }
            }
        });
        let mut thread = CodexThread {
            name: name.clone(),
            child,
            stdin,
            messages,
            thread: String::new(),
            next: 1,
            quiet: agent.quiet_limit(),
            model: agent.model().map(str::to_string),
            effort: agent.effort().map(str::to_string),
            total: Tokens::default(),
            _room: room,
        };
        thread.request(
            "initialize",
            json!({
                "clientInfo": { "name": "leo", "version": env!("CARGO_PKG_VERSION") },
                "capabilities": { "experimentalApi": true },
            }),
        )?;
        thread.notify("initialized")?;
        let mut params = json!({
            "cwd": thread._room.path(),
            "sandbox": "read-only",
            "approvalPolicy": "never",
            "ephemeral": true,
            "developerInstructions": system,
            "dynamicTools": tools,
        });
        if let Some(model) = &thread.model {
            params["model"] = json!(model);
        }
        let started = thread.request("thread/start", params)?;
        thread.thread = started["thread"]["id"]
            .as_str()
            .ok_or_else(|| fail("codex did not start a conversation".into()))?
            .to_string();
        if let Some(model) = started["model"]
            .as_str()
            .or_else(|| started["thread"]["model"].as_str())
        {
            thread.model = Some(model.to_string());
        }
        if thread.effort.is_none() {
            thread.effort = started["reasoningEffort"].as_str().map(str::to_string);
        }
        Ok(thread)
    }

    fn send(&mut self, message: Value) -> ProviderResult<()> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| ProviderError::Retryable(format!("{}: codex has stopped", self.name)))?;
        writeln!(stdin, "{message}")
            .and_then(|_| stdin.flush())
            .map_err(|e| ProviderError::Retryable(format!("{}: codex has stopped: {e}", self.name)))
    }

    fn notify(&mut self, method: &str) -> ProviderResult<()> {
        self.send(json!({ "jsonrpc": "2.0", "method": method }))
    }

    fn next_message(&mut self, until: Instant) -> ProviderResult<Value> {
        let wait = self
            .quiet
            .min(until.saturating_duration_since(Instant::now()));
        match self.messages.recv_timeout(wait) {
            Ok(message) => Ok(message),
            Err(RecvTimeoutError::Timeout) => {
                stop_all(&mut self.child);
                Err(ProviderError::Retryable(format!(
                    "{}: codex stopped answering and was stopped",
                    self.name
                )))
            }
            Err(RecvTimeoutError::Disconnected) => Err(ProviderError::Retryable(format!(
                "{}: codex ended. If it is not signed in, run `codex login`.",
                self.name
            ))),
        }
    }

    fn request(&mut self, method: &str, params: Value) -> ProviderResult<Value> {
        let id = self.next;
        self.next += 1;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))?;
        let until = Instant::now() + TOTAL_LIMIT;
        loop {
            let message = self.next_message(until)?;
            if message["id"] == id && message.get("method").is_none() {
                if let Some(error) = message.get("error") {
                    return Err(ProviderError::Retryable(format!(
                        "{}: codex refused {method}: {}",
                        self.name,
                        error["message"].as_str().unwrap_or("unknown error")
                    )));
                }
                return Ok(message["result"].clone());
            }
            self.answer_server(&message, &mut |_, _| String::new())?;
        }
    }

    fn answer_server(
        &mut self,
        message: &Value,
        call: &mut dyn FnMut(&str, &Value) -> String,
    ) -> ProviderResult<bool> {
        let (Some(id), Some(method)) = (message.get("id"), message["method"].as_str()) else {
            return Ok(false);
        };
        let result = if method == "item/tool/call" {
            let params = &message["params"];
            let text = call(params["tool"].as_str().unwrap_or(""), &params["arguments"]);
            json!({ "contentItems": [{ "type": "inputText", "text": text }], "success": true })
        } else {
            declined(method)
        };
        self.send(json!({ "jsonrpc": "2.0", "id": id, "result": result }))?;
        Ok(method == "item/tool/call")
    }

    pub fn say(&mut self, text: &str, turn: CodexTurn<'_>) -> ProviderResult<(String, Spent)> {
        let CodexTurn {
            sink,
            restart,
            call,
        } = turn;
        let id = self.next;
        self.next += 1;
        let mut params = json!({
            "threadId": self.thread,
            "input": [{ "type": "text", "text": text }],
        });
        if let Some(effort) = &self.effort {
            params["effort"] = json!(effort);
        }
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": "turn/start", "params": params }))?;
        let before = self.total.clone();
        let until = Instant::now() + TOTAL_LIMIT;
        let mut answer = String::new();
        loop {
            let message = self.next_message(until)?;
            if message["id"] == id && message.get("method").is_none() {
                if let Some(error) = message.get("error") {
                    return Err(ProviderError::Retryable(format!(
                        "{}: codex refused the message: {}",
                        self.name,
                        error["message"].as_str().unwrap_or("unknown error")
                    )));
                }
                continue;
            }
            if message.get("id").is_some() {
                let tool = message["method"] == "item/tool/call";
                if tool && !answer.trim().is_empty() {
                    restart();
                    answer.clear();
                }
                self.answer_server(&message, call)?;
                continue;
            }
            let params = &message["params"];
            match message["method"].as_str().unwrap_or("") {
                "item/agentMessage/delta" => {
                    if let Some(delta) = params["delta"].as_str() {
                        answer.push_str(delta);
                        sink(delta);
                    }
                }
                "thread/tokenUsage/updated" => self.total = tokens_of(&params["tokenUsage"]),
                "model/rerouted" => {
                    if let Some(model) = params["toModel"]
                        .as_str()
                        .or_else(|| params["model"].as_str())
                    {
                        self.model = Some(model.to_string());
                    }
                }
                "error" => {
                    return Err(ProviderError::Retryable(format!(
                        "{}: codex failed: {}",
                        self.name,
                        params["error"]["message"]
                            .as_str()
                            .or_else(|| params["message"].as_str())
                            .unwrap_or("unknown error")
                    )))
                }
                "turn/completed" if params["threadId"] == self.thread.as_str() => {
                    if let Some(error) = params["turn"]["error"]["message"].as_str() {
                        return Err(ProviderError::Retryable(format!(
                            "{}: codex failed: {error}",
                            self.name
                        )));
                    }
                    break;
                }
                _ => {}
            }
        }
        let used = Tokens {
            input: self.total.input.saturating_sub(before.input),
            cached: self.total.cached.saturating_sub(before.cached),
            output: self.total.output.saturating_sub(before.output),
        };
        let estimated = used == Tokens::default();
        Ok((
            answer.trim().to_string(),
            Spent {
                model: self.model.clone(),
                effort: self.effort.clone(),
                input: if estimated {
                    text.chars().count().div_ceil(4) as u64
                } else {
                    used.input
                },
                output: if estimated {
                    answer.chars().count().div_ceil(4) as u64
                } else {
                    used.output
                },
                estimated,
                cached: used.cached,
                cache_written: 0,
            },
        ))
    }
}

impl Drop for CodexThread {
    fn drop(&mut self) {
        self.stdin.take();
        stop_all(&mut self.child);
        let _ = self.child.wait();
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::ai::provider::agent_cli::Agent;
    use crate::config::provider::{ProviderConfig, ProviderKind};

    fn fake(dir: &std::path::Path) -> String {
        use std::os::unix::fs::PermissionsExt;
        let log = dir.join("sent.log");
        let path = dir.join("codex");
        let body = format!(
            r##"#!/bin/sh
if [ "$1" = "features" ]; then exit 0; fi
while IFS= read -r line; do
  echo "$line" >> "{log}"
  case "$line" in
    *'"method":"initialize"'*) printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{}}}}' ;;
    *'"method":"thread/start"'*) printf '%s\n' '{{"jsonrpc":"2.0","id":2,"result":{{"thread":{{"id":"t1"}},"model":"gpt-6.1-sol","reasoningEffort":"medium"}}}}' ;;
    *'"method":"turn/start"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":3,"result":{{"turn":{{"id":"u1"}}}}}}'
      printf '%s\n' '{{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{{"delta":"Let me look. "}}}}'
      printf '%s\n' '{{"jsonrpc":"2.0","id":90,"method":"item/tool/call","params":{{"tool":"search_notes","arguments":{{"query":"heap"}},"callId":"c1","threadId":"t1","turnId":"u1"}}}}'
      printf '%s\n' '{{"jsonrpc":"2.0","id":91,"method":"item/commandExecution/requestApproval","params":{{}}}}' ;;
    *'"id":91'*)
      printf '%s\n' '{{"jsonrpc":"2.0","method":"thread/tokenUsage/updated","params":{{"threadId":"t1","turnId":"u1","tokenUsage":{{"total":{{"inputTokens":1200,"cachedInputTokens":800,"outputTokens":40,"reasoningOutputTokens":10,"totalTokens":1250}},"last":{{}}}}}}}}'
      printf '%s\n' '{{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{{"delta":"Heaps keep the minimum on top."}}}}'
      printf '%s\n' '{{"jsonrpc":"2.0","method":"turn/completed","params":{{"threadId":"t1","turn":{{"id":"u1"}}}}}}' ;;
  esac
done
"##,
            log = log.display()
        );
        std::fs::write(&path, body).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.to_string_lossy().to_string()
    }

    #[test]
    fn codex_keeps_one_conversation_and_calls_leos_tools_natively() {
        let dir = tempfile::TempDir::new().unwrap();
        let bin = fake(dir.path());
        let quiet = ProviderConfig {
            kind: Some(ProviderKind::Codex),
            bin: Some(bin.clone()),
            ..ProviderConfig::default()
        };
        let default_effort = CodexThread::start(
            &AgentCli::new("codex".into(), Agent::Codex, &quiet),
            "x",
            &[],
        )
        .unwrap();
        assert_eq!(
            default_effort.effort.as_deref(),
            Some("medium"),
            "Codex says its own effort"
        );
        drop(default_effort);
        std::fs::remove_file(dir.path().join("sent.log")).unwrap();
        let cfg = ProviderConfig {
            kind: Some(ProviderKind::Codex),
            bin: Some(bin),
            effort: Some("high".into()),
            ..ProviderConfig::default()
        };
        let agent = AgentCli::new("codex".into(), Agent::Codex, &cfg);
        let tools = tool_specs(&[(
            "search_notes".into(),
            "Find notes.".into(),
            json!({ "type": "object", "properties": { "query": { "type": "string" } } }),
        )]);
        let mut thread = CodexThread::start(&agent, "You are Felix.", &tools).unwrap();
        let mut shown = String::new();
        let mut restarts = 0;
        let mut asked = Vec::new();
        let (answer, spent) = thread
            .say(
                "what is a heap?",
                CodexTurn {
                    sink: &mut |t| shown.push_str(t),
                    restart: &mut || restarts += 1,
                    call: &mut |name, args| {
                        asked.push(format!("{name} {}", args["query"]));
                        "[n1] \"Heaps\"".into()
                    },
                },
            )
            .unwrap();
        assert_eq!(answer, "Heaps keep the minimum on top.");
        assert_eq!(asked, ["search_notes \"heap\""]);
        assert_eq!(restarts, 1, "text before a tool call is taken back");
        assert_eq!(
            (
                spent.model.as_deref(),
                spent.effort.as_deref(),
                spent.input,
                spent.output,
                spent.estimated
            ),
            (Some("gpt-6.1-sol"), Some("high"), 1200, 50, false)
        );
        let sent = std::fs::read_to_string(dir.path().join("sent.log")).unwrap();
        let lines: Vec<Value> = sent
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines[0]["params"]["capabilities"]["experimentalApi"], true);
        let start = &lines[2]["params"];
        assert_eq!(
            (
                start["sandbox"].as_str(),
                start["approvalPolicy"].as_str(),
                start["ephemeral"].as_bool()
            ),
            (Some("read-only"), Some("never"), Some(true))
        );
        assert_eq!(start["developerInstructions"], "You are Felix.");
        assert_eq!(start["dynamicTools"][0]["name"], "search_notes");
        assert_eq!(lines[3]["params"]["effort"], "high");
        let tool_reply = lines.iter().find(|l| l["id"] == 90).unwrap();
        assert_eq!(
            tool_reply["result"]["contentItems"][0]["text"],
            "[n1] \"Heaps\""
        );
        let approval = lines.iter().find(|l| l["id"] == 91).unwrap();
        assert_eq!(approval["result"]["decision"], "decline");
    }

    #[test]
    #[ignore = "talks to the real Codex on this computer"]
    fn real_codex_uses_a_dynamic_tool() {
        let cfg = ProviderConfig {
            kind: Some(ProviderKind::Codex),
            ..ProviderConfig::default()
        };
        let agent = AgentCli::new("codex".into(), Agent::Codex, &cfg);
        let tools = tool_specs(&[(
            "lookup".into(),
            "Look up the secret word. Returns it.".into(),
            json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        )]);
        let mut thread =
            CodexThread::start(&agent, "Answer in one short sentence.", &tools).unwrap();
        let mut calls = 0;
        let (answer, spent) = thread
            .say(
                "Use the lookup tool, then tell me the secret word.",
                CodexTurn {
                    sink: &mut |_| {},
                    restart: &mut || {},
                    call: &mut |_, _| {
                        calls += 1;
                        "The secret word is maple.".into()
                    },
                },
            )
            .unwrap();
        println!("answer: {answer}\nspent: {spent:?}\ncalls: {calls}");
        assert!(calls >= 1);
        assert!(answer.to_lowercase().contains("maple"));
        let (again, _) = thread
            .say(
                "What was the secret word again? One word.",
                CodexTurn {
                    sink: &mut |_| {},
                    restart: &mut || {},
                    call: &mut |_, _| "unused".into(),
                },
            )
            .unwrap();
        println!("again: {again}");
        assert!(
            again.to_lowercase().contains("maple"),
            "the thread remembers"
        );
    }
}
