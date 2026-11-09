//! The only wall-clock API of the server (Plan/10 §5.5, R4, PL-100, PD-08).
//!
//! Calendar schedules ("Sat 18:00", "00:00", the maintenance hour) use **boundary-crossed** checks in the
//! server time zone: a job fires once when its next occurrence falls into the interval evaluated since the
//! last evaluation, even if a forward jump crossed several. Minute jobs ([`WallSpec::EveryMinute`],
//! [`WallSpec::Window`]) run once per crossed wall minute (at most 5 replayed). All of it is evaluated in the
//! 1 s slot (item 17) from the per-tick wall-clock sample [`TickClock::unix_ms`]; nothing here reads a
//! clock. The monotonic guard (`max_seen`) makes a backwards wall jump (NTP) fire nothing.
//!
//! Time zones: `chrono-tz` (the tz database is compiled into the binary, so a static musl build and a
//! replay on another host convert identically, ADR-017); `server.timezone = "local"` is resolved once at
//! startup from `TZ`, else the host zone via `iana-time-zone`, else UTC with a warning.

use crate::ctx::{GameCtx, Services};
use crate::sched::DocId;
use crate::tick::{LocalStamp, TickClock};
use chrono::{DateTime, Datelike, LocalResult, NaiveDate, Offset, TimeZone, Timelike, Utc};
use chrono_tz::Tz;
use flyff_common::{UnixMs, UnixTime};
use smallvec::SmallVec;
use std::collections::BTreeMap;

/// A wall job body: the stamp is that of the boundary / minute that fired.
pub type WallFn = fn(&mut Services, &mut GameCtx<'_>, &LocalStamp);

/// One schedule of a [`WallJob`] (Plan/10 §5.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WallSpec {
    /// Every day at `hour:minute` local (Tax `PayTime` "00:00", the maintenance hour).
    Daily {
        /// Hour 0–23.
        hour: u8,
        /// Minute 0–59.
        minute: u8,
    },
    /// Every week on `weekday` (0 = Sunday) at `hour:minute` local ("Sun 18:00").
    Weekly {
        /// 0 = Sunday … 6 = Saturday.
        weekday: u8,
        /// Hour 0–23.
        hour: u8,
        /// Minute 0–59.
        minute: u8,
    },
    /// Every hour at `minute` past.
    Hourly {
        /// Minute 0–59.
        minute: u8,
    },
    /// Every crossed wall minute.
    EveryMinute,
    /// Every crossed wall minute `m` with [`WallSpec::in_window`]: `from_hm <= hhmm(m) < to_hm`.
    Window {
        /// Restrict to one weekday (0 = Sunday).
        weekday: Option<u8>,
        /// Inclusive start, `HHMM` (2100 = 21:00).
        from_hm: u16,
        /// Exclusive end, `HHMM`.
        to_hm: u16,
    },
}

impl WallSpec {
    /// `true` for the boundary-crossed specs (`Daily`, `Weekly`, `Hourly`).
    pub fn is_calendar(&self) -> bool {
        matches!(
            self,
            WallSpec::Daily { .. } | WallSpec::Weekly { .. } | WallSpec::Hourly { .. }
        )
    }

    /// `Window`: `(weekday None or == t.weekday) && from_hm <= t.hm() < to_hm`; calendar specs: `false`;
    /// `EveryMinute`: `true`.
    pub fn in_window(&self, t: &LocalStamp) -> bool {
        match self {
            WallSpec::Window {
                weekday,
                from_hm,
                to_hm,
            } => weekday.is_none_or(|w| w == t.weekday) && *from_hm <= t.hm() && t.hm() < *to_hm,
            WallSpec::EveryMinute => true,
            _ => false,
        }
    }

