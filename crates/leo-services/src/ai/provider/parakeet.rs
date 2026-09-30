use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime};

use sherpa_onnx::{
    OfflineModelConfig, OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig,
};

use crate::ai::error::{ProviderError, ProviderResult};
use crate::ai::provider::audio::{expand_tilde, models_dir, read_wav};
use crate::ai::provider::TranscribeProvider;
use crate::config::provider::ProviderConfig;

pub const MODEL_NAME: &str = "parakeet-tdt-0.6b-v3";
pub const MODEL_DIR: &str = "parakeet-tdt-0.6b-v3-int8";
pub const MODEL_URL: &str = "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/2bda32ec70b097a55adaa07d9a7173915b43cc78";
pub const MODEL_MANIFEST: &str = "encoder.int8.onnx=acfc2b4456377e15d04f0243af540b7fe7c992f8d898d751cf134c3a55fd2247 decoder.int8.onnx=179e50c43d1a9de79c8a24149a2f9bac6eb5981823f2a2ed88d655b24248db4e joiner.int8.onnx=3164c13fc2821009440d20fcb5fdc78bff28b4db2f8d0f0b329101719c0948b3 tokens.txt=d58544679ea4bc6ac563d1f545eb7d474bd6cfa467f0a6e2c1dc1c7d37e3c35d";
pub const MODEL_MB: u32 = 670;

const IDLE: Duration = Duration::from_secs(60);
const RATE: usize = 16_000;
const PIECE: usize = 25 * RATE;
const EARLIEST_CUT: usize = 20 * RATE;
const FRAME: usize = RATE / 10;

pub fn manifest() -> Vec<(String, String)> {
    let text =
        std::env::var("LEO_INSTALL_MODEL_MANIFEST").unwrap_or_else(|_| MODEL_MANIFEST.to_string());
    text.split_whitespace()
        .filter_map(|pair| pair.split_once('='))
        .map(|(name, sha)| (name.to_string(), sha.to_lowercase()))
        .collect()
}

pub fn default_dir() -> PathBuf {
    models_dir().join(MODEL_DIR)
}

pub fn model_dir(cfg: &ProviderConfig) -> PathBuf {
    cfg.model_path
        .as_deref()
        .filter(|p| !p.trim().is_empty())
        .map(expand_tilde)
        .unwrap_or_else(default_dir)
}

pub fn present(dir: &Path) -> bool {
    [
        "encoder.int8.onnx",
        "decoder.int8.onnx",
        "joiner.int8.onnx",
        "tokens.txt",
    ]
    .iter()
    .all(|f| dir.join(f).is_file())
}

pub fn threads() -> i32 {
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2);
    (cores / 2).clamp(1, 4) as i32
}

fn background_priority() {
    #[cfg(target_os = "macos")]
    unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_UTILITY, 0);
        libc::setpriority(libc::PRIO_PROCESS, 0, 10);
    }
    #[cfg(target_os = "linux")]
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS, 0, 10);
    }
}

struct Job {
    dir: PathBuf,
    audio: Vec<f32>,
    reply: Sender<Result<String, String>>,
}

type Checked = (PathBuf, Vec<(u64, SystemTime)>);

fn fingerprint(dir: &Path) -> Option<Vec<(u64, SystemTime)>> {
    manifest()
        .iter()
        .map(|(name, _)| {
            let meta = std::fs::metadata(dir.join(name)).ok()?;
            Some((meta.len(), meta.modified().ok()?))
        })
        .collect()
}

fn verify(dir: &Path) -> Result<(), String> {
    static VERIFIED: OnceLock<Mutex<HashSet<Checked>>> = OnceLock::new();
    let verified = VERIFIED.get_or_init(|| Mutex::new(HashSet::new()));
    let key = (dir.to_path_buf(), fingerprint(dir).unwrap_or_default());
    if verified
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains(&key)
    {
        return Ok(());
    }
    match crate::providers::files_state(dir, &manifest()) {
        crate::providers::ModelState::Ready => {
            verified
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(key);
            Ok(())
        }
        crate::providers::ModelState::Missing => Err(format!(
            "the speech model is incomplete in {}; run `leo update`",
            dir.display()
        )),
        crate::providers::ModelState::Damaged => Err(format!(
            "the speech model in {} is damaged; run `leo update`",
            dir.display()
        )),
    }
}

