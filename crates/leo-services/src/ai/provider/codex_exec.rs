use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use chrono::{Datelike, Local, NaiveDate};

use super::agent_cli::{codex_disables, locate, probe, AgentCli};
use super::{ChatProvider, ChatRequest, Sink, Spent};
use crate::ai::error::{ProviderError, ProviderResult};

const MOST_CONTEXT_CHARS: usize = 2 * 1024 * 1024;
const MOST_PROGRAMS: usize = 64;
const MOST_DAY_FILES: usize = 8192;
const MOST_OWNED_THREADS: usize = 2;
const MOST_ROLLOUT_BYTES: u64 = 8 * 1024 * 1024;

pub(crate) fn supported(program: &Path) -> bool {
    static KNOWN: OnceLock<Mutex<HashMap<PathBuf, bool>>> = OnceLock::new();
    let cache = KNOWN.get_or_init(Default::default);
    if let Some(found) = cache.lock().ok().and_then(|c| c.get(program).copied()) {
        return found;
    }
    let Ok(help) = probe(program, &["exec", "resume", "--help"]) else {
        return false;
    };
    let found = ["--ignore-user-config", "--ignore-rules", "--json"]
        .iter()
        .all(|flag| help.split_whitespace().any(|word| word == *flag));
    if let Ok(mut entries) = cache.lock() {
        if entries.len() < MOST_PROGRAMS {
            entries.insert(program.to_path_buf(), found);
        }
    }
    found
}

pub(crate) struct CodexExec {
    agent: AgentCli,
    program: PathBuf,
    instructions: tempfile::NamedTempFile,
    room: tempfile::TempDir,
    home: PathBuf,
    disables: Vec<String>,
    thread: Option<String>,
    owned: Vec<(String, NaiveDate)>,
    previous: Option<Spent>,
    sent: usize,
    poisoned: bool,
}

