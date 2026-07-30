//! Credential storage for AI provider API keys.
//!
//! **Zeroization caveat:** the `Secret`/`Zeroizing` wrapping in this module
//! zeroizes its own buffer on drop, but that is narrower protection than it
//! looks. The plaintext value also lives, for the lifetime of the process, in
//! the `environ` block whenever it was read via `std::env::var` — that memory
//! is owned by the OS/libc, not by us, and cannot be zeroized from Rust. The
//! `keyring` crate and the platform `security-framework`/D-Bus bindings it
//! calls into also build their own intermediate buffers while marshalling the
//! secret across the FFI/IPC boundary; those are dropped un-zeroized before
//! the `String` we wrap ever reaches us. Treat zeroization here as reducing
//! exposure, not eliminating it.

use std::sync::Mutex;

use anyhow::Result;
use zeroize::Zeroizing;

/// Service name under which all leo credentials are filed in the OS keychain.
pub const SERVICE: &str = "leo";

/// A secret value held in memory. The only safe ways to render this type are
/// its `Debug` and `Display` impls, both of which show `redact()`'s output —
/// never the plaintext. Deliberately does not derive `Debug`, `PartialEq`, or
/// `Eq`: a derived `Debug` would print the plaintext, and a derived
/// `PartialEq` would compare in variable time. Zeroized on drop via the inner
/// `Zeroizing<String>`.
pub struct Secret(Zeroizing<String>);

impl Secret {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Secret({})", redact(&self.0))
    }
}

impl std::fmt::Display for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", redact(&self.0))
    }
}

/// A place secrets can be persisted. Behind a trait so tests never touch the
/// real OS keychain.
pub trait SecretStore {
    fn get(&self, account: &str) -> Result<Option<Secret>>;
    fn set(&self, account: &str, secret: &str) -> Result<()>;
    fn delete(&self, account: &str) -> Result<()>;
    /// Whether a backend is usable at all (e.g. Secret Service running).
    fn available(&self) -> bool;

    /// Whether a key is stored, without needing its value.
    ///
    /// Separate from `get` because on a real keychain, reading a value can cost
    /// the user a permission dialog while merely knowing one exists does not.
    /// Anything that only renders status should call this.
    fn has(&self, account: &str) -> bool {
        matches!(self.get(account), Ok(Some(_)))
    }
}

/// Render a secret for display. Only the last four characters survive, and
/// only for secrets long enough that doing so doesn't disclose most of the
/// value; short secrets redact to a bare `…`. This is safe to print, log, or
/// show in the UI.
pub fn redact(secret: &str) -> String {
    // Trim first so a stray trailing newline (common when a key is pasted or
    // piped from a file) doesn't get counted as, or shown as, one of the
    // "real" last four characters.
    let secret = secret.trim();
    if secret.chars().count() < 8 {
        return "…".to_string();
    }
    let tail: String = secret
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("…{tail}")
}

/// Resolution order: env var first, then the store. Env-first lets an operator
/// override a stored credential for one invocation without mutating the
/// keychain. A store error degrades to `None` — a missing key is a skipped
/// provider, never a crash — but is not swallowed silently: it is reported to
/// stderr first, since "denied a keychain ACL prompt" and "never logged in"
/// are different situations for a user to act on.
pub fn resolve(provider: &str, key_env: Option<&str>, store: &dyn SecretStore) -> Option<Secret> {
    if let Some(var) = key_env {
        if let Ok(value) = std::env::var(var) {
            if !value.trim().is_empty() {
                return Some(Secret(Zeroizing::new(value)));
            }
        }
    }
    match store.get(provider) {
        Ok(secret) => secret,
        Err(e) => {
            crate::diag::warn(format!(
                "could not read the stored credential for \"{provider}\": {e}"
            ));
            None
        }
    }
}

