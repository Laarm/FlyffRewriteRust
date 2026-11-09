//! The configuration of the 1:1 guild combat (Plan/21 §8.1; `GuildCombat1to1.cpp:794-814`) and its weekly opening: the
//! globals of `GuildCombat1TO1.lua`, the wall job `gc1to1_open` with one `Weekly` spec per `OPENTIME` entry and its
//! body [`on_open_time`].
//!
//! The schedule is doc 10's wall machinery (boundary-crossed, in `server.timezone`, PL-100): nothing here compares a
//! weekday, an hour or a minute with the clock (the original's database server asked the script's `CheckOpenTime` once
//! a minute).

use super::{Gc1to1State, MAX_LINEUP_PLAYERS, MAX_TENDER_GUILDS, tender};
use crate::ctx::{GameCtx, Services};
use crate::event::WarnOnce;
use crate::event::calendar::NEVER;
use crate::event::scripts::parse_weekday_time;
use crate::sched::wall::{CatchUp, WallJob, WallSpec};
use crate::sched::{DocId, Scheduler};
use crate::tick::LocalStamp;
use flyff_common::config::EventId;
use flyff_lua::{FamilyStatus, LuaHost, ScriptFamily};
use smallvec::SmallVec;

/// The `WallJob` name (the `schedule.last_fired.<job>` key suffix, 13 §15.2).
pub const GC1TO1_OPEN: &str = "gc1to1_open";

/// The owning document.
const DOC: DocId = DocId(21);

/// The globals of `GuildCombat1TO1.lua` (the shipped values are the defaults).
#[derive(Clone, Debug, PartialEq)]
pub struct Gc1to1Config {
    /// `MinJoinPenya`: the minimum tender (110 000 000).
    pub min_join_penya: i32,
    /// `MinGuildLevel`: the minimum guild level to tender (20).
    pub min_guild_level: i32,
    /// `MinJoinGuild`: fewer tenders than this cancel the opening (2).
    pub min_join_guild: i32,
    /// `MaxJoinGuild`: the guilds that fight, the best tenders (8; at most [`MAX_TENDER_GUILDS`]).
    pub max_join_guild: i32,
    /// `MinJoinPlayer`: the smallest line-up (6; `gameplay.gc1to1_min_join_player` overrides it).
    pub min_join_player: i32,
    /// `MaxJoinPlayer`: the largest line-up (11; at most [`MAX_LINEUP_PLAYERS`]).
    pub max_join_player: i32,
    /// `MinJoinPlayerLevel`: the minimum level of a fighter (30).
    pub min_join_player_level: i32,
    /// `PlayerLife`: the lives of a fighter in one round (1).
    pub player_life: i32,
    /// `CancelReturnRate`: the percentage that a cancelled tender gets back (80).
    pub cancel_return_rate: i32,
    /// `FailReturnRate`: the percentage that a failed tender gets back (98).
    pub fail_return_rate: i32,
    /// `MemberLineUpTime` in milliseconds (3 600 000).
    pub member_line_up_time_ms: u64,
    /// `EntranceWaitTime` in milliseconds (600 000).
    pub entrance_wait_time_ms: u64,
    /// `WarWaitTime` in milliseconds (60 000).
    pub war_wait_time_ms: u64,
    /// `WarTime` in milliseconds (600 000).
    pub war_time_ms: u64,
    /// `WarCloseWaitTime` in milliseconds (30 000).
    pub war_close_wait_time_ms: u64,
    /// `WinChipNum`: `MinJoinPenya * 0.9 * 0.00001 / MaxJoinPlayer` = 90 with the shipped script (the plan's "9" is an
    /// erratum); the chips of a round are `flyff_rules::social::gc1to1::win_chips`.
    pub win_chip_num: f64,
    /// `OPENTIME`: `"Sat 17:00"` entries; none = no opening.
    pub open_time: Vec<String>,
}

impl Default for Gc1to1Config {
    fn default() -> Self {
        Gc1to1Config {
            min_join_penya: 110_000_000,
            min_guild_level: 20,
            min_join_guild: 2,
            max_join_guild: 8,
            min_join_player: 6,
            max_join_player: 11,
            min_join_player_level: 30,
            player_life: 1,
            cancel_return_rate: 80,
            fail_return_rate: 98,
            member_line_up_time_ms: 3_600_000,
            entrance_wait_time_ms: 600_000,
            war_wait_time_ms: 60_000,
            war_time_ms: 600_000,
            war_close_wait_time_ms: 30_000,
            // the value of the shipped script's expression in double precision (90.00000000000001)
            win_chip_num: 110_000_000.0 * 0.9 * 0.00001 / 11.0,
            open_time: Vec::new(),
        }
    }
}

