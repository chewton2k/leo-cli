//! The one-line installer, run for real against a throwaway home directory.
//!
//! It is given a local archive of the binary cargo just built instead of
//! downloading one, so the test needs no network and no published release.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// A release-style archive: `leo` at the top level.
fn archive(dir: &Path) -> PathBuf {
    let staging = dir.join("staging");
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_leo"), staging.join("leo")).unwrap();
    let tarball = dir.join("leo.tar.gz");
    let ok = Command::new("tar")
        .arg("-czf")
        .arg(&tarball)
        .arg("-C")
        .arg(&staging)
        .arg("leo")
        .status()
        .unwrap()
        .success();
    assert!(ok, "could not build the test archive");
    tarball
}

fn install(home: &Path, shell: &str, tarball: &Path) -> String {
    install_with("sh", home, shell, tarball)
}

fn sha256(path: &Path) -> String {
    let out = ["shasum -a 256", "sha256sum"]
        .iter()
        .find_map(|tool| {
            Command::new("sh")
                .arg("-c")
                .arg(format!("{tool} '{}'", path.display()))
                .output()
                .ok()
                .filter(|o| o.status.success())
        })
        .expect("no sha256 tool");
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

const MODEL: &str = ".leo/models/parakeet-tdt-0.6b-v3-int8";

fn fake_model(home: &Path) -> (String, String) {
    let source = home.with_file_name("fake-model");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(
        source.join("encoder.int8.onnx"),
        b"a small stand-in encoder",
    )
    .unwrap();
    std::fs::write(source.join("tokens.txt"), b"a b c").unwrap();
    let manifest = ["encoder.int8.onnx", "tokens.txt"]
        .iter()
        .map(|name| format!("{name}={}", sha256(&source.join(name))))
        .collect::<Vec<_>>()
        .join(" ");
    (format!("file://{}", source.display()), manifest)
}

/// Run install.sh with a given interpreter, as `curl ... | <interpreter>` would.
fn install_with(interpreter: &str, home: &Path, shell: &str, tarball: &Path) -> String {
    let out = Command::new(interpreter)
        .arg(root().join("install.sh"))
        .env_clear()
        .env("HOME", home)
        .env("SHELL", shell)
        .env("PATH", "/usr/bin:/bin")
        .env("LEO_INSTALL_ARCHIVE", tarball)
        .env("LEO_INSTALL_MODEL_URL", fake_model(home).0)
        .env("LEO_INSTALL_MODEL_MANIFEST", fake_model(home).1)
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "install.sh failed:\n{text}");
    text
}

#[test]
fn installs_leo_and_puts_it_on_the_path_once() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let tarball = archive(tmp.path());

    let said = install(&home, "/bin/zsh", &tarball);
    let leo = home.join(".local/bin/leo");
    assert!(leo.is_file(), "leo was not installed:\n{said}");
    let version = Command::new(&leo).arg("--version").output().unwrap();
    assert!(version.status.success(), "the installed leo does not run");
    assert!(said.contains("leo doctor"), "no next step:\n{said}");
    let model = home.join(MODEL).join("encoder.int8.onnx");
    assert_eq!(
        std::fs::read(&model).unwrap(),
        b"a small stand-in encoder",
        "the speech model was not downloaded:\n{said}"
    );
    assert!(home.join(MODEL).join("tokens.txt").is_file());

    // Running it again must not add the PATH line a second time.
    let again = install(&home, "/bin/zsh", &tarball);
    assert!(again.contains("Speech model ready"), "{again}");

    std::fs::write(&model, b"a small stand-in enc").unwrap();
    let old = home.join(".leo/models/ggml-base.en.bin");
    std::fs::write(&old, b"the old whisper model").unwrap();
    let repaired = install(&home, "/bin/zsh", &tarball);
    assert!(repaired.contains("damaged"), "{repaired}");
    assert_eq!(
        std::fs::read(&model).unwrap(),
        b"a small stand-in encoder",
        "a damaged model was kept:\n{repaired}"
    );
    assert!(!old.exists(), "the old whisper model was left behind");
    let zshrc = std::fs::read_to_string(home.join(".zshrc")).unwrap();
    let lines = zshrc.lines().filter(|l| l.contains(".local/bin")).count();
    assert_eq!(lines, 1, "{zshrc}");
}

