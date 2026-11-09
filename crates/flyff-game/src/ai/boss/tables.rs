//! The decision tables of the four boss AIs (Plan/18 §10.1, §10.2; see the module docs of [`super`]).
//!
//! Every function draws from the world stream in the order of the original code, so the draws of a boss tick
//! stay in step with the original's `xRand` order.

use super::{BossAttack, Reach};
use crate::ai::BossKind;
use crate::ai::rng::x_random;
use flyff_common::rng::GameRng;
use flyff_sim::types::{OBJMSG_ATK1, OBJMSG_ATK2, OBJMSG_ATK3, OBJMSG_ATK4};

/// `JOB_MERCENARY` bonus of the non-ClockWorks bosses' aggro roll: 70 instead of 50.
pub const AGGRO_MERCENARY: u32 = 70;

/// Which attack item of the prop a swing uses (`dwAtk1` .. `dwAtk4`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtkSlot {
    /// `dwAtk1`.
    Atk1,
    /// `dwAtk2`.
    Atk2,
    /// `dwAtk3`.
    Atk3,
    /// `dwAtk4`.
    Atk4,
}

impl AtkSlot {
    /// The item id the prop holds in this slot.
    pub fn item(self, atk: [u32; 4]) -> u32 {
        match self {
            AtkSlot::Atk1 => atk[0],
            AtkSlot::Atk2 => atk[1],
            AtkSlot::Atk3 => atk[2],
            AtkSlot::Atk4 => atk[3],
        }
    }
}

/// Whom `SelectTarget` aims at after the 0..99 roll (the boss has a last attacker).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetPick {
    /// The one who hit the boss last.
    LastAttacker,
    /// `ScanTargetStrong(50)`: the highest level within 50 m (the last attacker if there is none or it flies).
    Strongest,
    /// `ScanTargetOverHealer(50)`: the first player with an over-heal (the last attacker if none).
    OverHealer,
}

/// The roll of `SelectTarget` for a boss that has a last attacker: `r = x_random(100)`; the last attacker with
/// 50 % (70 % when the boss is not the ClockWorks and the attacker is a Mercenary), up to 74 the strongest
/// player, else the over-healer. One draw.
pub fn target_pick(
    kind: BossKind,
    last_attacker_is_mercenary: bool,
    rng: &mut dyn GameRng,
) -> TargetPick {
    let r = x_random(rng, 100);
    let aggro = if kind != BossKind::ClockWorks && last_attacker_is_mercenary {
        AGGRO_MERCENARY
    } else {
        50
    };
    if r < aggro {
        TargetPick::LastAttacker
    } else if r < 75 {
        TargetPick::Strongest
    } else {
        TargetPick::OverHealer
    }
}

/// The scan range of a boss without a last attacker (`ScanTarget(S, JOB_ALL)`): 80 m for the ClockWorks, 40 m
/// for the others (plan §10.1).
pub fn opening_scan_range(kind: BossKind) -> i32 {
    match kind {
        BossKind::ClockWorks => 80,
        _ => 40,
    }
}

