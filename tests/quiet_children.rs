use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn shipped_rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        if path.is_dir() {
            if !matches!(name.as_str(), "tests" | "target" | "node_modules" | "gen") {
                shipped_rust_files(&path, out);
            }
        } else if name.ends_with(".rs")
            && name != "build.rs"
            && !path.ends_with("leo-core/src/children.rs")
            && !path.ends_with("leo-core/src/editor.rs")
        {
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

#[test]
fn only_test_items_are_skipped() {
    let text = "#[cfg(test)]\nconst A: &str = \"}\";\nfn a() { Command::new(\"x\"); }\n#[cfg(test)]\nmod tests {\n    fn b() { let c = '{'; Command::new(r#\"}\"#); }\n}\nfn d() { Command::new(\"y\"); }\n";
    let shipped = shipped_part(text);
    assert_eq!(shipped.lines().count(), text.lines().count());
    assert_eq!(shipped.matches("Command::new(").count(), 2);
    assert!(shipped.contains("fn a()") && shipped.contains("fn d()"));
    assert!(!shipped.contains("fn b()") && !shipped.contains("const A"));
}

fn statement_from(text: &str, at: usize) -> &str {
    let rest = &text[at..];
    let end = rest.find(';').map(|i| i + 1).unwrap_or(rest.len());
    &rest[..end]
}

#[test]
fn every_child_process_is_started_quietly() {
    let mut files = Vec::new();
    shipped_rust_files(&root().join("crates"), &mut files);
    let mut missing = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(file).unwrap();
        let shipped = &shipped_part(&text);
        let mut from = 0;
        while let Some(found) = shipped[from..].find("Command::new(") {
            let at = from + found;
            let line_start = shipped[..at].rfind('\n').map(|i| i + 1).unwrap_or(0);
            let statement = statement_from(shipped, line_start);
            if !statement.contains("quiet") {
                let line = shipped[..at].lines().count();
                missing.push(format!(
                    "{}:{}",
                    file.strip_prefix(root()).unwrap_or(file).display(),
                    line
                ));
            }
            from = at + "Command::new(".len();
        }
    }
    assert!(
        missing.is_empty(),
        "start these through leo_core::children::quiet:\n{}",
        missing.join("\n")
    );
}
