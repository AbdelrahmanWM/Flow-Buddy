//! Local dates and times, in the system's time zone: day boundaries,
//! quiet hours, summaries and the times people read ("today 14:02").

use jiff::civil::Date;
use jiff::tz::TimeZone;
use jiff::{Span, Timestamp, Zoned};

pub use jiff::Zoned as LocalTime;
pub use jiff::civil::Date as LocalDate;

/// The local time at `ms` Unix milliseconds; `None` out of range.
pub fn at_ms(ms: u64) -> Option<Zoned> {
    let ts = Timestamp::from_millisecond(i64::try_from(ms).ok()?).ok()?;
    Some(ts.to_zoned(TimeZone::system()))
}

/// The local time at `s` Unix seconds; `None` out of range.
pub fn at_s(s: u64) -> Option<Zoned> {
    let ts = Timestamp::from_second(i64::try_from(s).ok()?).ok()?;
    Some(ts.to_zoned(TimeZone::system()))
}

pub fn now() -> Zoned {
    Zoned::now()
}

/// Minutes since local midnight.
pub fn minute_of_day(t: &Zoned) -> u32 {
    t.hour() as u32 * 60 + t.minute() as u32
}

/// The local date of `ms`; 1970-01-01 if out of range.
pub fn date_of_ms(ms: u64) -> Date {
    at_ms(ms).map_or(Date::constant(1970, 1, 1), |t| t.date())
}

/// `n` days before `date` (or after, for negative `n`).
pub fn days_before(date: Date, n: i64) -> Date {
    date.checked_sub(Span::new().days(n)).unwrap_or(date)
}

/// `YYYY-MM-DD`.
pub fn date_key(date: Date) -> String {
    date.strftime("%Y-%m-%d").to_string()
}

/// `t` as `fmt` (strftime: `%a %H:%M`, `%-d %b`…).
pub fn format(t: &Zoned, fmt: &str) -> String {
    t.strftime(fmt).to_string()
}

/// Seconds from `a` to `b` (negative if `b` is earlier).
pub fn seconds_between(a: &Zoned, b: &Zoned) -> i64 {
    b.timestamp().as_second() - a.timestamp().as_second()
}

/// Seconds from `now` to the next local midnight.
pub fn until_tomorrow(now: &Zoned) -> Option<u64> {
    let midnight = now
        .date()
        .tomorrow()
        .ok()?
        .to_zoned(now.time_zone().clone())
        .ok()?;
    u64::try_from(seconds_between(now, &midnight)).ok()
}

/// Unix milliseconds for a local wall-clock time: for tests, mostly.
pub fn local_ms(year: i16, month: i8, day: i8, hour: i8, minute: i8) -> u64 {
    let t = Date::new(year, month, day)
        .and_then(|d| d.at(hour, minute, 0, 0).to_zoned(TimeZone::system()))
        .expect("a valid local time");
    t.timestamp().as_millisecond() as u64
}

/// A local wall-clock time: for tests, mostly.
pub fn local(year: i16, month: i8, day: i8, hour: i8, minute: i8) -> Zoned {
    at_ms(local_ms(year, month, day, hour, minute)).expect("in range")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_formats() {
        let t = local(2026, 10, 24, 22, 5);
        assert_eq!(date_key(t.date()), "2026-10-24");
        assert_eq!(minute_of_day(&t), 22 * 60 + 5);
        assert_eq!(format(&t, "%a %-d %b %H:%M"), "Sat 24 Oct 22:05");
        assert_eq!(until_tomorrow(&t), Some(115 * 60));
        assert_eq!(date_key(days_before(t.date(), 7)), "2026-10-17");
        assert_eq!(date_key(days_before(t.date(), -8)), "2026-11-01");
        let ms = local_ms(2026, 10, 24, 22, 5);
        assert_eq!(date_key(date_of_ms(ms)), "2026-10-24");
        assert_eq!(seconds_between(&t, &local(2026, 10, 25, 22, 5)), 86_400);
        assert!(at_ms(u64::MAX).is_none());
    }

    #[test]
    fn a_day_with_a_clock_change() {
        // 1 November 2026 in New York: clocks go back at 2:00, so the day
        // is 25 hours long. Midnight is still midnight, and it's one day.
        let Ok(tz) = TimeZone::get("America/New_York") else {
            return; // no time zone data here
        };
        let start = Date::constant(2026, 11, 1).to_zoned(tz.clone()).unwrap();
        assert_eq!(until_tomorrow(&start), Some(25 * 3600));
        let late = Date::constant(2026, 11, 1)
            .at(23, 30, 0, 0)
            .to_zoned(tz)
            .unwrap();
        assert_eq!(late.date(), start.date());
        assert_eq!(minute_of_day(&late), 23 * 60 + 30);
        assert_eq!(until_tomorrow(&late), Some(30 * 60));
    }
}
