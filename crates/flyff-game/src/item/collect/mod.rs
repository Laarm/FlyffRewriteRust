//! Collecting (Plan/19 §14; WP-ITEM-19; ref/13 §20; ref/07c §14; `User.cpp:3107-3243`, `Mover.cpp:11927-12003`,
//! `DPSrvr.cpp:10700-10730`, `collecting.cpp`).
//!
//! The collector is the worn right-hand weapon with `is_collector(false)` that is not expired
//! (`GetCollector`); its battery is the item's endurance slot (`Item.collector_battery`, at most
//! `collecting.max_battery` = 1800), its level the ability option (`0..=5`).
//!
//! * **`QueryStartCollecting`**: a disguised or dead or flying player is ignored; outside `RA_COLLECTING`
//!   (the player's region attribute) the answer is `TID_GAME_COLLECTING_REGION`; without a collector nothing
//!   happens; else the player enters `OBJSTA_COLLECT` ([`SimFx::StartCollecting`](crate::combat::SimFx): the
//!   destination is cleared, `OBJMSG_STOP`, `OBJMSG_COLLECT`) and `StartCollecting` goes to everybody around.
//!   The server does **not** set `DONMOVE_MODE` (the original did; the bit is part of the persisted mode word, so
//!   a collector who logged out during the pass came back unable to walk; the server never enforced it and the
//!   client sets its own bit when it receives the message).
//! * **`QueryStopCollecting`** while collecting resets the state and broadcasts `StopCollecting`. **FIX:** the
//!   original reset the whole action group on *any* such request, which also cleared a stun.
//! * **The pass** (every 16 ticks per player, `Mover.cpp:3866`): no collector, a disguise or a battery that ran
//!   down stops the collecting; else, unless a battery buff (`BUFF_ITEM2` of the silver, gold or 001 battery) is
//!   active, the battery loses one point (`UpdateItem(HP)`; 0 stops) and the counter grows; at
//!   `collecting.cool[AO]` (105, 90, 75, 60, 45, 30) it resets, one item is rolled over the cumulative
//!   `Collecting_Item` table out of 10^9 and created (`ItemOrigin::Collect`, audit `item.create`;
//!   `RestartCollecting{item_id}` to the player); with a full bag the item is lost (KEEP). An option above the
//!   table never collects (KEEP: the original returned `INT_MAX`).
//! * **Teleports, death, logout.** The original stopped a collector inside `Replace` and at the logout. Here the
//!   per-tick check ends the collecting of a player who is no longer where it started (world, layer or more than
//!   a metre away; a collector cannot walk), and a player who left the collect state otherwise (death) is reset
//!   silently, without `StopCollecting`: that message would make the client stand a dead player up. The logout
//!   needs nothing: the mover goes away and no mode bit was set.
//! * **`II_GEN_TOO_COL_NORMALBATTERY`** refills the battery of the worn collector to the maximum (refused with
//!   `TID_GAME_CANT_USE_BATTERY` without a collector or while collecting; the scroll is then not consumed).

use crate::combat::SimFx;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::follow::Followup;
use crate::item::container::{SerialSource, StorageId};
use crate::item::env::ItemEnv;
use crate::item::fx::Fx;
use crate::item::model::{Item, MAX_COLLECTOR_BATTERY};
use crate::item::ops::{ItemOrigin, UiParam, core as ops};
use crate::mover::Mover;
use crate::world::region::{is_region_attr, region_attr_of};
use flyff_common::defs::define::RA_COLLECTING;
use flyff_common::defs::item::{
    II_GEN_TOO_COL_BATTERY001, II_GEN_TOO_COL_GOLDBATTERY, II_GEN_TOO_COL_SILVERBATTERY,
};
use flyff_common::defs::neuz::PARTS_RWEAPON;
use flyff_common::defs::text::{TID_GAME_CANT_USE_BATTERY, TID_GAME_COLLECTING_REGION};
use flyff_common::math::Vec3;
use flyff_common::rng::GameRng;
use flyff_common::{LayerId, WorldId};
use flyff_data::items_ext::collecting::{COLLECTING_SPACE, CollectingTable};
use flyff_proto::pb;
use flyff_sim::types::OBJSTA_COLLECT;

use crate::buff::set::{BuffKey, BuffKind};

/// The period of the pass in ticks (`(m_nCount & 15) == 0`).
pub const PERIOD_TICKS: u64 = 16;

/// A collector may drift this far (metres, squared) from where it started before the collecting counts as
/// interrupted by a teleport or a push.
const MOVED_SQ: f32 = 1.0;

/// Where a collector started (the stop-on-teleport check).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Origin {
    /// The world.
    pub world: WorldId,
    /// The layer.
    pub layer: LayerId,
    /// The position.
    pub pos: Vec3,
}

