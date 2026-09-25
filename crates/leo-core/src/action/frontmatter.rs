//! Frontmatter and `@leo` prompts.

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
