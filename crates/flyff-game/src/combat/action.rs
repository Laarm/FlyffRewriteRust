//! The action layer (Plan/17 §3.1-§3.9; WP-COMBAT-05): starting attacks through the shared sim, the hit-frame
//! handlers that call the damage arbiter, and the API the monster brains use.
//!
//! **Starting an attack.** Every start goes through `flyff_sim::act::send_act_msg_cx` on the server's copy of the
//! mover ([`send_attack_msg`]). The sim consults [`StartRules`] for kill-steal and asks for the hit roll
//! (`GetAttackResult`); the roll is drawn from the world's stream right after the call returns, which is the same
//! position as in the original, and patched into the mover's `atk_flags` and the return value. The queued
//! attacks (melee: `m_qMeleeAtkMsg`, magic and bow: `m_qMagicAtkMsg`) live in the sim (`SimMover.act.queues`,
//! Plan/31 §6.6); the melee queue drains inside the sim, the magic queue at the `DrainMagicQueue` event
//! ([`drain_magic_queue`]).
//!
//! **Hit frames.** [`on_attr_hit`] dispatches the `AttrHit` events of the barrier ([`on_attack_melee`],
//! [`on_attack_magic`], [`on_attack_range`], [`on_attack_sp`]; skills go to the [`SkillHitHooks`] seam).
//!
//! **Monsters.** A monster's attack is started by its brain with [`npc_melee`] / [`npc_range`] / [`npc_special`]
//! (`DoAttackMelee` / `DoAttackRange` / `DoAttackSP`) from the AI pass; the notification to the viewers and the hit
//! roll are done at the barrier of the first frame by [`on_npc_attack_started`], which sees the target's stats.

use super::arbiter::{
    ApplyMask, AroundSpec, DamageRequest, around_candidates, damage_around_hits, process_damage,
};
use super::context::{ActionCx, StartRules};
use super::env::ArbiterCx;
use super::projectile::{HitPayload, ShotRequest, spawn_shot};
use super::projectile_kind::{kind_for_monster_range, kind_for_sfx};
use super::skillhit::{SkillHit, SkillHitHooks, SkillShot};
use super::state::{ActParams, ObjAct, is_die, is_live};
use super::view::{active_hand_prop, combatant, worn_item};
use crate::ai::{AiCx, AiMsg};
use crate::mover::Mover;
use crate::stats::out::{Aud, Out};
use crate::stats::pool::inc_mp;
use flyff_common::ObjId;
use flyff_common::defs::attribute::{HD_TWO, SRO_AROUND, WT_RANGE, WT_RANGE_BOW};
use flyff_common::defs::itemkind::{IK3_ARROW, IK3_WAND, IK3_YOYO};
use flyff_common::defs::neuz::{MTI_ATK_13, MTI_STAND_13, PARTS_BULLET};
use flyff_common::defs::obj::{
    MI_BIGMUSCLE, MI_CLOCKWORK1, MI_DU_METEONYKER, MI_DU_METEONYKER2, MI_DU_METEONYKER3,
    MI_DU_METEONYKER4, MI_KRRR, MI_LOADCLOCKWORK, MI_MUSHMOOT, XI_FIR_WAND01,
};
use flyff_common::defs::skill::SI_MER_ONE_REFLEXHIT;
use flyff_proto::pb;
use flyff_rules::combat::flags::wand_pays_mp;
use flyff_rules::combat::{
    AtkFlags, CombatantView, DamageParam, Hand, MAX_CHARGE_LEVEL, attack_result, objmsg,
};
use flyff_rules::consts::NULL_ID;
use flyff_rules::consts::job::{JOB_PSYCHIKEEPER, JOB_PSYCHIKEEPER_HERO, JOB_PSYCHIKEEPER_MASTER};
use flyff_rules::consts::parts::PARTS_RWEAPON;
use flyff_sim::mover::WeaponStance;
use flyff_sim::step::HitKind;
use flyff_sim::types::{
    ACT_BAD_TARGET, ACT_OK, AF_GENERIC, AF_MISS, MTI_ATK1, OBJMSG_ATK_MAGIC1, OBJMSG_ATK_RANGE1,
    OBJMSG_ATK1, OBJMSG_SP_ATK1, OBJMSG_STOP, OBJSTA_ATK_MELEESKILL, OBJSTA_ATK1,
};
use flyff_sim::{ActMsg, num};

/// The attack-speed animation factor a started action carries on the wire (`ani_speed`, PL-087): the sim plays a
/// player's attack at `attack_speed`, everything else at 1.0 (`ProcessAniSpeed`, Plan/31 §12.7).
pub fn start_ani_speed(m: &Mover) -> f32 {
    if m.is_player() {
        m.sim.params.attack_speed
    } else {
        1.0
    }
}

/// `true` when `target` is a mover of the attacker's layer (`prj.GetMover` + `IsValidObj`).
pub fn target_valid(cx: &ActionCx<'_>, att: ObjId, target: ObjId) -> bool {
    match (cx.mover(att), cx.mover(target)) {
        (Some(a), Some(t)) => a.core.world == t.core.world && a.core.layer == t.core.layer,
        _ => false,
    }
}

