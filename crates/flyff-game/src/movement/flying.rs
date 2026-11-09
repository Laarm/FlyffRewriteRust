//! Server-side flying of players (Plan/18 §6, F-106; WP-AI-17): the take-off and landing gates, the
//! dismount at a no-fly border, the turbo-off broadcast and the sync of the sim's flight state with the
//! worn ride. The flight physics are `flyff_sim::fly`; the `PlayerMoveState` variants of flight
//! (`MOVED2`/`BEHAVIOR2`/`CORR2`/`ANGLE`) are [`super::player`]'s, their relays are `MoverStateNtf` with the `fly`
//! fields.
//!
//! | what | where |
//! |---|---|
//! | [`take_off`] / [`land`] | the gates of 18 §6 over `OBJMSG_MODE_FLY` / `OBJMSG_MODE_GROUND`: refused when the world flag `fly` is 0 or the cell is `HATTR_NOFLY`, and (unless forced) while sitting, acting, attacking, `MVRF_NOMOVE` or rooted |
//! | [`reconcile`] | `RedoEquip`'s ride rule (ref/10 §13.6): a worn ride makes the player fly (`MODE_FLY`, forced), no ride lands it (`MODE_GROUND`, forced) |
//! | `tail` | runs [`reconcile`] for every awake player at the end of the tick, so every equip path (`DoEquipReq`, `DoUseItem`, NPC helpers, GM commands, the join) takes off and lands the sim copy in the same tick |
//! | `on_turbo_off` | `TurboOff` of the sim: `SendActMsg{MODE_TURBO_OFF, is_fly}` to the viewers, once per run-out |
//! | `on_no_fly_zone` + [`finish`] | `EnteredNoFly` of the sim (the step into the no-fly cell was undone): the ride is unequipped with force (`UnequipRide`) after the world phase, where the services exist, and the player lands |
//!
//! **Why a tail and a queue.** The equip handlers live in the services context, the sim runs in the world
//! phase: the tail sees the *result* of every equip (the worn ride) instead of hooking into each path, and the
//! dismount, which needs the item functions, is a request that the tick runs right after the world phase.

use super::{FrameLookup, intents};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::container::StorageId;
use crate::item::fx::Fx;
use crate::item::handlers::toggle_equip;
use crate::mover::Mover;
use crate::stats::equip::EquipFacts;
use crate::world::object::MoverKey;
use crate::world::phases::FrameCtx;
use crate::world::terrain::HeightAttr;
use flyff_common::defs::neuz::PARTS_RIDE;
use flyff_common::rng::GameRng;
use flyff_common::{ObjId, SimFrame};
use flyff_data::GameData;
use flyff_data::props::item::ItemProp;
use flyff_sim::mover::RideParams;
use flyff_sim::step::{EventMode, NoRules, SimEnv, SimEvent, SimEvents, SimRole, SimRules};
use flyff_sim::types::{
    ACT_OK, HATTR_NOFLY, OBJMSG_MODE_FLY, OBJMSG_MODE_GROUND, OBJMSG_MODE_TURBO_OFF,
};
use flyff_sim::{ActMsg, send_act_msg};
use std::collections::BTreeSet;

/// Why a take-off or landing was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlyRefusal {
    /// Only players fly on a ride.
    NotAPlayer,
    /// The mover is dead (`IsDie`).
    Dead,
    /// The world's `.wld` `fly` flag is 0: every cell counts as no-fly.
    WorldNoFly,
    /// The cell under the player is `HATTR_NOFLY`.
    NoFlyCell,
    /// Landing over a `HATTR_NOWALK` cell (unless forced).
    NoWalkCell,
    /// Sitting (unless forced).
    Sitting,
    /// In an action state (stun, collect, appear; unless forced).
    Acting,
    /// Attacking (unless forced).
    Attacking,
    /// `MVRF_NOMOVE` (unless forced).
    NoMove,
    /// Rooted (`CHS_LOOT`; unless forced).
    Rooted,
    /// The sim refused the message although the gates passed (cannot happen with the gates above).
    Refused,
}

/// What [`reconcile`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reconciled {
    /// The sim copy already agrees with the worn ride.
    InSync,
    /// A worn ride made the player fly.
    TookOff,
    /// No ride made the flyer land.
    Landed,
    /// A gate refused the change (the ride stays worn / the flyer stays up).
    Refused(FlyRefusal),
}

/// The pending requests of the flying code that need the services context: the players whose ride must come
/// off because they flew into a no-fly cell (`MovementState.fly`).
#[derive(Clone, Debug, Default)]
pub struct FlyQueue {
    unequip: BTreeSet<ObjId>,
}

