//! The damage pipeline `CAttackArbiter` (Plan/17 §5; `src:WORLDSERVER/AttackArbiter.cpp`,
//! `src:_AIInterface/ActionMoverMsg.cpp:1190-1228`; WP-COMBAT-04).
//!
//! * [`process_damage`] is `SendDamage` / `SendDamageForce` on an [`ArbiterCx`]: [`pre_process_damage`], then
//!   [`on_damage_msg`] in the exact order of §5.3, then the `AIMSG_DAMAGE` of the victim;
//! * the handler-context wrappers [`send_damage`], [`send_damage_force`] and [`send_damage_around`] build the
//!   context from the services and the [`GameCtx`] and deliver the collected [`Effects`](super::effects::Effects);
//! * `calc_damage` -> `calc_atk` -> `post_calc_damage` use the pure formulas of `flyff_rules::combat` on
//!   [`Combatant`](flyff_rules::combat::Combatant) views; everything they would change is collected and applied right after the hand's
//!   calculation, in the order of the original (so a dual-wield second hand sees the first hand's effects).
//!
//! Random draws keep the order of the original: per hand the formulas draw critical, knock-up, damage, defense,
//! block and PvP-block rolls; `ProcessAbnormal` draws the reflect roll and one roll per chance skill; a hook
//! called between two draws sees the stream exactly where the original's callee would.

use super::effects::{Arena, SimFx};
use super::env::{ArbiterCx, DamageEnv};
use super::hit_type::{HitCheck, hit_type_ex};
use super::hooks::{ActiveSkill, CombatHooks, KillCtx};
use super::state::{is_chaotic, is_live, is_peaceful};
use super::view::{attack_info, combatant};
use crate::ai::AiMsg;
use crate::ctx::{GameCtx, Services};
use crate::mover::Mover;
use crate::stats::DstSource;
use crate::stats::env::{max_hp as stat_max_hp, point_param, sm_active};
use crate::stats::out::Out;
use crate::stats::pool::{set_hp_silent, set_point_param};
use crate::stats::source::apply_dst;
use crate::world::object::ObjectStore;
use flyff_common::ObjId;
use flyff_common::defs::obj::{MI_DEMIAN5, MI_KEAKOON5, MI_MUFFRIN5};
use flyff_common::defs::skill::*;
use flyff_common::defs::text::TID_GAME_PRIORITYMOB;
use flyff_common::rng::GameRng;
use flyff_proto::pb;
use flyff_rules::combat::{
    AtkFlags, AtkType, AttackInfo, CombatantView, DamageParam, Hand, HitType, apply_dpc,
    atk_multiplier, can_fly_by_attack, damage_multiplier, hit_power, link_attack_bonus,
    magic_hit_power, magic_skill_power, melee_skill_power, objmsg, post_asalraalaikum,
    post_calc_generic, post_calc_magic_skill, x_random,
};
use flyff_rules::consts::dst::*;
use flyff_rules::consts::{
    MATCHLESS_MODE, MATCHLESS2_MODE, NULL_CHGPARAM, ONEKILL_MODE, SM_MAINTAIN_MP,
};
use flyff_rules::stats::c_int;
use flyff_sim::types::MVRF_CRITICAL;

/// One hit to deliver (`SendDamage(dwAtkFlags, idAttacker, nParam, bTarget, nReflect)`, Plan/17 §5.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DamageRequest {
    /// The victim (`this` of `CActionMover::SendDamage`).
    pub victim: ObjId,
    /// The attacker.
    pub attacker: ObjId,
    /// `dwAtkFlags`.
    pub flags: AtkFlags,
    /// `nParam` (see [`DamageParam`]).
    pub param: DamageParam,
    /// `bTarget`: the victim was the primary target (area side-targets pass `false`).
    pub is_target: bool,
    /// `nReflect`: 0 normal, 2 reflected damage (no PK flagging, no further reflection).
    pub reflect: i32,
    /// `OBJMSG_DAMAGE_FORCE`: only changes the death motion (the fly-back death).
    pub force: bool,
}

impl DamageRequest {
    /// A normal hit on the primary target.
    pub fn new(victim: ObjId, attacker: ObjId, flags: AtkFlags, param: DamageParam) -> Self {
        DamageRequest {
            victim,
            attacker,
            flags,
            param,
            is_target: true,
            reflect: 0,
            force: false,
        }
    }

    /// The same hit as `OBJMSG_DAMAGE_FORCE` (`SendDamageForce`).
    pub fn forced(mut self) -> Self {
        self.force = true;
        self
    }

    /// The same hit on a secondary target of an area attack.
    pub fn side_target(mut self) -> Self {
        self.is_target = false;
        self
    }
}

// ---- small readers over the store ------------------------------------------------------------------------------

/// `IsValidObj`: the mover exists and is not flagged for deletion.
fn valid<'s>(cx: &'s ArbiterCx<'_>, id: ObjId) -> Option<&'s Mover> {
    cx.w.store.mover(id).filter(|m| !m.core.deleted)
}

/// `GetHitPoint()` of mover `id`.
fn hit_point(cx: &ArbiterCx<'_>, id: ObjId) -> i32 {
    cx.w.store
        .mover(id)
        .map_or(0, |m| point_param(m, cx.env.core, DST_HP))
}

/// `IsReturnToBegin()`: a monster walking back to its spawn point (doc 18).
fn is_return_to_begin(m: &Mover) -> bool {
    m.monster().is_some_and(|e| {
        e.ai.monster().is_some_and(|b| b.returning)
            // `CAIMeteonyker::IsReturnToBegin` is virtual (`aimeteonyker.h:76`); the bosses keep the default `FALSE`
            || e.ai.meteonyker().is_some_and(|b| b.base.returning)
    })
}

// ---- entry points -------------------------------------------------------------------------------------------------

/// `CActionMover::SendDamage` / `SendDamageForce` (`OBJMSG_DAMAGE[_FORCE]`, `ActionMoverMsg.cpp:816-836`): the
/// damage the hit really did (0 when it was ignored, missed or transformed the victim).
pub fn process_damage(cx: &mut ArbiterCx<'_>, mut req: DamageRequest) -> i32 {
    if !pre_process_damage(cx, &req) {
        return 0;
    }
    if req.force
        && cx
            .w
            .store
            .mover(req.victim)
            .is_none_or(|v| !can_fly_by_attack(&combatant(v, cx.env.core, Hand::Right, None)))
    {
        req.force = false;
    }
    let dmg = on_damage_msg(cx, &req);
    // AIMSG_DAMAGE to the victim, only for a player attacker the summoned-monster rule allows
    if let (Some(att), Some(def)) = (cx.w.store.mover(req.attacker), cx.w.store.mover(req.victim))
        && att.is_player()
        && cx.hooks.create_monster_attackable(att, def)
    {
        cx.fx.mail.push((
            req.victim,
            AiMsg::Damage {
                attacker: req.attacker,
                damage: dmg,
            },
        ));
    }
    dmg
}