    /// A readable form: `daily 05:00`, `Sun 18:00`, `hourly :30`, `every minute`, `Sat 21:00-23:00`.
    pub fn describe(&self) -> String {
        const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
        let day = |d: u8| DAYS.get(usize::from(d)).copied().unwrap_or("?");
        let hm = |v: u16| format!("{:02}:{:02}", v / 100, v % 100);
        match *self {
            WallSpec::Daily { hour, minute } => format!("daily {hour:02}:{minute:02}"),
            WallSpec::Weekly {
                weekday,
                hour,
                minute,
            } => format!("{} {hour:02}:{minute:02}", day(weekday)),
            WallSpec::Hourly { minute } => format!("hourly :{minute:02}"),
            WallSpec::EveryMinute => "every minute".to_owned(),
            WallSpec::Window {
                weekday,
                from_hm,
                to_hm,
            } => match weekday {
                Some(d) => format!("{} {}-{} every minute", day(d), hm(from_hm), hm(to_hm)),
                None => format!("{}-{} every minute", hm(from_hm), hm(to_hm)),
            },
        }
    }

    fn validate(&self) -> bool {
        match *self {
            WallSpec::Daily { hour, minute } => hour < 24 && minute < 60,
            WallSpec::Weekly {
                weekday,
                hour,
                minute,
            } => weekday < 7 && hour < 24 && minute < 60,
            WallSpec::Hourly { minute } => minute < 60,
            WallSpec::EveryMinute => true,
            WallSpec::Window {
                weekday,
                from_hm,
                to_hm,
            } => weekday.is_none_or(|w| w < 7) && from_hm <= 2400 && to_hm <= 2400,
        }
    }
}

/// What a job does about boundaries crossed while the server was down (first evaluation only).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatchUp {
    /// Wait for the next occurrence.
    Skip,
    /// Fire once at start when `schedule.last_fired.<job>` is older than the most recent occurrence.
    /// Calendar specs only (checked at registration).
    RunOnceAtStart,
}

/// A registered wall-clock job (names = 13 §15.2 key suffixes).
#[derive(Clone, Debug)]
pub struct WallJob {
    /// Job name (`last_fired` key suffix).
    pub name: &'static str,
    /// Owning document.
    pub owner: DocId,
    /// Schedules; the job fires when any of them does.
    pub specs: SmallVec<[WallSpec; 2]>,
    /// Behaviour after downtime.
    pub catch_up: CatchUp,
    /// Body.
    pub run: WallFn,
}

/// The zone name could not be resolved.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ZoneError {
    /// Not a known IANA zone.
    #[error("unknown time zone '{0}' (use \"local\" or an IANA name such as \"Europe/Berlin\")")]
    Unknown(String),
}

/// The server time zone: the only zone-aware code on the game thread.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WallZone {
    tz: Tz,
    name: String,
}

impl WallZone {
    /// UTC.
    pub fn utc() -> WallZone {
        WallZone {
            tz: Tz::UTC,
            name: "UTC".to_owned(),
        }
    }

    /// An IANA zone by name (`"Europe/Berlin"`, `"UTC"`).
    pub fn from_iana(name: &str) -> Result<WallZone, ZoneError> {
        let tz: Tz = name
            .parse()
            .map_err(|_| ZoneError::Unknown(name.to_owned()))?;
        Ok(WallZone {
            tz,
            name: tz.name().to_owned(),
        })
    }

    /// Resolves `server.timezone` (startup only, §8.4 cross-key check): `"local"` through `TZ` and the host
    /// zone (`iana-time-zone`), anything else as an IANA name. Falls back to UTC with a warning when the
    /// host zone cannot be determined.
    #[allow(clippy::disallowed_methods)] // startup-only zone resolution (Plan/10 §5.5 rule 7)
    pub fn resolve(cfg_timezone: &str) -> Result<WallZone, ZoneError> {
        let tz_env = std::env::var("TZ").ok(); // det-ok: startup-only zone resolution (Plan/10 §5.5 rule 7)
        let (zone, warning) = Self::resolve_with(cfg_timezone, tz_env.as_deref(), || {
            iana_time_zone::get_timezone().ok()
        })?;
        if let Some(w) = warning {
            tracing::warn!("{w}");
        }
        Ok(zone)
    }

