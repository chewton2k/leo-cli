//! Which editor leo opens for writing a note.

/// The editor to run: `$EDITOR`, then `$VISUAL`, then — for someone who has
/// set neither, which is most people new to the terminal — nano, which shows
/// its keys on screen, rather than vi, which many cannot even quit.
pub fn command() -> String {
    choose(
        std::env::var("EDITOR").ok(),
        std::env::var("VISUAL").ok(),
        crate::paths::on_path("nano"),
    )
}

/// Open `path` in the editor and wait for it to close.
pub fn open(path: &std::path::Path) -> std::io::Result<std::process::ExitStatus> {
    let argv = argv(&command(), path);
    std::process::Command::new(program(&argv[0]))
        .args(&argv[1..])
        .status()
}

fn program(name: &str) -> std::path::PathBuf {
    if cfg!(windows) && !name.contains(['/', '\\']) {
        if let Some(found) = crate::paths::find(name) {
            return found;
        }
    }
    std::path::PathBuf::from(name)
}

/// The editor setting split into a program and its flags, then the file.
fn argv(command: &str, path: &std::path::Path) -> Vec<String> {
    let mut argv = words(command);
    if argv.is_empty() {
        argv.push("vi".to_string());
    }
    argv.push(path.to_string_lossy().into_owned());
    argv
}

fn words(command: &str) -> Vec<String> {
    let command = command.trim();
    if std::path::Path::new(command).is_file() {
        return vec![command.to_string()];
    }
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut started = false;
    for c in command.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => word.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                started = true;
            }
            (None, c) if c.is_whitespace() => {
                if started || !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
                started = false;
            }
            (None, c) => word.push(c),
        }
    }
    if started || !word.is_empty() {
        words.push(word);
    }
    words
}

fn choose(editor: Option<String>, visual: Option<String>, has_nano: bool) -> String {
    let set = |v: Option<String>| v.filter(|v| !v.trim().is_empty());
    set(editor).or_else(|| set(visual)).unwrap_or_else(|| {
        if has_nano {
            "nano"
        } else if cfg!(windows) {
            "notepad"
        } else {
            "vi"
        }
        .to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chosen_editor_wins() {
        assert_eq!(
            choose(Some("code -w".into()), Some("vim".into()), true),
            "code -w"
        );
        assert_eq!(choose(None, Some("hx".into()), true), "hx");
    }

    #[test]
    fn with_nothing_set_a_beginner_gets_nano() {
        assert_eq!(choose(None, None, true), "nano");
    }

    #[test]
    fn without_nano_it_falls_back_to_the_system_editor() {
        let fallback = if cfg!(windows) { "notepad" } else { "vi" };
        assert_eq!(choose(None, None, false), fallback);
    }

    /// `EDITOR="code -w"` is common; the flags must reach the program rather
    /// than be taken as part of its name.
    #[test]
    fn an_editor_setting_with_flags_is_split_into_arguments() {
        let path = std::path::Path::new("/tmp/note.md");
        assert_eq!(argv("code -w", path), vec!["code", "-w", "/tmp/note.md"]);
        assert_eq!(argv("nano", path), vec!["nano", "/tmp/note.md"]);
    }

    #[test]
    fn a_quoted_editor_path_with_spaces_stays_one_program() {
        let path = std::path::Path::new("/tmp/note.md");
        assert_eq!(
            argv(
                r#""C:\Program Files\Notepad++\notepad++.exe" -multiInst"#,
                path
            ),
            vec![
                r"C:\Program Files\Notepad++\notepad++.exe",
                "-multiInst",
                "/tmp/note.md"
            ]
        );
        assert_eq!(words("subl -n -w"), vec!["subl", "-n", "-w"]);
        assert_eq!(words("'my editor' --wait"), vec!["my editor", "--wait"]);
    }

    #[test]
    fn an_editor_file_path_with_spaces_needs_no_quotes() {
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("my editor");
        std::fs::write(&program, "").unwrap();
        let program = program.to_string_lossy().into_owned();
        assert_eq!(words(&program), vec![program.clone()]);
    }

    /// An empty variable is as good as unset.
    #[test]
    fn an_empty_setting_is_ignored() {
        assert_eq!(choose(Some("  ".into()), None, true), "nano");
    }
}