/// `CActionMover::PreProcessDamage` (`ActionMoverMsg.cpp:1190-1228`): `false` ignores the hit.
///
/// The checks, in order:
///
/// - a victim in `MATCHLESS_MODE` (GM invincible);
/// - a petrified victim (`CHS_SETSTONE`);
/// - the attacker must be a valid mover;
/// - an NPC victim must be killable;
/// - `hit_type == Fail`;
/// - the attacker loses its invisibility.
pub fn pre_process_damage(cx: &mut ArbiterCx<'_>, req: &DamageRequest) -> bool {
    let env = cx.env;
    let Some(def) = valid(cx, req.victim) else {
        return false;
    };
    if def.mv.mode & MATCHLESS_MODE != 0 {
        return false;
    }
    if def.mv.dst.table.adj(DST_CHRSTATE) as u32 & CHS_SETSTONE != 0 {
        return false;
    }
    let Some(att) = valid(cx, req.attacker) else {
        return false;
    };
    if !def.is_player()
        && env
            .core
            .data
            .mover(def.prop_id())
            .is_some_and(|p| p.killable != 1)
    {
        return false;
    }
    let HitCheck {
        ty,
        clear_attacker_target,
    } = hit_type_ex(att, def, env, &*cx.hooks, req.is_target, req.reflect);
    if clear_attacker_target {
        cx.fx
            .to_self(req.attacker, pb::SetTargetNtf { target: u32::MAX });
    }
    if ty == HitType::Fail {
        return false;
    }
    cx.hooks
        .remove_invisible(&mut cx.w, &mut cx.fx, req.attacker);
    true
}

// ---- OnDamageMsgW -------------------------------------------------------------------------------------------------

/// What `CalcDamage` wants done after the formulas ran (collected so the formulas can borrow the movers; applied
/// by [`apply_post`] in the order of the original).
#[derive(Clone, Copy, Debug, Default)]
struct Post {
    /// The one-shot party critical bonus was spent (`MVRF_CRITICAL` cleared).
    clear_party_crit: bool,
    /// Vanilla Asalraalaikum: the attacker's MP drops to 0.
    drain_mp: bool,
    /// `ResetDestParam(DST_IGNORE_DMG, n)`: the shield shrinks by `n`.
    shield_reduce: Option<i32>,
    /// The shield broke: `RemoveBuff(SI_RIG_MASTER_BLESSING)`.
    remove_blessing: bool,
    /// Holy Cross kill in the guild-war world: a point for the caster.
    holycross_point: bool,
    /// Vanilla: Holy Cross is consumed by the hit.
    remove_double: bool,
    /// A counter attack: `(remove_bits, level)` -> remove the state and cast `SI_GEN_ATK_COUNTER`.
    counter: Option<(u32, u32)>,
    /// Vanilla Hit of Penya: the gold the attacker pays.
    hop_pay: i32,
}

/// `CAttackArbiter::OnDamageMsgW` (A:134-308), in the exact order of Plan/17 §5.3. Returns the dealt damage.
pub fn on_damage_msg(cx: &mut ArbiterCx<'_>, req: &DamageRequest) -> i32 {
    let env = cx.env;
    let (a_id, d_id) = (req.attacker, req.victim);
    let mut flags = req.flags;

    // 1. CheckValidDamageMsg
    if !check_valid_damage_msg(cx, req, &mut flags) {
        return 0;
    }
    let (Some(att), Some(def)) = (valid(cx, a_id), valid(cx, d_id)) else {
        return 0;
    };
    let (att_is_player, def_is_player) = (att.is_player(), def.is_player());
    let att_onekill = att.mv.mode & ONEKILL_MODE != 0;
    let def_prop_id = def.prop_id();
    let mut info = attack_info(att, env.core, flags, req.param);

    // 2. per hand
    let mut damage = 0_i32;
    let mut reflect_dmg = 0_i32;
    let hands: Vec<Hand> = req.param.hands(flags).collect();
    for hand in hands {
        info.hand = hand;
        if !def_is_player && cx.hooks.is_event_spawn_monster(def_prop_id) && !att_onekill {
            damage = 1;
            continue;
        }
        let (n, post) = calc_damage(cx, a_id, d_id, &mut info);
        flags = info.flags;
        apply_post(cx, a_id, d_id, post);
        if n > 0 {
            if let Some(r) = process_abnormal(cx, a_id, d_id, n, &info) {
                reflect_dmg = r;
            }
            steal_hp(cx, a_id, n, info.atk_type());
            damage = damage.wrapping_add(n);
        }
    }

    // 3. PvP reduction
    if att_is_player && def_is_player {
        let (rate_dst, pk_value, gc_target) = match (cx.w.store.mover(a_id), cx.w.store.mover(d_id))
        {
            (Some(a), Some(d)) => (
                d.mv.dst.table.param(DST_PVP_DMG_RATE, 0),
                a.player().map_or(0, |p| p.pk.value),
                cx.hooks.is_guild_combat_target(a, d),
            ),
            _ => (0, 0, false),
        };
        damage = damage.wrapping_sub(damage.wrapping_mul(rate_dst) / 100);
        if !gc_target {
            let rate = pk_value.wrapping_mul(5).min(99);
            damage = damage.wrapping_sub((damage as f32 * (rate as f32 / 100.0_f32)) as i32);
        }
    }

    // 4. every non-missed hit deals at least 1
    damage = damage.max(1);

    // 5. MinusHP
    let hp = hit_point(cx, d_id);
    let mut new_hp = hp.wrapping_sub(damage);
    if new_hp <= 0 {
        let matchless2 =
            cx.w.store
                .mover(d_id)
                .is_some_and(|m| m.mv.mode & MATCHLESS2_MODE != 0);
        new_hp = i32::from(matchless2);
        damage = hp.wrapping_sub(new_hp);
    }

    // 6. monster transform
    if cx
        .hooks
        .monster_transform(&mut cx.w, &mut cx.fx, d_id, new_hp)
        || crate::ai::monster_skill::transform_check(cx, d_id, new_hp)
    {
        return 0;
    }

    // 7. a sitting victim stands up
    if cx
        .w
        .store
        .mover(d_id)
        .is_some_and(|m| m.sim.act.flags.sit())
    {
        cx.fx.sim.push((d_id, SimFx::ResetMove));
    }
    cx.fx.sim.push((d_id, SimFx::Stand));

    // 8. OnAttacked
    on_attacked(cx, a_id, d_id, damage, req.is_target, req.reflect);

    // 9. the DPS bookkeeping
    cx.hooks
        .on_hit_recorded(&mut cx.w, &mut cx.fx, a_id, d_id, damage);

    // 10. survive or die
    if new_hp > 0 {
        on_damaged(cx, a_id, d_id, damage, flags, new_hp);
        new_hp = auto_hp(cx, d_id, new_hp);
        if reflect_dmg != 0 {
            process_damage(
                cx,
                DamageRequest {
                    victim: a_id,
                    attacker: d_id,
                    flags: AtkFlags::FORCE,
                    param: DamageParam::force(reflect_dmg),
                    is_target: true,
                    reflect: 2,
                    force: false,
                },
            );
        }
    } else {
        let kill = KillCtx {
            victim: d_id,
            attacker: a_id,
            msg: if req.force {
                objmsg::DAMAGE_FORCE
            } else {
                objmsg::DAMAGE
            },
            reflect: req.reflect,
        };
        // the real `OnDied` needs the services: the death waits in the queue of the death pipeline
        cx.fx.deaths.push(kill);
        cx.hooks.on_died(&mut cx.w, &mut cx.fx, env, &kill);
    }

    // 11. HP commit (EndPVP): the stored value, without a message (the Damage carries hp_after, PL-083)
    let core = env.core;
    if let Some(m) = cx.w.store.mover_mut(d_id) {
        set_hp_silent(m, core, new_hp);
    }
    damage
}

