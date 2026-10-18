use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};

use super::agent_cli::{locate, probe_command, Agent};
use super::{process, ChatRequest};
use crate::ai::error::{ProviderError, ProviderResult};

const MOST_CONFIG: u64 = 1024 * 1024;
const MOST_PROFILE_ENTRIES: usize = 10_000;
const MOST_PROFILE_DEPTH: usize = 32;
const MOST_PROGRAMS: usize = 32;
const MOST_EVENTS: usize = 100_000;
const SIGN_IN_LIMIT: Duration = Duration::from_secs(15 * 60);
const SIGN_IN_POLL: Duration = Duration::from_millis(25);
const PROFILE_WAIT: Duration = Duration::from_secs(30);
const PROFILE_POLL: Duration = Duration::from_millis(25);
const MOST_PROFILE_WAITERS: usize = 4;

struct ProfileQueue {
    waiting: AtomicUsize,
}

impl ProfileQueue {
    const fn new() -> Self {
        Self {
            waiting: AtomicUsize::new(0),
        }
    }

    fn enter(&self) -> Option<ProfileWaiter<'_>> {
        self.waiting
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < MOST_PROFILE_WAITERS).then_some(count + 1)
            })
            .ok()
            .map(|_| ProfileWaiter(self))
    }
}

struct ProfileWaiter<'a>(&'a ProfileQueue);

impl Drop for ProfileWaiter<'_> {
    fn drop(&mut self) {
        self.0.waiting.fetch_sub(1, Ordering::AcqRel);
    }
}

fn profile_queue(agent: Agent) -> &'static ProfileQueue {
    static AGY: ProfileQueue = ProfileQueue::new();
    static COPILOT: ProfileQueue = ProfileQueue::new();
    if agent == Agent::Agy {
        &AGY
    } else {
        &COPILOT
    }
}

fn wait_for_profile(
    agent: Agent,
    queue: &ProfileQueue,
    wait: Duration,
    mut acquire: impl FnMut() -> Result<File>,
    stopping: impl Fn() -> bool,
) -> Result<File> {
    let name = match agent {
        Agent::Agy => "Google Antigravity",
        Agent::Copilot => "GitHub Copilot",
        _ => "The writing CLI",
    };
    ensure!(
        !stopping(),
        "Leo is shutting down. Start Leo again before requesting more writing."
    );
    match acquire() {
        Ok(file) => {
            ensure!(
                !stopping(),
                "Leo is shutting down. Start Leo again before requesting more writing."
            );
            return Ok(file);
        }
        Err(error) if crate::private_file::busy(&error) => {}
        Err(error) => return Err(error),
    }
    let _waiting = queue.enter().with_context(|| format!("{name} already has several writing requests waiting. Let the current note, import or Felix answer finish, then try again."))?;
    let until = Instant::now() + wait;
    loop {
        ensure!(
            !stopping(),
            "Leo is shutting down. Start Leo again before requesting more writing."
        );
        let left = until.checked_duration_since(Instant::now())
            .filter(|left| !left.is_zero())
            .with_context(|| format!("{name} is still busy with another note, import, Felix answer or sign-in. Let that work finish or stop it, then try again."))?;
        std::thread::sleep(PROFILE_POLL.min(left));
        ensure!(
            !stopping(),
            "Leo is shutting down. Start Leo again before requesting more writing."
        );
        match acquire() {
            Ok(file) => {
                ensure!(
                    !stopping(),
                    "Leo is shutting down. Start Leo again before requesting more writing."
                );
                return Ok(file);
            }
            Err(error) if crate::private_file::busy(&error) => {}
            Err(error) => return Err(error),
        }
    }
}

fn scoped(agent: Agent) -> bool {
    matches!(agent, Agent::Agy | Agent::Copilot)
}

pub fn profile_root() -> Result<PathBuf> {
    let config = crate::config::Config::config_path()?;
    let parent = config
        .parent()
        .context("Leo needs a private settings folder")?;
    leo_core::paths::contained_path(parent, Path::new("agent-profiles"))
}

fn profile(agent: Agent) -> Result<PathBuf> {
    ensure!(
        scoped(agent),
        "Only AGY and GitHub Copilot use an isolated sign-in profile"
    );
    let root = profile_root()?;
    crate::private_file::directory(&root)?;
    let path = leo_core::paths::contained_path(&root, Path::new(agent.program()))?;
    crate::private_file::directory(&path)?;
    Ok(path)
}

fn empty(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(false) => true,
        Value::Object(map) => map.values().all(empty),
        Value::Array(items) => items.iter().all(empty),
        _ => false,
    }
}

