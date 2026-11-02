use std::time::UNIX_EPOCH;

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> i64 {
    loki_sdk::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

/// "Just now", "5 min ago", "3 hours ago", "Yesterday", "4 days ago", then a
/// UTC date such as "12 Mar 2026".
pub fn relative_time(then_ms: i64, now_ms: i64) -> String {
    let minutes = (now_ms - then_ms).max(0) / 60_000;
    match minutes {
        0 => "Just now".into(),
        1..60 => format!("{minutes} min ago"),
        60..120 => "1 hour ago".into(),
        120..1440 => format!("{} hours ago", minutes / 60),
        1440..2880 => "Yesterday".into(),
        2880..10080 => format!("{} days ago", minutes / 1440),
        _ => date(then_ms),
    }
}

fn date(ms: i64) -> String {
    // Howard Hinnant's days-to-civil conversion.
    let z = ms.div_euclid(86_400_000) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!("{day} {} {year}", MONTHS[(month - 1) as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_recent_and_old_times() {
        let now = 1_772_000_000_000;
        assert_eq!(relative_time(now - 10_000, now), "Just now");
        assert_eq!(relative_time(now - 5 * 60_000, now), "5 min ago");
        assert_eq!(relative_time(now - 3 * 3_600_000, now), "3 hours ago");
        assert_eq!(relative_time(now - 30 * 3_600_000, now), "Yesterday");
        assert_eq!(relative_time(now - 4 * 86_400_000, now), "4 days ago");
        assert_eq!(relative_time(0, now), "1 Jan 1970");
        assert_eq!(relative_time(951_782_400_000, now), "29 Feb 2000");
    }
}
