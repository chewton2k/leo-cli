//! End-to-end: the real `leo` binary, run as a user would run it.
//!
//! Every test gets its own `LEO_HOME`, a scrubbed environment and a PATH holding
//! only the system directories, so nothing here can read the user's
//! notes, their API keys, their keychain, or reach the network — and no test
//! can open a microphone, since LEO_NO_MICROPHONE is always set.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

struct Leo {
    home: tempfile::TempDir,
    bin: PathBuf,
}

impl Leo {
    fn new() -> Leo {
        let home = tempfile::tempdir().unwrap();
        let bin = home.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        Leo { home, bin }
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
            .env("NO_COLOR", "1")
            .env("LEO_NO_UPDATE_CHECK", "1")
            .env("LEO_NO_MICROPHONE", "1")
            .env("LEO_NO_OPEN", "1")
            .env("LEO_INSTALL_NO_MODEL", "1")
            .env("LEO_NO_MEANING_MODEL", "1")
            .stdin(Stdio::null());
        cmd
    }

    fn note(&self, title: &str, body: &str) {
        std::fs::create_dir_all(self.notes_dir()).unwrap();
        std::fs::write(self.notes_dir().join(format!("{title}.md")), body).unwrap();
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

fn make_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

// ── the basics ──────────────────────────────────────────────────────────────

// ── notes ───────────────────────────────────────────────────────────────────

trait Retrying {
    fn output_retrying(&mut self) -> std::process::Output;
}

impl Retrying for Command {
    fn output_retrying(&mut self) -> std::process::Output {
        for _ in 0..100 {
            match self.output() {
                Ok(out) => return out,
                Err(e) if e.raw_os_error() == Some(26) => {
                    std::thread::sleep(Duration::from_millis(50))
                }
                Err(e) => panic!("could not run {self:?}: {e}"),
            }
        }
        panic!("{self:?} stayed busy")
    }
}

fn tripwire(leo: &Leo) -> (PathBuf, PathBuf) {
    let ran = leo.home.path().join("installer-ran");
    let script = leo.home.path().join("tripwire.sh");
    std::fs::write(&script, format!("#!/bin/sh\nenv > '{}'\n", ran.display())).unwrap();
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
        .output_retrying();
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
        .env_remove("LEO_INSTALL_NO_MODEL")
        .env("LEO_INSTALL_MODEL_URL", "file:///nonexistent")
        .env("LEO_UPDATE_SCRIPT", &script)
        .env("LEO_LATEST_RELEASE", "999.0.0")
        .output_retrying();
    assert!(newer.status.success(), "{}", describe(&newer));
    assert!(String::from_utf8_lossy(&newer.stdout).contains("999.0.0"));
    assert!(ran.exists(), "did not install the newer version");
    let installer_env = std::fs::read_to_string(&ran).unwrap();
    assert!(
        !installer_env.contains("LEO_INSTALL_SKIP_MODEL"),
        "the installer was told to skip the speech model"
    );
    let said = String::from_utf8_lossy(&newer.stdout);
    assert!(
        !said.contains("speech model"),
        "the old leo checked the model instead of leaving it to the new installer:\n{said}"
    );

    std::fs::remove_file(&ran).unwrap();
    leo.cmd_at(&exe, &["update", "--force"])
        .env("LEO_UPDATE_SCRIPT", &script)
        .env("LEO_LATEST_RELEASE", env!("CARGO_PKG_VERSION"))
        .output_retrying();
    assert!(ran.exists(), "--force did not reinstall");
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
fn update_fetches_the_model_that_finds_notes_by_meaning_once() {
    let leo = Leo::new();
    let exe = leo.installed();
    let (script, ran) = tripwire(&leo);
    let source = leo.home.path().join("meaning-source");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("config.json"), b"{}").unwrap();
    let manifest = format!("config.json={}", sha256_of(&source.join("config.json")));
    std::fs::write(
        leo.home.path().join("speech-model.json"),
        r#"{"id":"fixture-unknown-model","language":"auto"}"#,
    )
    .unwrap();
    let update = || {
        leo.cmd_at(&exe, &["update"])
            .env_remove("LEO_NO_MEANING_MODEL")
            .env_remove("LEO_INSTALL_NO_MODEL")
            .env("LEO_MEANING_URL", format!("file://{}", source.display()))
            .env("LEO_MEANING_MANIFEST", &manifest)
            .env("LEO_UPDATE_SCRIPT", &script)
            .env("LEO_LATEST_RELEASE", env!("CARGO_PKG_VERSION"))
            .output_retrying()
    };
    let out = update();
    assert!(out.status.success(), "{}", describe(&out));
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(said.contains("finds notes by meaning"), "{said}");
    assert!(leo
        .home
        .path()
        .join("models/bge-small-en-v1.5/config.json")
        .is_file());
    let again = update();
    assert!(!String::from_utf8_lossy(&again.stdout).contains("by meaning"));
    assert!(!ran.exists());
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
    leo.note("Keep me", "Keep me");
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

    let out = leo.cmd_at(&exe, &["uninstall", "--yes"]).output_retrying();
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
    leo.note("Keep me", "Keep me");
    let exe = leo.installed();
    let zshrc = leo.home.path().join(".zshrc");
    let mine = "alias ll='ls -l'\n";
    std::fs::write(
        &zshrc,
        format!("{mine}{}", installer_block(exe.parent().unwrap())),
    )
    .unwrap();

    let out = leo.cmd_at(&exe, &["uninstall", "--yes"]).output_retrying();
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
    let out = leo.cmd_at(&exe, &["uninstall"]).output_retrying();
    assert!(!out.status.success());
    assert!(exe.exists(), "removed without asking");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("--yes"),
        "{}",
        describe(&out)
    );
}

#[test]
fn a_failed_installer_download_cannot_report_a_successful_update() {
    let leo = Leo::new();
    let exe = leo.installed();
    let curl = leo.bin.join("curl");
    std::fs::write(&curl, "#!/bin/sh\nexit 7\n").unwrap();
    make_executable(&curl);
    let out = leo.cmd_at(&exe, &["update", "--force"]).output_retrying();
    assert!(
        !out.status.success(),
        "a failed download reported success: {}",
        describe(&out)
    );
    assert!(exe.exists());
}

#[test]
fn a_debug_worker_segv_fault_is_a_signal_without_undefined_rust_operations() {
    use std::os::unix::process::ExitStatusExt;
    let leo = Leo::new();
    let mut child = leo
        .cmd_at(
            Path::new(env!("CARGO_BIN_EXE_leo")),
            &["--leo-speech-worker", "--cpu-only"],
        )
        .env("LEO_WORKER_FAULT", "segv@init")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let input = child.stdin.take().unwrap();
    for _ in 0..200 {
        if let Some(status) = child.try_wait().unwrap() {
            drop(input);
            assert_eq!(status.signal(), Some(libc::SIGSEGV));
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let _ = child.kill();
    let _ = child.wait();
    panic!("faulted worker did not terminate");
}