/// Writes the weapon of the right hand into the sim's motion inputs (`SimMoverParams.stance`): the motion offset
/// of the attack and the wand / bow refusal of the combo attack read it. The equipment code does this on a
/// change of the weapon; an attack start re-reads it, so it cannot be stale.
pub fn sync_weapon_stance(m: &mut Mover, core: &crate::stats::StatsCore) {
    if !m.is_player() {
        return;
    }
    let right = worn_item(m, PARTS_RWEAPON)
        .and_then(|w| core.data.item(w.item_id))
        .or(core.rules.hand.as_ref());
    let Some(prop) = right else { return };
    let dual = combatant(m, core, Hand::Right, None).is_dual_weapon();
    let job = u32::try_from(m.mv.job).unwrap_or(u32::MAX);
    let stance = WeaponStance {
        kind3: prop.item_kind3,
        two_handed: prop.handed == HD_TWO,
        dual,
        master_model: false,
        psy_job: matches!(
            job,
            JOB_PSYCHIKEEPER | JOB_PSYCHIKEEPER_MASTER | JOB_PSYCHIKEEPER_HERO
        ),
    };
    if m.sim.params.stance != Some(stance) {
        m.sim.params.stance = Some(stance);
    }
}

/// `GetAttackResult(victim)` of `att` against `def`, drawn from the world's stream (`attack_result`).
pub fn roll_attack_result(cx: &mut ActionCx<'_>, att: ObjId, def: ObjId) -> bool {
    let core = cx.arb.env.core;
    let ArbiterCx { w, hooks, .. } = &mut cx.arb;
    let (Some(a), Some(d)) = (
        w.store.mover(att).filter(|m| !m.core.deleted),
        w.store.mover(def).filter(|m| !m.core.deleted),
    ) else {
        return true;
    };
    let event_victim = a.is_player() && !d.is_player() && hooks.is_event_spawn_monster(d.prop_id());
    let a_view = combatant(a, core, Hand::Right, None);
    let d_view = combatant(d, core, Hand::Right, None);
    attack_result(&a_view, &d_view, event_victim, &mut *w.rng)
}

/// `SendActMsg(am)` for an attack start on the server copy of `att` (see the module documentation): kill-steal
/// answered by the pre-evaluated rule (and `TID_GAME_PRIORITYMOB` sent), the hit roll drawn after the call and
/// patched into `atk_flags` and the result. `ACT_BAD_TARGET` when the mover does not exist.
pub fn send_attack_msg(cx: &mut ActionCx<'_>, att: ObjId, target: ObjId, am: ActMsg) -> i32 {
    let mut rules = StartRules::for_attack(cx, att, target);
    let Some(mut ret) = cx.sim_call(att, &[target], &mut rules, |sim, c| {
        flyff_sim::act::send_act_msg_cx(sim, c, am)
    }) else {
        return ACT_BAD_TARGET;
    };
    if rules.stole() {
        cx.arb.fx.to_self(
            att,
            crate::chat::ingress::notice::defined_text(
                flyff_common::defs::text::TID_GAME_PRIORITYMOB,
                Vec::new(),
            ),
        );
    }
    if let Some((a, d)) = rules.roll_wanted()
        && ret == AF_GENERIC as i32
        && !roll_attack_result(cx, a, d)
    {
        if let Some(m) = cx.mover_mut(a) {
            m.sim.act.atk_flags = AF_MISS;
        }
        ret = AF_MISS as i32;
    }
    ret
}

/// `SendActMsg` without the attack rules (stop, die, stand ...).
pub fn send_plain_msg(cx: &mut ActionCx<'_>, id: ObjId, am: ActMsg) -> i32 {
    let mut rules = StartRules::plain();
    cx.sim_call(id, &[], &mut rules, |sim, c| {
        flyff_sim::act::send_act_msg_cx(sim, c, am)
    })
    .unwrap_or(ACT_BAD_TARGET)
}

/// `SetAngle(GetDegree(target, pos))`: the attacker turns to the target (the sim quantises the angle).
pub(crate) fn face_target(cx: &mut ActionCx<'_>, att: ObjId, target: ObjId) {
    let Some(tp) = cx.mover(target).map(|t| t.sim.pos) else {
        return;
    };
    if let Some(a) = cx.mover_mut(att) {
        a.sim.angle = num::set_angle_quantized(num::get_degree(tp, a.sim.pos));
    }
}

/// `SetActParam(act, target, p1, ...)`.
fn set_act(cx: &mut ActionCx<'_>, att: ObjId, act: ObjAct, p: [i64; 5], projectile_id: u32) {
    if let Some(m) = cx.mover_mut(att) {
        m.combat.act = ActParams {
            act,
            p,
            projectile_id,
        };
    }
}

/// An attack notification to the viewers of the attacker (not to itself: its client predicted the swing).
fn emit_ntf(cx: &mut ActionCx<'_>, att: ObjId, body: impl Into<pb::server_msg::Msg>) {
    cx.arb.fx.push(att, Aud::NearExclSelf, body);
}

// ---- player attack starts ------------------------------------------------------------------------------------------

/// `MeleeAttackReq` after the VR-004 checks (Plan/17 §3.2 steps 3-7): `msg` is `ATK1..ATK4`. The low word of
/// `param3` is forced to 0 so the server always rolls the hit itself; a refused-but-busy swing (`0`) is queued
/// in the sim's melee queue; everything but `-2` is relayed.
pub fn player_melee(cx: &mut ActionCx<'_>, att: ObjId, msg: u32, target: ObjId, param3: i32) {
    if !target_valid(cx, att, target) {
        return;
    }
    let core = cx.arb.env.core;
    if let Some(m) = cx.mover_mut(att) {
        sync_weapon_stance(m, core);
    }
    let p3 = param3 & !0xFFFF;
    let am = ActMsg::with(msg, [target.0 as i32, 0, p3, 0, 0]);
    let ret = send_attack_msg(cx, att, target, am);
    if ret == 0
        && let Some(m) = cx.mover_mut(att)
    {
        flyff_sim::sync::enqueue_melee(
            &mut m.sim,
            flyff_sim::mover::QueuedAct {
                msg,
                p1: target.0 as i32,
                p2: 0,
                p3,
            },
        );
    }
    if ret != flyff_sim::types::ACT_DROP {
        let (frame, speed) = (cx.sc.frame.0, cx.mover(att).map_or(1.0, start_ani_speed));
        emit_ntf(
            cx,
            att,
            pb::MeleeAttackNtf {
                atk_msg: msg,
                target: target.0,
                param2: 0,
                param3: p3,
                server_frame: frame,
                ani_speed: speed,
            },
        );
    }
}

