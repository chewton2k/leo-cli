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
    let out = Command::new("sh")
        .arg(root().join("install.sh"))
        .env_clear()
        .env("HOME", home)
        .env("SHELL", shell)
        .env("PATH", "/usr/bin:/bin")
        .env("LEO_INSTALL_ARCHIVE", tarball)
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
    assert!(said.contains("leo setup"), "no next step:\n{said}");

    // Running it again must not add the PATH line a second time.
    install(&home, "/bin/zsh", &tarball);
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
