use std::path::Path;

const FOREGROUND: &str = "update-foreground";

pub fn may_open(gate: &leo_core::activity::Gate) -> bool {
    !gate.installing()
}

pub fn remember(config: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(config)?;
    let fresh = tempfile::NamedTempFile::new_in(config)?;
    fresh.persist(config.join(FOREGROUND))?;
    Ok(())
}

pub fn take(config: &Path) -> bool {
    std::fs::remove_file(config.join(FOREGROUND)).is_ok()
}

pub fn hidden(at_login: bool, foreground: bool) -> bool {
    at_login && !foreground
}

#[cfg(test)]
mod tests {
    #[test]
    fn opening_waits_for_installation_and_recovers_on_failure() {
        let gate = leo_core::activity::Gate::default();
        assert!(super::may_open(&gate));
        let recording = gate.begin().unwrap();
        assert!(super::may_open(&gate));
        drop(recording);
        let installation = gate.install().unwrap();
        assert!(!super::may_open(&gate));
        drop(installation);
        assert!(super::may_open(&gate));
    }

    #[test]
    fn an_update_restart_is_foreground_once_even_after_starting_at_login() {
        let dir = tempfile::tempdir().unwrap();
        for at_login in [false, true] {
            assert_eq!(super::hidden(at_login, super::take(dir.path())), at_login);
            super::remember(dir.path()).unwrap();
            assert!(!super::hidden(at_login, super::take(dir.path())));
            assert_eq!(super::hidden(at_login, super::take(dir.path())), at_login);
        }
    }
}
