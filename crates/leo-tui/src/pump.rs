//! Draining the worker channels: diagnostics, a streaming `:ask`, and the
//! listen task. Each returns whether anything changed, so the loop knows to redraw.

use super::*;

impl App {
    /// Surface anything the layers below queued while they had no terminal.
    /// Returns true when a message arrived, so the caller can redraw.
    pub(super) fn pump_diagnostics(&mut self) -> bool {
        let messages = leo_core::diag::drain();
        let last = messages.into_iter().next_back();
        match last {
            Some(message) => {
                self.say(Kind::Warn, message);
                true
            }
            None => false,
        }
    }

    /// Absorb whatever the worker has sent since the last tick. Returns true
    /// when something changed and a redraw is warranted.
    /// Drain the streaming `:ask` job, if one is running.
    pub(super) fn pump_ask<B: TuiBackend>(&mut self, terminal: &mut Terminal<B>) -> Result<bool> {
        let Some(ask) = self.asking.as_mut() else {
            return Ok(false);
        };

        let events = ask.job.drain();
        if events.is_empty() && !ask.job.is_done() {
            return Ok(false);
        }

        let mut expanded: Option<(String, String, usize)> = None;
        let mut answered: Option<(String, String)> = None;
        let mut failure: Option<String> = None;
        let mut fallbacks: Vec<String> = Vec::new();

        for event in events {
            match event {
                TaskEvent::Started { label } => {
                    ask.progress = view::progress::Progress::spinner(label);
                    ask.since = Instant::now();
                }
                TaskEvent::Streaming(text) => ask.text = text,
                TaskEvent::Expanded { note, body, count } => expanded = Some((note, body, count)),
                TaskEvent::Answered { question, text } => answered = Some((question, text)),
                TaskEvent::ProviderFallback { from, to } => {
                    fallbacks.push(format!("{from} → {to}"))
                }
                TaskEvent::Failed(e) => failure = Some(e),
                _ => {}
            }
        }

        for note in fallbacks {
            self.say(Kind::Warn, note);
        }

        if let Some(e) = failure {
            self.asking = None;
            self.say(Kind::Bad, e);
            return Ok(true);
        }

        if let Some((question, text)) = answered {
            self.asking = None;
            self.answer = Some((question, text));
            return Ok(true);
        }

        if let Some((note, body, count)) = expanded {
            self.asking = None;
            if count == 0 {
                self.say(Kind::Dim, "Nothing could be expanded.");
                return Ok(true);
            }
            // Written through the ordinary handler, with the answer already in
            // hand, so saving and the message are identical to the CLI path.
            let answered = PreExpanded {
                body: body.clone(),
                count,
            };
            let outcome = action::apply(
                Action::Ask { note },
                &mut self.store,
                Ctx {
                    current_dir: &self.current_dir,
                    numbering: &self.numbering,
                    selected: None,
                    marked: &[],
                },
                &answered,
            )?;
            self.absorb(outcome, terminal)?;
            return Ok(true);
        }

        Ok(true)
    }

    pub(super) fn pump_tasks<B: TuiBackend>(&mut self, terminal: &mut Terminal<B>) -> Result<bool> {
        if self.pump_ask(terminal)? {
            return Ok(true);
        }
        let Some(rec) = self.recording.as_mut() else {
            return Ok(false);
        };

        let events = rec.job.drain();
        if events.is_empty() && !rec.job.is_done() {
            return Ok(false);
        }

        let mut finished: Option<String> = None;
        let mut structured: Option<(Option<String>, String)> = None;
        let mut failure: Option<String> = None;
        let mut fallbacks: Vec<String> = Vec::new();

        for event in events {
            match event {
                TaskEvent::Started { label } => {
                    rec.progress = view::progress::Progress::spinner(label);
                    rec.since = Instant::now();
                }
                TaskEvent::Progress { label, steps } => {
                    // Restart the clock when the kind of work changes, so the
                    // elapsed time answers "how long has this step taken".
                    if rec.progress.label != label {
                        rec.since = Instant::now();
                    }
                    rec.progress = match steps {
                        Some((done, total)) => view::progress::Progress::steps(label, done, total),
                        None => view::progress::Progress::spinner(label),
                    };
                }
                TaskEvent::Transcript(text) => rec.transcript = text,
                TaskEvent::ProviderFallback { from, to } => {
                    fallbacks.push(format!("{from} unavailable, using {to}"))
                }
                TaskEvent::Finished { transcript } => finished = Some(transcript),
                TaskEvent::Structured { title, body } => structured = Some((title, body)),
                TaskEvent::Failed(e) => failure = Some(e),
                // Other jobs' events; not this one's business.
                TaskEvent::Streaming(_)
                | TaskEvent::Expanded { .. }
                | TaskEvent::Answered { .. }
                | TaskEvent::Pushed => {}
            }
        }

        for f in fallbacks {
            self.say(Kind::Warn, f);
        }

        if let Some(e) = failure {
            self.recording = None;
            self.say(Kind::Bad, e);
            return Ok(true);
        }

        // The recording is done; structuring is another request, so it runs on
        // its own thread and the UI keeps animating.
        if let Some(transcript) = finished {
            let rec = self.recording.take().expect("checked above");
            if transcript.trim().is_empty() {
                if rec.jotted.is_empty() {
                    self.say(Kind::Dim, "No speech detected.");
                    return Ok(true);
                }
                // Nothing was heard, but what was typed is still a note.
                let ready = ReadyNote {
                    title: Some(rec.req.title.clone().unwrap_or_else(|| "Notes".to_string())),
                    body: leo_services::ai::chat::points_as_markdown(&rec.jotted),
                };
                match action::apply_transcript(&mut self.store, &rec.req, "ready", &ready) {
                    Ok(outcome) => self.absorb(outcome, terminal)?,
                    Err(e) => self.say(Kind::Bad, e.to_string()),
                }
                return Ok(true);
            }
            let existing = rec
                .req
                .append_to
                .as_deref()
                .and_then(|target| self.store.find_by_index_or_prefix(target))
                .map(|n| n.body.clone());
            let length = rec.recorded().as_secs();
            self.recording = Some(Recording {
                job: task::start_structuring(transcript, existing, rec.jotted.clone(), length),
                progress: view::progress::Progress::spinner("Structuring notes"),
                since: Instant::now(),
                ..rec
            });
            return Ok(true);
        }

        // Structuring finished: write the note here, on the thread that owns the
        // store.
        if let Some((title, body)) = structured {
            let rec = self.recording.take().expect("checked above");
            let ready = ReadyNote { title, body };
            match action::apply_transcript(&mut self.store, &rec.req, "ready", &ready) {
                Ok(outcome) => self.absorb(outcome, terminal)?,
                Err(e) => self.say(Kind::Bad, e.to_string()),
            }
            return Ok(true);
        }

        // The worker ended without a terminal event.
        if self
            .recording
            .as_ref()
            .map(|r| r.job.is_done())
            .unwrap_or(false)
        {
            self.recording = None;
            self.say(Kind::Warn, "Recording ended unexpectedly.");
        }
        Ok(true)
    }
}
