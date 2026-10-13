use std::path::PathBuf;

use anyhow::Result;

use crate::providers::{download_verified, files_state, model_state, ModelState};

pub const URL: &str = "https://cdn.jsdelivr.net/pyodide/v0.28.3/full";
pub const MANIFEST: &str = "pyodide.js=24a458425dcb4ea9836eb5ce26701d18cb769374e2b79247602ba605bf093278 pyodide.asm.js=b22e5831eade9ff10e6fe2c811c68688cd91f10154377b4f80debcf5bafa1e56 pyodide.asm.wasm=5effb6a1a6cc4a1a85bec4622701aa797c031e1de923cbbaf2ad47abdc4ab325 python_stdlib.zip=71fee17f88a6260ec8c9c7c063533ee59c021fdc88a1ce76247378d3c4a35f4c pyodide-lock.json=f6e6f42f451f42affbbcddb00e8c9a3278dcbf399f57aab9f3f568839a7ff4a6";
pub const MB: u32 = 12;

pub fn manifest() -> Vec<(String, String)> {
    let text = std::env::var("LEO_PYODIDE_MANIFEST").unwrap_or_else(|_| MANIFEST.to_string());
    text.split_whitespace()
        .filter_map(|pair| pair.split_once('='))
        .map(|(name, sha)| (name.to_string(), sha.to_string()))
        .collect()
}

pub fn dir() -> PathBuf {
    crate::config::choice::models_dir().join("pyodide-0.28.3")
}

pub fn ready() -> bool {
    files_state(&dir(), &manifest()) == ModelState::Ready
}

pub fn fetch() -> Result<PathBuf> {
    let base = std::env::var("LEO_PYODIDE_URL").unwrap_or_else(|_| URL.to_string());
    let dir = dir();
    for (name, sha) in manifest() {
        if model_state(&dir.join(&name), &sha) == ModelState::Ready {
            continue;
        }
        download_verified(&format!("{base}/{name}"), &sha, &dir, &name, true)?;
    }
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pyodide_file_is_pinned_by_its_checksum() {
        let files = manifest();
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                "pyodide.js",
                "pyodide.asm.js",
                "pyodide.asm.wasm",
                "python_stdlib.zip",
                "pyodide-lock.json"
            ]
        );
        assert!(files
            .iter()
            .all(|(_, sha)| sha.len() == 64 && sha.chars().all(|c| c.is_ascii_hexdigit())));
        assert!(URL.starts_with("https://") && URL.contains("v0.28.3"));
    }
}
