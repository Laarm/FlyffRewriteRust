//! `GameLuaClock`: the [`LuaClock`] of the game thread's Lua families (Plan/20 §3.6, 22 §2.4 rule 4).
//!
//! The game samples it **once per tick** from the tick clock ([`GameLuaClock::sample`], called at the start of
//! every tick), so every `os.time()` / `os.date()` within one tick returns the same value. The calendar
//! breakdown is done in the server time zone (`server.timezone`, [`WallZone`]); the inverse (`mktime`) walks
//! the zone offset twice, which resolves everything except the instants of a DST gap or overlap (there it
//! picks one of the two candidates, as C does). `isdst` is reported as `false` (the scripts never read it and
//! `os.time` ignores it).
//!
//! The Event pass pins the clock to each crossed wall minute ([`LuaClock::pin`]) so a minute replayed after a
//! stall is evaluated with its own time; nothing else pins it.

use crate::sched::wall::WallZone;
use flyff_common::{GameMs, UnixMs};
use flyff_lua::LuaClock;
use flyff_lua::clock::{CivilFields, CivilTime, days_from_civil, utc_civil, utc_from_civil};
use std::cell::Cell;
use std::rc::Rc;

/// The clock behind `os.date` / `os.time` / `os.clock` of the Lua families.
#[derive(Debug)]
pub struct GameLuaClock {
    zone: WallZone,
    wall: Cell<UnixMs>,
    game: Cell<GameMs>,
    pin: Cell<Option<UnixMs>>,
}

impl GameLuaClock {
    /// A clock in `zone`, at wall time 0 until the first [`GameLuaClock::sample`].
    pub fn new(zone: WallZone) -> Rc<GameLuaClock> {
        Rc::new(GameLuaClock {
            zone,
            wall: Cell::new(UnixMs(0)),
            game: Cell::new(GameMs::ZERO),
            pin: Cell::new(None),
        })
    }

    /// The per-tick sample (the tick clock's `unix_ms` and `game_ms`).
    pub fn sample(&self, wall: UnixMs, game: GameMs) {
        self.wall.set(wall);
        self.game.set(game);
    }

    /// The pinned wall time, if any (tests).
    pub fn pinned(&self) -> Option<UnixMs> {
        self.pin.get()
    }
}

impl LuaClock for GameLuaClock {
    fn wall_now_ms(&self) -> UnixMs {
        self.pin.get().unwrap_or_else(|| self.wall.get())
    }

    fn game_ms(&self) -> GameMs {
        self.game.get()
    }

    fn to_civil(&self, unix_s: i64, utc: bool) -> CivilTime {
        if utc {
            return utc_civil(unix_s);
        }
        let ms = unix_s.saturating_mul(1000);
        let local = self.zone.local(UnixMs(ms));
        let (year, month, day) = (
            i64::from(local.year),
            i64::from(local.month),
            i64::from(local.day),
        );
        let yday = days_from_civil(year, month, day) - days_from_civil(year, 1, 1) + 1;
        CivilTime {
            year,
            month: local.month,
            day: local.day,
            hour: local.hour,
            min: local.minute,
            sec: local.second,
            wday: local.weekday,
            yday: u16::try_from(yday).unwrap_or(1),
            isdst: false,
        }
    }

    fn from_civil(&self, f: &CivilFields) -> Option<i64> {
        // the fields read as UTC, then shifted by the zone offset in force at the result
        let naive = utc_from_civil(f)?;
        let off1 = i64::from(
            self.zone
                .local(UnixMs(naive.checked_mul(1000)?))
                .utc_offset_s,
        );
        let guess = naive.checked_sub(off1)?;
        let off2 = i64::from(
            self.zone
                .local(UnixMs(guess.checked_mul(1000)?))
                .utc_offset_s,
        );
        naive.checked_sub(off2)
    }

    fn pin(&self, wall: UnixMs) {
        self.pin.set(Some(wall));
    }

    fn unpin(&self) {
        self.pin.set(None);
    }
}