impl CodexExec {
    pub(crate) fn start(agent: AgentCli, system: &str) -> ProviderResult<Self> {
        let home = std::env::var_os("CODEX_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| dirs::home_dir().map(|home| home.join(".codex")))
            .ok_or_else(private_error)?;
        Self::start_in(agent, system, home)
    }

    fn start_in(agent: AgentCli, system: &str, home: PathBuf) -> ProviderResult<Self> {
        let program = locate(agent.bin())
            .ok_or_else(|| ProviderError::Retryable(agent.unavailable_reason()))?;
        if !supported(&program) {
            return Err(ProviderError::Retryable(
                "This Codex version cannot resume an isolated chat. Update Codex, then try again."
                    .into(),
            ));
        }
        if !home.is_absolute()
            || !std::fs::symlink_metadata(&home).is_ok_and(|meta| meta.is_dir())
            || system.len() > MOST_CONTEXT_CHARS
        {
            return Err(private_error());
        }
        rollout_folders(&home, Local::now().date_naive())?;
        let disables = codex_disables(&program)?;
        let room = agent.room()?;
        let mut instructions = tempfile::Builder::new()
            .prefix("instructions-")
            .suffix(".md")
            .tempfile_in(room.path())
            .map_err(|_| private_error())?;
        instructions
            .write_all(system.as_bytes())
            .and_then(|_| instructions.as_file().sync_all())
            .map_err(|_| private_error())?;
        Ok(Self {
            agent,
            program,
            instructions,
            room,
            home,
            disables,
            thread: None,
            owned: Vec::new(),
            previous: None,
            sent: system.len(),
            poisoned: false,
        })
    }

    pub(crate) fn context_chars(&self) -> usize {
        self.sent
    }

    fn arguments(&self, text: &str) -> ProviderResult<(Vec<String>, String)> {
        let request = ChatRequest {
            system: None,
            prompt: text.to_string(),
            temperature: 0.2,
            max_tokens: 8192,
        };
        let (mut args, input) = self.agent.arguments(&request, &[], &self.disables, &[]);
        args.pop();
        args.retain(|arg| arg != "--ephemeral");
        for (key, value) in [
            ("model_instructions_file", self.instructions.path()),
            ("sqlite_home", self.room.path()),
            ("log_dir", self.room.path()),
        ] {
            args.extend([
                "-c".into(),
                format!(
                    "{key}={}",
                    serde_json::to_string(&value.to_string_lossy()).map_err(|_| private_error())?
                ),
            ]);
        }
        args.extend(["-c".into(), "history.persistence=\"none\"".into()]);
        if let Some(thread) = &self.thread {
            args.extend(["resume".into(), thread.clone()]);
        }
        args.push("-".into());
        Ok((args, input))
    }

    fn check_thread(&self, thread: &str) -> ProviderResult<()> {
        let (_, date) = self
            .owned
            .iter()
            .find(|(owned, _)| owned == thread)
            .ok_or_else(session_error)?;
        let paths = owned_rollouts(&self.home, thread, *date)?;
        if paths.is_empty() {
            return Err(session_error());
        }
        for path in paths {
            let meta = std::fs::symlink_metadata(path).map_err(|_| private_error())?;
            if !meta.is_file() || meta.len() > MOST_ROLLOUT_BYTES || !private_file(&meta) {
                return Err(private_error());
            }
        }
        Ok(())
    }

    pub(crate) fn say(
        &mut self,
        text: &str,
        sink: Sink<'_>,
        stopped: &dyn Fn() -> bool,
    ) -> ProviderResult<(String, Spent)> {
        if self.poisoned {
            return Err(ProviderError::Retryable(
                "The previous Codex turn was interrupted. Send the message again to reconnect."
                    .into(),
            ));
        }
        if stopped() {
            return Err(ProviderError::Fatal("The answer was stopped.".into()));
        }
        if self.sent.saturating_add(text.len()) > MOST_CONTEXT_CHARS {
            return Err(ProviderError::Retryable(
                "This conversation has reached its context limit. Start a new chat to continue."
                    .into(),
            ));
        }
        self.poisoned = true;
        if let Some(thread) = &self.thread {
            self.check_thread(thread)?;
        }
        rollout_folders(&self.home, Local::now().date_naive())?;
        let (args, input) = self.arguments(text)?;
        let answer = self.agent.spawn(
            &self.program,
            self.room.path(),
            &args,
            input,
            &mut |_| {},
            stopped,
        );
        let returned = self.agent.last_thread();
        if let Some(thread) = &returned {
            if self.owned.len() < MOST_OWNED_THREADS
                && !self.owned.iter().any(|(owned, _)| owned == thread)
            {
                self.owned.push((thread.clone(), Local::now().date_naive()));
            }
        }
        let answer = answer?;
        if stopped() {
            return Err(ProviderError::Fatal("The answer was stopped.".into()));
        }
        let Some(returned) = returned else {
            return Err(session_error());
        };
        if self
            .thread
            .as_ref()
            .is_some_and(|thread| thread != &returned)
        {
            return Err(session_error());
        }
        self.check_thread(&returned)?;
        self.thread = Some(returned);
        let request = ChatRequest {
            system: None,
            prompt: text.to_string(),
            temperature: 0.2,
            max_tokens: 8192,
        };
        let cumulative = self.agent.spent().unwrap_or_else(|| {
            Spent::guessed(self.agent.model().map(str::to_owned), &request, &answer)
        });
        let spent = delta(&cumulative, self.previous.as_ref());
        self.previous = Some(cumulative);
        self.sent = self
            .sent
            .saturating_add(text.len())
            .saturating_add(answer.len());
        self.poisoned = false;
        sink(&answer);
        Ok((answer, spent))
    }
}

fn delta(current: &Spent, previous: Option<&Spent>) -> Spent {
    let Some(previous) = previous.filter(|previous| {
        !previous.estimated
            && !current.estimated
            && current.input >= previous.input
            && current.output >= previous.output
    }) else {
        return current.clone();
    };
    Spent {
        input: current.input.saturating_sub(previous.input),
        output: current.output.saturating_sub(previous.output),
        cached: current
            .cached
            .saturating_sub(previous.cached)
            .min(current.input.saturating_sub(previous.input)),
        cache_written: current.cache_written.saturating_sub(previous.cache_written),
        ..current.clone()
    }
}

fn private_file(meta: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        meta.mode() & 0o077 == 0 && meta.nlink() == 1
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        true
    }
}

