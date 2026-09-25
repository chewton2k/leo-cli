//! Credentials in a file only this user can read.
//!
//! This is the default because the OS keychain, on macOS, cannot be made quiet
//! for a tool like this. A keychain item records *which binary* created it and
//! asks for permission whenever a different one reads it — and every
//! `cargo install` produces a different binary, so the prompt came back after
//! every upgrade, once per provider, with no way to answer it permanently.
//!
//! What this gives up, stated plainly: the file is not encrypted, so it is
//! readable by anything running as this user, and by anyone who can read the
//! disk while it is unlocked. What it keeps: mode `0600` in a `0700` directory,
//! so no other account on the machine can read it, and on macOS FileVault still
//! encrypts it at rest. That is the same arrangement as `~/.aws/credentials`,
//! `~/.npmrc`, and `gh`'s token file.
//!
//! Anyone who wants nothing on disk at all can set the provider's env var, which
//! takes precedence over this file, or opt into the keychain with
//! `LEO_USE_KEYCHAIN=1`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use zeroize::Zeroizing;

use super::secret::{Secret, SecretStore};

/// provider name -> key. `BTreeMap` so the file is stable and diffable.
type Bundle = BTreeMap<String, String>;

/// Permissions for the file: readable and writable by the owner only.
#[cfg(unix)]
const FILE_MODE: u32 = 0o600;
/// Permissions for the directory holding it.
#[cfg(unix)]
const DIR_MODE: u32 = 0o700;

pub struct FileStore {
    path: PathBuf,
}

impl FileStore {
    /// The default location, beside `config.toml`.
    pub fn new() -> Result<Self> {
        let path = super::Config::config_path()?.with_file_name("credentials.json");
        Ok(Self { path })
    }

    /// For tests, and for anyone who wants it somewhere else.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn read(&self) -> Bundle {
        let Ok(text) = std::fs::read_to_string(&self.path) else {
            return Bundle::new();
        };
        match serde_json::from_str::<Bundle>(&text) {
            Ok(bundle) => bundle,
            Err(e) => {
                // Never silently discard credentials: report and behave as
                // though none are stored, so a write does not clobber a file
                // that may be recoverable by hand.
                leo_core::diag::warn(format!(
                    "{} could not be read ({e}); treating it as empty",
                    self.path.display()
                ));
                Bundle::new()
            }
        }
    }

    fn write(&self, bundle: &Bundle) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("could not create {}", parent.display()))?;
            Self::restrict_dir(parent);
        }

        let json = serde_json::to_string_pretty(bundle)?;

        // Write to a sibling first so a crash cannot truncate the real file,
        // and create it with the right mode from the start rather than
        // loosening it afterwards — a window where the file is world-readable
        // is the bug this ordering avoids.
        let tmp = self.path.with_extension("json.new");
        Self::write_private(&tmp, json.as_bytes())
            .with_context(|| format!("could not write {}", tmp.display()))?;
        std::fs::rename(&tmp, &self.path)
            .with_context(|| format!("could not replace {}", self.path.display()))?;
        Ok(())
    }

    #[cfg(unix)]
    fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;

        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(FILE_MODE)
            .open(path)?;
        file.write_all(bytes)?;
        file.sync_all()
    }

    #[cfg(not(unix))]
    fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        // Windows inherits the user profile's ACL, which is already per-user.
        std::fs::write(path, bytes)
    }

    #[cfg(unix)]
    fn restrict_dir(dir: &Path) {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(DIR_MODE));
    }

    #[cfg(not(unix))]
    fn restrict_dir(_dir: &Path) {}

    /// Whether the file is readable by anyone but its owner.
    ///
    /// Checked rather than assumed: the file may predate this code, or have been
    /// copied in by hand.
    #[cfg(unix)]
    pub fn is_private(&self) -> bool {
        use std::os::unix::fs::PermissionsExt;
        match std::fs::metadata(&self.path) {
            Ok(meta) => meta.permissions().mode() & 0o077 == 0,
            // A file that does not exist cannot leak.
            Err(_) => true,
        }
    }

    #[cfg(not(unix))]
    pub fn is_private(&self) -> bool {
        true
    }

    /// Tighten permissions on a file that is more open than it should be.
    #[cfg(unix)]
    pub fn make_private(&self) -> bool {
        use std::os::unix::fs::PermissionsExt;
        if !self.path.exists() || self.is_private() {
            return false;
        }
        std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(FILE_MODE)).is_ok()
    }

    #[cfg(not(unix))]
    pub fn make_private(&self) -> bool {
        false
    }
}

