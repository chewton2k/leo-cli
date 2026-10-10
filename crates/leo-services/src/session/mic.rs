use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use super::wav::RATE;

pub const DEFAULT_SCREEN_DEVICE: &str = "BlackHole 2ch";

pub fn disabled() -> bool {
    std::env::var_os("LEO_NO_MICROPHONE").is_some_and(|v| !v.is_empty() && v != "0")
}

pub fn screen_device() -> Option<String> {
    std::env::var("LEO_SCREEN_DEVICE")
        .ok()
        .filter(|name| !name.trim().is_empty())
}

pub fn at_least(version: &str, wanted: (u32, u32)) -> bool {
    let mut parts = version.trim().split('.').map(|p| p.parse::<u32>().ok());
    match (parts.next().flatten(), parts.next().flatten().or(Some(0))) {
        (Some(major), Some(minor)) => (major, minor) >= wanted,
        _ => false,
    }
}

#[cfg(target_os = "macos")]
fn system_version() -> Option<String> {
    let name = std::ffi::CString::new("kern.osproductversion").ok()?;
    let mut buffer = [0u8; 32];
    let mut size = buffer.len();
    let status = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            buffer.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if status != 0 {
        return None;
    }
    let text = &buffer[..size.min(buffer.len())];
    let end = text.iter().position(|b| *b == 0).unwrap_or(text.len());
    String::from_utf8(text[..end].to_vec()).ok()
}

pub fn records_what_plays() -> bool {
    #[cfg(target_os = "macos")]
    {
        system_version().is_some_and(|v| at_least(&v, (14, 6)))
    }
    #[cfg(not(target_os = "macos"))]
    {
        cfg!(windows)
    }
}

fn named(host: &cpal::Host, wanted: &str) -> Option<cpal::Device> {
    let wanted = wanted.to_lowercase();
    host.input_devices()
        .ok()?
        .chain(host.output_devices().ok()?)
        .find(|d| device_name(d).to_lowercase().contains(&wanted))
}

fn device_name(device: &cpal::Device) -> String {
    device
        .description()
        .map(|d| d.name().to_string())
        .unwrap_or_default()
}

fn find_device(screen: bool) -> Result<cpal::Device> {
    let host = cpal::default_host();
    if !screen {
        return host.default_input_device().ok_or_else(|| {
            anyhow!("No microphone found. Plug one in, or check your sound settings.")
        });
    }
    if let Some(wanted) = screen_device() {
        return named(&host, &wanted).ok_or_else(|| {
            anyhow!("No audio device named '{wanted}' (LEO_SCREEN_DEVICE). Unset it to record what the computer plays.")
        });
    }
    if records_what_plays() {
        let output = host.default_output_device().ok_or_else(|| {
            anyhow!("No speakers or headphones found to record what the computer plays.")
        })?;
        if cfg!(windows) || !output.supports_input() {
            return Ok(output);
        }
        if let Some(device) = named(&host, DEFAULT_SCREEN_DEVICE) {
            return Ok(device);
        }
        bail!(
            "{} is a microphone and speakers in one, so leo cannot record only what it plays.\n  \
             Switch the sound output to other speakers or headphones, then try again.",
            device_name(&output)
        );
    }
    if let Some(device) = named(&host, DEFAULT_SCREEN_DEVICE) {
        return Ok(device);
    }
    if cfg!(target_os = "macos") {
        bail!(
            "Recording what the computer plays needs macOS 14.6 or newer.\n  \
             On this Mac, set up BlackHole instead:\n  \
             1. brew install blackhole-2ch\n  \
             2. Audio MIDI Setup → New Multi-Output Device (Speakers + BlackHole 2ch)\n  \
             3. Set that Multi-Output Device as System Output"
        );
    }
    bail!(
        "Recording what the computer plays is not built in here yet.\n  \
         Set LEO_SCREEN_DEVICE to a monitor device (see: pactl list sources short)."
    )
}

