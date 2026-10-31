//! Every word leo shows a user has to name a command that exists.
//!
//! Commands get renamed and retired; the text that mentions them is spread over
//! status messages, error messages, hints, the config template, help and the
//! README. This scans all of it — every non-comment line outside test modules,
//! plus the README and the web page — for commands that are gone or hidden, so
//! a change to the vocabulary cannot leave a stale instruction behind.

use std::path::{Path, PathBuf};

/// What users must no longer be told to type, and what replaced it.
const STALE: &[(&str, &str)] = &[
    ("leo serve", "open the leo app"),
    ("--new-token", "(removed)"),
    ("QR code", "(removed)"),
    ("leo doctor", "Settings, Health"),
    ("leo new", "the leo app"),
    ("leo list", "the leo app"),
    ("leo search", "the leo app"),
    ("leo edit", "the leo app"),
    ("leo view", "the leo app"),
    ("leo record", "Record in the leo app"),
    ("leo ask", "Ask Felix in the leo app"),
    ("leo pin", "the leo app"),
    ("leo sync", "Settings, Backup"),
    ("leo backup", "Settings, Backup"),
    ("leo trash", "Trash in the leo app"),
    ("leo help", "leo --help"),
    (":backup", "Settings, Backup"),
    (":settings", "Settings"),
    (":doctor", "Settings, Health"),
    (":record", "Record"),
    (":ask", "Ask Felix"),
    ("/settings", "Settings"),
    ("/doctor", "Settings, Health"),
    ("/record", "Record"),
    ("/backup", "Settings, Backup"),
    ("/ask", "Ask Felix"),
];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The lines of a source file a user could ever see: not comments, and not
/// inside a test module (tests name retired commands on purpose).
fn shipped_lines(path: &Path) -> Vec<(usize, String)> {
    if path.file_name().is_some_and(|n| n == "tests.rs") {
        return Vec::new();
    }
    let text = std::fs::read_to_string(path).unwrap();
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("#[cfg(test)]") {
            break;
        }
        let code = line.trim_start();
        if code.starts_with("//") {
            continue;
        }
        out.push((i + 1, line.to_string()));
    }
    out
}

/// Whether `line` mentions `word` as a whole word: `:list` is not in
/// `:listen`, and `:config` is not in the Rust path `leo::config`.
fn mentions(line: &str, word: &str) -> bool {
    let is_word =
        |c: char| c.is_alphanumeric() || c == '_' || c == ':' || c == '/' || c == '#' || c == '\\';
    line.match_indices(word).any(|(i, _)| {
        let before = line[..i].chars().next_back();
        let after = line[i + word.len()..].chars().next();
        !before.is_some_and(is_word) && !after.is_some_and(|c| c.is_alphanumeric() || c == '_')
    })
}

fn stale_in(text_lines: &[(usize, String)], file: &Path, found: &mut Vec<String>) {
    for (n, line) in text_lines {
        for (old, new) in STALE {
            if mentions(line, old) {
                found.push(format!(
                    "{}:{n}: says `{old}` — now `{new}`\n    {}",
                    file.display(),
                    line.trim()
                ));
            }
        }
    }
}

#[test]
fn nothing_shown_to_a_user_names_a_command_that_is_gone() {
    let mut files = Vec::new();
    rust_files(&root().join("src"), &mut files);
    rust_files(&root().join("crates"), &mut files);

    let mut found = Vec::new();
    for file in &files {
        stale_in(&shipped_lines(file), file, &mut found);
    }
    let page_parts = ["crates/leo-web/src/web/app", "crates/leo-web/src/web"]
        .into_iter()
        .flat_map(|dir| std::fs::read_dir(root().join(dir)).unwrap())
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "js"));
    let docs = [
        "README.md",
        "CONTRIBUTING.md",
        "install.sh",
        "install.ps1",
        "crates/leo-web/src/web/index.html",
    ]
    .map(|doc| root().join(doc));
    for path in docs.into_iter().chain(page_parts) {
        let lines: Vec<(usize, String)> = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .enumerate()
            .map(|(i, l)| (i + 1, l.to_string()))
            .collect();
        stale_in(&lines, &path, &mut found);
    }

    assert!(
        found.is_empty(),
        "stale instructions:\n{}",
        found.join("\n")
    );
}

#[test]
fn command_instructions_are_distinguished_from_routes_and_links() {
    assert!(mentions("type /settings", "/settings"));
    assert!(!mentions("https://obsidian.md", "/obsidian"));
    assert!(!mentions("go('#/settings')", "/settings"));
    assert!(!mentions("type :settings", "/settings"));
}
