use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::providers::{download_verified, files_state, ModelState};

pub const VERSION: &str = "22.0.0-git20542-10";
pub const URL: &str = "https://registry.npmjs.org/@yowasp/clang/-/clang-22.0.0-git20542-10.tgz";
pub const PACKAGE_SHA256: &str = "6230ea1afa9691fa065935cf68c01642ff9b31c183fe8ac64cdfda025df06009";
pub const PACKAGE_BYTES: u64 = 27_112_311;
pub const MANIFEST: &str = "bundle.js=c7ce70fb627eedc9bd6159928743da85eac7b4729aa2e776ae3771ae76c91dd6 llvm-resources.tar=79eef0c336fe55cf03ff8f5b42b784c8168f929a3603138b2c6301f4601e4c86 llvm.core.wasm=24fbed474c7b5b4968fd73fc4827440b93fb351c1b6264516130300eff3e7bf5 llvm.core2.wasm=960c326eb9b5db7aedbc169540421587a2d3f3ff987e93d6ad5b4da43430ffd4 llvm.core3.wasm=63680c043192abac4700bbda6a78e4c19b139fa086b1e872d1c6915d37a428a8 llvm.core4.wasm=f544dc9cc46f88a0f22d1b839a4fb0853dce2d6e231d880bd19754c59a5a234d";
const PACKAGE: &str = "clang.tgz";

pub fn manifest() -> Vec<(String, String)> {
    let text = std::env::var("LEO_CLANG_MANIFEST").unwrap_or_else(|_| MANIFEST.to_string());
    text.split_whitespace()
        .filter_map(|pair| pair.split_once('='))
        .map(|(name, sha)| (name.to_string(), sha.to_string()))
        .collect()
}

pub fn dir() -> PathBuf {
    crate::config::choice::models_dir().join(format!("clang-{VERSION}"))
}

pub fn ready() -> bool {
    files_state(&dir(), &manifest()) == ModelState::Ready
}

pub fn fetch() -> Result<PathBuf> {
    let url = std::env::var("LEO_CLANG_URL").unwrap_or_else(|_| URL.to_string());
    let sha = std::env::var("LEO_CLANG_SHA256").unwrap_or_else(|_| PACKAGE_SHA256.to_string());
    let dir = dir();
    fetch_into(&dir, &url, &sha, &manifest())?;
    Ok(dir)
}

pub fn fetch_into(dir: &Path, url: &str, sha: &str, manifest: &[(String, String)]) -> Result<()> {
    if files_state(dir, manifest) == ModelState::Ready {
        return Ok(());
    }
    let package = download_verified(url, sha, dir, PACKAGE, true)?;
    let unpacked = leo_core::children::quiet_command("tar")
        .arg("-xzf")
        .arg(&package)
        .arg("-C")
        .arg(dir)
        .args(["--strip-components=2", "package/gen"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    let _ = std::fs::remove_file(&package);
    anyhow::ensure!(
        unpacked.is_ok_and(|s| s.success()),
        "the C++ compiler could not be unpacked"
    );
    anyhow::ensure!(
        files_state(dir, manifest) == ModelState::Ready,
        "the C++ compiler download was damaged; try again"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_compiler_file_is_pinned_by_its_checksum() {
        let names: Vec<String> = manifest().into_iter().map(|(n, _)| n).collect();
        assert_eq!(
            names,
            [
                "bundle.js",
                "llvm-resources.tar",
                "llvm.core.wasm",
                "llvm.core2.wasm",
                "llvm.core3.wasm",
                "llvm.core4.wasm"
            ]
        );
        assert!(manifest()
            .iter()
            .all(|(_, sha)| sha.len() == 64 && sha.chars().all(|c| c.is_ascii_hexdigit())));
        assert!(URL.starts_with("https://registry.npmjs.org/") && URL.contains(VERSION));
        assert_eq!(PACKAGE_SHA256.len(), 64);
    }

    #[cfg(unix)]
    #[test]
    fn a_package_is_unpacked_and_checked_file_by_file() {
        let home = tempfile::tempdir().unwrap();
        let source = home.path().join("source");
        std::fs::create_dir_all(source.join("package/gen")).unwrap();
        std::fs::write(source.join("package/gen/bundle.js"), "export {}").unwrap();
        std::fs::write(source.join("package/README.md"), "ignored").unwrap();
        let tgz = home.path().join("clang.tgz");
        let made = std::process::Command::new("tar")
            .arg("-czf")
            .arg(&tgz)
            .arg("-C")
            .arg(&source)
            .arg("package")
            .status()
            .unwrap();
        assert!(made.success());
        let sha = crate::providers::sha256_of(&tgz).unwrap();
        let file_sha = crate::providers::sha256_of(&source.join("package/gen/bundle.js")).unwrap();
        let manifest = vec![("bundle.js".to_string(), file_sha)];
        let dir = home.path().join("clang");
        let url = format!("file://{}", tgz.display());
        assert!(fetch_into(&dir, &url, "0".repeat(64).as_str(), &manifest).is_err());
        fetch_into(&dir, &url, &sha, &manifest).unwrap();
        assert_eq!(files_state(&dir, &manifest), ModelState::Ready);
        assert!(!dir.join("README.md").exists() && !dir.join(PACKAGE).exists());
        fetch_into(&dir, "file:///nowhere", &sha, &manifest).unwrap();
    }
}
