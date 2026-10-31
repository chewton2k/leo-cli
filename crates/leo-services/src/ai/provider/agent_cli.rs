use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::process::{self, Process, Watchdog, MOST_OUTPUT, QUEUED_MESSAGES};
use crate::ai::error::{ProviderError, ProviderResult};
use crate::ai::provider::{ChatProvider, ChatRequest, Image, Sink, Spent};
use crate::config::provider::{ProviderConfig, ProviderKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Agent {
    ClaudeCode,
    Codex,
}

impl Agent {
    pub fn of(cfg: &ProviderConfig) -> Option<Agent> {
        match cfg.kind {
            Some(ProviderKind::ClaudeCode) => Some(Agent::ClaudeCode),
            Some(ProviderKind::Codex) => Some(Agent::Codex),
            _ => None,
        }
    }

    pub fn program(self) -> &'static str {
        match self {
            Agent::ClaudeCode => "claude",
            Agent::Codex => "codex",
        }
    }

    pub fn plan(self) -> &'static str {
        match self {
            Agent::ClaudeCode => "your Claude plan",
            Agent::Codex => "your ChatGPT plan",
        }
    }

    pub fn install(self) -> &'static str {
        match self {
            Agent::ClaudeCode => "install Claude Code: https://claude.com/claude-code",
            Agent::Codex => "install Codex: npm install -g @openai/codex",
        }
    }

    pub fn sign_in(self) -> &'static str {
        match self {
            Agent::ClaudeCode => "run `claude` once and sign in",
            Agent::Codex => "run `codex login`",
        }
    }
}

pub fn locate(bin: &str) -> Option<PathBuf> {
    let given = Path::new(bin);
    if given.components().count() > 1 {
        return given.is_file().then(|| given.to_path_buf());
    }
    leo_core::paths::find(bin)
}

const CODEX_OFF: [&str; 18] = [
    "shell_tool",
    "browser_use",
    "browser_use_external",
    "browser_use_full_cdp_access",
    "computer_use",
    "in_app_browser",
    "in_app_local_automation",
    "apps",
    "image_generation",
    "goals",
    "multi_agent",
    "plugins",
    "remote_plugin",
    "skill_search",
    "skill_mcp_dependency_install",
    "tool_suggest",
    "hooks",
    "daemon_auto_start",
];

pub fn known_features(listing: &str) -> Vec<String> {
    let names: std::collections::BTreeSet<&str> = listing
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .collect();
    CODEX_OFF
        .iter()
        .filter(|f| names.contains(*f))
        .map(|f| f.to_string())
        .collect()
}

const PROBE_LIMIT: Duration = Duration::from_secs(5);

pub(crate) fn probe(program: &Path, args: &[&str]) -> ProviderResult<String> {
    let mut command = leo_core::children::quiet_command(program);
    command.args(args);
    super::scoped_cli::plan_environment(&mut command);
    probe_command(command)
}

