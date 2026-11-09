//! `event::calendar`: the calendar specs of the four event wall jobs and the one pure date helper
//! (Plan/22 §2.4, WP-EVENT-01, PL-100).
//!
//! The wall-clock machinery is doc 10's ([`crate::sched::wall`]): boundary-crossed evaluation in the 1 s slot, the
//! `CatchUp` rules, `MinuteCursor`, the time zone. This module supplies only **what** the event jobs fire on:
//!
//! | job | specs (shipped scripts) | `CatchUp` | body |
//! |---|---|---|---|
//! | [`SECRET_ROOM_OPEN`] | `Weekly{0, 18, 0}` (`OpenTime "Sun 18:00"`) | `Skip` | [`secret_room::on_open_time`] |
//! | [`RAINBOW_RACE_OPEN`] | one `Weekly` per `SetTime` row, shipped `[Weekly{6, 20, 30}]` | `Skip` | [`rainbow::on_open_time`] |
//! | [`TAX_PAY`] | `Daily{0, 0}` (`PayTime "00:00"`) | `RunOnceAtStart` (PL-100) | [`tax::pay_daily`] |
//! | [`TAX_CHANGE`] | `Weekly{1, 0, 0}` (`ChangeTaxTime "Mon 00:00"`) | `Skip` | [`tax::try_change`] |
//!
//! [`EventScheduleConfig::legacy`] holds the defaults of the shipped scripts. It is what `sched::registry` registers;
//! after the Lua families are up the specs read from `SecretRoom.lua`, `Tax.lua` and `RainbowRace.lua`
//! ([`EventScheduleConfig::from_scripts`](super::scripts)) replace them with [`EventScheduleConfig::apply`], and again
//! after a `/lua rr` reload (22 §2.4 rule 6). The application windows of the rainbow race are predicates on the local
//! stamp (rule 3), not jobs.

use super::scripts::{
    RainbowRaceConfig, SecretRoomConfig, TaxConfig, parse_hm, parse_weekday_time,
};
use super::{rainbow, secret_room, tax};
use crate::sched::wall::{CatchUp, WallJob, WallSpec};
use crate::sched::{DocId, Scheduler};
use smallvec::{SmallVec, smallvec};

/// The `WallJob` name of the secret-room opening (the `schedule.last_fired.<job>` key suffix, 13 §15.2).
pub const SECRET_ROOM_OPEN: &str = "secret_room_open";
/// The `WallJob` name of the rainbow-race opening.
pub const RAINBOW_RACE_OPEN: &str = "rainbow_race_open";
/// The `WallJob` name of the daily tax payout.
pub const TAX_PAY: &str = "tax_pay";
/// The `WallJob` name of the weekly tax-rate change.
pub const TAX_CHANGE: &str = "tax_change";

/// The owning document of the event jobs.
const DOC: DocId = DocId(22);

/// A spec that never fires (an empty window): the `rainbow_race_open` job of a script without `SetTime` rows, which
/// the scheduler could not hold with an empty list.
pub const NEVER: WallSpec = WallSpec::Window {
    weekday: None,
    from_hm: 0,
    to_hm: 0,
};

/// The specs of the four event wall jobs (22 §2.4), plus the rainbow application windows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventScheduleConfig {
    /// `SecretRoom.lua` `OpenTime "Sun 18:00"` -> `Weekly{weekday: 0, hour: 18, minute: 0}`.
    pub secret_room_open: WallSpec,
    /// One `Weekly` per `RainbowRace.lua` `SetTime` row (the open time); shipped `[Weekly{6, 20, 30}]`.
    pub rainbow_race_open: SmallVec<[WallSpec; 2]>,
    /// One `Window{Some(weekday), start_hm, end_hm}` per `SetTime` row; shipped `[Window{Some(6), 1200, 2000}]`.
    pub rainbow_application: SmallVec<[WallSpec; 2]>,
    /// `Tax.lua` `PayTime "00:00"` -> `Daily{hour: 0, minute: 0}`.
    pub tax_pay: WallSpec,
    /// `Tax.lua` `ChangeTaxTime "Mon 00:00"` -> `Weekly{weekday: 1, hour: 0, minute: 0}`.
    pub tax_change: WallSpec,
}

impl EventScheduleConfig {
    /// The defaults of the shipped scripts (22 §2.4 table): what the jobs fire on until the Lua families are read,
    /// and while a family is `Missing` or `Faulted`.
    pub fn legacy() -> EventScheduleConfig {
        EventScheduleConfig {
            secret_room_open: WallSpec::Weekly {
                weekday: 0,
                hour: 18,
                minute: 0,
            },
            rainbow_race_open: smallvec![WallSpec::Weekly {
                weekday: 6,
                hour: 20,
                minute: 30,
            }],
            rainbow_application: smallvec![WallSpec::Window {
                weekday: Some(6),
                from_hm: 1200,
                to_hm: 2000,
            }],
            tax_pay: WallSpec::Daily { hour: 0, minute: 0 },
            tax_change: WallSpec::Weekly {
                weekday: 1,
                hour: 0,
                minute: 0,
            },
        }
    }

