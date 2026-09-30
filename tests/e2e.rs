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
        self.cmd_at(Path::new(env!("CARGO_BIN_EXE_leo")), args)
    }

    fn installed(&self) -> PathBuf {
        let dir = self.home.path().join(".local/bin");
        std::fs::create_dir_all(&dir).unwrap();
        let leo = dir.join("leo");
        std::fs::copy(env!("CARGO_BIN_EXE_leo"), &leo).unwrap();
        leo
    }

    fn cmd_at(&self, exe: &Path, args: &[&str]) -> Command {
        let mut cmd = Command::new(exe);
        cmd.args(args)
            .env_clear()
            .current_dir(self.home.path())
            .env("LEO_HOME", self.home.path())
            .env("HOME", self.home.path())
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()))
            .env("EDITOR", self.bin.join("fake-editor"))
            .env("NO_COLOR", "1")
            .env("LEO_NO_UPDATE_CHECK", "1")
            .env("LEO_INSTALL_SKIP_MODEL", "1")
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
    for cmd in [
        "new", "search", "record", "ask", "doctor", "backup", "serve",
    ] {
        assert!(
            help.contains(&format!("  {cmd} ")),
            "help lacks {cmd}:\n{help}"
        );
    }
    for old in [
        "setup", "model", "config", "sync", "listen", "trash", "list",
    ] {
        assert!(!help.contains(&format!("  {old} ")), "{help}");
    }
    let all = leo.ok(&["help", "--all"]);
    for cmd in ["list", "trash", "obsidian", "update", "uninstall"] {
        assert!(
            all.contains(&format!("  {cmd} ")),
            "help --all lacks {cmd}:\n{all}"
        );
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

#[test]
fn a_pinned_note_leads_the_list() {
    let leo = Leo::new();
    leo.ok(&["new", "Syllabus", "--body", "x"]);
    leo.ok(&["new", "Lecture 1", "--body", "x"]);
    assert!(leo.ok(&["pin", "Syllabus"]).contains("Pinned"));
    let list = leo.ok(&["list"]);
    let first = list
        .lines()
        .find(|l| l.contains("Syllabus") || l.contains("Lecture 1") || l.contains("manual"))
        .unwrap_or_default();
    assert!(first.contains("Syllabus"), "{list}");
}

#[test]
fn a_note_written_by_another_app_shows_up_and_is_left_alone() {
    let leo = Leo::new();
    leo.ok(&["new", "Mine", "--body", "x"]);
    let file = leo.notes_dir().join("From Obsidian.md");
    std::fs::write(&file, "---\ntags:\n  - vault\n---\nHello from the vault\n").unwrap();

    let listed = leo.ok(&["list"]);
    assert!(listed.contains("From Obsidian"), "{listed}");
    let found = leo.ok(&["search", "vault"]);
    assert!(found.contains("From Obsidian"), "{found}");
    let after = std::fs::read_to_string(&file).unwrap();
    assert!(
        after.starts_with("---\ntags:\n  - vault\n---"),
        "leo rewrote a note it did not change:\n{after}"
    );
}

#[test]
fn leo_and_obsidian_work_on_the_same_notes_folder() {
    let leo = Leo::new();
    leo.ok(&["new", "cs130/ Lecture 4", "--body", "BFS uses a queue"]);
    let cs130 = leo.notes_dir().join("cs130");
    let lecture = cs130.join("Lecture 4.md");
    assert!(lecture.is_file(), "not saved under its title");

    std::fs::write(cs130.join("Office hours.md"), "Tuesday 3-5pm\n").unwrap();
    let listed = leo.ok(&["list", "cs130"]);
    assert!(listed.contains("Office hours"), "{listed}");

    let renamed = cs130.join("Lecture 4 - BFS.md");
    std::fs::rename(&lecture, &renamed).unwrap();
    let with_alias = std::fs::read_to_string(&renamed).unwrap().replacen(
        "tags:",
        "aliases:\n- BFS lecture\ntags:",
        1,
    );
    std::fs::write(&renamed, with_alias).unwrap();
    let listed = leo.ok(&["list", "cs130"]);
    assert!(
        listed.contains("Lecture 4 - BFS"),
        "rename not picked up:\n{listed}"
    );

    leo.ok(&["edit", "Lecture 4 - BFS"]);
    let edited = std::fs::read_to_string(&renamed).unwrap();
    assert!(edited.contains("written in the editor"), "{edited}");
    assert!(
        edited.contains("BFS lecture"),
        "the alias was lost:\n{edited}"
    );
    assert!(!lecture.exists(), "the file was renamed back");

    assert_eq!(
        std::fs::read_to_string(cs130.join("Office hours.md")).unwrap(),
        "Tuesday 3-5pm\n",
        "a note leo did not change was rewritten"
    );

    leo.ok(&["delete", "Office hours", "--force"]);
    assert!(!cs130.join("Office hours.md").exists());
    assert!(leo.ok(&["trash"]).contains("Office hours"));
}

#[test]
fn a_note_duplicated_in_obsidian_is_a_second_note_and_editing_keeps_both() {
    let leo = Leo::new();
    leo.ok(&["new", "Lecture 4", "--body", "the original"]);
    let original = leo.notes_dir().join("Lecture 4.md");
    let an_hour_ago = std::time::SystemTime::now() - Duration::from_secs(3600);
    std::fs::File::options()
        .write(true)
        .open(&original)
        .unwrap()
        .set_modified(an_hour_ago)
        .unwrap();
    let copy = leo.notes_dir().join("Lecture 4 1.md");
    std::fs::write(
        &copy,
        std::fs::read_to_string(&original)
            .unwrap()
            .replace("the original", "the copy"),
    )
    .unwrap();

    let id = std::fs::read_to_string(&original)
        .unwrap()
        .lines()
        .find_map(|l| l.strip_prefix("id: ").map(str::to_string))
        .unwrap();
    leo.ok(&["edit", &id]);

    let original_text = std::fs::read_to_string(&original).unwrap();
    let copy_text = std::fs::read_to_string(&copy).unwrap();
    assert!(original_text.contains("the original"), "{original_text}");
    assert!(
        original_text.contains("written in the editor"),
        "{original_text}"
    );
    assert!(
        copy_text.contains("the copy"),
        "the duplicate was overwritten:\n{copy_text}"
    );
}

fn fake_opener(leo: &Leo, obsidian_running: bool) -> (PathBuf, PathBuf) {
    let log = leo.home.path().join("opened");
    let clipboard = leo.home.path().join("clipboard");
    for name in ["open", "xdg-open"] {
        let script = leo.bin.join(name);
        std::fs::write(
            &script,
            format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\n", log.display()),
        )
        .unwrap();
        make_executable(&script);
    }
    for name in ["pbcopy", "wl-copy", "xclip", "xsel"] {
        let script = leo.bin.join(name);
        std::fs::write(
            &script,
            format!("#!/bin/sh\ncat > '{}'\n", clipboard.display()),
        )
        .unwrap();
        make_executable(&script);
    }
    let pgrep = leo.bin.join("pgrep");
    std::fs::write(
        &pgrep,
        format!("#!/bin/sh\nexit {}\n", if obsidian_running { 0 } else { 1 }),
    )
    .unwrap();
    make_executable(&pgrep);
    (log, clipboard)
}

fn obsidian_config(leo: &Leo) -> PathBuf {
    if cfg!(target_os = "macos") {
        leo.home
            .path()
            .join("Library/Application Support/obsidian/obsidian.json")
    } else {
        leo.home.path().join(".config/obsidian/obsidian.json")
    }
}

fn run_obsidian(leo: &Leo) -> Output {
    let marker = leo.home.path().join("Obsidian.app");
    std::fs::create_dir_all(&marker).unwrap();
    leo.cmd(&["obsidian"])
        .env("LEO_OBSIDIAN_APP", &marker)
        .output()
        .unwrap()
}

#[test]
fn obsidian_adds_the_notes_folder_as_a_vault_and_opens_it() {
    let leo = Leo::new();
    let (log, _clipboard) = fake_opener(&leo, false);
    let config = obsidian_config(&leo);
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::fs::write(
        &config,
        r#"{"vaults":{"aaaa000000000001":{"path":"/Users/me/Vault","ts":1,"open":true}},"frame":"hidden"}"#,
    )
    .unwrap();

    let out = run_obsidian(&leo);
    assert!(out.status.success(), "{}", describe(&out));

    let value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
    assert_eq!(value["frame"], "hidden", "another setting was lost");
    assert_eq!(
        value["vaults"]["aaaa000000000001"]["path"],
        "/Users/me/Vault"
    );
    let vaults = value["vaults"].as_object().unwrap();
    let (id, vault) = vaults
        .iter()
        .find(|(_, v)| v["path"].as_str().unwrap_or("").ends_with("notes"))
        .expect("the notes folder was not added");
    assert!(
        config.with_extension("json.leo-backup").exists(),
        "no backup kept"
    );

    let opened = std::fs::read_to_string(&log).unwrap();
    assert!(
        opened.contains(&format!("obsidian://open?vault={id}")),
        "{opened}"
    );
    assert!(vault["ts"].as_u64().unwrap() > 0);

    let again = run_obsidian(&leo);
    assert!(again.status.success());
    let after: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
    assert_eq!(
        after["vaults"].as_object().unwrap().len(),
        2,
        "a second run added the folder twice"
    );
}

#[test]
fn obsidian_already_running_is_not_edited_and_the_steps_are_given() {
    let leo = Leo::new();
    let (log, clipboard) = fake_opener(&leo, true);
    let out = run_obsidian(&leo);
    assert!(out.status.success(), "{}", describe(&out));
    assert!(
        !obsidian_config(&leo).exists(),
        "edited a running Obsidian's list"
    );
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(said.contains("Open folder as vault"), "{said}");
    assert!(std::fs::read_to_string(&clipboard)
        .unwrap()
        .ends_with("notes"));
    assert!(std::fs::read_to_string(&log)
        .unwrap()
        .contains("obsidian://"));
}

#[test]
fn obsidian_without_it_installed_says_where_to_get_it() {
    let leo = Leo::new();
    let (log, _clipboard) = fake_opener(&leo, false);
    let out = leo
        .cmd(&["obsidian"])
        .env("LEO_OBSIDIAN_APP", leo.home.path().join("missing"))
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("obsidian.md"),
        "{}",
        describe(&out)
    );
    assert!(!log.exists(), "opened something anyway");
}