fn load(dir: &Path) -> Result<OfflineRecognizer, String> {
    verify(dir)?;
    let path = |f: &str| Some(dir.join(f).to_string_lossy().to_string());
    let config = OfflineRecognizerConfig {
        model_config: OfflineModelConfig {
            transducer: OfflineTransducerModelConfig {
                encoder: path("encoder.int8.onnx"),
                decoder: path("decoder.int8.onnx"),
                joiner: path("joiner.int8.onnx"),
            },
            tokens: path("tokens.txt"),
            num_threads: threads(),
            provider: Some("cpu".to_string()),
            model_type: Some("nemo_transducer".to_string()),
            ..Default::default()
        },
        decoding_method: Some("greedy_search".to_string()),
        ..Default::default()
    };
    OfflineRecognizer::create(&config)
        .ok_or_else(|| format!("could not load the speech model in {}", dir.display()))
}

pub fn cuts(audio: &[f32]) -> Vec<usize> {
    let mut cuts = Vec::new();
    let mut start = 0;
    while audio.len() - start > PIECE {
        let quietest = (start + EARLIEST_CUT..start + PIECE - FRAME)
            .step_by(FRAME)
            .min_by(|a, b| {
                let loudness = |at: usize| audio[at..at + FRAME].iter().map(|x| x * x).sum::<f32>();
                loudness(*a).total_cmp(&loudness(*b))
            })
            .unwrap_or(start + PIECE);
        cuts.push(quietest);
        start = quietest;
    }
    cuts
}

