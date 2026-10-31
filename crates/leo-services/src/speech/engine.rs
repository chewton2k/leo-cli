use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{ensure, Context, Result};
use transcribe_cpp::{Backend, RunOptions};

use super::library::Selected;
use super::protocol::DeviceSelector;
use super::supervisor::{EngineSupervisor, LoadSpec};
use super::vad::{
    frames_for_duration_ms, EarshotVad, SmoothedVad, VadFrame, VoiceActivityDetector,
    VAD_OFFLINE_HANGOVER_MS, VAD_ONSET_MS, VAD_PREFILL_MS,
};

const QUEUED: usize = 8;
const IDLE: Duration = Duration::from_secs(60);
const POLL: Duration = Duration::from_millis(20);
const ANSWER_LIMIT: Duration = Duration::from_secs(2 * 60 * 60);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Dictation,
    Segment,
    Live,
}

struct Job {
    _admission: Pin,
    selected: Selected,
    chunks: VecDeque<Vec<f32>>,
    text: Vec<String>,
    priority: Priority,
    reply: mpsc::SyncSender<Result<String>>,
}

enum Message {
    Run(Job),
    Stream {
        selected: Selected,
        audio: mpsc::Receiver<Option<Vec<f32>>>,
        text: Box<dyn FnMut(String) + Send>,
        reply: mpsc::SyncSender<Result<String>>,
    },
    Stop(mpsc::SyncSender<()>),
    Unload(mpsc::SyncSender<()>),
    Edit {
        path: std::path::PathBuf,
        current: bool,
        work: Box<dyn FnOnce() -> Result<serde_json::Value> + Send>,
        reply: mpsc::SyncSender<Result<serde_json::Value>>,
    },
}

pub struct Engine {
    requests: mpsc::SyncSender<Message>,
    supervisor: EngineSupervisor,
    pins: Arc<AtomicUsize>,
    dictating: Arc<AtomicUsize>,
    stopped: AtomicBool,
    admitted: Arc<AtomicUsize>,
}