/// `CMover::DoAttackMagic` (`MoverSkill.cpp:3224-3302`) for a player: `-1` for an invalid target, no weapon or a
/// weapon that is not a wand; otherwise the sim's answer. On a start (`1`): the attacker turns to the target
/// (not while flying), the act parameters are set, a charge of 1 or more pays one action point, a charge of 3
/// pays the wand's MP (KQ-05) and `MagicAttackNtf` goes to the viewers.
pub fn do_attack_magic(
    cx: &mut ActionCx<'_>,
    att: ObjId,
    target: ObjId,
    power: u32,
    projectile_id: u32,
) -> i32 {
    if !target_valid(cx, att, target) {
        return -1;
    }
    let core = cx.arb.env.core;
    let weapon = {
        let Some(m) = cx.mover(att) else { return -1 };
        worn_item(m, PARTS_RWEAPON).and_then(|w| core.data.item(w.item_id))
    };
    let Some(prop) = weapon else { return -1 };
    if prop.item_kind3 != IK3_WAND {
        return -1;
    }
    let req_mp = prop.req_mp;
    if let Some(m) = cx.mover_mut(att) {
        sync_weapon_stance(m, core);
    }
    let ret = send_attack_msg(
        cx,
        att,
        target,
        ActMsg::with(OBJMSG_ATK_MAGIC1, [target.0 as i32, 0, 0, 0, 0]),
    );
    if ret != ACT_OK {
        return ret;
    }
    let flying = cx.mover(att).is_some_and(|m| m.sim.act.flags.fly());
    if !flying {
        face_target(cx, att, target);
    }
    set_act(
        cx,
        att,
        ObjAct::MagicAttack,
        [i64::from(target.0), i64::from(power), 0, 0, 0],
        projectile_id,
    );
    let mut out = Out::new();
    if let Some(m) = cx.arb.w.store.mover_mut(att)
        && m.is_player()
    {
        if power >= 1
            && let Some(p) = m.player_mut()
            && p.skills_rt.action_points < 100
        {
            p.skills_rt.action_points += 1;
        }
        // KQ-05: only the 4th charge stage (3) pays
        if wand_pays_mp(power) {
            let n = flyff_rules::skill::cost::req_mp(
                &m.mv.dst.table,
                i32::try_from(req_mp).unwrap_or(0),
            );
            inc_mp(m, core, n.wrapping_neg(), &mut out);
        }
    }
    cx.arb.fx.extend_out(att, out);
    let (frame, speed) = (cx.sc.frame.0, cx.mover(att).map_or(1.0, start_ani_speed));
    emit_ntf(
        cx,
        att,
        pb::MagicAttackNtf {
            atk_msg: OBJMSG_ATK_MAGIC1,
            target: target.0,
            param2: 0,
            param3: 0,
            magic_power: i32::try_from(power).unwrap_or(0),
            projectile_id,
            server_frame: frame,
            ani_speed: speed,
        },
    );
    ret
}

/// `MagicAttackReq` after the VR-004 checks: `DoAttackMagic`, queued in the sim's magic queue when busy.
pub fn player_wand(cx: &mut ActionCx<'_>, att: ObjId, target: ObjId, power: u32) {
    if !target_valid(cx, att, target) {
        return;
    }
    let pid = cx.q.projectiles.alloc_id();
    let ret = do_attack_magic(cx, att, target, power, pid);
    let flying = cx.mover(att).is_none_or(|m| m.sim.act.flags.fly());
    if ret == 0
        && !flying
        && let Some(m) = cx.mover_mut(att)
    {
        flyff_sim::sync::enqueue_magic(
            &mut m.sim,
            flyff_sim::mover::QueuedMagic {
                target,
                kind: flyff_sim::mover::MagicKind::Magic {
                    power: i32::try_from(power).unwrap_or(0),
                },
                sfx_hit: 0,
            },
        );
    }
}

/// The attack item of `att` for a ranged attack: a player's weapon, a monster's named item (0 / `NULL_ID` = its
/// hand item).
fn range_prop<'a>(
    m: &Mover,
    core: &'a crate::stats::StatsCore,
    item: u32,
) -> Option<&'a flyff_data::props::item::ItemProp> {
    if m.is_player() || item == NULL_ID || item == 0 {
        active_hand_prop(m, core)
    } else {
        core.data.item(item)
    }
}