fn decode(recognizer: &OfflineRecognizer, audio: &[f32]) -> String {
    let mut bounds = vec![0];
    bounds.extend(cuts(audio));
    bounds.push(audio.len());
    bounds
        .windows(2)
        .map(|w| &audio[w[0]..w[1]])
        .filter_map(|piece| {
            let stream = recognizer.create_stream();
            stream.accept_waveform(RATE as i32, piece);
            recognizer.decode(&stream);
            stream.get_result().map(|r| r.text.trim().to_string())
        })
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn run(jobs: Receiver<Job>) {
    background_priority();
    let mut loaded: Option<(PathBuf, OfflineRecognizer)> = None;
    loop {
        let job = match jobs.recv_timeout(IDLE) {
            Ok(job) => job,
            Err(RecvTimeoutError::Timeout) => {
                loaded = None;
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => return,
        };
        if loaded.as_ref().is_none_or(|(dir, _)| *dir != job.dir) {
            loaded = None;
            match load(&job.dir) {
                Ok(recognizer) => loaded = Some((job.dir.clone(), recognizer)),
                Err(e) => {
                    let _ = job.reply.send(Err(e));
                    continue;
                }
            }
        }
        if let Some((_, recognizer)) = &loaded {
            let _ = job.reply.send(Ok(decode(recognizer, &job.audio)));
        }
    }
}

fn engine() -> Sender<Job> {
    static ENGINE: OnceLock<Mutex<Sender<Job>>> = OnceLock::new();
    ENGINE
        .get_or_init(|| {
            let (tx, rx) = mpsc::channel();
            let _ = std::thread::Builder::new()
                .name("leo-speech".to_string())
                .spawn(move || run(rx));
            Mutex::new(tx)
        })
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

pub fn transcribe_samples(dir: &Path, audio: Vec<f32>) -> Result<String, String> {
    let (reply, answer) = mpsc::channel();
    engine()
        .send(Job {
            dir: dir.to_path_buf(),
            audio,
            reply,
        })
        .map_err(|_| "the speech engine stopped".to_string())?;
    answer
        .recv()
        .map_err(|_| "the speech engine stopped".to_string())?
}

pub struct ParakeetTranscribe {
    name: String,
    dir: PathBuf,
}

impl ParakeetTranscribe {
    pub fn new(name: String, cfg: &ProviderConfig) -> Self {
        ParakeetTranscribe {
            name,
            dir: model_dir(cfg),
        }
    }
}

impl TranscribeProvider for ParakeetTranscribe {
    fn transcribe(&self, audio_path: &Path) -> ProviderResult<String> {
        let bytes = std::fs::read(audio_path)
            .map_err(|e| ProviderError::Fatal(format!("{}: cannot read audio: {e}", self.name)))?;
        let audio =
            read_wav(&bytes).map_err(|e| ProviderError::Fatal(format!("{}: {e}", self.name)))?;
        if audio.len() < RATE / 10 {
            return Ok(String::new());
        }
        transcribe_samples(&self.dir, audio)
            .map_err(|e| ProviderError::Fatal(format!("{}: {e}", self.name)))
    }

    fn max_bytes(&self) -> Option<u64> {
        None
    }

    fn available(&self) -> bool {
        present(&self.dir)
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn unavailable_reason(&self) -> String {
        format!(
            "{}: the speech model is not in {} (run `leo update` to download it)",
            self.name,
            self.dir.display()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_manifest_names_every_file_with_a_checksum() {
        let files = manifest();
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                "encoder.int8.onnx",
                "decoder.int8.onnx",
                "joiner.int8.onnx",
                "tokens.txt"
            ]
        );
        for (_, sha) in &files {
            assert_eq!(sha.len(), 64);
            assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));
        }
        assert!(MODEL_URL.contains("/resolve/2bda32ec"));
    }

    #[test]
    fn it_is_available_only_with_every_file() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = ProviderConfig {
            model_path: Some(dir.path().to_string_lossy().to_string()),
            ..Default::default()
        };
        let p = ParakeetTranscribe::new("parakeet".into(), &cfg);
        assert!(!p.available());
        assert!(p.unavailable_reason().contains("leo update"));
        for f in ["encoder.int8.onnx", "decoder.int8.onnx", "joiner.int8.onnx"] {
            std::fs::write(dir.path().join(f), b"x").unwrap();
        }
        assert!(!p.available());
        std::fs::write(dir.path().join("tokens.txt"), b"x").unwrap();
        assert!(p.available());
        assert_eq!(p.max_bytes(), None);
    }

    #[test]
    fn long_audio_is_cut_at_its_quietest_moment_in_pieces_of_at_most_25_seconds() {
        assert!(cuts(&vec![0.5; 25 * RATE]).is_empty());
        let mut audio = vec![0.5; 70 * RATE];
        let pause = 22 * RATE + RATE / 2;
        for x in &mut audio[pause..pause + FRAME * 2] {
            *x = 0.0;
        }
        let found = cuts(&audio);
        assert_eq!(found[0], pause);
        let mut bounds = vec![0];
        bounds.extend(&found);
        bounds.push(audio.len());
        for w in bounds.windows(2) {
            assert!(w[1] - w[0] <= PIECE, "{found:?}");
            assert!(w[1] > w[0]);
        }
    }

    #[test]
    fn it_uses_at_most_half_the_cores_and_never_more_than_four() {
        let n = threads();
        assert!((1..=4).contains(&n));
        let cores = std::thread::available_parallelism()
            .map(|c| c.get())
            .unwrap_or(2) as i32;
        assert!(n <= cores.max(2) / 2 || n == 1);
    }

    #[test]
    fn a_broken_model_is_an_error_not_a_crash() {
        let dir = tempfile::tempdir().unwrap();
        for f in [
            "encoder.int8.onnx",
            "decoder.int8.onnx",
            "joiner.int8.onnx",
            "tokens.txt",
        ] {
            std::fs::write(dir.path().join(f), b"not a model").unwrap();
        }
        let err = transcribe_samples(dir.path(), vec![0.0; 16_000]).unwrap_err();
        assert!(err.contains("damaged"), "{err}");
        let empty = tempfile::tempdir().unwrap();
        let err = transcribe_samples(empty.path(), vec![0.0; 16_000]).unwrap_err();
        assert!(err.contains("incomplete"), "{err}");
    }

    #[test]
    fn very_short_audio_needs_no_model() {
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("a.wav");
        crate::session::wav::write(&wav, &[0i16; 100]).unwrap();
        let cfg = ProviderConfig {
            model_path: Some(dir.path().to_string_lossy().to_string()),
            ..Default::default()
        };
        assert_eq!(
            ParakeetTranscribe::new("parakeet".into(), &cfg)
                .transcribe(&wav)
                .unwrap(),
            ""
        );
    }

    #[test]
    #[ignore]
    fn a_real_model_transcribes_real_speech() {
        let dir = std::env::var("LEO_TEST_PARAKEET_DIR").expect("LEO_TEST_PARAKEET_DIR");
        let audio = std::env::var("LEO_TEST_SPEECH_AUDIO").expect("LEO_TEST_SPEECH_AUDIO");
        let cfg = ProviderConfig {
            model_path: Some(dir),
            ..Default::default()
        };
        let p = ParakeetTranscribe::new("parakeet".into(), &cfg);
        let started = std::time::Instant::now();
        let first = p.transcribe(Path::new(&audio)).unwrap();
        let first_took = started.elapsed();
        let again = std::time::Instant::now();
        let second = p.transcribe(Path::new(&audio)).unwrap();
        eprintln!(
            "first {first_took:?}, second {:?}: {first}",
            again.elapsed()
        );
        assert_eq!(first, second);
        assert!(first.to_lowercase().contains("lecture"), "{first}");
    }
}
