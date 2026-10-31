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

fn shipped_part(text: &str) -> &str {
    match text.find("#[cfg(test)]") {
        Some(at) => &text[..at],
        None => text,
    }
}

fn statement_from(text: &str, at: usize) -> &str {
    let rest = &text[at..];
    let end = rest.find(';').map(|i| i + 1).unwrap_or(rest.len());
    &rest[..end]
}

#[test]
fn every_child_process_is_started_quietly() {
    let mut files = Vec::new();
    shipped_rust_files(&root().join("src"), &mut files);
    shipped_rust_files(&root().join("crates"), &mut files);
    let mut missing = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(file).unwrap();
        let shipped = shipped_part(&text);
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
