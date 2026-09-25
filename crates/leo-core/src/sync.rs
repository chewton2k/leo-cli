use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

pub fn is_initialized(notes_dir: &Path) -> bool {
    notes_dir.join(".git").exists()
}

pub fn init(notes_dir: &Path) -> Result<()> {
    if is_initialized(notes_dir) {
        println!("Notes repo already initialized.");
        return Ok(());
    }

    fs::create_dir_all(notes_dir)?;

    run_git(notes_dir, &["init", "-b", "main"])
        .context("git init failed — is git installed?")?;

    let gitignore = notes_dir.join(".gitignore");
    if !gitignore.exists() {
        fs::write(&gitignore, GITIGNORE)?;
    }

    // Commit any existing files (e.g. migrated notes)
    run_git(notes_dir, &["add", "."])?;
    // Suppress error if there is nothing to commit (empty repo)
    let _ = run_git(notes_dir, &["commit", "-m", "init: initialize leo notes repo"]);

    println!("Initialized notes repo in {}", notes_dir.display());
    Ok(())
}

pub fn connect(notes_dir: &Path, url: &str) -> Result<()> {
    if !is_initialized(notes_dir) {
        anyhow::bail!("Run 'leo sync init' first.");
    }
    run_git(notes_dir, &["remote", "add", "origin", url])?;
    println!("Connected to {url}");
    Ok(())
}

/// The branch the notes repo is actually on.
///
/// Asked rather than assumed: `main` was hardcoded, so a repository created by an
/// older git — or by anyone whose default is `master` — could not be pushed at
/// all, and said "src refspec main does not match any" instead of saying so.
pub fn current_branch(notes_dir: &Path) -> Result<String> {
    let out = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(notes_dir)
        .output()
        .context("failed to run git rev-parse")?;
    let branch = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() || branch.is_empty() || branch == "HEAD" {
        anyhow::bail!(
            "the notes repository has no branch yet — save a note first, then push"
        );
    }
    Ok(branch)
}

/// These three are what a user explicitly asked for, so their output is the
/// answer — print it. Callers that hold a full-screen UI run them with the
/// terminal handed back, so there is nothing to smear.
pub fn push(notes_dir: &Path) -> Result<()> {
    let branch = current_branch(notes_dir)?;
    print_output(run_git(notes_dir, &["push", "-u", "origin", &branch])?);
    Ok(())
}

pub fn pull(notes_dir: &Path) -> Result<()> {
    let branch = current_branch(notes_dir)?;
    print_output(run_git(notes_dir, &["pull", "origin", &branch])?);
    Ok(())
}

/// The configured remote URL, if any.
///
/// Read rather than inferred so the profile page can show where notes actually
/// go — "sync is set up" is not useful without saying set up to what.
pub fn remote_url(notes_dir: &Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(notes_dir)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let url = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!url.is_empty()).then_some(url)
}