pub struct Pin(Arc<AtomicUsize>);
impl Drop for Pin {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

pub fn engine() -> &'static Engine {
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    ENGINE.get_or_init(Engine::new)
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    pub fn new() -> Self {
        let (requests, queue) = mpsc::sync_channel(QUEUED);
        let supervisor =
            EngineSupervisor::new(!cfg!(all(target_os = "macos", target_arch = "x86_64")));
        let pins = Arc::new(AtomicUsize::new(0));
        let dictating = Arc::new(AtomicUsize::new(0));
        let (worker, held, first) = (supervisor.clone(), pins.clone(), dictating.clone());
        std::thread::spawn(move || run(queue, worker, held, first));
        Self {
            requests,
            supervisor,
            pins,
            dictating,
            stopped: AtomicBool::new(false),
            admitted: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn pin(&self) -> Pin {
        self.pins.fetch_add(1, Ordering::AcqRel);
        Pin(self.pins.clone())
    }
    pub fn dictation(&self) -> Pin {
        self.dictating.fetch_add(1, Ordering::AcqRel);
        Pin(self.dictating.clone())
    }
    pub fn recording(&self) -> bool {
        self.pins.load(Ordering::Acquire) > 0
    }
    pub fn dictating(&self) -> bool {
        self.dictating.load(Ordering::Acquire) > 0
    }
    pub fn loaded(&self) -> bool {
        self.supervisor.loaded().is_some()
    }
    pub fn cancel(&self) {
        self.supervisor.cancel();
    }

    pub fn transcribe(
        &self,
        selected: Selected,
        pcm: Vec<f32>,
        priority: Priority,
    ) -> Result<String> {
        ensure!(
            !self.stopped.load(Ordering::Acquire),
            "The speech engine is stopping. Reopen Leo to transcribe again."
        );
        #[allow(deprecated)]
        self.admitted
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < QUEUED).then_some(n + 1)
            })
            .map_err(|_| anyhow::anyhow!("The speech engine is busy. Try again in a moment."))?;
        let admission = Pin(self.admitted.clone());
        let chunks = super::prepare(pcm)?.into();
        let (reply, answer) = mpsc::sync_channel(1);
        self.requests.try_send(Message::Run(Job { _admission: admission, selected, chunks, text: Vec::new(), priority, reply }))
            .map_err(|_| anyhow::anyhow!("The speech engine is busy. Your recorded audio is kept; try again in a moment."))?;
        answer.recv_timeout(ANSWER_LIMIT).context(
            "Transcription took too long. Your audio is kept; try a faster speech model.",
        )?
    }

    pub fn stream(
        &self,
        selected: Selected,
        text: impl FnMut(String) + Send + 'static,
    ) -> Result<DictationStream> {
        ensure!(
            !self.stopped.load(Ordering::Acquire),
            "The speech engine is stopping. Reopen Leo."
        );
        let (feed, audio) = mpsc::sync_channel(128);
        let (reply, answer) = mpsc::sync_channel(1);
        self.requests
            .try_send(Message::Stream {
                selected,
                audio,
                text: Box::new(text),
                reply,
            })
            .map_err(|_| anyhow::anyhow!("The speech engine is busy. Try again."))?;
        Ok(DictationStream { feed, answer })
    }

    pub fn edit_model(
        &self,
        path: std::path::PathBuf,
        current: bool,
        work: impl FnOnce() -> Result<serde_json::Value> + Send + 'static,
    ) -> Result<serde_json::Value> {
        let (reply, answer) = mpsc::sync_channel(1);
        self.requests
            .try_send(Message::Edit {
                path,
                current,
                work: Box::new(work),
                reply,
            })
            .map_err(|_| anyhow::anyhow!("The speech engine is busy. Try again in a moment."))?;
        answer
            .recv_timeout(ANSWER_LIMIT)
            .context("The model is still in use. Stop recording or dictation and try again.")?
    }

    pub fn unload_worker(&self) -> Result<()> {
        self.supervisor.cancel();
        let (done, finished) = mpsc::sync_channel(1);
        self.requests.try_send(Message::Unload(done)).map_err(|_|anyhow::anyhow!("The speech engine is busy. Finish recording or dictation, then try updating again."))?;
        finished
            .recv_timeout(Duration::from_secs(5))
            .context("The speech worker did not stop. Try updating again in a moment.")
    }

    pub fn stop(&self) -> Result<()> {
        self.stopped.store(true, Ordering::Release);
        self.supervisor.cancel();
        let (done, stopped) = mpsc::sync_channel(1);
        self.requests.try_send(Message::Stop(done)).map_err(|_| {
            anyhow::anyhow!("The speech engine is still busy. Try again in a moment.")
        })?;
        stopped
            .recv_timeout(Duration::from_secs(5))
            .context("The speech engine has not stopped yet. Try again in a moment.")
    }
}

pub struct DictationStream {
    feed: mpsc::SyncSender<Option<Vec<f32>>>,
    answer: mpsc::Receiver<Result<String>>,
}
impl DictationStream {
    pub fn feed(&self, pcm: Vec<f32>) -> bool {
        self.feed.try_send(Some(pcm)).is_ok()
    }
    pub fn finish(self) -> Result<String> {
        self.feed.try_send(None).map_err(|_| {
            anyhow::anyhow!("Live transcription fell behind. Finishing from the recorded audio.")
        })?;
        self.answer
            .recv_timeout(ANSWER_LIMIT)
            .context("Live transcription did not finish. Try a smaller model.")?
    }
}

fn trim(pcm: Vec<f32>) -> Result<Vec<f32>> {
    let detector = EarshotVad::new(0.5)?;
    let frame = detector.frame_samples();
    let mut vad = SmoothedVad::new(
        Box::new(detector),
        frames_for_duration_ms(VAD_PREFILL_MS, frame),
        frames_for_duration_ms(VAD_OFFLINE_HANGOVER_MS, frame),
        frames_for_duration_ms(VAD_ONSET_MS, frame),
    );
    let mut out = Vec::new();
    for chunk in pcm.chunks(frame) {
        let mut padded = vec![0.0; frame];
        padded[..chunk.len()].copy_from_slice(chunk);
        if let VadFrame::Speech(heard) = vad.push_frame(&padded)? {
            out.extend_from_slice(heard);
        }
    }
    Ok(out)
}

