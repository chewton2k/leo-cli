use std::cell::RefCell;
use std::sync::Arc;

use anyhow::{anyhow, Result};

use leo_services::ai::chat::Jotted;
use leo_services::session::capture::Source;
use leo_services::session::recorder::{self, Controls, Event, Input, Request};
use leo_web::record::{Heard, Listener, Listening};

pub fn listener() -> Listener {
    Arc::new(|listening, heard| listen(listening, heard))
}

fn listen(listening: Listening, heard: &mut dyn FnMut(Heard)) -> Result<(String, String)> {
    let Listening {
        audio,
        screen,
        directory,
        title,
        stop,
        pause,
        points,
    } = listening;
    let controls = Controls {
        stop,
        pause,
        points: Default::default(),
    };
    let input = match audio {
        Some(rx) => Input::Fed(rx),
        None => Input::Device(Source::from_env(screen)),
    };
    let heard = RefCell::new(heard);
    let failed = RefCell::new(None);
    let finished = RefCell::new(None);
    let mirror = || {
        let (Ok(from), Ok(mut to)) = (points.lock(), controls.points.lock()) else {
            return;
        };
        if from.len() != to.len() {
            *to = from
                .iter()
                .map(|(at_secs, text)| Jotted {
                    at_secs: *at_secs,
                    text: text.clone(),
                })
                .collect();
        }
    };
    recorder::record(
        Request {
            title,
            append_to: None,
            dir: directory,
            screen,
            input,
        },
        &controls,
        &|event| {
            mirror();
            let mut tell = heard.borrow_mut();
            match event {
                Event::Started(_) => {}
                Event::Clock { secs, paused } => tell(Heard::Clock { secs, paused }),
                Event::Progress { label, steps } => tell(Heard::Step { label, steps }),
                Event::Transcript(text) => tell(Heard::Transcript(text)),
                Event::Fallback { from, to } => tell(Heard::Warning(format!("{from}: {to}"))),
                Event::Warning(text) => tell(Heard::Warning(text)),
                Event::Failed(text) => *failed.borrow_mut() = Some(text),
                Event::Finished { session, .. } => *finished.borrow_mut() = Some(session),
            }
        },
    );
    let heard = heard.into_inner();
    if let Some(problem) = failed.into_inner() {
        return Err(anyhow!(problem));
    }
    let dir = finished
        .into_inner()
        .ok_or_else(|| anyhow!("The recording ended without any audio."))?;
    heard(Heard::Step {
        label: "Writing the notes".into(),
        steps: None,
    });
    let (tx, rx) = std::sync::mpsc::channel::<(usize, usize)>();
    let written = std::thread::scope(|scope| {
        let work = scope.spawn(move || {
            recorder::write_up(&dir, None, &move |done, total| {
                let _ = tx.send((done, total));
            })
        });
        for (done, total) in rx {
            heard(Heard::Step {
                label: "Writing the notes".into(),
                steps: (total > 1).then_some((done, total)),
            });
        }
        work.join()
            .map_err(|_| anyhow!("leo hit an internal error while writing the notes."))?
    })?;
    for problem in written.problems {
        heard(Heard::Warning(problem));
    }
    Ok((written.title, written.body))
}
