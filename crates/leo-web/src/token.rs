use std::path::Path;

use anyhow::{Context, Result};

pub fn load_or_create(path: &Path, renew: bool) -> Result<String> {
    if !renew {
        if let Ok(text) = std::fs::read_to_string(path) {
            let token = text.trim();
            if token.len() >= 32 && token.chars().all(|c| c.is_ascii_hexdigit()) {
                return Ok(token.to_string());
            }
        }
    }
    let token = uuid::Uuid::new_v4().simple().to_string();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    write_private(path, &token).with_context(|| format!("could not save {}", path.display()))?;
    Ok(token)
}

fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(text.as_bytes())
}

pub fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |diff, (x, y)| diff | (x ^ y))
            == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_code_is_kept_until_a_new_one_is_asked_for() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("serve-token");
        let first = load_or_create(&path, false).unwrap();
        assert_eq!(first.len(), 32);
        assert_eq!(load_or_create(&path, false).unwrap(), first);
        let renewed = load_or_create(&path, true).unwrap();
        assert_ne!(renewed, first);
        assert_eq!(load_or_create(&path, false).unwrap(), renewed);
    }

    #[cfg(unix)]
    #[test]
    fn only_its_owner_can_read_the_code() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("serve-token");
        load_or_create(&path, false).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn a_damaged_file_gets_a_new_code() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("serve-token");
        std::fs::write(&path, "short").unwrap();
        assert_eq!(load_or_create(&path, false).unwrap().len(), 32);
    }

    #[test]
    fn codes_compare_exactly() {
        assert!(same("abc123", "abc123"));
        assert!(!same("abc123", "abc124"));
        assert!(!same("abc", "abc123"));
        assert!(!same("", "a"));
    }
}