/// Mac terminals start bash as a login shell, which reads ~/.bash_profile, not
/// ~/.bashrc — the file a first-time user's PATH line most often goes missing
/// from.
#[test]
fn bash_gets_the_path_line_in_the_file_it_reads() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    install(&home, "/bin/bash", &archive(tmp.path()));

    let expected = if cfg!(target_os = "macos") {
        ".bash_profile"
    } else {
        ".bashrc"
    };
    let rc = std::fs::read_to_string(home.join(expected))
        .unwrap_or_else(|_| panic!("nothing written to {expected}"));
    assert!(rc.contains(".local/bin"), "{rc}");
}

/// The README offers `| bash` as well as `| sh`; both must work.
#[test]
fn the_installer_runs_under_bash_too() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let said = install_with("bash", &home, "/bin/bash", &archive(tmp.path()));
    assert!(home.join(".local/bin/leo").is_file(), "{said}");
}

#[test]
fn the_installer_ends_with_thanks_and_how_to_start() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let said = install(&home, "/bin/zsh", &archive(tmp.path()));

    let version = env!("CARGO_PKG_VERSION");
    for expected in [
        "Installed to ~/.local/bin/leo",
        &format!("leo {version} is installed"),
        "Thank you",
        "leo doctor",
        "open your notes",
        "https://github.com/chewton2k/leo-cli",
    ] {
        assert!(said.contains(expected), "no {expected:?}:\n{said}");
    }
    assert!(!said.contains("Inside leo"), "{said}");
    assert!(
        !said.contains('\u{1b}'),
        "color codes in plain output:\n{said}"
    );
}

#[test]
fn a_second_install_says_it_was_an_update() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let tarball = archive(tmp.path());
    install(&home, "/bin/zsh", &tarball);
    let again = install(&home, "/bin/zsh", &tarball);
    let version = env!("CARGO_PKG_VERSION");
    assert!(
        again.contains(&format!("already up to date ({version})")),
        "{again}"
    );
}

#[test]
fn a_damaged_speech_model_is_thrown_away_and_the_install_still_succeeds() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let tarball = archive(tmp.path());
    let (source, manifest) = fake_model(&home);
    let broken = manifest.replace(&manifest[manifest.len() - 64..], &"0".repeat(64));
    let out = Command::new("sh")
        .arg(root().join("install.sh"))
        .env_clear()
        .env("HOME", &home)
        .env("SHELL", "/bin/zsh")
        .env("PATH", "/usr/bin:/bin")
        .env("LEO_INSTALL_ARCHIVE", &tarball)
        .env("LEO_INSTALL_MODEL_URL", &source)
        .env("LEO_INSTALL_MODEL_MANIFEST", broken)
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{said}");
    assert!(
        said.contains("Could not download the speech model"),
        "{said}"
    );
    assert!(home.join(".local/bin/leo").is_file());
    let models = home.join(MODEL);
    assert!(!models.join("tokens.txt").exists());
    assert!(!models.join("tokens.txt.part").exists());
}

#[test]
fn leo_home_keeps_the_speech_model_inside_it() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let leo_home = tmp.path().join("leo-home");
    std::fs::create_dir_all(&home).unwrap();
    let tarball = archive(tmp.path());
    let (source, manifest) = fake_model(&home);
    let out = Command::new("sh")
        .arg(root().join("install.sh"))
        .env_clear()
        .env("HOME", &home)
        .env("LEO_HOME", &leo_home)
        .env("SHELL", "/bin/zsh")
        .env("PATH", "/usr/bin:/bin")
        .env("LEO_INSTALL_ARCHIVE", &tarball)
        .env("LEO_INSTALL_MODEL_URL", &source)
        .env("LEO_INSTALL_MODEL_MANIFEST", manifest)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(leo_home
        .join("models/parakeet-tdt-0.6b-v3-int8/encoder.int8.onnx")
        .is_file());
    assert!(!home.join(".leo").exists());
}
