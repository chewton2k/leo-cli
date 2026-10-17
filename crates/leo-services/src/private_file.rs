use std::fs::File;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{ensure, Context, Result};
use fs2::FileExt;

pub(crate) fn directory(path: &Path) -> Result<()> {
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        ensure!(
            meta.is_dir(),
            "Choose a private folder instead of a symbolic link"
        );
    }
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn contended(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::WouldBlock
        || error
            .raw_os_error()
            .is_some_and(|code| Some(code) == fs2::lock_contended_error().raw_os_error())
}

pub(crate) fn lock(parent: &Path, name: &str, wait: Duration) -> Result<File> {
    directory(parent)?;
    let path = leo_core::paths::contained_path(parent, Path::new(name))?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options
        .open(&path)
        .context("The private settings lock could not be opened safely")?;
    let meta = file.metadata()?;
    ensure!(
        meta.is_file(),
        "The private settings lock must be a regular file"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        ensure!(
            meta.nlink() == 1,
            "The private settings lock must not have hard links"
        );
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    let until = Instant::now() + wait;
    loop {
        match file.try_lock_exclusive() {
            Ok(()) => return Ok(file),
            Err(error) if contended(&error) && Instant::now() < until => {
                std::thread::sleep(Duration::from_millis(10))
            }
            Err(error) if contended(&error) => anyhow::bail!(
                "Another Leo process is using these settings. Let it finish and try again."
            ),
            Err(error) => {
                return Err(error).context("The private settings lock could not be acquired")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independent_handles_cannot_modify_private_state_at_the_same_time() {
        let dir = tempfile::tempdir().unwrap();
        let held = lock(dir.path(), ".lock", Duration::ZERO).unwrap();
        assert!(lock(dir.path(), ".lock", Duration::ZERO).is_err());
        drop(held);
        assert!(lock(dir.path(), ".lock", Duration::ZERO).is_ok());
    }

    #[test]
    fn platform_lock_contention_retries_but_other_failures_do_not() {
        assert!(contended(&fs2::lock_contended_error()));
        assert!(!contended(&std::io::Error::from(
            std::io::ErrorKind::PermissionDenied
        )));
        let dir = tempfile::tempdir().unwrap();
        let held = lock(dir.path(), ".lock", Duration::ZERO).unwrap();
        let path = dir.path().to_path_buf();
        let waiting = std::thread::spawn(move || lock(&path, ".lock", Duration::from_secs(2)));
        std::thread::sleep(Duration::from_millis(30));
        drop(held);
        assert!(waiting.join().unwrap().is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn lock_files_refuse_links_and_never_change_their_targets() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("example");
        let path = dir.path().join(".lock");
        std::fs::write(&target, "keep").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644)).unwrap();
        symlink(&target, &path).unwrap();
        assert!(lock(dir.path(), ".lock", Duration::ZERO).is_err());
        std::fs::remove_file(&path).unwrap();
        std::fs::hard_link(&target, &path).unwrap();
        assert!(lock(dir.path(), ".lock", Duration::ZERO).is_err());
        assert_eq!(
            std::fs::metadata(target).unwrap().permissions().mode() & 0o777,
            0o644
        );
    }
}