impl SecretStore for FileStore {
    fn get(&self, account: &str) -> Result<Option<Secret>> {
        Ok(self
            .read()
            .get(account)
            .map(|s| Secret::new(Zeroizing::new(s.clone()))))
    }

    fn has(&self, account: &str) -> bool {
        self.read().contains_key(account)
    }

    fn set(&self, account: &str, secret: &str) -> Result<()> {
        let mut bundle = self.read();
        bundle.insert(account.to_string(), secret.to_string());
        self.write(&bundle)
    }

    fn delete(&self, account: &str) -> Result<()> {
        let mut bundle = self.read();
        if bundle.remove(account).is_some() {
            self.write(&bundle)?;
        }
        Ok(())
    }

    fn available(&self) -> bool {
        // A directory leo can write to is all this needs, and it makes one.
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (FileStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStore::at(dir.path().join("credentials.json"));
        (store, dir)
    }

    #[test]
    fn a_key_round_trips() {
        let (store, _d) = store();
        assert!(store.get("groq").unwrap().is_none());

        store.set("groq", "gsk-secret").unwrap();
        assert_eq!(store.get("groq").unwrap().unwrap().as_str(), "gsk-secret");
        assert!(store.has("groq"));

        store.delete("groq").unwrap();
        assert!(store.get("groq").unwrap().is_none());
        assert!(!store.has("groq"));
    }

    #[test]
    fn several_providers_share_the_file() {
        let (store, _d) = store();
        store.set("groq", "a").unwrap();
        store.set("openrouter", "b").unwrap();
        store.set("hf", "c").unwrap();

        assert_eq!(store.get("groq").unwrap().unwrap().as_str(), "a");
        assert_eq!(store.get("openrouter").unwrap().unwrap().as_str(), "b");
        assert_eq!(store.get("hf").unwrap().unwrap().as_str(), "c");

        // Removing one leaves the others.
        store.delete("openrouter").unwrap();
        assert!(store.has("groq"));
        assert!(!store.has("openrouter"));
        assert!(store.has("hf"));
    }

    /// The one protection this design relies on. If the mode is wrong, the
    /// trade-off that justified a file instead of the keychain does not hold.
    #[cfg(unix)]
    #[test]
    fn the_file_is_readable_only_by_its_owner() {
        use std::os::unix::fs::PermissionsExt;
        let (store, _d) = store();
        store.set("groq", "gsk-secret").unwrap();

        let mode = std::fs::metadata(store.path())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "mode is {:o}", mode & 0o777);
        assert!(store.is_private());

        let dir_mode = std::fs::metadata(store.path().parent().unwrap())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(
            dir_mode & 0o777,
            0o700,
            "dir mode is {:o}",
            dir_mode & 0o777
        );
    }

    /// A file left open by an older version, or copied in by hand, must be
    /// noticed and fixable.
    #[cfg(unix)]
    #[test]
    fn an_over_permissive_file_is_detected_and_tightened() {
        use std::os::unix::fs::PermissionsExt;
        let (store, _d) = store();
        store.set("groq", "gsk-secret").unwrap();

        std::fs::set_permissions(store.path(), std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(!store.is_private(), "0644 reported as private");

        assert!(store.make_private());
        assert!(store.is_private());
        // And the value survived the permission change.
        assert_eq!(store.get("groq").unwrap().unwrap().as_str(), "gsk-secret");
    }

    /// Writing must never leave a half-written file behind on the real path.
    #[test]
    fn a_write_leaves_no_temporary_file() {
        let (store, _d) = store();
        store.set("groq", "a").unwrap();
        assert!(!store.path().with_extension("json.new").exists());
    }

    #[test]
    fn a_corrupt_file_is_reported_rather_than_silently_replaced() {
        let (store, _d) = store();
        std::fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        std::fs::write(store.path(), "not json at all").unwrap();

        // Reads report nothing rather than panicking.
        assert!(store.get("groq").unwrap().is_none());
        assert!(!store.has("groq"));
    }

    #[test]
    fn a_missing_file_is_simply_empty() {
        let (store, _d) = store();
        assert!(!store.path().exists());
        assert!(store.get("anything").unwrap().is_none());
        assert!(store.available());
    }
}
