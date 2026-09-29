use chrono::{DateTime, Datelike, Local, Utc};

pub fn short(at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let seconds = (now - at).num_seconds();
    let local = at.with_timezone(&Local);
    let today = now.with_timezone(&Local);
    match seconds {
        s if s < 60 => "now".to_string(),
        s if s < 3600 => format!("{}m", s / 60),
        s if s < 86_400 => format!("{}h", s / 3600),
        s if s < 7 * 86_400 => local.format("%a").to_string(),
        _ if local.year() == today.year() => local.format("%b %-d").to_string(),
        _ => local.format("%b %-d %Y").to_string(),
    }
}

pub fn long(at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let local = at.with_timezone(&Local);
    let today = now.with_timezone(&Local);
    if local.year() == today.year() {
        local.format("edited %a %b %-d, %-I:%M %p").to_string()
    } else {
        local.format("edited %a %b %-d %Y, %-I:%M %p").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn recent_edits_read_as_how_long_ago() {
        let now = Utc::now();
        assert_eq!(short(now, now), "now");
        assert_eq!(short(now - Duration::minutes(5), now), "5m");
        assert_eq!(short(now - Duration::hours(3), now), "3h");
        let days = short(now - Duration::days(3), now);
        assert_eq!(days.len(), 3, "{days}");
    }

    #[test]
    fn older_edits_read_as_a_date() {
        let now = Utc::now();
        let then = now - Duration::days(400);
        let shown = short(then, now);
        assert!(
            shown.ends_with(&then.with_timezone(&Local).year().to_string()),
            "{shown}"
        );
    }

    #[test]
    fn the_title_bar_gives_the_day_and_the_time() {
        let now = Utc::now();
        let shown = long(now, now);
        assert!(shown.starts_with("edited "), "{shown}");
        assert!(shown.ends_with("AM") || shown.ends_with("PM"), "{shown}");
        assert!(long(now - Duration::days(400), now).contains(
            &(now - Duration::days(400))
                .with_timezone(&Local)
                .year()
                .to_string()
        ));
    }
}