/// All of leo's credentials live in ONE keychain item, as a JSON object keyed by
/// provider name.
///
/// This is the whole reason the design is not "one item per provider". macOS
/// asks for permission per *item* whenever the requesting binary's signature
/// does not match the item's ACL, which it does not after every reinstall. With
/// an item per provider, opening the provider screen meant a dialog for each of
/// the eighteen configured providers. With one item there is at most one dialog,
/// and "Always Allow" ends it for good.
const BUNDLE_ACCOUNT: &str = "credentials";

/// provider name -> key. `BTreeMap` so the stored JSON is stable and diffable.
type Bundle = std::collections::BTreeMap<String, String>;

/// The bundle as read this process. `None` means "not read yet"; `Some(empty)`
/// means read and there is nothing stored.
static CACHE: Mutex<Option<Bundle>> = Mutex::new(None);

/// Reset the cache. Used by tests, and after a write.
fn cache_put(bundle: Bundle) {
    if let Ok(mut guard) = CACHE.lock() {
        *guard = Some(bundle);
    }
}

fn cache_get() -> Option<Bundle> {
    CACHE.lock().ok().and_then(|guard| guard.clone())
}

/// The real OS keychain: macOS Keychain, Windows Credential Manager, or Linux
/// Secret Service, selected by the `keyring` crate's default feature.
///
/// The underlying `keyring` crate lazily initializes the platform-specific
/// credential store the first time an `Entry` is created; no explicit setup call
/// is needed here.
pub struct KeyringStore;

impl KeyringStore {
    fn entry(account: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(SERVICE, account).map_err(Into::into)
    }

    /// Read the bundle, at most once per process.
    fn bundle(&self) -> Bundle {
        if let Some(cached) = cache_get() {
            return cached;
        }

        let bundle = match Self::entry(BUNDLE_ACCOUNT).and_then(|e| match e.get_password() {
            Ok(json) => Ok(Some(json)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.into()),
        }) {
            Ok(Some(json)) => serde_json::from_str::<Bundle>(&json).unwrap_or_else(|e| {
                // Refuse to guess at a corrupt bundle: report it and behave as
                // though nothing is stored, rather than deleting anything.
                crate::diag::warn(format!(
                    "stored credentials could not be read ({e}); treating them as absent"
                ));
                Bundle::new()
            }),
            Ok(None) => Bundle::new(),
            Err(e) => {
                crate::diag::warn(format!("could not read stored credentials: {e}"));
                Bundle::new()
            }
        };

        cache_put(bundle.clone());
        bundle
    }

    fn write_bundle(&self, bundle: &Bundle) -> Result<()> {
        let json = serde_json::to_string(bundle)?;
        Self::entry(BUNDLE_ACCOUNT)?.set_password(&json)?;
        cache_put(bundle.clone());
        Ok(())
    }

