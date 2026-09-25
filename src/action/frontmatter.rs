//! Frontmatter and `@leo` prompts.

use anyhow::Result;


/// Parse an editor buffer's `---` frontmatter block into (title, tags, body).
/// Malformed or absent frontmatter yields an empty title and tags with the
/// whole buffer as the body, so a user who deletes the header keeps their text.
pub fn parse_frontmatter(raw: &str) -> (String, Vec<String>, String) {
    let trimmed = raw.trim_start();
    if !trimmed.starts_with("---") {
        return (String::new(), Vec::new(), raw.to_string());
    }

    let after_open = trimmed[3..].trim_start_matches('-');
    let after_open = after_open.strip_prefix('\n').unwrap_or(after_open);

    let Some(close_pos) = after_open.find("\n---") else {
        return (String::new(), Vec::new(), raw.to_string());
    };

    let front = &after_open[..close_pos];
    let body_start = close_pos + 4; // past "\n---"
    let body = after_open[body_start..]
        .strip_prefix('\n')
        .unwrap_or(&after_open[body_start..]);

    let mut title = String::new();
    let mut tags = Vec::new();
    for line in front.lines() {
        if let Some(val) = line.strip_prefix("title:") {
            title = val.trim().to_string();
        } else if let Some(val) = line.strip_prefix("tags:") {
            tags = val
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
    }
    (title, tags, body.to_string())
}

/// If `line` is `@leo <question>`, return the question.
pub fn is_leo_prompt(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if trimmed.len() < 5 || !trimmed[..5].eq_ignore_ascii_case("@leo ") {
        return None;
    }
    let q = trimmed[5..].trim();
    if q.is_empty() {
        None
    } else {
        Some(q)
    }
}

/// Replace every `@leo` line with the model's answer, giving each one five
/// lines of surrounding context plus the whole note for background. A prompt
/// that fails to expand is left in place rather than dropped.
pub fn expand_leo_prompts(body: &str, title: &str) -> Result<(String, usize)> {
    let lines: Vec<&str> = body.lines().collect();
    let mut result: Vec<String> = Vec::with_capacity(lines.len());
    let mut count = 0;

    for (i, &line) in lines.iter().enumerate() {
        let Some(question) = is_leo_prompt(line) else {
            result.push(line.to_string());
            continue;
        };

        let before = lines[i.saturating_sub(5)..i].join("\n");
        let after_end = (i + 6).min(lines.len());
        let after = lines[(i + 1)..after_end].join("\n");
        let local_context = format!("{before}\n{after}");

        match crate::ai::expand_prompt(question, &local_context, title, body) {
            Ok(expansion) if !expansion.is_empty() => {
                result.push(expansion);
                count += 1;
            }
            _ => result.push(line.to_string()),
        }
    }

    Ok((result.join("\n"), count))
}
