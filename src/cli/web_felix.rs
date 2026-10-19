use std::sync::Arc;

use leo_services::ai::session::{Session, Systems, ToolDef, Turn};
use leo_services::ai::spend::Answered;

pub fn writer(
    usage: Arc<leo_web::usage::Usage>,
    purpose: leo_web::usage::Purpose,
) -> leo_web::Writer {
    Arc::new(move |system, user, most| {
        let started = std::time::Instant::now();
        let prompt = leo_services::ai::chat::Prompt {
            system: system.into(),
            user: user.into(),
        };
        let result = if matches!(
            purpose,
            leo_web::usage::Purpose::Summary | leo_web::usage::Purpose::QuickReview
        ) {
            leo_services::ai::quick_outcome(prompt, most)
        } else {
            leo_services::ai::chat_outcome(prompt, most)
        };
        let spent = result
            .as_ref()
            .ok()
            .and_then(|outcome| outcome.spent.as_ref())
            .map(|spent| leo_web::Spent {
                input: spent.input,
                output: spent.output,
                cached: spent.cached,
                estimated: spent.estimated,
                ..Default::default()
            });
        usage.record(purpose, spent.as_ref(), started.elapsed(), result.is_err());
        result.map(|outcome| outcome.value)
    })
}

pub fn spent_of(answered: Answered) -> leo_web::Spent {
    leo_web::Spent {
        by: answered.by,
        model: answered.model,
        effort: answered.effort,
        input: answered.input,
        output: answered.output,
        estimated: answered.estimated,
        cost: answered.cost,
        plan: answered.plan,
        local: answered.local,
        cached: answered.cached,
        steps: 1,
    }
}

struct Talk(Box<dyn Session>);

impl leo_web::Conversation for Talk {
    fn native(&self) -> bool {
        self.0.native()
    }

    fn context_chars(&self) -> Option<usize> {
        self.0.context_chars()
    }

    fn say(
        &mut self,
        text: &str,
        exchange: leo_web::Exchange<'_>,
    ) -> anyhow::Result<leo_web::Reply> {
        let (text, answered) = self.0.say(
            text,
            Turn {
                tail: exchange.tail,
                max_tokens: exchange.max_tokens,
                most_calls: exchange.most_calls,
                sink: exchange.piece,
                restart: exchange.restart,
                call: exchange.call,
                stopped: exchange.stopped,
                tools_done: exchange.tools_done,
            },
        )?;
        Ok(leo_web::Reply {
            text,
            spent: answered.map(spent_of),
        })
    }
}

pub fn seer() -> leo_web::captions::Seer {
    Arc::new(
        |system: &str, user: &str, pictures: Vec<leo_web::captions::Picture>| {
            let images: Vec<leo_services::ai::provider::Image> = pictures
                .into_iter()
                .map(|p| leo_services::ai::provider::Image {
                    mime: p.mime,
                    bytes: p.bytes,
                })
                .collect();
            leo_services::ai::see(
                leo_services::ai::chat::Prompt {
                    system: system.to_string(),
                    user: user.to_string(),
                },
                &images,
                1_500,
            )
        },
    )
}

pub fn converser() -> leo_web::Converser {
    Arc::new(
        |given: &leo_web::Instructions, specs: &[leo_web::ToolSpec]| {
            let tools: Vec<ToolDef> = specs
                .iter()
                .map(|spec| ToolDef {
                    name: spec.name.clone(),
                    description: spec.description.clone(),
                    schema: spec.schema.clone(),
                })
                .collect();
            let systems = Systems {
                native: given.native,
                text: given.text,
            };
            leo_services::ai::open_session(&systems, &tools)
                .map(|session| Box::new(Talk(session)) as Box<dyn leo_web::Conversation>)
        },
    )
}

pub fn meaning() -> leo_web::Meaning {
    if leo_services::meaning::wanted() {
        std::thread::spawn(|| {
            let _ = leo_services::meaning::fetch(true);
        });
    }
    std::sync::Arc::new(|texts: &[String], query: bool| leo_services::meaning::embed(texts, query))
}
