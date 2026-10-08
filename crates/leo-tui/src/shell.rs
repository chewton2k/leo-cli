//! Terminal-bound work that [`leo_core::action`] handlers describe but cannot do.
//!
//! An [`Effect`] names something needing the user's terminal: spawning
//! `$EDITOR`, asking for confirmation, driving the microphone. The handlers
//! stay pure so they can be tested; these performers do the impure part. Both
//! the line shell and the CLI subcommands use them, so `leo edit 1` and
//! `leo> edit 1` cannot drift apart.

use anyhow::Result;
use colored::Colorize;

use leo_core::action::{
    self, Ai, ConfirmedAction, EditRequest, EditTarget, Kind, Line, ListenRequest, Outcome,
};
use leo_core::store::Store;
use leo_services::session::capture::{Capture, Source};
use leo_services::session::transcriber::{Policy, Transcriber, Update};
use leo_services::session::{self, Manifest, SegmentState, Session};

/// Style one output line. The only place [`Kind`] becomes color.
pub fn render(lines: &[Line]) {
    for line in lines {
        match line.kind {
            Kind::Blank => println!(),
            Kind::Plain => println!("  {}", line.text),
            Kind::Dim => println!("  {}", line.text.dimmed()),
            Kind::Good => println!("  {}", line.text.green()),
            Kind::Warn => println!("  {}", line.text.yellow()),
            Kind::Bad => println!("  {}", line.text.red()),
            Kind::Dir => println!("    {}", line.text.cyan().bold()),
        }
    }
}

/// Spawn `$EDITOR` on the request's temp file, then feed the result back
/// through [`action::apply_edit`].
pub fn run_editor(store: &mut Store, req: EditRequest, ai: &dyn Ai) -> Result<Outcome> {
    std::fs::write(&req.path, &req.seed)?;

    let status = leo_core::editor::open(&req.path)?;

    if !status.success() {
        let _ = std::fs::remove_file(&req.path);
        return Ok(Outcome::line(Line::bad("Editor exited with an error.")));
    }

    let raw = std::fs::read_to_string(&req.path)?;
    let _ = std::fs::remove_file(&req.path);

    // Expanding prompts blocks on the model, so say so first.
    if matches!(req.target, EditTarget::Existing { .. }) {
        let count = action::parse_frontmatter(&raw)
            .2
            .lines()
            .filter(|l| action::is_leo_prompt(l).is_some())
            .count();
        if count > 0 {
            println!(
                "  {}",
                format!("Expanding {count} prompt{}...", plural(count)).cyan()
            );
        }
    }

    action::apply_edit(store, &req.target, &raw, ai)
}

/// Ask before destroying anything. `assume_yes` is how `--force` skips it.
pub fn confirm(
    store: &mut Store,
    prompt: &str,
    on_yes: ConfirmedAction,
    assume_yes: bool,
) -> Result<Outcome> {
    if !assume_yes {
        use std::io::Write;
        print!("  {} (y/n): ", prompt.bold());
        std::io::stdout().flush()?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            return Ok(Outcome::line(Line::dim("Cancelled.")));
        }
    }
    action::apply_confirmed(store, &on_yes)
}

pub struct Prepared {
    pub title: Option<String>,
    pub body: String,
}

impl Ai for Prepared {
    fn expand_prompts(&self, body: &str, _title: &str) -> Result<(String, usize)> {
        Ok((body.to_string(), 0))
    }

    fn structure(&self, _transcript: &str) -> Result<(String, String)> {
        Ok((
            self.title
                .clone()
                .unwrap_or_else(|| "Recording".to_string()),
            self.body.clone(),
        ))
    }

    fn structure_append(&self, _transcript: &str, _existing: &str) -> Result<String> {
        Ok(self.body.clone())
    }
}

fn say(text: &str) {
    use std::io::Write;
    print!("\r\x1b[2K  {text}");
    let _ = std::io::stdout().flush();
}

