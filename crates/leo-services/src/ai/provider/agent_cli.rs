use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::ai::error::{ProviderError, ProviderResult};
use crate::ai::provider::{ChatProvider, ChatRequest, Image, Sink, Spent};
use crate::config::provider::{ProviderConfig, ProviderKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    let names: Vec<String> = if cfg!(windows) {
        vec![format!("{bin}.exe"), format!("{bin}.cmd"), bin.to_string()]
    } else {
        vec![bin.to_string()]
    };
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .find_map(|dir| names.iter().map(|n| dir.join(n)).find(|p| p.is_file()))
}

const CODEX_OFF: [&str; 16] = [
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

pub(crate) fn codex_disables(program: &Path) -> Vec<String> {
    static KNOWN: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<PathBuf, Vec<String>>>,
    > = std::sync::OnceLock::new();
    let cache = KNOWN.get_or_init(Default::default);
    if let Some(found) = cache.lock().ok().and_then(|c| c.get(program).cloned()) {
        return found;
    }
    let listing = Command::new(program)
        .args(["features", "list"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let found = known_features(&listing);
    if let Ok(mut c) = cache.lock() {
        c.insert(program.to_path_buf(), found.clone());
    }
    found
}

pub(crate) fn stop_all(child: &mut std::process::Child) {
    #[cfg(unix)]
    if let Ok(group) = libc::pid_t::try_from(child.id()) {
        unsafe {
            libc::kill(-group, libc::SIGKILL);
        }
    }
    let _ = child.kill();
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
            effort: cfg
                .effort
                .clone()
                .map(|e| e.trim().to_lowercase())
                .filter(|e| !e.is_empty() && e.chars().all(|c| c.is_ascii_alphanumeric())),
            quiet_limit: QUIET_LIMIT,
            total_limit: TOTAL_LIMIT,
            spent: Mutex::new(None),
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
                if let Some(system) = &req.system {
                    args.extend(["--system-prompt".to_string(), system.clone()]);
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
                    ]
                    .map(String::from),
                );
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

pub fn read_stream(lines: impl BufRead, sink: Sink<'_>) -> Heard {
    let mut heard = Heard::default();
    for line in lines.lines() {
        let Ok(line) = line else { break };
        hear(&line, &mut heard, sink);
    }
    heard
}

pub fn hear(line: &str, heard: &mut Heard, sink: Sink<'_>) -> bool {
    let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else {
        return false;
    };
    match event["type"].as_str() {
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
            }
            true
        }
        _ => false,
    }
}

pub struct ClaudeSession {
    name: String,
    child: std::process::Child,
    stdin: Option<std::process::ChildStdin>,
    lines: std::sync::mpsc::Receiver<String>,
    quiet: Duration,
    model: Option<String>,
    effort: Option<String>,
    _room: tempfile::TempDir,
}

impl ClaudeSession {
    pub fn say(&mut self, text: &str, sink: Sink<'_>) -> ProviderResult<(String, Spent)> {
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
        loop {
            match self.lines.recv_timeout(self.quiet) {
                Ok(line) => {
                    if hear(&line, &mut heard, sink) {
                        break;
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    stop_all(&mut self.child);
                    return Err(ProviderError::Retryable(format!(
                        "{}: claude stopped answering and was stopped",
                        self.name
                    )));
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
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
                )))
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
            },
        ))
    }
}

impl Drop for ClaudeSession {
    fn drop(&mut self) {
        self.stdin.take();
        stop_all(&mut self.child);
        let _ = self.child.wait();
    }
}

impl AgentCli {
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

    pub fn session_arguments(&self, system: &str) -> Vec<String> {
        let mut args: Vec<String> = [
            "-p",
            "--safe-mode",
            "--tools",
            "WebSearch",
            "--allowedTools",
            "WebSearch",
            "--no-session-persistence",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--include-partial-messages",
            "--verbose",
        ]
        .map(String::from)
        .to_vec();
        if let Some(model) = &self.model {
            args.extend(["--model".to_string(), model.clone()]);
        }
        if let Some(effort) = &self.effort {
            args.extend(["--effort".to_string(), effort.clone()]);
        }
        args.extend(["--system-prompt".to_string(), system.to_string()]);
        args
    }

