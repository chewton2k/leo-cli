use std::path::Path;

use anyhow::{Context, Result};

pub(crate) const AUTH_BYTES: u64 = 128 * 1024;
const MOST_TOKEN_BYTES: usize = 256;

pub fn load_or_create(path: &Path, renew: bool) -> Result<String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if !renew {
        if let Some(text) = read_private(path).with_context(|| {
            format!(
                "could not safely read {}; check its permissions",
                path.display()
            )
        })? {
            let token = text.trim();
            if (32..=MOST_TOKEN_BYTES).contains(&token.len())
                && token.chars().all(|c| c.is_ascii_hexdigit())
            {
                return Ok(token.to_string());
            }
        }
    }
    let token = uuid::Uuid::new_v4().simple().to_string();
    write_private(path, &token).with_context(|| format!("could not save {}", path.display()))?;
    Ok(token)
}

pub(crate) fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    if text.len() as u64 > AUTH_BYTES {
        return Err(std::io::Error::other("The sign-in file is too large"));
    }
    check_path(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("The sign-in file needs a folder"))?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(text.as_bytes())?;
    file.as_file().sync_all()?;
    check_path(path)?;
    file.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn check_path(path: &Path) -> std::io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("The sign-in file needs a folder"))?;
    if !std::fs::symlink_metadata(parent)?.is_dir() {
        return Err(std::io::Error::other(
            "The sign-in folder cannot be a symbolic link",
        ));
    }
    match std::fs::symlink_metadata(path) {
        Ok(meta) if !meta.is_file() => Err(std::io::Error::other(
            "The sign-in file must be a regular file, not a symbolic link",
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

pub(crate) fn read_private(path: &Path) -> std::io::Result<Option<String>> {
    use std::io::Read;
    check_path(path)?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > AUTH_BYTES {
        return Err(std::io::Error::other(
            "The sign-in file is not a regular file or is too large",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if meta.nlink() != 1 {
            return Err(std::io::Error::other(
                "The sign-in file cannot have hard links",
            ));
        }
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    let mut text = String::new();
    file.take(AUTH_BYTES + 1).read_to_string(&mut text)?;
    if text.len() as u64 > AUTH_BYTES {
        return Err(std::io::Error::other("The sign-in file is too large"));
    }
    Ok(Some(text))
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

    #[cfg(unix)]
    #[test]
    fn existing_codes_are_private_when_loaded_or_replaced() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("serve-token");
        let code = "0123456789abcdef0123456789abcdef";
        for renew in [false, true] {
            std::fs::write(&path, code).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            let found = load_or_create(&path, renew).unwrap();
            assert_eq!(found == code, !renew);
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn sign_in_files_refuse_symlinks_and_linked_reads_without_changing_the_target() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("private-example");
        let code = "0123456789abcdef0123456789abcdef";
        std::fs::write(&target, code).unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644)).unwrap();
        let path = tmp.path().join("serve-token");
        symlink(&target, &path).unwrap();
        assert!(load_or_create(&path, false).is_err());
        assert!(load_or_create(&path, true).is_err());
        assert!(write_private(&path, "replacement").is_err());
        std::fs::remove_file(&path).unwrap();
        std::fs::hard_link(&target, &path).unwrap();
        assert!(load_or_create(&path, false).is_err());
        assert_eq!(
            std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o644
        );
        assert_eq!(std::fs::read_to_string(&target).unwrap(), code);
        let fresh = load_or_create(&path, true).unwrap();
        assert_ne!(fresh, code);
        assert_eq!(std::fs::read_to_string(&target).unwrap(), code);
        let folder = tmp.path().join("linked-folder");
        symlink(tmp.path(), &folder).unwrap();
        assert!(load_or_create(&folder.join("serve-token"), false).is_err());
        assert!(load_or_create(&folder.join("serve-token"), true).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), fresh);
    }

    #[test]
    fn oversized_sign_in_files_are_refused_before_loading() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("serve-token");
        std::fs::write(&path, vec![b'a'; AUTH_BYTES as usize + 1]).unwrap();
        assert!(load_or_create(&path, false).is_err());
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