pub fn warm_up() {
    if disabled() {
        return;
    }
    static STARTED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    STARTED.get_or_init(|| {
        let _ = std::thread::Builder::new()
            .name("leo-audio-warm-up".into())
            .spawn(|| {
                let _ = cpal::default_host().default_input_device();
            });
    });
}

pub fn microphone_name() -> Option<String> {
    if disabled() {
        return None;
    }
    cpal::default_host()
        .default_input_device()
        .map(|d| device_name(&d))
}

pub struct Resampler {
    step: f64,
    cutoff: f64,
    half: usize,
    buffer: Vec<f32>,
    at: f64,
}

impl Resampler {
    pub fn new(from: u32, to: u32) -> Resampler {
        let step = from as f64 / to as f64;
        let half = (16.0 * step.max(1.0)).ceil() as usize;
        Resampler {
            step,
            cutoff: (to as f64 / from as f64).min(1.0) * 0.95,
            half,
            buffer: vec![0.0; half],
            at: half as f64,
        }
    }

    fn kernel(&self, x: f64) -> f64 {
        let width = self.half as f64;
        if x.abs() >= width {
            return 0.0;
        }
        let sinc = if x == 0.0 {
            1.0
        } else {
            let a = std::f64::consts::PI * self.cutoff * x;
            a.sin() / a
        };
        let window = 0.5 + 0.5 * (std::f64::consts::PI * x / width).cos();
        self.cutoff * sinc * window
    }

    pub fn push(&mut self, input: &[f32], out: &mut Vec<i16>) {
        if (self.step - 1.0).abs() < f64::EPSILON {
            out.extend(input.iter().map(|x| to_i16(*x)));
            return;
        }
        self.buffer.extend_from_slice(input);
        while self.at + (self.half as f64) < self.buffer.len() as f64 {
            let centre = self.at.floor() as isize;
            let mut sum = 0.0;
            for k in (centre - self.half as isize + 1)..=(centre + self.half as isize) {
                if k < 0 {
                    continue;
                }
                sum += self.buffer[k as usize] as f64 * self.kernel(self.at - k as f64);
            }
            out.push(to_i16(sum as f32));
            self.at += self.step;
        }
        let keep_from = (self.at.floor() as usize).saturating_sub(self.half);
        if keep_from > 0 {
            self.buffer.drain(..keep_from);
            self.at -= keep_from as f64;
        }
    }
}

fn to_i16(x: f32) -> i16 {
    (x.clamp(-1.0, 1.0) * 32767.0).round() as i16
}

fn mono<T: Copy>(data: &[T], channels: usize, convert: impl Fn(T) -> f32) -> Vec<f32> {
    let channels = channels.max(1);
    data.chunks(channels)
        .map(|frame| frame.iter().map(|s| convert(*s)).sum::<f32>() / frame.len() as f32)
        .collect()
}

pub fn fatal(kind: cpal::ErrorKind) -> bool {
    !matches!(
        kind,
        cpal::ErrorKind::DeviceChanged | cpal::ErrorKind::Xrun | cpal::ErrorKind::RealtimeDenied
    )
}