    /// [`Self::resolve`] with the environment injected: `tz_env` is the value of `TZ`, `host_zone` yields
    /// the host's zone name. Returns the zone and an optional warning (UTC fallback).
    pub fn resolve_with(
        cfg_timezone: &str,
        tz_env: Option<&str>,
        host_zone: impl FnOnce() -> Option<String>,
    ) -> Result<(WallZone, Option<String>), ZoneError> {
        let wanted = cfg_timezone.trim();
        if !wanted.eq_ignore_ascii_case("local") {
            return Self::from_iana(wanted).map(|z| (z, None));
        }
        // `TZ` may carry the POSIX leading colon (":Europe/Berlin").
        if let Some(name) = tz_env
            .map(|t| t.trim().trim_start_matches(':'))
            .filter(|t| !t.is_empty())
            && let Ok(zone) = Self::from_iana(name)
        {
            return Ok((zone, None));
        }
        if let Some(name) = host_zone()
            && let Ok(zone) = Self::from_iana(name.trim())
        {
            return Ok((zone, None));
        }
        Ok((
            WallZone::utc(),
            Some(
                "server.timezone = \"local\" could not be resolved from TZ or the host: using UTC"
                    .to_owned(),
            ),
        ))
    }

    /// The canonical IANA name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// `t` broken down in this zone.
    pub fn local(&self, t: UnixMs) -> LocalStamp {
        let utc = DateTime::<Utc>::from_timestamp_millis(t.0).unwrap_or(DateTime::UNIX_EPOCH);
        let l = utc.with_timezone(&self.tz);
        LocalStamp {
            year: i16::try_from(l.year()).unwrap_or(1970),
            month: u8::try_from(l.month()).unwrap_or(1),
            day: u8::try_from(l.day()).unwrap_or(1),
            hour: u8::try_from(l.hour()).unwrap_or(0),
            minute: u8::try_from(l.minute()).unwrap_or(0),
            second: u8::try_from(l.second()).unwrap_or(0),
            weekday: u8::try_from(l.weekday().num_days_from_sunday()).unwrap_or(0),
            utc_offset_s: l.offset().fix().local_minus_utc(),
            unix_s: t.0.div_euclid(1000),
        }
    }

    /// The first occurrence strictly after `after` of a calendar spec; `None` for minute specs.
    /// A time that does not exist on a day (spring-forward gap) is at the first instant after the gap, an
    /// ambiguous one (fall-back) at the earlier instant (rule 6).
    pub fn next_occurrence(&self, spec: &WallSpec, after: UnixMs) -> Option<UnixMs> {
        match *spec {
            WallSpec::Daily { hour, minute } => self.scan_dates(after, true, None, hour, minute),
            WallSpec::Weekly {
                weekday,
                hour,
                minute,
            } => self.scan_dates(after, true, Some(weekday), hour, minute),
            WallSpec::Hourly { minute } => self.scan_minutes(after, true, minute),
            WallSpec::EveryMinute | WallSpec::Window { .. } => None,
        }
    }

    /// The latest occurrence at or before `at` of a calendar spec (catch-up); `None` for minute specs.
    pub fn prev_occurrence(&self, spec: &WallSpec, at: UnixMs) -> Option<UnixMs> {
        match *spec {
            WallSpec::Daily { hour, minute } => self.scan_dates(at, false, None, hour, minute),
            WallSpec::Weekly {
                weekday,
                hour,
                minute,
            } => self.scan_dates(at, false, Some(weekday), hour, minute),
            WallSpec::Hourly { minute } => self.scan_minutes(at, false, minute),
            WallSpec::EveryMinute | WallSpec::Window { .. } => None,
        }
    }