fn wait_for_transcripts(
    session: &Session,
    transcriber: Transcriber,
    updates: &std::sync::mpsc::Receiver<Update>,
) {
    transcriber.recording_ended();
    while !transcriber.is_finished() {
        for update in updates.try_iter() {
            if let Update::Retrying {
                error,
                wait,
                attempt,
                ..
            } = update
            {
                if attempt >= 2 {
                    println!();
                    println!(
                        "  {}",
                        format!("Transcription is retrying ({error}); next try in {}s. Nothing is lost.", wait.as_secs()).yellow()
                    );
                }
            }
        }
        let segments = session.segments();
        let done = segments
            .iter()
            .filter(|s| matches!(s.state, SegmentState::Done(_) | SegmentState::Failed(_)))
            .count();
        say(&format!(
            "{} {done}/{}",
            "Transcribing".cyan(),
            segments.len()
        ));
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    transcriber.wait();
    println!();
}

fn save_session(store: &mut Store, dir: &std::path::Path) -> Result<Outcome> {
    let session = Session::open(dir)?;
    let assembled = session.assemble();
    let points = session.manifest.jotted();
    let req = ListenRequest {
        screen: session.manifest.screen,
        title: session.manifest.title.clone(),
        append_to: session.manifest.append_to.clone(),
        dir: session.manifest.dir.clone(),
    };
    if assembled.is_silent() && points.is_empty() {
        let _ = std::fs::remove_dir_all(dir);
        return Ok(Outcome::line(Line::dim("No speech detected.")));
    }
    let existing = req
        .append_to
        .as_deref()
        .and_then(|t| store.find_by_index_or_prefix(t))
        .map(|n| n.body.clone());
    let fallback = session
        .manifest
        .started
        .with_timezone(&chrono::Local)
        .format("Recording, %b %-d %-I:%M %p")
        .to_string();
    let structured = leo_services::ai::long::structure_recording(
        &assembled.parts,
        &points,
        existing.as_deref(),
        &fallback,
        &|prompt, max| leo_services::ai::chat_outcome(prompt, max).map(|o| o.value),
        &|done, total| say(&format!("{} {done}/{total}", "Writing the notes".cyan())),
    );
    println!();
    for problem in &structured.problems {
        println!("  {}", problem.yellow());
    }
    let notice = session::failure_notice(&assembled.failed, dir);
    let body = if notice.is_empty() {
        structured.body
    } else {
        format!("{notice}\n\n{}", structured.body)
    };
    let prepared = Prepared {
        title: existing.is_none().then_some(structured.title),
        body,
    };
    let outcome = action::apply_transcript(store, &req, "ready", &prepared)?;
    session.finish()?;
    Ok(outcome)
}

fn finish_interrupted(store: &mut Store, root: &std::path::Path) -> Result<()> {
    for dir in Session::unfinished(root) {
        let Ok(session) = Session::open(&dir) else {
            continue;
        };
        println!(
            "  {}",
            format!(
                "Finishing a recording from {} that was interrupted...",
                session
                    .manifest
                    .started
                    .with_timezone(&chrono::Local)
                    .format("%b %-d %-I:%M %p")
            )
            .cyan()
        );
        let lock = session.lock()?;
        session.recover_parts()?;
        let (tx, rx) = std::sync::mpsc::channel();
        let transcriber = Transcriber::start(
            &dir,
            session::recorder::transcribe_segment(),
            Policy {
                workers: leo_services::ai::parallel_transcriptions(),
                ..Policy::default()
            },
            false,
            tx,
        );
        wait_for_transcripts(&session, transcriber, &rx);
        drop(lock);
        let outcome = save_session(store, &dir)?;
        render(&outcome.lines);
    }
    Ok(())
}

/// Record, transcribe, and structure into a note.
pub fn record_and_apply(store: &mut Store, req: ListenRequest, _ai: &dyn Ai) -> Result<Outcome> {
    leo_services::session::mic::warm_up();
    let root = session::root()?;
    finish_interrupted(store, &root)?;

    let session = Session::create(
        &root,
        Manifest::new(
            req.title.clone(),
            req.append_to.clone(),
            &req.dir,
            req.screen,
        ),
    )?;
    let lock = session.lock()?;
    if !matches!(Source::from_env(req.screen), Source::Replay { .. }) {
        println!("  Opening the microphone…");
    }
    let capture = match Capture::start(
        &session.dir,
        0,
        session.manifest.segment_secs,
        Source::from_env(req.screen),
    ) {
        Ok(c) => c,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&session.dir);
            return Err(e);
        }
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let transcriber = Transcriber::start(
        &session.dir,
        session::recorder::transcribe_segment(),
        Policy {
            workers: leo_services::ai::parallel_transcriptions(),
            ..Policy::default()
        },
        true,
        tx,
    );

    let entered = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let entered = std::sync::Arc::clone(&entered);
        std::thread::spawn(move || {
            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line);
            entered.store(true, std::sync::atomic::Ordering::Relaxed);
        });
    }
    let label = if req.screen {
        "Recording screen"
    } else {
        "Recording"
    };
    while !entered.load(std::sync::atomic::Ordering::Relaxed) && !capture.ended() {
        let secs = capture.recorded_secs() as u64;
        let waiting = session::transcriber::waiting(&session.dir).len();
        let behind = if waiting > 1 {
            format!(" ({waiting} parts waiting to be transcribed)")
        } else {
            String::new()
        };
        say(&format!(
            "{} {}{}  {}",
            label.cyan().bold(),
            leo_services::ai::chat::clock(secs).cyan().bold(),
            behind.dimmed(),
            "press Enter to stop".dimmed()
        ));
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    println!();
    if let Some(problem) = capture.problem() {
        println!(
            "  {}",
            format!("{problem} What was recorded is being saved.").yellow()
        );
    }
    capture.stop()?;
    let mut session = session;
    session.manifest.stopped = true;
    session.save()?;
    wait_for_transcripts(&session, transcriber, &rx);
    drop(lock);
    let dir = session.dir.clone();
    save_session(store, &dir)
}

pub fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}
