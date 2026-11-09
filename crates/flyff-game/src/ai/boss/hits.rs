//! The hit handlers of the boss models (Plan/18 §10.4, Plan/17 §3.8; `MoverActEvent.cpp:921-1018, 1542-1695,
//! 1776-1832`; task boss-ai-core, WP-AI-18): what 17's hit dispatch calls for `MI_CLOCKWORK1` (and
//! `MI_LOADCLOCKWORK` with `features.better_clockwork_alpha`), `MI_BIGMUSCLE`, `MI_KRRR`, `MI_MUSHMOOT` and the
//! Meteonyker models, plus the helpers the per-boss handlers share: `OnDamage` ([`on_damage_at`],
//! [`on_damage_target`]), `GetEventPos` and `GetMatrixBone` on the server ([`event_pos`], [`bone_point`]) and the
//! `m_nMotionHitCount++` of the dispatcher ([`bump_motion_hit_count`]).
//!
//! * [`on_attack_melee`] runs at the barrier of the hit frame of a swing (`CMover::OnAttackMelee`); `true` means
//!   the boss handled it and the generic hit is skipped. `MI_EVENT04` runs the ClockWorks AI but its swings stay
//!   ordinary hits (`is_boss_model` does not list it, and it answers `false` here as well).
//! * [`on_attack_melee_continue`] runs at the barrier of every frame after the hit while `MVRF_HITCONT` is set
//!   (`CMover::OnAttackMeleeContinue`); [`wants_continue`] is its cheap pre-filter.
//!
//! **The hit counter.** The `m_nMotionHitCount++` after BigMuscle, Krrr, Bear and Meteonyker (and NOT after
//! ClockWorks) sits in the C++ dispatcher right after the per-boss handler; here it is in the dispatcher's arm
//! ([`bump_motion_hit_count`]), exactly once per hit, never inside a per-boss `on_hit`.
//!
//! **Where the other bosses plug in.** Each arm of [`on_attack_melee`] / [`on_attack_melee_continue`] that
//! WP-AI-19..21 adds has the form `MI_X => { x::on_hit(cx, boss, state, target); bump_motion_hit_count(cx, boss);
//! true }`.

use super::{bear, bigmuscle, clockworks, krrr, pose};
use crate::combat::ActionCx;
use crate::combat::arbiter::{
    ApplyMask, AroundSpec, DamageRequest, around_candidates, damage_around_hits, process_damage,
};
use flyff_common::ObjId;
use flyff_common::defs::attribute::SRO_AROUND;
use flyff_common::defs::obj::{
    MI_BIGMUSCLE, MI_CLOCKWORK1, MI_DU_METEONYKER, MI_DU_METEONYKER2, MI_DU_METEONYKER3,
    MI_DU_METEONYKER4, MI_KRRR, MI_LOADCLOCKWORK, MI_MUSHMOOT,
};
use flyff_common::math::Vec3;
use flyff_rules::combat::{AtkFlags, DamageParam};

/// `true` for the prop ids whose melee continuation is a boss script (a cheap filter that needs no config: every
/// melee monster raises `MeleeContinue` each frame after its hit): the ClockWorks models; WP-AI-21 adds the
/// Meteonyker ids. `MI_LOADCLOCKWORK`'s config switch is tested by [`on_attack_melee_continue`].
pub fn wants_continue(prop_id: u32) -> bool {
    matches!(prop_id, MI_CLOCKWORK1 | MI_LOADCLOCKWORK) || is_meteonyker(prop_id)
}

/// The four models of the Meteonyker (`MI_DU_METEONYKER` runs `CAIMonster`, the other three `CAIMeteonyker`; the
/// hit dispatcher of the original names all four).
pub const fn is_meteonyker(prop_id: u32) -> bool {
    matches!(
        prop_id,
        MI_DU_METEONYKER | MI_DU_METEONYKER2 | MI_DU_METEONYKER3 | MI_DU_METEONYKER4
    )
}