fn run(
    queue: mpsc::Receiver<Message>,
    supervisor: EngineSupervisor,
    pins: Arc<AtomicUsize>,
    dictating: Arc<AtomicUsize>,
) {
    let mut pending: Vec<Job> = Vec::new();
    let mut last_work = Instant::now();
    let mut model = None;
    loop {
        let message = queue.recv_timeout(if pending.is_empty() { IDLE } else { POLL });
        if matches!(message, Err(mpsc::RecvTimeoutError::Disconnected)) {
            supervisor.unload().wait();
            return;
        }
        let mut incoming = message.ok().into_iter().chain(queue.try_iter());
        for message in &mut incoming {
            match message {
                Message::Run(job) => pending.push(job),
                Message::Stream {
                    selected,
                    audio,
                    mut text,
                    reply,
                } => {
                    let result = (|| -> Result<String> {
                        if model.as_ref() != Some(&selected.path) || supervisor.loaded().is_none() {
                            supervisor.load(LoadSpec {
                                path: selected.path.clone(),
                                backend: Backend::Auto,
                                device: DeviceSelector::Auto,
                            })?;
                            model = Some(selected.path.clone());
                        }
                        let stream = supervisor.start_stream(
                            RunOptions {
                                language: (selected.language != "auto")
                                    .then_some(selected.language),
                                ..Default::default()
                            },
                            Default::default(),
                            move |progress| {
                                if let Some(snapshot) = progress.text {
                                    text(snapshot.display());
                                }
                            },
                        )?;
                        let started = Instant::now();
                        let mut samples = 0usize;
                        loop {
                            ensure!(
                                started.elapsed() < Duration::from_secs(310),
                                "Dictation reached five minutes. Start another dictation."
                            );
                            match audio.recv_timeout(Duration::from_millis(100)) {
                                Ok(Some(pcm)) => {
                                    samples = samples.saturating_add(pcm.len());
                                    ensure!(
                                        samples <= 300 * 16_000
                                            && pcm.len() <= 16_000
                                            && pcm.iter().all(|n| n.is_finite()),
                                        "Dictation audio is too long. Start another dictation."
                                    );
                                    ensure!(stream.feed(pcm), "Live transcription fell behind. Finishing from recorded audio.");
                                }
                                Ok(None) => break,
                                Err(mpsc::RecvTimeoutError::Timeout) => {}
                                Err(_) => anyhow::bail!("Dictation cancelled."),
                            }
                        }
                        Ok(stream
                            .finalize(false)?
                            .map(|f| f.text.full)
                            .unwrap_or_default())
                    })();
                    let _ = reply.send(result);
                    last_work = Instant::now();
                }
                Message::Edit {
                    path,
                    current,
                    work,
                    reply,
                } => {
                    if (current || model.as_ref() == Some(&path))
                        && (pins.load(Ordering::Acquire) > 0
                            || dictating.load(Ordering::Acquire) > 0)
                    {
                        let _ = reply.send(Err(anyhow::anyhow!(
                            "Stop recording or dictation before deleting the model in use."
                        )));
                        continue;
                    }
                    if model.as_ref() == Some(&path) {
                        supervisor.unload().wait();
                        model = None;
                    }
                    pending.retain(|job| {
                        if job.selected.path != path { true } else {
                            let _ = job.reply.send(Err(anyhow::anyhow!("Choose a model in Settings → Speech models. Your recorded audio is kept.")));
                            false
                        }
                    });
                    let _ = reply.send(work());
                }
                Message::Unload(done) => {
                    supervisor.unload().wait();
                    model = None;
                    let _ = done.send(());
                }
                Message::Stop(done) => {
                    supervisor.unload().wait();
                    for job in pending {
                        let _ = job.reply.send(Err(anyhow::anyhow!(
                            "Transcription stopped. Your recorded audio is kept."
                        )));
                    }
                    let _ = done.send(());
                    return;
                }
            }
        }
        if pending.is_empty() {
            if pins.load(Ordering::Acquire) == 0 && last_work.elapsed() >= IDLE {
                supervisor.unload().wait();
                model = None;
            }
            continue;
        }
        pending.sort_by_key(|job| job.priority);
        if dictating.load(Ordering::Acquire) > 0 && pending[0].priority != Priority::Dictation {
            continue;
        }
        let mut job = pending.remove(0);
        let result = (|| -> Result<()> {
            if model.as_ref() != Some(&job.selected.path) || supervisor.loaded().is_none() {
                supervisor.load(LoadSpec {
                    path: job.selected.path.clone(),
                    backend: Backend::Auto,
                    device: DeviceSelector::Auto,
                })?;
                model = Some(job.selected.path.clone());
            }
            if let Some(chunk) = job.chunks.pop_front() {
                let pcm = trim(chunk)?;
                if !pcm.is_empty() {
                    let run = RunOptions {
                        language: (job.selected.language != "auto")
                            .then(|| job.selected.language.clone()),
                        ..RunOptions::default()
                    };
                    job.text.push(supervisor.transcribe(pcm, run)?.text);
                }
            }
            Ok(())
        })();
        last_work = Instant::now();
        match result {
            Err(error) => {
                let _ = job.reply.send(Err(error.context("The speech model could not finish. Your recorded audio is kept; try again or choose a smaller model in Settings.")));
            }
            Ok(()) if job.chunks.is_empty() => {
                let refs = job.text.iter().map(String::as_str).collect::<Vec<_>>();
                let _ = job.reply.send(Ok(super::chunking::join_transcripts(&refs)));
            }
            Ok(()) => pending.push(job),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_jobs_are_bounded_even_while_dictation_reserves_the_engine() {
        let engine = Arc::new(Engine::new());
        let _dictating = engine.dictation();
        let (send, answers) = mpsc::channel();
        let mut workers = Vec::new();
        for _ in 0..QUEUED * 2 {
            let engine = engine.clone();
            let send = send.clone();
            workers.push(std::thread::spawn(move || {
                let selected = Selected {
                    id: "test".into(),
                    language: "en".into(),
                    path: "unused".into(),
                    streaming: false,
                    live: true,
                };
                let result = engine.transcribe(selected, Vec::new(), Priority::Segment);
                let _ = send.send(result);
            }));
        }
        drop(send);
        for _ in 0..QUEUED {
            assert!(answers
                .recv_timeout(Duration::from_secs(3))
                .unwrap()
                .is_err());
        }
        assert_eq!(engine.admitted.load(Ordering::Acquire), QUEUED);
        engine.stop().unwrap();
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(engine.admitted.load(Ordering::Acquire), 0);
    }
}

#[cfg(test)]
mod edit_tests {
    use super::*;
    #[test]
    fn a_cold_model_cannot_be_deleted_during_capture() {
        let engine = Engine::new();
        let pin = engine.pin();
        let changed = Arc::new(AtomicBool::new(false));
        let observed = changed.clone();
        assert!(engine
            .edit_model("cold-model".into(), true, move || {
                observed.store(true, Ordering::Release);
                Ok(serde_json::json!({}))
            })
            .is_err());
        assert!(!changed.load(Ordering::Acquire));
        drop(pin);
        engine.stop().unwrap();
    }
}

#[cfg(test)]
mod update_tests {
    #[test]
    fn unloading_for_an_update_keeps_the_engine_usable_if_installation_fails() {
        let engine = super::Engine::new();
        engine.unload_worker().unwrap();
        assert!(!engine.loaded());
        assert_eq!(
            engine
                .edit_model("unused".into(), false, || Ok(
                    serde_json::json!({"alive":true})
                ))
                .unwrap()["alive"],
            true
        );
        engine.stop().unwrap();
    }
}
