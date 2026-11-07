use anyhow::Result;
use leo_services::{
    config::{secret::default_store, Config},
    doctor::{self, Probe},
    health::State,
};
use serde_json::{json, Value};
use std::path::Path;
pub fn scan(notes: &Path) -> Result<Value> {
    let config = Config::load();
    let secrets = default_store();
    let path = Config::config_path()?;
    let sections = doctor::scan(&config, secrets.as_ref(), notes, &path, Probe::default());
    let sign_in=config.chat.chain.first().and_then(|name|config.provider(name)).and_then(|provider| {
        let agent=leo_services::ai::provider::agent_cli::Agent::of(provider)?;
        let signed_in=leo_services::ai::provider::agent_cli::signed_in(agent,provider.bin.as_deref().unwrap_or(agent.program()));
        Some(json!({"title":"Writing AI sign-in","checks":[{"name":format!("{} sign-in",agent.program()),"state":match signed_in { Some(true)=>"ready",Some(false)=>"missing",None=>"warn" },"fix":format!("If sign-in is needed, {}. You can also use Test in Writing AI.",agent.sign_in()),"section":"writing"}]}))
    });
    let mut sections:Vec<Value>=sections.into_iter().map(|section| {
        let checks:Vec<Value>=section.checks.into_iter().map(|check| {
            let (state,fix)=match check.state { State::Ready=>("ready",String::new()),State::Missing{fix}=>("missing",fix),State::Warn{note}=>("warn",note) };
            json!({"name":check.what,"state":state,"fix":fix,"detail":check.detail,"needed_for":check.needed_for})
        }).collect();
        json!({"title":section.title,"checks":checks})
    }).collect();
    if let Some(sign_in) = sign_in {
        sections.insert(0, sign_in);
    }
    if let Some(problem) = leo_services::ai::provider::codex_app::native_problem() {
        sections.insert(0, json!({"title":"Felix and Codex","checks":[{"name":"Codex native session","state":"warn","fix":problem,"section":"writing"}]}));
    }
    Ok(json!({"sections":sections}))
}
