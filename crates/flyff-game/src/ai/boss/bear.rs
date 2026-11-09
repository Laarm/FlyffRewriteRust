//! Bear / Mushmoot (`MI_MUSHMOOT`, `CAIBear`; Plan/18 §10.2-§10.4, task boss-ai-b, WP-AI-20): the 30 s buff
//! timer, the opener with its two minions, the arrival shouts and summons and the four hit handlers
//! (`OnAttackMelee_Bear`, `MoverActEvent.cpp:1395-1539`). The bands are [`super::tables::choose_attack`]; the
//! `BuffTime` override of the choice is [`after_choose`].
//!
//! | swing | `OBJMSG` / item | reach | arrival | hit frames |
//! |---|---|---|---|---|
//! | Normal | `ATK1` or `ATK2` (drawn at the arrival) / `dwAtk1` | 7 m | - | ATK1: attack-spell group around the boss, then a single hit; ATK2: a single hit |
//! | Loot | `ATK3` / `dwAtk2` | 7 m (a second Loot in a row: `R`) | shout `MSG_02` around the boss | hit 0: knock-back, hit 1: stun group |
//! | BuffTime | `ATK4` / `dwAtk3` | 10 m | caption `MSG_03`, three minions around the target | EVERY hit: ten self buffs, then the boss grows |

use super::fx::{BossFx, HitEnv, HitFx, HitList, SkillHow, on_hit_with};
use super::{BossAi, BossAttack, Reach, tables};
use crate::ai::ctx::{AiCmd, AiCx};
use crate::ai::monster::helpers::target_view;
use crate::ai::rng::x_random_f;
use crate::ai::scan::{JOB_ALL, scan_target};
use crate::ai::spawn::MonsterSpawn;
use crate::combat::ActionCx;
use crate::mover::Mover;
use flyff_common::defs::obj::MI_SMALL_MUSHPOIE;
use flyff_common::defs::skill::{
    SI_ASS_CHEER_ACCURACY, SI_ASS_CHEER_BEEFUP, SI_ASS_CHEER_CANNONBALL, SI_ASS_CHEER_CATSREFLEX,
    SI_ASS_CHEER_HASTE, SI_ASS_CHEER_HEAPUP, SI_ASS_CHEER_MENTALSIGN, SI_ASS_HEAL_HEALING,
    SI_GEN_ATKSPELL_RATEGROUP, SI_GEN_KNOCK_BACK, SI_GEN_STUNGROUP, SI_MUSHMOOT_BERSERK,
    SI_MUSHMOOT_WOODENSKIN,
};
use flyff_common::defs::text::{
    TID_GAME_BOSS_MUSHMOOT_MSG_01, TID_GAME_BOSS_MUSHMOOT_MSG_02, TID_GAME_BOSS_MUSHMOOT_MSG_03,
};
use flyff_common::math::Vec3;
use flyff_common::rng::GameRng;
use flyff_common::{GameMs, ObjId};
use flyff_sim::num::angle_to_vector_xz;
use flyff_sim::types::{OBJSTA_ATK1, OBJSTA_ATK2, OBJSTA_ATK3, OBJSTA_ATK4};

