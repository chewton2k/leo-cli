use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::ai::error::{ProviderError, ProviderResult};
use crate::ai::provider::TranscribeProvider;
use crate::config::provider::ProviderConfig;

pub struct WhisperCppTranscribe {
    name: String,
    bin: Option<String>,
    model_path: PathBuf,
}

pub fn expand_tilde(p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => dirs::home_dir()
            .map(|h| h.join(rest))
            .unwrap_or_else(|| PathBuf::from(p)),
        None => PathBuf::from(p),
    }
}

pub const STARTER: &str = "ggml-base.en.bin";

pub fn models_dir() -> PathBuf {
    match std::env::var_os("LEO_HOME").filter(|v| !v.is_empty()) {
        Some(home) => PathBuf::from(home).join("models"),
        None => expand_tilde("~/.leo/models"),
    }
}

pub fn model_file(cfg: &ProviderConfig) -> PathBuf {
    cfg.model_path
        .as_deref()
        .filter(|p| !p.trim().is_empty())
        .map(expand_tilde)
        .unwrap_or_else(|| models_dir().join(STARTER))
}

type Loaded = (SystemTime, Arc<WhisperContext>);

fn loaded() -> &'static Mutex<HashMap<PathBuf, Loaded>> {
    static LOADED: OnceLock<Mutex<HashMap<PathBuf, Loaded>>> = OnceLock::new();
    LOADED.get_or_init(|| {
        whisper_rs::install_logging_hooks();
        Mutex::new(HashMap::new())
    })
}

fn context_for(path: &Path) -> Result<Arc<WhisperContext>, String> {
    let modified = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut cache = loaded().lock().unwrap_or_else(|e| e.into_inner());
    if let Some((when, ctx)) = cache.get(path) {
        if *when == modified {
            return Ok(ctx.clone());
        }
    }
    let mut params = WhisperContextParameters::default();
    params.use_gpu(false);
    let ctx = WhisperContext::new_with_params(path, params)
        .map_err(|e| format!("could not load {}: {e}", path.display()))?;
    let ctx = Arc::new(ctx);
    cache.insert(path.to_path_buf(), (modified, ctx.clone()));
    Ok(ctx)
}

pub fn read_wav(bytes: &[u8]) -> Result<Vec<f32>, String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a WAV file".to_string());
    }
    let mut at = 12;
    let mut format: Option<(u16, u16, u32, u16)> = None;
    let mut data: Option<&[u8]> = None;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let size = u32::from_le_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]])
            as usize;
        let start = at + 8;
        let end = start.saturating_add(size).min(bytes.len());
        if id == b"fmt " && end - start >= 16 {
            let f = &bytes[start..end];
            format = Some((
                u16::from_le_bytes([f[0], f[1]]),
                u16::from_le_bytes([f[2], f[3]]),
                u32::from_le_bytes([f[4], f[5], f[6], f[7]]),
                u16::from_le_bytes([f[14], f[15]]),
            ));
        } else if id == b"data" {
            data = Some(&bytes[start..end]);
            break;
        }
        at = start.saturating_add(size).saturating_add(size & 1);
    }
    let (encoding, channels, rate, bits) = format.ok_or("the WAV file has no format")?;
    let data = data.ok_or("the WAV file has no audio")?;
    if encoding != 1 || bits != 16 || channels == 0 || rate == 0 {
        return Err(format!(
            "unsupported WAV: encoding {encoding}, {bits}-bit, {channels} channels"
        ));
    }
    let channels = channels as usize;
    let mono: Vec<f32> = data
        .as_chunks::<2>()
        .0
        .chunks_exact(channels)
        .map(|frame| {
            frame
                .iter()
                .map(|b| i16::from_le_bytes(*b) as f32 / 32768.0)
                .sum::<f32>()
                / channels as f32
        })
        .collect();
    Ok(resample(&mono, rate, 16_000))
}

fn resample(samples: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || samples.is_empty() {
        return samples.to_vec();
    }
    let out_len = (samples.len() as u64 * to as u64 / from as u64) as usize;
    let step = from as f64 / to as f64;
    (0..out_len)
        .map(|i| {
            let pos = i as f64 * step;
            let left = pos.floor() as usize;
            let right = (left + 1).min(samples.len() - 1);
            let frac = (pos - left as f64) as f32;
            samples[left] * (1.0 - frac) + samples[right] * frac
        })
        .collect()
}