fn build(
    device: &cpal::Device,
    raw: SyncSender<Vec<f32>>,
    problem: Arc<Mutex<Option<String>>>,
    dropped: Arc<AtomicU64>,
) -> Result<(cpal::Stream, u32)> {
    let config = if device.supports_input() {
        device.default_input_config()
    } else {
        device.default_output_config()
    }
    .map_err(|e| anyhow!("the microphone cannot record: {e}"))?;
    let rate = config.sample_rate();
    let channels = config.channels() as usize;
    let format = config.sample_format();
    let stream_config: cpal::StreamConfig = config.into();
    let on_error = move |e: cpal::Error| {
        if !fatal(e.kind()) {
            return;
        }
        if let Ok(mut p) = problem.lock() {
            p.get_or_insert_with(|| format!("the audio device stopped: {e}"));
        }
    };
    let send = move |samples: Vec<f32>| match raw.try_send(samples) {
        Ok(()) | Err(TrySendError::Disconnected(_)) => {}
        Err(TrySendError::Full(samples)) => {
            dropped.fetch_add(
                samples.len() as u64 * RATE as u64 / rate as u64,
                Ordering::Relaxed,
            );
        }
    };
    let stream = match format {
        cpal::SampleFormat::F32 => device.build_input_stream(
            stream_config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| send(mono(data, channels, |s| s)),
            on_error,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            stream_config,
            move |data: &[i16], _: &cpal::InputCallbackInfo| {
                send(mono(data, channels, |s| s as f32 / 32768.0))
            },
            on_error,
            None,
        ),
        cpal::SampleFormat::I32 => device.build_input_stream(
            stream_config,
            move |data: &[i32], _: &cpal::InputCallbackInfo| {
                send(mono(data, channels, |s| s as f32 / 2_147_483_648.0))
            },
            on_error,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_input_stream(
            stream_config,
            move |data: &[u16], _: &cpal::InputCallbackInfo| {
                send(mono(data, channels, |s| (s as f32 - 32768.0) / 32768.0))
            },
            on_error,
            None,
        ),
        other => bail!("the microphone uses an audio format leo cannot read ({other})"),
    }
    .map_err(|e| anyhow!("could not open the microphone: {e}"))?;
    stream
        .play()
        .map_err(|e| anyhow!("could not start the microphone: {e}"))?;
    Ok((stream, rate))
}

pub struct Mic {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    pub problem: Arc<Mutex<Option<String>>>,
    pub dropped: Arc<AtomicU64>,
}

impl Mic {
    pub fn open(screen: bool) -> Result<(Mic, Receiver<Vec<i16>>)> {
        if disabled() {
            bail!("the microphone is turned off for leo (LEO_NO_MICROPHONE is set)");
        }
        let (out_tx, out_rx) = mpsc::channel::<Vec<i16>>();
        let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<()>>(1);
        let stop = Arc::new(AtomicBool::new(false));
        let problem = Arc::new(Mutex::new(None));
        let dropped = Arc::new(AtomicU64::new(0));
        let thread = {
            let (stop, problem, dropped) = (
                Arc::clone(&stop),
                Arc::clone(&problem),
                Arc::clone(&dropped),
            );
            std::thread::Builder::new()
                .name("leo-mic".into())
                .spawn(move || {
                    let (raw_tx, raw_rx) = mpsc::sync_channel::<Vec<f32>>(4096);
                    let opened = find_device(screen).and_then(|device| {
                        build(&device, raw_tx, Arc::clone(&problem), Arc::clone(&dropped))
                    });
                    let (stream, rate) = match opened {
                        Ok(opened) => {
                            let _ = ready_tx.send(Ok(()));
                            opened
                        }
                        Err(e) => {
                            let _ = ready_tx.send(Err(e));
                            return;
                        }
                    };
                    let mut resampler = Resampler::new(rate, RATE);
                    let mut converted = Vec::new();
                    let mut seen_dropped = 0;
                    while !stop.load(Ordering::Relaxed) {
                        match raw_rx.recv_timeout(Duration::from_millis(100)) {
                            Ok(chunk) => {
                                converted.clear();
                                let lost = dropped.load(Ordering::Relaxed);
                                if lost > seen_dropped {
                                    converted.resize(
                                        (lost - seen_dropped).min(RATE as u64 * 60) as usize,
                                        0,
                                    );
                                    seen_dropped = lost;
                                }
                                resampler.push(&chunk, &mut converted);
                                if !converted.is_empty() && out_tx.send(converted.clone()).is_err()
                                {
                                    break;
                                }
                            }
                            Err(RecvTimeoutError::Timeout) => {}
                            Err(RecvTimeoutError::Disconnected) => break,
                        }
                    }
                    drop(stream);
                })?
        };
        match ready_rx.recv_timeout(Duration::from_secs(30)) {
            Ok(Ok(())) => Ok((
                Mic {
                    stop,
                    thread: Some(thread),
                    problem,
                    dropped,
                },
                out_rx,
            )),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                stop.store(true, Ordering::Relaxed);
                bail!("the microphone did not start within 30 seconds")
            }
        }
    }

    pub fn close(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Mic {
    fn drop(&mut self) {
        self.close();
    }
}

pub fn listen_for(seconds: f64) -> Result<Vec<i16>> {
    let (mut mic, rx) = Mic::open(false)?;
    let wanted = (seconds * RATE as f64) as usize;
    let deadline =
        std::time::Instant::now() + Duration::from_secs_f64(seconds) + Duration::from_secs(3);
    let mut heard = Vec::with_capacity(wanted);
    while heard.len() < wanted && std::time::Instant::now() < deadline {
        if let Ok(chunk) = rx.recv_timeout(Duration::from_millis(100)) {
            heard.extend(chunk);
        }
    }
    mic.close();
    if let Some(p) = mic.problem.lock().ok().and_then(|p| p.clone()) {
        bail!(p);
    }
    heard.truncate(wanted);
    Ok(heard)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_audio_needs_macos_14_6() {
        assert!(at_least("14.6", (14, 6)));
        assert!(at_least("14.6.1", (14, 6)));
        assert!(at_least("15", (14, 6)));
        assert!(at_least("27.2", (14, 6)));
        assert!(!at_least("14.5", (14, 6)));
        assert!(!at_least("13.7.2", (14, 6)));
        assert!(!at_least("", (14, 6)));
        assert!(!at_least("unknown", (14, 6)));
    }

    fn tone(rate: u32, hz: f64, secs: f64) -> Vec<f32> {
        (0..(rate as f64 * secs) as usize)
            .map(|i| (2.0 * std::f64::consts::PI * hz * i as f64 / rate as f64).sin() as f32 * 0.5)
            .collect()
    }

    fn level(samples: &[i16]) -> f64 {
        let tail = &samples[samples.len() / 4..];
        (tail.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / tail.len() as f64).sqrt() / 32768.0
    }

    #[test]
    fn speech_frequencies_survive_resampling_from_48_khz() {
        let mut r = Resampler::new(48_000, 16_000);
        let mut out = Vec::new();
        for chunk in tone(48_000, 440.0, 1.0).chunks(480) {
            r.push(chunk, &mut out);
        }
        assert!(
            (out.len() as i64 - 16_000).abs() < 64,
            "{} samples",
            out.len()
        );
        let rms = level(&out);
        assert!((rms - 0.5 / 2f64.sqrt()).abs() < 0.03, "rms {rms}");
    }

    #[test]
    fn sound_above_what_16_khz_can_hold_is_filtered_rather_than_folded_back() {
        let mut r = Resampler::new(48_000, 16_000);
        let mut out = Vec::new();
        r.push(&tone(48_000, 12_000.0, 1.0), &mut out);
        assert!(
            level(&out) < 0.02,
            "a 12 kHz tone leaked through at {}",
            level(&out)
        );
    }

    #[test]
    fn uneven_rates_and_tiny_chunks_keep_time() {
        let mut r = Resampler::new(44_100, 16_000);
        let mut out = Vec::new();
        for chunk in tone(44_100, 300.0, 2.0).chunks(7) {
            r.push(chunk, &mut out);
        }
        assert!(
            (out.len() as i64 - 32_000).abs() < 64,
            "{} samples",
            out.len()
        );
        assert!(level(&out) > 0.3);
    }

    #[test]
    fn the_same_rate_passes_straight_through() {
        let mut r = Resampler::new(16_000, 16_000);
        let mut out = Vec::new();
        r.push(&[0.0, 0.5, -1.0, 2.0], &mut out);
        assert_eq!(out, vec![0, 16384, -32767, 32767]);
    }

    #[test]
    fn hiccups_do_not_stop_a_recording_but_a_lost_device_does() {
        assert!(!fatal(cpal::ErrorKind::Xrun));
        assert!(!fatal(cpal::ErrorKind::DeviceChanged));
        assert!(!fatal(cpal::ErrorKind::RealtimeDenied));
        assert!(fatal(cpal::ErrorKind::DeviceNotAvailable));
    }

    #[test]
    #[ignore]
    fn the_screen_device_is_what_the_computer_plays() {
        let device = find_device(true).unwrap();
        eprintln!(
            "screen device: {} (input: {}, built in: {})",
            device_name(&device),
            device.supports_input(),
            records_what_plays()
        );
        if records_what_plays() && screen_device().is_none() {
            assert!(!device.supports_input());
        }
    }

    #[test]
    #[ignore]
    fn what_the_computer_plays_is_heard() {
        let (mut mic, samples) = Mic::open(true).unwrap();
        let until = std::time::Instant::now() + Duration::from_secs(5);
        let mut heard: Vec<i16> = Vec::new();
        while std::time::Instant::now() < until {
            if let Ok(chunk) = samples.recv_timeout(Duration::from_millis(200)) {
                heard.extend(chunk);
            }
        }
        mic.close();
        let peak = heard.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
        let rms = (heard.iter().map(|s| (*s as f64).powi(2)).sum::<f64>()
            / heard.len().max(1) as f64)
            .sqrt();
        eprintln!(
            "heard {:.1} s, peak {peak}, rms {rms:.0}, problem {:?}",
            heard.len() as f64 / RATE as f64,
            mic.problem.lock().unwrap()
        );
        assert!(heard.len() as f64 > RATE as f64 * 4.0);
        assert!(peak > 300, "only silence came through");
    }

    #[test]
    #[ignore]
    fn a_real_microphone_is_heard() {
        let started = std::time::Instant::now();
        let heard = listen_for(3.0).unwrap();
        let peak = crate::session::wav::peak(&heard);
        eprintln!(
            "device {:?}: {} samples ({:.2} s of audio) in {:?}, peak {peak:.3}",
            microphone_name(),
            heard.len(),
            heard.len() as f64 / RATE as f64,
            started.elapsed()
        );
        assert!(heard.len() >= (2.9 * RATE as f64) as usize);
    }

    #[test]
    #[ignore]
    fn real_speech_at_48_khz_converts_cleanly() {
        let input = std::env::var("LEO_TEST_SPEECH_48K").expect("LEO_TEST_SPEECH_48K");
        let output = std::env::var("LEO_TEST_SPEECH_OUT").expect("LEO_TEST_SPEECH_OUT");
        let bytes = std::fs::read(&input).unwrap();
        let at = bytes.windows(4).position(|w| w == b"data").unwrap() + 8;
        let samples: Vec<f32> = bytes[at..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| i16::from_le_bytes(*b) as f32 / 32768.0)
            .collect();
        let mut r = Resampler::new(48_000, 16_000);
        let mut out = Vec::new();
        for chunk in samples.chunks(470) {
            r.push(chunk, &mut out);
        }
        crate::session::wav::write(std::path::Path::new(&output), &out).unwrap();
        eprintln!(
            "{} samples at 48 kHz -> {} at 16 kHz",
            samples.len(),
            out.len()
        );
    }

    #[test]
    fn channels_are_averaged_into_one() {
        assert_eq!(mono(&[0.2f32, 0.4, -1.0, 1.0], 2, |s| s), vec![0.3f32, 0.0]);
        assert_eq!(mono(&[16384i16], 1, |s| s as f32 / 32768.0), vec![0.5]);
    }
}
