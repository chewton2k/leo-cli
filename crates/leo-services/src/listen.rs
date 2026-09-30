use std::process::{Command, Stdio};

/// The peak amplitude of a WAV, as a fraction of full scale.
///
/// Used to tell "nobody spoke" apart from "the microphone is not being heard" —
/// and to avoid sending silence to a transcriber that answers it with invented
/// text rather than nothing.
pub fn peak_amplitude(path: &std::path::Path) -> Option<f64> {
    // `sox stat` writes its report to stderr, one `Label: value` per line.
    let out = Command::new("sox")
        .arg(path)
        .arg("-n")
        .arg("stat")
        .output()
        .ok()?;
    let report = String::from_utf8_lossy(&out.stderr);
    for line in report.lines() {
        if let Some(rest) = line.trim().strip_prefix("Maximum amplitude:") {
            return rest.trim().parse::<f64>().ok().map(f64::abs);
        }
    }
    None
}

/// Record a short sample and report its peak amplitude, to check the microphone
/// is actually heard.
///
/// On macOS a denied microphone permission is not an error: `rec` succeeds and
/// the samples are all zero. Nothing downstream can tell that from a silent
/// room, so the only way to find out is to look at the numbers.
pub fn microphone_peak(seconds: f64) -> Option<f64> {
    microphone_peak_with(
        std::path::Path::new("rec"),
        seconds,
        std::time::Duration::from_secs(3),
    )
}

fn microphone_peak_with(
    recorder: &std::path::Path,
    seconds: f64,
    grace: std::time::Duration,
) -> Option<f64> {
    let probe = std::env::temp_dir().join(format!("leo-mic-probe-{}.wav", std::process::id()));
    let mut child = Command::new(recorder)
        .args([
            "-q",
            "-r",
            "16000",
            "-c",
            "1",
            "-b",
            "16",
            &probe.to_string_lossy(),
            "trim",
            "0",
            &seconds.to_string(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs_f64(seconds) + grace;
    let ok = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
        }
    };

    let level = if ok { peak_amplitude(&probe) } else { None };
    let _ = std::fs::remove_file(&probe);
    level
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recorder_that_never_finishes_is_stopped_and_reported_as_untested() {
        let tmp = tempfile::tempdir().unwrap();
        let fake = tmp.path().join("rec");
        std::fs::write(&fake, "#!/bin/sh\nexec sleep 60\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let started = std::time::Instant::now();
        let level = microphone_peak_with(&fake, 0.4, std::time::Duration::from_secs(1));
        assert_eq!(level, None);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "waited {:?} for a recorder that never finishes",
            started.elapsed()
        );
    }
    /// The level check has to agree with what sox reports, since the whole
    /// silence defence rests on it.
    #[test]
    fn peak_amplitude_reads_silence_and_sound_apart() {
        // Skip where sox is not installed rather than failing the suite.
        if !crate::health::on_path("sox") {
            return;
        }
        let dir = tempfile::tempdir().unwrap();

        let silent = dir.path().join("silent.wav");
        let ok = std::process::Command::new("sox")
            .args(["-n", "-r", "16000", "-c", "1", "-b", "16"])
            .arg(&silent)
            .args(["trim", "0", "1"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(ok, "could not synthesise a silent wav");
        let level = peak_amplitude(&silent).expect("a level for a silent file");
        assert!(
            crate::ai::live::is_silent(level),
            "synthesised silence measured {level}"
        );

        // A tone stands in for speech: the point is that it is not silence.
        let tone = dir.path().join("tone.wav");
        let ok = std::process::Command::new("sox")
            .args(["-n", "-r", "16000", "-c", "1", "-b", "16"])
            .arg(&tone)
            .args(["synth", "1", "sine", "440"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(ok, "could not synthesise a tone");
        let level = peak_amplitude(&tone).expect("a level for a tone");
        assert!(
            !crate::ai::live::is_silent(level),
            "a tone measured {level}"
        );
    }

    #[test]
    fn a_missing_file_has_no_level_rather_than_a_wrong_one() {
        assert!(peak_amplitude(std::path::Path::new("/nonexistent/nope.wav")).is_none());
    }
}