fn tripwire(leo: &Leo) -> (PathBuf, PathBuf) {
    let ran = leo.home.path().join("installer-ran");
    let script = leo.home.path().join("tripwire.sh");
    std::fs::write(&script, format!("#!/bin/sh\ntouch '{}'\n", ran.display())).unwrap();
    (script, ran)
}

#[test]
fn update_does_nothing_when_already_on_the_latest_version() {
    let leo = Leo::new();
    let exe = leo.installed();
    let (script, ran) = tripwire(&leo);
    let out = leo
        .cmd_at(&exe, &["update"])
        .env("LEO_UPDATE_SCRIPT", &script)
        .env("LEO_LATEST_RELEASE", env!("CARGO_PKG_VERSION"))
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{}", describe(&out));
    assert!(said.contains("latest"), "{said}");
    assert!(!ran.exists(), "downloaded anyway");
}

#[test]
fn update_installs_when_a_newer_version_is_out_or_when_forced() {
    let leo = Leo::new();
    let exe = leo.installed();
    let (script, ran) = tripwire(&leo);
    let newer = leo
        .cmd_at(&exe, &["update"])
        .env("LEO_UPDATE_SCRIPT", &script)
        .env("LEO_LATEST_RELEASE", "999.0.0")
        .output()
        .unwrap();
    assert!(newer.status.success(), "{}", describe(&newer));
    assert!(String::from_utf8_lossy(&newer.stdout).contains("999.0.0"));
    assert!(ran.exists(), "did not install the newer version");

    std::fs::remove_file(&ran).unwrap();
    leo.cmd_at(&exe, &["update", "--force"])
        .env("LEO_UPDATE_SCRIPT", &script)
        .env("LEO_LATEST_RELEASE", env!("CARGO_PKG_VERSION"))
        .output()
        .unwrap();
    assert!(ran.exists(), "--force did not reinstall");
}