/// `CMover::DoAttackRange` (`MoverSkill.cpp:3315-3410`): a bow (or a monster's ranged item). `item` is the charge
/// 0..4 of a player and the item id of a monster (0 / `NULL_ID` = its hand item). `-1` for an invalid target, a
/// player without an arrow in the bullet part, a weapon that is not a bow; otherwise the sim's answer, and on a
/// start: face the target, act parameters, `RangeAttackNtf` and (without `features.no_arrows_poster`) one arrow.
pub fn do_attack_range(
    cx: &mut ActionCx<'_>,
    att: ObjId,
    target: ObjId,
    item: u32,
    projectile_id: u32,
) -> i32 {
    if !target_valid(cx, att, target) {
        return -1;
    }
    let core = cx.arb.env.core;
    if cx.sc.shared.flags.fix_ranger_roller {
        send_plain_msg(cx, att, ActMsg::new(OBJMSG_STOP));
    }
    let (is_player, use_motion, weapon_ok) = {
        let Some(m) = cx.mover(att) else { return -1 };
        if m.is_player() {
            // the arrow in the bullet part
            let arrow = m
                .player()
                .and_then(|p| p.items.inventory.get_equip(PARTS_BULLET))
                .and_then(|i| core.data.item(i.item_id));
            if arrow.is_none_or(|a| a.item_kind3 != IK3_ARROW) {
                return -1;
            }
        }
        let Some(prop) = range_prop(m, core, item) else {
            return -1;
        };
        let ok = prop.weapon_type == WT_RANGE || prop.weapon_type == WT_RANGE_BOW;
        let motion = if prop.use_motion != NULL_ID {
            prop.use_motion
        } else if m.is_player() {
            MTI_ATK_13 - MTI_STAND_13
        } else {
            MTI_ATK1
        };
        (m.is_player(), motion, ok)
    };
    if !weapon_ok {
        return -1;
    }
    if let Some(m) = cx.mover_mut(att) {
        sync_weapon_stance(m, core);
    }
    let ret = send_attack_msg(
        cx,
        att,
        target,
        ActMsg::with(
            OBJMSG_ATK_RANGE1,
            [target.0 as i32, use_motion as i32, 0, 0, 0],
        ),
    );
    if ret != ACT_OK {
        return ret;
    }
    face_target(cx, att, target);
    set_act(
        cx,
        att,
        ObjAct::RangeAttack,
        [i64::from(target.0), i64::from(item), 0, 0, 0],
        projectile_id,
    );
    let (frame, speed) = (cx.sc.frame.0, cx.mover(att).map_or(1.0, start_ani_speed));
    emit_ntf(
        cx,
        att,
        pb::RangeAttackNtf {
            atk_msg: OBJMSG_ATK_RANGE1,
            target: target.0,
            item_id: item,
            param3: 0,
            projectile_id,
            server_frame: frame,
            ani_speed: speed,
        },
    );
    if is_player && !cx.act.no_arrows_poster {
        cx.arb.hooks.arrow_down(&mut cx.arb.w, &mut cx.arb.fx, att);
    }
    ret
}

/// `RangeAttackReq` after the VR-004 checks: `DoAttackRange`, queued in the sim's magic queue when busy.
pub fn player_bow(cx: &mut ActionCx<'_>, att: ObjId, target: ObjId, power: u32) {
    if !target_valid(cx, att, target) {
        return;
    }
    let pid = cx.q.projectiles.alloc_id();
    if do_attack_range(cx, att, target, power, pid) == 0
        && let Some(m) = cx.mover_mut(att)
    {
        flyff_sim::sync::enqueue_magic(
            &mut m.sim,
            flyff_sim::mover::QueuedMagic {
                target,
                kind: flyff_sim::mover::MagicKind::Range {
                    item: i32::try_from(power).unwrap_or(0),
                },
                sfx_hit: 0,
            },
        );
    }
}

/// `CMagicAtkMsgq::Process` at the end of an attack state (`DrainMagicQueue` event, Plan/17 §3.1 step 2): the head
/// is started as a wand or bow attack; `1` pops and stops, `0` stops and keeps it, a negative answer pops and
/// continues.
pub fn drain_magic_queue(cx: &mut ActionCx<'_>, att: ObjId) {
    use flyff_sim::mover::MagicKind;
    loop {
        let Some(head) = cx.mover(att).and_then(|m| m.sim.act.queues.magic.head()) else {
            return;
        };
        let pid = cx.q.projectiles.alloc_id();
        let ret = match head.kind {
            MagicKind::Magic { power } => {
                do_attack_magic(cx, att, head.target, u32::try_from(power).unwrap_or(0), pid)
            }
            MagicKind::Range { item } => {
                do_attack_range(cx, att, head.target, u32::try_from(item).unwrap_or(0), pid)
            }
        };
        if ret == 0 {
            return;
        }
        if let Some(m) = cx.mover_mut(att) {
            m.sim.act.queues.magic.pop();
        }
        if ret > 0 {
            return;
        }
    }
}

// ---- monster attacks (the API of the brains, 18 §9.7) -----------------------------------------------------------

/// `CMover::DoAttackMelee(target, ATK1, item)` for a brain (`MoverSkill.cpp:3466-3515`): stop, then
/// `SendActMsg(ATK1, target, 0, 0)`; on a start the act parameters name the attack item (0 = the hand item).
/// Returns `1` (the original's `TRUE`) once an attempt was made, `0` for an invalid target or a missing item.
/// The hit roll and the `MeleeAttackNtf` follow at the barrier of the first frame ([`on_npc_attack_started`]).
pub fn npc_melee(m: &mut Mover, cx: &mut AiCx<'_>, target: ObjId, item: Option<u32>) -> i32 {
    npc_melee_msg(m, cx, target, OBJMSG_ATK1, item)
}