    pub fn claude_session(&self, system: &str) -> ProviderResult<ClaudeSession> {
        let program = locate(&self.bin).ok_or_else(|| {
            ProviderError::Retryable(format!("{}: {} is not installed", self.name, self.bin))
        })?;
        let room = self.room()?;
        let mut command = Command::new(program);
        command
            .args(self.session_arguments(system))
            .current_dir(room.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        let mut child = command.spawn().map_err(|e| {
            ProviderError::Retryable(format!("{}: could not run {}: {e}", self.name, self.bin))
        })?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().ok_or_else(|| {
            ProviderError::Retryable(format!("{}: could not read {}", self.name, self.bin))
        })?;
        let (tx, lines) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Ok(ClaudeSession {
            name: self.name.clone(),
            child,
            stdin,
            lines,
            quiet: self.quiet_limit,
            model: self.model.clone(),
            effort: self.effort.clone(),
            _room: room,
        })
    }

    fn run(&self, req: &ChatRequest, images: &[Image], sink: Sink<'_>) -> ProviderResult<String> {
        let program =
            locate(&self.bin).ok_or_else(|| ProviderError::Retryable(self.unavailable_reason()))?;
        let disables = match self.agent {
            Agent::Codex => codex_disables(&program),
            Agent::ClaudeCode => Vec::new(),
        };
        let room = self.room()?;
        let files = self.image_files(room.path(), images)?;
        let (args, input) = self.arguments(req, images, &disables, &files);
        match self.spawn(&program, room.path(), &args, input.clone(), sink) {
            Err(ProviderError::Retryable(message))
                if message == self.no_answer() || message == self.stalled() =>
            {
                self.spawn(&program, room.path(), &args, input, sink)
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
        };
        if let Some((input, output)) = heard.tokens {
            (spent.input, spent.output, spent.estimated) = (input, output, false);
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

    fn spawn(
        &self,
        program: &Path,
        room: &Path,
        args: &[String],
        input: String,
        sink: Sink<'_>,
    ) -> ProviderResult<String> {
        let mut command = Command::new(program);
        command
            .args(args)
            .current_dir(room)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        let input_len = input.chars().count();
        let mut child = command.spawn().map_err(|e| {
            ProviderError::Retryable(format!("{}: could not run {}: {e}", self.name, self.bin))
        })?;
        let started = Instant::now();
        let last = Arc::new(Mutex::new(started));
        let stdin = child.stdin.take();
        let writer = std::thread::spawn(move || {
            if let Some(mut stdin) = stdin {
                let _ = stdin.write_all(input.as_bytes());
            }
        });
        let stderr = child.stderr.take().map(|inner| Watched {
            inner,
            last: Arc::clone(&last),
        });
        let errors = std::thread::spawn(move || {
            let mut text = String::new();
            if let Some(mut stderr) = stderr {
                let _ = stderr.read_to_string(&mut text);
            }
            text
        });
        let stdout = child.stdout.take().map(|inner| Watched {
            inner,
            last: Arc::clone(&last),
        });
        let child = Arc::new(Mutex::new(child));
        let finished = Arc::new(AtomicBool::new(false));
        let stalled = Arc::new(AtomicBool::new(false));
        let watchdog = {
            let (child, finished, stalled, last) = (
                Arc::clone(&child),
                Arc::clone(&finished),
                Arc::clone(&stalled),
                Arc::clone(&last),
            );
            let quiet = (self.agent == Agent::ClaudeCode).then_some(self.quiet_limit);
            let total = self.total_limit;
            std::thread::spawn(move || {
                while !finished.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(100));
                    let silent = last.lock().map(|l| l.elapsed()).unwrap_or_default();
                    if started.elapsed() > total || quiet.is_some_and(|q| silent > q) {
                        stalled.store(true, Ordering::Relaxed);
                        if let Ok(mut child) = child.lock() {
                            stop_all(&mut child);
                        }
                        return;
                    }
                }
            })
        };
        let heard = match (self.agent, stdout) {
            (Agent::ClaudeCode, Some(stdout)) => read_stream(BufReader::new(stdout), sink),
            (Agent::Codex, Some(mut stdout)) => {
                let mut answer = String::new();
                let _ = stdout.read_to_string(&mut answer);
                Heard {
                    answer,
                    ..Heard::default()
                }
            }
            (_, None) => Heard::default(),
        };
        finished.store(true, Ordering::Relaxed);
        let _ = watchdog.join();
        let status = child
            .lock()
            .map_err(|_| {
                ProviderError::Retryable(format!("{}: {} did not finish", self.name, self.bin))
            })?
            .wait()
            .map_err(|e| {
                ProviderError::Retryable(format!("{}: {} did not finish: {e}", self.name, self.bin))
            })?;
        if stalled.load(Ordering::Relaxed) {
            let _ = writer.join();
            let _ = errors.join();
            return Err(ProviderError::Retryable(self.stalled()));
        }
        let _ = writer.join();
        let stderr = errors.join().unwrap_or_default();
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
        if self.agent == Agent::Codex {
            sink(&answer);
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
        assert!(args
            .windows(2)
            .any(|w| w == ["--system-prompt", "You write notes."]));
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
        assert!(!args.contains(&"--model".to_string()));
        assert_eq!(args.last().unwrap(), "-");
        assert_eq!(input, "You write notes.\n\n<transcript>hello</transcript>");
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
        let listing = "apps                 stable   true\nshell_tool           stable   true\nbrowser_use          stable   true\nsomething_else       stable   false\n";
        assert_eq!(
            known_features(listing),
            ["shell_tool", "browser_use", "apps"]
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
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.to_string_lossy().to_string()
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
    fn a_claude_session_stays_open_and_answers_each_message_in_turn() {
        let dir = tempfile::TempDir::new().unwrap();
        let log = dir.path().join("heard.log");
        let bin = script(
            dir.path(),
            &format!(
                r##"echo "$@" > "{log}.args"
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
        assert!(args.contains("--system-prompt You are Felix."));
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
        let bin = script(dir.path(), "cat >/dev/null\necho '  # Notes'\necho 'hello'");
        let cfg = config(ProviderKind::Codex, &bin, None);
        let agent = AgentCli::new("codex".into(), Agent::Codex, &cfg);
        let mut pieces = Vec::new();
        let answer = agent
            .complete_streaming(&request(), &mut |p| pieces.push(p.to_string()))
            .unwrap();
        assert_eq!(answer, "# Notes\nhello");
        assert_eq!(pieces, ["# Notes\nhello"]);
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
                "[ \"$1\" = features ] && exit 0\ncat >/dev/null\nif [ -f '{0}' ]; then echo hello; else touch '{0}'; fi",
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
    fn an_effort_setting_reaches_both_programs_and_odd_values_are_dropped() {
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
        assert!(!args.iter().any(|a| a.contains("model_reasoning_effort")));
    }
}
