//! Sidebar.tsx's relative session time, shared with native session chrome.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn relative(value: u64, now: u64) -> String {
    if value == 0 {
        return String::new();
    }
    let seconds = now.saturating_sub(value).saturating_add(500) / 1_000;
    if seconds < 60 {
        return "now".into();
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{minutes}m");
    }
    let hours = minutes / 60;
    if hours < 24 {
        return if minutes.is_multiple_of(60) {
            format!("{hours}h")
        } else {
            format!("{hours}h {}m", minutes % 60)
        };
    }
    let days = hours / 24;
    if days < 7 {
        return format!("{days}d");
    }
    i64::try_from(value)
        .ok()
        .and_then(chrono::DateTime::from_timestamp_millis)
        .map(|date| date.format("%b %-d").to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn session_ages_match_the_sidebar_and_keep_unknown_times_blank() {
        let value = 1_700_000_000_000;
        assert_eq!(relative(0, value), "");
        assert_eq!(relative(value, value - 1_000), "now");
        assert_eq!(relative(value, value + 59_499), "now");
        assert_eq!(relative(value, value + 59_500), "1m");
        assert_eq!(relative(value, value + 900_000), "15m");
        assert_eq!(relative(value, value + 3_600_000), "1h");
        assert_eq!(relative(value, value + 10_740_000), "2h 59m");
        assert_eq!(relative(value, value + 86_400_000), "1d");
        assert_eq!(relative(value, value + 604_800_000), "Nov 14");
    }
}
