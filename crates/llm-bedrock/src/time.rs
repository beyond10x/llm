//! UTC civil time for signing and for credential expiry: no time-zone database, no date crate.
//!
//! The day arithmetic is Howard Hinnant's `days_from_civil` and `civil_from_days`, on the
//! proleptic Gregorian calendar.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

const SECONDS_PER_DAY: i64 = 86_400;

/// `at` in the `x-amz-date` form, `20150830T123600Z`, in UTC. A time before 1970 is written as
/// the epoch: no request is signed with a clock that far wrong.
pub(crate) fn amz_date(at: SystemTime) -> String {
    let seconds = at.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let seconds = i64::try_from(seconds).unwrap_or(i64::MAX);
    let (year, month, day) = civil_from_days(seconds.div_euclid(SECONDS_PER_DAY));
    let of_day = seconds.rem_euclid(SECONDS_PER_DAY);
    let hour = of_day / 3600;
    let minute = of_day % 3600 / 60;
    let second = of_day % 60;
    format!("{year:04}{month:02}{day:02}T{hour:02}{minute:02}{second:02}Z")
}

/// Reads an RFC 3339 instant in UTC: `2026-10-22T12:00:00Z`, with an optional fraction of a
/// second (`.250`, digits past the ninth ignored) and `Z` or `+00:00` as its offset. `T` and `Z`
/// may be lower case. Any other offset, an impossible date or time, or anything around the
/// instant is `None`.
pub(crate) fn parse_rfc3339_utc(text: &str) -> Option<SystemTime> {
    let (civil, rest) = text.as_bytes().split_at_checked(19)?;
    let separators = [(4_usize, b'-'), (7, b'-'), (13, b':'), (16, b':')];
    if separators.iter().any(|&(at, byte)| civil[at] != byte) || !matches!(civil[10], b'T' | b't') {
        return None;
    }
    let year = number(&civil[..4])?;
    let month = number(&civil[5..7])?;
    let day = number(&civil[8..10])?;
    let hour = number(&civil[11..13])?;
    let minute = number(&civil[14..16])?;
    // RFC 3339 allows a leap second; it is read as the first second of the next minute.
    let second = number(&civil[17..19])?;
    if !(1..=12).contains(&month)
        || !(1..=days_in_month(year, month)).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let (nanos, zone) = fraction(rest)?;
    if !matches!(zone, b"Z" | b"z" | b"+00:00") {
        return None;
    }
    let seconds =
        days_from_civil(year, month, day) * SECONDS_PER_DAY + hour * 3600 + minute * 60 + second;
    let whole = Duration::from_secs(seconds.unsigned_abs());
    let at = if seconds >= 0 {
        UNIX_EPOCH.checked_add(whole)?
    } else {
        UNIX_EPOCH.checked_sub(whole)?
    };
    at.checked_add(Duration::from_nanos(nanos))
}

/// Nanoseconds from an optional `.digits` fraction, and the bytes after it. A dot without a
/// digit is `None`.
fn fraction(rest: &[u8]) -> Option<(u64, &[u8])> {
    let Some(digits) = rest.strip_prefix(b".") else {
        return Some((0, rest));
    };
    let count = digits
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    if count == 0 {
        return None;
    }
    let (digits, zone) = digits.split_at(count);
    let nanos = (0..9).fold(0, |nanos, position| {
        nanos * 10
            + digits
                .get(position)
                .map_or(0, |digit| u64::from(digit - b'0'))
    });
    Some((nanos, zone))
}