pub(crate) fn probe_command(mut command: Command) -> ProviderResult<String> {
    let failed = || {
        ProviderError::Retryable(
        "The writing program could not be checked safely. Run its `--help` command, then try again.".into(),
    )
    };
    let (output, writer) = std::io::pipe().map_err(|_| failed())?;
    let errors = writer.try_clone().map_err(|_| failed())?;
    command.stdin(Stdio::null()).stdout(writer).stderr(errors);
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let child = command.spawn().map_err(|_| failed())?;
    drop(command);
    let child = Process::new(child);
    let watch = Watchdog::start(&child, None, PROBE_LIMIT);
    let (text, exceeded) = process::output(output);
    if exceeded {
        child.stop();
    }
    let status = loop {
        let status = child
            .0
            .lock()
            .ok()
            .and_then(|mut c| c.as_mut().and_then(|c| c.try_wait().ok()))
            .flatten();
        if let Some(status) = status {
            break status;
        }
        if watch.remaining(PROBE_LIMIT).is_none() {
            child.stop();
            return Err(ProviderError::Retryable(
                "The writing program took too long to check. Run its `--help` command, then try again.".into(),
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    if exceeded || !status.success() || watch.stalled.load(Ordering::Relaxed) {
        return Err(ProviderError::Retryable(
            "The writing program could not be checked safely. Run its `--help` command, then try again.".into(),
        ));
    }
    Ok(text)
}

pub(crate) fn codex_disables(program: &Path) -> ProviderResult<Vec<String>> {
    static KNOWN: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<PathBuf, Vec<String>>>,
    > = std::sync::OnceLock::new();
    let cache = KNOWN.get_or_init(Default::default);
    if let Some(found) = cache.lock().ok().and_then(|c| c.get(program).cloned()) {
        return Ok(found);
    }
    let found = probe(program, &["features", "list"]).and_then(|listing| {
        let found = known_features(&listing);
        if !found.iter().any(|f| f == "shell_tool") {
            return Err(ProviderError::Retryable(
                "Codex did not report its safety controls. Update Codex, then try again.".into(),
            ));
        }
        Ok(found)
    })?;
    if let Ok(mut c) = cache.lock() {
        c.insert(program.to_path_buf(), found.clone());
    }
    Ok(found)
}

pub(crate) fn codex_native_isolated(program: &Path) -> bool {
    static KNOWN: std::sync::OnceLock<Mutex<std::collections::HashMap<PathBuf, bool>>> =
        std::sync::OnceLock::new();
    let cache = KNOWN.get_or_init(Default::default);
    if let Some(found) = cache.lock().ok().and_then(|c| c.get(program).copied()) {
        return found;
    }
    let Ok(help) = probe(program, &["app-server", "--help"]) else {
        return false;
    };
    let found = help.split_whitespace().any(|s| s == "--ignore-user-config")
        && help.split_whitespace().any(|s| s == "--ignore-rules");
    if let Ok(mut c) = cache.lock() {
        c.insert(program.to_path_buf(), found);
    }
    found
}

pub(crate) fn stop_all(child: &mut std::process::Child) {
    leo_core::children::stop_tree(child);
}

pub const QUIET_LIMIT: Duration = Duration::from_secs(180);
pub const TOTAL_LIMIT: Duration = Duration::from_secs(15 * 60);

struct Watched<R> {
    inner: R,
    last: Arc<Mutex<Instant>>,
}

impl<R: Read> Read for Watched<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        if n > 0 {
            if let Ok(mut last) = self.last.lock() {
                *last = Instant::now();
            }
        }
        Ok(n)
    }
}

pub struct AgentCli {
    name: String,
    agent: Agent,
    bin: String,
    model: Option<String>,
    effort: Option<String>,
    quiet_limit: Duration,
    total_limit: Duration,
    spent: Mutex<Option<Spent>>,
    thread: Mutex<Option<String>>,
}

impl AgentCli {
    pub fn new(name: String, agent: Agent, cfg: &ProviderConfig) -> Self {
        AgentCli {
            name,
            agent,
            bin: cfg
                .bin
                .clone()
                .filter(|b| !b.trim().is_empty())
                .unwrap_or_else(|| agent.program().to_string()),
            model: cfg.model.clone().filter(|m| !m.trim().is_empty()),
            effort: crate::config::choice::effort_for(
                match agent {
                    Agent::Codex => "codex",
                    Agent::ClaudeCode => "claude_code",
                },
                cfg.model.as_deref().unwrap_or(""),
                cfg.effort.as_deref(),
            ),
            quiet_limit: QUIET_LIMIT,
            total_limit: TOTAL_LIMIT,
            spent: Mutex::new(None),
            thread: Mutex::new(None),
        }
    }

    pub fn with_limits(mut self, quiet: Duration, total: Duration) -> Self {
        self.quiet_limit = quiet;
        self.total_limit = total;
        self
    }

    pub fn arguments(
        &self,
        req: &ChatRequest,
        images: &[Image],
        disables: &[String],
        files: &[PathBuf],
    ) -> (Vec<String>, String) {
        let mut args: Vec<String> = Vec::new();
        let input = match self.agent {
            Agent::ClaudeCode => {
                args.extend(
                    [
                        "-p",
                        "--safe-mode",
                        "--tools",
                        "WebSearch",
                        "--allowedTools",
                        "WebSearch",
                        "--no-session-persistence",
                        "--output-format",
                        "stream-json",
                        "--include-partial-messages",
                        "--verbose",
                    ]
                    .map(String::from),
                );
                if let Some(model) = &self.model {
                    args.extend(["--model".to_string(), model.clone()]);
                }
                if let Some(effort) = &self.effort {
                    args.extend(["--effort".to_string(), effort.clone()]);
                }
                if images.is_empty() {
                    req.prompt.clone()
                } else {
                    use base64::Engine;
                    args.extend(["--input-format".to_string(), "stream-json".to_string()]);
                    let mut content: Vec<serde_json::Value> = images
                        .iter()
                        .map(|image| {
                            serde_json::json!({
                                "type": "image",
                                "source": {
                                    "type": "base64",
                                    "media_type": image.mime,
                                    "data": base64::engine::general_purpose::STANDARD.encode(&image.bytes),
                                },
                            })
                        })
                        .collect();
                    content.push(serde_json::json!({ "type": "text", "text": req.prompt }));
                    format!(
                        "{}\n",
                        serde_json::json!({ "type": "user", "message": { "role": "user", "content": content } })
                    )
                }
            }
            Agent::Codex => {
                args.extend(
                    [
                        "exec",
                        "--skip-git-repo-check",
                        "--ephemeral",
                        "--ignore-user-config",
                        "--ignore-rules",
                        "--sandbox",
                        "read-only",
                        "--color",
                        "never",
                        "--json",
                    ]
                    .map(String::from),
                );
                args.extend(["-c".into(), "skills.include_instructions=false".into()]);
                if let Some(model) = &self.model {
                    args.extend(["--model".to_string(), model.clone()]);
                }
                if let Some(effort) = &self.effort {
                    args.extend([
                        "-c".to_string(),
                        format!("model_reasoning_effort=\"{effort}\""),
                    ]);
                }
                for feature in disables {
                    args.extend(["--disable".to_string(), feature.clone()]);
                }
                for file in files {
                    args.extend(["-i".to_string(), file.display().to_string()]);
                }
                args.push("-".to_string());
                match &req.system {
                    Some(system) => format!("{system}\n\n{}", req.prompt),
                    None => req.prompt.clone(),
                }
            }
        };
        (args, input)
    }

    fn image_files(&self, room: &Path, images: &[Image]) -> ProviderResult<Vec<PathBuf>> {
        if self.agent != Agent::Codex {
            return Ok(Vec::new());
        }
        let mut files = Vec::new();
        for (i, image) in images.iter().enumerate() {
            let path = room.join(format!("image-{i}.{}", image.extension()));
            let written = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .and_then(|mut f| f.write_all(&image.bytes));
            written.map_err(|e| {
                ProviderError::Retryable(format!(
                    "{}: could not hand the image over: {e}",
                    self.name
                ))
            })?;
            files.push(path);
        }
        Ok(files)
    }

    pub(crate) fn room(&self) -> ProviderResult<tempfile::TempDir> {
        let mut builder = tempfile::Builder::new();
        builder.prefix("leo-writing-");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            builder.permissions(std::fs::Permissions::from_mode(0o700));
        }
        builder.tempdir().map_err(|e| {
            ProviderError::Retryable(format!(
                "{}: could not make a private folder to run in: {e}",
                self.name
            ))
        })
    }
}

fn instructions_failed() -> ProviderError {
    ProviderError::Retryable(
        "Leo could not prepare private writing instructions. Check free space and try again."
            .into(),
    )
}

fn instructions_file(room: &Path, system: &str) -> ProviderResult<tempfile::NamedTempFile> {
    let write = || {
        let mut file = tempfile::Builder::new()
            .prefix("instructions-")
            .suffix(".md")
            .tempfile_in(room)?;
        file.write_all(system.as_bytes())?;
        file.as_file().sync_all()?;
        Ok::<_, std::io::Error>(file)
    };
    write().map_err(|_| instructions_failed())
}

fn last_words(text: &str) -> String {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let tail = lines[lines.len().saturating_sub(3)..].join(" ");
    let chars: Vec<char> = tail.chars().collect();
    if chars.len() > 300 {
        chars[chars.len() - 300..].iter().collect()
    } else {
        tail
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct Heard {
    pub answer: String,
    pub result: Option<(bool, String)>,
    pub usage: Option<crate::usage::Usage>,
    pub model: Option<String>,
    pub tokens: Option<(u64, u64)>,
    pub cache: (u64, u64),
    pub exceeded: bool,
    pub thread: Option<String>,
}

pub fn codex_banner(stderr: &str) -> (Option<String>, Option<String>, Option<u64>) {
    let mut model = None;
    let mut effort = None;
    let mut tokens = None;
    let mut lines = stderr.lines().map(str::trim).peekable();
    while let Some(line) = lines.next() {
        if let Some(rest) = line.strip_prefix("model:") {
            model = Some(rest.trim().to_string()).filter(|m| !m.is_empty());
        } else if let Some(rest) = line.strip_prefix("reasoning effort:") {
            effort = Some(rest.trim().to_string()).filter(|e| !e.is_empty() && e != "none");
        } else if let Some(rest) = line.strip_prefix("tokens used") {
            let digits = |s: &str| s.chars().filter(char::is_ascii_digit).collect::<String>();
            let mut found = digits(rest);
            if found.is_empty() {
                found = lines.peek().map(|next| digits(next)).unwrap_or_default();
            }
            tokens = found.parse().ok().or(tokens);
        }
    }
    (model, effort, tokens)
}

pub fn read_stream(mut lines: impl BufRead, sink: Sink<'_>) -> Heard {
    let mut heard = Heard::default();
    let mut bytes = 0usize;
    loop {
        let line = match process::line(&mut lines) {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(_) => {
                heard.exceeded = true;
                break;
            }
        };
        bytes = bytes.saturating_add(line.len());
        if bytes > MOST_OUTPUT {
            heard.exceeded = true;
            break;
        }
        hear(&line, &mut heard, sink);
    }
    heard
}

pub fn hear(line: &str, heard: &mut Heard, sink: Sink<'_>) -> bool {
    let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else {
        return false;
    };
    match event["type"].as_str() {
        Some("thread.started") => {
            heard.thread = event["thread_id"]
                .as_str()
                .filter(|id| {
                    id.len() == 36
                        && id.bytes().enumerate().all(|(i, b)| {
                            if [8, 13, 18, 23].contains(&i) {
                                b == b'-'
                            } else {
                                b.is_ascii_hexdigit()
                            }
                        })
                })
                .map(str::to_string);
            false
        }
        Some("item.completed") if event["item"]["type"] == "agent_message" => {
            if let Some(text) = event["item"]["text"].as_str() {
                if !heard.answer.is_empty() {
                    heard.answer.push_str("\n\n");
                    sink("\n\n");
                }
                heard.answer.push_str(text);
                sink(text);
            }
            false
        }
        Some("turn.completed") => {
            let usage = &event["usage"];
            if let (Some(input), Some(output)) = (
                usage["input_tokens"].as_u64(),
                usage["output_tokens"].as_u64(),
            ) {
                heard.tokens = Some((input, output));
                heard.cache = (
                    usage["cached_input_tokens"].as_u64().unwrap_or(0),
                    usage["cache_write_input_tokens"].as_u64().unwrap_or(0),
                );
            }
            true
        }
        Some("turn.failed" | "error") => {
            let message = event["error"]["message"]
                .as_str()
                .or_else(|| event["message"].as_str())
                .unwrap_or("The writing program could not finish. Try again.");
            heard.result = Some((true, message.into()));
            true
        }
        Some("stream_event") => {
            let delta = &event["event"]["delta"];
            if event["event"]["type"] == "content_block_delta" && delta["type"] == "text_delta" {
                if let Some(text) = delta["text"].as_str() {
                    heard.answer.push_str(text);
                    sink(text);
                }
            }
            false
        }
        Some("system") if event["subtype"] == "init" => {
            heard.model = event["model"].as_str().map(str::to_string);
            false
        }
        Some("rate_limit_event") => {
            if let Some(usage) =
                crate::usage::from_claude(&event["rate_limit_info"], chrono::Utc::now())
            {
                heard.usage = Some(usage);
            }
            false
        }
        Some("result") => {
            let failed = event["is_error"].as_bool().unwrap_or(false)
                || event["subtype"].as_str().is_some_and(|s| s != "success");
            let said = event["result"].as_str().unwrap_or_default().to_string();
            heard.result = Some((failed, said));
            let usage = &event["usage"];
            let input = [
                "input_tokens",
                "cache_read_input_tokens",
                "cache_creation_input_tokens",
            ]
            .iter()
            .filter_map(|k| usage[*k].as_u64())
            .sum::<u64>();
            if let Some(output) = usage["output_tokens"].as_u64() {
                heard.tokens = Some((input, output));
                heard.cache = (
                    usage["cache_read_input_tokens"].as_u64().unwrap_or(0),
                    usage["cache_creation_input_tokens"].as_u64().unwrap_or(0),
                );
            }
            true
        }
        _ => false,
    }
}

pub struct ClaudeSession {
    name: String,
    child: Process,
    stdin: Option<std::process::ChildStdin>,
    lines: std::sync::mpsc::Receiver<super::mcp::Incoming>,
    quiet: Duration,
    total_limit: Duration,
    model: Option<String>,
    effort: Option<String>,
    native: bool,
    _bridge: Option<super::mcp::Bridge>,
    _room: tempfile::TempDir,
}

pub struct Control<'a> {
    pub stopped: &'a dyn Fn() -> bool,
    pub tools_done: &'a dyn Fn() -> bool,
    pub most_calls: usize,
}

impl ClaudeSession {
    pub fn native(&self) -> bool {
        self.native
    }

    pub fn say(&mut self, text: &str, sink: Sink<'_>) -> ProviderResult<(String, Spent)> {
        self.say_with(
            text,
            sink,
            &mut |_, _| "That did not work: no tools are offered here.".to_string(),
            &mut || {},
        )
    }

    pub fn say_with(
        &mut self,
        text: &str,
        sink: Sink<'_>,
        call: &mut dyn FnMut(&str, &serde_json::Value) -> String,
        gap: &mut dyn FnMut(),
    ) -> ProviderResult<(String, Spent)> {
        self.say_controlled(
            text,
            sink,
            call,
            gap,
            Control {
                stopped: &|| false,
                tools_done: &|| false,
                most_calls: 12,
            },
        )
    }

    pub fn say_controlled(
        &mut self,
        text: &str,
        sink: Sink<'_>,
        call: &mut dyn FnMut(&str, &serde_json::Value) -> String,
        gap: &mut dyn FnMut(),
        control: Control<'_>,
    ) -> ProviderResult<(String, Spent)> {
        let watch = Watchdog::start(&self.child, Some(self.quiet), self.total_limit);
        let mut calls = 0usize;
        let mut limited = false;
        let mut idle = Instant::now();
        let message = serde_json::json!({
            "type": "user",
            "message": { "role": "user", "content": [{ "type": "text", "text": text }] },
        });
        let stdin = self.stdin.as_mut().ok_or_else(|| {
            ProviderError::Retryable(format!("{}: the session has ended", self.name))
        })?;
        writeln!(stdin, "{message}")
            .and_then(|_| stdin.flush())
            .map_err(|e| {
                ProviderError::Retryable(format!("{}: the session ended: {e}", self.name))
            })?;
        let mut heard = Heard::default();
        let mut bytes = 0usize;
        loop {
            if (control.stopped)() {
                self.child.stop();
                return Err(ProviderError::Retryable("The answer was stopped.".into()));
            }
            let Some(wait) = watch.remaining(self.quiet.saturating_sub(idle.elapsed())) else {
                self.child.stop();
                return Err(ProviderError::Retryable(format!(
                    "{}: claude stopped answering and was stopped",
                    self.name
                )));
            };
            match self.lines.recv_timeout(wait.min(Duration::from_millis(50))) {
                Ok(super::mcp::Incoming::Line(line)) => {
                    bytes = bytes.saturating_add(line.len());
                    if bytes > MOST_OUTPUT {
                        self.child.stop();
                        return Err(ProviderError::Retryable(format!(
                            "{}: claude sent too much output. Try a smaller request.",
                            self.name
                        )));
                    }
                    watch.touch();
                    idle = Instant::now();
                    if hear(&line, &mut heard, sink) {
                        break;
                    }
                }
                Ok(super::mcp::Incoming::Call { name, args, reply }) => {
                    watch.touch();
                    idle = Instant::now();
                    if limited {
                        self.child.stop();
                        return Err(ProviderError::Retryable("The writing program kept requesting tools after its limit. Try a smaller request.".into()));
                    }
                    gap();
                    let result = if calls >= control.most_calls || (control.tools_done)() {
                        limited = true;
                        "No more tools are available for this answer. Use the results already returned and answer the user now. Use interpretable language.".to_string()
                    } else {
                        calls += 1;
                        call(&name, &args)
                    };
                    let _ = reply.send(result);
                }
                Ok(super::mcp::Incoming::Failed) => {
                    self.child.stop();
                    return Err(ProviderError::Retryable(format!(
                        "{}: claude sent unreadable or excessive output. Try a smaller request.",
                        self.name
                    )));
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if idle.elapsed() < self.quiet && watch.remaining(self.quiet).is_some() {
                        continue;
                    }
                    self.child.stop();
                    return Err(ProviderError::Retryable(format!(
                        "{}: claude stopped answering and was stopped",
                        self.name
                    )));
                }
                Ok(super::mcp::Incoming::Ended)
                | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    self.child.stop();
                    return Err(ProviderError::Retryable(format!(
                        "{}: claude ended the session. If it is not signed in, {}.",
                        self.name,
                        Agent::ClaudeCode.sign_in()
                    )));
                }
            }
        }
        if let Some(usage) = heard.usage.clone() {
            crate::usage::save(&self.name, usage);
        }
        if heard.model.is_some() {
            self.model = heard.model.clone();
        }
        let answer = match &heard.result {
            Some((true, said)) => {
                return Err(ProviderError::Retryable(format!(
                    "{}: claude stopped: {}",
                    self.name,
                    last_words(said)
                )));
            }
            Some((false, said)) if heard.answer.trim().is_empty() => said.trim().to_string(),
            _ => heard.answer.trim().to_string(),
        };
        let (input, output, estimated) = match heard.tokens {
            Some((input, output)) => (input, output, false),
            None => (
                text.chars().count().div_ceil(4) as u64,
                answer.chars().count().div_ceil(4) as u64,
                true,
            ),
        };
        Ok((
            answer,
            Spent {
                model: self.model.clone(),
                effort: self.effort.clone(),
                input,
                output,
                estimated,
                cached: heard.cache.0,
                cache_written: heard.cache.1,
            },
        ))
    }
}

impl Drop for ClaudeSession {
    fn drop(&mut self) {
        self.stdin.take();
        self.child.stop();
    }
}

impl AgentCli {
    pub(crate) fn last_thread(&self) -> Option<String> {
        self.thread.lock().ok().and_then(|thread| thread.clone())
    }

    pub fn is_claude(&self) -> bool {
        self.agent == Agent::ClaudeCode
    }

    pub fn provider_name(&self) -> &str {
        &self.name
    }

    pub fn bin(&self) -> &str {
        &self.bin
    }

    pub fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    pub fn effort(&self) -> Option<&str> {
        self.effort.as_deref()
    }

    pub fn quiet_limit(&self) -> Duration {
        self.quiet_limit
    }

    pub fn total_limit(&self) -> Duration {
        self.total_limit
    }

    pub fn session_arguments(&self, system_file: &Path) -> Vec<String> {
        self.session_arguments_with(system_file, None)
    }

    fn session_arguments_with(
        &self,
        system_file: &Path,
        bridge: Option<&super::mcp::Bridge>,
    ) -> Vec<String> {
        let allowed = match bridge {
            Some(_) => format!("WebSearch mcp__{}", super::mcp::SERVER),
            None => "WebSearch".to_string(),
        };
        let isolation: &[&str] = match bridge {
            Some(_) => &["--setting-sources", "", "--disable-slash-commands"],
            None => &["--safe-mode"],
        };
        let mut args: Vec<String> = ["-p"]
            .iter()
            .chain(isolation)
            .map(|a| a.to_string())
            .collect();
        args.extend(
            [
                "--tools",
                "WebSearch",
                "--allowedTools",
                allowed.as_str(),
                "--no-session-persistence",
                "--input-format",
                "stream-json",
                "--output-format",
                "stream-json",
                "--include-partial-messages",
                "--verbose",
            ]
            .map(String::from),
        );
        if let Some(model) = &self.model {
            args.extend(["--model".to_string(), model.clone()]);
        }
        if let Some(effort) = &self.effort {
            args.extend(["--effort".to_string(), effort.clone()]);
        }
        if let Some(bridge) = bridge {
            args.extend([
                "--mcp-config".to_string(),
                bridge.config(),
                "--strict-mcp-config".to_string(),
            ]);
        }
        args.extend([
            "--system-prompt-file".to_string(),
            system_file.display().to_string(),
        ]);
        args
    }

    pub fn claude_session(&self, system: &str) -> ProviderResult<ClaudeSession> {
        self.claude_session_with(system, Vec::new())
    }

    pub fn claude_native(
        &self,
        system: &str,
        tools: Vec<super::mcp::Tool>,
    ) -> ProviderResult<ClaudeSession> {
        self.claude_session_with(system, tools)
    }

    fn claude_session_with(
        &self,
        system: &str,
        tools: Vec<super::mcp::Tool>,
    ) -> ProviderResult<ClaudeSession> {
        let program = locate(&self.bin).ok_or_else(|| {
            ProviderError::Retryable(format!("{}: {} is not installed", self.name, self.bin))
        })?;
        let room = self.room()?;
        let system_file = instructions_file(room.path(), system)?
            .keep()
            .map_err(|_| instructions_failed())?
            .1;
        let (tx, lines) = std::sync::mpsc::sync_channel(QUEUED_MESSAGES);
        let bridge = if tools.is_empty() {
            None
        } else {
            Some(super::mcp::Bridge::start(tools, tx.clone()).map_err(|e| {
                ProviderError::Retryable(format!("{}: could not offer leo's tools: {e}", self.name))
            })?)
        };
        let mut command = leo_core::children::quiet_command(program);
        command
            .args(self.session_arguments_with(&system_file, bridge.as_ref()))
            .current_dir(room.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        super::scoped_cli::configure(&mut command, self.agent)?;
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        let mut child = command.spawn().map_err(|e| {
            ProviderError::Retryable(format!("{}: could not run {}: {e}", self.name, self.bin))
        })?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let child = Process::new(child);
        let stdout = stdout.ok_or_else(|| {
            ProviderError::Retryable(format!("{}: could not read {}", self.name, self.bin))
        })?;
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                match process::line(&mut reader) {
                    Ok(Some(line)) => {
                        if tx.send(super::mcp::Incoming::Line(line)).is_err() {
                            break;
                        }
                    }
                    Ok(None) => {
                        let _ = tx.send(super::mcp::Incoming::Ended);
                        break;
                    }
                    Err(_) => {
                        let _ = tx.send(super::mcp::Incoming::Failed);
                        break;
                    }
                }
            }
        });
        Ok(ClaudeSession {
            name: self.name.clone(),
            child,
            stdin,
            lines,
            quiet: self.quiet_limit,
            total_limit: self.total_limit,
            model: self.model.clone(),
            effort: self.effort.clone(),
            native: bridge.is_some(),
            _bridge: bridge,
            _room: room,
        })
    }

    fn run(&self, req: &ChatRequest, images: &[Image], sink: Sink<'_>) -> ProviderResult<String> {
        self.run_controlled(req, images, sink, &|| false)
    }

    pub(crate) fn controlled(
        &self,
        req: &ChatRequest,
        sink: Sink<'_>,
        stopped: &dyn Fn() -> bool,
    ) -> ProviderResult<String> {
        self.run_controlled(req, &[], sink, stopped)
    }

    fn run_controlled(
        &self,
        req: &ChatRequest,
        images: &[Image],
        sink: Sink<'_>,
        stopped: &dyn Fn() -> bool,
    ) -> ProviderResult<String> {
        if stopped() {
            return Err(ProviderError::Fatal("The answer was stopped.".into()));
        }
        let program =
            locate(&self.bin).ok_or_else(|| ProviderError::Retryable(self.unavailable_reason()))?;
        let disables = match self.agent {
            Agent::Codex => codex_disables(&program)?,
            Agent::ClaudeCode => Vec::new(),
        };
        let room = self.room()?;
        let files = self.image_files(room.path(), images)?;
        let instructions = req
            .system
            .as_ref()
            .map(|system| instructions_file(room.path(), system))
            .transpose()?;
        let material = ChatRequest {
            system: if instructions.is_some() {
                None
            } else {
                req.system.clone()
            },
            prompt: req.prompt.clone(),
            temperature: req.temperature,
            max_tokens: req.max_tokens,
        };
        let (mut args, input) = self.arguments(&material, images, &disables, &files);
        if let Some(file) = &instructions {
            match self.agent {
                Agent::Codex => {
                    args.pop();
                    args.extend([
                        "-c".into(),
                        format!(
                            "model_instructions_file={}",
                            serde_json::to_string(&file.path().to_string_lossy()).map_err(
                                |_| {
                                    ProviderError::Retryable(
                                        "Leo could not prepare private writing instructions. Try again."
                                            .into(),
                                    )
                                }
                            )?
                        ),
                        "-".into(),
                    ]);
                }
                Agent::ClaudeCode => args.extend([
                    "--system-prompt-file".into(),
                    file.path().display().to_string(),
                ]),
            }
        }
        match self.spawn(&program, room.path(), &args, input.clone(), sink, stopped) {
            Err(ProviderError::Retryable(message))
                if !stopped() && (message == self.no_answer() || message == self.stalled()) =>
            {
                self.spawn(&program, room.path(), &args, input, sink, stopped)
            }
            other => other,
        }
    }

    fn note_spent(&self, heard: &Heard, stderr: &str, asked: usize, answer: &str) {
        let (banner_model, banner_effort, banner_tokens) = match self.agent {
            Agent::Codex => codex_banner(stderr),
            Agent::ClaudeCode => (None, None, None),
        };
        let mut spent = Spent {
            model: heard
                .model
                .clone()
                .or(banner_model)
                .or_else(|| self.model.clone()),
            effort: self.effort.clone().or(banner_effort),
            input: asked.div_ceil(4) as u64,
            output: answer.chars().count().div_ceil(4) as u64,
            estimated: true,
            ..Spent::default()
        };
        if let Some((input, output)) = heard.tokens {
            (spent.input, spent.output, spent.estimated) = (input, output, false);
            (spent.cached, spent.cache_written) = heard.cache;
        } else if let Some(total) = banner_tokens {
            spent.output = spent.output.min(total);
            spent.input = total - spent.output;
            spent.estimated = false;
        }
        if let Ok(mut slot) = self.spent.lock() {
            *slot = Some(spent);
        }
    }

    fn stalled(&self) -> String {
        format!(
            "{}: {} stopped answering and was stopped",
            self.name, self.bin
        )
    }

    fn no_answer(&self) -> String {
        format!("{}: {} gave no answer", self.name, self.bin)
    }

    pub(crate) fn spawn(
        &self,
        program: &Path,
        room: &Path,
        args: &[String],
        input: String,
        sink: Sink<'_>,
        stopped: &dyn Fn() -> bool,
    ) -> ProviderResult<String> {
        if let Ok(mut thread) = self.thread.lock() {
            *thread = None;
        }
        let mut command = leo_core::children::quiet_command(program);
        command
            .args(args)
            .current_dir(room)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
            unsafe {
                command.pre_exec(|| {
                    libc::umask(0o077);
                    Ok(())
                });
            }
        }
        super::scoped_cli::configure(&mut command, self.agent)?;
        let input_len = input.chars().count();
        let mut child = command.spawn().map_err(|e| {
            ProviderError::Retryable(format!("{}: could not run {}: {e}", self.name, self.bin))
        })?;
        let stdin = child.stdin.take();
        let stderr = child.stderr.take();
        let stdout = child.stdout.take();
        let child = Process::new(child);
        let watch = Watchdog::start(
            &child,
            (self.agent == Agent::ClaudeCode).then_some(self.quiet_limit),
            self.total_limit,
        );
        let writer = std::thread::spawn(move || {
            if let Some(mut stdin) = stdin {
                let _ = stdin.write_all(input.as_bytes());
            }
        });
        let stderr = stderr.map(|inner| Watched {
            inner,
            last: Arc::clone(&watch.last),
        });
        let errors = {
            let child = Arc::clone(&child.0);
            std::thread::spawn(move || {
                let result = stderr.map(process::output).unwrap_or_default();
                if result.1 {
                    if let Ok(mut child) = child.lock() {
                        if let Some(child) = child.as_mut() {
                            stop_all(child);
                        }
                    }
                }
                result
            })
        };
        let stdout = stdout.map(|inner| Watched {
            inner,
            last: Arc::clone(&watch.last),
        });
        let (messages, lines) = std::sync::mpsc::sync_channel(QUEUED_MESSAGES);
        let reader = std::thread::spawn(move || {
            if let Some(stdout) = stdout {
                let mut reader = BufReader::new(stdout);
                loop {
                    let line = process::line(&mut reader);
                    let ended = !matches!(&line, Ok(Some(_)));
                    if messages.send(line).is_err() || ended {
                        break;
                    }
                }
            }
        });
        let mut heard = Heard::default();
        let mut bytes = 0usize;
        loop {
            if stopped() || watch.stalled.load(Ordering::Relaxed) {
                child.stop();
                break;
            }
            match lines.recv_timeout(Duration::from_millis(50)) {
                Ok(Ok(Some(line))) => {
                    bytes = bytes.saturating_add(line.len());
                    if bytes > MOST_OUTPUT {
                        heard.exceeded = true;
                        break;
                    }
                    hear(&line, &mut heard, sink);
                    if let Some(started) = &heard.thread {
                        if let Ok(mut thread) = self.thread.lock() {
                            *thread = Some(started.clone());
                        }
                    }
                }
                Ok(Ok(None)) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                Ok(Err(_)) => {
                    heard.exceeded = true;
                    break;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
        drop(lines);
        if heard.exceeded {
            child.stop();
        }
        let status = loop {
            if stopped() {
                child.stop();
                break Err(ProviderError::Fatal("The answer was stopped.".into()));
            }
            let waited = child
                .0
                .lock()
                .map_err(|_| {
                    ProviderError::Retryable(format!("{}: {} did not finish", self.name, self.bin))
                })
                .and_then(|mut guard| {
                    guard
                        .as_mut()
                        .ok_or_else(|| {
                            ProviderError::Retryable(format!(
                                "{}: {} has stopped",
                                self.name, self.bin
                            ))
                        })?
                        .try_wait()
                        .map_err(|e| {
                            ProviderError::Retryable(format!(
                                "{}: {} did not finish: {e}",
                                self.name, self.bin
                            ))
                        })
                });
            match waited {
                Ok(Some(status)) => break Ok(status),
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                Err(err) => break Err(err),
            }
        };
        let _ = writer.join();
        let _ = reader.join();
        let (stderr, exceeded) = errors.join().unwrap_or_default();
        let status = status?;
        if watch.stalled.load(Ordering::Relaxed) {
            return Err(ProviderError::Retryable(self.stalled()));
        }
        if heard.exceeded || exceeded {
            return Err(ProviderError::Retryable(format!(
                "{}: {} sent too much or unreadable output. Try a smaller request.",
                self.name, self.bin
            )));
        }
        if let Some(usage) = heard.usage.clone() {
            crate::usage::save(&self.name, usage);
        }
        let failed_in_result = matches!(heard.result, Some((true, _)));
        if !status.success() || failed_in_result {
            let said = match &heard.result {
                Some((true, said)) if !said.trim().is_empty() => last_words(said),
                _ if !stderr.trim().is_empty() => last_words(&stderr),
                _ => last_words(&heard.answer),
            };
            return Err(ProviderError::Retryable(format!(
                "{}: {} stopped ({}): {said}. If it is not signed in, {}.",
                self.name,
                self.bin,
                status,
                self.agent.sign_in()
            )));
        }
        let mut answer = heard.answer.trim().to_string();
        if answer.is_empty() {
            if let Some((false, said)) = &heard.result {
                answer = said.trim().to_string();
            }
        }
        if answer.is_empty() {
            return Err(ProviderError::Retryable(self.no_answer()));
        }
        self.note_spent(&heard, &stderr, input_len, &answer);
        Ok(answer)
    }
}

impl ChatProvider for AgentCli {
    fn complete(&self, req: &ChatRequest) -> ProviderResult<String> {
        self.run(req, &[], &mut |_| {})
    }

    fn complete_streaming(&self, req: &ChatRequest, sink: Sink<'_>) -> ProviderResult<String> {
        self.run(req, &[], sink)
    }

    fn complete_with_images(&self, req: &ChatRequest, images: &[Image]) -> ProviderResult<String> {
        self.run(req, images, &mut |_| {})
    }

    fn available(&self) -> bool {
        locate(&self.bin).is_some()
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn spent(&self) -> Option<Spent> {
        self.spent.lock().ok().and_then(|slot| slot.clone())
    }

    fn unavailable_reason(&self) -> String {
        format!(
            "{}: `{}` is not installed; {}",
            self.name,
            self.bin,
            self.agent.install()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> ChatRequest {
        ChatRequest {
            system: Some("You write notes.".to_string()),
            prompt: "<transcript>hello</transcript>".to_string(),
            temperature: 0.2,
            max_tokens: 100,
        }
    }

    fn config(kind: ProviderKind, bin: &str, model: Option<&str>) -> ProviderConfig {
        ProviderConfig {
            kind: Some(kind),
            bin: Some(bin.to_string()),
            model: model.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn claude_code_writes_with_only_web_search_and_reads_the_material_from_stdin() {
        let cfg = config(
            ProviderKind::ClaudeCode,
            "claude",
            Some("claude-sonnet-5-5"),
        );
        let agent = AgentCli::new("claude_code".into(), Agent::of(&cfg).unwrap(), &cfg);
        let (args, input) = agent.arguments(&request(), &[], &[], &[]);
        assert_eq!(
            &args[..7],
            [
                "-p",
                "--safe-mode",
                "--tools",
                "WebSearch",
                "--allowedTools",
                "WebSearch",
                "--no-session-persistence"
            ]
        );
        assert!(args
            .windows(2)
            .any(|w| w == ["--model", "claude-sonnet-5-5"]));
        assert!(!args.iter().any(|a| a.contains("You write notes.")));
        assert_eq!(input, "<transcript>hello</transcript>");
    }

    #[test]
    fn codex_runs_read_only_outside_any_project_and_gets_one_message() {
        let cfg = config(ProviderKind::Codex, "codex", None);
        let agent = AgentCli::new("codex".into(), Agent::of(&cfg).unwrap(), &cfg);
        let (args, input) = agent.arguments(&request(), &[], &[], &[]);
        assert_eq!(args[0], "exec");
        assert!(args.windows(2).any(|w| w == ["--sandbox", "read-only"]));
        assert!(args.contains(&"--ephemeral".to_string()));
        assert!(args.contains(&"--ignore-user-config".to_string()));
        assert!(args.contains(&"--ignore-rules".to_string()));
        assert!(args
            .windows(2)
            .any(|w| w == ["-c", "skills.include_instructions=false"]));
        assert!(!args.contains(&"--model".to_string()));
        assert_eq!(args.last().unwrap(), "-");
        assert_eq!(input, "You write notes.\n\n<transcript>hello</transcript>");
    }

    #[test]
    fn codex_thread_events_track_only_explicit_valid_ids_without_showing_them() {
        let mut heard = Heard::default();
        let mut shown = String::new();
        for id in [
            "../../other",
            "",
            "maple",
            "00000000-0000-0000-0000-00000000000g",
        ] {
            hear(
                &serde_json::json!({"type":"thread.started","thread_id":id}).to_string(),
                &mut heard,
                &mut |text| shown.push_str(text),
            );
            assert!(heard.thread.is_none());
        }
        let id = "00000000-0000-0000-0000-000000000001";
        hear(
            &serde_json::json!({"type":"thread.started","thread_id":id}).to_string(),
            &mut heard,
            &mut |text| shown.push_str(text),
        );
        assert_eq!(heard.thread.as_deref(), Some(id));
        assert!(shown.is_empty());
    }

    #[test]
    fn images_go_inside_the_message_for_claude_code_with_every_tool_still_off() {
        let cfg = config(ProviderKind::ClaudeCode, "claude", None);
        let agent = AgentCli::new("claude_code".into(), Agent::ClaudeCode, &cfg);
        let image = Image {
            mime: "image/png".into(),
            bytes: vec![1, 2, 3],
        };
        let (args, input) = agent.arguments(&request(), &[image], &[], &[]);
        assert!(args.windows(2).any(|w| w == ["--tools", "WebSearch"]));
        assert!(args
            .windows(2)
            .any(|w| w == ["--input-format", "stream-json"]));
        let message: serde_json::Value = serde_json::from_str(input.trim()).unwrap();
        assert_eq!(message["type"], "user");
        assert_eq!(message["message"]["content"][0]["source"]["data"], "AQID");
        assert_eq!(
            message["message"]["content"][1]["text"],
            "<transcript>hello</transcript>"
        );
    }

    #[test]
    fn codex_gets_image_files_and_its_tools_switched_off() {
        let cfg = config(ProviderKind::Codex, "codex", None);
        let agent = AgentCli::new("codex".into(), Agent::Codex, &cfg);
        let off = vec!["shell_tool".to_string(), "browser_use".to_string()];
        let files = vec![PathBuf::from("/tmp/a.jpg")];
        let (args, _) = agent.arguments(&request(), &[], &off, &files);
        assert!(args.windows(2).any(|w| w == ["--disable", "shell_tool"]));
        assert!(args.windows(2).any(|w| w == ["--disable", "browser_use"]));
        assert!(args.windows(2).any(|w| w == ["-i", "/tmp/a.jpg"]));
        assert_eq!(args.last().unwrap(), "-");
    }

    #[cfg(unix)]
    #[test]
    fn each_call_runs_in_its_own_private_folder_that_is_removed_after() {
        use std::os::unix::fs::PermissionsExt;
        let cfg = config(ProviderKind::Codex, "codex", None);
        let agent = AgentCli::new("codex".into(), Agent::Codex, &cfg);
        let room = agent.room().unwrap();
        let mode = std::fs::metadata(room.path()).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        let image = Image {
            mime: "image/png".into(),
            bytes: vec![7],
        };
        let files = agent
            .image_files(room.path(), &[image.clone(), image])
            .unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|f| f.starts_with(room.path())));
        let again = agent.image_files(
            room.path(),
            &[Image {
                mime: "image/png".into(),
                bytes: vec![1],
            }],
        );
        assert!(again.is_err(), "an existing file is never written through");
        let path = room.path().to_path_buf();
        drop(room);
        assert!(!path.exists());
    }

    #[test]
    fn only_features_the_installed_codex_knows_are_switched_off() {
        let listing = "apps                 stable   true\nshell_tool           stable   true\nbrowser_use          stable   true\ndaemon_auto_start    stable   true\nsomething_else       stable   false\n";
        assert_eq!(
            known_features(listing),
            ["shell_tool", "browser_use", "apps", "daemon_auto_start"]
        );
        assert!(known_features("").is_empty());
        assert!(known_features("Error: unknown command").is_empty());
    }

    #[test]
    fn a_missing_program_is_unavailable_and_says_how_to_install_it() {
        let cfg = config(ProviderKind::Codex, "leo-not-a-real-codex", None);
        let agent = AgentCli::new("codex".into(), Agent::Codex, &cfg);
        assert!(!agent.available());
        assert!(agent
            .unavailable_reason()
            .contains("npm install -g @openai/codex"));
        assert!(matches!(
            agent.complete(&request()),
            Err(ProviderError::Retryable(_))
        ));
    }

    #[cfg(unix)]
    fn script(dir: &Path, body: &str) -> String {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("fake-agent");
        std::fs::write(&path, format!("#!/bin/sh\ntest -z \"${{CI+x}}${{ANTHROPIC_API_KEY+x}}${{OPENAI_API_KEY+x}}${{CODEX_API_KEY+x}}${{NODE_OPTIONS+x}}${{BASH_ENV+x}}\" || exit 17\nif [ \"$1\" = features ]; then echo 'shell_tool stable true'; exit 0; fi\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.to_string_lossy().to_string()
    }

    #[cfg(unix)]
    #[test]
    fn capability_probes_accept_help_on_either_stream_and_reject_failed_or_oversized_output() {
        let flags = "--agent --sandbox --disable-slash-commands --input-format --output-format --print-timeout";
        for stream in [1, 2] {
            let dir = tempfile::tempdir().unwrap();
            let bin = script(dir.path(), &format!("printf '%s\\n' '{flags}' >&{stream}"));
            assert_eq!(
                probe(Path::new(&bin), &["--help"]).unwrap().trim(),
                flags,
                "help on stream {stream}"
            );

            let dir = tempfile::tempdir().unwrap();
            let bin = script(
                dir.path(),
                &format!("head -c {} /dev/zero >&{stream}", MOST_OUTPUT + 1),
            );
            assert!(probe(Path::new(&bin), &["--help"]).is_err());
        }
        let dir = tempfile::tempdir().unwrap();
        let bin = script(dir.path(), "printf stdout; printf stderr >&2");
        let text = probe(Path::new(&bin), &["--help"]).unwrap();
        assert!(text.contains("stdout") && text.contains("stderr"));
        let dir = tempfile::tempdir().unwrap();
        let bin = script(
            dir.path(),
            &format!("printf '%s\\n' '{flags} PRIVATE_DIAGNOSTIC' >&2; exit 7"),
        );
        let error = probe(Path::new(&bin), &["--help"]).unwrap_err().to_string();
        assert!(!error.contains("PRIVATE_DIAGNOSTIC"));
    }

    #[test]
    fn claude_code_text_arrives_piece_by_piece_and_the_result_is_kept() {
        let stream = [
            r##"{"type":"system","subtype":"init"}"##,
            r##"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"hm"}}}"##,
            r##"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"# Notes\n"}}}"##,
            "not json",
            r##"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"hello"}}}"##,
            r##"{"type":"rate_limit_event","rate_limit_info":{"unifiedWindows":{"five_hour":{"utilization":0.09},"seven_day":{"utilization":0.93}}}}"##,
            r##"{"type":"result","subtype":"success","is_error":false,"result":"# Notes\nhello"}"##,
        ]
        .join("\n");
        let mut pieces = Vec::new();
        let heard = read_stream(stream.as_bytes(), &mut |p| pieces.push(p.to_string()));
        assert_eq!(pieces, ["# Notes\n", "hello"]);
        assert_eq!(heard.answer, "# Notes\nhello");
        assert_eq!(heard.result, Some((false, "# Notes\nhello".to_string())));
        let usage = heard.usage.expect("the limits that came with the answer");
        assert_eq!(
            crate::usage::label(&usage, usage.seen_at),
            "5h: 9%, 7d: 93%"
        );
        let failed = read_stream(
            r##"{"type":"result","subtype":"success","is_error":true,"result":"Not logged in"}"##
                .as_bytes(),
            &mut |_| {},
        );
        assert_eq!(failed.result, Some((true, "Not logged in".to_string())));
    }

    #[cfg(unix)]
    #[test]
    fn claude_code_streams_what_it_writes() {
        let dir = tempfile::TempDir::new().unwrap();
        let bin = script(
            dir.path(),
            r##"cat >/dev/null
printf '%s\n' '{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"# Notes\n"}}}'
printf '%s\n' '{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"hello"}}}'
printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"# Notes\nhello"}'"##,
        );
        let cfg = config(ProviderKind::ClaudeCode, &bin, None);
        let agent = AgentCli::new("claude_code".into(), Agent::ClaudeCode, &cfg);
        assert!(agent.available());
        let mut pieces = Vec::new();
        let answer = agent
            .complete_streaming(&request(), &mut |p| pieces.push(p.to_string()))
            .unwrap();
        assert_eq!(answer, "# Notes\nhello");
        assert_eq!(pieces.len(), 2);
        assert_eq!(agent.complete(&request()).unwrap(), "# Notes\nhello");
    }

    #[cfg(unix)]
    #[test]
    fn claude_code_reads_long_instructions_from_a_private_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let log = dir.path().join("heard");
        let bin = script(
            dir.path(),
            &format!(
                r##"cat >/dev/null
echo "$@" > "{log}.args"
while [ $# -gt 0 ]; do
  if [ "$1" = --system-prompt-file ]; then cat "$2" > "{log}.system"; fi
  shift
done
printf '%s\n' '{{"type":"result","subtype":"success","is_error":false,"result":"done"}}'"##,
                log = log.display()
            ),
        );
        let cfg = config(ProviderKind::ClaudeCode, &bin, None);
        let agent = AgentCli::new("claude_code".into(), Agent::ClaudeCode, &cfg);
        let system = "You write notes.\nKeep every point.\r\n%PATH% \"quoted\"";
        let req = ChatRequest {
            system: Some(system.to_string()),
            ..request()
        };
        assert_eq!(agent.complete(&req).unwrap(), "done");
        let args = std::fs::read_to_string(format!("{}.args", log.display())).unwrap();
        assert!(!args.contains("Keep every point."));
        assert_eq!(
            std::fs::read_to_string(format!("{}.system", log.display())).unwrap(),
            system
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_claude_session_stays_open_and_answers_each_message_in_turn() {
        let dir = tempfile::TempDir::new().unwrap();
        let log = dir.path().join("heard.log");
        let bin = script(
            dir.path(),
            &format!(
                r##"echo "$@" > "{log}.args"
for a in "$@"; do
  if [ "$take" = 1 ]; then cat "$a" > "{log}.system"; fi
  take=0
  if [ "$a" = --system-prompt-file ]; then take=1; fi
done
printf '%s\n' '{{"type":"system","subtype":"init","model":"claude-sonnet-5-5"}}'
n=0
while IFS= read -r line; do
  n=$((n+1))
  echo "$line" >> "{log}"
  printf '%s\n' '{{"type":"stream_event","event":{{"type":"content_block_delta","delta":{{"type":"text_delta","text":"answer '$n'"}}}}}}'
  printf '%s\n' '{{"type":"result","subtype":"success","is_error":false,"result":"answer '$n'","usage":{{"input_tokens":10,"cache_read_input_tokens":90,"output_tokens":5}}}}'
done"##,
                log = log.display()
            ),
        );
        let mut cfg = config(ProviderKind::ClaudeCode, &bin, Some("claude-sonnet-5-5"));
        cfg.effort = Some("high".into());
        let agent = AgentCli::new("claude_code".into(), Agent::ClaudeCode, &cfg);
        let mut session = agent.claude_session("You are Felix.").unwrap();
        let mut pieces = Vec::new();
        let (first, spent) = session
            .say("hello", &mut |p| pieces.push(p.to_string()))
            .unwrap();
        assert_eq!(first, "answer 1");
        assert_eq!(
            (
                spent.model.as_deref(),
                spent.effort.as_deref(),
                spent.input,
                spent.output,
                spent.estimated
            ),
            (Some("claude-sonnet-5-5"), Some("high"), 100, 5, false)
        );
        let (second, _) = session
            .say("<tool_result>x</tool_result>", &mut |_| {})
            .unwrap();
        assert_eq!(
            second, "answer 2",
            "the same process answers the next message"
        );
        assert_eq!(pieces, ["answer 1"]);
        let heard = std::fs::read_to_string(&log).unwrap();
        let sent: Vec<serde_json::Value> = heard
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(
            sent[1]["message"]["content"][0]["text"],
            "<tool_result>x</tool_result>"
        );
        let args = std::fs::read_to_string(format!("{}.args", log.display())).unwrap();
        assert!(args.contains("--input-format stream-json") && args.contains("--effort high"));
        assert!(args.contains("--system-prompt-file") && !args.contains("You are Felix."));
        assert_eq!(
            std::fs::read_to_string(format!("{}.system", log.display())).unwrap(),
            "You are Felix."
        );
    }

    #[test]
    #[ignore = "talks to the real Claude Code on this computer"]
    fn real_claude_session_remembers_between_messages() {
        let cfg = config(ProviderKind::ClaudeCode, "claude", None);
        let agent = AgentCli::new("claude_code".into(), Agent::ClaudeCode, &cfg);
        let mut session = agent
            .claude_session("Answer in one short sentence.")
            .unwrap();
        let (first, spent) = session
            .say("The secret word is maple. Reply only: noted.", &mut |_| {})
            .unwrap();
        let (second, _) = session
            .say("What is the secret word? One word.", &mut |_| {})
            .unwrap();
        println!("first: {first}\nsecond: {second}\nspent: {spent:?}");
        assert!(second.to_lowercase().contains("maple"));
    }

    #[test]
    #[ignore = "talks to the real Claude Code on this computer"]
    fn real_claude_calls_leos_tools_natively_inside_one_turn() {
        let cfg = config(ProviderKind::ClaudeCode, "claude", None);
        let agent = AgentCli::new("claude_code".into(), Agent::ClaudeCode, &cfg);
        let tools = vec![
            super::super::mcp::Tool {
                name: "secret_word".into(),
                description: "Returns today's secret word.".into(),
                schema: serde_json::json!({ "type": "object", "properties": {} }),
            },
            super::super::mcp::Tool {
                name: "save_brief".into(),
                description: "Saves a brief. Text may span lines and contain quotes.".into(),
                schema: serde_json::json!({ "type": "object", "properties": { "text": { "type": "string" } }, "required": ["text"] }),
            },
        ];
        let mut session = agent
            .claude_native(
                "Use the tools you are given. Answer in one short sentence.",
                tools,
            )
            .unwrap();
        assert!(session.native());
        let mut calls: Vec<(String, serde_json::Value)> = Vec::new();
        let started = Instant::now();
        let (answer, spent) = session
            .say_with(
                "Call secret_word, then call save_brief with a three-line brief that quotes the word in double quotes, then tell me the word.",
                &mut |_| {},
                &mut |name, args| {
                    calls.push((name.to_string(), args.clone()));
                    if name == "secret_word" { "maple".into() } else { "Saved.".into() }
                },
                &mut || {},
            )
            .unwrap();
        println!(
            "answer: {answer}\ncalls: {calls:?}\nspent: {spent:?}\ntook: {:?}",
            started.elapsed()
        );
        assert_eq!(calls.first().map(|c| c.0.as_str()), Some("secret_word"));
        let brief = calls
            .iter()
            .find(|c| c.0 == "save_brief")
            .expect("save_brief called");
        assert!(brief.1["text"].as_str().unwrap().contains("\"maple\""));
        assert!(answer.to_lowercase().contains("maple"));
    }

    #[cfg(unix)]
    #[test]
    fn claude_code_reporting_an_error_is_a_failure_even_when_it_exits_cleanly() {
        let dir = tempfile::TempDir::new().unwrap();
        let bin = script(
            dir.path(),
            r##"cat >/dev/null
printf '%s\n' '{"type":"result","subtype":"success","is_error":true,"result":"Not logged in. Please run /login"}'"##,
        );
        let cfg = config(ProviderKind::ClaudeCode, &bin, None);
        let agent = AgentCli::new("claude_code".into(), Agent::ClaudeCode, &cfg);
        match agent.complete(&request()) {
            Err(ProviderError::Retryable(message)) => {
                assert!(message.contains("Not logged in"), "{message}");
                assert!(message.contains("sign in"), "{message}");
            }
            other => panic!("expected a retryable failure, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn codex_answers_in_one_piece() {
        let dir = tempfile::TempDir::new().unwrap();
        let bin = script(
            dir.path(),
            r##"cat >/dev/null
printf '%s\n' '{"type":"item.completed","item":{"type":"agent_message","text":"# Notes\nhello"}}'
printf '%s\n' '{"type":"turn.completed","usage":{"input_tokens":1200,"cached_input_tokens":800,"output_tokens":100,"reasoning_output_tokens":70}}'"##,
        );
        let cfg = config(ProviderKind::Codex, &bin, None);
        let agent = AgentCli::new("codex".into(), Agent::Codex, &cfg);
        let mut pieces = Vec::new();
        let answer = agent
            .complete_streaming(&request(), &mut |p| pieces.push(p.to_string()))
            .unwrap();
        assert_eq!(answer, "# Notes\nhello");
        assert_eq!(pieces, ["# Notes\nhello"]);
        let spent = agent.spent().unwrap();
        assert_eq!(
            (spent.input, spent.cached, spent.output, spent.estimated),
            (1200, 800, 100, false)
        );
    }

    #[cfg(unix)]
    #[test]
    fn codex_uses_private_system_instructions_instead_of_duplicate_coding_instructions() {
        let dir = tempfile::TempDir::new().unwrap();
        let bin = script(
            dir.path(),
            r##"test "$(find . -type f -name 'instructions-*.md' | wc -l | tr -d ' ')" = 1 || exit 21
test "$(cat instructions-*.md)" = 'You write notes.' || exit 22
test "$(cat)" = '<transcript>hello</transcript>' || exit 23
printf '%s\n' "$@" | grep '^model_instructions_file=' >/dev/null || exit 24
printf '%s\n' '{"type":"item.completed","item":{"type":"agent_message","text":"Notes ready."}}'
printf '%s\n' '{"type":"turn.completed","usage":{"input_tokens":100,"cached_input_tokens":80,"output_tokens":10}}'"##,
        );
        let cfg = config(ProviderKind::Codex, &bin, None);
        let agent = AgentCli::new("codex".into(), Agent::Codex, &cfg);
        assert_eq!(agent.complete(&request()).unwrap(), "Notes ready.");
        assert_eq!(agent.spent().unwrap().cached, 80);
    }

    #[cfg(unix)]
    #[test]
    fn stopping_a_stateless_codex_request_does_not_retry_and_reaps_its_child() {
        for startup in ["", "sleep 0.2\n"] {
            let dir = tempfile::TempDir::new().unwrap();
            let marker = dir.path().join("started");
            let bin = script(
                dir.path(),
                &format!(
                    "{startup}echo $$ >> '{}'\ncat >/dev/null\nsleep 30",
                    marker.display()
                ),
            );
            let cfg = config(ProviderKind::Codex, &bin, None);
            let agent = AgentCli::new("codex".into(), Agent::Codex, &cfg)
                .with_limits(Duration::from_secs(30), Duration::from_secs(30));
            let began = Instant::now();
            let stopped = std::cell::Cell::new(None);
            let result = agent.controlled(&request(), &mut |_| {}, &|| {
                if marker.exists() {
                    if stopped.get().is_none() {
                        stopped.set(Some(Instant::now()));
                    }
                    true
                } else {
                    began.elapsed() >= Duration::from_secs(5)
                }
            });
            assert!(result.unwrap_err().to_string().contains("stopped"));
            assert!(
                stopped
                    .get()
                    .expect("the fake CLI must start before cancellation")
                    .elapsed()
                    < Duration::from_secs(2)
            );
            let started = std::fs::read_to_string(marker).unwrap();
            assert_eq!(started.lines().count(), 1);
            let pid: libc::pid_t = started.trim().parse().unwrap();
            assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
            assert_eq!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::ESRCH)
            );
        }
    }

    #[test]
    fn codex_json_reports_partial_failures_without_exposing_internal_events() {
        let stream = [
            r#"{"type":"item.completed","item":{"type":"reasoning","text":"private reasoning"}}"#,
            r#"{"type":"item.completed","item":{"type":"mcp_tool_call","server":"leo","tool":"read_note"}}"#,
            r#"{"type":"turn.failed","error":{"message":"Try signing in again."}}"#,
        ].join("\n");
        let mut pieces = Vec::new();
        let heard = read_stream(stream.as_bytes(), &mut |text| pieces.push(text.to_string()));
        assert!(pieces.is_empty());
        assert!(heard.answer.is_empty());
        assert_eq!(heard.result, Some((true, "Try signing in again.".into())));
    }

    #[cfg(unix)]
    #[test]
    fn stdout_eof_does_not_disable_the_exit_watchdog() {
        let dir = tempfile::TempDir::new().unwrap();
        let bin = script(dir.path(), "cat >/dev/null\nexec 1>&-\nsleep 30");
        let cfg = config(ProviderKind::Codex, &bin, None);
        let agent = AgentCli::new("codex".into(), Agent::Codex, &cfg)
            .with_limits(Duration::from_secs(1), Duration::from_millis(150));
        let started = Instant::now();
        assert!(agent.complete(&request()).is_err());
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[cfg(unix)]
    #[test]
    fn native_claude_has_an_absolute_deadline_despite_steady_messages() {
        let dir = tempfile::TempDir::new().unwrap();
        let bin = script(
            dir.path(),
            "read -r line\nwhile :; do echo '{\"type\":\"heartbeat\"}'; sleep 0.02; done",
        );
        let cfg = config(ProviderKind::ClaudeCode, &bin, None);
        let agent = AgentCli::new("claude_code".into(), Agent::ClaudeCode, &cfg)
            .with_limits(Duration::from_secs(1), Duration::from_millis(150));
        let mut session = agent.claude_session("Use interpretable language.").unwrap();
        let started = Instant::now();
        assert!(session.say("hello", &mut |_| {}).is_err());
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[cfg(unix)]
    #[test]
    fn stopping_a_silent_claude_turn_terminates_its_process_promptly() {
        let dir = tempfile::TempDir::new().unwrap();
        let bin = script(dir.path(), "read -r line\nsleep 30");
        let cfg = config(ProviderKind::ClaudeCode, &bin, None);
        let agent = AgentCli::new("claude_code".into(), Agent::ClaudeCode, &cfg)
            .with_limits(Duration::from_secs(30), Duration::from_secs(30));
        let mut session = agent.claude_session("Use interpretable language.").unwrap();
        let started = Instant::now();
        let result = session.say_controlled(
            "hello",
            &mut |_| {},
            &mut |_, _| String::new(),
            &mut || {},
            Control {
                stopped: &|| started.elapsed() >= Duration::from_millis(50),
                tools_done: &|| false,
                most_calls: 12,
            },
        );
        assert!(result.unwrap_err().to_string().contains("stopped"));
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(session
            .child
            .0
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .try_wait()
            .unwrap()
            .is_some());
    }

    #[cfg(unix)]
    #[test]
    fn native_claude_reports_an_early_exit_without_waiting_for_the_quiet_limit() {
        let dir = tempfile::TempDir::new().unwrap();
        let bin = script(dir.path(), "read -r _line\nexit 1");
        let cfg = config(ProviderKind::ClaudeCode, &bin, None);
        let agent = AgentCli::new("claude_code".into(), Agent::ClaudeCode, &cfg)
            .with_limits(Duration::from_secs(30), Duration::from_secs(30));
        let tools = vec![super::super::mcp::Tool {
            name: "search_notes".into(),
            description: "Search notes.".into(),
            schema: serde_json::json!({ "type": "object", "properties": {} }),
        }];
        let mut session = agent
            .claude_native("Use interpretable language.", tools)
            .unwrap();
        let started = Instant::now();
        match session.say_with("hello", &mut |_| {}, &mut |_, _| String::new(), &mut || {}) {
            Err(ProviderError::Retryable(message)) => {
                assert!(message.contains("ended the session"), "{message}");
            }
            other => panic!("expected session-ended error, got {other:?}"),
        }
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[cfg(unix)]
    #[test]
    fn failed_feature_discovery_never_launches_a_writing_request() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::TempDir::new().unwrap();
        let bin = dir.path().join("codex");
        let marker = dir.path().join("writing-started");
        std::fs::write(
            &bin,
            format!(
                "#!/bin/sh\nif [ \"$1\" = features ]; then exit 1; fi\ntouch '{}'\n",
                marker.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o700)).unwrap();
        let cfg = config(ProviderKind::Codex, bin.to_str().unwrap(), None);
        assert!(AgentCli::new("codex".into(), Agent::Codex, &cfg)
            .complete(&request())
            .is_err());
        assert!(!marker.exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_cli_that_stops_answering_is_stopped_and_tried_once_more() {
        let dir = tempfile::TempDir::new().unwrap();
        let quiet = script(
            dir.path(),
            "[ \"$1\" = features ] && exit 0\ncat >/dev/null\nsleep 30",
        );
        let cfg = config(ProviderKind::ClaudeCode, &quiet, None);
        let agent = AgentCli::new("claude_code".into(), Agent::ClaudeCode, &cfg)
            .with_limits(Duration::from_millis(300), Duration::from_secs(20));
        let started = Instant::now();
        match agent.complete(&request()) {
            Err(ProviderError::Retryable(message)) => assert!(
                message.ends_with("stopped answering and was stopped"),
                "{message}"
            ),
            other => panic!("expected a stop, got {other:?}"),
        }
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );

        let slow = script(
            dir.path(),
            "[ \"$1\" = features ] && exit 0\ncat >/dev/null\nsleep 30\necho late",
        );
        let cfg = config(ProviderKind::Codex, &slow, None);
        let agent = AgentCli::new("codex".into(), Agent::Codex, &cfg)
            .with_limits(Duration::from_millis(100), Duration::from_millis(400));
        let started = Instant::now();
        assert!(agent.complete(&request()).is_err());
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );

        let talkative = script(
            dir.path(),
            r##"cat >/dev/null
for i in 1 2 3 4; do printf '%s\n' '{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"x"}}}'; sleep 0.15; done
printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"xxxx"}'"##,
        );
        let cfg = config(ProviderKind::ClaudeCode, &talkative, None);
        let agent = AgentCli::new("claude_code".into(), Agent::ClaudeCode, &cfg)
            .with_limits(Duration::from_millis(400), Duration::from_secs(20));
        assert_eq!(
            agent.complete(&request()).unwrap(),
            "xxxx",
            "steady output is never cut off"
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_empty_answer_is_asked_for_once_more_before_giving_up() {
        let dir = tempfile::TempDir::new().unwrap();
        let mark = dir.path().join("tried");
        let bin = script(
            dir.path(),
            &format!(
                "[ \"$1\" = features ] && exit 0\ncat >/dev/null\nif [ -f '{0}' ]; then printf '%s\\n' '{{\"type\":\"item.completed\",\"item\":{{\"type\":\"agent_message\",\"text\":\"hello\"}}}}'; else touch '{0}'; fi",
                mark.display()
            ),
        );
        let cfg = config(ProviderKind::Codex, &bin, None);
        let agent = AgentCli::new("codex".into(), Agent::Codex, &cfg);
        assert_eq!(agent.complete(&request()).unwrap(), "hello");

        let silent = script(
            dir.path(),
            "[ \"$1\" = features ] && exit 0\ncat >/dev/null",
        );
        let cfg = config(ProviderKind::Codex, &silent, None);
        let agent = AgentCli::new("codex".into(), Agent::Codex, &cfg);
        match agent.complete(&request()) {
            Err(ProviderError::Retryable(message)) => {
                assert!(message.ends_with("gave no answer"), "{message}")
            }
            other => panic!("expected no answer, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_failure_moves_on_to_the_next_provider_and_says_how_to_sign_in() {
        let dir = tempfile::TempDir::new().unwrap();
        let bin = script(
            dir.path(),
            "cat >/dev/null\necho 'Not logged in' >&2\nexit 1",
        );
        let cfg = config(ProviderKind::Codex, &bin, None);
        let agent = AgentCli::new("codex".into(), Agent::Codex, &cfg);
        match agent.complete(&request()) {
            Err(ProviderError::Retryable(message)) => {
                assert!(message.contains("Not logged in"), "{message}");
                assert!(message.contains("codex login"), "{message}");
            }
            other => panic!("expected a retryable failure, got {other:?}"),
        }
    }

    #[test]
    fn claude_code_says_which_model_answered_and_how_many_tokens_it_took() {
        let stream = [
            r##"{"type":"system","subtype":"init","model":"claude-sonnet-5-5"}"##,
            r##"{"type":"result","subtype":"success","is_error":false,"result":"hi","usage":{"input_tokens":12,"cache_read_input_tokens":3000,"cache_creation_input_tokens":40,"output_tokens":7}}"##,
        ]
        .join("\n");
        let heard = read_stream(stream.as_bytes(), &mut |_| {});
        assert_eq!(heard.model.as_deref(), Some("claude-sonnet-5-5"));
        assert_eq!(heard.tokens, Some((3052, 7)));
        assert_eq!(heard.cache, (3000, 40));
    }

    #[test]
    fn codex_says_its_model_and_effort_on_stderr() {
        let banner = "OpenAI Codex v0.130\n--------\nworkdir: /tmp/x\nmodel: gpt-6.1-sol\nprovider: openai\napproval: never\nsandbox: read-only\nreasoning effort: high\nreasoning summaries: auto\n--------\nuser\nhi\ntokens used\n12,345\n";
        assert_eq!(
            codex_banner(banner),
            (
                Some("gpt-6.1-sol".into()),
                Some("high".into()),
                Some(12_345)
            )
        );
        assert_eq!(
            codex_banner("tokens used: 99\nreasoning effort: none"),
            (None, None, Some(99))
        );
        assert_eq!(codex_banner(""), (None, None, None));
    }

    #[test]
    fn an_effort_setting_reaches_both_programs_and_odd_values_mean_medium() {
        let req = ChatRequest {
            system: None,
            prompt: "p".into(),
            temperature: 0.2,
            max_tokens: 10,
        };
        let mut cfg = config(ProviderKind::ClaudeCode, "claude", None);
        cfg.effort = Some(" High ".into());
        let (args, _) = AgentCli::new("claude_code".into(), Agent::ClaudeCode, &cfg).arguments(
            &req,
            &[],
            &[],
            &[],
        );
        assert!(args.windows(2).any(|w| w == ["--effort", "high"]));
        let mut cfg = config(ProviderKind::Codex, "codex", None);
        cfg.effort = Some("medium".into());
        let (args, _) =
            AgentCli::new("codex".into(), Agent::Codex, &cfg).arguments(&req, &[], &[], &[]);
        assert!(args
            .windows(2)
            .any(|w| w == ["-c", "model_reasoning_effort=\"medium\""]));
        cfg.effort = Some("high\"; rm".into());
        let (args, _) =
            AgentCli::new("codex".into(), Agent::Codex, &cfg).arguments(&req, &[], &[], &[]);
        assert!(args
            .windows(2)
            .any(|w| w == ["-c", "model_reasoning_effort=\"medium\""]));
        assert!(!args.iter().any(|a| a.contains("rm")));
    }
}