fn no_customizations(home: &Path, relative: &str) -> Result<()> {
    use std::io::Read;
    let path = leo_core::paths::contained_path(home, Path::new(relative))?;
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if meta.is_dir() {
        ensure!(std::fs::read_dir(path)?.next().is_none(), "This writing profile has plugins or hooks. Clear its profile in Storage and sign in again.");
        return Ok(());
    }
    ensure!(meta.is_file() && meta.len() <= MOST_CONFIG, "This writing profile has an unsafe customization file. Clear its profile in Storage and sign in again.");
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let mut bytes = Vec::new();
    options
        .open(path)?
        .take(MOST_CONFIG + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MOST_CONFIG,
        "The writing profile customization is too large"
    );
    if bytes.is_empty() {
        return Ok(());
    }
    let value: Value = serde_json::from_slice(&bytes).context("This writing profile has damaged customization settings. Clear its profile in Storage and sign in again.")?;
    ensure!(empty(&value), "This writing profile has MCP servers, plugins or hooks. Clear its profile in Storage and sign in again.");
    Ok(())
}

fn check_profile(agent: Agent, home: &Path) -> Result<()> {
    let roots: &[&str] = if agent == Agent::Agy {
        &[".gemini/config", ".gemini/antigravity-cli"]
    } else {
        &[""]
    };
    for root in roots {
        for name in [
            "mcp_config.json",
            "mcp-config.json",
            "mcp.json",
            "hooks.json",
            "plugins.json",
            "hooks",
            "plugins",
            "agents",
            "skills",
            "installed-plugins",
            "extensions",
            "commands",
            "lsp-config.json",
            "providers.json",
            "agents.json",
            "skills.json",
        ] {
            let relative = if root.is_empty() {
                name.to_string()
            } else {
                format!("{root}/{name}")
            };
            no_customizations(home, &relative)?;
        }
    }
    if agent == Agent::Agy {
        no_customizations(home, ".gemini/config/settings.json")?;
    }
    Ok(())
}

fn write_private(path: &Path, text: &str) -> Result<()> {
    use std::io::Write;
    let parent = path
        .parent()
        .context("The writing configuration needs a folder")?;
    crate::private_file::directory(parent)?;
    let name = path
        .file_name()
        .context("The writing configuration needs a name")?;
    let path = leo_core::paths::contained_path(parent, Path::new(name))?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(text.as_bytes())?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn prepare_in(agent: Agent, home: &Path, room: &Path, system: Option<&str>) -> Result<File> {
    let lock = crate::private_file::lock(home, ".leo-profile.lock", Duration::ZERO)?;
    prepare_locked(agent, home, room, system, lock)
}

fn prepare_locked(
    agent: Agent,
    home: &Path,
    room: &Path,
    system: Option<&str>,
    lock: File,
) -> Result<File> {
    check_profile(agent, home)?;
    if agent == Agent::Agy {
        let settings = json!({
            "toolPermission":"strict", "enableTerminalSandbox":true,
            "enableTelemetry":false, "useG1Credits":false,
            "permissions":{"deny":["command(*)","unsandboxed(*)","read_file(*)","write_file(*)","read_url(*)","mcp(*)"]}
        });
        let path = leo_core::paths::contained_path(
            home,
            Path::new(".gemini/antigravity-cli/settings.json"),
        )?;
        write_private(&path, &settings.to_string())?;
        let agent_file = room.join(".agents/agents/leo.md");
        write_private(&agent_file, &format!("---\nname: leo\ndescription: Leo note writing with only the supplied text tool protocol\nmainAgent: true\nsubagent: false\ntools: []\nmcpServers: []\nskills: []\nplugins: []\ninheritCustomizations: false\ninheritMcp: false\ncommandExecutionPolicy: \"off\"\n---\n\nUse interpretable language.\n{}", system.unwrap_or_default()))?;
    } else {
        let path = leo_core::paths::contained_path(home, Path::new("settings.json"))?;
        write_private(&path, &json!({"disableAllHooks":true}).to_string())?;
    }
    Ok(lock)
}

pub(crate) fn prepare(
    agent: Agent,
    room: &Path,
    system: Option<&str>,
    images: bool,
) -> ProviderResult<Option<File>> {
    if !scoped(agent) {
        return Ok(None);
    }
    if images {
        return Err(ProviderError::Retryable("This writing CLI accepts text only in Leo. Choose Codex or Claude Code to read scans and pictures.".into()));
    }
    let home = profile(agent).map_err(failure)?;
    let lock = wait_for_profile(
        agent,
        profile_queue(agent),
        PROFILE_WAIT,
        || crate::private_file::lock(&home, ".leo-profile.lock", Duration::ZERO),
        process::stopping,
    )
    .map_err(failure)?;
    prepare_locked(agent, &home, room, system, lock)
        .map(Some)
        .map_err(failure)
}

fn failure(error: impl std::fmt::Display) -> ProviderError {
    ProviderError::Retryable(format!(
        "The isolated writing profile could not be used: {error}"
    ))
}

pub(crate) fn clean_environment(command: &mut Command) {
    command.env_clear();
    for name in [
        "PATH",
        "HOME",
        "USERPROFILE",
        "USER",
        "LOGNAME",
        "TMPDIR",
        "TEMP",
        "TMP",
        "SYSTEMROOT",
        "WINDIR",
        "COMSPEC",
        "PATHEXT",
        "APPDATA",
        "LOCALAPPDATA",
        "XDG_RUNTIME_DIR",
        "LANG",
        "LC_ALL",
        "LC_CTYPE",
        "TERM",
        "COLORTERM",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
        "SYSTEMDRIVE",
        "HOMEDRIVE",
        "HOMEPATH",
    ] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
}

pub(crate) fn plan_environment(command: &mut Command) {
    clean_environment(command);
    for name in [
        "XDG_CONFIG_HOME",
        "XDG_CACHE_HOME",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
        "CODEX_HOME",
        "CODEX_SQLITE_HOME",
        "CLAUDE_CONFIG_DIR",
        "DBUS_SESSION_BUS_ADDRESS",
        "CODEX_CA_CERTIFICATE",
        "NODE_EXTRA_CA_CERTS",
    ] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
}

fn environment(command: &mut Command, agent: Agent, home: &Path) {
    clean_environment(command);
    command
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("APPDATA", home.join("AppData/Roaming"))
        .env("LOCALAPPDATA", home.join("AppData/Local"));
    if agent == Agent::Copilot {
        command
            .env("COPILOT_HOME", home)
            .env("COPILOT_CACHE_HOME", home.join("cache"))
            .env("COPILOT_AUTO_UPDATE", "false")
            .env("COPILOT_ALLOW_ALL", "false")
            .env("COPILOT_MCP_TOOL_CACHE", "false");
    }
}

pub(crate) fn configure(command: &mut Command, agent: Agent) -> ProviderResult<()> {
    if scoped(agent) {
        let home = profile(agent).map_err(failure)?;
        environment(command, agent, &home);
    } else {
        plan_environment(command);
    }
    Ok(())
}

fn required_flags(agent: Agent) -> &'static [&'static str] {
    match agent {
        Agent::Agy => &[
            "--agent",
            "--sandbox",
            "--disable-slash-commands",
            "--input-format",
            "--output-format",
            "--print-timeout",
        ],
        Agent::Copilot => &[
            "--available-tools",
            "--disable-builtin-mcps",
            "--no-custom-instructions",
            "--no-auto-update",
            "--no-remote-export",
            "--no-ask-user",
        ],
        _ => &[],
    }
}