/// The opener scans this far (`ScanTarget(pMover, 80, JOB_ALL)`).
pub const OPENER_RANGE: i32 = 80;
/// `m_cTimeBuff.Set(SEC(30))`: the interval of the `BuffTime` attack, ms.
pub const BUFF_INTERVAL_MS: u64 = 30_000;
/// `SetGold(GetLevel() * 15)` of a minion: the gold per level.
pub const MINION_GOLD_PER_LEVEL: u32 = 15;
/// The minions of the opener stand this far in front of and behind the first player, metres.
pub const OPENER_OFFSET: f32 = 2.0;
/// The minions of `BuffTime`.
pub const BUFF_MINIONS: usize = 3;
/// The walk of the minions of `BuffTime`: `xRandomF(9) - 4` on x then z, cumulative.
pub const BUFF_SCATTER: f32 = 9.0;
/// The ten self buffs of ATK4, in the order of the original (all level 1, `DoApplySkill(this, ..)`).
pub const SELF_BUFFS: [u32; 10] = [
    SI_ASS_HEAL_HEALING,
    SI_MUSHMOOT_BERSERK,
    SI_MUSHMOOT_WOODENSKIN,
    SI_ASS_CHEER_HEAPUP,
    SI_ASS_CHEER_CANNONBALL,
    SI_ASS_CHEER_MENTALSIGN,
    SI_ASS_CHEER_BEEFUP,
    SI_ASS_CHEER_CATSREFLEX,
    SI_ASS_CHEER_ACCURACY,
    SI_ASS_CHEER_HASTE,
];

/// The per-boss part of `InitAI` (`AIBear.cpp:94`): the 30 s buff timer starts BEFORE the switch to RAGE, so the
/// first choice is not a `BuffTime`.
pub fn init(b: &mut BossAi, now: GameMs) {
    b.buff_due = GameMs(now.0.saturating_add(BUFF_INTERVAL_MS));
}

/// The `BuffTime` override of the choice (after [`tables::choose_attack`] did not give up the target):
/// `CTimer::Over` (`now >= buff_due`) -> `BuffTime` at 10 m and the timer restarts; else `None` (the choice
/// stands).
pub fn after_choose(b: &mut BossAi, now: GameMs) -> Option<(BossAttack, Reach)> {
    if now >= b.buff_due {
        b.buff_due = GameMs(now.0.saturating_add(BUFF_INTERVAL_MS));
        Some(tables::bear_buff_time())
    } else {
        None
    }
}

/// `SummonMonster(MI_SMALL_MUSHPOIE, pos)`: an aggressive minion with `level * 15` gold in the layer of the boss
/// (Q17). A missing prop spawns nothing.
fn summon(cx: &mut AiCx<'_>, pos: Vec3) {
    let Some(level) = cx.data().mover(MI_SMALL_MUSHPOIE).map(|p| p.level) else {
        return;
    };
    cx.cmds.push(AiCmd::Spawn(MonsterSpawn {
        mover: MI_SMALL_MUSHPOIE,
        world: cx.world,
        layer: cx.layer,
        pos,
        active_attack: true,
        gold: i32::try_from(level.saturating_mul(MINION_GOLD_PER_LEVEL)).ok(),
        owner_ai: None,
    }));
}

/// The pre-step of `MoveProcessRage` (every RAGE `Process`, also during `Ready`): the opener. The first player
/// within 80 m gets the caption `MSG_01` (its world) and two minions 2 m in front of and 2 m behind it (along its
/// own facing, the height of the player). No draw is made.
pub fn pre_step(b: &mut BossAi, m: &mut Mover, cx: &mut AiCx<'_>) {
    if b.opener_done {
        return;
    }
    let Some(t) = scan_target(cx, m, OPENER_RANGE, JOB_ALL, 0, 0, 0) else {
        return;
    };
    b.opener_done = true;
    let Some(view) = target_view(cx, t) else {
        return;
    };
    let angle = cx.lookup.snapshot(t).map_or(0.0, |s| s.angle);
    cx.cmds.push(AiCmd::Boss(BossFx::Caption {
        world: cx.world,
        tid: TID_GAME_BOSS_MUSHMOOT_MSG_01,
        name_of: None,
    }));
    summon(cx, view.pos + angle_to_vector_xz(angle, OPENER_OFFSET));
    summon(cx, view.pos + angle_to_vector_xz(angle, -OPENER_OFFSET));
}

