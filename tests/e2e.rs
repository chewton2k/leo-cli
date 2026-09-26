//! End-to-end: the real `leo` binary, run as a user would run it.
//!
//! Every test gets its own `LEO_HOME`, a scrubbed environment and a PATH holding
//! only the system directories plus git, so nothing here can read the user's
//! notes, their API keys, their keychain, or reach the network — and no test
//! can start recording from a microphone, since SoX is never on the PATH.
//! `$EDITOR` is a small script that appends a line to whatever file it is given.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

struct Leo {
    home: tempfile::TempDir,
    bin: PathBuf,
}

impl Leo {
    fn new() -> Leo {
        let home = tempfile::tempdir().unwrap();
        let bin = home.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();

        let editor = bin.join("fake-editor");
        std::fs::write(
            &editor,
            "#!/bin/sh\nprintf 'written in the editor\\n' >> \"$1\"\n",
        )
        .unwrap();
        make_executable(&editor);

        // git, and nothing else from wherever it was installed.
        if let Some(git) = find_on_path("git") {
            #[cfg(unix)]
            std::os::unix::fs::symlink(git, bin.join("git")).unwrap();
        }
        Leo { home, bin }
    }

    fn cmd(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_leo"));
        cmd.args(args)
            .env_clear()
            .current_dir(self.home.path())
            .env("LEO_HOME", self.home.path())
            .env("HOME", self.home.path())
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()))
            .env("EDITOR", self.bin.join("fake-editor"))
            .env("NO_COLOR", "1")
            .env("GIT_AUTHOR_NAME", "leo test")
            .env("GIT_AUTHOR_EMAIL", "leo@example.com")
            .env("GIT_COMMITTER_NAME", "leo test")
            .env("GIT_COMMITTER_EMAIL", "leo@example.com")
            .stdin(Stdio::null());
        cmd
    }

    /// Run and require success, returning stdout.
    fn ok(&self, args: &[&str]) -> String {
        let out = self.cmd(args).output().unwrap();
        assert!(
            out.status.success(),
            "leo {args:?} failed:\n{}",
            describe(&out)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn notes_dir(&self) -> PathBuf {
        self.home.path().join("notes")
    }

    /// Every note file's text, read straight from disk. Hidden directories
    /// (git's, the trash) hold no live notes.
    fn files(&self) -> Vec<String> {
        let mut out = Vec::new();
        collect_md(&self.notes_dir(), &mut out);
        out
    }
}

fn collect_md(dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let hidden = path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with('.'));
        if path.is_dir() && !hidden {
            collect_md(&path, out);
        } else if path.extension().is_some_and(|e| e == "md") {
            out.push(std::fs::read_to_string(&path).unwrap());
        }
    }
}

fn describe(out: &Output) -> String {
    format!(
        "status: {}\nstdout:\n{}\nstderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|p| p.join(name))
            .find(|p| p.is_file())
    })
}

fn make_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn has_git() -> bool {
    find_on_path("git").is_some()
}

// ── the basics ──────────────────────────────────────────────────────────────

#[test]
fn help_lists_the_everyday_commands_and_hides_the_old_ones() {
    let leo = Leo::new();
    let help = leo.ok(&["--help"]);
    for cmd in ["new", "list", "search", "listen", "doctor", "sync", "serve"] {
        assert!(help.contains(cmd), "help lacks {cmd}:\n{help}");
    }
    for old in ["setup", "model", "config"] {
        assert!(!help.contains(&format!("  {old} ")), "{help}");
    }
}

#[test]
fn bare_leo_without_a_terminal_says_so_and_fails() {
    let leo = Leo::new();
    let out = leo.cmd(&[]).output().unwrap();
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("terminal"),
        "{}",
        describe(&out)
    );
}

// ── notes ───────────────────────────────────────────────────────────────────

#[test]
fn a_note_made_from_the_shell_is_listed_found_and_on_disk() {
    let leo = Leo::new();
    leo.ok(&[
        "new",
        "Rust ownership",
        "--body",
        "borrow checker rules",
        "--tags",
        "rust,lang",
    ]);

    let list = leo.ok(&["list"]);
    assert!(list.contains("Rust ownership"), "{list}");

    // Search reaches bodies without a flag, and #tags.
    assert!(leo.ok(&["search", "checker"]).contains("Rust ownership"));
    assert!(leo.ok(&["search", "#rust"]).contains("Rust ownership"));
    assert!(!leo
        .ok(&["search", "nothing-like-this"])
        .contains("Rust ownership"));

    let files = leo.files();
    assert_eq!(files.len(), 2, "the note and the manual: {files:?}");
    assert!(files
        .iter()
        .any(|f| f.contains("title: Rust ownership") && f.contains("borrow checker rules")));
}

