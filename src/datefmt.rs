//! Formatting timestamps for display in Stockholm local time. We apply
//! the EU DST rule directly rather than pull in a tz database — see
//! [`stockholm_offset`]. Used for clip upload/recording dates and label
//! wiki edit times.

use time::format_description::well_known::Rfc3339;
use time::macros::format_description;
use time::{Date, Month, OffsetDateTime, Time, UtcOffset, Weekday};

/// The civil date in Stockholm for a UTC instant, as ISO "YYYY-MM-DD".
pub fn iso_date(utc: OffsetDateTime) -> Option<String> {
    utc.to_offset(stockholm_offset(utc))
        .date()
        .format(format_description!("[year]-[month]-[day]"))
        .ok()
}

/// Format a stored UTC timestamp (RFC 3339) as the plain ISO date —
/// "2026-05-26" — it fell on in Stockholm. Anything that doesn't parse
/// as RFC 3339 (a bare date or year from an audio tag) is passed through
/// by its date part.
pub fn iso_date_from_rfc3339(raw: &str) -> String {
    OffsetDateTime::parse(raw, &Rfc3339)
        .ok()
        .and_then(iso_date)
        .unwrap_or_else(|| raw.split('T').next().unwrap_or(raw).to_string())
}

/// Format a stored UTC timestamp (RFC 3339) as Stockholm-local date and
/// time, "YYYY-MM-DD HH:MM". Falls back to the raw string if it doesn't
/// parse. Used for wiki revision timestamps, where time-of-day matters.
pub fn datetime_from_rfc3339(raw: &str) -> String {
    OffsetDateTime::parse(raw, &Rfc3339)
        .ok()
        .and_then(|utc| {
            utc.to_offset(stockholm_offset(utc))
                .format(format_description!("[year]-[month]-[day] [hour]:[minute]"))
                .ok()
        })
        .unwrap_or_else(|| raw.to_string())
}

/// Stockholm's UTC offset at a given instant under the EU DST rule:
/// CEST (+02:00) from 01:00 UTC on the last Sunday of March until 01:00
/// UTC on the last Sunday of October, CET (+01:00) otherwise.
fn stockholm_offset(utc: OffsetDateTime) -> UtcOffset {
    let cet = UtcOffset::from_hms(1, 0, 0).expect("valid offset");
    let cest = UtcOffset::from_hms(2, 0, 0).expect("valid offset");

    let at_0100_utc = |month| {
        last_sunday(utc.year(), month)
            .with_time(Time::from_hms(1, 0, 0).expect("valid time"))
            .assume_utc()
    };
    let dst_start = at_0100_utc(Month::March);
    let dst_end = at_0100_utc(Month::October);

    if utc >= dst_start && utc < dst_end { cest } else { cet }
}

/// The last Sunday of `month` in `year`. Only called for March and
/// October, both of which always have 31 days.
fn last_sunday(year: i32, month: Month) -> Date {
    let mut day = Date::from_calendar_date(year, month, 31).expect("31 is valid for Mar/Oct");
    while day.weekday() != Weekday::Sunday {
        day = day.previous_day().expect("a day before the 31st exists");
    }
    day
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_date_uses_stockholm_time() {
        // Summer (CEST, +02:00): 22:30 UTC is already past midnight in
        // Stockholm, so the date rolls forward. With a naive +01:00 it
        // would still read the 1st — this pins the DST offset.
        assert_eq!(iso_date_from_rfc3339("2026-08-01T22:30:00.000Z"), "2026-08-02");
        // Winter (CET, +01:00): 22:30 UTC is 23:30 in Stockholm, same day.
        assert_eq!(iso_date_from_rfc3339("2026-02-01T22:30:00.000Z"), "2026-02-01");
    }

    #[test]
    fn iso_date_handles_dst_transition_boundaries() {
        // DST 2026 runs [2026-03-29 01:00 UTC, 2026-10-25 01:00 UTC).
        // Just inside the autumn end it's still CEST (+02:00): 23:30 UTC
        // on the 24th is 01:30 on the 25th locally.
        assert_eq!(iso_date_from_rfc3339("2026-10-24T23:30:00.000Z"), "2026-10-25");
        // Just after the switch back to CET (+01:00): 23:30 UTC on the
        // 25th is 00:30 on the 26th locally.
        assert_eq!(iso_date_from_rfc3339("2026-10-25T23:30:00.000Z"), "2026-10-26");
    }

    #[test]
    fn iso_date_passes_through_non_rfc3339() {
        // Bare year/date from an ID3 tag isn't a full timestamp; keep the
        // date part rather than dropping it.
        assert_eq!(iso_date_from_rfc3339("2024"), "2024");
        assert_eq!(iso_date_from_rfc3339("2024-03-15"), "2024-03-15");
    }

    #[test]
    fn datetime_uses_stockholm_time() {
        // Summer +02:00: 19:46 UTC -> 21:46 local.
        assert_eq!(datetime_from_rfc3339("2026-05-29T19:46:11.000Z"), "2026-05-29 21:46");
        // Winter +01:00: 23:30 UTC -> 00:30 next day local.
        assert_eq!(datetime_from_rfc3339("2026-02-01T23:30:00.000Z"), "2026-02-02 00:30");
    }
}