/// The value of a run of ASCII digits; `None` when any byte is not one.
fn number(digits: &[u8]) -> Option<i64> {
    let mut value = 0_i64;
    for &byte in digits {
        if !byte.is_ascii_digit() {
            return None;
        }
        value = value * 10 + i64::from(byte - b'0');
    }
    Some(value)
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days since 1970-01-01 of a civil date.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    // The year runs from March, so the leap day is its last day.
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let from_march = (month + 9) % 12;
    let day_of_year = (153 * from_march + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The civil date (year, month, day) of a count of days since 1970-01-01.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * from_march + 2) / 5 + 1;
    let month = if from_march < 10 {
        from_march + 3
    } else {
        from_march - 9
    };
    let year = year_of_era + era * 400;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::{amz_date, parse_rfc3339_utc};
    use std::time::{Duration, UNIX_EPOCH};

    /// The suite's signing instant, 2015-08-30T12:36:00Z (`date -u -d … +%s`).
    const SUITE: u64 = 1_440_938_160;

    #[test]
    fn amz_date_writes_utc_civil_time() {
        let cases = [
            (0, "19700101T000000Z"),
            (SUITE, "20150830T123600Z"),
            (951_868_800, "20000301T000000Z"),
            (1_709_251_199, "20240229T235959Z"),
            (4_107_542_399, "21000228T235959Z"),
        ];
        for (seconds, expected) in cases {
            assert_eq!(
                amz_date(UNIX_EPOCH + Duration::from_secs(seconds)),
                expected,
                "{seconds}"
            );
        }
        assert_eq!(
            amz_date(UNIX_EPOCH - Duration::from_secs(1)),
            "19700101T000000Z"
        );
    }

    #[test]
    fn an_rfc3339_instant_is_read_with_or_without_a_fraction_and_with_a_zero_offset() {
        let at = UNIX_EPOCH + Duration::from_secs(SUITE);
        assert_eq!(parse_rfc3339_utc("2015-08-30T12:36:00Z"), Some(at));
        assert_eq!(parse_rfc3339_utc("2015-08-30t12:36:00z"), Some(at));
        assert_eq!(parse_rfc3339_utc("2015-08-30T12:36:00+00:00"), Some(at));
        assert_eq!(
            parse_rfc3339_utc("2015-08-30T12:36:00.250Z"),
            Some(at + Duration::from_millis(250))
        );
        assert_eq!(
            parse_rfc3339_utc("2015-08-30T12:36:00.123456789987+00:00"),
            Some(at + Duration::from_nanos(123_456_789))
        );
        assert_eq!(
            parse_rfc3339_utc("2024-02-29T23:59:59Z"),
            Some(UNIX_EPOCH + Duration::from_secs(1_709_251_199))
        );
        assert_eq!(
            parse_rfc3339_utc("2015-08-30T12:35:60Z"),
            Some(at),
            "a leap second is the next minute's first"
        );
        assert_eq!(
            parse_rfc3339_utc("1969-12-31T23:59:59Z"),
            Some(UNIX_EPOCH - Duration::from_secs(1))
        );
    }

    #[test]
    fn other_offsets_impossible_dates_and_surrounding_text_are_refused() {
        for text in [
            "",
            "2015-08-30",
            "2015-08-30T12:36Z",
            "2015-08-30T12:36:00",
            "2015-08-30T12:36:00+01:00",
            "2015-08-30T12:36:00-00:00",
            "2015-08-30T12:36:00+0000",
            "2015-08-30 12:36:00Z",
            "2015/08/30T12:36:00Z",
            "2015-08-30T12:36:00.Z",
            "2015-08-30T12:36:00ZZ",
            "2015-08-30T12:36:00Z ",
            " 2015-08-30T12:36:00Z",
            "+2015-08-30T12:36:00Z",
            "2015-13-01T00:00:00Z",
            "2015-00-01T00:00:00Z",
            "2015-08-00T00:00:00Z",
            "2015-08-32T00:00:00Z",
            "2015-04-31T00:00:00Z",
            "2023-02-29T00:00:00Z",
            "2100-02-29T00:00:00Z",
            "2015-08-30T24:00:00Z",
            "2015-08-30T12:60:00Z",
            "2015-08-30T12:36:61Z",
            "2015-08-3\u{0661}T12:36:00Z",
        ] {
            assert_eq!(parse_rfc3339_utc(text), None, "{text}");
        }
    }

    #[test]
    fn writing_and_reading_are_inverse_from_1970_to_2100() {
        for day in (0..47_482_u64).step_by(13) {
            let at = UNIX_EPOCH + Duration::from_secs(day * 86_400 + day * 7_919 % 86_400);
            let written = amz_date(at);
            let rfc3339 = format!(
                "{}-{}-{}T{}:{}:{}Z",
                &written[0..4],
                &written[4..6],
                &written[6..8],
                &written[9..11],
                &written[11..13],
                &written[13..15]
            );
            assert_eq!(parse_rfc3339_utc(&rfc3339), Some(at), "{rfc3339}");
        }
    }
}