#[test]
fn search_prints_where_inside_the_note_it_matched() {
    let leo = Leo::new();
    leo.ok(&[
        "new",
        "Graphs",
        "--body",
        "intro\nBFS explores level by level",
    ]);
    let out = leo.ok(&["search", "explores"]);
    assert!(out.contains("Graphs"), "{out}");
    assert!(out.contains("BFS explores level by level"), "{out}");
}

#[test]
fn list_shows_one_directory_when_named() {
    let leo = Leo::new();
    leo.ok(&["new", "Top level", "--body", "x"]);
    leo.ok(&["new", "cs130/ Lecture 4", "--body", "x"]);
    let inside = leo.ok(&["list", "cs130"]);
    assert!(inside.contains("Lecture 4"), "{inside}");
    assert!(!inside.contains("Top level"), "{inside}");
    // Trailing slashes are fine.
    assert!(leo.ok(&["list", "cs130/"]).contains("Lecture 4"));

    let out = leo.cmd(&["list", "nowhere"]).output().unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("No such directory"), "{text}");
}

#[test]
fn new_puts_a_note_in_a_directory_with_tags() {
    let leo = Leo::new();
    leo.ok(&["new", "cs130/ Lecture 4 #exam", "--body", "graphs"]);
    let path = leo.notes_dir().join("cs130");
    assert!(path.is_dir(), "no cs130 directory on disk");
    let mut here = Vec::new();
    collect_md(&path, &mut here);
    assert_eq!(here.len(), 1, "{here:?}");
    assert!(here[0].contains("title: Lecture 4"), "{}", here[0]);
    assert!(here[0].contains("exam"), "{}", here[0]);
}

#[test]
fn new_without_a_body_opens_the_editor() {
    let leo = Leo::new();
    leo.ok(&["new", "From the editor"]);
    assert!(leo
        .files()
        .iter()
        .any(|f| f.contains("title: From the editor") && f.contains("written in the editor")));
}

#[test]
fn edit_goes_through_the_editor_and_keeps_the_note() {
    let leo = Leo::new();
    leo.ok(&["new", "Graphs", "--body", "BFS"]);
    leo.ok(&["edit", "Graphs"]);
    let files = leo.files();
    let note = files
        .iter()
        .find(|f| f.contains("title: Graphs"))
        .expect("the note survived");
    assert!(
        note.contains("BFS") && note.contains("written in the editor"),
        "{note}"
    );
}

#[test]
fn view_prints_the_whole_note() {
    let leo = Leo::new();
    leo.ok(&["new", "Graphs", "--body", "BFS explores level by level"]);
    let out = leo.ok(&["view", "Graphs"]);
    assert!(out.contains("BFS explores level by level"), "{out}");
}

#[test]
fn delete_with_force_removes_the_file() {
    let leo = Leo::new();
    leo.ok(&["new", "Doomed", "--body", "x"]);
    leo.ok(&["delete", "Doomed", "--force"]);
    assert!(!leo.files().iter().any(|f| f.contains("title: Doomed")));
    assert!(!leo.ok(&["list"]).contains("Doomed"));
}

/// A deleted note waits in the trash: listed, restorable, and emptied only
/// when asked.
#[test]
fn a_deleted_note_can_be_restored_from_the_trash() {
    let leo = Leo::new();
    leo.ok(&["new", "cs130/ Lecture 4", "--body", "BFS"]);
    let said = leo.ok(&["delete", "Lecture 4", "--force"]);
    assert!(said.contains("trash"), "{said}");

    let listed = leo.ok(&["trash"]);
    assert!(listed.contains("Lecture 4"), "{listed}");
    assert!(listed.contains("/cs130"), "{listed}");

    let restored = leo.ok(&["trash", "restore", "1"]);
    assert!(restored.contains("Restored"), "{restored}");
    assert!(leo.ok(&["list", "cs130"]).contains("Lecture 4"));
    assert!(leo.ok(&["trash"]).contains("empty"));

    leo.ok(&["delete", "Lecture 4", "--force"]);
    leo.ok(&["trash", "empty", "--force"]);
    assert!(leo.ok(&["trash"]).contains("empty"));
    assert!(!leo.ok(&["list", "cs130"]).contains("Lecture 4"));
}