/// The global reader of the family: a missing global or a conversion error is `None` with one warning.
struct Globals<'a> {
    lua: &'a LuaHost,
    warned: &'a WarnOnce,
}

impl Globals<'_> {
    const FAMILY: ScriptFamily = ScriptFamily::GuildCombat1to1;

    fn get<R: serde::de::DeserializeOwned>(&self, name: &str) -> Option<R> {
        match self.lua.global::<R>(Self::FAMILY, name) {
            Ok(Some(v)) => Some(v),
            Ok(None) => {
                if self.warned.first(&format!("gc1to1:missing:{name}")) {
                    tracing::warn!(
                        global = name,
                        "GuildCombat1TO1.lua does not define a global the 1:1 guild combat reads: the shipped value is used"
                    );
                }
                None
            }
            Err(e) => {
                tracing::warn!(
                    global = name,
                    error = %e,
                    "GuildCombat1TO1.lua: a global could not be read; the shipped value is used"
                );
                None
            }
        }
    }

    /// `(int)GetGlobalNumber(name)`.
    fn int(&self, name: &str, default: i32) -> i32 {
        self.get::<f64>(name).map_or(default, |v| v as i32)
    }

    /// A time in milliseconds; a negative value is 0.
    fn millis(&self, name: &str, default: u64) -> u64 {
        self.get::<f64>(name)
            .map_or(default, |v| if v > 0.0 { v as u64 } else { 0 })
    }
}

impl Gc1to1Config {
    /// Reads the globals of the family. A family that is not `Ready` or a missing global keeps the shipped value with
    /// one warning (an empty `OPENTIME` means no opening). The clamps (each with a warning): `MaxJoinGuild` at most
    /// 8 (two guilds per arena world; the original sent a fifth pair to world 245, "Not Exist World") and at least 2,
    /// `MinJoinGuild` at least 2 (a pair needs two), `MaxJoinPlayer` at most 32 (`max_len` of
    /// `Gc1to1MemberLineUp.ids`), `PlayerLife` and `MinJoinPlayer` at least 1.
    pub fn read(lua: &LuaHost, warned: &WarnOnce) -> Gc1to1Config {
        let mut c = Gc1to1Config::default();
        if lua.family_status(Globals::FAMILY) != FamilyStatus::Ready {
            if warned.first("gc1to1:family") {
                tracing::warn!(
                    family = Globals::FAMILY.logical_name(),
                    "the script family is not ready: the 1:1 guild combat uses the shipped values and never opens"
                );
            }
            return c;
        }
        let g = Globals { lua, warned };
        c.min_join_penya = g.int("MinJoinPenya", c.min_join_penya);
        c.min_guild_level = g.int("MinGuildLevel", c.min_guild_level);
        c.min_join_guild = g.int("MinJoinGuild", c.min_join_guild);
        c.max_join_guild = g.int("MaxJoinGuild", c.max_join_guild);
        c.min_join_player = g.int("MinJoinPlayer", c.min_join_player);
        c.max_join_player = g.int("MaxJoinPlayer", c.max_join_player);
        c.min_join_player_level = g.int("MinJoinPlayerLevel", c.min_join_player_level);
        c.player_life = g.int("PlayerLife", c.player_life);
        c.cancel_return_rate = g.int("CancelReturnRate", c.cancel_return_rate);
        c.fail_return_rate = g.int("FailReturnRate", c.fail_return_rate);
        c.member_line_up_time_ms = g.millis("MemberLineUpTime", c.member_line_up_time_ms);
        c.entrance_wait_time_ms = g.millis("EntranceWaitTime", c.entrance_wait_time_ms);
        c.war_wait_time_ms = g.millis("WarWaitTime", c.war_wait_time_ms);
        c.war_time_ms = g.millis("WarTime", c.war_time_ms);
        c.war_close_wait_time_ms = g.millis("WarCloseWaitTime", c.war_close_wait_time_ms);
        c.win_chip_num = g.get::<f64>("WinChipNum").unwrap_or(c.win_chip_num);
        c.open_time = g.get::<Vec<String>>("OPENTIME").unwrap_or_default();
        c.clamp();
        c
    }

