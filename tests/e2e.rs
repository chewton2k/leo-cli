//! End-to-end: the real `leo` binary, run as a user would run it.
//!
//! Every test gets its own `LEO_HOME`, a scrubbed environment and a PATH holding
//! only the system directories plus git, so nothing here can read the user's
//! notes, their API keys, their keychain, or reach the network — and no test
//! can open a microphone, since LEO_NO_MICROPHONE is always set.
//! `$EDITOR` is a small script that appends a line to whatever file it is given.

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
            .env("LEO_NO_MICROPHONE", "1")
            .env("LEO_NO_OPEN", "1")
            .env("LEO_INSTALL_NO_MODEL", "1")
            .env("LEO_NO_MEANING_MODEL", "1")
            .env("GIT_AUTHOR_NAME", "leo test")
            .env("GIT_AUTHOR_EMAIL", "leo@example.com")
            .env("GIT_COMMITTER_NAME", "leo test")
            .env("GIT_COMMITTER_EMAIL", "leo@example.com")
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
        .output_retrying();
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
    let source = leo.home.path().join("fake-model");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(
        source.join("encoder.int8.onnx"),
        b"a small stand-in encoder",
    )
    .unwrap();
    std::fs::write(source.join("tokens.txt"), b"a b c").unwrap();
    let url = format!("file://{}", source.display());
    let manifest = |encoder: &str| {
        format!(
            "encoder.int8.onnx={encoder} tokens.txt={}",
            sha256_of(&source.join("tokens.txt"))
        )
    };
    let update = |url: &str, manifest: &str| {
        leo.cmd_at(&exe, &["update"])
            .env_remove("LEO_INSTALL_NO_MODEL")
            .env("LEO_INSTALL_MODEL_URL", url)
            .env("LEO_INSTALL_MODEL_MANIFEST", manifest)
            .env("LEO_UPDATE_SCRIPT", &script)
            .env("LEO_LATEST_RELEASE", env!("CARGO_PKG_VERSION"))
            .output_retrying()
    };
    let dir = leo.home.path().join("models/parakeet-tdt-0.6b-v3-int8");
    let encoder = dir.join("encoder.int8.onnx");
    let old = leo.home.path().join("models/ggml-base.en.bin");
    std::fs::create_dir_all(old.parent().unwrap()).unwrap();
    std::fs::write(&old, b"the old whisper model").unwrap();

    let out = update(&url, &manifest(&"0".repeat(64)));
    assert!(out.status.success(), "{}", describe(&out));
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(said.contains("speech model"), "{said}");
    assert!(
        said.contains("damaged"),
        "a wrong checksum was accepted:\n{said}"
    );
    assert!(!encoder.exists());
    assert!(!old.exists(), "the unused base.en was kept");
    assert!(!ran.exists());

    let real = manifest(&sha256_of(&source.join("encoder.int8.onnx")));
    let out = update(&url, &real);
    assert!(out.status.success(), "{}", describe(&out));
    assert_eq!(
        std::fs::read(&encoder).unwrap(),
        b"a small stand-in encoder"
    );
    assert!(dir.join("tokens.txt").is_file());
    assert!(!old.exists(), "the old whisper model was left behind");

    let again = update("file:///nonexistent", &real);
    let said = String::from_utf8_lossy(&again.stdout);
    assert!(!said.contains("speech model"), "downloaded twice:\n{said}");

    std::fs::write(&encoder, b"a small stand-in").unwrap();
    let repaired = update(&url, &real);
    let said = String::from_utf8_lossy(&repaired.stdout);
    assert!(said.contains("damaged"), "{said}");
    assert_eq!(
        std::fs::read(&encoder).unwrap(),
        b"a small stand-in encoder"
    );
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
    let update = || {
        leo.cmd_at(&exe, &["update"])
            .env_remove("LEO_NO_MEANING_MODEL")
            .env_remove("LEO_INSTALL_NO_MODEL")
            .env("LEO_INSTALL_MODEL_URL", "file:///nonexistent")
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
