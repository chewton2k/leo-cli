pub fn quiet(command: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(command, 0x0800_0000);
    command
}

pub fn quiet_command<S: AsRef<std::ffi::OsStr>>(program: S) -> std::process::Command {
    let mut command = std::process::Command::new(program);
    quiet(&mut command);
    command
}

pub fn stop_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    if let Ok(group) = libc::pid_t::try_from(child.id()) {
        unsafe {
            libc::kill(-group, libc::SIGKILL);
        }
    }
    #[cfg(windows)]
    if child.try_wait().ok().flatten().is_none() {
        let _ = quiet_command("taskkill")
            .args(["/T", "/F", "/PID", &child.id().to_string()])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    let _ = child.kill();
}

pub const WORKER_FLAG: &str = "--leo-speech-worker";

const MOST_MARKED: usize = 256;

static MARKED: std::sync::Mutex<Vec<(u32, &'static str)>> = std::sync::Mutex::new(Vec::new());

pub fn mark(pid: u32, kind: &'static str) {
    let Ok(mut marked) = MARKED.lock() else {
        return;
    };
    marked.retain(|(p, _)| *p != pid);
    if marked.len() >= MOST_MARKED {
        marked.remove(0);
    }
    marked.push((pid, kind));
}

fn marked() -> Vec<(u32, &'static str)> {
    MARKED.lock().map(|m| m.clone()).unwrap_or_default()
}

fn keep_marked(live: &[Owned]) {
    if let Ok(mut marked) = MARKED.lock() {
        marked.retain(|(pid, _)| live.iter().any(|p| p.pid == *pid));
    }
}

const MOST_OWNED: usize = 1_024;
#[cfg(unix)]
const PS_BYTES: u64 = 2_000_000;
#[cfg(unix)]
const PS_WAIT: std::time::Duration = std::time::Duration::from_millis(1_500);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owned {
    pub pid: u32,
    pub parent: u32,
    pub name: String,
    pub args: String,
    pub created: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub name: String,
    pub processes: Vec<(u32, String)>,
}

fn base_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn stem(name: &str) -> String {
    let lower = base_name(name).to_ascii_lowercase();
    for tail in [".exe", ".cmd", ".bat", ".js", ".mjs"] {
        if let Some(bare) = lower.strip_suffix(tail) {
            return bare.to_string();
        }
    }
    lower
}

#[cfg(any(unix, test))]
fn parse_ps(names: &str, args: &str) -> Vec<Owned> {
    let mut by_pid = std::collections::HashMap::new();
    for line in args.lines() {
        let line = line.trim_start();
        let Some((pid, rest)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        if let Ok(pid) = pid.parse::<u32>() {
            by_pid.insert(pid, rest.trim().to_string());
        }
    }
    let mut table = Vec::new();
    for line in names.lines() {
        let Some((pid, rest)) = line.trim_start().split_once(char::is_whitespace) else {
            continue;
        };
        let Some((parent, comm)) = rest.trim_start().split_once(char::is_whitespace) else {
            continue;
        };
        let (Ok(pid), Ok(parent)) = (pid.parse::<u32>(), parent.parse::<u32>()) else {
            continue;
        };
        let Some(args) = by_pid.get(&pid) else {
            continue;
        };
        let comm = comm.trim();
        if comm.is_empty() {
            continue;
        }
        let name = base_name(comm);
        table.push(Owned {
            pid,
            parent,
            name: name.to_string(),
            args: if args.contains(name) {
                args.clone()
            } else {
                String::new()
            },
            created: 0,
        });
    }
    table
}

fn descendants(table: &[Owned], root: u32) -> Vec<Owned> {
    let mut kids: std::collections::HashMap<u32, Vec<&Owned>> = std::collections::HashMap::new();
    for process in table {
        if process.pid != root {
            kids.entry(process.parent).or_default().push(process);
        }
    }
    let created = table
        .iter()
        .find(|p| p.pid == root)
        .map_or(0, |p| p.created);
    let mut seen = std::collections::HashSet::from([root]);
    let mut found = Vec::new();
    let mut queue = std::collections::VecDeque::from([(root, created)]);
    while let Some((parent, born)) = queue.pop_front() {
        for child in kids.get(&parent).into_iter().flatten() {
            let fresh = child.created == 0 || born == 0 || child.created >= born;
            if fresh && seen.insert(child.pid) {
                found.push((*child).clone());
                queue.push_back((child.pid, child.created));
            }
        }
    }
    found.sort_by_key(|p| p.pid);
    found.truncate(MOST_OWNED);
    found
}

fn kind_of(process: &Owned, own: &str, marks: &[(u32, &'static str)]) -> Vec<&'static str> {
    let name = stem(&process.name);
    let mut words = vec![name.clone()];
    words.extend(process.args.split_whitespace().take(3).map(stem));
    let mut kinds: Vec<&'static str> = marks
        .iter()
        .filter(|(pid, _)| *pid == process.pid)
        .map(|(_, kind)| *kind)
        .collect();
    let has = |what: &str| words.iter().any(|w| w == what);
    let path = process.args.to_ascii_lowercase();
    if has("codex") || path.contains("/@openai/codex") {
        kinds.push("Codex");
    }
    if has("claude") || path.contains("/claude-code/") {
        kinds.push("Claude Code");
    }
    if has("ollama") {
        kinds.push("Ollama");
    }
    if name == own {
        if process.args.split_whitespace().any(|a| a == WORKER_FLAG) {
            kinds.push("Speech engine");
        }
        kinds.push("leo helper");
    }
    if name == "msedgewebview2" {
        kinds.push("App window");
    }
    kinds
}

const ORDER: [&str; 6] = [
    "Codex",
    "Claude Code",
    "Ollama",
    "Speech engine",
    "App window",
    "leo helper",
];

fn groups(owned: &[Owned], root: u32, own: &str, marks: &[(u32, &'static str)]) -> Vec<Group> {
    let mut merged: Vec<Group> = Vec::new();
    for top in owned.iter().filter(|p| p.parent == root) {
        let tree = descendants(owned, top.pid);
        let members: Vec<&Owned> = std::iter::once(top).chain(tree.iter()).collect();
        let kinds: Vec<&str> = members
            .iter()
            .flat_map(|p| kind_of(p, own, marks))
            .collect();
        let name = ORDER
            .iter()
            .find(|k| kinds.contains(k))
            .map_or_else(|| top.name.clone(), |k| (*k).to_string());
        let processes = members.iter().map(|p| (p.pid, p.name.clone()));
        match merged.iter_mut().find(|g| g.name == name) {
            Some(group) => group.processes.extend(processes),
            None => merged.push(Group {
                name,
                processes: processes.collect(),
            }),
        }
    }
    merged
}

pub fn own_groups() -> Option<Vec<Group>> {
    let root = std::process::id();
    let own = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| stem(&n.to_string_lossy())))
        .unwrap_or_default();
    let owned = owned()?;
    keep_marked(&owned);
    Some(groups(&owned, root, &own, &marked()))
}

pub fn owned() -> Option<Vec<Owned>> {
    let root = std::process::id();
    let table = read_table()?;
    Some(descendants(&table, root))
}

#[cfg(unix)]
fn run_ps(args: &[&str]) -> Option<String> {
    use std::io::Read;
    let mut child = quiet_command("/bin/ps")
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let out = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        out.take(PS_BYTES + 1).read_to_end(&mut bytes).ok()?;
        Some(bytes)
    });
    let started = std::time::Instant::now();
    let finished = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if started.elapsed() < PS_WAIT => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
        }
    };
    let bytes = reader.join().ok().flatten()?;
    if !finished || bytes.len() as u64 > PS_BYTES {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(unix)]
fn read_table() -> Option<Vec<Owned>> {
    let names = run_ps(&["-A", "-o", "pid=,ppid=,comm="])?;
    let args = run_ps(&["-A", "-o", "pid=,args="])?;
    Some(parse_ps(&names, &args))
}

#[cfg(windows)]
fn read_table() -> Option<Vec<Owned>> {
    use windows::Win32::Foundation::{CloseHandle, FILETIME};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    let created = |pid: u32| -> u64 {
        unsafe {
            let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return 0;
            };
            let (mut born, mut gone, mut kernel, mut user) = (
                FILETIME::default(),
                FILETIME::default(),
                FILETIME::default(),
                FILETIME::default(),
            );
            let read = GetProcessTimes(handle, &mut born, &mut gone, &mut kernel, &mut user);
            let _ = CloseHandle(handle);
            if read.is_ok() {
                (u64::from(born.dwHighDateTime) << 32) | u64::from(born.dwLowDateTime)
            } else {
                0
            }
        }
    };
    let mut table = Vec::new();
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut more = Process32FirstW(snapshot, &mut entry).is_ok();
        while more {
            let len = entry
                .szExeFile
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(entry.szExeFile.len());
            table.push(Owned {
                pid: entry.th32ProcessID,
                parent: entry.th32ParentProcessID,
                name: String::from_utf16_lossy(&entry.szExeFile[..len]),
                args: String::new(),
                created: 0,
            });
            more = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
    }
    let root = std::process::id();
    let wanted: std::collections::HashSet<u32> = descendants(&table, root)
        .iter()
        .map(|p| p.pid)
        .chain([root])
        .collect();
    for process in table.iter_mut().filter(|p| wanted.contains(&p.pid)) {
        process.created = created(process.pid);
    }
    Some(table)
}

#[cfg(not(any(unix, windows)))]
fn read_table() -> Option<Vec<Owned>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: u32, parent: u32, name: &str, args: &str) -> Owned {
        Owned {
            pid,
            parent,
            name: name.into(),
            args: args.into(),
            created: 0,
        }
    }

    #[test]
    fn ps_output_is_joined_by_pid_and_names_keep_their_spaces() {
        let names = "  1     0 /sbin/launchd\n 50     1 /Applications/Big App.app/Contents/MacOS/Big App\n 51    50 /bin/sh\nbad line\n 60    50 /bin/zsh\n";
        let args = " 1 /sbin/launchd\n50 /Applications/Big App.app/Contents/MacOS/Big App --flag\n 51 sh -c codex exec\n";
        let table = parse_ps(names, args);
        assert_eq!(table.len(), 3);
        assert_eq!(table[1].name, "Big App");
        assert_eq!(table[2].args, "sh -c codex exec");
        let reused = parse_ps(" 70 1 /usr/bin/ollama\n", " 70 codex exec\n");
        assert_eq!(reused[0].args, "");
    }

    #[test]
    fn the_walk_follows_children_only_and_survives_a_loop() {
        let table = vec![
            p(10, 1, "leo", ""),
            p(11, 10, "a", ""),
            p(12, 11, "b", ""),
            p(20, 1, "orphan", ""),
            p(30, 31, "x", ""),
            p(31, 30, "y", ""),
        ];
        let found: Vec<u32> = descendants(&table, 10).iter().map(|o| o.pid).collect();
        assert_eq!(found, vec![11, 12]);
        assert!(descendants(&table, 30).iter().all(|o| o.pid == 31));
    }

    #[test]
    fn a_reused_pid_older_than_its_parent_is_not_a_child() {
        let mut table = vec![p(10, 1, "leo", ""), p(11, 10, "a", "")];
        table[0].created = 500;
        table[1].created = 100;
        assert!(descendants(&table, 10).is_empty());
        table[1].created = 600;
        assert_eq!(descendants(&table, 10).len(), 1);
    }

    #[test]
    fn groups_are_named_by_what_runs_inside_them_and_merged() {
        let table = vec![
            p(11, 10, "sh", "sh -c run"),
            p(12, 11, "node", "node /opt/bin/codex exec"),
            p(13, 10, "node", "node /usr/lib/claude-code/cli.js -p hi"),
            p(14, 13, "leo", "leo mcp"),
            p(15, 10, "node", "node /x/claude -p"),
            p(16, 10, "leo", "leo --leo-speech-worker"),
            p(17, 10, "leo", "leo something"),
            p(18, 10, "msedgewebview2.exe", ""),
            p(19, 10, "sleep", "sleep 5"),
            p(20, 10, "ollama", "ollama serve"),
        ];
        let named: Vec<(String, usize)> = groups(&table, 10, "leo", &[])
            .into_iter()
            .map(|g| (g.name, g.processes.len()))
            .collect();
        assert_eq!(
            named,
            vec![
                ("Codex".to_string(), 2),
                ("Claude Code".to_string(), 3),
                ("Speech engine".to_string(), 1),
                ("leo helper".to_string(), 1),
                ("App window".to_string(), 1),
                ("sleep".to_string(), 1),
                ("Ollama".to_string(), 1),
            ]
        );
    }

    #[test]
    fn without_arguments_marked_processes_still_name_their_group() {
        let table = vec![
            p(11, 10, "cmd.exe", ""),
            p(12, 11, "node.exe", ""),
            p(13, 10, "leo.exe", ""),
            p(14, 10, "codex.exe", ""),
        ];
        let marks = [(11, "Claude Code"), (13, "Speech engine")];
        let named: Vec<(String, usize)> = groups(&table, 10, "leo", &marks)
            .into_iter()
            .map(|g| (g.name, g.processes.len()))
            .collect();
        assert_eq!(
            named,
            vec![
                ("Claude Code".to_string(), 2),
                ("Speech engine".to_string(), 1),
                ("Codex".to_string(), 1),
            ]
        );
    }

    #[test]
    fn marks_forget_processes_that_are_gone() {
        mark(4_000_001, "Codex");
        mark(4_000_002, "Codex");
        keep_marked(&[p(4_000_002, 1, "codex", "")]);
        let left: Vec<u32> = marked()
            .iter()
            .map(|(pid, _)| *pid)
            .filter(|pid| *pid > 4_000_000)
            .collect();
        assert_eq!(left, vec![4_000_002]);
    }
}
