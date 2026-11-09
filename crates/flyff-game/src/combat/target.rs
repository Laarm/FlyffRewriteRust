//! Target claims, kill-steal protection and the target notifications (Plan/17 §3.10, §8.5; WP-COMBAT-09;
//! `src:WORLDSERVER/DPSrvr.cpp:4329-4373`, `src:_Common/Mover.cpp:1306-1336, 8248-8268, 7118-7165`).
//!
//! * A player **claims** a monster (`target.id_targeter = me`): by starting to attack it (`SetJJim`, the sim's
//!   `JJim` event: [`set_jjim`]) or by `SetTargetReq` with `clear = 0` when nobody has claimed it yet.
//!   `clear = 1` releases a claim the player owns (and cancels a Hero Drawing cast in progress), `clear = 2`
//!   only remembers the target for party skills and the skill queue (`id_set_target`).
//! * **Kill-steal protection** (`events.steal = false`; the repack default `true` switches it off) refuses the
//!   attack of a second player on a claimed monster with `TID_GAME_PRIORITYMOB`: the rule is
//!   [`super::arbiter::steal_check`], wired into the attack start ([`super::context::StartRules`]), the damage
//!   pipeline (step 1 of `CheckValidDamageMsg`) and the projectile impact.
//! * A claim ends when the claimer is gone or farther than 32 m ([`process_target`], every 128th tick of an
//!   NPC), on the monster's death and with the claimer's logout.
//! * [`notify_set_target`] / [`notify_clear_target`] are the two messages the server uses to make a client drop
//!   or change its focus (`SetTargetNtf`, `ClearTarget`).

use super::context::with_action;
use super::effects::{Effects, SimFx};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::mover::Mover;
use crate::stats::out::Aud;
use crate::world::object::ObjectStore;
use flyff_common::ObjId;
use flyff_common::defs::obj::{MI_DEMIAN5, MI_KEAKOON5, MI_MUFFRIN5};
use flyff_common::defs::skill::{SI_KNT_HERO_DRAWING, SI_RIG_HERO_RETURN};
use flyff_data::GameData;
use flyff_proto::pb;
use flyff_rules::consts::dst::RANK_SUPER;

/// `process_target`'s radius: a claim or a remembered attacker farther away than this is dropped (`IsValidArea
/// (other, 32.0f)`).
pub const CLAIM_RANGE: f32 = 32.0;

/// `SetTargetReq` (`OnSetTarget`, VR-004: `clear` must be 0, 1 or 2): see the module documentation.
pub fn on_set_target(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    req: pb::SetTargetReq,
) {
    if req.clear > 2 {
        return;
    }
    let me = who.0;
    let target = ObjId(req.target);
    if req.clear == 2 {
        if let Some(m) = ctx.worlds.mover_mut(me) {
            m.combat.id_set_target = Some(target);
        }
        return;
    }
    // 0 / 1 need a valid target mover
    if ctx.worlds.mover(target).is_none_or(|t| t.core.deleted) {
        return;
    }
    if req.clear == 0 {
        if let Some(t) = ctx.worlds.mover_mut(target)
            && t.combat.id_targeter.is_none()
        {
            t.combat.id_targeter = Some(me);
        }
        return;
    }
    // clear == 1: only the owner releases a claim
    let owned = ctx
        .worlds
        .mover(target)
        .is_some_and(|t| t.combat.id_targeter == Some(me));
    if !owned {
        return;
    }
    if let Some(t) = ctx.worlds.mover_mut(target) {
        t.combat.id_targeter = None;
    }
    let rm_hero = ctx.cfg.rules.features.rm_hero_skill;
    let frame = ctx.clock.frame0.0;
    let cancel = ctx.worlds.mover(me).is_some_and(|m| {
        let casting = m.sim.skill.casting_skill;
        m.sim.skill.casting_end_frame > frame
            && (casting == SI_KNT_HERO_DRAWING || (!rm_hero && casting == SI_RIG_HERO_RETURN))
    });
    if cancel {
        with_action(s, ctx, me, |cx| {
            if let Some(m) = cx.mover_mut(me) {
                m.sim.skill.casting_end_frame = 0;
            }
            cx.arb.fx.sim.push((me, SimFx::ClearState));
        });
    }
}