fn rollout_folders(home: &Path, date: NaiveDate) -> ProviderResult<Vec<PathBuf>> {
    [date.pred_opt(), Some(date), date.succ_opt()]
        .into_iter()
        .flatten()
        .map(|day| {
            let relative = PathBuf::from("sessions")
                .join(day.year().to_string())
                .join(format!("{:02}", day.month()))
                .join(format!("{:02}", day.day()));
            leo_core::paths::contained_path(home, &relative).map_err(|_| private_error())
        })
        .collect()
}

fn owned_rollouts(home: &Path, thread: &str, date: NaiveDate) -> ProviderResult<Vec<PathBuf>> {
    if uuid::Uuid::parse_str(thread).is_err()
        || !std::fs::symlink_metadata(home).is_ok_and(|meta| meta.is_dir())
    {
        return Err(private_error());
    }
    let mut found = Vec::new();
    for folder in rollout_folders(home, date)? {
        let entries = match std::fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(private_error()),
        };
        for (count, entry) in entries.enumerate() {
            if count >= MOST_DAY_FILES {
                return Err(private_error());
            }
            let entry = entry.map_err(|_| private_error())?;
            if owned_name(&entry.file_name().to_string_lossy(), thread) {
                if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
                    return Err(private_error());
                }
                found.push(entry.path());
            }
        }
    }
    Ok(found)
}

fn owned_name(name: &str, thread: &str) -> bool {
    let name = name.strip_suffix(".zst").unwrap_or(name);
    let Some(core) = name
        .strip_prefix("rollout-")
        .and_then(|name| name.strip_suffix(".jsonl"))
    else {
        return false;
    };
    let Some(ids) = core.get(20..) else {
        return false;
    };
    let (id, rollout) = ids.split_once('_').unwrap_or((ids, ids));
    id == thread && uuid::Uuid::parse_str(rollout).is_ok()
}

fn private_error() -> ProviderError {
    ProviderError::Retryable(
        "Leo could not keep this Codex conversation private. Check free space and your Codex folder permissions, then try again.".into(),
    )
}

fn session_error() -> ProviderError {
    ProviderError::Retryable(
        "Codex could not resume this conversation safely. Send the message again to reconnect."
            .into(),
    )
}