/// `CAttackArbiter::CheckValidDamageMsg` (A:310-348): `false` stops the call. A miss broadcasts a zero `Damage`
/// first. `flags` is the arbiter's `m_dwAtkFlags` (a refusal replaces it by `AF_MISS`).
fn check_valid_damage_msg(
    cx: &mut ArbiterCx<'_>,
    req: &DamageRequest,
    flags: &mut AtkFlags,
) -> bool {
    let env = cx.env;
    let (a_id, d_id) = (req.attacker, req.victim);
    let (Some(att), Some(def)) = (valid(cx, a_id), valid(cx, d_id)) else {
        return false;
    };
    // a. an event monster far below the attacker's level (KQ-32)
    if att.is_player()
        && !def.is_player()
        && let Some(gap) = cx
            .hooks
            .event_monster_level_gap(&env.core.data, def.prop_id())
        && att.mv.level.wrapping_sub(def.mv.level) >= gap
    {
        *flags = AtkFlags::MISS;
    }
    // b. summoned-monster ownership
    let mut lost_claim = false;
    let mut summon_text = None;
    if att.is_player() && !cx.hooks.create_monster_attackable(att, def) {
        *flags = AtkFlags::MISS;
        lost_claim = true;
        // `IsAttackAble(.., bTextOut = TRUE)`: the attacker is told whose monster it is
        summon_text = crate::ai::create_monster::priority_text(def);
    }
    if let Some(text) = summon_text {
        cx.fx.to_self(a_id, text);
    }
    if lost_claim && let Some(d) = cx.w.store.mover_mut(d_id) {
        d.combat.id_attacker = None;
        d.combat.id_targeter = None;
    }
    // c. a miss is broadcast with zero damage and stops here
    if flags.intersects(AtkFlags::MISS) {
        let hp = hit_point(cx, d_id);
        broadcast_damage(cx, a_id, d_id, 0, *flags, hp);
        return false;
    }
    // d. kill-steal protection
    if env.steal_protection && is_steal(cx, a_id, d_id) {
        return false;
    }
    true
}

/// `CMover::IsSteal(npc)` without the message (this = `a_id`, `Mover.cpp:7118-7165`, Plan/17 §8.5): a player
/// hitting an NPC that another player has claimed (`m_idTargeter`). Exempt: NPC attackers, the event monsters
/// `MI_DEMIAN5`, `MI_KEAKOON5`, `MI_MUFFRIN5`, `RANK_SUPER` bosses, a claim by an invalid or the same player,
/// the attacker's party members and (in a guild war) its guild mates. Whether protection is on at all
/// (`!events.steal`) is the caller's question.
pub fn steal_check(
    store: &ObjectStore,
    data: &flyff_data::GameData,
    a_id: ObjId,
    d_id: ObjId,
) -> bool {
    let valid = |id: ObjId| store.mover(id).filter(|m| !m.core.deleted);
    let (Some(att), Some(def)) = (valid(a_id), valid(d_id)) else {
        return false;
    };
    if !att.is_player() || def.is_player() {
        return false;
    }
    if matches!(def.prop_id(), MI_DEMIAN5 | MI_KEAKOON5 | MI_MUFFRIN5) {
        return false;
    }
    let Some(claimer) = def.combat.id_targeter else {
        return false;
    };
    let Some(other) = valid(claimer) else {
        return false;
    };
    let (me, them) = (att.player(), other.player());
    let mut check = true;
    if let (Some(me), Some(them)) = (me, them) {
        if me.social.party.is_some() && me.social.party == them.social.party {
            check = false;
        }
        if me.social.war != 0 && me.social.guild.is_some() && me.social.guild == them.social.guild {
            check = false;
        }
    }
    if data
        .mover(def.prop_id())
        .is_some_and(|p| p.class == RANK_SUPER)
    {
        check = false;
    }
    check && claimer != a_id
}

/// `CMover::IsSteal(npc)` (this = `attacker`): [`steal_check`] plus the side effect of the original: a steal
/// tells the attacker `TID_GAME_PRIORITYMOB`.
pub fn is_steal(cx: &mut ArbiterCx<'_>, a_id: ObjId, d_id: ObjId) -> bool {
    let steal = steal_check(cx.w.store, &cx.env.core.data, a_id, d_id);
    if steal {
        cx.fx.to_self(
            a_id,
            crate::chat::ingress::notice::defined_text(TID_GAME_PRIORITYMOB, Vec::new()),
        );
    }
    steal
}

// ---- CalcDamage -> CalcATK -> PostCalcDamage --------------------------------------------------------------------

/// `CalcDamage(info)` (A:398-407): `(damage, effects to apply)`. An attacker in `ONEKILL_MODE` deals the
/// victim's HP.
fn calc_damage(
    cx: &mut ArbiterCx<'_>,
    a_id: ObjId,
    d_id: ObjId,
    info: &mut AttackInfo<'_>,
) -> (i32, Post) {
    let env = cx.env;
    let core = env.core;
    let ccx = env.cx();
    let ArbiterCx { w, hooks, .. } = cx;
    let (Some(att), Some(def)) = (w.store.mover(a_id), w.store.mover(d_id)) else {
        return (0, Post::default());
    };
    if att.mv.mode & ONEKILL_MODE != 0 {
        return (point_param(def, core, DST_HP), Post::default());
    }
    let party_crit = if att.sim.mvrf.has(MVRF_CRITICAL)
        && att.player().is_some_and(|p| p.social.party.is_some())
    {
        hooks.party_critical(att)
    } else {
        None
    };
    // doc 21 (WP-COMBAT-14): the Link Attack facts of the attacker's party, read before the damage is computed
    let link_state = hooks.link_attack_state_in(&*w.store, att);
    let a = combatant(att, core, info.hand, party_crit);
    let d = combatant(def, core, Hand::Right, None);
    let rng: &mut dyn GameRng = &mut *w.rng;
    let mut post = Post::default();

    let atk = calc_atk(env, &a, &d, info, rng);
    let dmg = post_calc_damage(
        env,
        &ccx,
        att,
        def,
        &a,
        &d,
        &**hooks,
        &link_state,
        atk,
        info,
        rng,
        &mut post,
    );
    post.clear_party_crit = info.party_crit_used;
    (dmg, post)
}

