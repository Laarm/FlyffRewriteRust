//! `event`: the events and content module of doc 22 (Plan/22). Only the Event.lua part exists so far:
//!
//! * [`EventFactors`] / [`EventModifiers`] / [`NoEvents`]: the factor queries of the running events, with the
//!   repack's rates behind them and a neutral 1.0 default for every consumer (EXP, drops, gold, ATK/DEF, shop),
//! * [`EventLuaService`]: the minute-boundary evaluation of `Event.lua` on the `Event` Lua family (the P9
//!   scheduler job [`event_minute`]), the cached aggregates, the level-up gifts and the Lua event items,
//! * [`GameLuaClock`]: the per-tick time source of the Lua families,
//! * [`monsters`] / [`rangda`]: the `EventMonster.lua` monsters (the KQ-32 level-gap miss and the claim exemption)
//!   and the four roaming Rangda monsters on their 1 Hz controller (WP-EVENT-09).
//!
//! Wiring: `Game::new` boots the Lua families from `GameData.lua.live_scripts` with a [`GameLuaClock`], runs the
//! first Event.lua pass before tick 0 (so the rates hold from the start), and every tick samples the clock
//! before anything else runs. `Services.event.lua_events.modifiers()` is what the progression, drop and combat
//! code reads; `AdminCmd::ReloadScripts` for a family target reloads the script and re-runs the pass
//! (`Game::admin_reload_scripts`).

pub mod calendar;
pub mod combat;
pub mod desc;

pub mod drops;

pub mod flags;
pub mod game_rate;
pub mod generic;
pub mod gifts;
pub mod guild_house;
pub mod hooks;

pub mod housing;
pub mod instance;

pub mod join;
pub mod lord;
pub mod lua_clock;
pub mod lua_events;
pub mod minigame;
pub mod misc;

pub mod modifiers;
// ---- event monsters (WP-EVENT-09)

pub mod monsters;
pub mod notice;
pub mod payout;
pub mod quiz;
pub mod rainbow;
pub mod rangda;

pub mod scripts;
pub mod secret_room;

pub mod spawns;
pub mod tax;

pub mod weather;

pub use calendar::{EventScheduleConfig, days_from_civil};
pub use desc::{DescBoard, EventSecondHooks, resend_lua_desc};
pub use flags::{EventFlags, FlagError, FlagOrigin, set_flag};
pub use gifts::on_level_up;
// ---- guild house (WP-EVENT-14/15) ----
pub use guild_house::{GhFurniture, GhOp, GuildHouse, GuildHouseService};
// ---- housing (WP-EVENT-16) ----
pub use housing::{Housing, HousingItem, HousingService};
pub use instance::{
    DungeonState, InstanceDungeonService, InstanceWorldSet, PortalOutcome, on_party_left,
};

pub use lua_clock::GameLuaClock;
pub use lua_events::{EventChange, EventLuaService, LevelUpGift, MinuteReport, RunningEvent};
pub use modifiers::{EventFactors, EventModifiers, NoEvents};
pub use monsters::EventMonsterTable;
pub use quiz::{QuizPhase, QuizService};
pub use rangda::{RangdaService, RangdaState, RangdaStatus};
pub use scripts::{
    EventConfigs, EventScripts, RainbowRaceConfig, SecretRoomConfig, TaxConfig, WarnOnce,
};
// ---- secret room (doc 22 §11, task secret-room) ----
pub use secret_room::{SecretRoomService, SrMngState, SrSideState, SrWarState};

use crate::ctx::{GameCtx, Services};
use crate::input::AdminOutput;
use crate::sched::Scheduler;
use crate::tick::Game;
use flyff_data::GameData;
use flyff_lua::{FamilyStatus, LuaHost, ReloadTarget, ScriptFamily, ScriptSources};

