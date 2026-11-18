//! Day keys: stats are grouped by *local* date, as `YYYY-MM-DD`.

use flowbuddy_core::time::{self, LocalDate};

pub fn local_date(now_ms: u64) -> LocalDate {
    time::date_of_ms(now_ms)
}

pub fn key(date: LocalDate) -> String {
    time::date_key(date)
}

/// Today's key for a unix-millisecond timestamp.
pub fn day(now_ms: u64) -> String {
    key(local_date(now_ms))
}

/// (first, last) keys of the `n` days ending `offset` days before the day
/// of `now_ms`. `range(now, 7, 0)` is the last week, `range(now, 7, 7)`
/// the week before.
pub fn range(now_ms: u64, n: u64, offset: u64) -> (String, String) {
    let last = time::days_before(local_date(now_ms), offset as i64);
    let first = time::days_before(last, n.saturating_sub(1) as i64);
    (key(first), key(last))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        let noon = time::local_ms(2026, 10, 24, 12, 0);
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