impl FlyQueue {
    /// `true` when nothing waits.
    pub fn is_empty(&self) -> bool {
        self.unequip.is_empty()
    }

    /// The players waiting for their dismount.
    pub fn unequip_requests(&self) -> impl Iterator<Item = ObjId> + '_ {
        self.unequip.iter().copied()
    }
}

/// The sim's ride parameters of a ride item (`ItemProp` columns `fFlightSpeed`, `fFlightLRAngle`,
/// `fFlightTBAngle`, `dwUseMotion`, `dwFFuelReMax`).
pub fn ride_params(prop: &ItemProp) -> RideParams {
    RideParams {
        item: prop.id,
        flight_speed: prop.flight_speed,
        lr_angle: prop.flight_lr_angle,
        tb_angle: prop.flight_tb_angle,
        use_motion: prop.use_motion,
        f_fuel_re_max: i32::try_from(prop.f_fuel_re_max).unwrap_or(i32::MAX),
    }
}

/// The take-off gates of 18 §6 for the mover as it stands: `Ok` when `OBJMSG_MODE_FLY` may be sent.
///
/// * the world flag `fly == 0` and a `HATTR_NOFLY` cell refuse always (the equip planner checks them too, but
///   a teleport or a login can put a worn ride there);
/// * unless `force`: sitting, an action state, an attack, `MVRF_NOMOVE` and `CHS_LOOT` (the sim's own guards
///   of `MODE_FLY`, here with their names).
pub fn take_off_gate(m: &Mover, force: bool, env: &SimEnv<'_>) -> Result<(), FlyRefusal> {
    if !m.is_player() {
        return Err(FlyRefusal::NotAPlayer);
    }
    let s = &m.sim;
    if s.act.state.is_die() {
        return Err(FlyRefusal::Dead);
    }
    if !env.world.fly_allowed {
        return Err(FlyRefusal::WorldNoFly);
    }
    if env.world.height_attr(s.pos.x, s.pos.z) == HATTR_NOFLY {
        return Err(FlyRefusal::NoFlyCell);
    }
    if !force {
        if s.act.flags.sit() {
            return Err(FlyRefusal::Sitting);
        }
        if s.act.state.is_action() {
            return Err(FlyRefusal::Acting);
        }
        if s.act.state.is_act_attack() {
            return Err(FlyRefusal::Attacking);
        }
        if s.mvrf.has(flyff_sim::types::MVRF_NOMOVE) {
            return Err(FlyRefusal::NoMove);
        }
        if s.params.chr_state & flyff_sim::types::defs::CHS_LOOT != 0 {
            return Err(FlyRefusal::Rooted);
        }
    }
    Ok(())
}

/// `movement::take_off(player, ride)` (18 §6): mounts `ride` on the server copy. The gates of
/// [`take_off_gate`] refuse; then the sim's ride parameters are installed and `OBJMSG_MODE_FLY(item, 0, force)`
/// is sent (the FLY flag, the fuel initialised once from the ride, `ClearState`), and the destination and the
/// destination angle are cleared (`DoEquip`). The equip itself, its notification and the stats are the item
/// layer's.
pub fn take_off(
    m: &mut Mover,
    ride: &ItemProp,
    force: bool,
    env: &SimEnv<'_>,
    rules: &mut dyn SimRules,
    rng: &mut dyn GameRng,
) -> Result<(), FlyRefusal> {
    take_off_gate(m, force, env)?;
    m.sim.params.ride = Some(ride_params(ride));
    let mut scratch = SimEvents::new();
    let r = send_act_msg(
        &mut m.sim,
        env,
        rules,
        rng,
        ActMsg::with(
            OBJMSG_MODE_FLY,
            [
                i32::try_from(ride.id).unwrap_or(0),
                0,
                i32::from(force),
                0,
                0,
            ],
        ),
        &mut scratch,
    );
    if r != ACT_OK {
        m.sim.params.ride = None;
        return Err(FlyRefusal::Refused);
    }
    m.sim.clear_dest();
    m.sim.clear_dest_angle();
    Ok(())
}

/// `movement::land(player, force)` (18 §6): `OBJMSG_MODE_GROUND(0, 0, force)` on the server copy, refused over
/// a `HATTR_NOWALK` cell unless forced; FLY and ACC go, the state is cleared, the pitch levelled; the
/// destination is cleared and the sim's ride parameters are dropped. A mover that does not fly is already
/// down (`Ok`).
pub fn land(
    m: &mut Mover,
    force: bool,
    env: &SimEnv<'_>,
    rules: &mut dyn SimRules,
    rng: &mut dyn GameRng,
) -> Result<(), FlyRefusal> {
    if !m.sim.act.flags.fly() {
        m.sim.params.ride = None;
        return Ok(());
    }
    let mut scratch = SimEvents::new();
    let r = send_act_msg(
        &mut m.sim,
        env,
        rules,
        rng,
        ActMsg::with(OBJMSG_MODE_GROUND, [0, 0, i32::from(force), 0, 0]),
        &mut scratch,
    );
    if r != ACT_OK {
        return Err(FlyRefusal::NoWalkCell);
    }
    m.sim.params.ride = None;
    m.sim.clear_dest();
    m.sim.clear_dest_angle();
    Ok(())
}