/// `CMover::SetJJim(jjimer)` on `target` (`Mover.cpp:8248-8268`): a player that starts to attack an NPC claims it.
/// `RANK_SUPER` bosses and the event monsters stay free for everybody.
pub fn set_jjim(store: &mut ObjectStore, data: &GameData, jjimer: ObjId, target: ObjId) {
    let Some(j) = store.mover(jjimer) else { return };
    if !j.is_player() {
        return;
    }
    let Some(t) = store.mover(target) else { return };
    if t.is_player() {
        return;
    }
    if data
        .mover(t.prop_id())
        .is_some_and(|p| p.class == RANK_SUPER)
    {
        return;
    }
    // ---- event monsters (WP-EVENT-09): `CEventMonster::IsEventMonster` stays free for everybody (Mover.cpp:8257-8261)
    if crate::event::monsters::find(&data.lua.event_monsters, t.prop_id()).is_some() {
        return;
    }
    if matches!(t.prop_id(), MI_DEMIAN5 | MI_KEAKOON5 | MI_MUFFRIN5) {
        return;
    }
    if let Some(t) = store.mover_mut(target) {
        t.combat.id_targeter = Some(jjimer);
    }
}

/// `IsValidArea(other, 32)` seen from `from`: the same world and layer and a squared distance strictly below 32².
fn in_claim_range(from: &Mover, other: &Mover) -> bool {
    from.core.world == other.core.world
        && from.core.layer == other.core.layer
        && (other.core.pos - from.core.pos).length_squared() < CLAIM_RANGE * CLAIM_RANGE
}

/// `CMover::ProcessTarget` (`Mover.cpp:1306-1336`) of NPC `id`: the remembered attacker and the claimer are
/// dropped when they are gone or farther than 32 m.
pub fn process_target(store: &mut ObjectStore, id: ObjId) {
    let Some(me) = store.mover(id) else { return };
    let (attacker, targeter) = (me.combat.id_attacker, me.combat.id_targeter);
    if attacker.is_none() && targeter.is_none() {
        return;
    }
    let keep = |other: Option<ObjId>| {
        other.filter(|o| {
            store
                .mover(*o)
                .is_some_and(|m| !m.core.deleted && in_claim_range(me, m))
        })
    };
    let (attacker, targeter) = (keep(attacker), keep(targeter));
    if let Some(m) = store.mover_mut(id) {
        m.combat.id_attacker = attacker;
        m.combat.id_targeter = targeter;
    }
}

/// Releases every claim `player` holds (its logout, or a death): the monsters it claimed are free again. The
/// legacy sweep notices within 128 ticks; the explicit call makes the release immediate.
pub fn release_claims(store: &mut ObjectStore, player: ObjId) -> usize {
    let ids: Vec<ObjId> = store
        .ids()
        .filter(|id| {
            store.mover(*id).is_some_and(|m| {
                m.combat.id_targeter == Some(player) || m.combat.id_attacker == Some(player)
            })
        })
        .collect();
    for id in &ids {
        if let Some(m) = store.mover_mut(*id) {
            if m.combat.id_targeter == Some(player) {
                m.combat.id_targeter = None;
            }
            if m.combat.id_attacker == Some(player) {
                m.combat.id_attacker = None;
            }
        }
    }
    ids.len()
}

/// `AddSetTarget(target)`: the server changes the focus of `player` (`SetTargetNtf`, wire object 0). `None` is
/// the legacy `NULL_ID`, which the client treats as "drop it" (it has already, §3.10).
pub fn notify_set_target(fx: &mut Effects, player: ObjId, target: Option<ObjId>) {
    fx.push(
        player,
        Aud::SelfZero,
        pb::SetTargetNtf {
            target: target.map_or(u32::MAX, |t| t.0),
        },
    );
}

/// `AddClearTarget()`: the server forces `player`'s client to drop its focus.
pub fn notify_clear_target(fx: &mut Effects, player: ObjId) {
    fx.push(player, Aud::SelfZero, pb::ClearTarget {});
}
