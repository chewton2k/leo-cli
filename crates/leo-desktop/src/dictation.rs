use anyhow::{ensure, Context, Result};
use leo_services::dictation::{
    activation::{Action, Activation},
    generation::Generation,
    settings::{Popup, Settings, Writing},
    visualizer::Visualizer,
};
use leo_services::speech::{
    engine::{engine, DictationStream, Priority},
    library::library,
};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex, OnceLock,
    },
    time::Instant,
};
use tauri::{AppHandle, Emitter};

const FEED_SAMPLES: usize = 512;
const SHOWN_CHARS: usize = 25_000;

const NOTICE_FOR: std::time::Duration = std::time::Duration::from_secs(4);
enum Finish {
    Done,
    Problem(String),
    Notice(String),
}
enum Event {
    Input(bool, bool),
    Complete(u64),
    Cancel,
    Shutdown(mpsc::SyncSender<()>),
}
pub struct Dictation {
    app: AppHandle,
    settings: Mutex<Settings>,
    path: PathBuf,
    state: Mutex<Value>,
    theme: Mutex<String>,
    problem: Mutex<Option<String>>,
    generation: Generation,
    revision: AtomicU64,
    capturing: Arc<AtomicBool>,
    escape: Arc<AtomicBool>,
    stop: Mutex<Option<Arc<AtomicBool>>>,
    tx: mpsc::SyncSender<Event>,
    shortcut: OnceLock<crate::shortcut::Shortcuts>,
}
impl Dictation {
    pub fn new(app: AppHandle) -> Result<Arc<Self>> {
        let path = leo_core::paths::config_dir()?.join("dictation.json");
        let (settings, problem) = Settings::recover(&path);
        let (tx, rx) = mpsc::sync_channel(32);
        let this = Arc::new(Self {
            app: app.clone(),
            settings: Mutex::new(settings.clone()),
            path,
            state: Mutex::new(json!({"state":"idle"})),
            theme: Mutex::new("system".into()),
            problem: Mutex::new(problem),
            generation: Generation::default(),
            revision: AtomicU64::new(0),
            capturing: Arc::new(AtomicBool::new(false)),
            escape: Arc::new(AtomicBool::new(false)),
            stop: Mutex::new(None),
            tx,
            shortcut: OnceLock::new(),
        });
        let input = this.tx.clone();
        if !Self::test_mode() {
            let keys = crate::shortcut::Shortcuts::start(
                app,
                settings.shortcut,
                this.escape.clone(),
                Arc::new(move |pressed, cancel| {
                    let _ = input.try_send(Event::Input(pressed, cancel));
                }),
            );
            let _ = this.shortcut.set(keys);
        }
        let worker = this.clone();
        std::thread::spawn(move || worker.run(rx));
        Ok(this)
    }
    fn test_mode() -> bool {
        cfg!(debug_assertions) && std::env::var_os("LEO_DICTATION_TEST").is_some()
    }
    fn preferences(&self) -> Settings {
        self.settings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    pub fn popup(&self) -> Value {
        let mut value = self.state.lock().unwrap_or_else(|e| e.into_inner()).clone();
        self.decorate(&mut value);
        value
    }
    fn decorate(&self, value: &mut Value) {
        value["theme"] = json!(self.theme.lock().unwrap_or_else(|e| e.into_inner()).clone());
        let settings = self.preferences();
        value["minimal"] = json!(settings.popup == Popup::Minimal);
        value["top"] = json!(settings.top);
    }
    fn publish(&self, mut view: Value) {
        if self.preferences().popup == Popup::Off {
            return;
        }
        self.decorate(&mut view);
        let _ = self.app.emit_to("dictation", "dictation", view);
    }
    fn stamp(&self, state: &mut Value) -> Value {
        state["rev"] = json!(self.revision.fetch_add(1, Ordering::AcqRel) + 1);
        state.clone()
    }
    fn change(&self, id: u64, apply: impl FnOnce(&mut Value)) -> bool {
        let view = self.generation.apply(id, &self.state, |state| {
            apply(state);
            self.stamp(state)
        });
        let changed = view.is_some();
        if let Some(view) = view {
            self.publish(view);
        }
        changed
    }
    fn update(&self, id: u64, value: Value) -> bool {
        self.change(id, |state| *state = value)
    }
    fn set_escape(&self, active: bool) {
        self.escape.store(active, Ordering::Release);
        if let Some(keys) = self.shortcut.get() {
            keys.wake();
        }
    }
    fn phase(&self, id: u64, phase: &str) {
        self.change(id, |state| state["state"] = json!(phase));
    }
    fn notice(self: &Arc<Self>, id: u64, settings: &Settings, message: &str) {
        if !self.update(id, json!({"state":"notice","message":message})) {
            return;
        }
        self.show(id, settings);
        let this = self.clone();
        std::thread::spawn(move || {
            std::thread::sleep(NOTICE_FOR);
            if this.update(id, json!({"state":"idle"})) {
                crate::overlay::hide(&this.app);
            }
        });
    }
    fn idle(&self) {
        let view = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            *state = json!({"state":"idle"});
            self.stamp(&mut state)
        };
        self.publish(view);
        crate::overlay::hide(&self.app);
    }
    fn show(self: &Arc<Self>, id: u64, settings: &Settings) {
        let this = self.clone();
        crate::overlay::show(
            &self.app,
            settings,
            Arc::new(move || this.generation.current(id)),
        );
    }
    pub fn cancel(&self) {
        self.generation.cancel();
        if let Some(stop) = self.stop.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            stop.store(true, Ordering::Release);
        }
        self.capturing.store(false, Ordering::Release);
        self.set_escape(false);
        engine().cancel();
        self.idle();
        let _ = self.tx.try_send(Event::Cancel);
    }
    pub fn shutdown(&self) {
        self.generation.cancel();
        if let Some(stop) = self.stop.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            stop.store(true, Ordering::Release);
        }
        engine().cancel();
        if let Some(keys) = self.shortcut.get() {
            keys.stop();
        }
        let (done, finished) = mpsc::sync_channel(1);
        let _ = self.tx.try_send(Event::Shutdown(done));
        let _ = finished.recv_timeout(std::time::Duration::from_secs(5));
    }
    fn run(self: Arc<Self>, rx: mpsc::Receiver<Event>) {
        let mut activation = Activation::default();
        let mut busy = None;
        let mut queued = Activation::default();
        let mut queued_wants = false;
        while let Ok(event) = rx.recv() {
            match event {
                Event::Input(pressed, cancel) => {
                    if cancel {
                        self.cancel();
                        continue;
                    }
                    let mode = self.preferences().mode;
                    if busy.is_some() && !self.capturing.load(Ordering::Acquire) {
                        match queued.input(pressed, Instant::now(), mode) {
                            Action::Start => queued_wants = true,
                            Action::Stop => queued_wants = false,
                            Action::None => {}
                        }
                        continue;
                    }
                    match activation.input(pressed, Instant::now(), mode) {
                        Action::Start if busy.is_none() => {
                            busy = Some(self.begin());
                        }
                        Action::Stop => {
                            if let Some(stop) =
                                self.stop.lock().unwrap_or_else(|e| e.into_inner()).as_ref()
                            {
                                stop.store(true, Ordering::Release);
                            }
                            self.capturing.store(false, Ordering::Release);
                            if let Some(id) = busy {
                                self.phase(
                                    id,
                                    if engine().loaded() {
                                        "transcribing"
                                    } else {
                                        "loading"
                                    },
                                );
                            }
                            activation.reset();
                        }
                        _ => {}
                    }
                }
                Event::Cancel => {
                    self.generation.cancel();
                    queued.reset();
                    queued_wants = false;
                    activation.reset();
                    if let Some(stop) = self.stop.lock().unwrap_or_else(|e| e.into_inner()).as_ref()
                    {
                        stop.store(true, Ordering::Release);
                    }
                    self.capturing.store(false, Ordering::Release);
                    self.set_escape(false);
                    engine().cancel();
                    self.idle();
                }
                Event::Complete(id) => {
                    if busy == Some(id) {
                        busy = None;
                        self.capturing.store(false, Ordering::Release);
                        activation.reset();
                        if queued_wants && self.generation.current(id) {
                            queued_wants = false;
                            activation = queued;
                            queued = Activation::default();
                            busy = Some(self.begin());
                        }
                    }
                }
                Event::Shutdown(done) => {
                    let _ = done.send(());
                    break;
                }
            }
        }
    }
    fn begin(self: &Arc<Self>) -> u64 {
        let activity = leo_core::activity::global().begin();
        let id = self.generation.next();
        let settings = self.preferences();
        let stop = Arc::new(AtomicBool::new(false));
        *self.stop.lock().unwrap_or_else(|e| e.into_inner()) = Some(stop.clone());
        self.capturing.store(true, Ordering::Release);
        self.set_escape(true);
        self.update(
            id,
            json!({"state":"listening","seconds":0,"levels":[],"committed":"","tentative":""}),
        );
        self.show(id, &settings);
        let this = self.clone();
        std::thread::spawn(move || {
            let result = activity.and_then(|_activity| this.capture(id, &settings, &stop));
            if this.generation.current(id) {
                this.set_escape(false);
                match result {
                    Ok(Finish::Problem(message)) => {
                        this.update(id, json!({"state":"error","message":message}));
                        this.show(id, &settings);
                    }
                    Ok(Finish::Notice(message)) => this.notice(id, &settings, &message),
                    Ok(Finish::Done) => {
                        this.update(id, json!({"state":"idle"}));
                        crate::overlay::hide(&this.app);
                    }
                    Err(e) => {
                        this.update(id, json!({"state":"error","message":e.to_string()}));
                        this.show(id, &settings);
                    }
                }
            }
            let _ = this.tx.send(Event::Complete(id));
        });
        id
    }
    fn capture(
        self: &Arc<Self>,
        id: u64,
        settings: &Settings,
        stop: &AtomicBool,
    ) -> Result<Finish> {
        let _awake = leo_services::session::awake::Awake::recording();
        let _reservation = engine().dictation();
        let selected = library()?.selected()?;
        crate::audio_feedback::play(settings.sounds, true);
        ensure!(self.generation.current(id), "Dictation cancelled.");
        let named = if crate::clamshell::closed() {
            settings
                .closed_microphone
                .clone()
                .or_else(|| settings.microphone.clone())
        } else {
            settings.microphone.clone()
        };
        if stop.load(Ordering::Acquire) {
            return Ok(Finish::Done);
        }
        let (mut mic, rx) = if let Ok(path) = std::env::var("LEO_FAKE_AUDIO") {
            let pcm = leo_services::session::wav::read(std::path::Path::new(&path))?;
            let (tx, rx) = mpsc::sync_channel(64);
            let flag = self.clone();
            std::thread::spawn(move || {
                for part in pcm.chunks(1600) {
                    if !flag.generation.current(id) || tx.send(part.to_vec()).is_err() {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            });
            (None, rx)
        } else {
            let (mic, rx) = leo_services::session::mic::Mic::open_named(named)?;
            (Some(mic), rx)
        };
        let this = self.clone();
        let mut stream = if selected.streaming {
            engine()
                .stream(selected.clone(), move |committed, tentative| {
                    this.change(id, |state| {
                        state["committed"] = json!(newest(&committed));
                        state["tentative"] = json!(newest(&tentative));
                    });
                })
                .ok()
        } else {
            None
        };
        let mut pending = Vec::new();
        let mut speech = StreamingVad::new()?;
        let mut visualizer = Visualizer::new();
        let pcm = leo_services::dictation::capture::collect(
            &rx,
            stop,
            |chunk, _, seconds| {
                if let Some(levels) = visualizer.feed(chunk) {
                    self.change(id, |state| {
                        state["seconds"] = json!(seconds);
                        state["levels"] = json!(levels);
                    });
                }
                if let Some(live) = stream.as_ref() {
                    pending.extend(speech.push(chunk));
                    if pending.len() >= FEED_SAMPLES {
                        if !feed(live, &pending) {
                            stream = None;
                        }
                        pending.clear();
                    }
                }
            },
            || {
                if let Some(mic) = mic.as_mut() {
                    mic.close();
                }
            },
        );
        self.capturing.store(false, Ordering::Release);
        let pcm = pcm?;
        if let Some(mic) = &mic {
            if let Some(problem) = mic.problem.lock().ok().and_then(|p| p.clone()) {
                anyhow::bail!(problem);
            }
        }
        ensure!(self.generation.current(id), "Dictation cancelled.");
        self.phase(
            id,
            if engine().loaded() {
                "transcribing"
            } else {
                "loading"
            },
        );
        crate::audio_feedback::play(settings.sounds, false);
        let transcript = if let Some(stream) = stream {
            feed(&stream, &pending);
            stream.finish().ok().filter(|text| !text.trim().is_empty())
        } else {
            None
        };
        let text = match transcript {
            Some(text) => text,
            None => engine().transcribe(selected.clone(), pcm, Priority::Dictation)?,
        };
        ensure!(self.generation.current(id), "Dictation cancelled.");
        let language = if selected.language != "auto" {
            Some(selected.language.as_str())
        } else {
            leo_services::speech::catalog::catalog()
                .get(&selected.id)
                .ok()
                .filter(|m| m.languages.len() == 1)
                .and_then(|m| m.languages.first().map(String::as_str))
        };
        ensure!(
            text.len() <= 100_000,
            "This dictation is too long. Try shorter dictations or a smaller model."
        );
        let text = leo_services::dictation::text::clean(&text, settings, language);
        if text.trim().is_empty() {
            return Ok(Finish::Done);
        }
        let mut fallback = None;
        let text = if settings.writing == Writing::Off {
            text
        } else {
            self.phase(id, "writing");
            let this = self.clone();
            match leo_services::dictation::polish::polish(
                &text,
                settings.writing,
                language,
                Arc::new(move || !this.generation.current(id)),
            ) {
                Ok(polished) => polished,
                Err(reason) => {
                    ensure!(self.generation.current(id), "Dictation cancelled.");
                    fallback = Some(reason);
                    text
                }
            }
        };
        ensure!(self.generation.current(id), "Dictation cancelled.");
        leo_web::dictation::remember(&leo_core::paths::data_dir()?, &text)?;
        let output = if settings.trailing_space {
            format!("{text} ")
        } else {
            text
        };
        ensure!(self.generation.current(id), "Dictation cancelled.");
        if Self::test_mode() {
            if let Some(path) = std::env::var_os("LEO_DICTATION_OUTPUT") {
                use std::io::Write;
                let path = PathBuf::from(path);
                let mut file = tempfile::NamedTempFile::new_in(
                    path.parent().context("The test output needs a folder.")?,
                )?;
                file.write_all(output.as_bytes())?;
                file.persist(path).map_err(|e| e.error)?;
                return Ok(Finish::Done);
            }
        }
        let pasted = crate::clipboard::paste(&self.app, &output, settings.enter, {
            let this = self.clone();
            Arc::new(move || this.generation.current(id))
        })
        .map_err(anyhow::Error::msg)?;
        Ok(match (pasted, fallback) {
            (Some(message), _) => Finish::Problem(message),
            (None, Some(reason)) => Finish::Notice(reason.into()),
            (None, None) => Finish::Done,
        })
    }
}
impl leo_web::dictation::DictationApi for Dictation {
    fn view(&self) -> Result<Value> {
        let settings = self.preferences();
        let runtime = self.popup();
        let shortcut = self.shortcut.get().map(|s| s.view());
        let problem = self
            .problem
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let microphones = leo_services::session::mic::microphones();
        Ok(
            json!({"settings":settings,"runtime":runtime,"shortcut":shortcut,"microphones":microphones,"worker_loaded":engine().loaded(),"problem":problem,"writing_ready":leo_services::ai::writing_ready()}),
        )
    }
    fn change(&self, value: Value) -> Result<Value> {
        match value["action"].as_str() {
            Some("test_press") if Self::test_mode() => {
                self.tx.try_send(Event::Input(true, false))?;
            }
            Some("test_release") if Self::test_mode() => {
                self.tx.try_send(Event::Input(false, false))?;
            }
            Some("save") => {
                ensure!(
                    !self.capturing.load(Ordering::Acquire),
                    "Finish dictating before changing its settings."
                );
                let settings: Settings = serde_json::from_value(value["settings"].clone())
                    .context(
                    "These dictation settings could not be read. Check the fields and try again.",
                )?;
                settings.validate()?;
                crate::shortcut::validate(&settings.shortcut).map_err(anyhow::Error::msg)?;
                settings.commit(&self.path, || {
                    let previous = self.preferences();
                    if let Some(shortcut) = self.shortcut.get() {
                        if let Err(error) = shortcut.configure(&settings.shortcut) {
                            let _ = shortcut.configure(&previous.shortcut);
                            return Err(anyhow::anyhow!(error));
                        }
                    }
                    *self.settings.lock().unwrap_or_else(|e| e.into_inner()) = settings.clone();
                    Ok(())
                })?;
                *self.problem.lock().unwrap_or_else(|e| e.into_inner()) = None;
            }
            Some("record_shortcut") => {
                ensure!(
                    !self.capturing.load(Ordering::Acquire),
                    "Finish dictating before recording a shortcut."
                );
                self.shortcut
                    .get()
                    .unwrap()
                    .record()
                    .map_err(anyhow::Error::msg)?;
            }
            Some("resume_shortcut") => self.shortcut.get().unwrap().resume(),
            Some("cancel") => self.cancel(),
            Some("copy") => {
                let text = value["text"]
                    .as_str()
                    .context("Choose a recent dictation.")?;
                ensure!(text.len() <= 100_000, "This dictation is too long to copy.");
                crate::clipboard::copy(&self.app, text).map_err(anyhow::Error::msg)?;
            }
            Some("theme") => {
                let choice = value["theme"].as_str().unwrap_or("system");
                ensure!(
                    ["light", "dark", "system"].contains(&choice),
                    "Choose Light, Dark or Match."
                );
                *self.theme.lock().unwrap_or_else(|e| e.into_inner()) = choice.into();
                let view = self.state.lock().unwrap_or_else(|e| e.into_inner()).clone();
                self.publish(view);
            }
            _ => anyhow::bail!("Choose an action in Dictation settings."),
        }
        Ok(json!({"ok":true}))
    }
}
fn feed(stream: &DictationStream, pcm: &[f32]) -> bool {
    pcm.chunks(16_000).all(|part| stream.feed(part.to_vec()))
}
fn newest(text: &str) -> String {
    let count = text.chars().count();
    text.chars()
        .skip(count.saturating_sub(SHOWN_CHARS))
        .collect()
}
struct StreamingVad {
    vad: leo_services::speech::vad::SmoothedVad,
    pending: Vec<f32>,
    frame: usize,
}
impl StreamingVad {
    fn new() -> Result<Self> {
        use leo_services::speech::vad::*;
        let vad = EarshotVad::new(0.5)?;
        let frame = vad.frame_samples();
        Ok(Self {
            vad: SmoothedVad::new(
                Box::new(vad),
                frames_for_duration_ms(VAD_PREFILL_MS, frame),
                frames_for_duration_ms(VAD_STREAMING_HANGOVER_MS, frame),
                frames_for_duration_ms(VAD_ONSET_MS, frame),
            ),
            pending: Vec::new(),
            frame,
        })
    }
    fn push(&mut self, chunk: &[f32]) -> Vec<f32> {
        use leo_services::speech::vad::{VadFrame, VoiceActivityDetector};
        self.pending.extend_from_slice(chunk);
        let mut out = Vec::new();
        while self.pending.len() >= self.frame {
            let frame: Vec<f32> = self.pending.drain(..self.frame).collect();
            if let Ok(VadFrame::Speech(pcm)) = self.vad.push_frame(&frame) {
                out.extend_from_slice(pcm);
            }
        }
        out
    }
}