/// The runtime state of a collector (`m_nCollecting` and the start spot); never persisted.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CollectState {
    /// `m_nCollecting`: the passes since the last item.
    pub counter: u32,
    /// Where the collecting started; `None` while not collecting.
    pub origin: Option<Origin>,
}

/// How a start request ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartResult {
    /// The player collects now.
    Started,
    /// Not collecting (silently): disguised, dead, flying, already collecting or without a collector.
    Ignored,
    /// Outside `RA_COLLECTING`: `TID_GAME_COLLECTING_REGION`.
    WrongRegion,
}

/// `IsCollecting()`: the action state is `OBJSTA_COLLECT`.
pub fn is_collecting(m: &Mover) -> bool {
    m.sim.act.state.bits() & OBJSTA_COLLECT != 0
}

/// `GetCollector()`: the storage id of the worn right-hand weapon when it is a collector that is not expired.
pub fn collector(m: &Mover) -> Option<StorageId> {
    let inv = &m.player()?.items.inventory;
    let id = inv.equip_id(PARTS_RWEAPON)?;
    let it = inv.get_at_id(id)?;
    (it.is_collector(false) && !it.is_expired()).then_some(id)
}

/// The battery capacity (`nMaxBattery`).
pub fn max_battery(table: &CollectingTable) -> u32 {
    u32::try_from(table.max_battery)
        .ok()
        .filter(|&v| v > 0)
        .unwrap_or(MAX_COLLECTOR_BATTERY)
}

/// `HasBuff(BUFF_ITEM2, battery)` for the three battery items.
fn has_battery_buff(m: &Mover) -> bool {
    [
        II_GEN_TOO_COL_SILVERBATTERY,
        II_GEN_TOO_COL_GOLDBATTERY,
        II_GEN_TOO_COL_BATTERY001,
    ]
    .iter()
    .any(|&id| m.buffs.contains(BuffKey::new(BuffKind::Item2, id)))
}

/// `CUser::StartCollecting` with the checks of `OnQueryStartCollecting`.
pub fn start(m: &mut Mover, fx: &mut Fx) -> StartResult {
    if m.mv.disguise.is_some() || m.sim.act.state.is_die() || m.sim.act.flags.fly() {
        return StartResult::Ignored;
    }
    if !is_region_attr(region_attr_of(m), RA_COLLECTING) {
        fx.text(TID_GAME_COLLECTING_REGION);
        return StartResult::WrongRegion;
    }
    if collector(m).is_none() || is_collecting(m) {
        return StartResult::Ignored;
    }
    let origin = Origin {
        world: m.core.world,
        layer: m.core.layer,
        pos: m.core.pos,
    };
    if let Some(p) = m.player_mut() {
        p.items.collect.origin = Some(origin);
    }
    fx.then(Followup::Sim {
        mover: m.id(),
        fx: SimFx::StartCollecting,
    });
    fx.out.near(pb::StartCollecting {});
    StartResult::Started
}

/// `CUser::StopCollecting`: the counter and the start spot reset, the state follows, everybody around is told.
pub fn stop(m: &mut Mover, fx: &mut Fx) {
    if let Some(p) = m.player_mut() {
        p.items.collect = CollectState::default();
    }
    fx.then(Followup::Sim {
        mover: m.id(),
        fx: SimFx::StopCollecting,
    });
    fx.out.near(pb::StopCollecting {});
}

/// `QueryStopCollecting`: stops a collecting player, ignores everyone else (see the module documentation).
pub fn request_stop(m: &mut Mover, fx: &mut Fx) -> bool {
    if !is_collecting(m) {
        return false;
    }
    stop(m, fx);
    true
}

/// `DoUseItemBattery`: refills the battery of the worn collector. `false` (with `TID_GAME_CANT_USE_BATTERY`)
/// without a collector or while collecting.
pub fn use_battery(m: &mut Mover, env: &ItemEnv<'_>, fx: &mut Fx) -> bool {
    let Some(id) = collector(m) else {
        fx.text(TID_GAME_CANT_USE_BATTERY);
        return false;
    };
    if is_collecting(m) {
        fx.text(TID_GAME_CANT_USE_BATTERY);
        return false;
    }
    let max = max_battery(&env.data().collecting);
    ops::update_item(m, env, id, UiParam::Hp, max, 0, fx);
    true
}

/// The result of one pass of a mover (for the tests).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PassOutcome {
    /// The collecting was stopped by this pass.
    pub stopped: bool,
    /// The item that was created, if any.
    pub collected: Option<u32>,
}