/// The item id of the worn ride, if any.
pub fn worn_ride(m: &Mover) -> Option<u32> {
    m.player()?
        .items
        .inventory
        .get_equip(PARTS_RIDE)
        .map(|it| it.item_id)
}

/// `RedoEquip`'s ride rule (ref/10 §13.6, `MoverEquip.cpp:1960-2040`): a worn ride that the mover does not fly
/// on makes it fly (`MODE_FLY`, forced), no ride on a flying mover lands it (`MODE_GROUND`, forced); a worn
/// ride other than the one in the sim's parameters replaces the parameters. The gates of [`take_off`] still
/// apply to the take-off (a login on a no-fly cell keeps the ride worn and the player on the ground).
pub fn reconcile(
    m: &mut Mover,
    data: &GameData,
    env: &SimEnv<'_>,
    rules: &mut dyn SimRules,
    rng: &mut dyn GameRng,
) -> Reconciled {
    if !m.is_player() || m.sim.act.state.is_die() {
        return Reconciled::InSync;
    }
    let flying = m.sim.act.flags.fly();
    match (worn_ride(m).and_then(|id| data.item(id)), flying) {
        (Some(prop), false) => match take_off(m, prop, true, env, rules, rng) {
            Ok(()) => Reconciled::TookOff,
            Err(why) => Reconciled::Refused(why),
        },
        (None, true) => match land(m, true, env, rules, rng) {
            Ok(()) => Reconciled::Landed,
            Err(why) => Reconciled::Refused(why),
        },
        (Some(prop), true) => {
            // the ride in the sim follows the worn one (a ride swapped in flight cannot happen through the
            // planner, but the data may have changed under a saved flyer)
            let want = ride_params(prop);
            if m.sim.params.ride != Some(want) {
                m.sim.params.ride = Some(want);
            }
            Reconciled::InSync
        }
        (None, false) => {
            if m.sim.params.ride.is_some() {
                m.sim.params.ride = None;
            }
            Reconciled::InSync
        }
    }
}

/// The flight EXP seam of the kill-EXP code (doc 16 §0.13, ref/10 §12.9): `m_nFxp` is a counter without any
/// effect in V15 (the flight level is derived from the character level, `SetFlightLv` is a no-op), so the only rule
/// is who may gain it: a killer that flies gets the monster's `nFxpValue`, anybody else nothing. WP-MOVER-21
/// owns the counter and the `SetFxp` message; this is the stub it calls.
pub fn flight_exp_for_kill(killer: &Mover, fxp_value: i32) -> i32 {
    if killer.sim.act.flags.fly() {
        fxp_value.max(0)
    } else {
        0
    }
}

// ---- the world phase ---------------------------------------------------------------------------------------------

/// The tick tail of one awake mover (called by the pipeline before the other consumers): [`reconcile`] for
/// players.
pub(super) fn tail(ctx: &mut FrameCtx<'_>, key: MoverKey) {
    let tick = ctx.tick;
    let Some(m) = ctx.store.movers.get_mut(key) else {
        return;
    };
    if !m.is_player() {
        return;
    }
    // the cheap test first: most players have no ride and are on the ground
    let worn = worn_ride(m).is_some();
    if !worn && !m.sim.act.flags.fly() && m.sim.params.ride.is_none() {
        return;
    }
    let shared = &*ctx.movement.shared;
    let lookup = FrameLookup::default();
    let env = SimEnv {
        frame: tick.frames_completed_after(),
        role: SimRole::Server,
        flags: shared.flags,
        world: &ctx.statics.collision,
        dynamic: ctx.dynamic,
        motions: &shared.motions,
        lookup: &lookup,
        mode: EventMode::Normal,
    };
    let r = reconcile(m, &shared.data, &env, &mut NoRules, &mut *ctx.rng);
    match r {
        Reconciled::InSync => {}
        Reconciled::TookOff | Reconciled::Landed => {
            tracing::debug!(
                player = m.core.id.0,
                ?r,
                "flying: the sim copy follows the ride"
            );
        }
        Reconciled::Refused(why) => {
            tracing::debug!(
                player = m.core.id.0,
                ?why,
                "flying: the ride cannot change the flight"
            );
        }
    }
}