/// `CMover::DoAttackMelee(target, msg, item)` (`MoverSkill.cpp:3466-3518`): [`npc_melee`] with the attack message
/// of the caller, `OBJMSG_ATK1 .. OBJMSG_ATK4` (the boss brains, 18 §10.2). `STOP`, an invalid target gives 0,
/// a missing item prop gives 0, then `SendActMsg(msg, target)`; on a start the act parameters name the target and
/// the item. Returns `1` even when the sim refused the swing.
pub fn npc_melee_msg(
    m: &mut Mover,
    cx: &mut AiCx<'_>,
    target: ObjId,
    msg: u32,
    item: Option<u32>,
) -> i32 {
    cx.send_act(m, ActMsg::new(OBJMSG_STOP));
    if cx.lookup.snapshot(target).is_none() {
        return 0;
    }
    let id = item.unwrap_or(NULL_ID);
    let prop_ok = if id == NULL_ID || id == 0 {
        cx.data().item(m.mv.virt_item).is_some()
    } else {
        cx.data().item(id).is_some()
    };
    if !prop_ok {
        return 0;
    }
    let r = cx.send_act(m, ActMsg::with(msg, [target.0 as i32, 0, 0, 0, 0]));
    if r > 0 {
        m.combat.act = ActParams {
            act: ObjAct::MeleeAttack,
            p: [i64::from(target.0), i64::from(id), 0, 0, 0],
            projectile_id: 0,
        };
    }
    1
}

/// `CMover::DoAttackRange(target, item, id)` for a brain: the item (0 / `NULL_ID` = the hand item) must be a bow
/// or ranged weapon; `-1` when it is not, otherwise the sim's answer. Every monster ranged attack uses the
/// projectile path ([`on_attack_range`]); the notification follows at the barrier.
pub fn npc_range(m: &mut Mover, cx: &mut AiCx<'_>, target: ObjId, item: u32) -> i32 {
    cx.send_act(m, ActMsg::new(OBJMSG_STOP));
    if cx.lookup.snapshot(target).is_none() {
        return -1;
    }
    let data = cx.data();
    let prop = if item == NULL_ID || item == 0 {
        data.item(m.mv.virt_item)
    } else {
        data.item(item)
    };
    let Some(prop) = prop else { return -1 };
    if prop.weapon_type != WT_RANGE && prop.weapon_type != WT_RANGE_BOW {
        return -1;
    }
    let motion = if prop.use_motion == NULL_ID {
        MTI_ATK1
    } else {
        prop.use_motion
    };
    let r = cx.send_act(
        m,
        ActMsg::with(OBJMSG_ATK_RANGE1, [target.0 as i32, motion as i32, 0, 0, 0]),
    );
    if r == ACT_OK {
        m.combat.act = ActParams {
            act: ObjAct::RangeAttack,
            p: [i64::from(target.0), i64::from(item), 0, 0, 0],
            projectile_id: 0,
        };
    }
    r
}

/// `CMover::DoAttackSP(target, item)` for a brain (`MoverSkill.cpp:3413-3463`): the special attack item (0 /
/// `NULL_ID` = the hand item) names the motion; `-1` for an invalid target, `0` for a missing item.
pub fn npc_special(m: &mut Mover, cx: &mut AiCx<'_>, target: ObjId, item: u32) -> i32 {
    if cx.lookup.snapshot(target).is_none() {
        return -1;
    }
    let data = cx.data();
    let prop = if item == NULL_ID || item == 0 {
        data.item(m.mv.virt_item)
    } else {
        data.item(item)
    };
    let Some(prop) = prop else { return 0 };
    let motion = if prop.use_motion == NULL_ID {
        MTI_ATK1
    } else {
        prop.use_motion
    };
    let r = cx.send_act(
        m,
        ActMsg::with(
            OBJMSG_SP_ATK1,
            [target.0 as i32, item as i32, motion as i32, 0, 0],
        ),
    );
    if r == ACT_OK {
        m.combat.act = ActParams {
            act: ObjAct::SpAttack,
            p: [i64::from(target.0), i64::from(item), 0, 0, 0],
            projectile_id: 0,
        };
    }
    r
}

/// A monster started an attack (`AttackStarted` at the barrier of its first frame): the hit roll of a melee swing
/// (the sim answered with the placeholder `AF_GENERIC`; the real `GetAttackResult` is drawn here, where the
/// target's stats are at hand) and the notification to the viewers.
pub fn on_npc_attack_started(cx: &mut ActionCx<'_>, att: ObjId, msg: u32, target: ObjId) {
    let (frame, act) = (
        cx.sc.frame.0.saturating_sub(1),
        cx.mover(att).map(|m| m.combat.act),
    );
    let Some(act) = act else { return };
    match msg {
        objmsg::ATK1 | objmsg::ATK2 | objmsg::ATK3 | objmsg::ATK4 => {
            let flags = cx.mover(att).map_or(0, |m| m.sim.act.atk_flags);
            let mut flags = flags;
            if flags == AF_GENERIC
                && target_valid(cx, att, target)
                && !roll_attack_result(cx, att, target)
            {
                flags = AF_MISS;
                if let Some(m) = cx.mover_mut(att) {
                    m.sim.act.atk_flags = AF_MISS;
                }
            }
            emit_ntf(
                cx,
                att,
                pb::MeleeAttackNtf {
                    atk_msg: msg,
                    target: target.0,
                    param2: 0,
                    param3: (flags << 16) as i32,
                    server_frame: frame,
                    ani_speed: 1.0,
                },
            );
        }
        OBJMSG_SP_ATK1 => emit_ntf(
            cx,
            att,
            pb::SpAttack {
                atk_msg: msg,
                target: target.0,
                item_id: u32::try_from(act.p[1]).unwrap_or(0),
                param3: 0,
                server_frame: frame,
                ani_speed: 1.0,
            },
        ),
        OBJMSG_ATK_RANGE1 => emit_ntf(
            cx,
            att,
            pb::RangeAttackNtf {
                atk_msg: msg,
                target: target.0,
                item_id: u32::try_from(act.p[1]).unwrap_or(0),
                param3: 0,
                projectile_id: 0,
                server_frame: frame,
                ani_speed: 1.0,
            },
        ),
        _ => {}
    }
}