/// `CMover::OnAttackMelee(state, target)` for a boss model (the dispatch of `MoverActEvent.cpp:1650-1695`):
/// `true` when the boss handled the hit: the ClockWorks, `MI_BIGMUSCLE`, `MI_KRRR`, `MI_MUSHMOOT` and the four
/// Meteonyker ids (the last four bump the hit counter in their arm); everything else answers `false`.
pub fn on_attack_melee(cx: &mut ActionCx<'_>, boss: ObjId, state: u32, target: ObjId) -> bool {
    let Some(prop) = cx.mover(boss).map(|m| m.prop_id()) else {
        return false;
    };
    match prop {
        MI_CLOCKWORK1 => {
            clockworks::on_hit(cx, boss, state, target);
            true
        }
        MI_LOADCLOCKWORK if cx.act.better_clockwork_alpha => {
            clockworks::on_hit(cx, boss, state, target);
            true
        }
        // ---- boss-ai-b (WP-AI-19, -20, -21) ----
        MI_BIGMUSCLE => {
            bigmuscle::on_hit(cx, boss, state, target);
            bump_motion_hit_count(cx, boss);
            true
        }
        MI_KRRR => {
            krrr::on_hit(cx, boss, state, target);
            bump_motion_hit_count(cx, boss);
            true
        }
        MI_MUSHMOOT => {
            bear::on_hit(cx, boss, state, target);
            bump_motion_hit_count(cx, boss);
            true
        }
        MI_DU_METEONYKER | MI_DU_METEONYKER2 | MI_DU_METEONYKER3 | MI_DU_METEONYKER4 => {
            crate::ai::meteonyker::on_hit(cx, boss, state, target);
            bump_motion_hit_count(cx, boss);
            true
        }
        _ => false,
    }
}

/// `CMover::OnAttackMeleeContinue(state)` (`MoverActEvent.cpp:1776-1832`): `count` is the free-running
/// `CActionMover::m_nCount` and `hit_count` `m_nMotionHitCount` at the frame (the `MeleeContinue` payload). A
/// dead boss does nothing.
pub fn on_attack_melee_continue(
    cx: &mut ActionCx<'_>,
    boss: ObjId,
    state: u32,
    count: i32,
    hit_count: i32,
) {
    let Some(m) = cx.mover(boss) else { return };
    if crate::combat::is_die(m) {
        return;
    }
    let prop = m.prop_id();
    match prop {
        MI_CLOCKWORK1 => clockworks::on_continue(cx, boss, state, count, hit_count),
        MI_LOADCLOCKWORK if cx.act.better_clockwork_alpha => {
            clockworks::on_continue(cx, boss, state, count, hit_count);
        }
        // ---- boss-ai-b (WP-AI-21): the Meteonyker fire breath ----
        MI_DU_METEONYKER | MI_DU_METEONYKER2 | MI_DU_METEONYKER3 | MI_DU_METEONYKER4 => {
            crate::ai::meteonyker::on_continue(cx, boss, state, count, hit_count);
        }
        _ => {}
    }
}

/// `m_pActMover->m_nMotionHitCount++` (the dispatcher's line after BigMuscle, Krrr, Bear and Meteonyker).
pub fn bump_motion_hit_count(cx: &mut ActionCx<'_>, boss: ObjId) {
    if let Some(m) = cx.mover_mut(boss) {
        m.sim.act.motion_hit_count = m.sim.act.motion_hit_count.saturating_add(1);
    }
}

/// `GetActParam(1)`: the attack item the swing was started with (`m_nAParam[1]`, stored by `DoAttackMelee`).
pub fn act_item(cx: &ActionCx<'_>, boss: ObjId) -> u32 {
    cx.mover(boss)
        .and_then(|m| u32::try_from(m.combat.act.p[1]).ok())
        .unwrap_or(0)
}

// ---- OnDamage ----------------------------------------------------------------------------------------------------

/// `CMover::OnDamage(item, state, NULL, &pos, flags)` (`MoverActEvent.cpp:1542-1643`, no hit object): an item with
/// `dwSpellRegion == SRO_AROUND` damages every player within `_dwSkillRange` metres of `pos`
/// (`CWorld::SendDamageAround`); anything else (a single-target item, an unknown item) does nothing, there being
/// no hit object. The default `flags` of the original is `AF_GENERIC`.
pub fn on_damage_at(cx: &mut ActionCx<'_>, boss: ObjId, item: u32, pos: Vec3, flags: AtkFlags) {
    let Some(range) = around_range(cx, item) else {
        return;
    };
    let spec = AroundSpec::at_point(pos, flags, boss, ApplyMask::PLAYER, item, range);
    let hits = around_candidates(cx.grid, cx.arb.w.store, &spec);
    damage_around_hits(&mut cx.arb, &spec, &hits);
}