/// `SimEvent::TurboOff` of a player (18 §6): the fuel ran out and the sim turned turbo off on its copy; the
/// viewers learn it from `SendActMsg{MODE_TURBO_OFF, is_fly: true}` (`AddSendActMsg`, once per run-out because
/// the sim raises the event only on the frame the flag goes).
pub(super) fn on_turbo_off(ctx: &mut FrameCtx<'_>, key: MoverKey) {
    if let Some(m) = ctx.store.movers.get_mut(key) {
        intents::broadcast_act_msg(m, OBJMSG_MODE_TURBO_OFF as i32, true, [0, 0, 0]);
    }
}

/// `SimEvent::EnteredNoFly` of a player (18 §6): the sim undid the step into the no-fly cell. The ride comes off
/// after the world phase ([`finish`]); the request is idempotent (the event repeats every frame the flyer pushes
/// against the border).
pub(super) fn on_no_fly_zone(ctx: &mut FrameCtx<'_>, player: ObjId) {
    ctx.movement.fly.unequip.insert(player);
}

/// Routes the flying events of the barrier (called by the dispatch table).
pub(super) fn on_event(
    ctx: &mut FrameCtx<'_>,
    key: MoverKey,
    id: ObjId,
    is_player: bool,
    ev: &SimEvent,
) {
    if !is_player {
        return;
    }
    match ev {
        SimEvent::TurboOff => on_turbo_off(ctx, key),
        SimEvent::EnteredNoFly => on_no_fly_zone(ctx, id),
        _ => {}
    }
}

// ---- the services context ----------------------------------------------------------------------------------------

/// The dismounts the world phase requested (`UnequipRide`, `MoverEquip.cpp:2608`): runs right after the phase, in
/// player-id order. Each is an unequip of the worn ride that ignores the terrain refusal of `IsUnEquipAble`
/// (the original `EquipItem` has none) but not an attack (`EquipItem` returns silently) nor a full bag (the
/// planner's `TID_GAME_LACKSPACE`: the flyer stays up at the border); on success the equip notification goes
/// out and the sim copy lands in the same tick.
pub fn finish(s: &mut Services, ctx: &mut GameCtx<'_>) {
    if ctx.worlds.movement.fly.is_empty() {
        return;
    }
    let requests = std::mem::take(&mut ctx.worlds.movement.fly.unequip);
    for id in requests {
        unequip_ride(s, ctx, id);
    }
}

fn unequip_ride(s: &mut Services, ctx: &mut GameCtx<'_>, id: ObjId) {
    let who = PlayerRef(id);
    let core = s.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    let now = ctx.clock.game_ms;
    let mut fx = Fx::new();
    let done = {
        let Some(m) = ctx.worlds.player_mut(id) else {
            return;
        };
        let Some(storage) = m
            .player()
            .and_then(|p| p.items.inventory.equip_id(PARTS_RIDE))
            .and_then(|sid| StorageId::try_from(sid).ok())
        else {
            return;
        };
        // the terrain refusal is not part of `UnequipRide`: plan with a neutral terrain
        let facts = EquipFacts {
            terrain: HeightAttr::None,
            ..EquipFacts::default()
        };
        toggle_equip(m, &env, storage, PARTS_RIDE as i32, &facts, now, &mut fx)
    };
    if done {
        fx.flush(s, ctx, who);
        land_now(ctx, id);
    }
    // a refusal (a full bag, an attack in progress) is silent like the original `EquipItem`: the event repeats
    // every frame the flyer pushes on, a text per tick would be spam
}

/// Lands the sim copy of `id` at the end of the world phase (frame `4n + 4`), forced.
fn land_now(ctx: &mut GameCtx<'_>, id: ObjId) {
    let frame = ctx.clock.tick.frames_completed_after();
    let shared = ctx.worlds.movement.shared.clone();
    let Some(statics) = ctx.worlds.statics_of(id).cloned() else {
        return;
    };
    let Some((world, layer)) = ctx.worlds.core(id).map(|c| (c.world, c.layer)) else {
        return;
    };
    let dynamic = ctx.worlds.dynamic_of(world, layer);
    let lookup = FrameLookup::default();
    let rng = ctx.rng.world(world);
    let Some(m) = ctx.worlds.player_mut(id) else {
        return;
    };
    let env = SimEnv {
        frame: SimFrame(frame.0),
        role: SimRole::Server,
        flags: shared.flags,
        world: &statics.collision,
        dynamic: &dynamic,
        motions: &shared.motions,
        lookup: &lookup,
        mode: EventMode::Normal,
    };
    let _ = land(m, true, &env, &mut NoRules, rng);
}