    /// The clamps of [`Gc1to1Config::read`].
    fn clamp(&mut self) {
        let clamp = |name: &str, v: &mut i32, lo: i32, hi: i32| {
            let c = (*v).clamp(lo, hi);
            if c != *v {
                tracing::warn!(
                    global = name,
                    value = *v,
                    used = c,
                    "GuildCombat1TO1.lua: a value outside the range the arenas support is clamped"
                );
                *v = c;
            }
        };
        clamp(
            "MaxJoinGuild",
            &mut self.max_join_guild,
            2,
            MAX_TENDER_GUILDS,
        );
        clamp("MinJoinGuild", &mut self.min_join_guild, 2, i32::MAX);
        clamp(
            "MaxJoinPlayer",
            &mut self.max_join_player,
            1,
            MAX_LINEUP_PLAYERS,
        );
        clamp("MinJoinPlayer", &mut self.min_join_player, 1, i32::MAX);
        clamp("PlayerLife", &mut self.player_life, 1, i32::MAX);
        // a line-up of a minimum above the maximum could never be submitted
        if self.min_join_player > self.max_join_player {
            tracing::warn!(
                min = self.min_join_player,
                max = self.max_join_player,
                "GuildCombat1TO1.lua: MinJoinPlayer is above MaxJoinPlayer: the maximum is raised"
            );
            self.max_join_player = self.min_join_player.min(MAX_LINEUP_PLAYERS);
            self.min_join_player = self.max_join_player;
        }
    }

    /// `gameplay.gc1to1_min_join_player >= 1` replaces `MinJoinPlayer` (the original's test builds used 1,
    /// `GuildCombat1to1.cpp:804-806`).
    pub fn apply_overrides(&mut self, cfg: &crate::config::GameConfig) {
        let n = cfg.rules.gameplay.gc1to1_min_join_player;
        if n >= 1 {
            self.min_join_player = i32::try_from(n).unwrap_or(i32::MAX).min(MAX_LINEUP_PLAYERS);
            self.max_join_player = self.max_join_player.max(self.min_join_player);
        }
    }
}

/// The specs of `OPENTIME`: one `Weekly{weekday, hour, minute}` per entry in order (`"Sat 17:00"` is
/// `Weekly{weekday: 6, hour: 17, minute: 0}` in `server.timezone`). An entry that does not parse is skipped with a
/// warning; no valid entry gives the single spec [`NEVER`] (a job needs a spec).
pub fn specs(config: &Gc1to1Config) -> SmallVec<[WallSpec; 2]> {
    let mut out: SmallVec<[WallSpec; 2]> = SmallVec::new();
    for entry in &config.open_time {
        match parse_weekday_time(entry) {
            Some((weekday, hour, minute)) => out.push(WallSpec::Weekly {
                weekday,
                hour,
                minute,
            }),
            None => tracing::warn!(
                entry = %entry,
                "GuildCombat1TO1.lua: an OPENTIME entry is not \"Ddd HH:MM\": skipped"
            ),
        }
    }
    if out.is_empty() {
        out.push(NEVER);
    }
    out
}

/// Registers the job `gc1to1_open` (`CatchUp::Skip`: an opening missed while the server was down is not made up). The
/// specs are valid by construction, so `register_wall` cannot panic.
pub fn register(sched: &mut Scheduler, config: &Gc1to1Config) {
    sched.register_wall(WallJob {
        name: GC1TO1_OPEN,
        owner: DOC,
        specs: specs(config),
        catch_up: CatchUp::Skip,
        run: on_open_time,
    });
}

/// The body of the job: only with `events.guild_combat_1to1` on (checked here) and in state `Close`; the opening
/// itself is [`tender::open`] (also what `/gc1to1open` runs). Nothing here compares the weekday, the hour or the
/// minute (PL-100: once per crossed boundary; doc 10 writes `schedule.last_fired.gc1to1_open`).
pub fn on_open_time(svc: &mut Services, ctx: &mut GameCtx<'_>, _stamp: &LocalStamp) {
    if ctx.live.event_state(EventId::GuildCombat1to1) == 0 {
        return;
    }
    if svc.social.gc1to1.state() != Gc1to1State::Close {
        return;
    }
    let _ = tender::open(svc, ctx, false);
}
