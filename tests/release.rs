#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .output()
        .unwrap()
        .status
        .success();
    assert!(ok, "git {args:?} failed");
}

fn repo(version: &str) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    git(dir, &["init", "-q", "-b", "main"]);
    write_version(dir, version);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(dir.join("README.md"), "leo\n").unwrap();
    commit(dir, "start");
    tmp
}

fn write_version(dir: &Path, version: &str) {
    std::fs::write(
        dir.join("Cargo.toml"),
        format!(
            "[workspace]\nmembers = []\n\n[workspace.package]\nversion = \"{version}\"\n\
             edition = \"2021\"\n\n[workspace.dependencies]\nclap = {{ version = \"4\" }}\n"
        ),
    )
    .unwrap();
}

fn commit(dir: &Path, message: &str) {
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", message]);
}

fn change(dir: &Path, file: &str) {
    let path = dir.join(file);
    let mut text = std::fs::read_to_string(&path).unwrap_or_default();
    text.push_str("// more\n");
    std::fs::write(path, text).unwrap();
    commit(dir, &format!("change {file}"));
}

fn next_version(dir: &Path) -> String {
    let out = Command::new("sh")
        .arg(root().join("scripts/next-version.sh"))
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "next-version failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn the_first_release_uses_the_version_in_cargo_toml() {
    let repo = repo("0.2.0");
    assert_eq!(next_version(repo.path()), "0.2.0");
}

#[test]
fn a_code_change_after_a_release_bumps_the_patch_number() {
    let repo = repo("0.2.0");
    git(repo.path(), &["tag", "v0.2.0"]);
    change(repo.path(), "src/main.rs");
    assert_eq!(next_version(repo.path()), "0.2.1");
    git(repo.path(), &["tag", "v0.2.1"]);
    change(repo.path(), "src/main.rs");
    assert_eq!(next_version(repo.path()), "0.2.2");
}

#[test]
fn versions_count_past_nine() {
    let repo = repo("0.2.0");
    git(repo.path(), &["tag", "v0.2.9"]);
    change(repo.path(), "src/main.rs");
    assert_eq!(next_version(repo.path()), "0.2.10");
    git(repo.path(), &["tag", "v0.2.10"]);
    change(repo.path(), "src/main.rs");
    assert_eq!(next_version(repo.path()), "0.2.11");
}

#[test]
fn a_change_that_does_not_touch_leo_releases_nothing() {
    let repo = repo("0.2.0");
    git(repo.path(), &["tag", "v0.2.0"]);
    assert_eq!(
        next_version(repo.path()),
        "",
        "released the tagged commit again"
    );
    change(repo.path(), "README.md");
    assert_eq!(next_version(repo.path()), "");
}

#[test]
fn raising_the_version_in_cargo_toml_releases_that_version() {
    let repo = repo("0.2.0");
    git(repo.path(), &["tag", "v0.2.5"]);
    write_version(repo.path(), "0.3.0");
    commit(repo.path(), "0.3");
    assert_eq!(next_version(repo.path()), "0.3.0");
}

#[test]
fn the_version_bump_commit_releases_nothing() {
    let repo = repo("0.2.0");
    git(repo.path(), &["tag", "v0.2.1"]);
    write_version(repo.path(), "0.2.1");
    commit(repo.path(), "release: v0.2.1");
    assert_eq!(next_version(repo.path()), "");
    change(repo.path(), "src/main.rs");
    assert_eq!(next_version(repo.path()), "0.2.2");
}

#[test]
fn a_dependency_update_is_released() {
    let repo = repo("0.2.0");
    let lock = |version: &str, sum: &str| {
        format!("[[package]]\nname = \"clap\"\nversion = \"{version}\"\nchecksum = \"{sum}\"\n")
    };
    std::fs::write(repo.path().join("Cargo.lock"), lock("4.0.0", "aaa")).unwrap();
    commit(repo.path(), "lock");
    git(repo.path(), &["tag", "v0.2.0"]);
    std::fs::write(repo.path().join("Cargo.lock"), lock("4.1.0", "bbb")).unwrap();
    commit(repo.path(), "update clap");
    assert_eq!(next_version(repo.path()), "0.2.1");
}

#[test]
fn set_version_changes_the_workspace_and_its_lock_entries_only() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let toml = std::fs::read_to_string(root().join("Cargo.toml")).unwrap();
    let lock = std::fs::read_to_string(root().join("Cargo.lock")).unwrap();
    std::fs::write(dir.join("Cargo.toml"), &toml).unwrap();
    std::fs::write(dir.join("Cargo.lock"), &lock).unwrap();

    let ok = Command::new("sh")
        .arg(root().join("scripts/set-version.sh"))
        .arg("7.8.9")
        .current_dir(dir)
        .status()
        .unwrap()
        .success();
    assert!(ok);

    let new_toml = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap();
    let new_lock = std::fs::read_to_string(dir.join("Cargo.lock")).unwrap();
    let changed = |old: &str, new: &str| {
        old.lines()
            .zip(new.lines())
            .filter(|(a, b)| a != b)
            .map(|(_, b)| b.to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(changed(&toml, &new_toml), ["version = \"7.8.9\""]);
    let lock_changes = changed(&lock, &new_lock);
    assert_eq!(lock_changes.len(), 5, "{lock_changes:?}");
    assert!(lock_changes.iter().all(|l| l == "version = \"7.8.9\""));
    assert_eq!(lock.lines().count(), new_lock.lines().count());
}
