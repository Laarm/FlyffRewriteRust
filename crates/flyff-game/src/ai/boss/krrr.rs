//! Krrr (`MI_KRRR`, `CAIKrrr`; Plan/18 §10.2-§10.4, task boss-ai-b, WP-AI-19): the opener and the meteor shower
//! of its pre-step, the shouts of its arrival (centred on the BOSS, unlike BigMuscle's) and the four hit handlers
//! (`OnAttackMelee_Krrr`, `MoverActEvent.cpp:1158-1306`). The bands are [`super::tables::choose_attack`].
//!
//! | swing | `OBJMSG` / item | reach | arrival shout | hit frames |
//! |---|---|---|---|---|
//! | Normal | `ATK1` or `ATK2` (drawn at the arrival) / `dwAtk1` | 7 m | - | ATK1: single hit; ATK2: 51 % poison, else single hit |
//! | DoubleCrash | `ATK3` / `dwAtk2` | 10 m | `KRR_MSG_02` (target name) | knock-back around the boss, 7 m area at the target |
//! | Summons | `ATK4` / `dwAtk3` | 0 m | `KRR_MSG_03` | hit 0: the party-quest pull (no data), hit 1: stun group, hit 3: 5 m area at the target |

use super::fx::{BossFx, HitEnv, HitFx, HitList, SkillHow, on_hit_with};
use super::{BossAi, BossAttack, MeteorEvent};
use crate::ai::ctx::{AiCmd, AiCx};
use crate::ai::monster::helpers::target_view;
use crate::ai::rng::{x_random, x_random_f};
use crate::ai::scan::{JOB_ALL, scan_target};
use crate::combat::ActionCx;
use crate::mover::Mover;
use flyff_common::ObjId;
use flyff_common::defs::item::{II_WEA_MOB_MONSTER4_ATK1, II_WEA_MOB_MONSTER4_ATK2};
use flyff_common::defs::obj::XI_SKILL_ELE_MULTI_METEOSHOWER02;
use flyff_common::defs::skill::{SI_GEN_KNOCK_BACK, SI_KRRR_POISON, SI_KRRR_STUNGROUP};
use flyff_common::defs::text::{
    TID_GAME_BOSS_KRR_MSG_01, TID_GAME_BOSS_KRR_MSG_02, TID_GAME_BOSS_KRR_MSG_03,
};
use flyff_common::math::Vec3;
use flyff_common::rng::GameRng;
use flyff_rules::combat::AtkFlags;
use flyff_sim::types::{OBJSTA_ATK1, OBJSTA_ATK2, OBJSTA_ATK3, OBJSTA_ATK4};

/// The opener scans this far (`ScanTarget(pMover, 60, JOB_ALL)`).
pub const OPENER_RANGE: i32 = 60;
/// `m_nEventCount`: the meteors of the shower.
pub const METEORS: u32 = 10;
/// A meteor falls when `xRandom(20) <= 0`: one in twenty processed ticks.
pub const METEOR_ODDS: u32 = 20;
/// The scatter of a meteor around the event target: `xRandomF(20) - 10` on x and z, metres.
pub const METEOR_SCATTER: f32 = 20.0;
/// The area of one meteor, metres.
pub const METEOR_RADIUS: f32 = 2.0;
/// `nRandom <= 50`: the poison threshold of ATK2 (51 of 100 values).
pub const POISON_MAX_ROLL: u32 = 50;
/// The poison and the stun group are applied at level 1.
pub const SKILL_LEVEL: u32 = 1;

/// The pre-step of `MoveProcessRage` (every RAGE `Process`, also during `Ready`).
///
/// The opener (until it ran): the first player within 60 m is the event target, the caption `KRR_MSG_01` goes to
/// its world and the shower starts with ten meteors. The meteor tick runs in the SAME call (the opener's tick
/// included) while meteors remain: one `x_random(20)` per call, `== 0` -> the counter drops (also when the event
/// target is gone) and, while the target resolves (a dead one still counts), a meteor falls at its position
/// scattered by `x_random_f(20) - 10` on x then z: the effect on the target's viewers and a 2 m area there. At 0
/// no further draw is made.
pub fn pre_step(b: &mut BossAi, m: &mut Mover, cx: &mut AiCx<'_>) {
    let boss = m.id();
    if !b.opener_done
        && let Some(t) = scan_target(cx, m, OPENER_RANGE, JOB_ALL, 0, 0, 0)
    {
        b.opener_done = true;
        cx.cmds.push(AiCmd::Boss(BossFx::Caption {
            world: cx.world,
            tid: TID_GAME_BOSS_KRR_MSG_01,
            name_of: None,
        }));
        b.meteor = Some(MeteorEvent {
            event_target: t,
            remaining: METEORS,
        });
    }
    let Some(ev) = b.meteor.as_mut() else { return };
    if ev.remaining == 0 || x_random(cx.rng, METEOR_ODDS) != 0 {
        return;
    }
    ev.remaining -= 1;
    let event_target = ev.event_target;
    if let Some(t) = target_view(cx, event_target) {
        let mut p = t.pos;
        p.x += x_random_f(cx.rng, METEOR_SCATTER) - METEOR_SCATTER / 2.0;
        p.z += x_random_f(cx.rng, METEOR_SCATTER) - METEOR_SCATTER / 2.0;
        cx.cmds.push(AiCmd::Boss(BossFx::Sfx {
            anchor: event_target,
            sfx: XI_SKILL_ELE_MULTI_METEOSHOWER02,
            pos: p,
        }));
        cx.cmds.push(AiCmd::DamageAround {
            attacker: boss,
            center: p,
            flags: AtkFlags::GENERIC,
            item: II_WEA_MOB_MONSTER4_ATK2,
            radius: METEOR_RADIUS,
        });
    }
}

