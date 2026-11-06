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
         restart: &mut dyn FnMut(),
         stopped: &dyn Fn() -> bool| {
            let (text, answered) = leo_services::ai::chat_streaming(
                leo_services::ai::chat::Prompt {
                    system: system.to_string(),
                    user: user.to_string(),
                },
                most,
                piece,
                restart,
                stopped,
            )?;
            Ok(leo_web::Reply {
                text,
                spent: answered.map(crate::felix::spent_of),
            })
        },
    );
    let toolless_chat: leo_web::Streamer = std::sync::Arc::new(
        |system: &str,
         user: &str,
         most: u32,
         piece: &mut dyn FnMut(&str),
         restart: &mut dyn FnMut(),
         stopped: &dyn Fn() -> bool| {
            let (text, answered) = leo_services::ai::toolless_streaming(
                leo_services::ai::chat::Prompt {
                    system: system.to_string(),
                    user: user.to_string(),
                },
                most,
                piece,
                restart,
                stopped,
            )?;
            Ok(leo_web::Reply {
                text,
                spent: answered.map(crate::felix::spent_of),
            })
        },
    );
    leo_web::Powers {
        writer: Some(writer),
        quick: Some(crate::felix::writer(
            std::sync::Arc::clone(&usage),
            leo_web::usage::Purpose::QuickReview,
        )),
        toolless: Some(crate::felix::writer(
            std::sync::Arc::clone(&usage),
            leo_web::usage::Purpose::MarginRead,
        )),
        housekeeping: Some(crate::felix::writer(
            std::sync::Arc::clone(&usage),
            leo_web::usage::Purpose::Summary,
        )),
        usage,
        chat: Some(streamer),
        toolless_chat: Some(toolless_chat),
        dictation: None,
        app_settings: None,
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
        housekeeper: Some(std::sync::Arc::new(crate::housekeeping::Housekeeping)),
        room: Some(std::sync::Arc::new(leo_services::ai::felix_room)),
        context: Some(std::sync::Arc::new(leo_services::ai::felix_context)),
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