    /// The schedule the typed script configs describe (22 §2.4, §2.5 rule 2): `OpenTime` -> `Weekly`, `PayTime` ->
    /// `Daily`, `ChangeTaxTime` -> `Weekly`, and per `SetTime` row of the rainbow race one `Weekly` (the open time)
    /// and one application `Window`. A string that does not parse keeps the shipped spec with a warning; a rainbow
    /// script without a `SetTime` row never opens ([`NEVER`]).
    pub fn from_configs(
        secret_room: &SecretRoomConfig,
        tax: &TaxConfig,
        rainbow: &RainbowRaceConfig,
    ) -> EventScheduleConfig {
        let legacy = EventScheduleConfig::legacy();
        let weekly = |what: &str, text: &str, fallback: WallSpec| match parse_weekday_time(text) {
            Some((weekday, hour, minute)) => WallSpec::Weekly {
                weekday,
                hour,
                minute,
            },
            None => {
                tracing::warn!(
                    what,
                    text,
                    "event schedule: unreadable time, the shipped one is used"
                );
                fallback
            }
        };
        let tax_pay = match parse_hm(&tax.pay_time) {
            Some((hour, minute)) => WallSpec::Daily { hour, minute },
            None => {
                tracing::warn!(text = %tax.pay_time, "Tax.lua PayTime is unreadable, 00:00 is used");
                legacy.tax_pay
            }
        };
        let mut open: SmallVec<[WallSpec; 2]> = SmallVec::new();
        let mut application: SmallVec<[WallSpec; 2]> = SmallVec::new();
        for (&weekday, t) in &rainbow.times {
            let (hour, minute) = (t.open_hm / 100, t.open_hm % 100);
            if hour < 24 && minute < 60 && t.start_hm <= 2400 && t.end_hm <= 2400 {
                open.push(WallSpec::Weekly {
                    weekday,
                    hour: hour as u8,
                    minute: minute as u8,
                });
                application.push(WallSpec::Window {
                    weekday: Some(weekday),
                    from_hm: t.start_hm,
                    to_hm: t.end_hm,
                });
            } else {
                tracing::warn!(
                    weekday,
                    "RainbowRace.lua: SetTime with an impossible time, the row is ignored"
                );
            }
        }
        if open.is_empty() {
            open.push(NEVER);
        }
        EventScheduleConfig {
            secret_room_open: weekly(
                "SecretRoom OpenTime",
                &secret_room.open_time,
                legacy.secret_room_open,
            ),
            rainbow_race_open: open,
            rainbow_application: application,
            tax_pay,
            tax_change: weekly("Tax ChangeTaxTime", &tax.change_tax_time, legacy.tax_change),
        }
    }

    /// Registers the four jobs with these specs (Game construction, once; 22 §2.3 item 17). The bodies are the
    /// owning WPs' (12, 13, 17): empty until they land.
    pub fn register(&self, sched: &mut Scheduler) {
        sched.register_wall(WallJob {
            name: SECRET_ROOM_OPEN,
            owner: DOC,
            specs: smallvec![self.secret_room_open],
            catch_up: CatchUp::Skip,
            run: secret_room::on_open_time,
        });
        sched.register_wall(WallJob {
            name: RAINBOW_RACE_OPEN,
            owner: DOC,
            specs: self.rainbow_race_open.clone(),
            catch_up: CatchUp::Skip,
            run: rainbow::on_open_time,
        });
        // PL-100 (approved): a 00:00 boundary passed while the server was down pays once in the first 1 s slot
        // after the start; `tax.last_paid_on` (WP-EVENT-12) keeps it from paying twice on one local date
        sched.register_wall(WallJob {
            name: TAX_PAY,
            owner: DOC,
            specs: smallvec![self.tax_pay],
            catch_up: CatchUp::RunOnceAtStart,
            run: tax::pay_daily,
        });
        // `Skip`: P5 `tax::minute` re-checks `now >= next_change(changed_at)` every minute, which is the legacy
        // catch-up (ref/15 §11)
        sched.register_wall(WallJob {
            name: TAX_CHANGE,
            owner: DOC,
            specs: smallvec![self.tax_change],
            catch_up: CatchUp::Skip,
            run: tax::try_change,
        });
    }

    /// Replaces the specs of the four registered jobs (`Scheduler::respec_wall`; the next occurrence is recomputed at
    /// the next evaluation). An empty rainbow list becomes [`NEVER`] (a job needs a spec).
    pub fn apply(&self, sched: &mut Scheduler) {
        sched.respec_wall(SECRET_ROOM_OPEN, smallvec![self.secret_room_open]);
        let open = if self.rainbow_race_open.is_empty() {
            smallvec![NEVER]
        } else {
            self.rainbow_race_open.clone()
        };
        sched.respec_wall(RAINBOW_RACE_OPEN, open);
        sched.respec_wall(TAX_PAY, smallvec![self.tax_pay]);
        sched.respec_wall(TAX_CHANGE, smallvec![self.tax_change]);
    }

    /// `true` when `local` lies inside one of the rainbow application windows (22 §14.1, a predicate: no job).
    pub fn in_rainbow_application(&self, local: &crate::tick::LocalStamp) -> bool {
        self.rainbow_application.iter().any(|w| w.in_window(local))
    }
}

impl Default for EventScheduleConfig {
    fn default() -> Self {
        EventScheduleConfig::legacy()
    }
}

/// Civil day number (proleptic Gregorian, days since 1970-01-01) of a local date: whole-day differences without any
/// zone arithmetic (the attendance roll, 22 §6.4). `month` 1-12 and `day` 1-31 are not validated; an out-of-range
/// value continues the calendar arithmetically.
pub fn days_from_civil(year: i16, month: u8, day: u8) -> i64 {
    let y = i64::from(year) - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400; // [0, 399]
    let mp = (i64::from(month) + 9) % 12; // March = 0
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}
