use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{bail, Context, Result};
use colored::Colorize;

/// Record audio from microphone or system audio device.
/// Returns path to the recorded WAV file.
/// Requires `sox` to be installed (provides the `rec` command).
/// For screen audio, also requires BlackHole: `brew install blackhole-2ch`.
pub fn record_audio(screen: bool) -> Result<PathBuf> {
    // Check if sox/rec is available
    if Command::new("rec")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_err()
    {
        bail!(
            "Audio recording requires SoX. Install it:\n  \
             macOS:   brew install sox\n  \
             Linux:   sudo apt install sox\n  \
             Windows: choco install sox"
        );
    }

    let tmp_path = std::env::temp_dir().join("leo-recording.wav");

    // Remove stale recording if it exists
    let _ = std::fs::remove_file(&tmp_path);

    // Resolve audio device for screen mode
    let device = if screen {
        Some(std::env::var("LEO_SCREEN_DEVICE").unwrap_or_else(|_| "BlackHole 2ch".to_string()))
    } else {
        None
    };

    let path_str = tmp_path.to_str().context("temp path is not valid UTF-8")?;

    // Build rec args: <output> rate 16000 channels 1
    // Device selection uses AUDIODEV env var (not -d flag, which means --default-device on macOS)
    let rec_args = [path_str, "rate", "16000", "channels", "1"];

    // Start recording in background
    let mut cmd = Command::new("rec");
    if let Some(ref dev) = device {
        cmd.env("AUDIODEV", dev);
    }
    let mut child = cmd
        .args(rec_args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| {
            if let Some(ref dev) = device {
                format!(
                    "Failed to start recording from screen audio device '{dev}'.\n  \
                     Set up system audio capture:\n  \
                     1. brew install blackhole-2ch\n  \
                     2. Open Audio MIDI Setup → New Multi-Output Device (Speakers + BlackHole 2ch)\n  \
                     3. Set that Multi-Output Device as System Output in Sound Settings\n  \
                     To use a different device: add LEO_SCREEN_DEVICE=<name> to your .env"
                )
            } else {
                "Failed to start recording".to_string()
            }
        })?;

    // Live stopwatch display
    let label = if screen {
        "Recording screen"
    } else {
        "Recording"
    };
    let running = Arc::new(AtomicBool::new(true));
    let running_clone = Arc::clone(&running);
    let start = Instant::now();

    let stopwatch = std::thread::spawn(move || {
        while running_clone.load(Ordering::Relaxed) {
            let elapsed = start.elapsed().as_secs();
            let mins = elapsed / 60;
            let secs = elapsed % 60;
            print!(
                "\r  {} {} {}",
                label.cyan().bold(),
                format!("{:02}:{:02}", mins, secs).cyan().bold(),
                "press Enter to stop".dimmed()
            );
            io::stdout().flush().ok();
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    });

    // Wait for user to press Enter
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;

    // Stop stopwatch and capture final elapsed time
    running.store(false, Ordering::Relaxed);
    let elapsed = start.elapsed().as_secs();
    stopwatch.join().ok();

    // Stop recording
    child.kill().ok();
    child.wait().ok();

    // Repair WAV header: `rec` is killed before it can write the final DataSize field,
    // leaving it as 0. sox --ignore-length reads to EOF and writes a correct header.
    let fixed = std::env::temp_dir().join("leo-recording-fixed.wav");
    let repaired = Command::new("sox")
        .args([
            "--ignore-length",
            tmp_path.to_str().unwrap(),
            fixed.to_str().unwrap(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success() && fixed.exists())
        .unwrap_or(false);
    if repaired {
        let _ = std::fs::rename(&fixed, &tmp_path);
    }

    if !tmp_path.exists() || std::fs::metadata(&tmp_path)?.len() == 0 {
        if let Some(ref dev) = device {
            bail!(
                "Recording failed — no audio captured from screen audio device '{dev}'.\n  \
                 Check your setup:\n  \
                 1. brew install blackhole-2ch\n  \
                 2. Open Audio MIDI Setup → New Multi-Output Device (Speakers + BlackHole 2ch)\n  \
                 3. Set that Multi-Output Device as System Output in Sound Settings\n  \
                 To use a different device: add LEO_SCREEN_DEVICE=<name> to your .env"
            );
        }
        bail!("Recording failed — no audio captured.");
    }

    let size = std::fs::metadata(&tmp_path)?.len();
    // Get actual duration from WAV header via sox; fall back to byte-rate estimate
    let file_secs = Command::new("sox")
        .args(["--i", "-D", tmp_path.to_str().unwrap()])
        .output()
        .ok()
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .parse::<f64>()
                .ok()
        })
        .map(|d| d as u64)
        .unwrap_or_else(|| size / (16000 * 2));
    let file_mins = file_secs / 60;
    let duration = if file_mins > 0 {
        format!("~{}m{}s", file_mins, file_secs % 60)
    } else {
        format!("~{}s", file_secs)
    };

    // Overwrite stopwatch line with final summary
    let e_mins = elapsed / 60;
    let e_secs = elapsed % 60;
    print!("\r\x1b[2K\x1b[1A\x1b[2K");
    println!(
        "  {} {} ({}, {:.1}MB)",
        "Recorded".green(),
        format!("{:02}:{:02}", e_mins, e_secs).dimmed(),
        duration,
        size as f64 / (1024.0 * 1024.0)
    );

    Ok(tmp_path)
}

/// Where the in-progress recording is written. One fixed path, since only one
/// recording can run at a time.
pub fn recording_path() -> PathBuf {
    std::env::temp_dir().join("leo-recording.wav")
}

/// Check that SoX is installed, with the install command in the error.
fn require_sox() -> Result<()> {
    if Command::new("rec")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_err()
    {
        bail!(
            "Audio recording requires SoX. Install it:\n  \
             macOS:   brew install sox\n  \
             Linux:   sudo apt install sox\n  \
             Windows: choco install sox"
        );
    }
    Ok(())
}

/// A recording in progress, owned by whoever started it.
///
/// The blocking [`record_audio`] waits for Enter; this exists for the TUI,
/// where recording has to run alongside a live event loop and the same growing
/// file has to be readable for rolling transcription.
pub struct Recorder {
    /// `None` when the audio is being replayed from a file instead of captured.
    child: Option<std::process::Child>,
    /// Set to stop a replay thread.
    replaying: Option<Arc<AtomicBool>>,
    path: PathBuf,
    started: Instant,
}

impl Recorder {
    /// Start `rec` writing a 16kHz mono WAV, and return immediately.
    pub fn start(screen: bool) -> Result<Recorder> {
        require_sox()?;

        let path = recording_path();
        let _ = std::fs::remove_file(&path);

        // A microphone cannot be scripted, so live transcription had no way to
        // be verified end to end — and a mic that macOS has silenced looks
        // identical to a quiet room. `LEO_FAKE_AUDIO=<wav>` replays a file at
        // real-time pace into the same growing WAV the rolling loop reads, so
        // the whole path can be exercised without speaking.
        if let Ok(source) = std::env::var("LEO_FAKE_AUDIO") {
            return Recorder::replay(std::path::Path::new(&source), path);
        }

        let device = if screen {
            Some(std::env::var("LEO_SCREEN_DEVICE").unwrap_or_else(|_| "BlackHole 2ch".to_string()))
        } else {
            None
        };

        let path_str = path.to_str().context("temp path is not valid UTF-8")?;
        let mut cmd = Command::new("rec");
        if let Some(dev) = &device {
            cmd.env("AUDIODEV", dev);
        }
        let child = cmd
            .args([path_str, "rate", "16000", "channels", "1"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| match &device {
                Some(dev) => format!(
                    "Failed to start recording from screen audio device '{dev}'.\n  \
                     Set up system audio capture:\n  \
                     1. brew install blackhole-2ch\n  \
                     2. Audio MIDI Setup → New Multi-Output Device (Speakers + BlackHole 2ch)\n  \
                     3. Set that Multi-Output Device as System Output\n  \
                     To use a different device: set LEO_SCREEN_DEVICE=<name>"
                ),
                None => "Failed to start recording".to_string(),
            })?;

        Ok(Recorder {
            child: Some(child),
            replaying: None,
            path,
            started: Instant::now(),
        })
    }

    /// Replay a WAV into `dest` at real-time pace, imitating `rec`.
    ///
    /// Writes a header claiming zero length, exactly as `rec` does while still
    /// recording, so the reader's header repair is exercised too rather than
    /// bypassed.
    pub(crate) fn replay(source: &std::path::Path, dest: PathBuf) -> Result<Recorder> {
        let audio = std::fs::read(source)
            .with_context(|| format!("could not read {}", source.display()))?;
        let data_at = find_data_chunk(&audio)
            .with_context(|| format!("{} is not a WAV file", source.display()))?;

        // Header with DataSize left at zero.
        let mut header = audio[..data_at].to_vec();
        let len = header.len();
        header[len - 4..].copy_from_slice(&0u32.to_le_bytes());
        if len >= 8 {
            header[4..8].copy_from_slice(&0u32.to_le_bytes());
        }
        std::fs::write(&dest, &header)?;

        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let target = dest.clone();
        std::thread::spawn(move || {
            use std::io::Write;
            // 16 kHz, mono, 16-bit — the format leo records in.
            const BYTES_PER_SEC: usize = 32_000;
            const STEP: usize = BYTES_PER_SEC / 10;
            let body = &audio[data_at..];
            let mut written = 0;
            while written < body.len() && !worker_stop.load(Ordering::Relaxed) {
                let end = (written + STEP).min(body.len());
                if let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(&target) {
                    let _ = file.write_all(&body[written..end]);
                }
                written = end;
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        });

        Ok(Recorder {
            child: None,
            replaying: Some(stop),
            path: dest,
            started: Instant::now(),
        })
    }

    pub fn elapsed(&self) -> std::time::Duration {
        self.started.elapsed()
    }

    /// The file being written. Safe to read while recording continues, which is
    /// what the rolling transcription loop does.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Stop recording and finalize the file. Returns the finished WAV.
    pub fn stop(mut self) -> Result<PathBuf> {
        if let Some(child) = self.child.as_mut() {
            child.kill().ok();
            child.wait().ok();
        }
        if let Some(stop) = &self.replaying {
            stop.store(true, Ordering::Relaxed);
        }
        repair_wav_header(&self.path);

        if !self.path.exists() || std::fs::metadata(&self.path)?.len() < 100 {
            bail!("Recording failed — no audio captured.");
        }
        Ok(self.path)
    }
}

/// A stretch of a recording the user paused: when it started, and when it
/// ended — `None` if the recording stopped while still paused.
pub type Pause = (f64, Option<f64>);

/// sox `trim` positions that keep the recording and drop the paused stretches.
/// trim copies until the first position after 0, then alternates discarding
/// and copying at each one after that.
fn trim_positions(pauses: &[Pause]) -> Vec<String> {
    let mut out = vec!["0".to_string()];
    for (start, end) in pauses {
        out.push(format!("={start:.3}"));
        if let Some(end) = end {
            out.push(format!("={end:.3}"));
        }
    }
    out
}

/// Remove the paused stretches from a finished recording, so what was said
/// while paused is never transcribed or sent anywhere. Returns the file to
/// transcribe: the original when nothing was paused.
pub fn cut_pauses(path: &std::path::Path, pauses: &[Pause]) -> Result<PathBuf> {
    if pauses.is_empty() {
        return Ok(path.to_path_buf());
    }
    let out = path.with_extension("kept.wav");
    let status = Command::new("sox")
        .arg(path)
        .arg(&out)
        .arg("trim")
        .args(trim_positions(pauses))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("could not run sox to remove the paused parts")?;
    if !status.success() || !out.exists() {
        bail!("sox could not remove the paused parts of the recording");
    }
    let _ = std::fs::remove_file(path);
    Ok(out)
}

/// A WAV's length in seconds, from sox.
pub fn wav_seconds(path: &std::path::Path) -> Option<f64> {
    let out = Command::new("sox")
        .arg("--i")
        .arg("-D")
        .arg(path)
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

/// `rec` is killed before it can write the final DataSize field, leaving it
/// zero. `sox --ignore-length` reads to EOF and writes a correct header.
///
/// Used for the finished file and, during live transcription, for a copy of the
/// growing one — a slice cut from a header that claims zero length yields
/// nothing.
/// Byte offset of the start of a WAV's sample data.
fn find_data_chunk(bytes: &[u8]) -> Option<usize> {
    // Walk the chunk list rather than assuming a 44-byte header: `say` and sox
    // both emit files with extra chunks before `data`.
    let mut i = 12;
    while i + 8 <= bytes.len() {
        let id = &bytes[i..i + 4];
        let size = u32::from_le_bytes(bytes[i + 4..i + 8].try_into().ok()?) as usize;
        if id == b"data" {
            return Some(i + 8);
        }
        i += 8 + size + (size % 2);
    }
    None
}

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
    let probe = std::env::temp_dir().join(format!("leo-mic-probe-{}.wav", std::process::id()));
    let ok = Command::new("rec")
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
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    let level = if ok { peak_amplitude(&probe) } else { None };
    let _ = std::fs::remove_file(&probe);
    level
}

pub fn repair_wav_header(path: &std::path::Path) {
    let fixed = path.with_extension("fixed.wav");
    let ok = Command::new("sox")
        .args([
            "--ignore-length",
            &path.to_string_lossy(),
            &fixed.to_string_lossy(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success() && fixed.exists())
        .unwrap_or(false);
    if ok {
        let _ = std::fs::rename(&fixed, path);
    } else {
        let _ = std::fs::remove_file(&fixed);
    }
}

#[cfg(test)]
mod tests {
    /// The replay hook must produce a *growing* WAV that the rolling loop can
    /// read, or live transcription has no way to be tested at all.
    #[test]
    fn the_replay_hook_grows_a_readable_wav() {
        if !crate::health::on_path("sox") {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("tone.wav");
        let ok = std::process::Command::new("sox")
            .args(["-n", "-r", "16000", "-c", "1", "-b", "16"])
            .arg(&source)
            .args(["synth", "3", "sine", "440"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(ok, "could not synthesise a source");

        let dest = dir.path().join("growing.wav");
        let recorder = Recorder::replay(&source, dest.clone()).expect("replay started");

        let first = std::fs::metadata(&dest).unwrap().len();
        std::thread::sleep(std::time::Duration::from_millis(600));
        let second = std::fs::metadata(&dest).unwrap().len();
        assert!(second > first, "the file did not grow: {first} -> {second}");

        // And a header-repaired copy reports a real duration, which is what the
        // rolling loop depends on.
        let snap = dir.path().join("snap.wav");
        std::fs::copy(&dest, &snap).unwrap();
        repair_wav_header(&snap);
        let level = peak_amplitude(&snap).expect("a level");
        assert!(
            !crate::ai::live::is_silent(level),
            "replayed audio read silent"
        );

        let finished = recorder.stop().expect("a finished file");
        assert!(finished.exists());
    }

    // ── pausing ─────────────────────────────────────────────────────────────

    /// sox's trim alternates between copying and discarding at each position,
    /// so the positions are the pause boundaries after a leading 0.
    #[test]
    fn paused_stretches_become_trim_positions() {
        assert_eq!(
            trim_positions(&[(10.0, Some(20.5))]),
            vec!["0", "=10.000", "=20.500"]
        );
        // A pause still open when recording stopped discards to the end.
        assert_eq!(
            trim_positions(&[(10.0, Some(20.0)), (30.0, None)]),
            vec!["0", "=10.000", "=20.000", "=30.000"]
        );
    }

    #[test]
    fn cutting_the_paused_stretches_out_of_a_recording() {
        if !crate::health::on_path("sox") {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("rec.wav");
        let ok = std::process::Command::new("sox")
            .args(["-n", "-r", "16000", "-c", "1", "-b", "16"])
            .arg(&wav)
            .args(["synth", "3", "sine", "440"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(ok);

        let cut = cut_pauses(&wav, &[(1.0, Some(2.0))]).unwrap();
        let secs = wav_seconds(&cut).expect("a length");
        assert!((secs - 2.0).abs() < 0.05, "expected about 2s, got {secs}");

        // Nothing paused: the recording is used as it is.
        assert_eq!(cut_pauses(&wav, &[]).unwrap(), wav);
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

    use super::*;

    #[test]
    fn record_audio_accepts_screen_bool() {
        // Compile-time proof the signature is correct.
        let _f: fn(bool) -> anyhow::Result<std::path::PathBuf> = record_audio;
    }

    #[test]
    fn the_recorder_reports_a_path_before_any_audio_arrives() {
        // Nothing here starts `rec`; this pins the naming contract the live
        // transcription worker relies on.
        let path = recording_path();
        assert!(path.to_string_lossy().ends_with(".wav"));
        assert!(path.to_string_lossy().contains("leo-recording"));
    }
}
