use std::sync::Arc;

use leo_services::ai::session::{Session, Systems, ToolDef, Turn};
use leo_services::ai::spend::Answered;

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
