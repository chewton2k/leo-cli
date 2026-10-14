use std::path::{Path, PathBuf};

use anyhow::{bail, ensure, Result};

pub struct Pack {
    pub folder: &'static str,
    pub asset: &'static str,
    pub files_sha256: Option<&'static str>,
}

pub const GO_VERSION: &str = "go1.27.2";
pub const GO: Pack = Pack {
    folder: "go-go1.27.2",
    asset: "leo-go.tar.gz",
    files_sha256: Some("ebe6932ac6e36739735ce4da0b0ea2bcdf61f19ded09dc02e8d540506e4b5cd0"),
};
pub const JAVA: Pack = Pack {
    folder: "java-teavm-javac",
    asset: "leo-java.tar.gz",
    files_sha256: None,
};
pub const RUST: Pack = Pack {
    folder: "rust-miri-2024-06",
    asset: "leo-rust.tar.gz",
    files_sha256: Some("fce14624025b377e9054f49427064b9494af5540fd2df3a3a5884dce7046276a"),
};
const REPO: &str = "chewton2k/leo-cli";
const LIST: &str = "files.sha256";
const MOST_FILES: usize = 400;

pub fn dir(pack: &Pack) -> PathBuf {
    crate::config::choice::models_dir().join(pack.folder)
}

fn listed(dir: &Path, pin: Option<&str>) -> Result<Vec<(String, String)>> {
    let list = dir.join(LIST);
    if let Some(pin) = pin {
        ensure!(
            crate::providers::sha256_of(&list)?.eq_ignore_ascii_case(pin),
            "the runtime's file list does not match this leo"
        );
    }
    let text = std::fs::read_to_string(&list)?;
    let mut out = Vec::new();
    for line in text.lines() {
        let Some((sha, name)) = line.split_once("  ") else {
            bail!("the runtime's file list is damaged");
        };
        ensure!(
            sha.len() == 64
                && !name.starts_with('/')
                && name
                    .split('/')
                    .all(|part| !part.is_empty() && part != ".." && part != "."),
            "the runtime's file list is damaged"
        );
        out.push((name.to_string(), sha.to_string()));
    }
    ensure!(
        !out.is_empty() && out.len() <= MOST_FILES,
        "the runtime's file list is damaged"
    );
    Ok(out)
}

pub fn check(pack: &Pack, dir: &Path) -> Result<Vec<String>> {
    check_with(dir, pack.files_sha256)
}

fn check_with(dir: &Path, pin: Option<&str>) -> Result<Vec<String>> {
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

pub fn ready(pack: &Pack) -> bool {
    check(pack, &dir(pack)).is_ok()
}

pub fn urls(pack: &Pack) -> Vec<String> {
    vec![
        format!(
            "https://github.com/{REPO}/releases/download/v{}/{}",
            env!("CARGO_PKG_VERSION"),
            pack.asset
        ),
        format!(
            "https://github.com/{REPO}/releases/latest/download/{}",
            pack.asset
        ),
    ]
}

pub fn fetch(pack: &Pack) -> Result<PathBuf> {
    let dir = dir(pack);
    fetch_into(&dir, &urls(pack), pack.files_sha256)?;
    Ok(dir)
}

pub fn fetch_into(dir: &Path, urls: &[String], pin: Option<&str>) -> Result<()> {
    if check_with(dir, pin).is_ok() {
        return Ok(());
    }
    std::fs::create_dir_all(dir)?;
    let package = dir.join("runtime.tar.gz.part");
    let mut last = anyhow::anyhow!("the runtime could not be downloaded; try again when online");
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
            last = anyhow::anyhow!("the runtime could not be unpacked");
            continue;
        }
        match check_with(dir, pin) {
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
    fn the_pins_match_the_runner_build_scripts() {
        assert_eq!(
            Some(include_str!("../../../scripts/go-runner/MANIFEST").trim()),
            GO.files_sha256
        );
        assert_eq!(
            Some(include_str!("../../../scripts/rust-runner/MANIFEST").trim()),
            RUST.files_sha256
        );
        assert_eq!(
            JAVA.files_sha256, None,
            "Java builds differ between machines, so each release carries its own"
        );
        assert_eq!(
            include_str!("../../../scripts/go-runner/GO_VERSION").trim(),
            GO_VERSION
        );
        assert!(GO.folder.ends_with(GO_VERSION));
        for pack in [GO, JAVA, RUST] {
            assert!(urls(&pack)
                .iter()
                .all(|u| u.starts_with("https://github.com/") && u.ends_with(pack.asset)));
        }
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
            check_with(dir.path(), Some(&pin)).unwrap(),
            ["compile.wasm", "std/math/bits.a"]
        );
        assert!(
            check_with(dir.path(), Some(&"0".repeat(64))).is_err(),
            "an unpinned list is refused"
        );
        std::fs::write(dir.path().join("std/math/bits.a"), "changed").unwrap();
        assert!(
            check_with(dir.path(), Some(&pin)).is_err(),
            "a damaged file is refused"
        );
        for bad in ["../evil", "/abs", "a//b", "./a"] {
            std::fs::write(
                dir.path().join(LIST),
                format!("{}  {bad}\n", "0".repeat(64)),
            )
            .unwrap();
            let pin = sum(LIST);
            assert!(check_with(dir.path(), Some(&pin)).is_err(), "{bad}");
        }
        assert!(fetch_into(
            dir.path(),
            &["file:///nowhere/leo-go.tar.gz".to_string()],
            Some(&pin)
        )
        .is_err());
    }

    #[test]
    fn an_unpinned_pack_still_needs_every_listed_file_to_match() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("compiler.wasm"), "c").unwrap();
        let sum = crate::providers::sha256_of(&dir.path().join("compiler.wasm")).unwrap();
        std::fs::write(dir.path().join(LIST), format!("{sum}  compiler.wasm\n")).unwrap();
        assert!(check_with(dir.path(), None).is_ok());
        std::fs::write(dir.path().join("compiler.wasm"), "changed").unwrap();
        assert!(check_with(dir.path(), None).is_err());
    }
}