/// `CalcATK(info)` (A:350-394): the base ATK of the hit by its type, times the attacker's ATK multiplier (also for
/// `AF_FORCE`, KQ-05), x0.1 for a repeated tick, plus `DST_ATKPOWER`, plus the Event.lua ATK for players, at
/// least 0.
fn calc_atk(
    env: &DamageEnv<'_>,
    a: &dyn CombatantView,
    d: &dyn CombatantView,
    info: &mut AttackInfo<'_>,
    rng: &mut dyn GameRng,
) -> i32 {
    let ccx = env.cx();
    let mut count = info.param.attack_count();
    let mut atk = match info.atk_type() {
        AtkType::Force => {
            count = 0;
            info.param.as_i32()
        }
        AtkType::MeleeSkill => melee_skill_power(a, d, info, rng),
        AtkType::MagicSkill => magic_skill_power(a, d, info, &env.cfg, rng),
        AtkType::Magic => magic_hit_power(a, info.charge_level(), &ccx, rng),
        AtkType::Generic => hit_power(a, d, info, &ccx, rng),
    };
    atk = (atk as f32 * atk_multiplier(a, d, &ccx)) as i32;
    if count > 0 {
        atk = (atk as f32 * 0.1_f32) as i32;
    }
    atk = atk.wrapping_add(a.param(DST_ATKPOWER, 0));
    if a.is_player() {
        atk = atk.wrapping_add(ccx.event_attack_power);
    }
    atk.max(0)
}

/// `PostCalcDamage(atk, info)` (A:522-586): defense, block, critical, Link Attack, Asalraalaikum, the damage
/// multiplier, the Blessing shield and `OnAfterDamage`.
#[allow(clippy::too_many_arguments)]
fn post_calc_damage(
    env: &DamageEnv<'_>,
    ccx: &flyff_rules::combat::CombatCx<'_>,
    att: &Mover,
    def: &Mover,
    a: &dyn CombatantView,
    d: &dyn CombatantView,
    hooks: &dyn CombatHooks,
    link_state: &flyff_rules::combat::LinkAttackState,
    mut atk: i32,
    info: &mut AttackInfo<'_>,
    rng: &mut dyn GameRng,
    post: &mut Post,
) -> i32 {
    if info.flags.intersects(AtkFlags::FORCE) {
        return atk;
    }
    // a monster hitting a player with anything but a magic skill: +5 % per level above
    if !info.flags.intersects(AtkFlags::MAGICSKILL) && !a.is_player() && d.is_player() {
        let delta = a.level().wrapping_sub(d.level());
        if delta > 0 {
            atk = (atk as f32 * (1.0_f32 + 0.05_f32 * delta as f32)) as i32;
        }
    }
    let mut dmg = if info.flags.intersects(AtkFlags::MAGICSKILL) {
        post_calc_magic_skill(a, d, atk, info, ccx)
    } else if info.flags.intersects(AtkFlags::GENERIC) {
        post_calc_generic(a, d, atk, info, ccx, rng)
    } else {
        apply_dpc(d, a, atk, info, ccx, rng)
    };
    if dmg <= 0 {
        return 0;
    }
    dmg = dmg.wrapping_add(link_attack_bonus(dmg, link_state, &env.cfg));
    if !env.cfg.no_asal_mp && info.skill_id() == SI_BIL_PST_ASALRAALAIKUM {
        let mp = point_param(att, env.core, DST_MP);
        // a monster casts it at the level of its `MonsterSkill.lua` entry (0 without one), a player at its own
        let level = if att.is_player() {
            info.skill_level()
        } else {
            crate::skill::monster::monster_skill_level(
                &env.core.data,
                att.prop_id(),
                SI_BIL_PST_ASALRAALAIKUM,
            )
        };
        dmg = dmg.wrapping_add(post_asalraalaikum(a.str_(), level, mp));
        post.drain_mp = att.is_player() && !sm_active(att, SM_MAINTAIN_MP);
    }
    dmg = (dmg as f32 * damage_multiplier(a, d, info, ccx, rng)) as i32;
    if env.cfg.new_skill_attribute {
        let imm = d.param(DST_IGNORE_DMG, 0);
        if imm > 0 {
            if imm.wrapping_sub(dmg) <= 0 {
                dmg = dmg.wrapping_sub(imm);
                post.remove_blessing = true;
            } else {
                post.shield_reduce = Some(dmg);
                dmg = 0;
            }
        }
    }
    on_after_damage(env, att, def, a, d, hooks, info, dmg, post)
}

/// `OnAfterDamage(info, dmg)` (A:409-501), the decisions; the effects go into `post`.
#[allow(clippy::too_many_arguments)]
fn on_after_damage(
    env: &DamageEnv<'_>,
    att: &Mover,
    def: &Mover,
    a: &dyn CombatantView,
    d: &dyn CombatantView,
    hooks: &dyn CombatHooks,
    info: &mut AttackInfo<'_>,
    mut dmg: i32,
    post: &mut Post,
) -> i32 {
    let state = d.chr_state();
    // 1. Holy Cross
    if a.is_player() && dmg > 0 && state & CHS_DOUBLE != 0 {
        if env.cfg.holycross_support_gw
            && d.hp().wrapping_sub(dmg) <= 0
            && a.in_guild_war_world()
            && d.in_guild_war_world()
        {
            post.holycross_point = true;
        }
        if !env.cfg.better_holycross {
            post.remove_double = true;
        }
    }
    // 2. counter attacks within 3 m of the victim
    let near = || {
        let radius = flyff_sim::bounds::radius(&def.sim.params.bounds, def.sim.params.scale);
        flyff_sim::bounds::is_range_point(def.sim.pos, radius, att.sim.pos, 3.0)
    };
    if state & CHS_ATK_COUNTERATTACK != 0 {
        if near() {
            // a player who knows the skill casts its own level (`GetSkillLevel`, may be 0), anybody else level 1
            let level = if def.is_player() {
                match hooks.skill_level(def, SI_ACR_YOYO_COUNTER) {
                    0 => crate::skill::use_skill::known_level_or(
                        def,
                        env.core,
                        SI_ACR_YOYO_COUNTER,
                        1,
                    ),
                    n => n,
                }
            } else {
                1
            };
            post.counter = Some((CHS_ATK_COUNTERATTACK, level));
            dmg = 0;
            info.flags = AtkFlags::MISS;
        }
    } else if state & CHS_DMG_COUNTERATTACK != 0 && near() {
        post.counter = Some((CHS_DMG_COUNTERATTACK, info.skill_level()));
    }
    // 3. Hit of Penya costs gold without `features.no_hop_cost`
    if !env.cfg.no_hop_cost && info.skill_id() == SI_JST_YOYO_HITOFPENYA && dmg > 0 {
        dmg = hooks.gold(att).min(dmg);
        post.hop_pay = dmg;
    }
    // 4. a monster walking home takes nothing
    if is_return_to_begin(def) {
        dmg = 0;
        info.flags = AtkFlags::MISS;
    }
    dmg
}

