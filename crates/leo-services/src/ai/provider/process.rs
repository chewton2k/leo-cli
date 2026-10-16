use std::io::{BufRead, Read};
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub(crate) const MOST_OUTPUT: usize = 16 * 1024 * 1024;
pub(crate) const MOST_LINE: usize = 2 * 1024 * 1024;
pub(crate) const QUEUED_MESSAGES: usize = 8;

pub(crate) fn line(reader: &mut impl BufRead) -> std::io::Result<Option<String>> {
    let mut bytes = Vec::new();
    let n = reader
        .take((MOST_LINE + 1) as u64)
        .read_until(b'\n', &mut bytes)?;
    if n > MOST_LINE {
        return Err(std::io::Error::other("the agent sent too much output"));
    }
    if n == 0 {
        return Ok(None);
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| std::io::Error::other("the agent sent unreadable output"))
}

pub(crate) fn output(reader: impl Read) -> (String, bool) {
    let mut bytes = Vec::new();
    let failed = reader
        .take((MOST_OUTPUT + 1) as u64)
        .read_to_end(&mut bytes)
        .is_err();
    let exceeded = bytes.len() > MOST_OUTPUT;
    bytes.truncate(MOST_OUTPUT);
    (
        String::from_utf8_lossy(&bytes).into_owned(),
        failed || exceeded,
    )
}

pub(crate) struct Process(pub(crate) Arc<Mutex<Option<Child>>>);

impl Process {
    pub(crate) fn new(child: Child) -> Self {
        Self(Arc::new(Mutex::new(Some(child))))
    }

    pub(crate) fn stop(&self) {
        let child = self.0.lock().ok().and_then(|mut child| child.take());
        if let Some(mut child) = child {
            super::agent_cli::stop_all(&mut child);
            let _ = child.wait();
            if let Ok(mut held) = self.0.lock() {
                *held = Some(child);
            }
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let child = self.0.lock().ok().and_then(|mut child| child.take());
        if let Some(mut child) = child {
            super::agent_cli::stop_all(&mut child);
            let _ = child.wait();
        }
    }
}

pub(crate) struct Watchdog {
    pub(crate) last: Arc<Mutex<Instant>>,
    pub(crate) stalled: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    until: Instant,
}

impl Watchdog {
    pub(crate) fn start(process: &Process, quiet: Option<Duration>, total: Duration) -> Self {
        let started = Instant::now();
        let until = started + total;
        let last = Arc::new(Mutex::new(started));
        let stalled = Arc::new(AtomicBool::new(false));
        let finished = Arc::new(AtomicBool::new(false));
        let thread = {
            let (child, last, stalled, finished) = (
                Arc::clone(&process.0),
                Arc::clone(&last),
                Arc::clone(&stalled),
                Arc::clone(&finished),
            );
            std::thread::spawn(move || {
                while !finished.load(Ordering::Relaxed) {
                    let silent = last.lock().map(|l| l.elapsed()).unwrap_or_default();
                    if Instant::now() >= until || quiet.is_some_and(|q| silent >= q) {
                        stalled.store(true, Ordering::Relaxed);
                        if let Ok(mut child) = child.lock() {
                            if let Some(child) = child.as_mut() {
                                super::agent_cli::stop_all(child);
                            }
                        }
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
            })
        };
        Self {
            last,
            stalled,
            finished,
            thread: Some(thread),
            until,
        }
    }

    pub(crate) fn remaining(&self, quiet: Duration) -> Option<Duration> {
        let left = self.until.checked_duration_since(Instant::now())?;
        if self.stalled.load(Ordering::Relaxed) {
            return None;
        }
        Some(left.min(quiet))
    }

    pub(crate) fn touch(&self) {
        if let Ok(mut last) = self.last.lock() {
            *last = Instant::now();
        }
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        self.finished.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::Write;
    use std::process::{Command, Stdio};

    fn child(script: &str) -> Child {
        let mut command = Command::new("sh");
        command
            .args(["-c", script])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        command.spawn().unwrap()
    }

    #[test]
    fn the_deadline_covers_exit_and_stderr_after_stdout_closes() {
        let mut child = child("exec 1>&-; sleep 30");
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let process = Process::new(child);
        let started = Instant::now();
        let watch = Watchdog::start(&process, None, Duration::from_millis(150));
        assert!(output(stdout).0.is_empty());
        while process
            .0
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .try_wait()
            .unwrap()
            .is_none()
        {
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "stdout EOF disabled the exit deadline"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(output(stderr).0.is_empty());
        assert!(watch.stalled.load(Ordering::Relaxed));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn steady_output_does_not_extend_the_absolute_deadline() {
        let mut child = child("while :; do echo x; sleep 0.02; done");
        let stdout = child.stdout.take().unwrap();
        let process = Process::new(child);
        let watch = Watchdog::start(
            &process,
            Some(Duration::from_secs(1)),
            Duration::from_millis(150),
        );
        let started = Instant::now();
        assert!(!output(stdout).0.is_empty());
        assert!(watch.stalled.load(Ordering::Relaxed));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn the_deadline_interrupts_a_blocked_input_write() {
        let mut child = child("sleep 30");
        let mut stdin = child.stdin.take().unwrap();
        let process = Process::new(child);
        let _watch = Watchdog::start(&process, None, Duration::from_millis(150));
        let started = Instant::now();
        assert!(stdin.write_all(&vec![b'x'; MOST_LINE]).is_err());
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn output_and_unterminated_lines_are_bounded() {
        assert!(line(&mut std::io::Cursor::new(vec![b'x'; MOST_LINE + 1])).is_err());
        assert!(output(std::io::Cursor::new(vec![b'x'; MOST_OUTPUT + 1])).1);
        assert_eq!(
            line(&mut std::io::Cursor::new("ok\n")).unwrap(),
            Some("ok\n".into())
        );
    }

    #[test]
    fn stopping_a_process_reaps_the_child_without_waiting_for_drop() {
        let child = child("sleep 30");
        let process = Process::new(child);
        process.stop();
        let status = process
            .0
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .try_wait()
            .unwrap();
        assert!(status.is_some());
    }
}