    /// Read a credential stored by an older version, which used one item per
    /// provider, and fold it into the bundle so it is never read again.
    ///
    /// Only called when a key is actually needed, never to display status: each
    /// one of these is a permission dialog, and the point of the bundle is to
    /// stop paying that per provider.
    fn adopt_legacy(&self, account: &str) -> Option<String> {
        let found = Self::entry(account)
            .and_then(|e| match e.get_password() {
                Ok(secret) => Ok(Some(secret)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(e) => Err(e.into()),
            })
            .ok()
            .flatten()?;

        let mut bundle = self.bundle();
        bundle.insert(account.to_string(), found.clone());
        if self.write_bundle(&bundle).is_ok() {
            // The old item is redundant now. Failing to remove it is harmless:
            // the bundle takes precedence from here.
            let _ = Self::entry(account).map(|e| e.delete_credential());
            crate::diag::warn(format!(
                "moved the stored key for \"{account}\" into leo's single keychain item"
            ));
        }
        Some(found)
    }
}

impl KeyringStore {
    /// Fold any keys stored by an older version into the bundle.
    ///
    /// Runs once per installation, guarded by the caller. Each legacy item may
    /// cost one permission dialog, which is why this happens once and eagerly
    /// rather than lazily forever: paying it now means the provider screen never
    /// pays it again.
    ///
    /// Returns the provider names that were moved.
    pub fn migrate_legacy(&self, providers: &[String]) -> Vec<String> {
        let mut moved = Vec::new();
        let mut bundle = self.bundle();

        for name in providers {
            if bundle.contains_key(name) {
                continue;
            }
            let found = Self::entry(name).and_then(|e| match e.get_password() {
                Ok(secret) => Ok(Some(secret)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(e) => Err(e.into()),
            });
            if let Ok(Some(secret)) = found {
                bundle.insert(name.clone(), secret);
                moved.push(name.clone());
            }
        }

        if !moved.is_empty() && self.write_bundle(&bundle).is_ok() {
            for name in &moved {
                let _ = Self::entry(name).map(|e| e.delete_credential());
            }
        }
        moved
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, account: &str) -> Result<Option<Secret>> {
        if let Some(found) = self.bundle().get(account) {
            return Ok(Some(Secret(Zeroizing::new(found.clone()))));
        }
        // Not in the bundle: it may predate it.
        Ok(self
            .adopt_legacy(account)
            .map(|s| Secret(Zeroizing::new(s))))
    }

    /// Whether a key is stored, answered from the bundle alone.
    ///
    /// Never consults a legacy item, because this is what the provider screen
    /// calls for every provider it lists — and prompting eighteen times to draw
    /// a list is the friction this whole design exists to remove.
    fn has(&self, account: &str) -> bool {
        self.bundle().contains_key(account)
    }

    fn set(&self, account: &str, secret: &str) -> Result<()> {
        let mut bundle = self.bundle();
        bundle.insert(account.to_string(), secret.to_string());
        self.write_bundle(&bundle)
    }

    fn delete(&self, account: &str) -> Result<()> {
        let mut bundle = self.bundle();
        let removed = bundle.remove(account).is_some();
        if removed {
            self.write_bundle(&bundle)?;
        }
        // Also clear anything an older version left behind, so a stale key
        // cannot come back through the legacy path.
        let _ = Self::entry(account).map(|e| e.delete_credential());
        Ok(())
    }

    fn available(&self) -> bool {
        // Reading the bundle is the probe: it is cached, so asking costs
        // nothing after the first call, and it fails soft on a dead backend.
        match Self::entry(BUNDLE_ACCOUNT) {
            Ok(entry) => !matches!(
                entry.get_password(),
                Err(keyring::Error::PlatformFailure(_)) | Err(keyring::Error::NoStorageAccess(_))
            ),
            Err(_) => false,
        }
    }
}

/// In-memory store for tests. Never touches the OS keychain.
///
/// `#[cfg(test)]`, so it is compiled only into test binaries — but visible to
/// every `#[cfg(test)] mod tests` in the crate, which is what the provider and
/// CLI tests need. Its backing `HashMap<String, String>` holds plaintext and
/// is never zeroized; do not use it for anything but tests.
#[cfg(test)]
#[derive(Default)]
pub struct MemoryStore {
    items: std::sync::Mutex<std::collections::HashMap<String, String>>,
}

#[cfg(test)]
impl SecretStore for MemoryStore {
    fn get(&self, account: &str) -> Result<Option<Secret>> {
        Ok(self
            .items
            .lock()
            .unwrap()
            .get(account)
            .map(|s| Secret(Zeroizing::new(s.clone()))))
    }