/// Applies what a hand's calculation collected, in the order of the original: the spent party bonus, the
/// Asalraalaikum MP drain, the Blessing shield, Holy Cross, the counter attack, the Hit of Penya payment.
fn apply_post(cx: &mut ArbiterCx<'_>, a_id: ObjId, d_id: ObjId, post: Post) {
    let core = cx.env.core;
    if post.clear_party_crit
        && let Some(m) = cx.w.store.mover_mut(a_id)
    {
        m.sim.mvrf.remove(MVRF_CRITICAL);
    }
    if post.drain_mp
        && let Some(m) = cx.w.store.mover_mut(a_id)
    {
        let mut out = Out::new();
        set_point_param(m, core, DST_MP, 0, false, &mut out);
        cx.fx.extend_out(a_id, out);
    }
    if let Some(n) = post.shield_reduce
        && let Some(m) = cx.w.store.mover_mut(d_id)
    {
        m.mv.dst.table.reset_primitive(DST_IGNORE_DMG, n);
        cx.fx.near(
            d_id,
            pb::ResetDestParam {
                dst: DST_IGNORE_DMG as i32,
                adj: n,
            },
        );
    }
    if post.remove_blessing {
        cx.hooks
            .remove_skill_buff(&mut cx.w, &mut cx.fx, d_id, SI_RIG_MASTER_BLESSING);
    }
    if post.holycross_point {
        cx.hooks
            .holycross_gc_point(&mut cx.w, &mut cx.fx, d_id, a_id);
    }
    if post.remove_double {
        cx.hooks
            .remove_chr_state_buffs(&mut cx.w, &mut cx.fx, d_id, CHS_DOUBLE);
    }
    if let Some((bits, level)) = post.counter {
        cx.hooks
            .remove_chr_state_buffs(&mut cx.w, &mut cx.fx, d_id, bits);
        cx.hooks
            .do_use_skill(&mut cx.w, &mut cx.fx, d_id, SI_GEN_ATK_COUNTER, level, a_id);
    }
    if post.hop_pay != 0 {
        cx.hooks
            .add_gold(&mut cx.w, &mut cx.fx, a_id, post.hop_pay.wrapping_neg());
    }
}

// ---- ProcessAbnormal and StealHP ---------------------------------------------------------------------------------

/// `CAttackArbiter::ChanceSkill(dst)` (A:~640): `rate = attacker.adj(dst)`; when positive, `x_random(100) <= rate`
/// triggers `DoActiveSkill` of the `AddSkillProp` row whose id the attacker holds in `chg(dst)`.
fn chance_skill(cx: &mut ArbiterCx<'_>, a_id: ObjId, d_id: ObjId, dst: u32) {
    let env = cx.env;
    let Some(att) = valid(cx, a_id) else { return };
    let rate = att.mv.dst.table.adj(dst);
    let chg = att.mv.dst.table.chg(dst);
    if rate > 0 && (x_random(&mut *cx.w.rng, 100) as i32) <= rate {
        let row = u32::try_from(chg)
            .ok()
            .and_then(|id| env.core.data.add_skill_by_id(id));
        if let Some(add) = row {
            cx.hooks.do_active_skill(
                &mut cx.w,
                &mut cx.fx,
                ActiveSkill {
                    caster: a_id,
                    target: d_id,
                    skill: add.skill_id,
                    level: add.skill_lvl,
                    ignore_prob: false,
                },
            );
        }
    }
}

/// `CAttackArbiter::ProcessAbnormal(dmg, info)` (A:657-768): the reflect decision, the chance skills and the
/// active skills of the weapon and the skill. Returns the damage to reflect (the original stores
/// `m_nReflectDmg`; it is sent after the HP update, only if the victim survives).
///
/// * Reflect: `rate = victim.adj(REFLECT_DAMAGE)`; when positive and `chg(REFLECT_DAMAGE) > 0` (an **unset**
///   `chg` is `0x7FFFFFFF`, which is `> 0`: the roll is drawn and always passes) the roll `x_random(100) > chg`
///   cancels it; a skill hit is not reflected by a victim with `SI_PSY_NLG_CRUCIOSPELL` and `AF_FORCE` never;
///   `(int)(dmg * (rate / 100.0f))`, x0.1 between players.
/// * `AF_MAGICSKILL` by a player with `SI_BIL_PST_BGVURTIALBOLD` / `ASALRAALAIKUM`: `ChanceSkill(STUN)`.
/// * `AF_GENERIC | AF_MELEESKILL`: a player attacker rolls `ChanceSkill` for stun, poison, dark, bleeding and
///   steal-HP in that order; the weapon's active skill (`rate == NULL_ID || x_random(100) < rate`) follows.
/// * `AF_MELEESKILL | AF_MAGICSKILL`: the level row's active skill (`rate == NULL_ID || x_random(100) < rate`).
fn process_abnormal(
    cx: &mut ArbiterCx<'_>,
    a_id: ObjId,
    d_id: ObjId,
    dmg: i32,
    info: &AttackInfo<'_>,
) -> Option<i32> {
    let env = cx.env;
    let skill = info.skill_id();
    let flags = info.flags;
    let atk_type = info.atk_type();
    let mut reflect = None;

    // reflect
    let (rate, chg, crucio, att_is_player, both_players) = {
        let (Some(att), Some(def)) = (valid(cx, a_id), valid(cx, d_id)) else {
            return None;
        };
        (
            def.mv.dst.table.adj(DST_REFLECT_DAMAGE),
            def.mv.dst.table.chg(DST_REFLECT_DAMAGE),
            def.buffs
                .has(flyff_rules::combat::BUFF_SKILL, SI_PSY_NLG_CRUCIOSPELL),
            att.is_player(),
            att.is_player() && def.is_player(),
        )
    };
    if rate > 0 {
        let mut able = true;
        if chg > 0 && (x_random(&mut *cx.w.rng, 100) as i32) > chg {
            able = false;
        }
        match atk_type {
            AtkType::MeleeSkill | AtkType::MagicSkill => {
                if crucio {
                    able = false;
                }
            }
            AtkType::Force => able = false,
            _ => {}
        }
        if able {
            let mut n = (dmg as f32 * (rate as f32 / 100.0_f32)) as i32;
            if both_players {
                n = (n as f32 * 0.1_f32) as i32;
            }
            reflect = Some(n);
        }
    }

    // chance skills of a magic skill
    if flags.intersects(AtkFlags::MAGICSKILL)
        && att_is_player
        && matches!(skill, SI_BIL_PST_BGVURTIALBOLD | SI_BIL_PST_ASALRAALAIKUM)
    {
        chance_skill(cx, a_id, d_id, DST_CHR_CHANCESTUN);
    }

    // autoattacks and melee skills
    if flags.intersects(AtkFlags::GENERIC | AtkFlags::MELEESKILL) {
        if att_is_player {
            for dst in [
                DST_CHR_CHANCESTUN,
                DST_CHR_CHANCEPOISON,
                DST_CHR_CHANCEDARK,
                DST_CHR_CHANCEBLEEDING,
                DST_CHR_CHANCESTEALHP,
            ] {
                chance_skill(cx, a_id, d_id, dst);
            }
        }
        // the weapon's active skill
        let weapon = valid(cx, a_id)
            .and_then(|m| super::view::active_hand_prop(m, env.core))
            .map(|p| (p.active_skill, p.active_skill_lv, p.active_skill_rate));
        if let Some((skill_id, lvl, rate)) = weapon {
            let lvl = if lvl == flyff_rules::consts::NULL_ID {
                1
            } else {
                lvl
            };
            if skill_id != flyff_rules::consts::NULL_ID
                && (rate == flyff_rules::consts::NULL_ID || x_random(&mut *cx.w.rng, 100) < rate)
            {
                cx.hooks.do_active_skill(
                    &mut cx.w,
                    &mut cx.fx,
                    ActiveSkill {
                        caster: a_id,
                        target: d_id,
                        skill: skill_id,
                        level: lvl,
                        ignore_prob: true,
                    },
                );
            }
        }
    }

    // the active skill of the skill's level row
    if flags.intersects(AtkFlags::MELEESKILL | AtkFlags::MAGICSKILL)
        && let Some(add) = info.skill.add
        && add.active_skill != flyff_rules::consts::NULL_ID
    {
        let rate = if both_players {
            add.active_skill_rate_pvp
        } else {
            add.active_skill_rate
        };
        if rate == flyff_rules::consts::NULL_ID || x_random(&mut *cx.w.rng, 100) < rate {
            cx.hooks.do_active_skill(
                &mut cx.w,
                &mut cx.fx,
                ActiveSkill {
                    caster: a_id,
                    target: d_id,
                    skill: add.active_skill,
                    level: add.skill_lvl,
                    ignore_prob: false,
                },
            );
        }
    }
    reflect
}