/// The band table of `MoveProcessRage`: the attack and the arrival range for a target at squared distance `d2`
/// (3-D) from a boss whose `GetRadiusXZ()` squared is `r2`; `None` when the boss gives up the target. `prev` is
/// the attack of the previous choice (the Bear does not restart a `Loot`, Q21).
///
/// | boss | bands (`d2` against `r2`) |
/// |---|---|
/// | ClockWorks | `< 1.5` Quake, 0 m; `< 3` Slash or Laser (50 %), `R`; `< 6` Shoot, 30 m; else give up |
/// | BigMuscle | `< 4` 85 %: Normal or Normal2, 5 m, else QuakeDouble, `R`; `< 6` 60 % QuakeOne, 15 m, else give up; else Normal, 10 m |
/// | Krrr | `< 32` roll <= 9 Summons 0 m, <= 30 DoubleCrash 10 m, else Normal 7 m; else Summons, 0 m |
/// | Bear | `< 32` roll > 85 Loot 7 m (a repeated Loot keeps `R`), else Normal 7 m; else give up |
pub fn choose_attack(
    kind: BossKind,
    d2: f32,
    r2: f32,
    prev: Option<BossAttack>,
    rng: &mut dyn GameRng,
) -> Option<(BossAttack, Reach)> {
    match kind {
        BossKind::ClockWorks => {
            if d2 < r2 * 1.5 {
                Some((BossAttack::Quake, Reach::Metres(0.0)))
            } else if d2 < r2 * 3.0 {
                let a = if x_random(rng, 100) < 50 {
                    BossAttack::Slash
                } else {
                    BossAttack::Laser
                };
                Some((a, Reach::Radius))
            } else if d2 < r2 * 6.0 {
                Some((BossAttack::Shoot, Reach::Metres(30.0)))
            } else {
                None
            }
        }
        BossKind::BigMuscle => {
            if d2 < r2 * 4.0 {
                if x_random(rng, 100) < 85 {
                    let a = if x_random(rng, 2) != 0 {
                        BossAttack::Normal
                    } else {
                        BossAttack::Normal2
                    };
                    Some((a, Reach::Metres(5.0)))
                } else {
                    Some((BossAttack::QuakeDouble, Reach::Radius))
                }
            } else if d2 < r2 * 6.0 {
                if x_random(rng, 100) < 60 {
                    Some((BossAttack::QuakeOne, Reach::Metres(15.0)))
                } else {
                    None
                }
            } else {
                Some((BossAttack::Normal, Reach::Metres(10.0)))
            }
        }
        BossKind::Krrr => {
            if d2 < r2 * 32.0 {
                let r = x_random(rng, 100);
                Some(if r <= 9 {
                    (BossAttack::Summons, Reach::Metres(0.0))
                } else if r <= 30 {
                    (BossAttack::DoubleCrash, Reach::Metres(10.0))
                } else {
                    (BossAttack::Normal, Reach::Metres(7.0))
                })
            } else {
                Some((BossAttack::Summons, Reach::Metres(0.0)))
            }
        }
        BossKind::Bear => {
            if d2 < r2 * 32.0 {
                if x_random(rng, 100) > 85 {
                    // a second Loot in a row keeps the type and the default reach (the guard is on `m_nAttackType`)
                    if prev == Some(BossAttack::Loot) {
                        Some((BossAttack::Loot, Reach::Radius))
                    } else {
                        Some((BossAttack::Loot, Reach::Metres(7.0)))
                    }
                } else {
                    Some((BossAttack::Normal, Reach::Metres(7.0)))
                }
            } else {
                None
            }
        }
    }
}

/// The Bear's `BuffTime` override of the choice (its 30 s `CTimer` ran out): 10 m. Applied after
/// [`choose_attack`] and only when that did not give up.
pub fn bear_buff_time() -> (BossAttack, Reach) {
    (BossAttack::BuffTime, Reach::Metres(10.0))
}

/// What a boss does when it arrives (`AIMSG_ARRIVAL`): the `OBJMSG` and the attack item slot. `Normal` of Krrr and
/// Bear draws `x_random(2)` **here** (ATK1 or ATK2, both with `dwAtk1`). `None` for an attack the boss does not
/// have.
pub fn arrival(
    kind: BossKind,
    attack: BossAttack,
    rng: &mut dyn GameRng,
) -> Option<(u32, AtkSlot)> {
    use AtkSlot::*;
    use BossAttack::*;
    Some(match (kind, attack) {
        (BossKind::ClockWorks, Slash) => (OBJMSG_ATK1, Atk1),
        (BossKind::ClockWorks, Laser) => (OBJMSG_ATK3, Atk2),
        (BossKind::ClockWorks, Shoot) => (OBJMSG_ATK2, Atk3),
        (BossKind::ClockWorks, Quake) => (OBJMSG_ATK4, Atk4),
        (BossKind::BigMuscle, Normal) => (OBJMSG_ATK1, Atk1),
        (BossKind::BigMuscle, Normal2) => (OBJMSG_ATK2, Atk1),
        (BossKind::BigMuscle, QuakeDouble) => (OBJMSG_ATK3, Atk3),
        (BossKind::BigMuscle, QuakeOne) => (OBJMSG_ATK4, Atk2),
        (BossKind::Krrr, Normal) | (BossKind::Bear, Normal) => {
            let msg = if x_random(rng, 2) != 0 {
                OBJMSG_ATK1
            } else {
                OBJMSG_ATK2
            };
            (msg, Atk1)
        }
        (BossKind::Krrr, DoubleCrash) => (OBJMSG_ATK3, Atk2),
        (BossKind::Krrr, Summons) => (OBJMSG_ATK4, Atk3),
        (BossKind::Bear, Loot) => (OBJMSG_ATK3, Atk2),
        (BossKind::Bear, BuffTime) => (OBJMSG_ATK4, Atk3),
        _ => return None,
    })
}