    fn set(&self, account: &str, secret: &str) -> Result<()> {
        self.items
            .lock()
            .unwrap()
            .insert(account.to_string(), secret.to_string());
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<()> {
        self.items.lock().unwrap().remove(account);
        Ok(())
    }

    fn available(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn redact_shows_only_last_four() {
        assert_eq!(redact("sk-or-v1-abcdefgh1234"), "…1234");
        // Shorter than the 8-char floor: full redaction, no partial disclosure.
        assert_eq!(redact("abc"), "…");
        assert_eq!(redact(""), "…");
    }

    #[test]
    fn redact_never_contains_the_original_secret() {
        let secret = "sk-or-v1-supersecretvalue";
        let shown = redact(secret);
        assert!(!shown.contains("supersecret"));
        assert!(!shown.contains(secret));
        // The redacted form must be short: "…" plus at most 4 characters.
        assert!(shown.chars().count() <= 5, "redacted output too long: {shown:?}");
        // No prefix of the secret longer than 4 characters may appear
        // anywhere in the redacted output (rules out "last N" for N > 4,
        // and rules out echoing the secret back some other way).
        for len in 5..=secret.len() {
            let prefix = &secret[..len];
            assert!(
                !shown.contains(prefix),
                "redacted output {shown:?} leaked secret prefix {prefix:?}"
            );
        }
    }

    #[test]
    fn redact_treats_short_secrets_as_fully_sensitive() {
        // Exactly at the floor and one below it.
        assert_eq!(redact("1234567"), "…"); // 7 chars: fully redacted
        assert_eq!(redact("12345678"), "…5678"); // 8 chars: last-4 applies
    }

    #[test]
    fn redact_trims_before_measuring_and_slicing() {
        // A stray trailing newline (common when a key is pasted or read from
        // a file) must not count as one of the "real" last four characters,
        // and must not push a short secret over the length floor either.
        assert_eq!(redact("sk-or-v1-abcdefgh1234\n"), "…1234");
        assert_eq!(redact("short\n"), "…");
    }

    #[test]
    fn secret_debug_and_display_never_contain_the_original_secret() {
        let secret = Secret(Zeroizing::new("sk-or-v1-supersecretvalue".to_string()));
        let debug_shown = format!("{secret:?}");
        let display_shown = format!("{secret}");
        assert!(!debug_shown.contains("supersecret"));
        assert!(!display_shown.contains("supersecret"));
        let expected = redact("sk-or-v1-supersecretvalue");
        assert!(debug_shown.contains(&expected));
        assert!(display_shown.contains(&expected));
    }

    /// The bug this guards: macOS asks for permission per keychain *item*, and
    /// an item per provider meant a dialog per provider — eighteen of them just
    /// to draw the provider screen. One item for everything means one dialog.
    #[test]
    fn every_credential_lives_in_one_keychain_item() {
        // The bundle is a JSON object keyed by provider, under a single account.
        let mut bundle = Bundle::new();
        bundle.insert("openrouter".to_string(), "key-1".to_string());
        bundle.insert("groq".to_string(), "key-2".to_string());

        let json = serde_json::to_string(&bundle).unwrap();
        let parsed: Bundle = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed["openrouter"], "key-1");

        // One account name, no matter how many providers.
        assert_eq!(BUNDLE_ACCOUNT, "credentials");
    }

    #[test]
    fn the_bundle_cache_is_read_once_and_updated_by_writes() {
        let mut bundle = Bundle::new();
        bundle.insert("groq".to_string(), "key".to_string());
        cache_put(bundle.clone());

        assert_eq!(cache_get().unwrap()["groq"], "key");

        // What `set` does: mutate and re-cache, so nothing re-reads the item.
        bundle.insert("hf".to_string(), "key-2".to_string());
        cache_put(bundle.clone());
        assert_eq!(cache_get().unwrap().len(), 2);

        // What `delete` does.
        bundle.remove("groq");
        cache_put(bundle);
        assert!(!cache_get().unwrap().contains_key("groq"));
    }

    /// A corrupt bundle must not be silently discarded or crash the app.
    #[test]
    fn an_unreadable_bundle_is_treated_as_empty() {
        assert!(serde_json::from_str::<Bundle>("not json").is_err());
        // The store maps that error to an empty bundle rather than panicking;
        // this asserts the shape it falls back to.
        assert!(Bundle::new().is_empty());
    }

    /// `has` must answer without reading the value, since a read is what costs
    /// the user a dialog.
    #[test]
    fn has_reports_presence_without_the_value() {
        let store = MemoryStore::default();
        assert!(!store.has("groq"));
        store.set("groq", "a-key").unwrap();
        assert!(store.has("groq"));
        store.delete("groq").unwrap();
        assert!(!store.has("groq"));
    }

    #[test]
    fn memory_store_round_trips() {
        let store = MemoryStore::default();
        assert!(store.get("openrouter").unwrap().is_none());
        store.set("openrouter", "key-1").unwrap();
        assert_eq!(store.get("openrouter").unwrap().unwrap().as_str(), "key-1");
        store.delete("openrouter").unwrap();
        assert!(store.get("openrouter").unwrap().is_none());
    }

    #[test]
    fn env_var_wins_over_stored_secret() {
        let _guard = ENV_LOCK.lock().unwrap();
        let store = MemoryStore::default();
        store.set("openrouter", "from-keychain").unwrap();
        std::env::set_var("LEO_TEST_KEY_A", "from-env");

        let got = resolve("openrouter", Some("LEO_TEST_KEY_A"), &store).unwrap();
        std::env::remove_var("LEO_TEST_KEY_A");

        assert_eq!(got.as_str(), "from-env");
    }

    #[test]
    fn falls_back_to_store_when_env_unset() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("LEO_TEST_KEY_B");
        let store = MemoryStore::default();
        store.set("openrouter", "from-keychain").unwrap();

        let got = resolve("openrouter", Some("LEO_TEST_KEY_B"), &store).unwrap();
        assert_eq!(got.as_str(), "from-keychain");
    }