#[test]
fn update_reinstalls_in_place() {
    let leo = Leo::new();
    let exe = leo.installed();
    let staging = leo.home.path().join("staging");
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_leo"), staging.join("leo")).unwrap();
    let tarball = leo.home.path().join("leo.tar.gz");
    assert!(Command::new("tar")
        .arg("-czf")
        .arg(&tarball)
        .arg("-C")
        .arg(&staging)
        .arg("leo")
        .status()
        .unwrap()
        .success());

    let out = leo
        .cmd_at(&exe, &["update", "--force"])
        .env("SHELL", "/bin/zsh")
        .env(
            "LEO_UPDATE_SCRIPT",
            Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh"),
        )
        .env("LEO_INSTALL_ARCHIVE", &tarball)
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{}", describe(&out));
    assert!(said.contains("already up to date"), "{said}");
    assert!(
        said.contains("~/.local/bin/leo"),
        "not updated in place:\n{said}"
    );
    assert!(exe.exists());
    assert!(
        !leo.home.path().join(".zshrc").exists(),
        "an update edited the shell's startup file"
    );
}

fn sha256_of(path: &Path) -> String {
    let run = |program: &str, args: &[&str]| {
        Command::new(program)
            .args(args)
            .arg(path)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
    };
    run("shasum", &["-a", "256"])
        .or_else(|| run("sha256sum", &[]))
        .expect("no sha256 tool")
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

#[test]
fn update_downloads_the_speech_model_first_when_it_is_missing() {
    let leo = Leo::new();
    let exe = leo.installed();
    let (script, ran) = tripwire(&leo);
    let source = leo.home.path().join("fake-model.bin");
    std::fs::write(&source, b"a small stand-in for base.en").unwrap();
    let sha = "0e2cb5e4b8ad0c9ee4d74c4e0d7d0b4d25b37b1c2f3b6b8d5d1b0f5b7b2f2a1c";
    let out = leo
        .cmd_at(&exe, &["update"])
        .env_remove("LEO_INSTALL_SKIP_MODEL")
        .env(
            "LEO_INSTALL_MODEL_URL",
            format!("file://{}", source.display()),
        )
        .env("LEO_INSTALL_MODEL_SHA256", sha)
        .env("LEO_UPDATE_SCRIPT", &script)
        .env("LEO_LATEST_RELEASE", env!("CARGO_PKG_VERSION"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", describe(&out));
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(said.contains("speech model"), "{said}");
    assert!(
        said.contains("damaged"),
        "a wrong checksum was accepted:\n{said}"
    );
    let model = leo.home.path().join("models/ggml-base.en.bin");
    assert!(!model.exists());
    assert!(!ran.exists());

    let real = sha256_of(&source);
    let out = leo
        .cmd_at(&exe, &["update"])
        .env_remove("LEO_INSTALL_SKIP_MODEL")
        .env(
            "LEO_INSTALL_MODEL_URL",
            format!("file://{}", source.display()),
        )
        .env("LEO_INSTALL_MODEL_SHA256", &real)
        .env("LEO_UPDATE_SCRIPT", &script)
        .env("LEO_LATEST_RELEASE", env!("CARGO_PKG_VERSION"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", describe(&out));
    assert_eq!(
        std::fs::read(&model).unwrap(),
        b"a small stand-in for base.en"
    );

    let again = leo
        .cmd_at(&exe, &["update"])
        .env_remove("LEO_INSTALL_SKIP_MODEL")
        .env("LEO_INSTALL_MODEL_URL", "file:///nonexistent")
        .env("LEO_UPDATE_SCRIPT", &script)
        .env("LEO_LATEST_RELEASE", env!("CARGO_PKG_VERSION"))
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&again.stdout);
    assert!(!said.contains("speech model"), "downloaded twice:\n{said}");
}

fn installer_block(dir: &Path) -> String {
    format!(
        "\n# Added by the leo installer\nexport PATH=\"{}:$PATH\"\n",
        dir.display()
    )
}

#[test]
fn uninstall_removes_everything_leo_made_except_the_notes() {
    let leo = Leo::new();
    leo.ok(&["new", "Keep me", "--body", "x"]);
    let home = leo.home.path();
    for made in [
        "config.toml",
        "credentials.json",
        "serve-token",
        "update-check.json",
        "recent.json",
        ".env",
    ] {
        std::fs::write(home.join(made), "x").unwrap();
    }
    std::fs::create_dir_all(home.join(".leo/models")).unwrap();
    std::fs::write(home.join(".leo/models/ggml-base.en.bin"), "model").unwrap();
    std::fs::create_dir_all(home.join("models")).unwrap();
    std::fs::write(home.join("models/ggml-base.en.bin"), "model").unwrap();
    std::fs::write(home.join("my-own-file.txt"), "not leo's").unwrap();
    let exe = leo.installed();

    let out = leo.cmd_at(&exe, &["uninstall", "--yes"]).output().unwrap();
    assert!(out.status.success(), "{}", describe(&out));
    for made in [
        "config.toml",
        "credentials.json",
        "serve-token",
        "update-check.json",
        "recent.json",
        ".env",
        ".leo",
        "models",
        ".manual-installed",
    ] {
        assert!(!home.join(made).exists(), "{made} is still there");
    }
    assert!(
        home.join("my-own-file.txt").exists(),
        "removed a file that is not leo's"
    );
    assert!(
        leo.files().iter().any(|f| f.contains("Keep me")),
        "notes went too"
    );
    assert!(home.join("notes").is_dir());
}

#[test]
fn uninstall_removes_leo_and_its_path_line_but_keeps_the_notes() {
    let leo = Leo::new();
    leo.ok(&["new", "Keep me", "--body", "x"]);
    let exe = leo.installed();
    let zshrc = leo.home.path().join(".zshrc");
    let mine = "alias ll='ls -l'\n";
    std::fs::write(
        &zshrc,
        format!("{mine}{}", installer_block(exe.parent().unwrap())),
    )
    .unwrap();

    let out = leo.cmd_at(&exe, &["uninstall", "--yes"]).output().unwrap();
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{}", describe(&out));
    assert!(!exe.exists(), "leo is still installed");
    assert_eq!(std::fs::read_to_string(&zshrc).unwrap(), mine);
    assert!(said.contains("~/.zshrc"), "{said}");
    assert!(
        leo.files().iter().any(|f| f.contains("Keep me")),
        "notes went too"
    );
    assert!(
        said.contains("notes"),
        "does not say the notes stay:\n{said}"
    );
}

#[test]
fn uninstall_asks_first() {
    let leo = Leo::new();
    let exe = leo.installed();
    let out = leo.cmd_at(&exe, &["uninstall"]).output().unwrap();
    assert!(!out.status.success());
    assert!(exe.exists(), "removed without asking");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("--yes"),
        "{}",
        describe(&out)
    );
}

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
fn delete_moves_the_note_to_the_trash_without_asking_and_says_how_to_get_it_back() {
    let leo = Leo::new();
    leo.ok(&["new", "Keeper", "--body", "x"]);
    let said = leo.ok(&["delete", "Keeper"]);
    assert!(said.contains("leo trash restore"), "{said}");
    assert!(!leo.files().iter().any(|f| f.contains("title: Keeper")));
    leo.ok(&["trash", "restore", "Keeper"]);
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
fn backup_sends_notes_to_a_git_remote() {
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
    leo.ok(&["backup", "init"]);
    leo.ok(&["backup", "connect", remote.to_str().unwrap()]);
    leo.ok(&["new", "Second", "--body", "two"]);
    leo.ok(&["backup"]);

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
    laptop.ok(&["backup", "connect", url]);
    laptop.ok(&["backup"]);

    desktop.ok(&["new", "Written on the desktop", "--body", "two"]);
    desktop.ok(&["backup", "connect", url]);
    desktop.ok(&["backup"]);
    let listed = desktop.ok(&["list"]);
    assert!(listed.contains("Written on the laptop"), "{listed}");
    assert!(listed.contains("Written on the desktop"), "{listed}");

    laptop.ok(&["backup"]);
    assert!(laptop.ok(&["list"]).contains("Written on the desktop"));

    let manuals: Vec<String> = desktop
        .files()
        .into_iter()
        .filter(|f| f.contains("title: leo manual"))
        .collect();
    assert_eq!(
        manuals.len(),
        2,
        "each computer's manual note should survive"
    );
    for manual in &manuals {
        assert_eq!(
            manual.matches("\nid: ").count(),
            1,
            "two notes were merged into one:\n{manual}"
        );
    }
}

fn fake_gh(leo: &Leo, github: &Path) {
    let script = format!(
        r#"#!/bin/sh
case "$1 $2" in
    "auth token") echo gho_fake ;;
    "auth setup-git") ;;
    "config get") echo https ;;
    "repo view")
        [ -d "{github}/$3.git" ] || {{ echo "Could not resolve to a Repository" >&2; exit 1; }}
        echo "{github}/$3.git" ;;
    "repo create") git init -q --bare "{github}/$3.git" ;;
    *) echo "fake gh: $*" >&2; exit 2 ;;