impl Drop for CodexExec {
    fn drop(&mut self) {
        for (thread, date) in &self.owned {
            if let Ok(paths) = owned_rollouts(&self.home, thread, *date) {
                for path in paths {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const THREAD: &str = "01999aaa-bbbb-7ccc-8ddd-000000000001";
    const OTHER: &str = "01999aaa-bbbb-7ccc-8ddd-000000000002";

    #[test]
    fn rollout_names_belong_to_exact_threads_including_reverted_and_compressed_files() {
        let base = format!("rollout-2026-10-18T12-34-56-{THREAD}");
        assert!(owned_name(&format!("{base}.jsonl"), THREAD));
        assert!(owned_name(&format!("{base}.jsonl.zst"), THREAD));
        assert!(owned_name(&format!("{base}_{OTHER}.jsonl"), THREAD));
        for name in [
            format!("{base}.jsonl.bak"),
            format!("{base}_invalid.jsonl"),
            format!("rollout-2026-10-18T12-34-56-{OTHER}.jsonl"),
            format!("other-{THREAD}.jsonl"),
            "rollout-文文文文文文文.jsonl".into(),
        ] {
            assert!(!owned_name(&name, THREAD), "{name}");
        }
    }

    #[test]
    fn resumed_usage_counts_only_new_tokens_and_handles_counter_resets() {
        let previous = Spent {
            input: 100,
            output: 10,
            cached: 50,
            cache_written: 20,
            ..Spent::default()
        };
        let current = Spent {
            input: 250,
            output: 30,
            cached: 175,
            cache_written: 25,
            ..Spent::default()
        };
        assert_eq!(delta(&previous, None), previous);
        let new = delta(&current, Some(&previous));
        assert_eq!(
            (new.input, new.output, new.cached, new.cache_written),
            (150, 20, 125, 5)
        );
        assert_eq!(delta(&previous, Some(&current)), previous);
        let estimated = Spent {
            estimated: true,
            ..previous.clone()
        };
        assert_eq!(delta(&current, Some(&estimated)), current);
        let reset_cache = Spent {
            cached: 10,
            ..current
        };
        assert_eq!(delta(&reset_cache, Some(&previous)).cached, 0);
    }

    #[cfg(unix)]
    fn fixture(mode: &str) -> (tempfile::TempDir, AgentCli, PathBuf) {
        use super::super::agent_cli::Agent;
        use crate::config::provider::{ProviderConfig, ProviderKind};
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("codex-home");
        std::fs::create_dir(&home).unwrap();
        let program = root.path().join("codex");
        let script = format!(
            r#"#!/bin/sh
root=$(dirname "$0")
if [ "$1" = features ]; then
  printf 'shell_tool stable true\nhooks stable true\n'
  exit 0
fi
if [ "$2" = resume ] && [ "$3" = --help ]; then
  printf '%s\n' '--ignore-user-config --ignore-rules --json'
  exit 0
fi
count=1
if [ -f "$root/count" ]; then count=2; fi
printf '%s' "$count" > "$root/count"
printf '%s\n' "$@" > "$root/args.$count"
cat > "$root/input.$count"
thread='{THREAD}'
if [ '{mode}' = mismatch ] && [ "$count" = 2 ]; then thread='{OTHER}'; fi
folder="$root/codex-home/sessions/{year}/{month:02}/{day:02}"
mkdir -p "$folder"
printf '%s\n' '{{"type":"session_meta"}}' > "$folder/rollout-2026-10-18T12-34-56-$thread.jsonl"
printf '{{"type":"thread.started","thread_id":"%s"}}\n' "$thread"
if [ '{mode}' = failed ]; then exit 1; fi
printf '{{"type":"item.completed","item":{{"type":"agent_message","text":"Offline answer"}}}}\n'
printf '{{"type":"turn.completed","usage":{{"input_tokens":%s,"output_tokens":%s,"cached_input_tokens":0}}}}\n' "$((count * 100))" "$((count * 5))"
"#,
            year = Local::now().year(),
            month = Local::now().month(),
            day = Local::now().day(),
        );
        std::fs::write(&program, script).unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
        let cfg = ProviderConfig {
            kind: Some(ProviderKind::Codex),
            bin: Some(program.to_string_lossy().into_owned()),
            ..ProviderConfig::default()
        };
        let agent = AgentCli::new("codex".into(), Agent::Codex, &cfg);
        (root, agent, home)
    }

    #[cfg(unix)]
    #[test]
    fn codex_resumes_one_private_thread_with_only_new_material_and_cleans_its_own_files() {
        use std::os::unix::fs::PermissionsExt;
        let (root, agent, home) = fixture("normal");
        let mut session =
            CodexExec::start_in(agent, "Use interpretable language.", home.clone()).unwrap();
        let room = session.room.path().to_path_buf();
        let instructions = session.instructions.path().to_path_buf();
        let (_, first) = session
            .say("First marker maple", &mut |_| {}, &|| false)
            .unwrap();
        let (_, second) = session
            .say("Only this result", &mut |_| {}, &|| false)
            .unwrap();
        assert_eq!(
            (first.input, second.input, first.output, second.output),
            (100, 100, 5, 5)
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join("input.2")).unwrap(),
            "Only this result"
        );
        let args = std::fs::read_to_string(root.path().join("args.2")).unwrap();
        assert!(args.contains(&format!("resume\n{THREAD}\n-\n")));
        assert!(args.contains("--ignore-user-config\n--ignore-rules\n"));
        assert!(!args.contains("--ephemeral"));
        assert!(args.contains("sqlite_home="));
        assert!(args.contains("history.persistence=\"none\""));
        let own = owned_rollouts(&home, THREAD, Local::now().date_naive()).unwrap();
        assert_eq!(own.len(), 1);
        assert_eq!(
            std::fs::metadata(&own[0]).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let other = own[0]
            .parent()
            .unwrap()
            .join(format!("rollout-2026-10-18T12-34-56-{OTHER}.jsonl"));
        std::fs::write(&other, "Other conversation").unwrap();
        drop(session);
        assert!(!own[0].exists());
        assert!(other.exists());
        assert!(!room.exists());
        assert!(!instructions.exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_resumed_thread_mismatch_is_poisoned_and_both_owned_rollouts_are_removed() {
        let (_root, agent, home) = fixture("mismatch");
        let mut session =
            CodexExec::start_in(agent, "Use interpretable language.", home.clone()).unwrap();
        session.say("First", &mut |_| {}, &|| false).unwrap();
        let mut shown = String::new();
        assert!(session
            .say("Second", &mut |text| shown.push_str(text), &|| false)
            .is_err());
        assert!(shown.is_empty());
        assert!(session.say("Third", &mut |_| {}, &|| false).is_err());
        drop(session);
        for thread in [THREAD, OTHER] {
            assert!(owned_rollouts(&home, thread, Local::now().date_naive())
                .unwrap()
                .is_empty());
        }
    }

    #[cfg(unix)]
    #[test]
    fn failed_and_cancelled_calls_are_not_retried_and_leave_no_owned_rollout() {
        let (root, agent, home) = fixture("failed");
        let mut session =
            CodexExec::start_in(agent, "Use interpretable language.", home.clone()).unwrap();
        assert!(session
            .say("Stop before launching", &mut |_| {}, &|| true)
            .is_err());
        assert!(!root.path().join("count").exists());
        assert!(session
            .say("Fails after starting", &mut |_| {}, &|| false)
            .is_err());
        assert_eq!(
            std::fs::read_to_string(root.path().join("count")).unwrap(),
            "1"
        );
        drop(session);
        assert!(owned_rollouts(&home, THREAD, Local::now().date_naive())
            .unwrap()
            .is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn rollout_lookup_rejects_symbolic_links_and_never_deletes_their_targets() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("keep.jsonl");
        std::fs::write(&target, "Keep this").unwrap();
        symlink(outside.path(), root.path().join("sessions")).unwrap();
        assert!(owned_rollouts(root.path(), THREAD, Local::now().date_naive()).is_err());
        assert_eq!(std::fs::read_to_string(target).unwrap(), "Keep this");
    }

    #[cfg(unix)]
    #[test]
    fn missing_rollouts_and_symlinked_storage_are_rejected_before_another_request() {
        let (root, agent, home) = fixture("normal");
        let mut session =
            CodexExec::start_in(agent, "Use interpretable language.", home.clone()).unwrap();
        session.say("First", &mut |_| {}, &|| false).unwrap();
        let paths = owned_rollouts(&home, THREAD, Local::now().date_naive()).unwrap();
        std::fs::remove_file(&paths[0]).unwrap();
        assert!(session.say("Second", &mut |_| {}, &|| false).is_err());
        assert!(!root.path().join("input.2").exists());

        let (root, agent, home) = fixture("normal");
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), home.join("sessions")).unwrap();
        assert!(CodexExec::start_in(agent, "Use interpretable language.", home).is_err());
        assert!(!root.path().join("count").exists());
    }
}
