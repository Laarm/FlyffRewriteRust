//! Time for Lua: the [`LuaClock`] trait and the deterministic `os.date` / `os.time` / `os.clock` /
//! `os.difftime` replacements (Plan/20 §3.6, 02 §11.3).
//!
//! The scripts use `os.date("%a %H:%M")`, `os.date("%Y%m%d%H%M")`, `os.date("*t")` and `os.time{...}` (the live
//! families), always in the **server time zone**. The host never reads a system clock: the implementor of
//! [`LuaClock`] (flyff-game's per-tick sample, or `testkit::FixedClock`) supplies the wall time, the
//! game time, the civil breakdown in the server zone, the inverse `mktime`, and the minute *pin* that makes a
//! replayed minute evaluate with its own time (22 §2.4 rule 4).
//!
//! The C locale is fixed: English day and month names, no localised formats.

use flyff_common::{GameMs, UnixMs};
use mlua::{Lua, Table, Value, Variadic};
use std::rc::Rc;

/// A broken-down time (the fields of C `struct tm`, with a 1-based month and the full year).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CivilTime {
    /// Full year (`2026`).
    pub year: i64,
    /// 1-12.
    pub month: u8,
    /// 1-31.
    pub day: u8,
    /// 0-23.
    pub hour: u8,
    /// 0-59.
    pub min: u8,
    /// 0-59.
    pub sec: u8,
    /// 0 = Sunday ... 6 = Saturday (`%w`; the Lua `wday` field is this plus one).
    pub wday: u8,
    /// 1-366.
    pub yday: u16,
    /// Daylight saving time in force.
    pub isdst: bool,
}

/// The fields handed to `os.time{...}`, before normalisation (`mktime` input).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CivilFields {
    /// Full year.
    pub year: i64,
    /// Month, 1-12 (out-of-range values are normalised).
    pub month: i64,
    /// Day of the month, 1-31 (normalised: day 32 of January is February 1).
    pub day: i64,
    /// Hour.
    pub hour: i64,
    /// Minute.
    pub min: i64,
    /// Second.
    pub sec: i64,
}

/// The time source of a Lua state (Plan/20 §3.1). Implemented by flyff-game over the per-tick samples; every
/// implementation honours the pin.
pub trait LuaClock {
    /// The tick's wall-clock sample (identical for every call within one tick), or the pinned value while
    /// pinned.
    fn wall_now_ms(&self) -> UnixMs;
    /// In-game milliseconds; never affected by the pin.
    fn game_ms(&self) -> GameMs;
    /// `unix_s` broken down in the server time zone (`utc`: in UTC).
    fn to_civil(&self, unix_s: i64, utc: bool) -> CivilTime;
    #[allow(clippy::wrong_self_convention)] // the plan's name (Plan/20 §3.1): the inverse of `to_civil`
    /// `mktime`: the Unix seconds of local `f` in the server zone, fields normalised like C; `None` when the
    /// result cannot be represented.
    fn from_civil(&self, f: &CivilFields) -> Option<i64>;
    /// Makes [`LuaClock::wall_now_ms`] return `wall` until [`LuaClock::unpin`] (a second pin replaces the
    /// value). Used by the Event pass to replay a stalled minute (22 §2.4 rule 4).
    fn pin(&self, wall: UnixMs);
    /// Back to the tick sample; a no-op when not pinned.
    fn unpin(&self);
}

// ---- civil arithmetic (proleptic Gregorian, UTC) ------------------------------------------------------------

/// Days since 1970-01-01 of the civil date (`month` 1-12), Howard Hinnant's algorithm.
pub fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (month + 9) % 12; // March = 0
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The civil date `(year, month, day)` of the days since 1970-01-01.
pub fn civil_from_days(z: i64) -> (i64, u8, u8) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year, m as u8, d as u8)
}

/// `true` in a Gregorian leap year.
pub fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// `unix_s` as UTC civil time (`isdst` false). Pure helper for clock implementations.
pub fn utc_civil(unix_s: i64) -> CivilTime {
    let days = unix_s.div_euclid(86_400);
    let secs = unix_s.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let yday = days - days_from_civil(year, 1, 1) + 1;
    CivilTime {
        year,
        month,
        day,
        hour: (secs / 3600) as u8,
        min: (secs % 3600 / 60) as u8,
        sec: (secs % 60) as u8,
        wday: (days + 4).rem_euclid(7) as u8, // 1970-01-01 was a Thursday
        yday: yday as u16,
        isdst: false,
    }
}

/// `mktime` in UTC: the Unix seconds of the fields, normalised (month 13 is January of the next year, day 0
/// the last day of the previous month, 90 seconds are a minute and a half, ...). `None` on overflow.
pub fn utc_from_civil(f: &CivilFields) -> Option<i64> {
    const MAX_YEAR: i64 = 1 << 40;
    let m0 = f.month.checked_sub(1)?;
    let year = f.year.checked_add(m0.div_euclid(12))?;
    if year.abs() > MAX_YEAR || f.day.abs() > MAX_YEAR * 400 {
        return None;
    }
    let month = m0.rem_euclid(12) + 1;
    let days = days_from_civil(year, month, 1).checked_add(f.day.checked_sub(1)?)?;
    days.checked_mul(86_400)?
        .checked_add(f.hour.checked_mul(3600)?)?
        .checked_add(f.min.checked_mul(60)?)?
        .checked_add(f.sec)
}

