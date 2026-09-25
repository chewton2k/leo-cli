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
    std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .status()
}

/// The editor setting split into a program and its flags, then the file.
fn argv(command: &str, path: &std::path::Path) -> Vec<String> {
    let mut argv: Vec<String> = command.split_whitespace().map(str::to_string).collect();
    if argv.is_empty() {
        argv.push("vi".to_string());
    }
    argv.push(path.to_string_lossy().into_owned());
    argv
}

fn choose(editor: Option<String>, visual: Option<String>, has_nano: bool) -> String {
    let set = |v: Option<String>| v.filter(|v| !v.trim().is_empty());
    set(editor)
        .or_else(|| set(visual))
        .unwrap_or_else(|| if has_nano { "nano" } else { "vi" }.to_string())
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
    fn without_nano_it_falls_back_to_vi() {
        assert_eq!(choose(None, None, false), "vi");
    }

    /// `EDITOR="code -w"` is common; the flags must reach the program rather
    /// than be taken as part of its name.
    #[test]
    fn an_editor_setting_with_flags_is_split_into_arguments() {
        let path = std::path::Path::new("/tmp/note.md");
        assert_eq!(argv("code -w", path), vec!["code", "-w", "/tmp/note.md"]);
        assert_eq!(argv("nano", path), vec!["nano", "/tmp/note.md"]);
    }

    /// An empty variable is as good as unset.
    #[test]
    fn an_empty_setting_is_ignored() {
        assert_eq!(choose(Some("  ".into()), None, true), "nano");
    }
}
