//! BigMuscle (`MI_BIGMUSCLE`, `CAIBigMuscle`; Plan/18 §10.2-§10.4, task boss-ai-b, WP-AI-19): the opener of its
//! pre-step, the shouts of its arrival and the four hit handlers (`OnAttackMelee_BigMuscle`,
//! `MoverActEvent.cpp:1020-1156`). The bands are [`super::tables::choose_attack`], the process is
//! [`super::rage`].
//!
//! | swing | `OBJMSG` / item | reach | arrival shout | hit frames |
//! |---|---|---|---|---|
//! | Normal | `ATK1` / `dwAtk1` | 5 m (far: 10 m) | - | every hit: single hit on the target |
//! | Normal2 | `ATK2` / `dwAtk1` | 5 m | - | every hit: single hit on the target |
//! | QuakeDouble | `ATK3` / `dwAtk3` | `R` | `MSG_04` around the target | hit 1: 16 m area + bleeding; other hits: stun group |
//! | QuakeOne | `ATK4` / `dwAtk2` | 15 m | `MSG_05` (target name) around the target | hit 0 remembers the target's position, hit 1: bleeding + 4 m area there |
//!
//! The defence mode of the original is commented out for BigMuscle.

use super::fx::{BossFx, HitEnv, HitFx, HitList, on_hit_with};
use super::{BossAi, BossAttack};
use crate::ai::ctx::{AiCmd, AiCx};
use crate::ai::scan::{JOB_ALL, scan_target};
use crate::combat::ActionCx;
use crate::mover::Mover;
use flyff_common::ObjId;
use flyff_common::defs::item::II_WEA_MOB_MONSTER4_ATK2;
use flyff_common::defs::obj::XI_SKILL_DROP_DUST_RAIN;
use flyff_common::defs::skill::{SI_BIGMUSCLE_STUNGROUP, SI_GEN_BLEEDING};
use flyff_common::defs::text::{
    TID_GAME_BOSS_BIGMUSCLE_MSG_01, TID_GAME_BOSS_BIGMUSCLE_MSG_02, TID_GAME_BOSS_BIGMUSCLE_MSG_03,
    TID_GAME_BOSS_BIGMUSCLE_MSG_04, TID_GAME_BOSS_BIGMUSCLE_MSG_05,
};
use flyff_common::math::Vec3;
use flyff_common::rng::GameRng;
use flyff_rules::combat::AtkFlags;
use flyff_sim::types::{OBJSTA_ATK1, OBJSTA_ATK2, OBJSTA_ATK3, OBJSTA_ATK4};

/// The opener scans this far (`ScanTarget(pMover, 80, JOB_ALL)`).
pub const OPENER_RANGE: i32 = 80;
/// The bleeding skill of the opener and of the ground slams, level 10.
pub const BLEEDING_LEVEL: u32 = 10;
/// `ApplySkillAround(.., 255.0f)`: the opener's bleeding reaches everybody in the visibility range.
pub const OPENER_SKILL_RANGE: f32 = 255.0;
/// The area of the two-handed slam (hit 1 of ATK3), metres.
pub const SLAM_RADIUS: f32 = 16.0;
/// The area of the one-handed slam (hit 1 of ATK4), metres.
pub const STOMP_RADIUS: f32 = 4.0;

/// The pre-step of `MoveProcessRage` (every RAGE `Process`, also during `Ready`): until it ran, the opener looks
/// for the first player within 80 m (`ScanTarget`, strict 3-D, a ground boss never takes a flying one) and, once
/// found, in this order: the caption (the world of the target), two shouts around the target, the dust-rain
/// effect on the target, the quake of the target's camera and the bleeding skill around the boss with a range of
/// 255 m. No draw is made.
pub fn pre_step(b: &mut BossAi, m: &mut Mover, cx: &mut AiCx<'_>) {
    if b.opener_done {
        return;
    }
    let Some(t) = scan_target(cx, m, OPENER_RANGE, JOB_ALL, 0, 0, 0) else {
        return;
    };
    b.opener_done = true;
    let Some(view) = crate::ai::monster::helpers::target_view(cx, t) else {
        return;
    };
    let (boss, world) = (m.id(), cx.world);
    let shout = |tid| BossFx::Shout {
        speaker: boss,
        world,
        center: view.pos,
        tid,
        name_of: None,
    };
    cx.cmds.extend([
        AiCmd::Boss(BossFx::Caption {
            world,
            tid: TID_GAME_BOSS_BIGMUSCLE_MSG_01,
            name_of: None,
        }),
        AiCmd::Boss(shout(TID_GAME_BOSS_BIGMUSCLE_MSG_02)),
        AiCmd::Boss(shout(TID_GAME_BOSS_BIGMUSCLE_MSG_03)),
        AiCmd::Boss(BossFx::Sfx {
            anchor: t,
            sfx: XI_SKILL_DROP_DUST_RAIN,
            pos: Vec3::ZERO,
        }),
        AiCmd::Boss(BossFx::Quake { anchor: t }),
        AiCmd::Boss(BossFx::SkillAround {
            caster: boss,
            skill: SI_GEN_BLEEDING,
            level: BLEEDING_LEVEL,
            range: OPENER_SKILL_RANGE,
        }),
    ]);
}