/// The event services (`Services.event`, Plan/22 §2.2). The other services of the module arrive with their
/// work packages.
#[derive(Debug, Default)]
pub struct EventServices {
    /// The event-flag store `g_eLocal` (22 §3): what the NPC load filter, `Gate::Event`, the join batch and
    /// `GetLocalEventState` read. Initialised from the start-up `LiveConfig` by `Game::new`.
    pub flags: EventFlags,
    /// The Event.lua engine.
    pub lua_events: EventLuaService,
    /// The typed configs of `SecretRoom.lua`, `Tax.lua` and `RainbowRace.lua` and the schedule derived from them
    /// (22 §2.5 rule 2); the defaults until [`EventServices::read_configs`] ran.
    pub configs: EventConfigs,
    /// The Event.lua spawn events (22 §4.10, WP-EVENT-06).
    pub spawns: spawns::EventSpawnService,
    /// The instance dungeons (22 §8, WP-EVENT-08): memory only (KQ-21).
    pub instances: InstanceDungeonService,
    /// The `propEvent.inc` generic events: flag timer, factors, paced item drops and item spawns (22 §6, WP-EVENT-07).
    pub generic: generic::GenericEventService,
    /// The NPC event mini-games: the per-player state of Kawibawibo, Reassemble, find-word and Five-system (22 §16,
    /// WP-EVENT-20).
    pub minigames: minigame::NpcMiniGameService,
    /// The lord election, the lord events and the lord skills (22 §9, WP-EVENT-10/11).
    pub lord: lord::LordService,
    /// Tax collection, the daily payout, the weekly rate change and the Heaven Tower (22 §10, WP-EVENT-12).
    pub tax: tax::TaxService,
    /// The quiz event (22 §15, WP-EVENT-19): memory only (E9).
    pub quiz: QuizService,
    // ---- secret room (doc 22 §11, task secret-room) ----
    /// The secret room: tenders, line-ups, the manager and its two sides (22 §11, WP-EVENT-13).
    pub secret_room: SecretRoomService,
    // ---- guild house (WP-EVENT-14/15) ----
    /// The small guild houses and their furniture (22 §12, WP-EVENT-14); the rest points live in `PlayerMisc`.
    pub guild_houses: GuildHouseService,
    // ---- housing (WP-EVENT-16) ----
    /// The mini rooms of the characters in the world (22 §13, WP-EVENT-16).
    pub housing: HousingService,
    // ---- rainbow race (doc 22 §14, task rainbow-race) ----
    /// The rainbow race: the manager, the applicants and racers, the prizes (22 §14, WP-EVENT-17/18); memory only.
    pub rainbow: rainbow::RainbowRaceService,
    // ---- event monsters (WP-EVENT-09)
    /// The `EventMonster.lua` monsters (22 §7.1): `monsters.event_rule(mover)`; static, built by `Game::new`.
    pub monsters: EventMonsterTable,
    /// The four roaming Rangda monsters on their 1 Hz controller (22 §7.2): memory only (E9).
    pub rangda: RangdaService,
}

impl EventServices {
    /// The cached aggregates of the running events: the [`EventFactors`] every consumer reads.
    pub fn factors(&self) -> &EventModifiers {
        self.lua_events.modifiers()
    }

    /// Reads the `SecretRoom`, `Tax` and `RainbowRace` families into [`EventServices::configs`] and puts the schedule
    /// derived from them on the four event wall jobs (22 §2.5 rule 2, §2.4 rule 6). Called after the families are
    /// up (`Game::new`) and after a successful reload of one of them; the jobs are re-specced only when their specs
    /// changed. A family that is `Missing` or `Faulted` gives the shipped defaults (one warning).
    pub fn read_configs(&mut self, lua: &LuaHost, data: &GameData, sched: &mut Scheduler) {
        let new = EventConfigs::read(lua, data, self.lua_events.warned());
        if new.schedule != self.configs.schedule {
            new.schedule.apply(sched);
        }
        self.configs = new;
        // 22 §10.1: a reload of `Tax.lua` updates the bounds, the earning rates and the occupation items
        self.tax.apply_config(&self.configs.tax);
    }
}

