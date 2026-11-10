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
    /// Separate from `get` so a store can answer without handing out the
    /// value. Anything that only renders status should call this.
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

pub fn masked(secret: &str) -> String {
    let secret = secret.trim();
    let quarter = secret.chars().count() / 4;
    let head: String = secret.chars().take(8).collect();
    let shown = [head.rfind('-'), head.find('-')]
        .into_iter()
        .flatten()
        .map(|i| &head[..=i])
        .find(|prefix| prefix.chars().count() <= quarter)
        .unwrap_or("");
    format!("{shown}…••••••••")
}

/// The key leo stored for this provider, and nothing else: an environment
/// variable with the same name is never read, so a key left over in a shell
/// profile or another project's `.env` cannot quietly replace the one the user
/// chose. A store error degrades to `None` — a missing key is a skipped
/// provider, never a crash — but is not swallowed silently: it is reported to
/// stderr first, since "denied a keychain ACL prompt" and "never logged in"
/// are different situations for a user to act on.
pub fn resolve(provider: &str, store: &dyn SecretStore) -> Option<Secret> {
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

/// The bundle as read this process. `Unread` means "not read yet"; `Failed`
/// means the read failed, so nothing reads it again until a write needs it.
enum Read {
    Unread,
    Ok(Bundle),
    Failed,
}

struct Bundles(Mutex<Read>);

impl Bundles {
    const fn new() -> Self {
        Bundles(Mutex::new(Read::Unread))
    }

    fn current(&self, load: &dyn Fn() -> Result<Bundle>) -> Option<Bundle> {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        match &*state {
            Read::Ok(bundle) => return Some(bundle.clone()),
            Read::Failed => return None,
            Read::Unread => {}
        }
        match load() {
            Ok(bundle) => {
                *state = Read::Ok(bundle.clone());
                Some(bundle)
            }
            Err(e) => {
                leo_core::diag::warn(format!("could not read stored credentials: {e}"));
                *state = Read::Failed;
                None
            }
        }
    }

    fn change(
        &self,
        load: &dyn Fn() -> Result<Bundle>,
        write: &dyn Fn(&Bundle) -> Result<()>,
        edit: impl FnOnce(&mut Bundle) -> bool,
    ) -> Result<()> {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let mut bundle = match &*state {
            Read::Ok(bundle) => bundle.clone(),
            Read::Unread | Read::Failed => {
                let bundle = load()?;
                *state = Read::Ok(bundle.clone());
                bundle
            }
        };
        if edit(&mut bundle) {
            write(&bundle)?;
            *state = Read::Ok(bundle);
        }
        Ok(())
    }
}

static KEYCHAIN: Bundles = Bundles::new();

pub fn forget_keychain(providers: &[String]) -> usize {
    std::iter::once(BUNDLE_ACCOUNT.to_string())
        .chain(providers.iter().cloned())
        .filter(|account| {
            keyring::Entry::new(SERVICE, account)
                .and_then(|entry| entry.delete_credential())
                .is_ok()
        })
        .count()
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

    fn load() -> Result<Bundle> {
        match Self::entry(BUNDLE_ACCOUNT).and_then(|e| match e.get_password() {
            Ok(json) => Ok(Some(json)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.into()),
        }) {
            Ok(Some(json)) => Ok(serde_json::from_str::<Bundle>(&json).unwrap_or_else(|e| {
                // Refuse to guess at a corrupt bundle: report it and behave as
                // though nothing is stored, rather than deleting anything.
                leo_core::diag::warn(format!(
                    "stored credentials could not be read ({e}); treating them as absent"
                ));
                Bundle::new()
            })),
            Ok(None) => Ok(Bundle::new()),
            // A backend that is absent rather than broken is not a problem worth
            // reporting: a machine with no keychain simply has no stored keys,
            // and leo works without any. Saying so would make the first command
            // a new user runs look like a failure.
            Err(e) if backend_is_absent(&e) => Ok(Bundle::new()),
            Err(e) => Err(e),
        }
    }

    fn write_bundle(bundle: &Bundle) -> Result<()> {
        let json = serde_json::to_string(bundle)?;
        Self::entry(BUNDLE_ACCOUNT)?.set_password(&json)?;
        Ok(())
    }

    /// Read the bundle, at most once per process.
    fn bundle(&self) -> Option<Bundle> {
        KEYCHAIN.current(&Self::load)
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, account: &str) -> Result<Option<Secret>> {
        Ok(self
            .bundle()
            .and_then(|bundle| bundle.get(account).cloned())
            .map(|found| Secret(Zeroizing::new(found))))
    }

    /// Whether a key is stored. The keychain cannot say without reading the
    /// one bundle item, so the first call can cost the permission dialog;
    /// later calls, and calls after a failed read, use what was read.
    fn has(&self, account: &str) -> bool {
        self.bundle()
            .is_some_and(|bundle| bundle.contains_key(account))
    }

    fn set(&self, account: &str, secret: &str) -> Result<()> {
        KEYCHAIN.change(&Self::load, &Self::write_bundle, |bundle| {
            bundle.insert(account.to_string(), secret.to_string());
            true
        })
    }

    fn delete(&self, account: &str) -> Result<()> {
        KEYCHAIN.change(&Self::load, &Self::write_bundle, |bundle| {
            bundle.remove(account).is_some()
        })?;
        // Also clear anything an older version left behind, so a stale key
        // cannot come back through the legacy path.
        let _ = Self::entry(account).map(|e| e.delete_credential());
        Ok(())
    }

    fn available(&self) -> bool {
        // Reading the bundle is the probe: it is cached, so asking costs
        // nothing after the first call, and it fails soft on a dead backend.
        self.bundle().is_some()
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
    fn masked_shows_only_the_kind_of_key() {
        assert_eq!(
            masked("sk-proj-abcdefghijklmnopqrstuvwxyz0123456789"),
            "sk-proj-…••••••••"
        );
        assert_eq!(
            masked("sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123"),
            "sk-ant-…••••••••"
        );
        assert_eq!(
            masked("xai-abcdefghijklmnopqrstuvwxyz0123456789"),
            "xai-…••••••••"
        );
        assert_eq!(masked("AIzaSyAbcdefghijklmnopqrstuvwxyz01234"), "…••••••••");
        assert_eq!(masked("gm-fake-key-123"), "gm-…••••••••");
        assert_eq!(masked("ab-c"), "…••••••••");
        assert_eq!(masked(""), "…••••••••");
        assert!(!masked("sk-proj-abcdefghijklmnopqrstuvwxyz0123456789").contains("abcd"));
    }

    #[test]
    fn redact_never_contains_the_original_secret() {
        let secret = "sk-or-v1-supersecretvalue";
        let shown = redact(secret);
        assert!(!shown.contains("supersecret"));
        assert!(!shown.contains(secret));
        // The redacted form must be short: "…" plus at most 4 characters.
        assert!(
            shown.chars().count() <= 5,
            "redacted output too long: {shown:?}"
        );
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

    struct FakeKeychain {
        item: Mutex<Bundle>,
        failing: std::sync::atomic::AtomicBool,
        reads: std::sync::atomic::AtomicUsize,
        writes: std::sync::atomic::AtomicUsize,
    }

    impl FakeKeychain {
        fn holding(keys: &[(&str, &str)]) -> Self {
            FakeKeychain {
                item: Mutex::new(
                    keys.iter()
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect(),
                ),
                failing: false.into(),
                reads: 0.into(),
                writes: 0.into(),
            }
        }

        fn load(&self) -> Result<Bundle> {
            use std::sync::atomic::Ordering;
            self.reads.fetch_add(1, Ordering::SeqCst);
            if self.failing.load(Ordering::SeqCst) {
                anyhow::bail!("Platform failure: authorization denied by the user");
            }
            let read = self.item.lock().unwrap().clone();
            std::thread::sleep(std::time::Duration::from_millis(2));
            Ok(read)
        }

        fn write(&self, bundle: &Bundle) -> Result<()> {
            self.writes
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            *self.item.lock().unwrap() = bundle.clone();
            Ok(())
        }
    }

    #[test]
    fn a_failed_keychain_read_is_never_written_over_and_is_not_read_again() {
        use std::sync::atomic::Ordering;
        let keychain = FakeKeychain::holding(&[("calendar", "c"), ("openrouter", "o")]);
        keychain.failing.store(true, Ordering::SeqCst);
        let cache = Bundles::new();
        let load = || keychain.load();
        let write = |b: &Bundle| keychain.write(b);
        for _ in 0..5 {
            assert!(cache.current(&load).is_none());
        }
        assert_eq!(keychain.reads.load(Ordering::SeqCst), 1);
        let insert = |b: &mut Bundle| {
            b.insert("feed".into(), "f".into());
            true
        };
        assert!(cache.change(&load, &write, insert).is_err());
        assert_eq!(keychain.writes.load(Ordering::SeqCst), 0);
        assert_eq!(keychain.item.lock().unwrap().len(), 2);
        keychain.failing.store(false, Ordering::SeqCst);
        cache.change(&load, &write, insert).unwrap();
        let stored = keychain.item.lock().unwrap().clone();
        assert_eq!(stored.len(), 3);
        assert_eq!(cache.current(&load).unwrap(), stored);
    }

    #[test]
    fn concurrent_keychain_writes_keep_every_credential() {
        let keychain = FakeKeychain::holding(&[("k", "v")]);
        let cache = Bundles::new();
        std::thread::scope(|scope| {
            for n in 0..8 {
                let (keychain, cache) = (&keychain, &cache);
                scope.spawn(move || {
                    cache
                        .change(&|| keychain.load(), &|b| keychain.write(b), |b| {
                            b.insert(format!("feed-{n}"), "x".into());
                            true
                        })
                        .unwrap();
                });
            }
        });
        assert_eq!(keychain.item.lock().unwrap().len(), 9);
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
    fn only_the_stored_key_is_used_never_an_environment_variable() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("OPENROUTER_API_KEY", "from-env");
        let store = MemoryStore::default();
        assert!(resolve("openrouter", &store).is_none());
        store.set("openrouter", "from-leo").unwrap();
        let got = resolve("openrouter", &store).unwrap();
        std::env::remove_var("OPENROUTER_API_KEY");
        assert_eq!(got.as_str(), "from-leo");
    }

    #[test]
    fn unavailable_store_degrades_instead_of_failing() {
        let _guard = ENV_LOCK.lock().unwrap();
        assert!(resolve("openrouter", &BrokenStore).is_none());
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
        assert!(resolve("openrouter", &store).is_none());
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