/// What BigMuscle says when its swing was accepted (`AIMSG_ARRIVAL`): QuakeDouble shouts `MSG_04` and QuakeOne
/// `MSG_05` (with the target's name), both centred on the TARGET and only while the target still resolves; Normal
/// and Normal2 say nothing.
pub fn arrival_extras(b: &BossAi, m: &Mover, cx: &mut AiCx<'_>, target: ObjId) {
    let (tid, named) = match b.attack {
        Some(BossAttack::QuakeDouble) => (TID_GAME_BOSS_BIGMUSCLE_MSG_04, None),
        Some(BossAttack::QuakeOne) => (TID_GAME_BOSS_BIGMUSCLE_MSG_05, Some(target)),
        _ => return,
    };
    let Some(view) = crate::ai::monster::helpers::target_view(cx, target) else {
        return;
    };
    cx.cmds.push(AiCmd::Boss(BossFx::Shout {
        speaker: m.id(),
        world: cx.world,
        center: view.pos,
        tid,
        name_of: named,
    }));
}

/// `OnAttackMelee_BigMuscle` at a hit frame (the `m_nMotionHitCount++` is the dispatcher's).
pub fn on_hit(cx: &mut ActionCx<'_>, boss: ObjId, state: u32, target: ObjId) {
    on_hit_with(cx, boss, target, |hit, me, t, env, rng| {
        hit_effects(state, hit, me, t, env, rng)
    });
}

/// The effects of the hit `hit` (`m_nMotionHitCount`) of swing `state`, pure over its arguments.
///
/// * ATK1 / ATK2: a single hit on the hit object (nothing without one);
/// * ATK3: hit 1 -> a 16 m area around the boss (`AF_GENERIC | AF_CRITICAL`, the `MONSTER4_ATK2` item) and the
///   bleeding skill around it; every other hit -> the stun group around it;
/// * ATK4: hit 0 remembers the target's position in `env.atk4_pos` (a function-static in the original, Q18: here
///   one per boss; without a target the memory is cleared); hit 1 -> bleeding on the target and a 4 m area at the
///   remembered position (FIX: no area without one; the original read a stale or zero static); later hits nothing.
pub fn hit_effects(
    state: u32,
    hit: i32,
    me: Vec3,
    target: Option<(ObjId, Vec3)>,
    env: &mut HitEnv<'_>,
    _rng: &mut dyn GameRng,
) -> HitList {
    let boss = env.boss;
    let mut out = HitList::new();
    match state {
        OBJSTA_ATK1 | OBJSTA_ATK2 => {
            if let Some((t, _)) = target {
                out.push(HitFx::Damage {
                    item: None,
                    target: t,
                });
            }
        }
        OBJSTA_ATK3 => {
            if hit == 1 {
                out.push(HitFx::Around {
                    center: me,
                    flags: AtkFlags::GENERIC | AtkFlags::CRITICAL,
                    item: II_WEA_MOB_MONSTER4_ATK2,
                    radius: SLAM_RADIUS,
                });
                out.push(HitFx::Job(BossFx::SkillAround {
                    caster: boss,
                    skill: SI_GEN_BLEEDING,
                    level: BLEEDING_LEVEL,
                    range: 0.0,
                }));
            } else {
                out.push(HitFx::Job(BossFx::SkillAround {
                    caster: boss,
                    skill: SI_BIGMUSCLE_STUNGROUP,
                    level: 1,
                    range: 0.0,
                }));
            }
        }
        OBJSTA_ATK4 => match hit {
            0 => {
                if let Some(slot) = env.atk4_pos.as_deref_mut() {
                    *slot = target.map(|(_, p)| p);
                }
            }
            1 => {
                if let Some((t, _)) = target {
                    out.push(HitFx::Job(BossFx::Skill {
                        caster: boss,
                        target: t,
                        skill: SI_GEN_BLEEDING,
                        level: BLEEDING_LEVEL,
                        how: super::fx::SkillHow::Do,
                    }));
                }
                let remembered = env.atk4_pos.as_deref().copied().flatten();
                if let Some(p) = remembered {
                    out.push(HitFx::Around {
                        center: p,
                        flags: AtkFlags::GENERIC | AtkFlags::CRITICAL,
                        item: II_WEA_MOB_MONSTER4_ATK2,
                        radius: STOMP_RADIUS,
                    });
                }
            }
            _ => {}
        },
        _ => {}
    }
    out
}