/// `DeathHooks::world_die` (`DoDie` step 9): the one dispatcher of the event kill hooks. The order is binding:
/// the siege part first, then (each at its own banner) gc1to1, the instance dungeons, the secret room. Later event
/// kill hooks are lines here, never a second dispatcher.
pub fn on_world_die(s: &mut Services, ctx: &mut GameCtx<'_>, k: &crate::combat::KillCtx) {
    crate::social::siege::fight::on_world_die(s, ctx, k);
    // ---- gc1to1 (doc 21, task gc11) ----
    crate::social::gc1to1::on_world_die(s, ctx, k);
    // ---- instances (doc 22 §8, task instances) ----
    instance::on_npc_killed(s, ctx, k);
    // ---- secret room (doc 22 §11, task secret-room) ----
    secret_room::on_world_die(s, ctx, k);
}

/// P9 `event::event_minute` (22 §2.3): the Event.lua state pass for every wall minute crossed since the last
/// run, with the Lua clock pinned to that minute. Registered in `sched/registry.rs`.
pub fn event_minute(s: &mut Services, ctx: &mut GameCtx<'_>) {
    // the legacy DB minute-timer order (src:databaseserver/DatabaseServer.cpp:549-565): the propEvent flags first
    generic::minute(s, ctx);
    let shop = shop_factors(s);
    let report = s
        .event
        .lua_events
        .minute(ctx.lua, ctx.data, ctx.clock.unix_ms);
    publish(s, ctx, &report, shop);
}

/// What follows a state pass (22 §4.2 steps 2 and 3, §4.10, §4.11): the auto notices of the crossed minutes (before
/// the descriptions of the run: over a replayed stall the C++ interleaved them per minute, only the order differs),
/// the `EventLuaDesc` of every changed event to everybody, the spawn events of the changes
/// ([`spawns::EventSpawnService::apply_changes`], after the minute loop) and, when the Event.lua shop factors
/// moved, `GameRate` `SHOP_BUY` / `SHOP_SELL`. The aggregates were already recomputed by the pass.
fn publish(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    report: &MinuteReport,
    shop_before: (f32, f32),
) {
    for c in &report.changes {
        tracing::info!(
            id = c.id,
            running = c.running,
            desc = %c.desc,
            "Event.lua: event state changed"
        );
    }
    for lines in &report.notices {
        tracing::info!(lines = lines.len(), "Event.lua: auto notice due");
        notice::send_auto_notice(s, ctx, lines);
    }
    desc::announce(ctx, &report.changes);
    {
        let EventServices {
            lua_events, spawns, ..
        } = &mut s.event;
        let scripts = lua_events.scripts(ctx.lua, ctx.data);
        spawns.apply_changes(&scripts, &report.changes);
    }
    let (buy, sell) = shop_factors(s);
    if buy != shop_before.0 {
        game_rate::broadcast(ctx, game_rate::SHOP_BUY, buy);
    }
    if sell != shop_before.1 {
        game_rate::broadcast(ctx, game_rate::SHOP_SELL, sell);
    }
}

fn shop_factors(s: &Services) -> (f32, f32) {
    let m = s.event.factors();
    (m.shop_buy, m.shop_sell)
}

/// `lua_events::after_reload` (22 §4.1 step 3, §2.5 rule 4): after a **successful** reload of the `Event` family the
/// state pass runs again with descriptions (the clients hear about the events that are running), the modifiers are
/// recomputed and the shop factors are announced if they moved. A failed reload (`kept_previous`) must not call
/// this: nothing changes.
pub fn after_reload(s: &mut Services, ctx: &mut GameCtx<'_>) -> MinuteReport {
    // `LoadScript` (EventLua.cpp:184-186): every active spawn list moves to the end list; the fresh state reports
    // the running events again and they restart
    s.event.spawns.end_all();
    let shop = shop_factors(s);
    let report = s
        .event
        .lua_events
        .on_reload(ctx.lua, ctx.data, ctx.clock.unix_ms);
    publish(s, ctx, &report, shop);
    report
}

impl EventServices {
    /// `/eventlist` (22 §4.13): one line `"ID : %d,  Title : %s,  State : %d"` per event of `GetAllEventList`. The
    /// format is the one of `CEventLua::GetAllEventList` with TWO spaces after each comma (Plan/22 §4.13 quotes it
    /// with one; the C++ wins, README ruling 26).
    pub fn list_lines(&self, lua: &LuaHost, data: &GameData) -> Vec<String> {
        self.lua_events
            .scripts(lua, data)
            .all_event_list()
            .into_iter()
            .map(|e| format!("ID : {},  Title : {},  State : {}", e.id, e.title, e.state))
            .collect()
    }