// ---- hit frames -----------------------------------------------------------------------------------------------------

/// What an `AttrHit` event carries.
#[derive(Clone, Copy, Debug)]
pub struct HitEvent {
    /// What hit.
    pub kind: HitKind,
    /// The state word at the hit.
    pub state: u32,
    /// The target.
    pub target: ObjId,
    /// The hit index of a multi-hit skill.
    pub hit_index: i32,
    /// The frame being stepped.
    pub frame: u64,
}

/// `OnAttrHit` (Plan/17 §3.8): dispatches the hit frame of `att` on its sub-state.
pub fn on_attr_hit(cx: &mut ActionCx<'_>, skill: &mut dyn SkillHitHooks, att: ObjId, ev: HitEvent) {
    cx.q.stats.hit_events += 1;

    // a mover at 0 HP that has not died yet (the death waits for the service step) hits nothing
    if cx.mover(att).is_none_or(is_die) {
        return;
    }
    match ev.kind {
        HitKind::Melee => on_attack_melee(cx, att, ev.state, ev.target),
        HitKind::FlyMelee => {
            if cx.mover(ev.target).is_some() {
                process_damage(
                    &mut cx.arb,
                    DamageRequest::new(ev.target, att, AtkFlags::GENERIC, DamageParam(0)),
                );
            }
        }
        // the wand, bow and special-attack handlers read the target of the act parameters (`GetActParam(0)`), the
        // sim's `id_target` is not set by every start
        HitKind::Wand => on_attack_magic(cx, att, act_target(cx, att, ev.target)),
        HitKind::Range => on_attack_range(cx, att, act_target(cx, att, ev.target)),
        HitKind::Special => on_attack_sp(cx, att, act_target(cx, att, ev.target)),
        HitKind::MeleeSkill | HitKind::MagicSkill => skill.on_skill_hit(
            cx,
            &SkillHit {
                attacker: att,
                kind: ev.kind,
                target: ev.target,
                hit_index: ev.hit_index,
                state: ev.state,
                frame: ev.frame,
            },
        ),
    }
}

/// `GetActParam(0)`: the target the attack was started against; the sim's own target when no act is recorded.
fn act_target(cx: &ActionCx<'_>, att: ObjId, fallback: ObjId) -> ObjId {
    cx.mover(att)
        .filter(|m| m.combat.act.act != ObjAct::None)
        .and_then(|m| u32::try_from(m.combat.act.p[0]).ok())
        .map_or(fallback, ObjId)
}

/// A repeated shot of a multi-shot skill.
pub fn on_repeat_shot(
    cx: &mut ActionCx<'_>,
    skill: &mut dyn SkillHitHooks,
    att: ObjId,
    kind: flyff_sim::step::ShotKind,
    frame: u64,
) {
    skill.on_repeat_shot(
        cx,
        &SkillShot {
            attacker: att,
            kind,
            frame,
        },
    );
}

/// `true` for the boss models whose melee hit is a script of their brain (doc 18).
fn is_boss_model(cx: &ActionCx<'_>, id: u32) -> bool {
    matches!(id, MI_CLOCKWORK1 | MI_BIGMUSCLE | MI_KRRR | MI_MUSHMOOT)
        || (id == MI_LOADCLOCKWORK && cx.act.better_clockwork_alpha)
        || matches!(
            id,
            MI_DU_METEONYKER | MI_DU_METEONYKER2 | MI_DU_METEONYKER3 | MI_DU_METEONYKER4
        )
}

/// `CMover::OnAttackMelee(state, target)` (`MoverActEvent.cpp:1650-1760`, Plan/17 §3.8).
pub fn on_attack_melee(cx: &mut ActionCx<'_>, att: ObjId, state: u32, target: ObjId) {
    let Some(a) = cx.mover(att) else { return };
    let (is_player, mover_id) = (a.is_player(), a.prop_id());
    if !is_player {
        // `PostAIMsg(AIMSG_ATTACK_MELEE)`
        cx.arb.fx.mail.push((att, AiMsg::AttackMelee));
    }
    if !is_player && is_boss_model(cx, mover_id) {
        // the boss handlers are the brains' (doc 18, `ai::boss::hits`: ClockWorks since WP-AI-18; the others
        // still answer `false` and the swing is an ordinary hit); the hook is an override that goes first
        if cx
            .arb
            .hooks
            .boss_attack_melee(&mut cx.arb.w, &mut cx.arb.fx, att, state, target)
            || crate::ai::boss::hits::on_attack_melee(cx, att, state, target)
        {
            return;
        }
    }
    if !is_player
        && (cx.arb.hooks.monster_skill_apply(
            &mut cx.arb.w,
            &mut cx.arb.fx,
            att,
            target,
            super::hooks::MonsterSkillTrigger::Melee,
        ) || crate::ai::monster_skill::apply(
            cx,
            att,
            target,
            super::hooks::MonsterSkillTrigger::Melee,
        ))
    {
        return;
    }
    // KQ-08: a monster's swing at a player more than 10 m away does nothing
    if !is_player && cx.mover(target).is_some_and(Mover::is_player) {
        let in_range = match (cx.mover(target), cx.mover(att)) {
            (Some(t), Some(a)) => {
                let tr = flyff_sim::bounds::radius(&t.sim.params.bounds, t.sim.params.scale);
                let ar = flyff_sim::bounds::radius(&a.sim.params.bounds, a.sim.params.scale);
                flyff_sim::bounds::is_range_obj(t.sim.pos, tr, a.sim.pos, ar, 10.0)
            }
            _ => false,
        };
        if !in_range {
            return;
        }
    }
    let damage = on_damage(cx, att, state, target);
    if damage <= 0 {
        return;
    }
    // Reflex Hit: a victim in the middle of the skill gathers the damage
    if let Some(t) = cx.mover_mut(target)
        && t.sim.act.state.bits() & OBJSTA_ATK_MELEESKILL != 0
        && t.sim.skill.skill == SI_MER_ONE_REFLEXHIT
    {
        t.combat.reflex_dmg = t.combat.reflex_dmg.wrapping_add(damage);
    }
    // +1 action point for an ATK1 hit
    if state == OBJSTA_ATK1
        && let Some(p) = cx.mover_mut(att).and_then(|m| m.player_mut())
        && p.skills_rt.action_points < 100
    {
        p.skills_rt.action_points += 1;
    }
}