pub(crate) fn supported(agent: Agent, program: &Path) -> bool {
    if !scoped(agent) {
        return true;
    }
    type Stamp = (u64, Option<std::time::SystemTime>);
    type Cache = HashMap<(Agent, PathBuf), (Option<Stamp>, bool)>;
    static CHECKED: OnceLock<Mutex<Cache>> = OnceLock::new();
    let checked = CHECKED.get_or_init(Default::default);
    let key = (agent, program.to_path_buf());
    let stamp = std::fs::metadata(program)
        .ok()
        .map(|meta| (meta.len(), meta.modified().ok()));
    if let Some(value) = checked.lock().ok().and_then(|map| {
        map.get(&key)
            .filter(|(seen, _)| *seen == stamp)
            .map(|(_, good)| *good)
    }) {
        return value;
    }
    let good = tempfile::Builder::new()
        .prefix("leo-probe-")
        .tempdir()
        .ok()
        .and_then(|room| {
            let mut command = Command::new(program);
            command.arg("--help").current_dir(room.path());
            environment(&mut command, agent, room.path());
            probe_command(command).ok()
        })
        .is_some_and(|help| required_flags(agent).iter().all(|flag| help.contains(flag)));
    if let Ok(mut map) = checked.lock() {
        if map.len() >= MOST_PROGRAMS {
            if let Some(old) = map.keys().next().cloned() {
                map.remove(&old);
            }
        }
        map.insert(key, (stamp, good));
    }
    good
}

