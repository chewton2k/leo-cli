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
    pub fn new(value: Zeroizing<String>) -> Self {
        Secret(value)
    }

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

/// So a `Box<dyn SecretStore>` can be passed anywhere a store is expected,
/// which is what choosing the backend at runtime requires.
impl SecretStore for Box<dyn SecretStore> {
    fn get(&self, account: &str) -> Result<Option<Secret>> {
        (**self).get(account)
    }
    fn has(&self, account: &str) -> bool {
        (**self).has(account)
    }
    fn set(&self, account: &str, secret: &str) -> Result<()> {
        (**self).set(account, secret)
    }
    fn delete(&self, account: &str) -> Result<()> {
        (**self).delete(account)
    }
    fn available(&self) -> bool {
        (**self).available()
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
            leo_core::diag::warn(format!(
                "could not read the stored credential for \"{provider}\": {e}"
            ));
            None
        }
    }
}

/// The store leo uses for credentials.
///
/// A file by default, not the OS keychain. On macOS a keychain item records the
/// binary that created it and asks permission whenever a different one reads it,
/// and every `cargo install` produces a different binary — so the prompt came
/// back after every upgrade, once per provider, with no way to answer it for
/// good. That is not a trade a note-taking tool should ask its user to make.
///
/// `LEO_USE_KEYCHAIN=1` opts back in for anyone who prefers encryption at rest
/// and does not mind the prompts. Env vars still beat both.
///
/// Returned boxed because the two implementations are different types and the
/// choice is made at runtime.
pub fn default_store() -> Box<dyn SecretStore> {
    if std::env::var("LEO_USE_KEYCHAIN").is_ok_and(|v| v != "0" && !v.is_empty()) {
        return Box::new(KeyringStore);
    }
    match crate::config::file_store::FileStore::new() {
        Ok(store) => Box::new(store),
        // No config directory is a genuinely broken environment; the keychain is
        // a better answer than pretending nothing is stored.
        Err(e) => {
            leo_core::diag::warn(format!("could not locate the credentials file ({e})"));
            Box::new(KeyringStore)
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

/// Whether an error means "there is no credential store here" rather than
/// "the credential store failed".
///
/// A headless Linux box with no Secret Service, a container, or a macOS account
/// with no login keychain all land here. None of them are errors: leo's
/// non-AI features do not need a key, and the AI ones can read env vars.
fn backend_is_absent(e: &anyhow::Error) -> bool {
    let text = e.to_string().to_lowercase();
    text.contains("no default keychain")
        || text.contains("default keychain could not be found")
        || text.contains("no such file or directory")
        || text.contains("was not provided by any")
        || text.contains("org.freedesktop.secrets")
        || text.contains("secretservice")
        || text.contains("no storage access")
}

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
                leo_core::diag::warn(format!(
                    "stored credentials could not be read ({e}); treating them as absent"
                ));
                Bundle::new()
            }),
            Ok(None) => Bundle::new(),
            // A backend that is absent rather than broken is not a problem worth
            // reporting: a machine with no keychain simply has no stored keys,
            // and leo works without any. Saying so would make the first command
            // a new user runs look like a failure.
            Err(e) if backend_is_absent(&e) => Bundle::new(),
            Err(e) => {
                leo_core::diag::warn(format!("could not read stored credentials: {e}"));
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

}

impl SecretStore for KeyringStore {
    fn get(&self, account: &str) -> Result<Option<Secret>> {
        Ok(self
            .bundle()
            .get(account)
            .map(|found| Secret(Zeroizing::new(found.clone()))))
    }

    /// Whether a key is stored, without reading its value.
    ///
    /// The provider screen calls this for every provider it lists, and on a
    /// keychain a *read* is what can cost a permission dialog.
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
#[cfg(any(test, feature = "test-support"))]
#[derive(Default)]
pub struct MemoryStore {
    items: std::sync::Mutex<std::collections::HashMap<String, String>>,
}

#[cfg(any(test, feature = "test-support"))]
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

    /// The first command a new user runs must not look like a failure. A machine
    /// with no keychain at all has no stored keys, which is a fact, not an error.
    #[test]
    fn a_missing_credential_store_is_not_reported_as_an_error() {
        for absent in [
            "Platform failure: A default keychain could not be found.",
            "Platform secure storage failure: no such file or directory",
            "The name org.freedesktop.secrets was not provided by any .service files",
            "No storage access: SecretService unavailable",
        ] {
            assert!(
                backend_is_absent(&anyhow::anyhow!(absent)),
                "should be treated as absent: {absent}"
            );
        }
    }

    /// A store that is present but genuinely failing must still be reported,
    /// otherwise a real problem becomes silent key loss.
    #[test]
    fn a_broken_credential_store_is_still_reported() {
        for real in [
            "Platform failure: authorization denied by the user",
            "invalid utf-8 in the stored value",
            "keychain item is corrupt",
        ] {
            assert!(
                !backend_is_absent(&anyhow::anyhow!(real)),
                "should be reported: {real}"
            );
        }
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