// ---- strftime (C locale) -----------------------------------------------------------------------------------

const DAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

fn name3(table: &[&'static str], i: usize) -> &'static str {
    table.get(i).map_or("???", |n| n.get(..3).unwrap_or(n))
}

/// Formats `t` like C `strftime` in the C locale. Supported: `%a %A %b %B %c %C %d %D %e %F %h %H %I %j %m
/// %M %n %p %R %S %t %T %u %w %x %X %y %Y %%` (`%c` = `%a %b %e %H:%M:%S %Y`, `%x` = `%m/%d/%y`, `%X` =
/// `%H:%M:%S`). Any other conversion is `Err(specifier)`, like the invalid-conversion check of Lua 5.3.
pub fn strftime(fmt: &str, t: &CivilTime) -> Result<String, String> {
    let mut out = String::with_capacity(fmt.len() + 8);
    let mut chars = fmt.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let Some(spec) = chars.next() else {
            return Err("%".to_owned());
        };
        let wday = usize::from(t.wday);
        let month = usize::from(t.month.saturating_sub(1));
        let hour12 = if t.hour.is_multiple_of(12) {
            12
        } else {
            t.hour % 12
        };
        match spec {
            'a' => out.push_str(name3(&DAYS, wday)),
            'A' => out.push_str(DAYS.get(wday).copied().unwrap_or("???")),
            'b' | 'h' => out.push_str(name3(&MONTHS, month)),
            'B' => out.push_str(MONTHS.get(month).copied().unwrap_or("???")),
            'c' => out.push_str(&strftime("%a %b %e %H:%M:%S %Y", t)?),
            'C' => out.push_str(&format!("{:02}", t.year.div_euclid(100))),
            'd' => out.push_str(&format!("{:02}", t.day)),
            'D' | 'x' => out.push_str(&strftime("%m/%d/%y", t)?),
            'e' => out.push_str(&format!("{:2}", t.day)),
            'F' => out.push_str(&strftime("%Y-%m-%d", t)?),
            'H' => out.push_str(&format!("{:02}", t.hour)),
            'I' => out.push_str(&format!("{hour12:02}")),
            'j' => out.push_str(&format!("{:03}", t.yday)),
            'm' => out.push_str(&format!("{:02}", t.month)),
            'M' => out.push_str(&format!("{:02}", t.min)),
            'n' => out.push('\n'),
            'p' => out.push_str(if t.hour < 12 { "AM" } else { "PM" }),
            'R' => out.push_str(&strftime("%H:%M", t)?),
            'S' => out.push_str(&format!("{:02}", t.sec)),
            't' => out.push('\t'),
            'T' | 'X' => out.push_str(&strftime("%H:%M:%S", t)?),
            'u' => out.push_str(&(if t.wday == 0 { 7 } else { t.wday }).to_string()),
            'w' => out.push_str(&t.wday.to_string()),
            'y' => out.push_str(&format!("{:02}", t.year.rem_euclid(100))),
            'Y' => out.push_str(&t.year.to_string()),
            '%' => out.push('%'),
            other => return Err(format!("%{other}")),
        }
    }
    Ok(out)
}

// ---- the os library ----------------------------------------------------------------------------------------

fn rt(msg: impl std::fmt::Display) -> mlua::Error {
    mlua::Error::runtime(msg)
}

/// A date-table field as an integer (`getfield` of `loslib.c`): numbers with an integer value and numeric
/// strings are accepted.
fn field(t: &Table, key: &str, default: Option<i64>) -> mlua::Result<i64> {
    match t.get::<Value>(key)? {
        Value::Nil => default.ok_or_else(|| rt(format!("field '{key}' missing in date table"))),
        Value::Integer(i) => Ok(i),
        Value::Number(n) if n.fract() == 0.0 && n.abs() < 9.0e15 => Ok(n as i64),
        Value::String(s) => s
            .to_str()
            .ok()
            .and_then(|s| s.trim().parse::<f64>().ok())
            .filter(|n| n.fract() == 0.0 && n.abs() < 9.0e15)
            .map(|n| n as i64)
            .ok_or_else(|| rt(format!("field '{key}' is not an integer"))),
        _ => Err(rt(format!("field '{key}' is not an integer"))),
    }
}

/// The time argument of `os.date` / `os.difftime` (an integer or a float with an integer value).
fn time_arg(v: &Value, pos: usize, func: &str) -> mlua::Result<i64> {
    match v {
        Value::Integer(i) => Ok(*i),
        Value::Number(n) if n.fract() == 0.0 && n.abs() < 9.0e18 => Ok(*n as i64),
        Value::Number(_) => Err(rt(format!(
            "bad argument #{pos} to '{func}' (number has no integer representation)"
        ))),
        other => Err(rt(format!(
            "bad argument #{pos} to '{func}' (number expected, got {})",
            other.type_name()
        ))),
    }
}

/// The `os.date("*t")` table.
fn date_table(lua: &Lua, t: &CivilTime) -> mlua::Result<Table> {
    let tbl = lua.create_table()?;
    set_date_fields(&tbl, t)?;
    Ok(tbl)
}

fn set_date_fields(tbl: &Table, t: &CivilTime) -> mlua::Result<()> {
    tbl.set("year", t.year)?;
    tbl.set("month", i64::from(t.month))?;
    tbl.set("day", i64::from(t.day))?;
    tbl.set("hour", i64::from(t.hour))?;
    tbl.set("min", i64::from(t.min))?;
    tbl.set("sec", i64::from(t.sec))?;
    tbl.set("yday", i64::from(t.yday))?;
    tbl.set("wday", i64::from(t.wday) + 1)?;
    tbl.set("isdst", t.isdst)?;
    Ok(())
}

/// Replaces `os.date`, `os.time`, `os.clock` and `os.difftime` of `lua`; every other `os` function is removed
/// (`execute`, `exit`, `getenv`, `remove`, `rename`, `tmpname`, `setlocale`).
pub(crate) fn install_os(lua: &Lua, clock: &Rc<dyn LuaClock>) -> mlua::Result<()> {
    let os: Table = lua.globals().get("os")?;
    let keep = lua.create_table()?;

    let c = Rc::clone(clock);
    keep.set(
        "date",
        lua.create_function(move |lua, args: Variadic<Value>| {
            let fmt = match args.first() {
                None | Some(Value::Nil) => "%c".to_owned(),
                Some(Value::String(s)) => s.to_str().map_err(rt)?.to_string(),
                Some(Value::Integer(i)) => i.to_string(),
                Some(Value::Number(n)) => n.to_string(),
                Some(other) => {
                    return Err(rt(format!(
                        "bad argument #1 to 'date' (string expected, got {})",
                        other.type_name()
                    )));
                }
            };
            let secs = match args.get(1) {
                None | Some(Value::Nil) => c.wall_now_ms().0.div_euclid(1000),
                Some(v) => time_arg(v, 2, "date")?,
            };
            let (utc, body) = match fmt.strip_prefix('!') {
                Some(rest) => (true, rest),
                None => (false, fmt.as_str()),
            };
            let civil = c.to_civil(secs, utc);
            if body.starts_with("*t") {
                return Ok(Value::Table(date_table(lua, &civil)?));
            }
            match strftime(body, &civil) {
                Ok(s) => Ok(Value::String(lua.create_string(&s)?)),
                Err(spec) => Err(rt(format!(
                    "bad argument #1 to 'date' (invalid conversion specifier '{spec}')"
                ))),
            }
        })?,
    )?;

    let c = Rc::clone(clock);
    keep.set(
        "time",
        lua.create_function(move |_, arg: Option<Table>| {
            let Some(t) = arg else {
                return Ok(c.wall_now_ms().0.div_euclid(1000));
            };
            let f = CivilFields {
                year: field(&t, "year", None)?,
                month: field(&t, "month", None)?,
                day: field(&t, "day", None)?,
                hour: field(&t, "hour", Some(12))?,
                min: field(&t, "min", Some(0))?,
                sec: field(&t, "sec", Some(0))?,
            };
            // `loslib.c`: every field must fit an `int` (the year after subtracting 1900)
            for (name, v) in [
                ("month", f.month),
                ("day", f.day),
                ("hour", f.hour),
                ("min", f.min),
                ("sec", f.sec),
            ] {
                if !(i64::from(i32::MIN)..=i64::from(i32::MAX)).contains(&v) {
                    return Err(rt(format!("field '{name}' is out-of-bound")));
                }
            }
            if !(i64::from(i32::MIN) + 1900..=i64::from(i32::MAX)).contains(&f.year) {
                return Err(rt("field 'year' is out-of-bound"));
            }
            let secs = c
                .from_civil(&f)
                .ok_or_else(|| rt("time result cannot be represented in this installation"))?;
            // like `mktime`, `os.time` writes the normalised fields back into the table
            set_date_fields(&t, &c.to_civil(secs, false))?;
            Ok(secs)
        })?,
    )?;

    let c = Rc::clone(clock);
    keep.set(
        "clock",
        lua.create_function(move |_, ()| Ok(c.game_ms().0 as f64 / 1000.0))?,
    )?;

    keep.set(
        "difftime",
        lua.create_function(|_, (a, b): (Value, Value)| {
            Ok((time_arg(&a, 1, "difftime")? - time_arg(&b, 2, "difftime")?) as f64)
        })?,
    )?;

    // drop everything else, then install the four
    let mut names = Vec::new();
    os.for_each(|k: Value, _: Value| {
        if let Value::String(s) = &k {
            names.push(s.to_string_lossy());
        }
        Ok(())
    })?;
    for n in names {
        os.set(n, Value::Nil)?;
    }
    keep.for_each(|k: String, v: Value| os.set(k, v))?;
    Ok(())
}