/// `CAttackArbiter::StealHP(dmg, type)` (A:807-835): a weapon with `DST_CHR_STEALHP` in its first slot gives the
/// attacker `(int)(dmg * (adj / 100.0f))` HP; a player with `DST_MELEE_STEALHP` > 0 gets the same share of an
/// autoattack (and, with `features.magic_steal_hp`, of a magic skill).
fn steal_hp(cx: &mut ArbiterCx<'_>, a_id: ObjId, dmg: i32, atk_type: AtkType) {
    let env = cx.env;
    let core = env.core;
    let Some(att) = valid(cx, a_id) else { return };
    let weapon_steal = super::view::active_hand_prop(att, core)
        .filter(|p| p.dest_param[0] == DST_CHR_STEALHP)
        .map(|p| (dmg as f32 * (p.adj_param_val[0] as f32 / 100.0_f32)) as i32);
    let dst_hp = att.mv.dst.table.param(DST_MELEE_STEALHP, 0);
    let melee_steal = (att.is_player()
        && (atk_type == AtkType::Generic
            || (env.cfg.magic_steal_hp && atk_type == AtkType::MagicSkill)))
        .then(|| (dmg as f32 * (dst_hp as f32 / 100.0_f32)) as i32);
    let melee_steal = melee_steal.filter(|_| 0 < dst_hp);
    for n in [weapon_steal, melee_steal].into_iter().flatten() {
        // SetDestParam(DST_HP, n, NULL_CHGPARAM): a pool effect on the attacker
        if let Some(m) = cx.w.store.mover_mut(a_id) {
            let mut out = Out::new();
            apply_dst(
                m,
                core,
                DstSource::Equip { part: 0 },
                (DST_HP, n, NULL_CHGPARAM),
                true,
                &mut out,
            );
            cx.fx.extend_out(a_id, out);
        }
    }
}

// ---- OnAttacked ------------------------------------------------------------------------------------------------

/// `CMover::OnAttacked(attacker, dmg, is_target, reflect)` (M:1864-1943, Plan/17 §5.16) for `d_id` being hit by
/// `a_id`: the hate list (unless a duel, war, school or arena hit), the PK / PvP hooks, the write-only exp ratio
/// of an NPC and the attacker's last hit.
pub(crate) fn on_attacked(
    cx: &mut ArbiterCx<'_>,
    a_id: ObjId,
    d_id: ObjId,
    dmg: i32,
    is_target: bool,
    reflect: i32,
) {
    if a_id == d_id {
        return;
    }
    let env = cx.env;
    let now = cx.w.now;
    let ty = match (valid(cx, a_id), valid(cx, d_id)) {
        (Some(a), Some(d)) => hit_type_ex(a, d, env, &*cx.hooks, is_target, reflect).ty,
        _ => return,
    };
    if !matches!(
        ty,
        HitType::Pvp | HitType::War | HitType::School | HitType::Arena
    ) {
        if let Some(d) = cx.w.store.mover_mut(d_id) {
            d.combat.hate.add(a_id, dmg, now);
        }
        if let Some(a) = cx.w.store.mover_mut(a_id) {
            a.combat.hate.add(d_id, 0, now);
        }
    }
    match ty {
        HitType::Pk => {
            let (a_chaotic, d_chaotic) = (
                cx.w.store.mover(a_id).is_some_and(is_chaotic),
                cx.w.store.mover(d_id).is_some_and(is_chaotic),
            );
            if reflect == 0 && !a_chaotic && !d_chaotic {
                super::pvp::pk::on_pk_hit(cx, a_id, d_id);
                cx.hooks.on_pk_hit(&mut cx.w, &mut cx.fx, a_id, d_id);
            }
        }
        HitType::Pvp => {
            super::duel::reset_duel_time(cx.w.store, now, a_id, d_id);
            cx.hooks.on_pvp_hit(&mut cx.w, a_id, d_id);
        }
        _ => {}
    }
    // a player hitting an NPC: the (write-only) exp ratio and the remembered attacker
    let (att_is_player, def_is_player) = (
        cx.w.store.mover(a_id).is_some_and(Mover::is_player),
        cx.w.store.mover(d_id).is_some_and(Mover::is_player),
    );
    if att_is_player && !def_is_player {
        let previous = cx.w.store.mover(d_id).and_then(|d| d.combat.id_attacker);
        if previous.is_some() && dmg > 0 {
            change_exp_ratio(cx, a_id, d_id);
        }
        if let Some(d) = cx.w.store.mover_mut(d_id) {
            d.combat.id_attacker = Some(a_id);
        }
    }
    if let Some(a) = cx.w.store.mover_mut(a_id) {
        a.combat.id_last_hit = Some(d_id);
    }
}

/// `ChangeExpRatio(attacker, defender)` (M:1840-1862): a different attacker (a different party, or one of them
/// without a party) of a non-`SUPER` NPC sets `m_fExpRatio` to the NPC's remaining HP share. **Write-only**: no
/// code reads `exp_ratio` (Plan/17 §1.4, §5.16).
fn change_exp_ratio(cx: &mut ArbiterCx<'_>, a_id: ObjId, d_id: ObjId) {
    let env = cx.env;
    let core = env.core;
    let (Some(att), Some(def)) = (valid(cx, a_id), valid(cx, d_id)) else {
        return;
    };
    let mut differ = false;
    if def.combat.id_attacker != Some(a_id) {
        let party = |m: &Mover| m.player().and_then(|p| p.social.party);
        if party(att).is_some() {
            if let Some(last) = def.combat.id_attacker.and_then(|id| valid(cx, id))
                && party(last) != party(att)
            {
                differ = true;
            }
        } else {
            differ = true;
        }
    }
    if differ
        && core
            .data
            .mover(def.prop_id())
            .is_none_or(|p| p.class != RANK_SUPER)
    {
        let max = stat_max_hp(def, core);
        let ratio = point_param(def, core, DST_HP) as f32 / max as f32;
        if let Some(d) = cx.w.store.mover_mut(d_id) {
            d.combat.exp_ratio = ratio;
        }
    }
}