/// What Krrr says when its swing was accepted: DoubleCrash `KRR_MSG_02` with the target's name (FIX: nothing when
/// the target vanished; the original dereferenced it) and Summons `KRR_MSG_03`, both centred on the BOSS.
pub fn arrival_extras(b: &BossAi, m: &Mover, cx: &mut AiCx<'_>, target: ObjId) {
    let (tid, named) = match b.attack {
        Some(BossAttack::DoubleCrash) => {
            if target_view(cx, target).is_none() {
                return;
            }
            (TID_GAME_BOSS_KRR_MSG_02, Some(target))
        }
        Some(BossAttack::Summons) => (TID_GAME_BOSS_KRR_MSG_03, None),
        _ => return,
    };
    cx.cmds.push(AiCmd::Boss(BossFx::Shout {
        speaker: m.id(),
        world: cx.world,
        center: m.sim.pos,
        tid,
        name_of: named,
    }));
}

/// `OnAttackMelee_Krrr` at a hit frame (the `m_nMotionHitCount++` is the dispatcher's).
pub fn on_hit(cx: &mut ActionCx<'_>, boss: ObjId, state: u32, target: ObjId) {
    on_hit_with(cx, boss, target, |hit, me, t, env, rng| {
        hit_effects(state, hit, me, t, env, rng)
    });
}

/// The effects of the hit `hit` (`m_nMotionHitCount`) of swing `state`, pure over its arguments.
///
/// * ATK1: a single hit on the hit object; ATK2: `r = x_random(100)` is ALWAYS drawn, `r <= 50` -> the poison
///   on the target (`pHitObj->ApplySkill(this, ..)`, no draw of its own), else a single hit;
/// * ATK3: every hit -> the knock-back around the boss, then a 7 m area at the target (`MONSTER4_ATK1`);
/// * ATK4: hit 0 -> nothing here: the original pulls the quest party of the `CPartyQuestProcessor` rectangle the
///   boss stands in to 5 m in front of it, and V15 has no party quest (`propPartyQuest.inc` is empty, F-253);
///   hit 1 -> the stun group around the boss; hit 2 -> client effects only; hit 3 -> a 5 m area at the target.
///
/// Every dereference of the hit object is guarded (FIX): without one its part is skipped.
pub fn hit_effects(
    state: u32,
    hit: i32,
    _me: Vec3,
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
        OBJSTA_ATK2 => {
            let roll = x_random(rng, 100);
            if let Some((t, _)) = target {
                out.push(if roll <= POISON_MAX_ROLL {
                    HitFx::Job(BossFx::Skill {
                        caster: boss,
                        target: t,
                        skill: SI_KRRR_POISON,
                        level: SKILL_LEVEL,
                        how: SkillHow::Receive,
                    })
                } else {
                    HitFx::Damage {
                        item: None,
                        target: t,
                    }
                });
            }
        }
        OBJSTA_ATK3 => {
            out.push(HitFx::Job(BossFx::SkillAround {
                caster: boss,
                skill: SI_GEN_KNOCK_BACK,
                level: 4,
                range: 0.0,
            }));
            if let Some((_, p)) = target {
                out.push(HitFx::Around {
                    center: p,
                    flags: AtkFlags::GENERIC,
                    item: II_WEA_MOB_MONSTER4_ATK1,
                    radius: 7.0,
                });
            }
        }
        OBJSTA_ATK4 => match hit {
            1 => out.push(HitFx::Job(BossFx::SkillAround {
                caster: boss,
                skill: SI_KRRR_STUNGROUP,
                level: SKILL_LEVEL,
                range: 0.0,
            })),
            3 => {
                if let Some((_, p)) = target {
                    out.push(HitFx::Around {
                        center: p,
                        flags: AtkFlags::GENERIC,
                        item: II_WEA_MOB_MONSTER4_ATK2,
                        radius: 5.0,
                    });
                }
            }
            _ => {}
        },
        _ => {}
    }
    out
}