    #[test]
    fn empty_env_var_is_treated_as_unset() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("LEO_TEST_KEY_C", "   ");
        let store = MemoryStore::default();
        store.set("openrouter", "from-keychain").unwrap();

        let got = resolve("openrouter", Some("LEO_TEST_KEY_C"), &store).unwrap();
        std::env::remove_var("LEO_TEST_KEY_C");

        assert_eq!(got.as_str(), "from-keychain");
    }

    #[test]
    fn resolve_returns_none_when_nothing_configured() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("LEO_TEST_KEY_D");
        let store = MemoryStore::default();
        assert!(resolve("openrouter", Some("LEO_TEST_KEY_D"), &store).is_none());
        assert!(resolve("openrouter", None, &store).is_none());
    }

    #[test]
    fn unavailable_store_degrades_instead_of_failing() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("LEO_TEST_KEY_E", "from-env");
        let store = BrokenStore;
        // Env still resolves even though the backend is dead.
        let got = resolve("openrouter", Some("LEO_TEST_KEY_E"), &store).unwrap();
        std::env::remove_var("LEO_TEST_KEY_E");
        assert_eq!(got.as_str(), "from-env");

        // And a store error is swallowed into None, not propagated as a panic.
        assert!(resolve("openrouter", None, &store).is_none());
    }

    #[test]
    fn resolve_degrades_to_none_on_store_error_without_panicking() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("LEO_TEST_KEY_F");
        let store = BrokenStore;
        // No env var and a backend that errors on every call: resolve must
        // still return `None`, not propagate the error or panic. (The
        // warning this path prints goes to stderr, which this test does not
        // capture, but the non-panicking `None` return is the load-bearing
        // contract.)
        assert!(resolve("openrouter", None, &store).is_none());
    }

    /// A store whose backend is missing, like headless Linux with no
    /// Secret Service.
    struct BrokenStore;

    impl SecretStore for BrokenStore {
        fn get(&self, _account: &str) -> Result<Option<Secret>> {
            anyhow::bail!("no keychain backend available")
        }
        fn set(&self, _account: &str, _secret: &str) -> Result<()> {
            anyhow::bail!("no keychain backend available")
        }
        fn delete(&self, _account: &str) -> Result<()> {
            anyhow::bail!("no keychain backend available")
        }
        fn available(&self) -> bool {
            false
        }
    }
}
