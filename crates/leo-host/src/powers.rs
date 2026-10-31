pub fn powers() -> leo_web::Powers {
    let usage = std::sync::Arc::new(leo_web::usage::Usage::default());
    let writer = crate::felix::writer(
        std::sync::Arc::clone(&usage),
        leo_web::usage::Purpose::OtherWriting,
    );
    let streamer: leo_web::Streamer = std::sync::Arc::new(
        |system: &str,
         user: &str,
         most: u32,
         piece: &mut dyn FnMut(&str),
         restart: &mut dyn FnMut()| {
            let (text, answered) = leo_services::ai::chat_streaming(
                leo_services::ai::chat::Prompt {
                    system: system.to_string(),
                    user: user.to_string(),
                },
                most,
                piece,
                restart,
            )?;
            Ok(leo_web::Reply {
                text,
                spent: answered.map(crate::felix::spent_of),
            })
        },
    );
    leo_web::Powers {
        python: Some(leo_web::Runtime {
            dir: leo_services::pyodide::dir(),
            ready: std::sync::Arc::new(leo_services::pyodide::ready),
            fetch: std::sync::Arc::new(|| leo_services::pyodide::fetch().map(|_| ())),
        }),
        cpp: Some(leo_web::Runtime {
            dir: leo_services::clang::dir(),
            ready: std::sync::Arc::new(leo_services::clang::ready),
            fetch: std::sync::Arc::new(|| leo_services::clang::fetch().map(|_| ())),
        }),
        go: Some(leo_web::Runtime {
            dir: leo_services::runtimes::dir(&leo_services::runtimes::GO),
            ready: std::sync::Arc::new(|| {
                leo_services::runtimes::ready(&leo_services::runtimes::GO)
            }),
            fetch: std::sync::Arc::new(|| {
                leo_services::runtimes::fetch(&leo_services::runtimes::GO).map(|_| ())
            }),
        }),
        rust: Some(leo_web::Runtime {
            dir: leo_services::runtimes::dir(&leo_services::runtimes::RUST),
            ready: std::sync::Arc::new(|| {
                leo_services::runtimes::ready(&leo_services::runtimes::RUST)
            }),
            fetch: std::sync::Arc::new(|| {
                leo_services::runtimes::fetch(&leo_services::runtimes::RUST).map(|_| ())
            }),
        }),
        java: Some(leo_web::Runtime {
            dir: leo_services::runtimes::dir(&leo_services::runtimes::JAVA),
            ready: std::sync::Arc::new(|| {
                leo_services::runtimes::ready(&leo_services::runtimes::JAVA)
            }),
            fetch: std::sync::Arc::new(|| {
                leo_services::runtimes::fetch(&leo_services::runtimes::JAVA).map(|_| ())
            }),
        }),
        writer: Some(writer),
        quick: Some(crate::felix::writer(
            std::sync::Arc::clone(&usage),
            leo_web::usage::Purpose::QuickReview,
        )),
        housekeeping: Some(crate::felix::writer(
            std::sync::Arc::clone(&usage),
            leo_web::usage::Purpose::Summary,
        )),
        usage,
        chat: Some(streamer),
        settings: Some(std::sync::Arc::new(crate::settings::WebSettings)),
        importer: Some(std::sync::Arc::new(
            |files: Vec<leo_web::UploadFile>,
             wants: &str,
             progress: &mut dyn FnMut(&str, usize, usize)| {
                let uploads: Vec<leo_services::import::Upload> = files
                    .into_iter()
                    .map(|f| leo_services::import::Upload {
                        name: f.name,
                        mime: f.mime,
                        bytes: f.bytes,
                    })
                    .collect();
                let made = leo_services::import::import(&uploads, wants, progress)?;
                Ok(leo_web::Made {
                    title: made.title,
                    body: made.body,
                    figures: made
                        .figures
                        .into_iter()
                        .map(|f| leo_web::Figure {
                            place: f.place,
                            bytes: f.bytes,
                            photo: f.photo,
                        })
                        .collect(),
                })
            },
        )),
        speech: Some(std::sync::Arc::new(crate::speech::Speech)),
        listener: Some(crate::record::listener()),
        regenerator: Some(crate::record::regenerator()),
        calendar_access: Some(std::sync::Arc::new(crate::settings::WebSettings)),
        housekeeper: Some(std::sync::Arc::new(crate::storage::Housekeeping)),
        room: Some(std::sync::Arc::new(leo_services::ai::felix_room)),
        context: Some(std::sync::Arc::new(leo_services::ai::felix_context)),
        web: Some(leo_web::Web {
            search: std::sync::Arc::new(|query: &str| {
                Ok(leo_services::web::search(query)?
                    .into_iter()
                    .map(|h| leo_web::WebHit {
                        title: h.title,
                        url: h.url,
                        snippet: h.snippet,
                    })
                    .collect())
            }),
            page: std::sync::Arc::new(|address: &str| leo_services::web::page(address)),
            needed: std::sync::Arc::new(|| {
                !leo_services::web::has_own_web(&leo_services::config::Config::load())
            }),
        }),
        reader: Some(std::sync::Arc::new(
            |file: leo_web::UploadFile, progress: &mut dyn FnMut(&str)| {
                leo_services::import::read_for_chat(
                    &leo_services::import::Upload {
                        name: file.name,
                        mime: file.mime,
                        bytes: file.bytes,
                    },
                    &|prompt, images, most| leo_services::ai::see(prompt, images, most),
                    progress,
                )
            },
        )),
        converse: Some(crate::felix::converser()),
        seer: Some(crate::felix::seer()),
        meaning: Some(crate::felix::meaning()),
    }
}