#[test]
fn delete_without_confirmation_keeps_the_note() {
    let leo = Leo::new();
    leo.ok(&["new", "Keeper", "--body", "x"]);
    // stdin is empty, so the question gets no "y".
    let _ = leo.cmd(&["delete", "Keeper"]).output().unwrap();
    assert!(leo.files().iter().any(|f| f.contains("title: Keeper")));
}

#[test]
fn an_unknown_note_is_reported_not_guessed() {
    let leo = Leo::new();
    leo.ok(&["new", "Graphs", "--body", "x"]);
    let out = leo.cmd(&["view", "no-such-note"]).output().unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("No note found"), "{text}");
}

#[test]
fn ask_on_a_note_without_prompts_makes_no_request() {
    let leo = Leo::new();
    leo.ok(&["new", "Plain", "--body", "no questions here"]);
    assert!(leo.ok(&["ask", "Plain"]).contains("No @leo prompts"));
}

/// `leo ask` with a question none of the notes mention says so, without
/// calling any AI (there are no keys here, so a call would fail).
#[test]
fn ask_across_notes_with_nothing_relevant_says_so() {
    let leo = Leo::new();
    leo.ok(&["new", "Groceries", "--body", "milk, eggs"]);
    let out = leo.ok(&["ask", "what did we cover about quantum chromodynamics?"]);
    assert!(out.contains("None of your notes mention that"), "{out}");
}

/// A question that does match notes reaches for the AI, which is not set up
/// here — so it must fail with a message, not hang or crash.
#[test]
fn ask_across_notes_without_any_ai_fails_cleanly() {
    let leo = Leo::new();
    leo.ok(&["new", "Graph traversals", "--body", "BFS uses a queue"]);
    let out = leo.cmd(&["ask", "how does BFS work?"]).output().unwrap();
    assert!(!out.status.success(), "{}", describe(&out));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("Error"), "{}", describe(&out));
}

/// A note file with a broken header is skipped, and saving other notes must
/// never delete it.
#[test]
fn a_note_leo_cannot_read_is_never_deleted() {
    let leo = Leo::new();
    leo.ok(&["new", "First", "--body", "x"]);
    let broken = leo.notes_dir().join("broken.md");
    std::fs::write(&broken, "---\ntitle: [unclosed\n---\nmy words").unwrap();
    leo.ok(&["new", "Second", "--body", "y"]);
    leo.ok(&["delete", "First", "--force"]);
    assert!(broken.exists(), "saving deleted a note leo could not read");
    assert!(std::fs::read_to_string(&broken)
        .unwrap()
        .contains("my words"));
}

#[test]
fn the_manual_is_installed_once() {
    let leo = Leo::new();
    leo.ok(&["list"]);
    leo.ok(&["list"]);
    let manuals = leo
        .files()
        .iter()
        .filter(|f| f.contains("title: leo manual"))
        .count();
    assert_eq!(manuals, 1);
}

// ── doctor ──────────────────────────────────────────────────────────────────

/// With no AI key, doctor would offer to store one — but only when someone is
/// at a terminal to answer.
#[test]
fn doctor_reports_without_asking_when_nobody_is_there_to_answer() {
    let leo = Leo::new();
    let run = leo.cmd(&["doctor"]).output().unwrap();
    let out = String::from_utf8_lossy(&run.stdout).to_string();
    assert!(out.contains("notes"), "{out}");
    assert!(
        out.contains(&leo.home.path().display().to_string()),
        "paths are not LEO_HOME's:\n{out}"
    );
    assert!(
        !out.contains("Store an API key now"),
        "asked a question with no terminal:\n{out}"
    );
}

#[test]
fn the_old_setup_commands_are_gone() {
    let leo = Leo::new();
    for old in [
        &["setup"][..],
        &["model", "list"],
        &["config", "path"],
        &["env"],
    ] {
        assert!(
            !leo.cmd(old).output().unwrap().status.success(),
            "{old:?} still works"
        );
    }
}

/// `leo doctor` checks everything and says so by section; anything broken
/// makes it exit non-zero, so a script can tell.
#[test]
fn doctor_scans_every_part_and_fails_when_something_is_broken() {
    let leo = Leo::new();
    leo.ok(&["new", "A note", "--body", "x"]);
    std::fs::write(
        leo.notes_dir().join("broken.md"),
        "---\ntitle: [unclosed\n---\nmy words",
    )
    .unwrap();

    let out = leo.cmd(&["doctor"]).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    for heading in ["leo", "notes", "AI", "recording", "backup"] {
        assert!(
            text.lines().any(|l| l.trim() == heading),
            "no {heading} section:\n{text}"
        );
    }
    assert!(text.contains("broken.md"), "{text}");
    assert!(!out.status.success(), "a broken note should fail the scan");
    // Reading the notes must not have touched them.
    assert!(leo.notes_dir().join("broken.md").exists());
}

