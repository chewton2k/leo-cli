use std::path::{Path, PathBuf};

use anyhow::{bail, ensure, Result};

pub const GO_VERSION: &str = "go1.27.2";
pub const FILES_SHA256: &str = "ebe6932ac6e36739735ce4da0b0ea2bcdf61f19ded09dc02e8d540506e4b5cd0";
pub const ASSET: &str = "leo-go.tar.gz";
pub const PACKAGE_BYTES: u64 = 20_900_000;
const REPO: &str = "chewton2k/leo-cli";
const LIST: &str = "files.sha256";
const MOST_FILES: usize = 400;

pub fn dir() -> PathBuf {
    crate::config::choice::models_dir().join(format!("go-{GO_VERSION}"))
}

fn listed(dir: &Path, pin: &str) -> Result<Vec<(String, String)>> {
    let list = dir.join(LIST);
    ensure!(
        crate::providers::sha256_of(&list)?.eq_ignore_ascii_case(pin),
        "the Go runner's file list does not match this leo"
    );
    let text = std::fs::read_to_string(&list)?;
    let mut out = Vec::new();
    for line in text.lines() {
        let Some((sha, name)) = line.split_once("  ") else {
            bail!("the Go runner's file list is damaged");
        };
        ensure!(
            sha.len() == 64
                && !name.starts_with('/')
                && name
                    .split('/')
                    .all(|part| !part.is_empty() && part != ".." && part != "."),
            "the Go runner's file list is damaged"
        );
        out.push((name.to_string(), sha.to_string()));
    }
    ensure!(
        !out.is_empty() && out.len() <= MOST_FILES,
        "the Go runner's file list is damaged"
    );
    Ok(out)
}

pub fn check(dir: &Path) -> Result<Vec<String>> {
    check_with(dir, FILES_SHA256)
}

fn check_with(dir: &Path, pin: &str) -> Result<Vec<String>> {
    let files = listed(dir, pin)?;
    for (name, sha) in &files {
        ensure!(
            crate::providers::model_state(&dir.join(name), sha)
                == crate::providers::ModelState::Ready,
            "{name} is missing or damaged"
        );
    }
    Ok(files.into_iter().map(|(name, _)| name).collect())
}

pub fn ready() -> bool {
    check(&dir()).is_ok()
}

pub fn urls() -> Vec<String> {
    if let Ok(url) = std::env::var("LEO_GO_URL") {
        return vec![url];
    }
    vec![
        format!(
            "https://github.com/{REPO}/releases/download/v{}/{ASSET}",
            env!("CARGO_PKG_VERSION")
        ),
        format!("https://github.com/{REPO}/releases/latest/download/{ASSET}"),
    ]
}

pub fn fetch() -> Result<PathBuf> {
    let dir = dir();
    fetch_into(&dir, &urls())?;
    Ok(dir)
}

pub fn fetch_into(dir: &Path, urls: &[String]) -> Result<()> {
    if check(dir).is_ok() {
        return Ok(());
    }
    std::fs::create_dir_all(dir)?;
    let package = dir.join(format!("{ASSET}.part"));
    let mut last = anyhow::anyhow!("the Go runner could not be downloaded; try again when online");
    for url in urls {
        let _ = std::fs::remove_file(&package);
        let fetched = std::process::Command::new("curl")
            .args(["-L", "--fail", "-sS", "-o"])
            .arg(&package)
            .arg(url)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        if !fetched.is_ok_and(|s| s.success()) {
            continue;
        }
        let unpacked = std::process::Command::new("tar")
            .arg("-xzf")
            .arg(&package)
            .arg("-C")
            .arg(dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        let _ = std::fs::remove_file(&package);
        if !unpacked.is_ok_and(|s| s.success()) {
            last = anyhow::anyhow!("the Go runner could not be unpacked");
            continue;
        }
        match check(dir) {
            Ok(_) => return Ok(()),
            Err(why) => last = why,
        }
    }
    Err(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pin_matches_the_runner_build_script() {
        let pinned = include_str!("../../../scripts/go-runner/MANIFEST").trim();
        assert_eq!(pinned, FILES_SHA256);
        assert_eq!(
            include_str!("../../../scripts/go-runner/GO_VERSION").trim(),
            GO_VERSION
        );
        assert!(urls()
            .iter()
            .all(|u| u.starts_with("https://github.com/") && u.ends_with(ASSET)));
    }

    #[test]
    fn only_listed_files_with_their_pinned_sums_and_safe_paths_pass() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("std/math")).unwrap();
        std::fs::write(dir.path().join("compile.wasm"), "c").unwrap();
        std::fs::write(dir.path().join("std/math/bits.a"), "b").unwrap();
        let sum = |p: &str| crate::providers::sha256_of(&dir.path().join(p)).unwrap();
        let list = format!(
            "{}  compile.wasm\n{}  std/math/bits.a\n",
            sum("compile.wasm"),
            sum("std/math/bits.a")
        );
        std::fs::write(dir.path().join(LIST), &list).unwrap();
        let pin = sum(LIST);
        assert_eq!(
            check_with(dir.path(), &pin).unwrap(),
            ["compile.wasm", "std/math/bits.a"]
        );
        assert!(
            check_with(dir.path(), &"0".repeat(64)).is_err(),
            "an unpinned list is refused"
        );
        std::fs::write(dir.path().join("std/math/bits.a"), "changed").unwrap();
        assert!(
            check_with(dir.path(), &pin).is_err(),
            "a damaged file is refused"
        );
        for bad in ["../evil", "/abs", "a//b", "./a"] {
            std::fs::write(
                dir.path().join(LIST),
                format!("{}  {bad}\n", "0".repeat(64)),
            )
            .unwrap();
            let pin = sum(LIST);
            assert!(check_with(dir.path(), &pin).is_err(), "{bad}");
        }
        assert!(fetch_into(dir.path(), &["file:///nowhere/leo-go.tar.gz".to_string()]).is_err());
    }
}
