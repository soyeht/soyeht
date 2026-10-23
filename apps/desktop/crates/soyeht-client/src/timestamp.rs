//! The engine's timestamps: ISO 8601 with a time zone designator, with or without fractional
//! seconds (`2026-09-15T18:00:00.000Z`). The Mac reads them with `ISO8601DateFormatter`.

use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, SecondsFormat};

/// Seconds since the Unix epoch of a timestamp, or `None` when `value` is not one.
pub fn seconds_since_epoch(value: &str) -> Option<f64> {
    let Ok(parsed) = DateTime::parse_from_rfc3339(value)
        .or_else(|_| DateTime::parse_from_rfc3339(&with_offset_colon(value)))
    else {
        return None;
    };
    Some(parsed.timestamp() as f64 + f64::from(parsed.timestamp_subsec_nanos()) / 1e9)
}

/// A timestamp of `seconds` since the Unix epoch, in UTC, to the second: the spelling the
/// engine uses.
pub fn format(seconds: i64) -> String {
    DateTime::from_timestamp(seconds, 0)
        .unwrap_or_default()
        .to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// Now, in the same unit as [`seconds_since_epoch`].
pub fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |elapsed| elapsed.as_secs_f64())
}

/// `text` with a colon in a trailing `+hhmm` or `-hhmm` offset, which RFC 3339 requires and
/// ISO 8601 does not; any other text as it is.
fn with_offset_colon(text: &str) -> String {
    let colonless = text.len().checked_sub(5).and_then(|cut| {
        let (clock, offset) = (text.get(..cut)?, text.get(cut..)?);
        let digits = offset.strip_prefix(['+', '-'])?;
        let numbered = digits.bytes().all(|byte| byte.is_ascii_digit());
        numbered.then(|| format!("{clock}{}:{}", &offset[..3], &digits[2..]))
    });
    colonless.unwrap_or_else(|| text.to_owned())
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

    #[test]
    fn text_with_multibyte_characters_near_the_zone_is_refused_not_split() {
        for text in [
            "2026-09-15T18:00:00ééé",
            "é+0230",
            "2026-09-15T18:00:00+é30",
        ] {
            assert_eq!(seconds_since_epoch(text), None, "{text}");
        }
    }
}
