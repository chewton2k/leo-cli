pub fn closed() -> bool {
    #[cfg(target_os = "macos")]
    {
        let Ok(mut child) = leo_core::children::quiet_command("ioreg")
            .args(["-r", "-k", "AppleClamshellState", "-d", "4"])
            .stdout(std::process::Stdio::piped())
            .spawn()
        else {
            return false;
        };
        let Some(stdout) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return false;
        };
        let reader = std::thread::spawn(move || {
            use std::io::Read;
            let mut text = String::new();
            let _ = stdout.take(256 * 1024).read_to_string(&mut text);
            text
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        loop {
            if child.try_wait().ok().flatten().is_some() {
                break;
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        reader
            .join()
            .unwrap_or_default()
            .contains("\"AppleClamshellState\" = Yes")
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}
