use anyhow::{Context, Result};
use std::{
    ffi::OsStr,
    io::{ErrorKind, Read},
    os::{fd::AsRawFd, unix::process::CommandExt},
    process::{Output, Stdio},
    time::{Duration, Instant},
};
fn drain(reader: &mut impl Read, bytes: &mut Vec<u8>) {
    let mut buffer = [0u8; 4096];
    while bytes.len() < 64 * 1024 {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => bytes.extend_from_slice(&buffer[..n.min(64 * 1024 - bytes.len())]),
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => break,
            Err(_) => break,
        }
    }
}
fn nonblocking(fd: std::os::fd::RawFd) -> Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags == -1 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}
pub fn capture(program: &str, args: &[&OsStr], timeout: Duration) -> Result<Output> {
    let mut command = leo_core::children::quiet_command(program);
    command
        .args(args)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .context("The helper could not start. Check Health for its setup instructions.")?;
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    if let Err(error) =
        nonblocking(stdout.as_raw_fd()).and_then(|_| nonblocking(stderr.as_raw_fd()))
    {
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGKILL);
        }
        let _ = child.kill();
        let _ = child.wait();
        return Err(
            error.context("The helper output could not be read safely. Reopen Leo and try again.")
        );
    }
    let mut out = Vec::new();
    let mut err = Vec::new();
    let deadline = Instant::now() + timeout;
    let status = loop {
        drain(&mut stdout, &mut out);
        drain(&mut stderr, &mut err);
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("The helper took too long. Try again in a moment.");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    drain(&mut stdout, &mut out);
    drain(&mut stderr, &mut err);
    Ok(Output {
        status,
        stdout: out,
        stderr: err,
    })
}
#[cfg(test)]
mod tests {
    #[test]
    fn invalid_output_handles_fail_instead_of_falling_back_to_blocking_reads() {
        assert!(super::nonblocking(-1).is_err());
    }
    #[test]
    fn a_helper_cannot_wait_on_a_descendant_holding_its_output() {
        use std::{
            ffi::OsStr,
            time::{Duration, Instant},
        };
        let now = Instant::now();
        let output = super::capture(
            "/bin/sh",
            &[OsStr::new("-c"), OsStr::new("(sleep 2) & printf done")],
            Duration::from_millis(150),
        )
        .unwrap();
        assert_eq!(output.stdout, b"done");
        assert!(now.elapsed() < Duration::from_millis(500));
    }
}
