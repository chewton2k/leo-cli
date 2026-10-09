use crate::ai::provider::Spent;
use crate::config::choice::WRITING;
use crate::config::provider::ProviderKind;
use crate::config::Config;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Answered {
    pub by: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub input: u64,
    pub output: u64,
    pub estimated: bool,
    pub cost: Option<f64>,
    pub plan: bool,
    pub local: bool,
    pub cached: u64,
}

pub fn cache_read_share(provider: &str, model: &str) -> f64 {
    let model = model.to_lowercase();
    match provider {
        "anthropic" if model.contains("fable-5-1") || model.contains("mythos-5-1") => 0.025,
        "anthropic" if model.contains("opus-5-5") || model.contains("sonnet-5-5") => 0.05,
        "anthropic" | "openai" => 0.1,
        "gemini" | "xai" => 0.25,
        _ => 1.0,
    }
}

pub const CACHE_WRITE_SHARE: f64 = 1.25;

pub fn rates(price: &str) -> Option<(f64, f64)> {
    if !price.contains('$') {
        return price.trim_start().starts_with("free").then_some((0.0, 0.0));
    }
    let mut input = None;
    let mut output = None;
    for piece in price.split('$').skip(1) {
        let number: String = piece
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        let Ok(value) = number.parse::<f64>() else {
            continue;
        };
        let rest = piece[number.len()..].trim_start();
        if rest.starts_with("in") && input.is_none() {
            input = Some(value);
        } else if rest.starts_with("out") && output.is_none() {
            output = Some(value);
        }
    }
    Some((input?, output?))
}

fn on_this_computer(url: &str) -> bool {
    ["://localhost", "://127.0.0.1", "://[::1]", "://0.0.0.0"]
        .iter()
        .any(|host| url.contains(host))
}

pub fn answered(cfg: &Config, provider: &str, spent: Spent) -> Answered {
    let pc = cfg.provider(provider);
    let kind = pc.and_then(|p| p.kind);
    let choice = WRITING.iter().find(|c| c.provider == provider);
    let plan = match kind {
        Some(kind) => matches!(kind, ProviderKind::ClaudeCode | ProviderKind::Codex),
        None => matches!(provider, "claude_code" | "codex"),
    };
    let local = choice.is_some_and(|c| c.local())
        || pc
            .and_then(|p| p.base_url.as_deref())
            .is_some_and(on_this_computer);
    let cost = if plan {
        None
    } else if local {
        Some(0.0)
    } else {
        spent
            .model
            .as_deref()
            .and_then(|model| choice.and_then(|c| c.price(model)))
            .and_then(rates)
            .map(|(input, output)| {
                let read = cache_read_share(provider, spent.model.as_deref().unwrap_or(""));
                let cached = spent.cached.min(spent.input);
                let written = spent.cache_written.min(spent.input - cached);
                let fresh = spent.input - cached - written;
                (fresh as f64 * input
                    + cached as f64 * input * read
                    + written as f64 * input * CACHE_WRITE_SHARE
                    + spent.output as f64 * output)
                    / 1_000_000.0
            })
    };
    Answered {
        by: choice.map_or_else(|| provider.to_string(), |c| c.name.to_string()),
        model: spent.model,
        effort: spent.effort,
        input: spent.input,
        output: spent.output,
        estimated: spent.estimated,
        cost,
        plan,
        local,
        cached: spent.cached,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prices_are_read_from_the_model_list() {
        assert_eq!(rates("$2 in, $10 out per 1M tokens"), Some((2.0, 10.0)));
        assert_eq!(
            rates("free tier, then $0.25 in, $1.50 out per 1M tokens"),
            Some((0.25, 1.5))
        );
        assert_eq!(rates("free, with daily limits"), Some((0.0, 0.0)));
        assert_eq!(rates("free"), Some((0.0, 0.0)));
        assert_eq!(rates("included in your Claude plan"), None);
        assert_eq!(
            rates("$0.10 in, $0.50 out per 1M tokens (more above 100K-token prompts)"),
            Some((0.1, 0.5))
        );
    }

    fn spent(model: &str, input: u64, output: u64) -> Spent {
        Spent {
            model: Some(model.into()),
            input,
            output,
            ..Spent::default()
        }
    }

    #[test]
    fn a_paid_answer_costs_its_tokens_and_a_plan_or_this_computer_costs_nothing_extra() {
        let cfg = Config::default();
        let paid = answered(
            &cfg,
            "anthropic",
            spent("claude-sonnet-5-5", 1_000_000, 100_000),
        );
        assert_eq!(paid.by, "Anthropic");
        assert!((paid.cost.unwrap() - 3.0).abs() < 1e-9);
        assert!(!paid.plan && !paid.local);

        let plan = answered(&cfg, "claude_code", spent("claude-sonnet-5-5", 10, 10));
        assert_eq!(
            (plan.by.as_str(), plan.cost, plan.plan),
            ("Claude Code", None, true)
        );

        let local = answered(&cfg, "ollama", spent("qwen3:8b", 10, 10));
        assert_eq!((local.cost, local.local), (Some(0.0), true));

        let unknown = answered(&cfg, "openai", spent("not-a-listed-model", 10, 10));
        assert_eq!(unknown.cost, None);
    }

    #[test]
    fn tokens_read_from_the_cache_cost_a_fraction_and_tokens_written_a_little_more() {
        let cfg = Config::default();
        let cached = answered(
            &cfg,
            "anthropic",
            Spent {
                cached: 800_000,
                cache_written: 100_000,
                ..spent("claude-sonnet-5-5", 1_000_000, 0)
            },
        );
        assert_eq!(cached.cached, 800_000);
        let rate = 2.0;
        let expected = (100_000.0 * rate + 800_000.0 * rate * 0.05 + 100_000.0 * rate * 1.25) / 1e6;
        assert!(
            (cached.cost.unwrap() - expected).abs() < 1e-9,
            "{:?}",
            cached.cost
        );
        assert!((cache_read_share("openai", "gpt-5") - 0.1).abs() < 1e-9);
        assert!(
            (cache_read_share("openrouter", "x") - 1.0).abs() < 1e-9,
            "unknown discounts are not assumed"
        );
    }
}
