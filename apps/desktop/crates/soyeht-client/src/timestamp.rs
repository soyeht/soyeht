//! The engine's timestamps: ISO 8601 with a time zone designator, with or without fractional
//! seconds (`2026-09-15T18:00:00.000Z`). The Mac reads them with `ISO8601DateFormatter`.

use std::time::{SystemTime, UNIX_EPOCH};

/// Seconds since the Unix epoch of a timestamp, or `None` when `value` is not one.
pub fn seconds_since_epoch(value: &str) -> Option<f64> {
    let (date, time) = value.split_once(['T', 't'])?;
    let mut date = date.splitn(3, '-');
    let year: i64 = parsed(date.next()?)?;
    let month: i64 = number(date.next()?, 2)?;
    let day: i64 = number(date.next()?, 2)?;
    let (clock, offset) = split_zone(time)?;
    let mut clock = clock.splitn(3, ':');
    let hour: i64 = number(clock.next()?, 2)?;
    let minute: i64 = number(clock.next()?, 2)?;
    let (second, fraction) = split_fraction(clock.next()?)?;
    let valid = (1..=12).contains(&month)
        && (1..=days_in_month(year, month)).contains(&day)
        && hour < 24
        && minute < 60
        && second < 61;
    valid.then(|| {
        let days = days_from_civil(year, month, day);
        ((days * 86_400 + hour * 3_600 + minute * 60 + second - offset) as f64) + fraction
    })
}

/// A timestamp of `seconds` since the Unix epoch, in UTC, to the second: the spelling the
/// engine uses.
pub fn format(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60
    )
}

/// Now, in the same unit as [`seconds_since_epoch`].
pub fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |elapsed| elapsed.as_secs_f64())
}

/// A fixed-width decimal field: all digits, exactly `width` of them.
fn number(text: &str, width: usize) -> Option<i64> {
    if text.len() == width && text.bytes().all(|byte| byte.is_ascii_digit()) {
        parsed(text)
    } else {
        None
    }
}

/// A number written in `text`, or none when it is not one.
#[allow(
    clippy::manual_ok_err,
    reason = "text that is not a number has no value, and that is the answer"
)]
pub fn parsed<T: std::str::FromStr>(text: &str) -> Option<T> {
    match text.parse() {
        Ok(value) => Some(value),
        Err(_) => None,
    }
}

/// The clock part and the zone's offset from UTC in seconds.
fn split_zone(time: &str) -> Option<(&str, i64)> {
    if let Some(clock) = time.strip_suffix(['Z', 'z']) {
        return Some((clock, 0));
    }
    let at = time.rfind(['+', '-'])?;
    let (clock, zone) = time.split_at(at);
    let sign = if zone.starts_with('-') { -1 } else { 1 };
    let (hours, minutes) = zone[1..].split_once(':').unwrap_or_else(|| {
        let middle = zone[1..].len().min(2);
        zone[1..].split_at(middle)
    });
    let offset = number(hours, 2)? * 3_600 + number(minutes, 2)? * 60;
    Some((clock, sign * offset))
}

/// The whole seconds and the fraction of the last clock field.
fn split_fraction(field: &str) -> Option<(i64, f64)> {
    match field.split_once('.') {
        None => Some((number(field, 2)?, 0.0)),
        Some((whole, digits)) => {
            let valid = !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit());
            let fraction = parsed(&format!("0.{digits}"))?;
            valid.then_some((number(whole, 2)?, fraction))
        }
    }
}

fn is_leap(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days from 1970-01-01 to a date of the proleptic Gregorian calendar.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The date of a number of days since 1970-01-01.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_timestamp_is_read_with_or_without_fractions_and_in_any_zone() {
        let at = |text| seconds_since_epoch(text).unwrap();
        assert_eq!(at("1970-01-01T00:00:00Z"), 0.0);
        assert_eq!(at("2000-03-01T00:00:00Z"), 951_868_800.0);
        assert_eq!(
            at("2026-09-15T18:00:00.500Z"),
            at("2026-09-15T18:00:00Z") + 0.5
        );
        assert_eq!(at("2026-09-15T15:00:00-03:00"), at("2026-09-15T18:00:00Z"));
        assert_eq!(at("2026-09-15T20:30:00+0230"), at("2026-09-15T18:00:00Z"));
        assert_eq!(
            at("2024-02-29T00:00:00Z") + 86_400.0,
            at("2024-03-01T00:00:00Z")
        );
    }

    #[test]
    fn a_time_is_written_as_the_engine_writes_it() {
        assert_eq!(format(0), "1970-01-01T00:00:00Z");
        assert_eq!(format(951_868_800), "2000-03-01T00:00:00Z");
        assert_eq!(format(1_791_000_005), "2026-10-03T04:00:05Z");
        for seconds in [
            0.0,
            86_399.0,
            951_868_800.0,
            1_791_000_005.0,
            4_102_444_799.0,
        ] {
            assert_eq!(seconds_since_epoch(&format(seconds as i64)), Some(seconds));
        }
    }

    #[test]
    fn text_that_is_not_a_timestamp_is_refused() {
        for text in [
            "",
            "not-a-timestamp",
            "2026-09-15",
            "2026-09-15T18:00:00",
            "2026-13-01T00:00:00Z",
            "2026-02-30T00:00:00Z",
            "2026-09-15T24:00:00Z",
            "2026-09-15T18:60:00Z",
            "2026-09-15T18:00:00.Z",
            "2026-9-15T18:00:00Z",
        ] {
            assert_eq!(seconds_since_epoch(text), None, "{text}");
        }
    }
}