    /// Walks the local dates around `from` (forward: first occurrence `> from`; backward: last `<= from`).
    fn scan_dates(
        &self,
        from: UnixMs,
        forward: bool,
        weekday: Option<u8>,
        hour: u8,
        minute: u8,
    ) -> Option<UnixMs> {
        let utc = DateTime::<Utc>::from_timestamp_millis(from.0)?;
        let start: NaiveDate = utc.with_timezone(&self.tz).date_naive();
        let span: i64 = if weekday.is_some() { 9 } else { 4 };
        for step in 0..=span {
            let date = if forward {
                start.checked_add_signed(chrono::Duration::days(step))?
            } else {
                start.checked_sub_signed(chrono::Duration::days(step))?
            };
            if let Some(w) = weekday
                && date.weekday().num_days_from_sunday() != u32::from(w)
            {
                continue;
            }
            let Some(at) = self.occurrence_on(date, hour, minute) else {
                continue;
            };
            let ok = if forward { at > from } else { at <= from };
            if ok {
                return Some(at);
            }
        }
        None
    }

    /// The instant of `hour:minute` local on `date`, with the DST rules.
    fn occurrence_on(&self, date: NaiveDate, hour: u8, minute: u8) -> Option<UnixMs> {
        let naive = date.and_hms_opt(u32::from(hour), u32::from(minute), 0)?;
        match self.tz.from_local_datetime(&naive) {
            LocalResult::Single(dt) => Some(UnixMs(dt.timestamp_millis())),
            LocalResult::Ambiguous(earlier, _later) => Some(UnixMs(earlier.timestamp_millis())),
            LocalResult::None => {
                // Spring-forward gap: the first UTC instant whose local time is >= `naive`. Local time is
                // monotonic around a single transition and offsets lie in -12h..=+14h, so a window of
                // +-26h around `naive` read as UTC brackets the answer.
                let centre = naive.and_utc().timestamp();
                let (mut lo, mut hi) = (centre - 26 * 3600, centre + 26 * 3600);
                while hi - lo > 1 {
                    let mid = lo + (hi - lo) / 2;
                    let local = DateTime::<Utc>::from_timestamp(mid, 0)?
                        .with_timezone(&self.tz)
                        .naive_local();
                    if local >= naive {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
                hi.checked_mul(1000).map(UnixMs)
            }
        }
    }

    /// Hourly specs: steps real minutes (every real instant is considered once).
    fn scan_minutes(&self, from: UnixMs, forward: bool, minute: u8) -> Option<UnixMs> {
        let base = from.0.div_euclid(60_000).checked_mul(60_000)?;
        for k in 0..=61i64 {
            let t = if forward {
                base.checked_add((k + 1).checked_mul(60_000)?)?
            } else {
                base.checked_sub(k.checked_mul(60_000)?)?
            };
            if self.local(UnixMs(t)).minute == minute {
                return Some(UnixMs(t));
            }
        }
        None
    }
}

/// Crossed wall minutes since the previous call (used by slot item 17 for minute specs and by P9's Lua
/// pass, 22 §2.4).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MinuteCursor {
    last_minute: Option<i64>,
}

impl MinuteCursor {
    /// How many crossed minutes are replayed at most.
    pub const MAX_REPLAY: usize = 5;