// ---- OnDamaged and the Damage message ------------------------------------------------------------------------------

/// `CAttackArbiter::OnDamaged(dmg)` (A:~810): the victim enters attack mode (`atk_cnt = 1`), takes its damage
/// motion and the `Damage` is broadcast with the HP after the hit.
fn on_damaged(
    cx: &mut ArbiterCx<'_>,
    a_id: ObjId,
    d_id: ObjId,
    dmg: i32,
    flags: AtkFlags,
    hp_after: i32,
) {
    let yaw = cx.w.store.mover(a_id).map(|a| a.sim.angle);
    if let Some(d) = cx.w.store.mover_mut(d_id) {
        d.sim.atk_cnt = 1;
    }
    cx.fx.sim.push((
        d_id,
        SimFx::DamagedMotion {
            atk_flags: flags.bits(),
            attacker: yaw.map(|y| (a_id, y)),
        },
    ));
    broadcast_damage(cx, a_id, d_id, dmg, flags, hp_after);
}

/// Queues the `Damage` message (`ServerMsg.obj_id` = the victim, `NearInclSelf`): `defender_pos` /
/// `defender_angle` only for a knock-up, `server_frame` = the frame the effect takes on the victim.
fn broadcast_damage(
    cx: &mut ArbiterCx<'_>,
    a_id: ObjId,
    d_id: ObjId,
    dmg: i32,
    flags: AtkFlags,
    hp_after: i32,
) {
    let attacker_angle = cx.w.store.mover(a_id).map_or(0.0, |a| a.sim.angle);
    let Some(d) = cx.w.store.mover(d_id) else {
        return;
    };
    let flying = flags.intersects(AtkFlags::FLYING);
    let msg = pb::Damage {
        attacker: a_id.0,
        damage: u32::try_from(dmg).unwrap_or(0),
        atk_flags: flags.bits(),
        defender_pos: flying.then_some(pb::Vec3 {
            x: d.sim.pos.x,
            y: d.sim.pos.y,
            z: d.sim.pos.z,
        }),
        defender_angle: if flying { d.sim.angle } else { 0.0 },
        hp_after,
        server_frame: cx
            .w
            .frame
            .unwrap_or_else(|| crate::movement::effect_frame(d, cx.w.tick))
            .0,
        attacker_angle,
    };
    cx.fx.near(d_id, msg);
}

/// The AUTOHP step after `OnDamaged` (A:~250): `rate = victim.adj(AUTOHP)`; when positive and `hp / max_hp <=
/// rate / 100` the victim heals `(int)(max_hp * (chg(AUTOHP) / 100.0f))`: a forced `SetPointParam(HP)`, the
/// `XI_GEN_CURE01` effect and the end of `SI_ASS_HEAL_PREVENTION`. Returns the HP the commit stores.
fn auto_hp(cx: &mut ArbiterCx<'_>, d_id: ObjId, new_hp: i32) -> i32 {
    let core = cx.env.core;
    let Some(def) = cx.w.store.mover(d_id) else {
        return new_hp;
    };
    let rate = def.mv.dst.table.adj(DST_AUTOHP);
    if rate <= 0 {
        return new_hp;
    }
    let max = stat_max_hp(def, core);
    let chg = def.mv.dst.table.chg(DST_AUTOHP);
    let percent = new_hp as f32 / max as f32;
    if percent > rate as f32 / 100.0_f32 {
        return new_hp;
    }
    let add = c_int(max as f32 * (chg as f32 / 100.0_f32));
    let hp = new_hp.wrapping_add(add);
    if let Some(m) = cx.w.store.mover_mut(d_id) {
        let mut out = Out::new();
        set_point_param(m, core, DST_HP, hp, true, &mut out);
        cx.fx.extend_out(d_id, out);
    }
    cx.fx.near(
        d_id,
        pb::CreateSfxObj {
            sfx_id: flyff_common::defs::obj::XI_GEN_CURE01,
            x: 0.0,
            y: 0.0,
            z: 0.0,
            flag: false,
        },
    );
    cx.hooks
        .remove_skill_buff(&mut cx.w, &mut cx.fx, d_id, SI_ASS_HEAL_PREVENTION);
    hp
}

// ---- area damage -------------------------------------------------------------------------------------------------------

/// `nApplyType` of `SendDamageAround`: which kinds of movers are hit (`OBJTYPE_PLAYER | OBJTYPE_MONSTER`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApplyMask(u8);

impl ApplyMask {
    /// Players.
    pub const PLAYER: ApplyMask = ApplyMask(1);
    /// Non-peaceful NPCs.
    pub const MONSTER: ApplyMask = ApplyMask(2);
    /// Both.
    pub const BOTH: ApplyMask = ApplyMask(3);

    /// `true` when every bit of `other` is in this mask.
    pub const fn has(self, other: ApplyMask) -> bool {
        self.0 & other.0 == other.0
    }
}

/// The inputs of `CMover::SendDamageAround` (`MoverSkill.cpp:3571-3689`; the area is centred on the mover
/// `center` = `this`) and of `CWorld::SendDamageAround` (`World.cpp:1804`; a bare position, `center: None`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AroundSpec {
    /// The target the area is centred on (`this`); `None` for the point variant.
    pub center: Option<ObjId>,
    /// The centre of the area (the mover's position, or the point).
    pub pos: flyff_common::math::Vec3,
    /// `nDmgType`: the `AF_*` of every hit.
    pub flags: AtkFlags,
    /// The attacker, never hit itself; its world and layer are the area's.
    pub attacker: ObjId,
    /// Which kinds of movers are hit.
    pub apply: ApplyMask,
    /// The skill (`AF_MAGICSKILL`) or item id (`nAttackID`); the parameter of every hit is `id << 16`.
    pub attack_id: u32,
    /// `fRange` in metres (hits the movers strictly closer than this).
    pub range: f32,
    /// `fTargetRatio`: 0 spares the centre mover itself; any other value hits it like the rest (the ratios have no
    /// other effect). Unused by the point variant.
    pub target_ratio: f32,
}

impl AroundSpec {
    /// `CWorld::SendDamageAround(pos, flags, attacker, apply, id, range)` (bosses, doc 18): the area around a bare
    /// position; every target counts as the primary one. Deliver it with [`send_damage_around`].
    pub fn at_point(
        pos: flyff_common::math::Vec3,
        flags: AtkFlags,
        attacker: ObjId,
        apply: ApplyMask,
        attack_id: u32,
        range: f32,
    ) -> AroundSpec {
        AroundSpec {
            center: None,
            pos,
            flags,
            attacker,
            apply,
            attack_id,
            range,
            target_ratio: 1.0,
        }
    }
}

/// One mover an area attack hits: `(id, is_target)`.
pub type AroundHit = (ObjId, bool);