pub(crate) fn arguments(
    agent: Agent,
    request: &ChatRequest,
    model: Option<&str>,
    effort: Option<&str>,
) -> (Vec<String>, String) {
    let (mut args, input) = if agent == Agent::Agy {
        (
            vec![
                "--input-format",
                "stream-json",
                "--output-format",
                "stream-json",
                "--agent",
                "leo",
                "--sandbox",
                "--disable-slash-commands",
                "--print-timeout",
                "15m",
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>(),
            format!(
                "{}\n",
                json!({"event":"user","message":{"content":request.prompt}})
            ),
        )
    } else {
        (
            vec![
                "--silent",
                "--no-color",
                "--no-ask-user",
                "--no-custom-instructions",
                "--no-auto-update",
                "--no-remote-export",
                "--disable-builtin-mcps",
                "--available-tools=web_fetch",
                "--deny-tool=read",
                "--deny-tool=write",
                "--deny-tool=shell",
                "--deny-tool=memory",
                "--deny-tool=url",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
            format!(
                "Use interpretable language.\n{}\n\n{}",
                request.system.as_deref().unwrap_or_default(),
                request.prompt
            ),
        )
    };
    if let Some(model) = model.filter(|model| *model != "default") {
        args.extend(["--model".into(), model.into()]);
    }
    if let Some(effort) = effort {
        args.extend(["--effort".into(), effort.into()]);
    }
    (args, input)
}

pub(crate) fn answer(agent: Agent, output: &str) -> ProviderResult<String> {
    if agent == Agent::Copilot {
        return Ok(output.trim().to_string());
    }
    let mut result = None;
    for line in output.lines().take(MOST_EVENTS) {
        if line.len() > process::MOST_LINE {
            return Err(failure(
                "The writing CLI returned too much text. Try a shorter request.",
            ));
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if value["event"] == "result" {
            if result.is_some() {
                return Err(failure(
                    "The writing CLI returned more than one answer. Try again.",
                ));
            }
            result = Some(value["result"].clone());
        }
    }
    let result = result.ok_or_else(|| failure("AGY did not return a completed answer. Sign in with `leo doctor --sign-in agy`, then try again."))?;
    if result["status"] != "SUCCESS" {
        return Err(failure("AGY could not finish. Check your sign-in with `leo doctor --sign-in agy`, then try again."));
    }
    result["response"]
        .as_str()
        .filter(|text| !text.trim().is_empty())
        .map(|text| text.trim().to_string())
        .ok_or_else(|| failure("AGY returned no text. Try again."))
}

pub fn sign_in(name: &str) -> Result<()> {
    use std::io::IsTerminal;
    ensure!(
        std::io::stdin().is_terminal(),
        "Open a terminal and run `leo doctor --sign-in {name}` to sign in"
    );
    let agent = match name {
        "agy" => Agent::Agy,
        "copilot" => Agent::Copilot,
        _ => anyhow::bail!("Choose `agy` or `copilot` for isolated sign-in"),
    };
    let program = locate(agent.program()).context(agent.install())?;
    ensure!(
        supported(agent, &program),
        "Install a current {} with its isolation controls first: {}",
        agent.program(),
        agent.install()
    );
    let room = tempfile::Builder::new().prefix("leo-sign-in-").tempdir()?;
    let home = profile(agent)?;
    let _guard = prepare_in(
        agent,
        &home,
        room.path(),
        Some(
            "Use interpretable language. Sign in, then exit; do not run commands or change files.",
        ),
    )?;
    let mut command = Command::new(program);
    command
        .current_dir(room.path())
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    environment(&mut command, agent, &home);
    if agent == Agent::Copilot {
        command.arg("login");
    } else {
        command.args(["--agent", "leo", "--sandbox", "--disable-slash-commands"]);
    }
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let process = process::Process::new(command.spawn()?);
    #[cfg(unix)]
    let _foreground = {
        let id = process
            .0
            .lock()
            .ok()
            .and_then(|held| held.as_ref().map(|child| child.id()))
            .context("The sign-in process stopped before it could open")?;
        Foreground::give(id)?
    };
    let _watch = process::Watchdog::start(&process, None, SIGN_IN_LIMIT);
    let until = std::time::Instant::now() + SIGN_IN_LIMIT;
    loop {
        if std::time::Instant::now() >= until {
            process.stop();
            anyhow::bail!("Sign-in took too long. Run `leo doctor --sign-in {name}` again.");
        }
        let status = process.0.lock().ok().and_then(|mut held| {
            held.as_mut()
                .and_then(|child| child.try_wait().ok())
                .flatten()
        });
        if let Some(status) = status {
            ensure!(
                status.success(),
                "Sign-in did not finish. Run `leo doctor --sign-in {name}` again."
            );
            return Ok(());
        }
        std::thread::sleep(SIGN_IN_POLL);
    }
}

pub fn clear_profile(name: &str) -> Result<()> {
    let agent = match name {
        "agy" => Agent::Agy,
        "copilot" => Agent::Copilot,
        _ => anyhow::bail!("Choose an AGY or GitHub Copilot profile"),
    };
    let home = profile(agent)?;
    clear_profile_in(&home)
}

fn clear_profile_in(home: &Path) -> Result<()> {
    let _lock = crate::private_file::lock(home, ".leo-profile.lock", Duration::ZERO)?;
    fn checked_leaf(root: &Path, path: &Path) -> Result<PathBuf> {
        let parent = path.parent().context("The profile entry needs a folder")?;
        let checked = leo_core::paths::contained_path(root, parent.strip_prefix(root)?)?;
        Ok(checked.join(path.file_name().context("The profile entry needs a name")?))
    }
    fn collect(root: &Path, here: &Path, depth: usize, paths: &mut Vec<PathBuf>) -> Result<()> {
        ensure!(
            depth <= MOST_PROFILE_DEPTH,
            "The writing profile has too many folder levels; remove it on this computer"
        );
        for entry in std::fs::read_dir(here)? {
            ensure!(
                paths.len() < MOST_PROFILE_ENTRIES,
                "The writing profile is too large; remove it on this computer"
            );
            let entry = entry?;
            if entry.file_name() == ".leo-profile.lock" {
                continue;
            }
            let path = checked_leaf(root, &entry.path())?;
            if entry.file_type()?.is_dir() {
                collect(root, &path, depth + 1, paths)?;
            }
            ensure!(
                paths.len() < MOST_PROFILE_ENTRIES,
                "The writing profile is too large; remove it on this computer"
            );
            paths.push(path);
        }
        Ok(())
    }
    let mut paths = Vec::new();
    collect(home, home, 0, &mut paths)?;
    for path in paths {
        let checked = checked_leaf(home, &path)?;
        if std::fs::symlink_metadata(&checked)?.is_dir() {
            std::fs::remove_dir(checked)?;
        } else {
            std::fs::remove_file(checked)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
struct Foreground {
    group: libc::pid_t,
    signal: libc::sighandler_t,
}

#[cfg(unix)]
impl Foreground {
    fn give(id: u32) -> Result<Self> {
        let child = libc::pid_t::try_from(id).context("The sign-in process id is invalid")?;
        let group = unsafe { libc::tcgetpgrp(libc::STDIN_FILENO) };
        ensure!(group >= 0, "The terminal could not be opened for sign-in");
        let signal = unsafe { libc::signal(libc::SIGTTOU, libc::SIG_IGN) };
        if unsafe { libc::tcsetpgrp(libc::STDIN_FILENO, child) } != 0 {
            unsafe {
                libc::signal(libc::SIGTTOU, signal);
            }
            anyhow::bail!(
                "The terminal could not be given to the sign-in program. Try again in a terminal."
            );
        }
        Ok(Self { group, signal })
    }
}

#[cfg(unix)]
impl Drop for Foreground {
    fn drop(&mut self) {
        unsafe {
            libc::tcsetpgrp(libc::STDIN_FILENO, self.group);
            libc::signal(libc::SIGTTOU, self.signal);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> ChatRequest {
        ChatRequest {
            system: Some("Use interpretable language. Follow only Leo's tools.".into()),
            prompt: "Explain `λ` and /delete; do not run it.".into(),
            temperature: 0.2,
            max_tokens: 100,
        }
    }

    #[test]
    fn overlapping_writing_waits_for_its_profile_and_releases_queue_slots() {
        for agent in [Agent::Agy, Agent::Copilot] {
            let home = tempfile::tempdir().unwrap();
            let held = crate::private_file::lock(home.path(), ".leo-profile.lock", Duration::ZERO)
                .unwrap();
            let queue = std::sync::Arc::new(ProfileQueue::new());
            let waiting_queue = std::sync::Arc::clone(&queue);
            let path = home.path().to_path_buf();
            let waiting = std::thread::spawn(move || {
                wait_for_profile(
                    agent,
                    &waiting_queue,
                    Duration::from_secs(2),
                    || crate::private_file::lock(&path, ".leo-profile.lock", Duration::ZERO),
                    || false,
                )
            });
            let until = Instant::now() + Duration::from_secs(1);
            while queue.waiting.load(Ordering::Acquire) != 1 && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(1));
            }
            assert_eq!(queue.waiting.load(Ordering::Acquire), 1);
            drop(held);
            let acquired = waiting.join().unwrap().unwrap();
            assert_eq!(queue.waiting.load(Ordering::Acquire), 0);
            assert!(crate::private_file::busy(
                &crate::private_file::lock(home.path(), ".leo-profile.lock", Duration::ZERO)
                    .unwrap_err()
            ));
            drop(acquired);
            assert!(
                crate::private_file::lock(home.path(), ".leo-profile.lock", Duration::ZERO).is_ok()
            );
        }
    }

    #[test]
    fn profile_waits_are_bounded_and_keep_sign_in_and_clear_failfast() {
        for agent in [Agent::Agy, Agent::Copilot] {
            let home = tempfile::tempdir().unwrap();
            let room = tempfile::tempdir().unwrap();
            let held = prepare_in(agent, home.path(), room.path(), None).unwrap();
            let queue = ProfileQueue::new();
            let started = Instant::now();
            let error = wait_for_profile(
                agent,
                &queue,
                Duration::from_millis(35),
                || crate::private_file::lock(home.path(), ".leo-profile.lock", Duration::ZERO),
                || false,
            )
            .unwrap_err();
            assert!(started.elapsed() >= Duration::from_millis(35));
            assert!(started.elapsed() < Duration::from_secs(2));
            assert!(error.to_string().contains("still busy"));
            assert!(!error.to_string().contains("Another Leo process"));
            assert_eq!(queue.waiting.load(Ordering::Acquire), 0);
            let started = Instant::now();
            assert!(prepare_in(agent, home.path(), room.path(), None).is_err());
            assert!(clear_profile_in(home.path()).is_err());
            assert!(started.elapsed() < Duration::from_secs(1));
            drop(held);
        }
    }

    #[test]
    fn full_profile_queues_and_shutdown_do_not_start_work_or_leave_slots() {
        for agent in [Agent::Agy, Agent::Copilot] {
            let home = tempfile::tempdir().unwrap();
            let held = crate::private_file::lock(home.path(), ".leo-profile.lock", Duration::ZERO)
                .unwrap();
            let queue = ProfileQueue::new();
            let admitted: Vec<_> = (0..MOST_PROFILE_WAITERS)
                .map(|_| queue.enter().unwrap())
                .collect();
            assert!(queue.enter().is_none());
            let error = wait_for_profile(
                agent,
                &queue,
                Duration::from_secs(2),
                || crate::private_file::lock(home.path(), ".leo-profile.lock", Duration::ZERO),
                || false,
            )
            .unwrap_err();
            assert!(error
                .to_string()
                .contains("several writing requests waiting"));
            assert_eq!(queue.waiting.load(Ordering::Acquire), MOST_PROFILE_WAITERS);
            drop(admitted);
            assert_eq!(queue.waiting.load(Ordering::Acquire), 0);
            let checks = AtomicUsize::new(0);
            let error = wait_for_profile(
                agent,
                &queue,
                Duration::from_secs(2),
                || crate::private_file::lock(home.path(), ".leo-profile.lock", Duration::ZERO),
                || checks.fetch_add(1, Ordering::AcqRel) >= 2,
            )
            .unwrap_err();
            assert!(error.to_string().contains("shutting down"));
            assert_eq!(queue.waiting.load(Ordering::Acquire), 0);
            let called = AtomicUsize::new(0);
            assert!(wait_for_profile(
                agent,
                &queue,
                Duration::from_secs(2),
                || {
                    called.fetch_add(1, Ordering::AcqRel);
                    unreachable!()
                },
                || true
            )
            .is_err());
            assert_eq!(called.load(Ordering::Acquire), 0);
            drop(held);
        }
    }

    #[test]
    fn unsafe_profile_paths_are_never_retried_as_busy() {
        for agent in [Agent::Agy, Agent::Copilot] {
            let queue = ProfileQueue::new();
            let called = AtomicUsize::new(0);
            let error = wait_for_profile(
                agent,
                &queue,
                Duration::from_secs(2),
                || {
                    called.fetch_add(1, Ordering::AcqRel);
                    Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied).into())
                },
                || false,
            )
            .unwrap_err();
            assert_eq!(called.load(Ordering::Acquire), 1);
            assert!(!crate::private_file::busy(&error));
            assert!(!error.to_string().contains("busy"));
            assert_eq!(queue.waiting.load(Ordering::Acquire), 0);
        }
    }

    #[test]
    fn prompts_stay_on_stdin_and_never_become_cli_switches_or_slash_commands() {
        for agent in [Agent::Agy, Agent::Copilot] {
            let (args, input) = arguments(agent, &request(), Some("default"), Some("medium"));
            assert!(!args
                .iter()
                .any(|arg| arg.contains('λ') || arg.contains("/delete")));
            assert!(!args
                .iter()
                .any(|arg| arg.contains("dangerously") || arg == "--allow-all"));
            assert!(input.contains('λ'));
            if agent == Agent::Agy {
                let value: Value = serde_json::from_str(input.trim()).unwrap();
                assert_eq!(value["message"]["content"], request().prompt);
                assert!(args
                    .windows(2)
                    .any(|pair| pair == ["--input-format", "stream-json"]));
            } else {
                assert!(!args.iter().any(|arg| arg == "-p" || arg == "--prompt"));
                assert!(args.iter().any(|arg| arg == "--disable-builtin-mcps"));
            }
        }
    }

    #[test]
    fn only_completed_agy_results_are_shown_and_errors_never_leak_raw_diagnostics() {
        let good = "{\"event\":\"init\"}\n{\"event\":\"result\",\"result\":{\"status\":\"SUCCESS\",\"response\":\" Hello λ \"}}\n";
        assert_eq!(answer(Agent::Agy, good).unwrap(), "Hello λ");
        for bad in ["not json", "{\"event\":\"result\",\"result\":{\"status\":\"ERROR\",\"error\":\"PRIVATE DIAGNOSTIC\"}}", "{\"event\":\"result\",\"result\":{\"status\":\"SUCCESS\",\"response\":3}}", "{\"event\":\"result\",\"result\":{\"status\":\"RUNNING\",\"response\":\"partial\"}}"] {
            let error = answer(Agent::Agy, bad).unwrap_err();
            assert!(!format!("{error:?}").contains("PRIVATE DIAGNOSTIC"));
        }
        assert!(answer(Agent::Agy, &format!("{good}{good}")).is_err());
        assert_eq!(answer(Agent::Copilot, "hello").unwrap(), "hello");
    }

    #[test]
    fn isolated_profiles_lock_out_concurrent_runs_and_reject_foreign_customizations() {
        let home = tempfile::tempdir().unwrap();
        let room = tempfile::tempdir().unwrap();
        let guard = prepare_in(
            Agent::Agy,
            home.path(),
            room.path(),
            request().system.as_deref(),
        )
        .unwrap();
        assert!(prepare_in(Agent::Agy, home.path(), room.path(), None).is_err());
        let settings: Value = serde_json::from_str(
            &std::fs::read_to_string(home.path().join(".gemini/antigravity-cli/settings.json"))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(settings["useG1Credits"], false);
        assert!(settings["permissions"]["deny"]
            .as_array()
            .unwrap()
            .contains(&json!("mcp(*)")));
        let instructions =
            std::fs::read_to_string(room.path().join(".agents/agents/leo.md")).unwrap();
        assert!(
            instructions.contains("tools: []")
                && instructions.contains("inheritCustomizations: false")
        );
        drop(guard);
        std::fs::write(
            home.path().join(".gemini/antigravity-cli/mcp_config.json"),
            r#"{"mcpServers":{"unsafe":{"command":"must-not-run"}}}"#,
        )
        .unwrap();
        assert!(prepare_in(Agent::Agy, home.path(), room.path(), None).is_err());
    }

    #[test]
    fn child_profiles_override_inherited_automation_without_changing_this_process() {
        let home = tempfile::tempdir().unwrap();
        for agent in [Agent::Agy, Agent::Copilot] {
            let mut command = Command::new("not-run");
            environment(&mut command, agent, home.path());
            let env: HashMap<_, _> = command.get_envs().collect();
            let key = if agent == Agent::Agy {
                "HOME"
            } else {
                "COPILOT_HOME"
            };
            assert_eq!(
                env.get(std::ffi::OsStr::new(key)).copied().flatten(),
                Some(home.path().as_os_str())
            );
            for name in [
                "GEMINI_API_KEY",
                "NODE_OPTIONS",
                "COPILOT_PROVIDER_API_KEY_COMMAND",
                "COPILOT_PROVIDERS_CONFIG",
                "COPILOT_SKILLS_DIRS",
            ] {
                assert!(!env.contains_key(std::ffi::OsStr::new(name)));
            }
        }
    }

    #[test]
    fn plan_writers_preserve_login_locations_without_inheriting_credentials_or_startup_options() {
        for agent in [Agent::Codex, Agent::ClaudeCode] {
            let mut command = Command::new("not-run");
            for name in [
                "ANTHROPIC_API_KEY",
                "ANTHROPIC_AUTH_TOKEN",
                "ANTHROPIC_BASE_URL",
                "CLAUDE_CODE_OAUTH_TOKEN",
                "CLAUDE_CODE_USE_BEDROCK",
                "OPENAI_API_KEY",
                "OPENAI_BASE_URL",
                "CODEX_API_KEY",
                "CODEX_ACCESS_TOKEN",
                "NODE_OPTIONS",
                "NODE_PATH",
                "BASH_ENV",
                "LD_PRELOAD",
                "DYLD_INSERT_LIBRARIES",
            ] {
                command.env(name, "fixture, never used");
            }
            configure(&mut command, agent).unwrap();
            let env: HashMap<_, _> = command.get_envs().collect();
            assert!(!env.contains_key(std::ffi::OsStr::new("ANTHROPIC_API_KEY")));
            assert!(!env.contains_key(std::ffi::OsStr::new("NODE_OPTIONS")));
            for name in [
                "HOME",
                "USERPROFILE",
                "PATH",
                "XDG_CONFIG_HOME",
                "XDG_DATA_HOME",
                "CODEX_HOME",
                "CLAUDE_CONFIG_DIR",
                "DBUS_SESSION_BUS_ADDRESS",
                "CODEX_CA_CERTIFICATE",
                "NODE_EXTRA_CA_CERTS",
            ] {
                let expected = std::env::var_os(name);
                assert_eq!(
                    env.get(std::ffi::OsStr::new(name)).copied().flatten(),
                    expected.as_deref(),
                    "{name}"
                );
            }
            assert!(env.keys().all(|name| {
                !name.to_string_lossy().contains("API_KEY")
                    && !name.to_string_lossy().contains("TOKEN")
            }));
        }
    }

    #[cfg(unix)]
    #[test]
    fn probes_drop_explicit_runtime_and_provider_injection_without_parent_env_changes() {
        let home = tempfile::tempdir().unwrap();
        for agent in [
            None,
            Some(Agent::Codex),
            Some(Agent::ClaudeCode),
            Some(Agent::Agy),
            Some(Agent::Copilot),
        ] {
            let mut command = Command::new("sh");
            command.args(["-c", "test -z \"${ANTHROPIC_API_KEY+x}${ANTHROPIC_AUTH_TOKEN+x}${OPENAI_API_KEY+x}${CODEX_API_KEY+x}${NODE_OPTIONS+x}${BASH_ENV+x}${COPILOT_PROVIDER_API_KEY_COMMAND+x}${COPILOT_PROVIDERS_CONFIG+x}${COPILOT_SKILLS_DIRS+x}${GITHUB_COPILOT_PROMPT_MODE_EXTENSIONS+x}\" || exit 17; printf clean"]);
            for name in [
                "ANTHROPIC_API_KEY",
                "ANTHROPIC_AUTH_TOKEN",
                "OPENAI_API_KEY",
                "CODEX_API_KEY",
                "NODE_OPTIONS",
                "BASH_ENV",
                "COPILOT_PROVIDER_API_KEY_COMMAND",
                "COPILOT_PROVIDERS_CONFIG",
                "COPILOT_SKILLS_DIRS",
                "GITHUB_COPILOT_PROMPT_MODE_EXTENSIONS",
            ] {
                command.env(name, "untrusted fixture, never executed");
            }
            if let Some(agent) = agent {
                if scoped(agent) {
                    environment(&mut command, agent, home.path());
                } else {
                    configure(&mut command, agent).unwrap();
                }
            } else {
                plan_environment(&mut command);
            }
            assert_eq!(probe_command(command).unwrap(), "clean");
        }
    }

    #[test]
    fn profiles_reject_every_external_customization_surface() {
        for agent in [Agent::Agy, Agent::Copilot] {
            for name in [
                "installed-plugins",
                "extensions",
                "lsp-config.json",
                "providers.json",
                "hooks.json",
            ] {
                let home = tempfile::tempdir().unwrap();
                let root = if agent == Agent::Agy {
                    home.path().join(".gemini/config")
                } else {
                    home.path().to_path_buf()
                };
                std::fs::create_dir_all(&root).unwrap();
                std::fs::write(root.join(name), r#"{"command":"must-not-run"}"#).unwrap();
                assert!(
                    check_profile(agent, home.path()).is_err(),
                    "{agent:?}: {name}"
                );
            }
        }
        let home = tempfile::tempdir().unwrap();
        let room = tempfile::tempdir().unwrap();
        std::fs::write(
            home.path().join("settings.json"),
            r#"{"hooks":{"start":"must-not-run"}}"#,
        )
        .unwrap();
        let _guard = prepare_in(Agent::Copilot, home.path(), room.path(), None).unwrap();
        let settings: Value = serde_json::from_str(
            &std::fs::read_to_string(home.path().join("settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(settings, json!({"disableAllHooks":true}));
    }

    #[cfg(unix)]
    #[test]
    fn clearing_a_profile_unlinks_customizations_without_touching_their_targets() {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("keep");
        std::fs::write(&target, "keep").unwrap();
        std::os::unix::fs::symlink(outside.path(), home.path().join("hooks")).unwrap();
        clear_profile_in(home.path()).unwrap();
        assert!(!home.path().join("hooks").exists());
        assert_eq!(std::fs::read_to_string(target).unwrap(), "keep");
    }

    #[cfg(unix)]
    #[test]
    fn aws_copilot_is_not_mistaken_for_the_github_writing_cli() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("copilot");
        std::fs::write(
            &bin,
            "#!/bin/sh\necho 'Launch and manage containerized applications on AWS'\n",
        )
        .unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(!supported(Agent::Copilot, &bin));
    }
}