    /// A cursor that has not seen any minute yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// The minute indices (`unix_ms / 60 000`) crossed since the previous call, ascending: the current
    /// minute on the first call; nothing while the clock stands still or runs backwards; at most the 5
    /// most recent after a forward jump (a warning is logged for the rest).
    pub fn crossed(&mut self, now: UnixMs) -> SmallVec<[i64; 5]> {
        let minute = now.0.div_euclid(60_000);
        let mut out: SmallVec<[i64; 5]> = SmallVec::new();
        match self.last_minute {
            None => out.push(minute),
            Some(last) if minute > last => {
                let total = minute - last;
                let skipped = total.saturating_sub(Self::MAX_REPLAY as i64);
                if skipped > 0 {
                    tracing::warn!("wall clock jumped forward, skipped {skipped} minute(s)");
                }
                let first = last + 1 + skipped.max(0);
                out.extend(first..=minute);
            }
            Some(_) => return out,
        }
        self.last_minute = Some(minute);
        out
    }
}

/// `schedule.last_fired.<job>` storage (13 §15.2). The `server_kv` implementation is wired by WP-OPS-07.
pub trait WallStore: Send {
    /// When the job last fired (`None` = never).
    fn last_fired(&self, job: &str) -> Option<UnixTime>;
    /// Records a calendar firing.
    fn set_last_fired(&mut self, job: &str, at: UnixTime);
}

/// In-memory [`WallStore`] (tests, and the default until persistence is wired).
#[derive(Clone, Debug, Default)]
pub struct MemWallStore {
    map: BTreeMap<String, UnixTime>,
}

impl MemWallStore {
    /// Pre-loads one job's last firing (`StartupState.server_kv`).
    pub fn with(mut self, job: &str, at: UnixTime) -> Self {
        self.map.insert(job.to_owned(), at);
        self
    }
}

impl WallStore for MemWallStore {
    fn last_fired(&self, job: &str) -> Option<UnixTime> {
        self.map.get(job).copied()
    }

    fn set_last_fired(&mut self, job: &str, at: UnixTime) {
        self.map.insert(job.to_owned(), at);
    }
}

/// One due wall job: the runtime calls `run(services, ctx, &stamp)`.
#[derive(Clone, Copy, Debug)]
pub struct WallFire {
    /// Job name.
    pub name: &'static str,
    /// Body.
    pub run: WallFn,
    /// Stamp of the boundary or minute that fired.
    pub stamp: LocalStamp,
}

struct WallEntry {
    job: WallJob,
    /// Earliest next occurrence over the calendar specs (`None` until computed).
    next_due: Option<UnixMs>,
}

/// The evaluation state of all wall jobs (owned by the `Scheduler`).
pub(crate) struct WallEngine {
    zone: WallZone,
    store: Box<dyn WallStore>,
    entries: Vec<WallEntry>,
    max_seen: Option<UnixMs>,
    cursor: MinuteCursor,
    first_done: bool,
}

impl WallEngine {
    pub(crate) fn new(zone: WallZone, store: Box<dyn WallStore>) -> Self {
        Self {
            zone,
            store,
            entries: Vec::new(),
            max_seen: None,
            cursor: MinuteCursor::new(),
            first_done: false,
        }
    }