/// `CMover::OnDamage(item, state, target)` (`MoverActEvent.cpp:1542-1643`): the damage of one swing. Returns the
/// damage dealt (0 for a yo-yo splash, which counts as nothing, KEEP-QUIRK; the splash exists only with
/// `features.splash_autoattacks` on, which is off by default since owner decision #007).
pub fn on_damage(cx: &mut ActionCx<'_>, att: ObjId, state: u32, target: ObjId) -> i32 {
    let core = cx.arb.env.core;
    let (is_player, atk_flags, act_item, yoyo, hand_item, dual) = {
        let Some(a) = cx.mover(att) else { return 0 };
        let weapon = if a.is_player() {
            worn_item(a, PARTS_RWEAPON).and_then(|w| core.data.item(w.item_id))
        } else {
            None
        };
        (
            a.is_player(),
            a.sim.act.atk_flags,
            u32::try_from(a.combat.act.p[1]).unwrap_or(0),
            weapon.is_some_and(|p| p.item_kind3 == IK3_YOYO),
            weapon.map_or(0, |p| p.id),
            a.is_player() && combatant(a, core, Hand::Right, None).is_dual_weapon(),
        )
    };
    let mut param = 1_u32;
    if is_player && dual && atk_flags & AF_GENERIC != 0 {
        param |= match state {
            OBJSTA_ATK1 => 0x01,
            s if s == flyff_sim::types::OBJSTA_ATK2 => 0x02,
            s if s == flyff_sim::types::OBJSTA_ATK3 => 0x01,
            s if s == flyff_sim::types::OBJSTA_ATK4 => 0x03,
            _ => 0,
        };
    }
    if is_player {
        if cx.act.splash_autoattacks && yoyo {
            let target_is_player = cx.mover(target).is_some_and(Mover::is_player);
            let apply = if target_is_player {
                ApplyMask::PLAYER
            } else {
                ApplyMask::MONSTER
            };
            let Some(pos) = cx.mover(target).map(|t| t.core.pos) else {
                return 0;
            };
            let spec = AroundSpec {
                center: Some(target),
                pos,
                flags: AtkFlags::GENERIC,
                attacker: att,
                apply,
                attack_id: hand_item,
                range: 5.0,
                target_ratio: 1.0,
            };
            around(cx, &spec);
            return 0;
        }
        // KQ-03: the repack ignores the pre-rolled miss of a player's swing
        let flags = if cx.act.player_melee_can_miss {
            AtkFlags(atk_flags)
        } else {
            AtkFlags::GENERIC
        };
        return process_damage(
            &mut cx.arb,
            DamageRequest::new(target, att, flags, DamageParam(param)),
        );
    }
    // a monster: an area attack item, else the single hit with the pre-rolled flags
    let prop = (act_item != 0 && act_item != NULL_ID)
        .then(|| core.data.item(act_item))
        .flatten();
    if let Some(p) = prop
        && p.spell_region == SRO_AROUND
    {
        let Some(pos) = cx.mover(target).map(|t| t.core.pos) else {
            return 0;
        };
        let spec = AroundSpec {
            center: Some(target),
            pos,
            flags: AtkFlags::GENERIC,
            attacker: att,
            apply: ApplyMask::PLAYER,
            attack_id: act_item,
            range: p.skill_range as f32,
            target_ratio: 1.0,
        };
        around(cx, &spec);
        return 0;
    }
    process_damage(
        &mut cx.arb,
        DamageRequest::new(target, att, AtkFlags(atk_flags), DamageParam(1)),
    )
}

/// An area attack: the candidates of the layer's grid, each hit through the arbiter.
fn around(cx: &mut ActionCx<'_>, spec: &AroundSpec) -> u32 {
    let hits = around_candidates(cx.grid, cx.arb.w.store, spec);
    damage_around_hits(&mut cx.arb, spec, &hits)
}