pub fn language_for(model: &Path) -> &'static str {
    let name = model
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    if name.contains(".en.") || name.ends_with(".en.bin") || name.contains(".en-") {
        "en"
    } else {
        "auto"
    }
}

impl WhisperCppTranscribe {
    pub fn new(name: String, cfg: &ProviderConfig) -> Self {
        WhisperCppTranscribe {
            name,
            bin: cfg.bin.clone().filter(|b| !b.trim().is_empty()),
            model_path: model_file(cfg),
        }
    }

    fn binary_on_path(bin: &str) -> bool {
        if bin.contains('/') {
            return Path::new(bin).is_file();
        }
        let Some(paths) = std::env::var_os("PATH") else {
            return false;
        };
        std::env::split_paths(&paths).any(|dir| {
            dir.join(bin).is_file() || (cfg!(windows) && dir.join(format!("{bin}.exe")).is_file())
        })
    }

    fn built_in(&self, audio_path: &Path) -> ProviderResult<String> {
        let bytes = std::fs::read(audio_path)
            .map_err(|e| ProviderError::Fatal(format!("{}: cannot read audio: {e}", self.name)))?;
        let audio =
            read_wav(&bytes).map_err(|e| ProviderError::Fatal(format!("{}: {e}", self.name)))?;
        if audio.len() < 1_600 {
            return Ok(String::new());
        }
        let ctx = context_for(&self.model_path)
            .map_err(|e| ProviderError::Fatal(format!("{}: {e}", self.name)))?;
        let mut state = ctx
            .create_state()
            .map_err(|e| ProviderError::Retryable(format!("{}: {e}", self.name)))?;
        let mut params = FullParams::new(SamplingStrategy::BeamSearch {
            beam_size: 5,
            patience: -1.0,
        });
        let threads = std::thread::available_parallelism()
            .map(|n| n.get().min(8))
            .unwrap_or(4);
        params.set_n_threads(threads as i32);
        params.set_language(Some(language_for(&self.model_path)));
        params.set_no_timestamps(true);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_special(false);
        params.set_print_timestamps(false);
        state
            .full(params, &audio)
            .map_err(|e| ProviderError::Retryable(format!("{}: {e}", self.name)))?;
        let text: Vec<String> = state
            .as_iter()
            .filter_map(|segment| segment.to_str_lossy().ok().map(|t| t.trim().to_string()))
            .filter(|t| !t.is_empty())
            .collect();
        Ok(text.join(" "))
    }

    fn external(&self, bin: &str, audio_path: &Path) -> ProviderResult<String> {
        let output = Command::new(bin)
            .arg("-m")
            .arg(&self.model_path)
            .arg("-f")
            .arg(audio_path)
            .arg("-nt")
            .arg("-np")
            .output()
            .map_err(|e| {
                ProviderError::Retryable(format!("{}: could not run {bin}: {e}", self.name))
            })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ProviderError::Retryable(format!(
                "{}: {bin} exited with {}: {}",
                self.name,
                output.status,
                stderr.trim()
            )));
        }
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if text.is_empty() {
            return Err(ProviderError::Retryable(format!(
                "{}: produced no output",
                self.name
            )));
        }
        Ok(text)
    }
}

impl TranscribeProvider for WhisperCppTranscribe {
    fn transcribe(&self, audio_path: &Path) -> ProviderResult<String> {
        match &self.bin {
            Some(bin) => self.external(bin, audio_path),
            None => self.built_in(audio_path),
        }
    }

    fn max_bytes(&self) -> Option<u64> {
        None
    }

    fn available(&self) -> bool {
        self.model_path.is_file() && self.bin.as_deref().is_none_or(Self::binary_on_path)
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn unavailable_reason(&self) -> String {
        match &self.bin {
            Some(bin) if !Self::binary_on_path(bin) => {
                format!("{}: `{bin}` not on PATH", self.name)
            }
            _ => format!(
                "{}: no speech model at {} (in /settings, Enter on speech model downloads one)",
                self.name,
                self.model_path.display()
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(channels: u16, rate: u32, samples: &[i16]) -> Vec<u8> {
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + 12 + data.len() as u32).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"LIST");
        out.extend_from_slice(&4u32.to_le_bytes());
        out.extend_from_slice(b"INFO");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * 2 * channels as u32).to_le_bytes());
        out.extend_from_slice(&(2 * channels).to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&data);
        out
    }

    #[test]
    fn a_leo_segment_reads_back_as_samples_between_minus_one_and_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        crate::session::wav::write(&path, &[0, 16384, -32768]).unwrap();
        let samples = read_wav(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(samples, vec![0.0, 0.5, -1.0]);
    }