esac
"#,
        github = github.display()
    );
    let gh = leo.bin.join("gh");
    std::fs::write(&gh, script).unwrap();
    make_executable(&gh);
}

#[test]
fn backup_github_makes_the_repository_then_a_second_computer_joins_it() {
    if !has_git() {
        eprintln!("skipping: git is not installed");
        return;
    }
    let laptop = Leo::new();
    let desktop = Leo::new();
    let github = laptop.home.path().join("github");
    std::fs::create_dir_all(&github).unwrap();
    fake_gh(&laptop, &github);
    fake_gh(&desktop, &github);

    laptop.ok(&["new", "Written on the laptop", "--body", "one"]);
    let made = laptop.ok(&["backup", "github"]);
    assert!(github.join("leo-notes.git").is_dir(), "no repository made");
    assert!(made.contains("leo-notes"), "{made}");
    assert!(made.to_lowercase().contains("made"), "{made}");

    desktop.ok(&["new", "Written on the desktop", "--body", "two"]);
    let joined = desktop.ok(&["backup", "github"]);
    assert!(joined.to_lowercase().contains("joined"), "{joined}");
    let listed = desktop.ok(&["list"]);
    assert!(listed.contains("Written on the laptop"), "{listed}");

    laptop.ok(&["backup"]);
    assert!(laptop.ok(&["list"]).contains("Written on the desktop"));
}