/// `CMover::OnAttackMagic`: the wand bolt leaves (Plan/17 §4.2): the class by the attacker's state (`WAND_ATK_AIR`
/// while flying, else the weapon's `sfx_obj2`, default `XI_FIR_WAND01`), `AF_MAGIC`, the charge as the power.
pub fn on_attack_magic(cx: &mut ActionCx<'_>, att: ObjId, target: ObjId) {
    let core = cx.arb.env.core;
    let (power, pid, flying, sfx, angle) = {
        let Some(a) = cx.mover(att) else { return };
        if cx.mover(target).is_none() {
            return;
        }
        let prop = active_hand_prop(a, core);
        (
            u32::try_from(a.combat.act.p[1])
                .unwrap_or(0)
                .min(MAX_CHARGE_LEVEL as u32),
            a.combat.act.projectile_id,
            a.sim.act.flags.fly(),
            prop.map_or(XI_FIR_WAND01, |p| p.sfx_obj[1]),
            a.sim.angle,
        )
    };
    let (sfx_id, kind) = if flying {
        (
            flyff_common::defs::obj::XI_FIR_WAND02,
            flyff_sim::projectile::ProjectileKind::WandAtkAir,
        )
    } else {
        let id = if sfx == NULL_ID { XI_FIR_WAND01 } else { sfx };
        match kind_for_sfx(id) {
            Some(k) => (id, k),
            None => return,
        }
    };
    spawn_shot(
        cx,
        ShotRequest {
            attacker: att,
            target,
            kind,
            sfx_id,
            hit_sfx_id: 0,
            projectile_id: (pid != 0).then_some(pid),
            payload: HitPayload::autoattack(power, AtkFlags::MAGIC, angle),
            shot_index: 0,
        },
    );
}

/// `CMover::OnAttackRange` (Plan/17 §3.8, §4.2): a player's arrow (class by the arrow's `sfx_obj2`, hit effect by
/// the bow's `sfx_obj3`), a monster's shot (class by its item's `sfx_obj2`; no class, no projectile, no damage:
/// KQ-35). Flags `AF_GENERIC`, plus `AF_RANGE` and the charge for a player.
pub fn on_attack_range(cx: &mut ActionCx<'_>, att: ObjId, target: ObjId) {
    let core = cx.arb.env.core;
    if cx.mover(target).is_none() {
        return;
    }
    let (is_player, item, pid, angle) = {
        let Some(a) = cx.mover(att) else { return };
        (
            a.is_player(),
            u32::try_from(a.combat.act.p[1]).unwrap_or(0),
            a.combat.act.projectile_id,
            a.sim.angle,
        )
    };
    if !is_player {
        // `CMonsterSkill::ApplySkill(this, target, ATK_RANGE)`: the result does not replace the shot
        let _ = cx.arb.hooks.monster_skill_apply(
            &mut cx.arb.w,
            &mut cx.arb.fx,
            att,
            target,
            super::hooks::MonsterSkillTrigger::Range,
        ) || crate::ai::monster_skill::apply(
            cx,
            att,
            target,
            super::hooks::MonsterSkillTrigger::Range,
        );
    }
    let (sfx_id, hit_sfx, kind, flags, power) = if is_player {
        let Some(a) = cx.mover(att) else { return };
        let bow = active_hand_prop(a, core);
        let arrow = a
            .player()
            .and_then(|p| p.items.inventory.get_equip(PARTS_BULLET))
            .and_then(|i| core.data.item(i.item_id));
        let Some(arrow) = arrow else { return };
        let sfx = arrow.sfx_obj[1];
        if sfx == NULL_ID {
            return;
        }
        let Some(kind) = kind_for_sfx(sfx) else {
            return;
        };
        (
            sfx,
            bow.map_or(0, |b| {
                if b.sfx_obj[2] == NULL_ID {
                    0
                } else {
                    b.sfx_obj[2]
                }
            }),
            kind,
            AtkFlags(AtkFlags::GENERIC.bits() | AtkFlags::RANGE.bits()),
            item.min(MAX_CHARGE_LEVEL as u32),
        )
    } else {
        let Some(a) = cx.mover(att) else { return };
        let prop = range_prop(a, core, item);
        let Some(prop) = prop else { return };
        let sfx = prop.sfx_obj[1];
        if sfx == NULL_ID {
            return;
        }
        let Some(kind) = kind_for_monster_range(sfx) else {
            return;
        };
        (sfx, 0, kind, AtkFlags::GENERIC, 0)
    };
    spawn_shot(
        cx,
        ShotRequest {
            attacker: att,
            target,
            kind,
            sfx_id,
            hit_sfx_id: hit_sfx,
            projectile_id: (pid != 0).then_some(pid),
            payload: HitPayload::autoattack(power, flags, angle),
            shot_index: 0,
        },
    );
}

/// `CMover::OnAttackSP` (`MoverActEvent.cpp:1915-1950`): a monster's special attack. An area item hits the players
/// around the monster itself (target ratio 0), otherwise the live target takes `item << 16`.
pub fn on_attack_sp(cx: &mut ActionCx<'_>, att: ObjId, target: ObjId) {
    let core = cx.arb.env.core;
    if cx.mover(target).is_none() {
        return;
    }
    let item = cx
        .mover(att)
        .map_or(0, |a| u32::try_from(a.combat.act.p[1]).unwrap_or(0));
    let prop = {
        let Some(a) = cx.mover(att) else { return };
        if item == NULL_ID || item == 0 {
            active_hand_prop(a, core)
        } else {
            core.data.item(item)
        }
    };
    let Some(prop) = prop else { return };
    if prop.spell_region == SRO_AROUND {
        let Some(pos) = cx.mover(att).map(|a| a.core.pos) else {
            return;
        };
        let spec = AroundSpec {
            center: Some(att),
            pos,
            flags: AtkFlags::GENERIC,
            attacker: att,
            apply: ApplyMask::PLAYER,
            attack_id: item,
            range: prop.skill_range as f32,
            target_ratio: 0.0,
        };
        around(cx, &spec);
    } else if cx.mover(target).is_some_and(is_live) {
        process_damage(
            &mut cx.arb,
            DamageRequest::new(
                target,
                att,
                AtkFlags::GENERIC,
                DamageParam::item_attack(item),
            ),
        );
    }
}
