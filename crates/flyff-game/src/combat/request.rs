//! The attack requests of the client (Plan/17 §3.2-§3.4; WP-COMBAT-05): `MeleeAttackReq`, `MagicAttackReq` and
//! `RangeAttackReq`, run in the player's handler scope.
//!
//! VR-004 (Plan/03): the message enums are validated, which the original did not (it passed any `OBJMSG`
//! through): `atk_msg` must be `ATK1..ATK4` for a melee swing, `ATK_MAGIC1` for a wand and `ATK_RANGE1` for a
//! bow; a charge (`magic_power`, `item_id`) must be 0..=4 (the client sends 4 at full charge, an MP-starved wand
//! fires at 3). An invalid request is dropped silently and counted in [`CombatStats::requests_dropped`]. The
//! target must exist in the attacker's layer. Everything else (alive, busy, steal, weapon) is the sim's and the
//! action layer's business ([`super::action`]).
//!
//! [`CombatStats::requests_dropped`]: super::world::CombatStats::requests_dropped

use super::action::{player_bow, player_melee, player_wand};
use super::context::with_action;
use crate::ctx::{GameCtx, PlayerRef, Services};
use flyff_common::ObjId;
use flyff_proto::pb;
use flyff_rules::combat::{MAX_CHARGE_LEVEL, objmsg};

fn drop_request(ctx: &mut GameCtx<'_>) {
    ctx.worlds.movement.combat.queues.stats.requests_dropped += 1;
}

/// `MeleeAttackReq` (ground and flying): `OnMeleeAttack` of `DPSrvr.cpp:4165-4246`.
pub fn on_melee_attack(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    req: pb::MeleeAttackReq,
) {
    if !(objmsg::ATK1..=objmsg::ATK4).contains(&req.atk_msg) {
        return drop_request(ctx);
    }
    let (me, target) = (who.0, ObjId(req.target));
    with_action(s, ctx, me, |cx| {
        player_melee(cx, me, req.atk_msg, target, req.param3);
    });
}

/// `MagicAttackReq` (wand): `OnMagicAttack` of `DPSrvr.cpp:4248-4274`.
pub fn on_magic_attack(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    req: pb::MagicAttackReq,
) {
    let power = u32::try_from(req.magic_power)
        .ok()
        .filter(|p| *p <= MAX_CHARGE_LEVEL as u32);
    let Some(power) = power.filter(|_| req.atk_msg == objmsg::ATK_MAGIC1) else {
        return drop_request(ctx);
    };
    let (me, target) = (who.0, ObjId(req.target));
    with_action(s, ctx, me, |cx| player_wand(cx, me, target, power));
}

/// `RangeAttackReq` (bow): `OnRangeAttack` of `DPSrvr.cpp:4276-4300`.
pub fn on_range_attack(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    req: pb::RangeAttackReq,
) {
    if req.atk_msg != objmsg::ATK_RANGE1 || req.item_id > MAX_CHARGE_LEVEL as u32 {
        return drop_request(ctx);
    }
    let (me, target) = (who.0, ObjId(req.target));
    with_action(s, ctx, me, |cx| player_bow(cx, me, target, req.item_id));
}
