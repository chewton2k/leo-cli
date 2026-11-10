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
    ("leo sync", "(removed)"),
    ("leo backup", "(removed)"),
    ("leo trash", "Trash in the leo app"),
    ("leo help", "leo --help"),
    (":backup", "(removed)"),
    (":settings", "Settings"),
    (":doctor", "Settings, Health"),
    (":record", "Record"),
    (":ask", "Ask Felix"),
    ("/settings", "Settings"),
    ("/doctor", "Settings, Health"),
    ("/record", "Record"),
    ("/backup", "(removed)"),
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

fn item_end(text: &str) -> usize {
    let bytes = text.as_bytes();
    let mut i = "#[cfg(test)]".len();
    let mut depth = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                i += text[i..].find('\n').unwrap_or(text.len() - i);
                continue;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += text[i..].find("*/").map_or(text.len() - i, |e| e + 2);
                continue;
            }
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
            }
            b'r' if matches!(bytes.get(i + 1), Some(b'#' | b'"')) => {
                let hashes = text[i + 1..].len() - text[i + 1..].trim_start_matches('#').len();
                if bytes.get(i + 1 + hashes) == Some(&b'"') {
                    let close = format!("\"{}", "#".repeat(hashes));
                    let from = i + 2 + hashes;
                    i = text[from..]
                        .find(&close)
                        .map_or(text.len(), |e| from + e + close.len());
                    continue;
                }
            }
            b'\'' => {
                if bytes.get(i + 1) == Some(&b'\\') {
                    i += 3 + text[i + 3..].find('\'').unwrap_or(0);
                } else if let Some(c) = text[i + 1..].chars().next() {
                    if bytes.get(i + 1 + c.len_utf8()) == Some(&b'\'') {
                        i += 1 + c.len_utf8();
                    }
                }
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                if depth == 0 {
                    return i;
                }
                depth -= 1;
                if depth == 0 && bytes[i] == b'}' {
                    return i + 1;
                }
            }
            b';' | b',' if depth == 0 => return i + 1,
            _ => {}
        }
        i += 1;
    }
    text.len()
}

fn shipped_part(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("#[cfg(test)]") {
        out.push_str(&rest[..at]);
        let end = (at + item_end(&rest[at..])).min(rest.len());
        out.extend(
            rest[at..end]
                .chars()
                .map(|c| if c == '\n' { c } else { ' ' }),
        );
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// The lines of a source file a user could ever see: not comments, and not
/// inside a test module (tests name retired commands on purpose).
fn shipped_lines(path: &Path) -> Vec<(usize, String)> {
    if path.file_name().is_some_and(|n| n == "tests.rs") {
        return Vec::new();
    }
    let text = shipped_part(&std::fs::read_to_string(path).unwrap());
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
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
    if word.starts_with(':') && line.contains(word) {
        let all_json_keys = line.match_indices(word).all(|(i, _)| {
            let Some(prefix) = line.get(..i).and_then(|s| s.strip_suffix('"')) else {
                return false;
            };
            let Some((before, key)) = prefix.rsplit_once('"') else {
                return false;
            };
            !key.is_empty()
                && key.chars().all(|c| c.is_alphanumeric() || c == '_')
                && before.trim_end().ends_with(['{', ','])
        });
        if all_json_keys {
            return false;
        }
    }
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
    let mut styles: Vec<_> = std::fs::read_dir(root().join("crates/leo-web/src/web/styles"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "css"))
        .collect();
    styles.sort();
    let docs = [
        "README.md",
        "CONTRIBUTING.md",
        "install.sh",
        "install.ps1",
        "crates/leo-web/src/web/page-head.html",
        "crates/leo-web/src/web/page-tail.html",
    ]
    .map(|doc| root().join(doc));
    for path in docs.into_iter().chain(page_parts).chain(styles) {
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

#[test]
fn json_keys_are_not_commands_but_command_values_still_are() {
    assert!(!mentions(
        r#"json!({"settings":settings,"runtime":runtime})"#,
        ":settings"
    ));
    assert!(mentions(
        r#"json!({"instruction":":settings"})"#,
        ":settings"
    ));
    assert!(mentions(r#"Use ":settings" to open it"#, ":settings"));
    assert!(mentions(
        r#"json!({"settings":settings,"message":":settings"})"#,
        ":settings"
    ));
}

#[test]
fn code_after_a_test_module_is_still_read() {
    let text = "#[cfg(test)]\nmod tests {\n    const OLD: &str = \"leo serve\";\n}\nconst HINT: &str = \"run leo serve\";\n";
    let shipped = shipped_part(text);
    let lines: Vec<&str> = shipped.lines().map(str::trim).collect();
    assert_eq!(lines.len(), 5);
    assert_eq!(lines.iter().filter(|l| mentions(l, "leo serve")).count(), 1);
    assert_eq!(lines[4], "const HINT: &str = \"run leo serve\";");
}
