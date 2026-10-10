use std::cell::RefCell;
use std::sync::Arc;

use anyhow::{anyhow, Result};

use leo_services::ai::chat::Jotted;
use leo_services::session::capture::Source;
use leo_services::session::recorder::{self, Controls, Event, Input, Request};
use leo_web::record::{Heard, Listener, Listening, Recorded};

pub fn listener() -> Listener {
    Arc::new(|listening, heard| listen(listening, heard))
}

fn listen(listening: Listening, heard: &mut dyn FnMut(Heard)) -> Result<Recorded> {
    if listening.call {
        return listen_call(listening, heard);
    }
    let Listening {
        id,
        profile,
        other_audio: _,
        call: _,
        finish_now,
        audio,
        screen,
        directory,
        title,
        stop,
        pause,
        points,
    } = listening;
    let controls = Controls {
        finish_now,
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
            id: Some(id),
            profile: Some(profile),
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
                Event::Clock {
                    secs,
                    paused,
                    level,
                } => tell(Heard::Clock {
                    secs,
                    paused,
                    level,
                }),
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
    let work_dir = dir.clone();
    let written = std::thread::scope(|scope| {
        let work = scope.spawn(move || {
            recorder::write_up(&work_dir, None, &move |done, total| {
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
    let source = leo_services::session::Session::open(&dir)?.archive();
    Ok(Recorded {
        title: written.title,
        body: written.body,
        source: Some(source),
        commit: Some(Arc::new(move |notes, note| {
            leo_services::session::Session::open(&dir)?.commit(notes, note)
        })),
    })
}

pub fn regenerator() -> leo_web::record::Regenerator {
    Arc::new(|sources, profile, workflows| {
        let written = write_sources(sources, &profile, &workflows);
        if !written.problems.is_empty() {
            anyhow::bail!(written.problems.join("; "));
        }
        Ok((written.title, written.body))
    })
}

fn write_sources(
    sources: Vec<leo_core::recording::Archive>,
    profile: &leo_core::workflows::Profile,
    workflows: &leo_core::workflows::Workflows,
) -> leo_services::ai::long::Structured {
    let speakers: std::collections::HashSet<&str> = sources
        .iter()
        .flat_map(|s| s.passages.iter().map(|p| p.speaker.as_str()))
        .collect();
    let named = speakers.len() > 1;
    let mut parts = Vec::new();
    let mut points = Vec::new();
    let mut offset = 0;
    for source in sources {
        let end = source
            .passages
            .iter()
            .map(|p| p.end_secs)
            .max()
            .unwrap_or(0);
        for p in source.passages {
            parts.push(leo_services::session::Part {
                index: parts.len() as u32,
                start_secs: offset + p.start_secs,
                end_secs: offset + p.end_secs,
                text: if named && !p.speaker.is_empty() {
                    format!("[{}] {}", p.speaker, p.text)
                } else {
                    p.text
                },
            });
        }
        for p in source.points {
            points.push(Jotted {
                at_secs: offset + p.at_secs,
                text: p.text,
            });
        }
        offset += end;
    }
    leo_services::ai::long::structure_recording(
        &parts,
        &points,
        None,
        "Recording",
        &|prompt, max| {
            leo_services::ai::chat_outcome(
                leo_services::ai::long::with_profile(prompt, profile, workflows),
                max,
            )
            .map(|o| o.value)
        },
        &|_, _| {},
        leo_services::ai::writing_budget(),
    )
}

fn listen_call(listening: Listening, heard: &mut dyn FnMut(Heard)) -> Result<Recorded> {
    let Listening {
        id,
        profile,
        audio,
        other_audio,
        directory,
        title,
        stop,
        pause,
        finish_now,
        points,
        ..
    } = listening;
    let tracks = [("You", audio), ("Others", other_audio)];
    let (tx, rx) = std::sync::mpsc::channel();
    let mut dirs = Vec::new();
    let mut warnings = Vec::new();
    std::thread::scope(|scope| {
        for (speaker, audio) in tracks {
            let Some(audio) = audio else {
                continue;
            };
            let tx = tx.clone();
            let id = id.clone();
            let profile = profile.clone();
            let title = title.clone();
            let directory = directory.clone();
            let controls = Controls {
                finish_now: finish_now.clone(),
                stop: stop.clone(),
                pause: pause.clone(),
                points: Default::default(),
            };
            let points = points.clone();
            scope.spawn(move || {
                recorder::record(
                    Request {
                        id: Some(format!("{id}-{}", speaker.to_lowercase())),
                        profile: Some(profile),
                        title,
                        append_to: None,
                        dir: directory,
                        screen: speaker == "Others",
                        input: Input::Fed(audio),
                    },
                    &controls,
                    &|event| {
                        if speaker == "You" {
                            if let (Ok(from), Ok(mut to)) = (points.lock(), controls.points.lock())
                            {
                                *to = from
                                    .iter()
                                    .map(|(at_secs, text)| Jotted {
                                        at_secs: *at_secs,
                                        text: text.clone(),
                                    })
                                    .collect();
                            }
                        }
                        let _ = tx.send((speaker, event));
                    },
                );
            });
        }
        drop(tx);
        for (speaker, event) in rx {
            match event {
                Event::Clock {
                    secs,
                    paused,
                    level,
                } => heard(Heard::Clock {
                    secs,
                    paused,
                    level,
                }),
                Event::Transcript(text) => heard(Heard::Transcript(format!("{speaker}: {text}"))),
                Event::Warning(text) | Event::Failed(text) => {
                    warnings.push(format!("{speaker}: {text}"))
                }
                Event::Progress { label, steps } => heard(Heard::Step {
                    label: format!("{speaker}: {label}"),
                    steps,
                }),
                Event::Finished { session, .. } => dirs.push((speaker, session)),
                _ => {}
            }
        }
    });
    if dirs.is_empty() {
        anyhow::bail!(warnings.join("; "));
    }
    let mut source = leo_services::session::Session::open(&dirs[0].1)?.archive();
    source.id = id;
    source.passages.clear();
    source.points.clear();
    for (speaker, dir) in &dirs {
        let session = leo_services::session::Session::open(dir)?;
        let archive = session.archive();
        source
            .passages
            .extend(archive.passages.into_iter().map(|mut p| {
                p.speaker = (*speaker).into();
                p
            }));
        source.points.extend(archive.points);
        source.warnings.extend(archive.warnings);
    }
    source.passages.sort_by_key(|p| p.start_secs);
    source.warnings.extend(warnings);
    for warning in &source.warnings {
        heard(Heard::Warning(warning.clone()));
    }
    heard(Heard::Step {
        label: "Writing the call notes".into(),
        steps: None,
    });
    let workflows = leo_core::workflows::Workflows::load(&leo_core::store::Store::notes_dir()?)?;
    let written = write_sources(vec![source.clone()], &profile, &workflows);
    for problem in &written.problems {
        heard(Heard::Warning(problem.clone()));
    }
    let (title, body) = (written.title, written.body);
    let committed = source.clone();
    Ok(Recorded {
        title,
        body,
        source: Some(source),
        commit: Some(Arc::new(move |notes, note| {
            leo_core::recording::save(notes, note, &committed)?;
            for (_, dir) in &dirs {
                leo_services::session::Session::open(dir)?.finish()?;
            }
            Ok(())
        })),
    })
}
