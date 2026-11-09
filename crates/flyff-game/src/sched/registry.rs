//! The registered job table (Plan/10 §5.1 registration rule): one line per row of §5.3 and per item of
//! §5.4, in table order. A line whose owner has not landed yet points at an empty stub in [`stubs`]; the
//! owning WP replaces the function path (and nothing else) in the WP that implements the body. The event lines
//! (doc 22) point at the empty bodies of `crate::event::*` since WP-EVENT-01. An owner MUST NOT aggregate
//! several rows or items into one function.
//!
//! Wall jobs (§5.5) are registered by their owners with [`Scheduler::register_wall`]: the daily maintenance
//! (doc 13) and the four event jobs of doc 22 (`secret_room_open`, `rainbow_race_open`, `tax_pay`, `tax_change`,
//! [`crate::event::EventScheduleConfig`]) are here; the doc 21 jobs arrive with their WPs.

use super::{Gate, PeriodicJob, SLOT_ROW, Scheduler, SlotItem};
use crate::config::GameConfig;
use flyff_common::config::EventId;

/// The empty bodies the table points at until the owning WPs land.
pub mod stubs {
    use crate::ctx::{GameCtx, Services};

    macro_rules! stub {
        ($($(#[$m:meta])* $name:ident),* $(,)?) => {
            $(
                $(#[$m])*
                pub fn $name(_s: &mut Services, _ctx: &mut GameCtx<'_>) {}
            )*
        };
    }

    stub! {
        /// P2 marker: the runtime calls the slot items instead.
        slot_marker,
        /// P3.2 `ai::create_monster::expire` (doc 18 §14).
        monster_expire,
        /// P14 `runtime::ccu` (doc 10 §11).
        ccu,
        /// P18 `session::finish_leave` driver (doc 12).
        linger_check,
    }
}

/// `period` of the minute rows (`60 s` = 900 ticks).
const MINUTE: u64 = 900;

/// Registers the whole table of §5.3 and §5.4 into `sched` (at `Game` construction, once).
///
/// * P12 is registered only when `gameplay.grant_poll_s > 0` (`0` = the original login-only delivery),
/// * P17 does not exist (`MoverKeyframe` is emitted inside the per-mover pipeline),
/// * slot items 2–3 are not registered (guild/party quests are deferred); item 1 is `game_clock` (15 §17.1: the
///   cached once-per-second in-game time that the respawn job reads).
pub fn register_all(sched: &mut Scheduler, cfg: &GameConfig) {
    use Gate::Always;
    use stubs as f;
    let mut periodic = |job: PeriodicJob| sched.register_periodic(job);
    // ---- §5.3 in table order ----
    periodic(PeriodicJob::new(
        "P1",
        "respawn",
        15,
        15,
        3,
        Always,
        crate::world::respawn::process,
    ));
    periodic(PeriodicJob::new(
        SLOT_ROW,
        "slot_1s",
        10,
        15,
        9,
        Always,
        f::slot_marker,
    ));
    periodic(PeriodicJob::new(
        "P3",
        "call_the_roll",
        22,
        MINUTE,
        1,
        Always,
        crate::event::generic::call_the_roll,
    ));
    periodic(PeriodicJob::new(
        "P3",
        "monster_expire",
        18,
        MINUTE,
        1,
        Always,
        crate::ai::create_monster::expire,
    ));
    periodic(PeriodicJob::new(
        "P4",
        "lord_minute",
        22,
        MINUTE,
        61,
        Always,
        crate::event::lord::minute,
    ));
    periodic(PeriodicJob::new(
        "P5",
        "tax_minute",
        22,
        MINUTE,
        121,
        Always,
        crate::event::tax::minute,
    ));
    periodic(PeriodicJob::new(
        "P6",
        "couple_expire",
        21,
        MINUTE,
        181,
        Always,
        crate::social::couple::expire,
    ));
    periodic(PeriodicJob::new(
        "P7",
        "housing_minute",
        22,
        MINUTE,
        241,
        Always,
        crate::event::housing::minute,
    ));
    periodic(PeriodicJob::new(
        "P8",
        "mail_expire",
        19,
        MINUTE,
        301,
        Always,
        crate::econ::mail::expire,
    ));
    periodic(PeriodicJob::new(
        "P9",
        "event_minute",
        22,
        MINUTE,
        361,
        Always,
        crate::event::event_minute,
    ));
    periodic(PeriodicJob::new(
        "P10",
        "weather_minute",
        22,
        MINUTE,
        421,
        Always,
        crate::event::weather::minute,
    ));
    periodic(PeriodicJob::new(
        "P11",
        "guild_house_expiry",
        22,
        75,
        5,
        Always,
        crate::event::guild_house::expiry_step,
    ));
    let grant_poll_ticks = u64::from(cfg.rules.gameplay.grant_poll_s) * 15;
    if grant_poll_ticks > 0 {
        let phase = 13 % grant_poll_ticks;
        periodic(PeriodicJob::new(
            "P12",
            "grants_poll",
            13,
            grant_poll_ticks,
            phase,
            Always,
            crate::persist::grants::poll,
        ));
    }
    let guild_bank = Gate::Event(EventId::GuildInventory);
    periodic(PeriodicJob::new(
        "P13",
        "guild_bank_removal_poll",
        13,
        450,
        28,
        guild_bank,
        crate::social::guild::bank::removal_poll,
    ));
    periodic(PeriodicJob::new(
        "P14",
        "audit_ccu",
        10,
        9000,
        7,
        Always,
        crate::audit::ccu,
    ));
    periodic(PeriodicJob::new(
        "P15",
        "keep_connect_drain",
        22,
        1,
        0,
        Always,
        crate::event::lua_events::keep_connect_drain,
    ));
    periodic(PeriodicJob::new(
        "P16",
        "event_generic_spawn",
        22,
        1,
        0,
        Always,
        crate::event::generic::spawn_tick,
    ));
    periodic(PeriodicJob::new(
        "P18",
        "linger_check",
        12,
        1,
        0,
        Always,
        crate::session::linger_check,
    ));
    // P19 (doc 20 §9.9, task quest-effects): `CMover::ProcessQuest` ran per mover and tick (patrol zones, the
    // minute timer; the server-side expiry of PL-108); per-player work is not a slot item (10 §5.4)
    periodic(PeriodicJob::new(
        "P19",
        "quest_process",
        20,
        1,
        0,
        Always,
        crate::quest::track::process,
    ));

    // ---- §5.4: the 1 s slot: item 1 `game_clock` (the once-per-second sample of the in-game clock, 15 §17.1),
    // items 4-16 in the legacy order (2-3 absent, 17 is the wall evaluation) ----
    let mut slot = |item: SlotItem| sched.register_slot_item(item);
    slot(SlotItem::new(
        1,
        "game_clock",
        15,
        Always,
        crate::world::clock::game_clock,
    ));
    slot(SlotItem::new(
        4,
        "siege",
        21,
        Gate::Event(EventId::GuildCombat),
        crate::social::siege::process,
    ));
    slot(SlotItem::new(
        5,
        "gc1to1",
        21,
        Gate::Event(EventId::GuildCombat1to1),
        crate::social::gc1to1::process,
    ));
    slot(SlotItem::new(
        6,
        "secret_room",
        22,
        Gate::Event(EventId::SecretRoom),
        crate::event::secret_room::process,
    ));
    slot(SlotItem::new(
        7,
        "rainbow_race",
        22,
        Gate::Event(EventId::RainbowRace),
        crate::event::rainbow::process,
    ));
    slot(SlotItem::new(
        8,
        "instance_dungeons",
        22,
        Always,
        crate::event::instance::process,
    ));
    slot(SlotItem::new(
        9,
        "quiz",
        22,
        Always,
        crate::event::quiz::process,
    ));
    slot(SlotItem::new(
        10,
        "guild_manager",
        21,
        Gate::Event(EventId::GuildInventory),
        crate::social::guild::process,
    ));
    slot(SlotItem::new(
        11,
        "guild_wars",
        21,
        Gate::Event(EventId::GuildWar),
        crate::social::guild::war::process,
    ));
    slot(SlotItem::new(
        12,
        "party_map_info",
        21,
        Always,
        crate::social::party::map_info,
    ));
    slot(SlotItem::new(
        13,
        "rangda",
        22,
        Always,
        crate::event::rangda::process,
    ));
    slot(SlotItem::new(
        14,
        "event_spawns",
        22,
        Always,
        crate::event::spawns::process,
    ));
    slot(SlotItem::new(
        15,
        "party_worker",
        21,
        Always,
        crate::social::party::worker,
    ));
    slot(SlotItem::new(
        16,
        "quiz_notice",
        22,
        Always,
        crate::event::quiz::notice_second,
    ));
    // item 17 is the wall evaluation (the runtime). Item 18 is new: the `#auto` idle speech of the NPCs (20 §8.3).
    slot(SlotItem::new(
        18,
        "npc_idle_speech",
        20,
        Always,
        crate::npc::idle::process,
    ));

    // ---- §5.5 wall jobs registered by this table: the daily maintenance (13 §23, WP-DB-16) and the four calendar
    // triggers of doc 22 with their legacy default specs (22 §2.4; `Game::new` re-specs them from the Lua families) ----
    crate::persist::maintenance::register(sched, cfg);
    // the guild salary: one minute window job, 21:00-21:59 local (21 §2.2, §5.10, WP-SOCIAL-14)
    crate::social::guild::salary::register(sched);
    crate::event::EventScheduleConfig::legacy().register(sched);
    // the admin console's scheduled notices: one minute job (25 §8.1, WP-ADMIN-27)
    crate::admin::actions::comm::register(sched);
}
