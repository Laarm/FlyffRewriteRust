//! Proleptic-Gregorian calendar arithmetic on Unix times (no clock, no time-zone database).
//!
//! The persistence layer needs three calendar views of an instant: the UTC date of an audit line (daily
//! rotation, Plan/13 §25.1), the UTC stamp in a backup file name (§2.1) and the *local* calendar day and ISO week of
//! a backup for the grandfather-father-son rotation (§21.2). The local views take the zone's UTC offset at that
//! instant as a plain number of seconds, so the zone database stays in `flyff-server` and tests inject fixed
//! offsets. The conversions are the algorithms of H. Hinnant's `days_from_civil` / `civil_from_days`.

use flyff_common::UnixTime;

/// Seconds per day.
pub const DAY_S: i64 = 86_400;

/// Days since 1970-01-01 of the civil date `year-month-day` (`month` 1..=12, `day` 1..=31).
pub fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = i64::from((month + 9) % 12);
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The civil date `(year, month, day)` of `days` since 1970-01-01.
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let month = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// `YYYY-MM-DD` of the UTC day with index `days`.
pub fn date_string(days: i64) -> String {
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Day index (days since 1970-01-01) of a Unix time in milliseconds, UTC.
pub fn utc_day_of_ms(ts_ms: i64) -> i64 {
    ts_ms.div_euclid(1000).div_euclid(DAY_S)
}

/// Parses `YYYY-MM-DD` into a day index; `None` for anything else (including impossible dates).
pub fn parse_date(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    if bytes.len() != 10 || bytes.get(4) != Some(&b'-') || bytes.get(7) != Some(&b'-') {
        return None;
    }
    let year: i64 = text.get(0..4)?.parse().ok()?;
    let month: u32 = text.get(5..7)?.parse().ok()?;
    let day: u32 = text.get(8..10)?.parse().ok()?;
    valid_civil(year, month, day).then(|| days_from_civil(year, month, day))
}

/// `true` when the civil date exists.
fn valid_civil(year: i64, month: u32, day: u32) -> bool {
    (1..=12).contains(&month)
        && day >= 1
        && civil_from_days(days_from_civil(year, month, day)) == (year, month, day)
}

/// Parses the `YYYYMMDD-HHMMSS` stamp of a backup file name (UTC) into a Unix time.
pub fn parse_compact(stamp: &str) -> Option<UnixTime> {
    let bytes = stamp.as_bytes();
    if bytes.len() != 15 || bytes.get(8) != Some(&b'-') {
        return None;
    }
    if !bytes
        .iter()
        .enumerate()
        .all(|(i, b)| i == 8 || b.is_ascii_digit())
    {
        return None;
    }
    let year: i64 = stamp.get(0..4)?.parse().ok()?;
    let month: u32 = stamp.get(4..6)?.parse().ok()?;
    let day: u32 = stamp.get(6..8)?.parse().ok()?;
    let hour: i64 = stamp.get(9..11)?.parse().ok()?;
    let minute: i64 = stamp.get(11..13)?.parse().ok()?;
    let second: i64 = stamp.get(13..15)?.parse().ok()?;
    if !valid_civil(year, month, day) || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    Some(UnixTime(
        days_from_civil(year, month, day) * DAY_S + hour * 3600 + minute * 60 + second,
    ))
}

/// Local calendar day index of `t` for a zone whose UTC offset at that instant is `utc_offset_s`.
pub fn local_day(t: UnixTime, utc_offset_s: i32) -> i64 {
    (t.0 + i64::from(utc_offset_s)).div_euclid(DAY_S)
}

/// ISO-8601 week `(iso_year, week 1..=53)` of the day index `days`.
pub fn iso_week(days: i64) -> (i64, u32) {
    // Monday = 0 ... Sunday = 6 (1970-01-01 was a Thursday)
    let weekday = (days + 3).rem_euclid(7);
    // the Thursday of this ISO week decides the year
    let thursday = days - weekday + 3;
    let (year, _, _) = civil_from_days(thursday);
    let week = (thursday - days_from_civil(year, 1, 1)) / 7 + 1;
    (year, u32::try_from(week).unwrap_or(1))
}