/// How many commits are waiting to be pushed, when that can be determined.
///
/// `None` when there is no upstream yet, which is a different state from zero and
/// should not be reported as "up to date".
pub fn unpushed(notes_dir: &Path) -> Option<usize> {
    let out = std::process::Command::new("git")
        .args(["rev-list", "--count", "@{u}..HEAD"])
        .current_dir(notes_dir)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

/// Back up now: pull what another machine pushed, then push. Says how to set
/// backup up when it is not, rather than failing inside git.
pub fn now(notes_dir: &Path) -> Result<()> {
    if !is_initialized(notes_dir) {
        anyhow::bail!("Backup is not set up. Run `leo sync` in a shell, or press Ctrl-S.");
    }
    if remote_url(notes_dir).is_none() {
        anyhow::bail!("No remote to back up to. Run `leo sync connect <url>`, or press Ctrl-S.");
    }
    pull(notes_dir)?;
    push(notes_dir)
}

pub fn status(notes_dir: &Path) -> Result<()> {
    print_output(run_git(notes_dir, &["status"])?);
    Ok(())
}

fn print_output(output: String) {
    let text = output.trim_end();
    if !text.is_empty() {
        println!("{text}");
    }
}

/// Commit whatever changed. Runs from `Store::save`, so it must never print:
/// the caller may be a full-screen UI.
pub fn auto_commit(notes_dir: &Path) -> Result<()> {
    run_git(notes_dir, &["add", "."])?;

    // Only create a commit if there are staged changes
    let has_changes = !Command::new("git")
        .args(["diff", "--cached", "--quiet"])
        .current_dir(notes_dir)
        .output()
        .context("failed to run git diff --cached")?
        .status
        .success();

    if has_changes {
        run_git(notes_dir, &["commit", "-m", "update notes"])?;
    }
    Ok(())
}

/// Files inside the notes directory that are leo's business, not the user's
/// notes, and so must never be pushed to their remote.
const GITIGNORE: &str = "*.wav\n*.bak\n.manual-installed\n";

/// Run git and capture what it says.
///
/// Capturing rather than inheriting is the whole point: `auto_commit` runs on
/// every save, including while the TUI owns the terminal, and git's "2 files
/// changed" chatter printed straight onto the alternate screen. Callers that
/// want the output show it themselves.
fn run_git(dir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .context("failed to run git — is git installed?")?;

    if !output.status.success() {
        // git puts failures on stderr; include them so the error is actionable
        // rather than just a status code.
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let detail = if stderr.is_empty() {
            String::new()
        } else {
            format!(": {stderr}")
        };
        anyhow::bail!("git {} failed{detail}", args.join(" "));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Backing up with nothing set up says how to set it up, rather than
    /// failing inside git.
    #[test]
    fn backing_up_before_setup_says_what_to_do() {
        let tmp = TempDir::new().unwrap();
        let err = now(tmp.path()).unwrap_err().to_string();
        assert!(err.contains("leo sync") && err.contains("Ctrl-S"), "{err}");
    }

    #[test]
    fn backing_up_without_a_remote_says_what_to_do() {
        let tmp = TempDir::new().unwrap();
        init(tmp.path()).unwrap();
        let err = now(tmp.path()).unwrap_err().to_string();
        assert!(err.contains("remote"), "{err}");
    }

    #[test]
    fn test_is_initialized_false_before_init() {
        let tmp = TempDir::new().unwrap();
        assert!(!is_initialized(tmp.path()));
    }

    #[test]
    fn test_init_creates_git_repo_and_gitignore() {
        let tmp = TempDir::new().unwrap();
        let notes_dir = tmp.path().join("notes");
        std::fs::create_dir_all(&notes_dir).unwrap();

        init(&notes_dir).unwrap();

        assert!(is_initialized(&notes_dir), ".git dir should exist");
        let gitignore = std::fs::read_to_string(notes_dir.join(".gitignore")).unwrap();
        assert!(gitignore.contains("*.wav"));
        assert!(gitignore.contains("*.bak"));
    }

    #[test]
    fn test_connect_before_init_returns_error() {
        let tmp = TempDir::new().unwrap();
        let err = connect(tmp.path(), "https://github.com/user/repo.git").unwrap_err();
        assert!(err.to_string().contains("leo sync init"));
    }

    #[test]
    fn test_auto_commit_no_op_when_nothing_staged() {
        let tmp = TempDir::new().unwrap();
        let notes_dir = tmp.path().join("notes");
        std::fs::create_dir_all(&notes_dir).unwrap();
        init(&notes_dir).unwrap();

        auto_commit(&notes_dir).unwrap();
    }

    /// The bug this guards: git's commit summary printed onto the TUI's
    /// alternate screen on every save. Nothing in the auto-commit path may
    /// write to stdout or stderr.
    #[test]
    fn auto_commit_captures_git_output_instead_of_inheriting_it() {
        let tmp = TempDir::new().unwrap();
        let notes_dir = tmp.path().join("notes");
        std::fs::create_dir_all(&notes_dir).unwrap();
        init(&notes_dir).unwrap();
        std::fs::write(notes_dir.join("a.md"), "content").unwrap();

        // `git add` and `git commit` both go through run_git, which returns
        // their output as a String rather than letting it reach the terminal.
        let added = run_git(&notes_dir, &["add", "."]).unwrap();
        assert!(added.is_empty(), "git add should be silent");

        let committed = run_git(&notes_dir, &["commit", "-m", "x"]).unwrap();
        assert!(
            committed.contains("1 file changed") || committed.contains("a.md"),
            "the summary must be returned, not printed: {committed:?}"
        );
    }

    #[test]
    fn a_failed_git_command_reports_what_git_said() {
        let tmp = TempDir::new().unwrap();
        // Not a repo, so this fails.
        let err = run_git(tmp.path(), &["log"]).unwrap_err().to_string();
        assert!(err.contains("git log failed"), "got: {err}");
        assert!(
            err.to_lowercase().contains("repository") || err.contains(':'),
            "the error should carry git's own message: {err}"
        );
    }

    /// leo's own marker files live in the notes directory but are not notes,
    /// so a fresh repo must not push them.
    #[test]
    fn the_gitignore_covers_leos_own_files() {
        let tmp = TempDir::new().unwrap();
        let notes_dir = tmp.path().join("notes");
        std::fs::create_dir_all(&notes_dir).unwrap();
        init(&notes_dir).unwrap();

        let gitignore = std::fs::read_to_string(notes_dir.join(".gitignore")).unwrap();
        assert!(gitignore.contains(".manual-installed"));
        assert!(gitignore.contains("*.wav"));
        assert!(gitignore.contains("*.bak"));
    }

    #[test]
    fn test_auto_commit_commits_new_file() {
        let tmp = TempDir::new().unwrap();
        let notes_dir = tmp.path().join("notes");
        std::fs::create_dir_all(&notes_dir).unwrap();
        init(&notes_dir).unwrap();

        std::fs::write(notes_dir.join("new.md"), "content").unwrap();
        auto_commit(&notes_dir).unwrap();

        let log = Command::new("git")
            .args(["log", "--oneline"])
            .current_dir(&notes_dir)
            .output()
            .unwrap();
        let log_str = String::from_utf8(log.stdout).unwrap();
        assert!(log_str.contains("update notes"), "expected commit, got: {log_str}");
    }

    /// The bug this guards: `main` was hardcoded, so a repo on any other branch
    /// could not be pushed and blamed the refspec rather than saying so.
    #[test]
    fn the_branch_is_read_from_the_repository() {
        let tmp = TempDir::new().unwrap();
        let notes_dir = tmp.path().join("notes");
        std::fs::create_dir_all(&notes_dir).unwrap();

        // A repo whose branch is deliberately not `main`.
        for args in [
            vec!["init", "-q", "-b", "trunk"],
            vec!["config", "user.email", "t@example.com"],
            vec!["config", "user.name", "t"],
        ] {
            let ok = Command::new("git")
                .args(&args)
                .current_dir(&notes_dir)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if !ok {
                return; // no git, or too old for -b
            }
        }
        std::fs::write(notes_dir.join("a.md"), "x").unwrap();
        for args in [vec!["add", "-A"], vec!["commit", "-qm", "x"]] {
            Command::new("git")
                .args(&args)
                .current_dir(&notes_dir)
                .output()
                .unwrap();
        }

        assert_eq!(current_branch(&notes_dir).unwrap(), "trunk");
    }

    /// A repository with no commit yet has no branch to push, and should say that
    /// rather than producing a refspec error.
    #[test]
    fn a_repo_with_no_commits_explains_itself() {
        let tmp = TempDir::new().unwrap();
        let notes_dir = tmp.path().join("notes");
        std::fs::create_dir_all(&notes_dir).unwrap();
        if init(&notes_dir).is_err() {
            return;
        }
        // `init` commits, so remove the commit to reach the empty state.
        let empty = tmp.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        if !Command::new("git")
            .args(["init", "-q"])
            .current_dir(&empty)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return;
        }
        let err = current_branch(&empty).unwrap_err().to_string();
        assert!(err.contains("no branch yet"), "{err}");
    }
}