#[test]
fn backup_github_without_the_tool_says_how_to_get_it() {
    let leo = Leo::new();
    let out = leo.cmd(&["backup", "github"]).output().unwrap();
    assert!(!out.status.success());
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(said.contains("gh auth login"), "{}", describe(&out));
}

#[test]
fn backup_before_setup_says_what_to_do() {
    let leo = Leo::new();
    let out = leo.cmd(&["backup"]).output().unwrap();
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("leo backup"),
        "{}",
        describe(&out)
    );
}

// ── the web server ──────────────────────────────────────────────────────────

struct Serving {
    child: std::process::Child,
    port: u16,
    lines: std::sync::mpsc::Receiver<String>,
}

impl Serving {
    fn start(leo: &Leo, extra: &[&str]) -> Serving {
        static NEXT: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(0);
        let port = 38000
            + (std::process::id() % 500) as u16 * 4
            + NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let port_arg = port.to_string();
        let mut args = vec!["serve", "--port", &port_arg];
        args.extend_from_slice(extra);
        let mut child = leo
            .cmd(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let _ = tx.send(line);
            }
        });
        Serving { child, port, lines }
    }

    fn link(&self, needle: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if let Ok(line) = self.lines.recv_timeout(Duration::from_millis(200)) {
                if let Some(i) = line.find("http") {
                    let link: String = line[i..]
                        .chars()
                        .take_while(|c| !c.is_whitespace())
                        .collect();
                    if link.contains(needle) {
                        return link;
                    }
                }
            }
        }
        panic!("serve never printed a link with {needle:?}");
    }

    fn token(&self) -> String {
        let link = self.link("token=");
        let i = link.find("token=").unwrap();
        link[i + 6..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect()
    }

    fn get(&self, path: &str, headers: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Ok(mut s) = std::net::TcpStream::connect(("127.0.0.1", self.port)) {
                s.set_read_timeout(Some(Duration::from_secs(5))).ok();
                write!(
                    s,
                    "GET {path} HTTP/1.1\r\nHost: localhost\r\n{headers}Connection: close\r\n\r\n"
                )
                .unwrap();
                let mut response = String::new();
                s.read_to_string(&mut response).unwrap();
                return response;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("the server did not answer");
    }
}

impl Drop for Serving {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn serve_needs_its_token_and_then_lists_the_notes() {
    let leo = Leo::new();
    leo.ok(&["new", "Served note", "--body", "x"]);
    let server = Serving::start(&leo, &["--local"]);
    let token = server.token();

    let refused = server.get("/api/notes", "");
    assert!(
        refused.starts_with("HTTP/1.1 401"),
        "no code was accepted:\n{refused}"
    );
    let wrong = server.get("/api/notes?token=0000", "");
    assert!(
        wrong.starts_with("HTTP/1.1 401"),
        "a wrong code was accepted:\n{wrong}"
    );
    let allowed = server.get(&format!("/api/notes?token={token}"), "");
    assert!(allowed.starts_with("HTTP/1.1 200"), "{allowed}");
    assert!(allowed.contains("Served note"), "{allowed}");
}

#[test]
fn opening_the_link_keeps_the_code_out_of_the_address_bar() {
    let leo = Leo::new();
    let server = Serving::start(&leo, &["--local"]);
    let token = server.token();

    let first = server.get(&format!("/?token={token}"), "");
    assert!(first.starts_with("HTTP/1.1 303"), "{first}");
    assert!(first.to_lowercase().contains("location: /\r\n"), "{first}");
    let cookie = first
        .lines()
        .find(|l| l.to_lowercase().starts_with("set-cookie:"))
        .expect("no cookie set");
    assert!(cookie.contains("HttpOnly"), "{cookie}");

    let page = server.get("/", &format!("Cookie: leo_token={token}\r\n"));
    assert!(page.starts_with("HTTP/1.1 200"), "{page}");
    let lower = page.to_lowercase();
    assert!(lower.contains("referrer-policy: no-referrer"), "{page}");
    assert!(lower.contains("x-content-type-options: nosniff"), "{page}");
    assert!(
        !lower.contains("access-control-allow-origin"),
        "any site may call it:\n{page}"
    );
    assert!(
        lower.contains("content-security-policy: default-src 'none'; script-src 'self'"),
        "{page}"
    );
    assert!(page.contains("<script src=\"/app.js\">"), "{page}");

    for script in ["/app.js", "/markdown.js", "/editing.js", "/doc.js"] {
        let served = server.get(script, &format!("Cookie: leo_token={token}\r\n"));
        assert!(served.starts_with("HTTP/1.1 200"), "{script}: {served}");
        assert!(
            served
                .to_lowercase()
                .contains("content-type: text/javascript"),
            "{served}"
        );
        assert!(
            server.get(script, "").starts_with("HTTP/1.1 401"),
            "{script} without the code"
        );
    }

    let tunneled = server.get(&format!("/?token={token}"), "X-Forwarded-Proto: https\r\n");
    assert!(tunneled.contains("Secure"), "{tunneled}");

    let lost = server.get("/", "");
    assert!(lost.starts_with("HTTP/1.1 401"), "{lost}");
    assert!(lost.contains("leo serve"), "{lost}");
}

#[test]
fn the_link_survives_a_restart_until_a_new_one_is_asked_for() {
    let leo = Leo::new();
    let first = Serving::start(&leo, &["--local"]).token();
    let again = Serving::start(&leo, &["--local"]).token();
    assert_eq!(first, again);
    let fresh = Serving::start(&leo, &["--local", "--new-token"]);
    let new = fresh.token();
    assert_ne!(new, first);
    let old = fresh.get(&format!("/api/notes?token={first}"), "");
    assert!(
        old.starts_with("HTTP/1.1 401"),
        "the old link still works:\n{old}"
    );
}

#[test]
fn the_server_sees_notes_added_while_it_runs() {
    let leo = Leo::new();
    let server = Serving::start(&leo, &["--local"]);
    let token = server.token();
    leo.ok(&["new", "Added while serving", "--body", "x"]);
    let listed = server.get(&format!("/api/notes?token={token}"), "");
    assert!(listed.contains("Added while serving"), "{listed}");
}

#[test]
fn serve_prints_the_link_that_works_from_anywhere() {
    let leo = Leo::new();
    let fake = leo.bin.join("cloudflared");
    std::fs::write(
        &fake,
        "#!/bin/sh\necho 'INF |  https://quiet-fox-123.trycloudflare.com  |' >&2\nexec sleep 30\n",
    )
    .unwrap();
    make_executable(&fake);
    let server = Serving::start(&leo, &[]);
    let link = server.link("trycloudflare.com");
    assert!(
        link.starts_with("https://quiet-fox-123.trycloudflare.com/?token="),
        "{link}"
    );
}

#[test]
fn serve_without_cloudflared_says_how_to_get_it_and_about_local() {
    let leo = Leo::new();
    let out = leo.cmd(&["serve", "--port", "38999"]).output().unwrap();
    assert!(!out.status.success());
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(said.contains("cloudflared"), "{}", describe(&out));
    assert!(said.contains("--local"), "{}", describe(&out));
}

// ── long recordings ─────────────────────────────────────────────────────────

fn fake_transcription(leo: &Leo) {
    let whisper = leo.bin.join("fake-whisper");
    std::fs::write(
        &whisper,
        "#!/bin/sh\nwhile [ \"$1\" != \"-f\" ]; do shift; done\nprintf 'heard %s\\n' \"$(basename \"$2\")\"\n",
    )
    .unwrap();
    make_executable(&whisper);
    let model = leo.home.path().join("model.bin");
    std::fs::write(&model, "model").unwrap();
    std::fs::write(
        leo.home.path().join("config.toml"),
        format!(
            "[chat]\nchain = []\n\n[transcribe]\nchain = [\"fake\"]\n\n[providers.fake]\nkind = \"whisper_cpp\"\nbin = \"{}\"\nmodel_path = \"{}\"\n",
            whisper.display(),
            model.display()
        ),
    )
    .unwrap();
}

fn tone(path: &Path, secs: u64) {
    let samples: Vec<i16> = (0..secs * 16_000)
        .map(|i| if (i / 20) % 2 == 0 { 3000 } else { -3000 })
        .collect();
    leo_services::session::wav::write(path, &samples).unwrap();
}

fn record_until_the_audio_ends(leo: &Leo, audio: &Path) -> std::process::Output {
    let mut child = leo
        .cmd(&["record"])
        .env("LEO_FAKE_AUDIO", audio)
        .env("LEO_FAKE_SPEED", "300")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let keep_open = child.stdin.take();
    let started = Instant::now();
    while child.try_wait().unwrap().is_none() {
        assert!(
            started.elapsed() < Duration::from_secs(120),
            "leo record never finished"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    drop(keep_open);
    child.wait_with_output().unwrap()
}

#[test]
fn a_long_recording_is_saved_in_order_even_with_no_ai_to_write_notes() {
    let leo = Leo::new();
    fake_transcription(&leo);
    let audio = leo.home.path().join("lecture.wav");
    tone(&audio, 22 * 60);

    let out = record_until_the_audio_ends(&leo, &audio);
    assert!(out.status.success(), "{}", describe(&out));
    let files = leo.files();
    let note = files
        .iter()
        .find(|f| f.contains("heard seg-00000.wav"))
        .unwrap_or_else(|| panic!("no note with the transcript:\n{}", describe(&out)));
    let mut last = 0;
    for i in 0..5 {
        let at = note
            .find(&format!("heard seg-{i:05}.wav"))
            .unwrap_or_else(|| panic!("segment {i} is missing:\n{note}"));
        assert!(at >= last, "segment {i} is out of order");
        last = at;
    }
    assert!(note.contains("title: Recording,"), "{note}");
    let recordings = leo.home.path().join("recordings");
    let left: Vec<_> = std::fs::read_dir(&recordings)
        .map(|d| d.flatten().collect())
        .unwrap_or_default();
    assert!(
        left.is_empty(),
        "the recording was not cleaned up after saving"
    );
}

#[test]
fn an_interrupted_recording_is_finished_before_the_next_one_starts() {
    let leo = Leo::new();
    fake_transcription(&leo);
    let left = leo.home.path().join("recordings/20260101-090000");
    std::fs::create_dir_all(&left).unwrap();
    std::fs::write(
        left.join("session.json"),
        "{\"started\":\"2026-01-01T09:00:00Z\",\"title\":\"Before the crash\",\"segment_secs\":300,\"points\":[{\"at_secs\":10,\"text\":\"remember this\"}]}",
    )
    .unwrap();
    tone(&left.join("seg-00000.wav"), 300);
    tone(&left.join("seg-00001.part.wav"), 40);

    let audio = leo.home.path().join("short.wav");
    tone(&audio, 20);
    let out = record_until_the_audio_ends(&leo, &audio);
    assert!(out.status.success(), "{}", describe(&out));
    let files = leo.files();
    let recovered = files
        .iter()
        .find(|f| f.contains("title: Before the crash"))
        .unwrap_or_else(|| {
            panic!(
                "the interrupted recording was not saved:\n{}",
                describe(&out)
            )
        });
    assert!(recovered.contains("heard seg-00000.wav"), "{recovered}");
    assert!(
        recovered.contains("heard seg-00001.wav"),
        "the part cut short by the crash was lost"
    );
    assert!(
        recovered.contains("remember this"),
        "a typed point was lost"
    );
    assert!(!left.exists());
    assert!(
        files.iter().any(|f| f.contains("title: Recording,")),
        "the new recording was not saved"
    );
}