/// The movers an area attack covers, in the order of the original: first the players of the legacy link-map
/// window (`FOR_LINKMAP linkPlayer`, window `aoe_n_range(range)`), then the non-peaceful NPCs. A mover is
/// covered when it is strictly closer than `range` (3-D, `f32`), is not the attacker, and is not the centre
/// mover when `target_ratio == 0`. Players carry `is_target = (id == centre)` (always `true` for the point
/// variant); monsters always `true`.
pub fn around_candidates(
    grid: &crate::world::grid::Grid,
    store: &ObjectStore,
    spec: &AroundSpec,
) -> Vec<AroundHit> {
    use crate::world::grid::{LinkSet, aoe_n_range};
    let mut out = Vec::new();
    let pos = spec.pos;
    let n_range = aoe_n_range(spec.range);
    let r2 = spec.range * spec.range;
    let near = |id: ObjId| -> Option<&Mover> {
        let m = store.mover(id)?;
        let d = m.core.pos - pos;
        let dist_sq = d.x * d.x + d.y * d.y + d.z * d.z;
        (dist_sq < r2).then_some(m)
    };
    let skip =
        |id: ObjId| (Some(id) == spec.center && spec.target_ratio == 0.0) || id == spec.attacker;
    if spec.apply.has(ApplyMask::PLAYER) {
        grid.for_each_legacy(pos, n_range, LinkSet::Players, |id| {
            if near(id).is_some() && !skip(id) {
                out.push((id, spec.center.is_none_or(|c| c == id)));
            }
        });
    }
    if spec.apply.has(ApplyMask::MONSTER) {
        grid.for_each_legacy(pos, n_range, LinkSet::Others, |id| {
            if let Some(m) = near(id)
                && !is_peaceful(m)
                && !skip(id)
            {
                out.push((id, true));
            }
        });
    }
    out
}

/// Delivers the hits of an area attack: each live target gets `send_damage(flags, attacker, id << 16, is_target)`
/// (`send_damage_force` for a `CT_FINISH` combo style). Returns the number of movers the loop delivered to.
pub fn damage_around_hits(cx: &mut ArbiterCx<'_>, spec: &AroundSpec, hits: &[AroundHit]) -> u32 {
    let data = &cx.env.core.data;
    let prop = if spec.flags.intersects(AtkFlags::MAGICSKILL) {
        data.skill(spec.attack_id)
    } else {
        data.item(spec.attack_id)
    };
    let Some(prop) = prop else {
        return 0;
    };
    let finish = prop.combo_style == CT_FINISH;
    let mut n = 0;
    for &(id, is_target) in hits {
        // a target that died from an earlier hit of the loop is skipped (`IsValidObj && IsLive`)
        if !valid(cx, id).is_some_and(is_live) {
            continue;
        }
        let mut req = DamageRequest::new(
            id,
            spec.attacker,
            spec.flags,
            DamageParam::item_attack(spec.attack_id),
        );
        req.is_target = is_target;
        req.force = finish;
        process_damage(cx, req);
        n += 1;
    }
    n
}

// ---- handler-context entry points --------------------------------------------------------------------------------------

/// The read-only environment of the damage pipeline for a handler: the rule tables of the stats service, the
/// switches of the configuration, the element table, the Event.lua factors and the live settings.
pub fn damage_env<'s>(s: &'s Services, ctx: &GameCtx<'_>) -> DamageEnv<'s> {
    DamageEnv {
        core: s.stats.core(),
        cfg: flyff_rules::combat::CombatConfig::from_config(&ctx.cfg.rules),
        attr: s.combat.attr.get(ctx.data),
        events: s.event.factors(),
        steal_protection: ctx.live.event_state(flyff_common::config::EventId::Steal) == 0,
        monster_hit_rate: ctx.live.rates.monster_hit.unwrap_or(1.0),
        pvp: super::pvp::PvpEnv::from_services(s, ctx),
    }
}

/// Builds the damage context of a handler (the services and the [`GameCtx`]), runs `f` on it, delivers the
/// collected effects and processes the deaths the call caused. `world_of` names the mover whose world supplies
/// the random stream; `None` when that mover does not exist.
pub fn with_arbiter<R>(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    world_of: ObjId,
    f: impl FnOnce(&mut ArbiterCx<'_>) -> R,
) -> Option<R> {
    let world = ctx.worlds.core(world_of)?.world;
    let env = damage_env(s, ctx);
    let frame = ctx.clock.frame0;
    let (r, fx) = {
        let GameCtx {
            worlds, rng, clock, ..
        } = &mut *ctx;
        let crate::world::Worlds {
            store, movement, ..
        } = &mut **worlds;
        let w = Arena {
            store,
            rng: rng.world(world),
            now: clock.game_ms,
            tick: clock.tick,
            frame: None,
        };
        let mut cx = ArbiterCx::new(w, &env, &mut *movement.combat.hooks);
        let r = f(&mut cx);
        (r, cx.fx)
    };
    fx.deliver_game_at(ctx, frame);
    super::death::process_pending(s, ctx);
    crate::skill::cast::drain_jobs(s, ctx);
    Some(r)
}

/// `CActionMover::SendDamage` in a handler context (GM commands, skill application, tests): the damage the hit
/// did. The `Damage` broadcast, the AI mail and the sim calls are delivered before it returns.
pub fn send_damage(s: &mut Services, ctx: &mut GameCtx<'_>, req: DamageRequest) -> i32 {
    with_arbiter(s, ctx, req.victim, |cx| process_damage(cx, req)).unwrap_or(0)
}

/// `CMover::OnAttacked(attacker, 0, TRUE, 0)` of a hostile skill (`CCtrl::DoApplySkill`, `Ctrl.cpp:1458`): the
/// victim remembers the attacker (hate list, PK and PvP bookkeeping) although the skill may have dealt no damage.
pub fn skill_attacked(s: &mut Services, ctx: &mut GameCtx<'_>, victim: ObjId, attacker: ObjId) {
    let _ = with_arbiter(s, ctx, victim, |cx| {
        on_attacked(cx, attacker, victim, 0, true, 0)
    });
}

/// `CActionMover::SendDamageForce` (`OBJMSG_DAMAGE_FORCE`): like [`send_damage`]; it only changes the death
/// motion, and is downgraded for a victim that cannot be knocked up.
pub fn send_damage_force(s: &mut Services, ctx: &mut GameCtx<'_>, req: DamageRequest) -> i32 {
    send_damage(s, ctx, req.forced())
}

/// `CMover::SendDamageAround` / `CWorld::SendDamageAround` in a handler context: the movers of
/// [`around_candidates`] (the attacker's world and layer) each take a hit. Returns the number of movers hit.
pub fn send_damage_around(s: &mut Services, ctx: &mut GameCtx<'_>, spec: AroundSpec) -> u32 {
    let hits = {
        let Some(core) = ctx.worlds.core(spec.attacker) else {
            return 0;
        };
        let (world, layer) = (core.world, core.layer);
        let Some(l) = ctx.worlds.layer(world, layer) else {
            return 0;
        };
        around_candidates(&l.grid, ctx.worlds.store(), &spec)
    };
    with_arbiter(s, ctx, spec.attacker, |cx| {
        damage_around_hits(cx, &spec, &hits)
    })
    .unwrap_or(0)
}