/// `CMover::OnDamage(item, state, target, NULL)` with a hit object: an `SRO_AROUND` item damages the players
/// around `target` (`CMover::SendDamageAround`, centred on the target mover, `AF_GENERIC`); otherwise the single
/// hit with the flags of the swing (`m_dwAtkFlags`, rolled at the swing's first frame). Returns the damage of the
/// single hit (0 for an area attack).
pub fn on_damage_target(cx: &mut ActionCx<'_>, boss: ObjId, item: u32, target: ObjId) -> i32 {
    if let Some(range) = around_range(cx, item) {
        let Some(pos) = cx.mover(target).map(|t| t.core.pos) else {
            return 0;
        };
        let spec = AroundSpec {
            center: Some(target),
            pos,
            flags: AtkFlags::GENERIC,
            attacker: boss,
            apply: ApplyMask::PLAYER,
            attack_id: item,
            range,
            target_ratio: 1.0,
        };
        let hits = around_candidates(cx.grid, cx.arb.w.store, &spec);
        damage_around_hits(&mut cx.arb, &spec, &hits);
        return 0;
    }
    let flags = cx.mover(boss).map_or(0, |m| m.sim.act.atk_flags);
    process_damage(
        &mut cx.arb,
        DamageRequest::new(target, boss, AtkFlags(flags), DamageParam(1)),
    )
}

/// The area range of an `SRO_AROUND` attack item (`_dwSkillRange`, metres); `None` for any other or an unknown
/// item.
fn around_range(cx: &ActionCx<'_>, item: u32) -> Option<f32> {
    if item == 0 || item == flyff_rules::consts::NULL_ID {
        return None;
    }
    cx.arb
        .env
        .core
        .data
        .item(item)
        .filter(|p| p.spell_region == SRO_AROUND)
        .map(|p| p.skill_range as f32)
}

// ---- GetEventPos / GetMatrixBone ---------------------------------------------------------------------------------

/// The pose inputs of a boss at its hit: prop id, motion id, the clock the barrier sees, the world matrix and
/// the position.
struct HitPose {
    prop: u32,
    mti: u32,
    clock: flyff_sim::MotionClock,
    world: flyff_common::math::D3dMatrix,
    pos: Vec3,
}

fn hit_pose(cx: &ActionCx<'_>, boss: ObjId) -> Option<HitPose> {
    let m = cx.mover(boss)?;
    Some(HitPose {
        prop: m.prop_id(),
        mti: m.sim.motion.id,
        clock: m.sim.motion.clock,
        world: pose::world_matrix(m),
        pos: m.sim.pos,
    })
}

/// Logs ONE warning per boss object (`BossAi.pose_warned`; Plan/18 §10.4 says "per mover id", a respawned boss
/// is a new object and logs again) that its pose data is missing and the hit uses its position.
fn warn_missing_pose(cx: &mut ActionCx<'_>, boss: ObjId, p: &HitPose, what: &str) {
    let brain = cx
        .mover_mut(boss)
        .and_then(|m| m.monster_mut())
        .and_then(|e| e.ai.boss_mut());
    if let Some(b) = brain {
        if b.pose_warned {
            return;
        }
        b.pose_warned = true;
    }
    tracing::warn!(
        boss = ?boss,
        prop = p.prop,
        motion = p.mti,
        what,
        "boss pose data missing (no rig, skeleton, clip, event slot or bone): the hit uses the boss position"
    );
}

/// `GetEventPos(event)` then `TransformCoord(.., GetMatrixWorldPtr())` of `boss`, at the frame its hit saw
/// ([`pose::event_pos`] on `MoveShared.boss_poses`). Without pose data (no source, skeleton, clip, event slot or
/// bone) the boss position, with one warning per boss object.
pub fn event_pos(cx: &mut ActionCx<'_>, boss: ObjId, event: usize) -> Vec3 {
    let Some(p) = hit_pose(cx, boss) else {
        return Vec3::ZERO;
    };
    let rig = cx.sc.shared.boss_poses.as_deref();
    match pose::event_pos(rig, p.prop, p.mti, &p.clock, event, &p.world) {
        Some(v) => v,
        None => {
            warn_missing_pose(cx, boss, &p, "event point");
            p.pos
        }
    }
}

/// `TransformCoord(TransformCoord(local, GetMatrixBone(bone)), GetMatrixWorldPtr())` of `boss`. Without pose data
/// the bone is the identity: `world * local` (the model-space offset scaled and turned with the boss), with one
/// warning per boss object.
pub fn bone_point(cx: &mut ActionCx<'_>, boss: ObjId, bone: usize, local: Vec3) -> Vec3 {
    let Some(p) = hit_pose(cx, boss) else {
        return local;
    };
    let rig = cx.sc.shared.boss_poses.as_deref();
    match pose::bone_point(rig, p.prop, p.mti, &p.clock, bone, local, &p.world) {
        Some(v) => v,
        None => {
            warn_missing_pose(cx, boss, &p, "bone");
            flyff_sim::pose::quat::transform_coord(&p.world, local)
        }
    }
}
