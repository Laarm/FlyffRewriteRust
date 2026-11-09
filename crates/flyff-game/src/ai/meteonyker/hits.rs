//! The hit handlers of the Meteonyker models (`OnAttackMelee_Meteonyker`, `OnAttackMeleeContinue`;
//! `MoverActEvent.cpp:1309-1392, 1858-1900`; Plan/18 §10.4; task boss-ai-b, WP-AI-21): the pure tables
//! [`hit_effects`] / [`continue_effects`] and the barrier entry points [`on_hit`] / [`on_continue`] that
//! `ai::boss::hits` calls for `MI_DU_METEONYKER`, `_2`, `_3` and `_4`.

use crate::ai::boss::fx::{BossFx, HitEnv, HitFx, HitList, SkillHow, exec, on_hit_with};
use crate::ai::boss::hits;
use crate::ai::rng::x_rand;
use crate::combat::ActionCx;
use flyff_common::ObjId;
use flyff_common::math::Vec3;
use flyff_common::rng::GameRng;
use flyff_sim::types::{OBJSTA_ATK1, OBJSTA_ATK2, OBJSTA_ATK3, OBJSTA_ATK4};

/// `OnAttackMelee_Meteonyker` at a hit frame (`hits::on_attack_melee` bumps the hit counter after it).
pub fn on_hit(cx: &mut ActionCx<'_>, boss: ObjId, state: u32, target: ObjId) {
    on_hit_with(cx, boss, target, |hit, me, t, env, rng| {
        hit_effects(state, hit, me, t, env, rng)
    });
}

/// The effects of the hit `hit` (`m_nMotionHitCount`) of swing `state`, pure over its arguments.
///
/// * ATK1: every hit -> a hit on the hit object (`METEONYKER_ATK1` is `SRO_AROUND` 10 m: the area around it);
/// * ATK3 (the fireball): hit 0 with a hit object -> the damage (`METEONYKER_ATK3`, 5 m around the target; the
///   original passes the target's position too, ignored with a hit object), then, only when the `SI_GEN_STUNSINGLE`
///   lv 5 row exists, one raw draw: odd -> the stun on the target;
/// * ATK4 (the burn): hit 0, only when the `SI_GEN_BURN` lv 1 row exists -> the burn skill around the boss
///   (range 10) and the `dwAtk4` area (10 m) around the boss (the damage lands first, see the module docs of
///   [`crate::ai::boss::fx`]);
/// * ATK2: nothing at the hit frame (see [`continue_effects`]).
pub fn hit_effects(
    state: u32,
    hit: i32,
    me: Vec3,
    target: Option<(ObjId, Vec3)>,
    env: &mut HitEnv<'_>,
    rng: &mut dyn GameRng,
) -> HitList {
    let boss = env.boss;
    let mut out = HitList::new();
    match state {
        OBJSTA_ATK1 => {
            if let Some((t, _)) = target {
                out.push(HitFx::Damage {
                    item: None,
                    target: t,
                });
            }
        }
        OBJSTA_ATK3 if hit == 0 => {
            if let Some((t, _)) = target {
                out.push(HitFx::Damage {
                    item: None,
                    target: t,
                });
                // the draw is made only when the row exists (`if( bFind ) if( xRand() % 2 )`)
                if (env.skill_exists)(STUN_SKILL, STUN_LEVEL) && !x_rand(rng).is_multiple_of(2) {
                    out.push(HitFx::Job(BossFx::Skill {
                        caster: boss,
                        target: t,
                        skill: STUN_SKILL,
                        level: STUN_LEVEL,
                        how: SkillHow::Do,
                    }));
                }
            }
        }
        OBJSTA_ATK4 if hit == 0 && (env.skill_exists)(BURN_SKILL, BURN_LEVEL) => {
            out.push(HitFx::Job(BossFx::SkillAround {
                caster: boss,
                skill: BURN_SKILL,
                level: BURN_LEVEL,
                range: BURN_RANGE,
            }));
            out.push(HitFx::DamageAt {
                item: Some(env.atk[3]),
                pos: me,
            });
        }
        _ => {}
    }
    out
}

/// `SI_GEN_STUNSINGLE`, applied by the fireball with a chance of one half.
pub const STUN_SKILL: u32 = flyff_common::defs::skill::SI_GEN_STUNSINGLE;
/// Its level.
pub const STUN_LEVEL: u32 = 5;
/// `SI_GEN_BURN`, applied around the Meteonyker by the burn swing.
pub const BURN_SKILL: u32 = flyff_common::defs::skill::SI_GEN_BURN;
/// Its level.
pub const BURN_LEVEL: u32 = 1;
/// `ApplySkillAround(.., 10.0F)`: the burn's range in metres.
pub const BURN_RANGE: f32 = 10.0;

/// `OnAttackMeleeContinue_Meteonyker` (the fire breath): while the ATK2 swing plays with `m_nMotionHitCount == 1`,
/// every frame whose free-running `count & 3 == 0` (about 15 Hz) damages the players around the Meteonyker with
/// the swing's item (`METEONYKER_ATK2`, 30 m). The count stays 1 after the single `MA_HIT` of the clip, so the
/// breath lasts to the end of the swing (KQ-13 Q9, KEEP); the event points and bone 6 of the original are unused
/// server side.
pub fn continue_effects(state: u32, count: i32, hit_count: i32, me: Vec3) -> Option<HitFx> {
    (state == OBJSTA_ATK2 && hit_count == 1 && count & 3 == 0).then_some(HitFx::DamageAt {
        item: None,
        pos: me,
    })
}

/// `OnAttackMeleeContinue` of the Meteonyker models (`hits::on_attack_melee_continue` calls it every frame after
/// the hit while `MVRF_HITCONT`; `count` and `hit_count` are the values of the frame).
pub fn on_continue(cx: &mut ActionCx<'_>, boss: ObjId, state: u32, count: i32, hit_count: i32) {
    let Some(me) = cx.mover(boss).map(|m| m.sim.pos) else {
        return;
    };
    if let Some(fx) = continue_effects(state, count, hit_count, me) {
        let item = hits::act_item(cx, boss);
        exec(cx, boss, item, &[fx]);
    }
}