    #[test]
    fn stereo_is_mixed_down_and_other_rates_are_resampled_after_skipping_other_chunks() {
        let stereo = wav(2, 16_000, &[1000, 3000, -2000, -4000]);
        assert_eq!(
            read_wav(&stereo).unwrap(),
            vec![2000.0 / 32768.0, -3000.0 / 32768.0]
        );
        let fast = wav(1, 32_000, &[0; 3200]);
        assert_eq!(read_wav(&fast).unwrap().len(), 1600);
    }

    #[test]
    fn a_file_that_is_not_pcm_wav_is_refused_plainly() {
        assert!(read_wav(b"hello").is_err());
        let mut float = wav(1, 16_000, &[0; 4]);
        float[20 + 12] = 3;
        assert!(read_wav(&float).unwrap_err().contains("unsupported"));
    }

    #[test]
    fn english_only_models_are_told_the_language_and_others_detect_it() {
        assert_eq!(language_for(Path::new("/m/ggml-base.en.bin")), "en");
        assert_eq!(
            language_for(Path::new("/m/ggml-large-v3-turbo.bin")),
            "auto"
        );
        assert_eq!(language_for(Path::new("/m/ggml-small.en-q5_1.bin")), "en");
    }

    #[test]
    fn the_built_in_engine_needs_only_the_model_file() {
        let model = tempfile::NamedTempFile::new().unwrap();
        let cfg = ProviderConfig {
            model_path: Some(model.path().to_string_lossy().to_string()),
            ..Default::default()
        };
        let provider = WhisperCppTranscribe::new("whisper_cpp".to_string(), &cfg);
        assert!(provider.available());
        assert_eq!(provider.max_bytes(), None);

        let missing = ProviderConfig {
            model_path: Some("/nonexistent/ggml-base.en.bin".to_string()),
            ..Default::default()
        };
        let provider = WhisperCppTranscribe::new("whisper_cpp".to_string(), &missing);
        assert!(!provider.available());
        assert!(provider.unavailable_reason().contains("speech model"));
    }

    #[cfg(unix)]
    #[test]
    fn a_named_binary_is_run_instead_and_must_exist() {
        let model = tempfile::NamedTempFile::new().unwrap();
        let with = |bin: &str| ProviderConfig {
            bin: Some(bin.to_string()),
            model_path: Some(model.path().to_string_lossy().to_string()),
            ..Default::default()
        };
        assert!(WhisperCppTranscribe::new("w".into(), &with("sh")).available());
        let missing = WhisperCppTranscribe::new("w".into(), &with("leo-not-a-real-binary"));
        assert!(!missing.available());
        assert!(missing.unavailable_reason().contains("not on PATH"));
    }

    #[test]
    fn a_broken_model_file_fails_without_crashing() {
        let dir = tempfile::tempdir().unwrap();
        let model = dir.path().join("ggml-broken.bin");
        std::fs::write(&model, b"not a model").unwrap();
        let audio = dir.path().join("a.wav");
        crate::session::wav::write(&audio, &vec![0i16; 16_000]).unwrap();
        let cfg = ProviderConfig {
            model_path: Some(model.to_string_lossy().to_string()),
            ..Default::default()
        };
        let err = WhisperCppTranscribe::new("whisper_cpp".into(), &cfg)
            .transcribe(&audio)
            .unwrap_err();
        assert!(err.to_string().contains("could not load"), "{err}");
    }

    #[test]
    #[ignore]
    fn a_real_model_transcribes_real_speech() {
        let model = std::env::var("LEO_TEST_WHISPER_MODEL").expect("LEO_TEST_WHISPER_MODEL");
        let audio = std::env::var("LEO_TEST_WHISPER_AUDIO").expect("LEO_TEST_WHISPER_AUDIO");
        let cfg = ProviderConfig {
            model_path: Some(model),
            ..Default::default()
        };
        let provider = WhisperCppTranscribe::new("whisper_cpp".into(), &cfg);
        let started = std::time::Instant::now();
        let first = provider.transcribe(Path::new(&audio)).unwrap();
        let first_took = started.elapsed();
        let again = std::time::Instant::now();
        let second = provider.transcribe(Path::new(&audio)).unwrap();
        eprintln!(
            "first {first_took:?}, second {:?}: {first}",
            again.elapsed()
        );
        assert_eq!(first, second);
        assert!(first.to_lowercase().contains("lecture"), "{first}");
    }
}