/// `CUser::ProcessCollecting` for one mover (see the module documentation).
pub fn process(
    m: &mut Mover,
    env: &ItemEnv<'_>,
    rng: &mut dyn GameRng,
    serials: &mut dyn SerialSource,
    fx: &mut Fx,
) -> PassOutcome {
    let mut out = PassOutcome::default();
    let collecting = is_collecting(m);
    let Some(col) = collector(m) else {
        if collecting {
            stop(m, fx);
            out.stopped = true;
        }
        return out;
    };
    if m.mv.disguise.is_some() {
        if collecting {
            stop(m, fx);
            out.stopped = true;
        }
        return out;
    }
    if !collecting {
        return out;
    }
    // 3.1 the battery
    if !has_battery_buff(m) {
        let battery = m
            .player()
            .and_then(|p| p.items.inventory.get_at_id(col))
            .and_then(|it| it.collector_battery)
            .unwrap_or(0)
            .saturating_sub(1);
        ops::update_item(m, env, col, UiParam::Hp, battery, 0, fx);
        if battery == 0 {
            stop(m, fx);
            out.stopped = true;
            return out;
        }
    }
    // 3.2 the item
    let table = &env.data().collecting;
    let ao = m
        .player()
        .and_then(|p| p.items.inventory.get_at_id(col))
        .map_or(0, |it| it.upgrade_level);
    let cool = table.cool_for(ao);
    let Some(p) = m.player_mut() else {
        return out;
    };
    p.items.collect.counter = p.items.collect.counter.saturating_add(1);
    if i64::from(p.items.collect.counter) < i64::from(cool) {
        return out;
    }
    p.items.collect.counter = 0;
    let Some(item_id) = table
        .pick(rng.random_below(COLLECTING_SPACE))
        .filter(|&i| i != 0)
    else {
        return out;
    };
    if ops::create_item(
        m,
        env,
        serials,
        Item::new(item_id, 1),
        ItemOrigin::Collect,
        fx,
    )
    .is_ok()
    {
        fx.out.to_self(pb::RestartCollecting { item_id });
        out.collected = Some(item_id);
    }
    out
}

/// Ends the collecting of a player who is no longer where it started or who left the collect state otherwise.
/// Returns `true` when the collecting was ended.
pub fn check_interrupted(m: &mut Mover, fx: &mut Fx) -> bool {
    let Some(origin) = m.player().and_then(|p| p.items.collect.origin) else {
        return false;
    };
    if !is_collecting(m) {
        // died (or was otherwise reset): forget silently
        if let Some(p) = m.player_mut() {
            p.items.collect = CollectState::default();
        }
        return false;
    }
    let d = m.core.pos - origin.pos;
    let moved = m.core.world != origin.world
        || m.core.layer != origin.layer
        || d.x * d.x + d.y * d.y + d.z * d.z > MOVED_SQ;
    if moved {
        stop(m, fx);
    }
    moved
}

// ---- handlers and the tick -------------------------------------------------------------------------------------------

/// `QueryStartCollecting` (C2S).
pub fn on_start(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    _: pb::QueryStartCollecting,
) {
    let mut fx = Fx::new();
    let now = ctx.clock.game_ms;
    if let Some(m) = ctx.worlds.player_mut(who.0)
        && start(m, &mut fx) == StartResult::Started
        && let Some(p) = m.player_mut()
    {
        // `CUser::StartCollecting`: `m_dwHonorCheckTime = GetTickCount()` (21 §13.3)
        crate::social::honor::restart_clock(p, now);
    }
    fx.flush(svc, ctx, who);
}

/// `QueryStopCollecting` (C2S).
pub fn on_stop(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    _: pb::QueryStopCollecting,
) {
    let mut fx = Fx::new();
    if let Some(m) = ctx.worlds.player_mut(who.0) {
        request_stop(m, &mut fx);
    }
    fx.flush(svc, ctx, who);
}

/// The job of one tick: the interruption check of every collecting player, and the pass of the players whose
/// phase it is. Called by [`crate::item::tick::process`].
pub fn tick(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    let tick = ctx.clock.tick.0;
    let players: Vec<_> = ctx.sessions.iter().map(|e| e.player).collect();
    for id in players {
        let who = PlayerRef(id);
        let active = ctx
            .worlds
            .player(id)
            .and_then(|m| m.player())
            .is_some_and(|p| p.items.collect.origin.is_some());
        let due = (tick + u64::from(id.0)).is_multiple_of(PERIOD_TICKS);
        if !active && !due {
            continue;
        }
        let core = svc.items.handle();
        let env = core.env(ctx.clock.unix_ms.to_unix_time());
        let Some(world) = ctx.worlds.core(id).map(|c| c.world) else {
            continue;
        };
        let mut fx = Fx::new();
        {
            let mut serials =
                crate::item::ops::CtxSerials::new(&mut svc.serials, ctx.persist, ctx.tickets);
            let Some(m) = ctx.worlds.player_mut(id) else {
                continue;
            };
            if active && check_interrupted(m, &mut fx) {
                // stopped: no further pass this tick
            } else if due {
                let r = process(m, &env, ctx.rng.world(world), &mut serials, &mut fx);
                if r.collected.is_some() || r.stopped {
                    fx.mark_urgent();
                }
            }
        }
        fx.flush(svc, ctx, who);
    }
}
