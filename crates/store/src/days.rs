//! Day keys: stats are grouped by *local* date, as `YYYY-MM-DD`.

use chrono::{Days, Local, NaiveDate, TimeZone};

pub fn local_date(now_ms: u64) -> NaiveDate {
    Local
        .timestamp_millis_opt(now_ms as i64)
        .single()
        .map(|t| t.date_naive())
        .unwrap_or_default()
}

pub fn key(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

/// Today's key for a unix-millisecond timestamp.
pub fn day(now_ms: u64) -> String {
    key(local_date(now_ms))
}

/// (first, last) keys of the `n` days ending `offset` days before the day
/// of `now_ms`. `range(now, 7, 0)` is the last week, `range(now, 7, 7)`
/// the week before.
pub fn range(now_ms: u64, n: u64, offset: u64) -> (String, String) {
    let last = local_date(now_ms) - Days::new(offset);
    let first = last - Days::new(n.saturating_sub(1));
    (key(first), key(last))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        let noon = Local
            .with_ymd_and_hms(2026, 10, 24, 12, 0, 0)
            .unwrap()
            .timestamp_millis() as u64;
        assert_eq!(day(noon), "2026-10-24");
        assert_eq!(
            range(noon, 1, 0),
            ("2026-10-24".into(), "2026-10-24".into())
        );
        assert_eq!(
            range(noon, 7, 0),
            ("2026-10-18".into(), "2026-10-24".into())
        );
        assert_eq!(
            range(noon, 7, 7),
            ("2026-10-11".into(), "2026-10-17".into())
        );
    }
}