    /// `/eventinfo <id>` (22 §4.13): the lines of `GetEventInfo(id)`.
    pub fn info_lines(&self, lua: &LuaHost, data: &GameData, id: u32) -> Vec<String> {
        self.lua_events.scripts(lua, data).event_info(id)
    }

    /// The admin API's "list modifiers" (22 §4.13): the cached [`EventModifiers`] as JSON.
    pub fn modifiers_json(&self) -> serde_json::Value {
        self.factors().to_json()
    }
}

/// `AdminCmd::ReloadScripts` for the live Lua families (Plan/20 §3.9, `/lua event`, `/lua rr`, `/reload lua`): the
/// scripts are re-read from the resource tree and re-run from scratch; a failure keeps the previous version. After a
/// successful reload of `Event.lua` the state pass runs again and the aggregates are recomputed (22 §4.1). The NPC
/// targets (`/loadscript`) swap the whole dialog registry atomically (WP-NPC-05). The body of
/// [`Game::admin_reload_scripts`] on the `(Services, GameCtx)` pair, so that a GM command (`gm::cmd::reload`) runs the
/// same reload inside its handler; every family that follows its reload with a configuration step has its arm here
/// (a later family adds its arm at the end, in this form).
pub(crate) fn reload_scripts(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    target: ReloadTarget,
    npc_sources: Option<ScriptSources>,
) -> AdminOutput {
    let report = ctx.lua.reload(target, npc_sources);
    if report
        .ok
        .iter()
        .any(|n| n == ScriptFamily::Event.logical_name())
    {
        let r = after_reload(s, ctx);
        tracing::info!(changes = r.changes.len(), "Event.lua reloaded");
    }
    // the typed configs of the other families follow their reload, the rainbow specs are re-applied
    // (22 §2.4 rule 6, §2.5 rule 4)
    if report.ok.iter().any(|n| {
        [
            ScriptFamily::Tax,
            ScriptFamily::SecretRoom,
            ScriptFamily::RainbowRace,
        ]
        .iter()
        .any(|f| n == f.logical_name())
    }) {
        s.event.read_configs(ctx.lua, ctx.data, ctx.sched);
    }
    // 21 §8 (task gc11): the globals and the opening specs of the 1:1 guild combat follow a reload of its family
    // (also through `ReloadTarget::All`; no GM row reloads this family)
    if report
        .ok
        .iter()
        .any(|n| n == ScriptFamily::GuildCombat1to1.logical_name())
    {
        s.social.gc1to1.reconfigure(ctx.lua, ctx.cfg, ctx.sched);
    }
    let mut lines: Vec<String> = report.ok.iter().map(|n| format!("reloaded {n}")).collect();
    for (name, error) in &report.failed {
        lines.push(format!("{name}: {error}"));
    }
    if report.failed.is_empty() {
        if lines.is_empty() {
            lines.push("nothing to reload".to_owned());
        }
        AdminOutput::Lines(lines)
    } else {
        if report.kept_previous {
            lines.push("the previous version stays in force".to_owned());
        }
        AdminOutput::Error(lines.join("; "))
    }
}

impl Game {
    /// `AdminCmd::ReloadScripts` for the live Lua families (Plan/20 §3.9, `/lua event`, `/lua rr`): see
    /// [`reload_scripts`].
    pub(crate) fn admin_reload_scripts(
        &mut self,
        target: ReloadTarget,
        npc_sources: Option<ScriptSources>,
    ) -> AdminOutput {
        let (svc, mut ctx) = self.split();
        reload_scripts(svc, &mut ctx, target, npc_sources)
    }

    /// Where the Event family stands (`/lua event` answers, admin status).
    pub fn event_family_status(&self) -> FamilyStatus {
        self.lua.family_status(ScriptFamily::Event)
    }
}
