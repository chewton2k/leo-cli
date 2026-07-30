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

use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::Result;
use zeroize::Zeroizing;

/// Service name under which all leo credentials are filed in the OS keychain.
pub const SERVICE: &str = "leo";

/// Account name used only to ask whether a backend answers at all.
const PROBE: &str = "__leo_probe__";

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

/// Reads already served this process, so the keychain is asked at most once per
/// provider per run.
///
/// macOS prompts for permission on each access when the binary's signature does
/// not match the entry's ACL, and leo reads several providers in a row — the
/// provider screen alone touches every configured one. Without this, opening it
/// meant a dozen consecutive permission dialogs.
///
/// A cache is safe here because writes go through the same type and update it:
/// nothing else in the process can change a keychain entry behind our back, and
/// an entry changed by another application mid-session is not worth a prompt
/// storm to notice.
static CACHE: Mutex<Option<HashMap<String, Option<String>>>> = Mutex::new(None);

fn cached(account: &str) -> Option<Option<Secret>> {
    let guard = CACHE.lock().ok()?;
    let map = guard.as_ref()?;
    map.get(account)
        .map(|v| v.as_ref().map(|s| Secret(Zeroizing::new(s.clone()))))
}

fn remember(account: &str, value: Option<&str>) {
    if let Ok(mut guard) = CACHE.lock() {
        guard
            .get_or_insert_with(HashMap::new)
            .insert(account.to_string(), value.map(str::to_string));
    }
}

/// The real OS keychain: macOS Keychain, Windows Credential Manager, or Linux
/// Secret Service, selected by the `keyring` crate's default feature.
///
/// The underlying `keyring` crate lazily initializes the platform-specific
/// credential store the first time an `Entry` is created (see `v1::Entry::new`
/// in the `keyring` crate source); no explicit setup call is needed here.
pub struct KeyringStore;

impl KeyringStore {
    fn entry(account: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(SERVICE, account).map_err(Into::into)
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, account: &str) -> Result<Option<Secret>> {
        if let Some(hit) = cached(account) {
            return Ok(hit);
        }
        match Self::entry(account)?.get_password() {
            Ok(secret) => {
                remember(account, Some(&secret));
                Ok(Some(Secret(Zeroizing::new(secret))))
            }
            Err(keyring::Error::NoEntry) => {
                // Remember the absence too: a provider with no key is read just
                // as often as one with a key.
                remember(account, None);
                Ok(None)
            }
            // `keyring_core::Error::BadEncoding(Vec<u8>)` and `BadDataFormat`
            // carry the raw credential bytes and derive `Debug`. We are safe
            // here only because `anyhow::Error`'s `Display` (which `e.into()`
            // plus `resolve`'s `{e}` format both go through) renders via each
            // error's `Display` impl, which omits those bytes — a downstream
            // `downcast_ref::<keyring::Error>()` followed by `{:?}` would leak
            // them, so never do that with this error.
            Err(e) => Err(e.into()),
        }
    }

    fn set(&self, account: &str, secret: &str) -> Result<()> {
        Self::entry(account)?.set_password(secret)?;
        remember(account, Some(secret));
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<()> {
        let result = match Self::entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.into()),
        };
        // Whether or not the delete found anything, there is no key now.
        remember(account, None);
        result
    }

    fn available(&self) -> bool {
        // A probe read against a name we never write. NoEntry means the backend
        // answered, which is what we are testing for. Cached like any other
        // read, so asking repeatedly costs nothing.
        // A cached probe entry means the backend answered once, which is all
        // this reports.
        if cached(PROBE).is_some() {
            return true;
        }
        let answered = matches!(
            Self::entry(PROBE).and_then(|e| {
                match e.get_password() {
                    Ok(_) => Ok(()),
                    Err(keyring::Error::NoEntry) => Ok(()),
                    Err(e) => Err(e.into()),
                }
            }),
            Ok(())
        );
        if answered {
            remember(PROBE, None);
        }
        answered
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

    /// The bug this guards: macOS prompts for permission on every keychain
    /// access, and the provider screen reads every configured provider. Without
    /// caching, opening it meant a dialog per provider.
    #[test]
    fn repeated_reads_of_the_same_account_hit_the_cache_once() {
        // Exercised through the cache helpers directly: the real keychain is
        // never touched by tests.
        let account = "leo_test_cache_account";
        assert!(cached(account).is_none(), "nothing cached yet");

        remember(account, Some("a-key"));
        let first = cached(account).expect("a cached entry");
        assert_eq!(first.map(|s| s.as_str().to_string()), Some("a-key".to_string()));
        // Still cached: a second read does not need the backend.
        assert!(cached(account).is_some());

        // An absent key is remembered too, since a provider with no key is read
        // just as often as one with a key.
        let missing = "leo_test_cache_missing";
        remember(missing, None);
        let hit = cached(missing).expect("the absence is cached");
        assert!(hit.is_none());
    }

    /// Storing or removing a key must not leave a stale cache behind, or the
    /// provider screen would keep showing the old state.
    #[test]
    fn writing_updates_the_cache_rather_than_invalidating_it_lazily() {
        let account = "leo_test_cache_write";
        remember(account, None);
        assert!(cached(account).unwrap().is_none());

        // What `set` does after a successful write.
        remember(account, Some("new-key"));
        assert_eq!(
            cached(account).unwrap().map(|s| s.as_str().to_string()),
            Some("new-key".to_string())
        );

        // What `delete` does.
        remember(account, None);
        assert!(cached(account).unwrap().is_none());
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
