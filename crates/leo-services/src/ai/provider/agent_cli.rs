use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::ai::error::{ProviderError, ProviderResult};
use crate::ai::provider::{ChatProvider, ChatRequest, Image, Sink};
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

const CODEX_OFF: [&str; 6] = [
    "shell_tool",
    "browser_use",
    "browser_use_external",
    "computer_use",
    "in_app_browser",
    "apps",
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

fn codex_disables(program: &Path) -> Vec<String> {
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

pub struct AgentCli {
    name: String,
    agent: Agent,
    bin: String,
    model: Option<String>,
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
        }
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
                        "",
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

    fn image_files(&self, images: &[Image]) -> ProviderResult<Vec<PathBuf>> {
        if self.agent != Agent::Codex || images.is_empty() {
            return Ok(Vec::new());
        }
        static COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let batch = COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut files = Vec::new();
        for (i, image) in images.iter().enumerate() {
            let path = Self::workspace().join(format!(
                "leo-image-{}-{batch}-{i}.{}",
                std::process::id(),
                image.extension()
            ));
            std::fs::write(&path, &image.bytes).map_err(|e| {
                ProviderError::Retryable(format!(
                    "{}: could not hand the image over: {e}",
                    self.name
                ))
            })?;
            files.push(path);
        }
        Ok(files)
    }

    fn workspace() -> PathBuf {
        let dir = std::env::temp_dir().join("leo-writing");
        let _ = std::fs::create_dir_all(&dir);
        dir
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
}

pub fn read_stream(lines: impl BufRead, sink: Sink<'_>) -> Heard {
    let mut heard = Heard::default();
    for line in lines.lines() {
        let Ok(line) = line else { break };
        let Ok(event) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        match event["type"].as_str() {
            Some("stream_event") => {
                let delta = &event["event"]["delta"];
                if event["event"]["type"] == "content_block_delta" && delta["type"] == "text_delta"
                {
                    if let Some(text) = delta["text"].as_str() {
                        heard.answer.push_str(text);
                        sink(text);
                    }
                }
            }
            Some("rate_limit_event") => {
                if let Some(usage) =
                    crate::usage::from_claude(&event["rate_limit_info"], chrono::Utc::now())
                {
                    heard.usage = Some(usage);
                }
            }
            Some("result") => {
                let failed = event["is_error"].as_bool().unwrap_or(false)
                    || event["subtype"].as_str().is_some_and(|s| s != "success");
                let said = event["result"].as_str().unwrap_or_default().to_string();
                heard.result = Some((failed, said));
            }
            _ => {}
        }
    }
    heard
}

impl AgentCli {
    fn run(&self, req: &ChatRequest, images: &[Image], sink: Sink<'_>) -> ProviderResult<String> {
        let program =
            locate(&self.bin).ok_or_else(|| ProviderError::Retryable(self.unavailable_reason()))?;
        let disables = match self.agent {
            Agent::Codex => codex_disables(&program),
            Agent::ClaudeCode => Vec::new(),
        };
        let files = self.image_files(images)?;
        let (args, input) = self.arguments(req, images, &disables, &files);
        let outcome = self.spawn(&program, &args, input, sink);
        for file in files {
            let _ = std::fs::remove_file(file);
        }
        outcome
    }

    fn spawn(
        &self,
        program: &Path,
        args: &[String],
        input: String,
        sink: Sink<'_>,
    ) -> ProviderResult<String> {
        let mut child = Command::new(program)
            .args(args)
            .current_dir(Self::workspace())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                ProviderError::Retryable(format!("{}: could not run {}: {e}", self.name, self.bin))
            })?;
        let stdin = child.stdin.take();
        let writer = std::thread::spawn(move || {
            if let Some(mut stdin) = stdin {
                let _ = stdin.write_all(input.as_bytes());
            }
        });
        let stderr = child.stderr.take();
        let errors = std::thread::spawn(move || {
            let mut text = String::new();
            if let Some(mut stderr) = stderr {
                let _ = stderr.read_to_string(&mut text);
            }
            text
        });
        let heard = match (self.agent, child.stdout.take()) {
            (Agent::ClaudeCode, Some(stdout)) => read_stream(BufReader::new(stdout), sink),
            (Agent::Codex, Some(mut stdout)) => {
                let mut answer = String::new();
                let _ = stdout.read_to_string(&mut answer);
                Heard {
                    answer,
                    result: None,
                    usage: None,
                }
            }
            (_, None) => Heard::default(),
        };
        let status = child.wait().map_err(|e| {
            ProviderError::Retryable(format!("{}: {} did not finish: {e}", self.name, self.bin))
        })?;
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
            return Err(ProviderError::Retryable(format!(
                "{}: {} gave no answer",
                self.name, self.bin
            )));
        }
        if self.agent == Agent::Codex {
            sink(&answer);
        }
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
    fn claude_code_writes_with_no_tools_and_reads_the_material_from_stdin() {
        let cfg = config(
            ProviderKind::ClaudeCode,
            "claude",
            Some("claude-sonnet-5-5"),
        );
        let agent = AgentCli::new("claude_code".into(), Agent::of(&cfg).unwrap(), &cfg);
        let (args, input) = agent.arguments(&request(), &[], &[], &[]);
        assert_eq!(
            &args[..5],
            [
                "-p",
                "--safe-mode",
                "--tools",
                "",
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
        assert!(args.windows(2).any(|w| w == ["--tools", ""]));
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
}
