use std::process::Stdio;

use anyhow::{Context, Result};

pub fn valid_uri(uri: &str) -> bool {
    let trimmed = uri.trim();
    if trimmed.is_empty()
        || trimmed.len() > 2048
        || trimmed.starts_with('-')
        || trimmed
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || matches!(c, '"' | '<' | '>' | '\\'))
    {
        return false;
    }
    let Some((scheme, rest)) = trimmed.split_once(':') else {
        return false;
    };
    if scheme.eq_ignore_ascii_case("mailto") {
        return !rest.is_empty() && !rest.starts_with('/');
    }
    if !scheme.eq_ignore_ascii_case("https") && !scheme.eq_ignore_ascii_case("http") {
        return false;
    }
    let Some(rest) = rest.strip_prefix("//") else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '.' | ':' | '[' | ']'))
}

pub fn link(uri: &str) -> Result<()> {
    let trimmed = uri.trim();
    if !valid_uri(trimmed) {
        anyhow::bail!("refusing to open an unsafe or unsupported link");
    }
    let (opener, args): (&str, Vec<&str>) = if cfg!(target_os = "macos") {
        ("open", vec![trimmed])
    } else if cfg!(windows) {
        ("rundll32", vec!["url.dll,FileProtocolHandler", trimmed])
    } else {
        ("xdg-open", vec![trimmed])
    };
    let status = crate::children::quiet_command(opener)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("could not run {opener}"))?;
    if !status.success() {
        anyhow::bail!("{opener} could not open {trimmed}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_safe_web_and_mail_links_are_accepted() {
        assert!(valid_uri("https://example.com/path?a=1"));
        assert!(valid_uri("http://example.com"));
        assert!(valid_uri(
            "https://example.com/search?q=hello%20world&lang=en"
        ));
        assert!(valid_uri(
            "mailto:hello@example.com?subject=Hello%20there&body=Thanks"
        ));
        assert!(valid_uri("https://[::1]:4000/path"));
        assert!(!valid_uri("https:///missing-host"));
        assert!(!valid_uri("https://example.com\\evil"));
        assert!(valid_uri("mailto:hello@example.com"));
        assert!(!valid_uri("file:///etc/passwd"));
        assert!(!valid_uri("javascript:alert(1)"));
        assert!(!valid_uri("-a Calculator"));
        assert!(!valid_uri("https://example.com&calc.exe"));
        assert!(!valid_uri("https://example.com\nmalicious"));
    }
}