// ── backup ──────────────────────────────────────────────────────────────────

#[test]
fn sync_backs_notes_up_to_a_git_remote() {
    if !has_git() {
        eprintln!("skipping: git is not installed");
        return;
    }
    let leo = Leo::new();
    let remote = leo.home.path().join("remote.git");
    let init = Command::new("git")
        .args(["init", "--bare", "-q"])
        .arg(&remote)
        .output()
        .unwrap();
    assert!(init.status.success(), "{}", describe(&init));

    leo.ok(&["new", "First", "--body", "one"]);
    leo.ok(&["sync", "init"]);
    leo.ok(&["sync", "connect", remote.to_str().unwrap()]);
    leo.ok(&["new", "Second", "--body", "two"]);
    leo.ok(&["sync"]);

    let log = Command::new("git")
        .args([
            "--git-dir",
            remote.to_str().unwrap(),
            "log",
            "--all",
            "--name-only",
            "--format=",
        ])
        .output()
        .unwrap();
    let files = String::from_utf8_lossy(&log.stdout);
    assert!(
        files.lines().filter(|l| l.ends_with(".md")).count() >= 2,
        "remote has: {files}"
    );
}

/// Two computers, one backup: each ends up with both computers' notes.
#[test]
fn a_second_computer_joins_the_backup_and_both_share_notes() {
    if !has_git() {
        eprintln!("skipping: git is not installed");
        return;
    }
    let laptop = Leo::new();
    let desktop = Leo::new();
    let remote = laptop.home.path().join("remote.git");
    let init = Command::new("git")
        .args(["init", "--bare", "-q"])
        .arg(&remote)
        .output()
        .unwrap();
    assert!(init.status.success(), "{}", describe(&init));
    let url = remote.to_str().unwrap();

    laptop.ok(&["new", "Written on the laptop", "--body", "one"]);
    laptop.ok(&["sync", "connect", url]);
    laptop.ok(&["sync"]);

    desktop.ok(&["new", "Written on the desktop", "--body", "two"]);
    desktop.ok(&["sync", "connect", url]);
    desktop.ok(&["sync"]);
    let listed = desktop.ok(&["list"]);
    assert!(listed.contains("Written on the laptop"), "{listed}");
    assert!(listed.contains("Written on the desktop"), "{listed}");

    laptop.ok(&["sync"]);
    assert!(laptop.ok(&["list"]).contains("Written on the desktop"));
}

#[test]
fn sync_before_setup_says_what_to_do() {
    let leo = Leo::new();
    let out = leo.cmd(&["sync"]).output().unwrap();
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("leo sync"),
        "{}",
        describe(&out)
    );
}

// ── the web server ──────────────────────────────────────────────────────────

/// `leo serve` answers with the notes when given its token, and refuses
/// without it.
#[test]
fn serve_needs_its_token_and_then_lists_the_notes() {
    let leo = Leo::new();
    leo.ok(&["new", "Served note", "--body", "x"]);

    let port = 38000 + (std::process::id() % 1000) as u16;
    let mut child = leo
        .cmd(&["serve", "--port", &port.to_string()])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        // Keep reading after the token: dropping the pipe would make the
        // server's next line of output fail, and it would exit.
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(i) = line.find("token=") {
                let token: String = line[i + 6..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric())
                    .collect();
                let _ = tx.send(token);
            }
        }
    });
    let token = rx
        .recv_timeout(Duration::from_secs(20))
        .expect("serve never printed its link");

    let get = |path: &str| -> Option<String> {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Ok(mut s) = std::net::TcpStream::connect(("127.0.0.1", port)) {
                s.set_read_timeout(Some(Duration::from_secs(5))).ok();
                write!(
                    s,
                    "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
                )
                .ok()?;
                let mut body = String::new();
                s.read_to_string(&mut body).ok()?;
                return Some(body);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        None
    };

    let refused = get("/api/notes").expect("server did not answer");
    let allowed = get(&format!("/api/notes?token={token}")).expect("server did not answer");
    let _ = child.kill();
    let _ = child.wait();

    assert!(
        refused.starts_with("HTTP/1.1 401"),
        "no token was accepted:\n{refused}"
    );
    assert!(allowed.starts_with("HTTP/1.1 200"), "{allowed}");
    assert!(allowed.contains("Served note"), "{allowed}");
}