/// What the Bear does when its swing was accepted: Loot shouts `MSG_02` around the boss; BuffTime sends the
/// caption `MSG_03` (the boss's world) and, while the target resolves (FIX: the original dereferenced it), three
/// minions on a CUMULATIVE walk from the target (`p.x += x_random_f(9) - 4; p.z += x_random_f(9) - 4`, x drawn
/// first; Q21 KEEP).
pub fn arrival_extras(b: &BossAi, m: &Mover, cx: &mut AiCx<'_>, target: ObjId) {
    match b.attack {
        Some(BossAttack::Loot) => cx.cmds.push(AiCmd::Boss(BossFx::Shout {
            speaker: m.id(),
            world: cx.world,
            center: m.sim.pos,
            tid: TID_GAME_BOSS_MUSHMOOT_MSG_02,
            name_of: None,
        })),
        Some(BossAttack::BuffTime) => {
            cx.cmds.push(AiCmd::Boss(BossFx::Caption {
                world: cx.world,
                tid: TID_GAME_BOSS_MUSHMOOT_MSG_03,
                name_of: None,
            }));
            let Some(t) = target_view(cx, target) else {
                return;
            };
            let mut p = t.pos;
            for _ in 0..BUFF_MINIONS {
                p.x += x_random_f(cx.rng, BUFF_SCATTER) - 4.0;
                p.z += x_random_f(cx.rng, BUFF_SCATTER) - 4.0;
                summon(cx, p);
            }
        }
        _ => {}
    }
}

/// `OnAttackMelee_Bear` at a hit frame (the `m_nMotionHitCount++` is the dispatcher's).
pub fn on_hit(cx: &mut ActionCx<'_>, boss: ObjId, state: u32, target: ObjId) {
    on_hit_with(cx, boss, target, |hit, me, t, env, rng| {
        hit_effects(state, hit, me, t, env, rng)
    });
}

/// The effects of the hit `hit` (`m_nMotionHitCount`) of swing `state`, pure over its arguments.
///
/// * ATK1: the attack-spell group (level 5) around the boss, then a single hit on the hit object;
/// * ATK2: a single hit;
/// * ATK3: hit 0 -> the knock-back (level 4) around the boss, hit 1 -> the stun group (level 5), later hits nothing;
/// * ATK4 (EVERY hit, the original has no hit test): the ten [`SELF_BUFFS`] (`DoApplySkill(this, ..)`, caster and
///   target are the boss) and the growth ([`BossFx::BearGrow`]: `scale * 1.3` while the product stays below 10).
pub fn hit_effects(
    state: u32,
    hit: i32,
    _me: Vec3,
    target: Option<(ObjId, Vec3)>,
    env: &mut HitEnv<'_>,
    _rng: &mut dyn GameRng,
) -> HitList {
    let boss = env.boss;
    let around = |skill, level| {
        HitFx::Job(BossFx::SkillAround {
            caster: boss,
            skill,
            level,
            range: 0.0,
        })
    };
    let mut out = HitList::new();
    match state {
        OBJSTA_ATK1 => {
            out.push(around(SI_GEN_ATKSPELL_RATEGROUP, 5));
            if let Some((t, _)) = target {
                out.push(HitFx::Damage {
                    item: None,
                    target: t,
                });
            }
        }
        OBJSTA_ATK2 => {
            if let Some((t, _)) = target {
                out.push(HitFx::Damage {
                    item: None,
                    target: t,
                });
            }
        }
        OBJSTA_ATK3 => match hit {
            0 => out.push(around(SI_GEN_KNOCK_BACK, 4)),
            1 => out.push(around(SI_GEN_STUNGROUP, 5)),
            _ => {}
        },
        OBJSTA_ATK4 => {
            for skill in SELF_BUFFS {
                out.push(HitFx::Job(BossFx::Skill {
                    caster: boss,
                    target: boss,
                    skill,
                    level: 1,
                    how: SkillHow::Do,
                }));
            }
            out.push(HitFx::Job(BossFx::BearGrow { boss }));
        }
        _ => {}
    }
    out
}