    pub(crate) fn zone(&self) -> &WallZone {
        &self.zone
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn register(&mut self, job: WallJob) {
        assert!(
            !job.specs.is_empty(),
            "wall job '{}' needs at least one spec",
            job.name
        );
        assert!(
            job.specs.iter().all(WallSpec::validate),
            "wall job '{}' has an invalid spec",
            job.name
        );
        assert!(
            job.catch_up == CatchUp::Skip || job.specs.iter().all(WallSpec::is_calendar),
            "wall job '{}': RunOnceAtStart is only allowed with calendar specs",
            job.name
        );
        assert!(
            self.entries.iter().all(|e| e.job.name != job.name),
            "wall job '{}' registered twice",
            job.name
        );
        self.entries.push(WallEntry {
            job,
            next_due: None,
        });
    }

    pub(crate) fn has(&self, name: &str) -> bool {
        self.entries.iter().any(|e| e.job.name == name)
    }

    pub(crate) fn body(&self, name: &str) -> Option<WallFn> {
        self.entries
            .iter()
            .find(|e| e.job.name == name)
            .map(|e| e.job.run)
    }

    /// `(name, owner, schedule text)` of every job in registration order.
    pub(crate) fn describe(&self) -> Vec<(&'static str, DocId, String)> {
        self.entries
            .iter()
            .map(|e| {
                let specs: Vec<String> = e.job.specs.iter().map(WallSpec::describe).collect();
                (e.job.name, e.job.owner, specs.join(" + "))
            })
            .collect()
    }

    pub(crate) fn next_fire(&self, name: &str) -> Option<UnixMs> {
        self.entries
            .iter()
            .find(|e| e.job.name == name)
            .and_then(|e| e.next_due)
    }

    pub(crate) fn respec(&mut self, name: &str, specs: SmallVec<[WallSpec; 2]>) {
        let valid = !specs.is_empty() && specs.iter().all(WallSpec::validate);
        if !valid {
            tracing::warn!(job = name, "respec_wall ignored: invalid specs");
            return;
        }
        if let Some(e) = self.entries.iter_mut().find(|e| e.job.name == name) {
            if e.job.catch_up == CatchUp::RunOnceAtStart && !specs.iter().all(WallSpec::is_calendar)
            {
                tracing::warn!(
                    job = name,
                    "respec_wall ignored: RunOnceAtStart needs calendar specs"
                );
                return;
            }
            e.job.specs = specs;
            e.next_due = None;
        }
    }

    /// Slot item 17: evaluates every job at `clock.unix_ms` (rules 1–6) and returns the due bodies in
    /// registration order.
    pub(crate) fn evaluate(&mut self, clock: &TickClock) -> SmallVec<[WallFire; 4]> {
        let now = clock.unix_ms;
        let mut fires: SmallVec<[WallFire; 4]> = SmallVec::new();
        let old = match self.max_seen {
            None => UnixMs(now.0.saturating_sub(1000)),
            Some(m) => {
                if now <= m {
                    // Rule 1, monotonic guard: a backwards wall jump never re-fires a boundary.
                    return fires;
                }
                m
            }
        };
        self.max_seen = Some(now);
        let first = !self.first_done;
        self.first_done = true;
        let minutes = self.cursor.crossed(now);

        let zone = &self.zone;
        let store = &mut self.store;
        for entry in &mut self.entries {
            let job = &entry.job;
            let mut calendar_fired = false;
            // Rule 5: catch-up, first evaluation only.
            if first && job.catch_up == CatchUp::RunOnceAtStart {
                let recent = job
                    .specs
                    .iter()
                    .filter_map(|s| zone.prev_occurrence(s, now))
                    .max();
                if let (Some(recent), Some(last)) = (recent, store.last_fired(job.name))
                    && last.to_unix_ms() < recent
                {
                    fires.push(WallFire {
                        name: job.name,
                        run: job.run,
                        stamp: zone.local(recent),
                    });
                    store.set_last_fired(job.name, recent.to_unix_time());
                    calendar_fired = true;
                    entry.next_due = job
                        .specs
                        .iter()
                        .filter_map(|s| zone.next_occurrence(s, now))
                        .min();
                }
            }
            // Rule 3: calendar specs, at most one firing per evaluation.
            if !calendar_fired && job.specs.iter().any(WallSpec::is_calendar) {
                let due = match entry.next_due {
                    Some(d) => Some(d),
                    None => job
                        .specs
                        .iter()
                        .filter_map(|s| zone.next_occurrence(s, old))
                        .min(),
                };
                entry.next_due = due;
                if let Some(d) = due
                    && d <= now
                {
                    fires.push(WallFire {
                        name: job.name,
                        run: job.run,
                        stamp: zone.local(d),
                    });
                    store.set_last_fired(job.name, d.to_unix_time());
                    entry.next_due = job
                        .specs
                        .iter()
                        .filter_map(|s| zone.next_occurrence(s, now))
                        .min();
                }
            }
            // Rule 4: minute specs, once per crossed minute.
            if job.specs.iter().any(|s| !s.is_calendar()) {
                for &m in &minutes {
                    let stamp = zone.local(UnixMs(m.saturating_mul(60_000)));
                    let hit = job
                        .specs
                        .iter()
                        .any(|s| !s.is_calendar() && s.in_window(&stamp));
                    if hit {
                        fires.push(WallFire {
                            name: job.name,
                            run: job.run,
                            stamp,
                        });
                    }
                }
            }
        }
        fires
    }
}
