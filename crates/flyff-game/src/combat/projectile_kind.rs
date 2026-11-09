//! The per-kind gameplay parameters of the server projectiles and the SFX-to-kind selection (Plan/17 §4.2,
//! §4.3; WP-COMBAT-07).
//!
//! **Data only.** The step function and its native / wasm32 determinism are `flyff_sim::projectile` (WP-SIM-22);
//! this table holds what the *game* feeds it: where a shot starts relative to the attacker, the speed pair of
//! `features.faster_projectiles`, the frames between contact and damage, the safety timeout and what happens when
//! the target vanishes. Every row cites its `src:_Common/Sfx.cpp` lines. [`PROJECTILE_KIND_PARAMS`] is indexed by
//! `ProjectileKind::as_u8()` (the wire number; row 0 is `PROJECTILE_KIND_UNSPECIFIED`). The cross-check test
//! compares every row with the sim's own table.
//!
//! **Deviation from the plan table.** Plan/17 §4.3 gives the origin of the generic shot, the wand bolt, the
//! arrows and the wave as `pos + (0, 1.01, 0)`. The source adds exactly `1.0` (`CreateObj.cpp:562`,
//! `vPosShoot.y += 1.0f`, and `Mover.cpp:8796` hands the hand position of the shooter to the same function); the
//! sim's arrival golden (`golden/projectile_arrival.txt`) uses `(0, 1, 0)` too. The source value is used; the
//! `0.01` has no origin in the code.
//!
//! The two selections: [`kind_for_sfx`] is the merged table of the plan (`CreateShootSfx` plus the hitting classes
//! of `CreateSfx`; unknown ids are `SHOOT`), [`kind_for_monster_range`] is what a monster's ranged attack goes
//! through in the original (`CreateSfx`, whose `default` is the base `CSfx` that never reports a hit: KQ-35).

use flyff_common::defs::obj::*;
use flyff_rules::consts::NULL_ID;
use flyff_sim::projectile::ProjectileKind;

/// A 3-vector offset (`x, y, z`), kept free of the sim's math types so the table is plain data.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Offset {
    /// X.
    pub x: f32,
    /// Y.
    pub y: f32,
    /// Z.
    pub z: f32,
}

const fn off(x: f32, y: f32, z: f32) -> Offset {
    Offset { x, y, z }
}

/// What a class does when its target is gone (`prj.GetCtrl(m_idDest)` invalid) while it flies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetLostRule {
    /// The class deletes itself: the projectile expires at once (`ProjectileHit{expired}`, no damage).
    Delete,
    /// The class flies on to the last destination and expires at the contact (no damage).
    FlyOn,
}

/// One row of [`PROJECTILE_KIND_PARAMS`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProjectileKindParams {
    /// Where the projectile starts, relative to the attacker's position at the hit frame.
    pub origin_offset: Offset,
    /// Speed per 60 Hz frame with `features.faster_projectiles`.
    pub speed_fast: f32,
    /// Speed per frame without it (`WAND_ATK_AIR`, the arrows: the same value).
    pub speed_slow: f32,
    /// Frames between the contact and the damage (the legacy classes that apply it when their hit-SFX is at
    /// frame 0 apply it on the next frame).
    pub damage_delay_frames: u8,
    /// The safety timeout in frames (`0` = no flight).
    pub timeout_frames: u16,
    /// The vanish rule.
    pub on_target_lost: TargetLostRule,
}

const ZERO_ROW: ProjectileKindParams = ProjectileKindParams {
    origin_offset: off(0.0, 0.0, 0.0),
    speed_fast: 0.0,
    speed_slow: 0.0,
    damage_delay_frames: 0,
    timeout_frames: 0,
    on_target_lost: TargetLostRule::Delete,
};

/// The table, indexed by the wire number of the kind (`ProjectileKind::as_u8()`).
///
/// | # | kind | class and lines (`Sfx.cpp`) |
/// |---|---|---|
/// | 1 | `SHOOT` | `CSfxShoot` 3509-3570: homing at the target `+ (0,1,0)`, speed 0.4 / 0.2, contact `< 0.2` |
/// | 2 | `WAND_ATK1` | `CSfxItemWandAtk1` 848-940: homing at the box centre, speed 0.4 / 0.2, damage on the frame after the contact, deleted when the target is gone or dead |
/// | 3 | `WAND_ATK_AIR` | `CSfxItemWandAtkAir` 617-698: inertial, impulse 0.75, 480 frames, flies on to the last destination |
/// | 4 | `RANGE_ATK1` | `CSfxItemRangeAtk1` 968-1070: speed 0.7, contact `< 0.4`, deleted when the target is gone |
/// | 5 | `RANGE_ATK_JUNKBOW` | `CSfxItemRangeAtk_JunkBow` 1362-1420: as the arrow |
/// | 6 | `MAG_STRONG_WIND` | `CSfxSkillMagStrongWind` 2474-2543: starts at the caster's position, speed 0.4 / 0.2, damage on the frame after the contact |
/// | 7 | `MAG_SWORD_WIND` | `CSfxSkillMagSwordWind` 2566-2620: as the strong wind, contact `< 0.4` |
/// | 8-11 | the four missiles | `CSfxSkillMagFireBoomerang` 2632-2700, `...IceMissile` 3150-3236, `...LightningBall` 3238-3323, `...SpikeStone` 3325-3420: start at the caster `+ (0,1,0)`, aim `+0.3 y` |
/// | 12 | `SHOOT_WAVE` | `CSfxShootWave` 3625-3700: speed 0.45 / 0.25, at the target's height |
/// | 13, 14 | `YOYO_ATK`, `ATK_STRAIGHT` | client visuals; the server never spawns them |
/// | 15 | `MAG_HOT_AIR` | `CSfxSkillMagHotAir` 2749: attached to the attacker, no flight |
pub const PROJECTILE_KIND_PARAMS: [ProjectileKindParams; 16] = [
    // 0 PROJECTILE_KIND_UNSPECIFIED
    ZERO_ROW,
    // 1 SHOOT: `CreateObj.cpp:562` adds 1.0 to the shooter position
    ProjectileKindParams {
        origin_offset: off(0.0, 1.0, 0.0),
        speed_fast: 0.4,
        speed_slow: 0.2,
        damage_delay_frames: 0,
        timeout_frames: 600,
        on_target_lost: TargetLostRule::FlyOn,
    },
    // 2 WAND_ATK1
    ProjectileKindParams {
        origin_offset: off(0.0, 1.0, 0.0),
        speed_fast: 0.4,
        speed_slow: 0.2,
        damage_delay_frames: 1,
        timeout_frames: 600,
        on_target_lost: TargetLostRule::Delete,
    },
    // 3 WAND_ATK_AIR: `ShootSfx(angle, -angleX, 0.75)` from the shooter + (0,1,0)
    ProjectileKindParams {
        origin_offset: off(0.0, 1.0, 0.0),
        speed_fast: 0.75,
        speed_slow: 0.75,
        damage_delay_frames: 0,
        timeout_frames: 480,
        on_target_lost: TargetLostRule::FlyOn,
    },
    // 4 RANGE_ATK1
    ProjectileKindParams {
        origin_offset: off(0.0, 1.0, 0.0),
        speed_fast: 0.7,
        speed_slow: 0.7,
        damage_delay_frames: 0,
        timeout_frames: 600,
        on_target_lost: TargetLostRule::Delete,
    },
    // 5 RANGE_ATK_JUNKBOW
    ProjectileKindParams {
        origin_offset: off(0.0, 1.0, 0.0),
        speed_fast: 0.7,
        speed_slow: 0.7,
        damage_delay_frames: 0,
        timeout_frames: 600,
        on_target_lost: TargetLostRule::Delete,
    },
    // 6 MAG_STRONG_WIND: `m_pSfxObj->m_vPos = pObjSrc->GetPos()` at frame 0
    ProjectileKindParams {
        origin_offset: off(0.0, 0.0, 0.0),
        speed_fast: 0.4,
        speed_slow: 0.2,
        damage_delay_frames: 1,
        timeout_frames: 600,
        on_target_lost: TargetLostRule::FlyOn,
    },
    // 7 MAG_SWORD_WIND
    ProjectileKindParams {
        origin_offset: off(0.0, 0.0, 0.0),
        speed_fast: 0.4,
        speed_slow: 0.2,
        damage_delay_frames: 1,
        timeout_frames: 600,
        on_target_lost: TargetLostRule::FlyOn,
    },
    // 8 MAG_FIRE_BOOMERANG: `pObjSrc->GetPos() + (0, 1, 0)` at frame 0 (Sfx.cpp:2645)
    ProjectileKindParams {
        origin_offset: off(0.0, 1.0, 0.0),
        speed_fast: 0.4,
        speed_slow: 0.2,
        damage_delay_frames: 1,
        timeout_frames: 600,
        on_target_lost: TargetLostRule::FlyOn,
    },
    // 9 MAG_ICE_MISSILE
    ProjectileKindParams {
        origin_offset: off(0.0, 1.0, 0.0),
        speed_fast: 0.4,
        speed_slow: 0.2,
        damage_delay_frames: 1,
        timeout_frames: 600,
        on_target_lost: TargetLostRule::FlyOn,
    },
    // 10 MAG_LIGHTNING_BALL
    ProjectileKindParams {
        origin_offset: off(0.0, 1.0, 0.0),
        speed_fast: 0.4,
        speed_slow: 0.2,
        damage_delay_frames: 1,
        timeout_frames: 600,
        on_target_lost: TargetLostRule::FlyOn,
    },
    // 11 MAG_SPIKE_STONE
    ProjectileKindParams {
        origin_offset: off(0.0, 1.0, 0.0),
        speed_fast: 0.4,
        speed_slow: 0.2,
        damage_delay_frames: 1,
        timeout_frames: 600,
        on_target_lost: TargetLostRule::FlyOn,
    },
    // 12 SHOOT_WAVE: "target gone: fly on to the last coordinate" (Sfx.cpp:3632-3635)
    ProjectileKindParams {
        origin_offset: off(0.0, 1.0, 0.0),
        speed_fast: 0.45,
        speed_slow: 0.25,
        damage_delay_frames: 0,
        timeout_frames: 600,
        on_target_lost: TargetLostRule::FlyOn,
    },
    // 13 YOYO_ATK: client visual (`CreateSfxYoYo`), never spawned by the server
    ProjectileKindParams {
        origin_offset: off(0.0, 1.0, 0.0),
        speed_fast: 0.3,
        speed_slow: 0.3,
        damage_delay_frames: 0,
        timeout_frames: 300,
        on_target_lost: TargetLostRule::Delete,
    },
    // 14 ATK_STRAIGHT: client visual (`XI_NPCSP1RANBALL`), never spawned by the server
    ProjectileKindParams {
        origin_offset: off(0.0, 1.0, 0.0),
        speed_fast: 2.0,
        speed_slow: 2.0,
        damage_delay_frames: 0,
        timeout_frames: 180,
        on_target_lost: TargetLostRule::FlyOn,
    },
    // 15 MAG_HOT_AIR: attached to the attacker; the first and only damage is at the hit frame
    ZERO_ROW,
];

/// The row of `kind`.
pub fn params(kind: ProjectileKind) -> &'static ProjectileKindParams {
    PROJECTILE_KIND_PARAMS
        .get(usize::from(kind.as_u8()))
        .unwrap_or(&PROJECTILE_KIND_PARAMS[0])
}

/// The per-frame speed of `kind` for the `features.faster_projectiles` switch.
pub fn speed(kind: ProjectileKind, faster: bool) -> f32 {
    let p = params(kind);
    if faster { p.speed_fast } else { p.speed_slow }
}

/// `true` for the SFX ids the original's generic `CreateSfx` turns into a class that reports a hit: the wand
/// bolts, the arrows and the six ranged-monster ids.
fn generic_hitting_class(sfx: u32) -> Option<ProjectileKind> {
    use ProjectileKind::*;
    Some(match sfx {
        XI_FIR_WAND01 | XI_ITEM_WAND_ATK1 | XI_ITEM_WAND_ATK2 | XI_ITEM_WAND_ATK3
        | XI_ITEM_WAND_ATK4 => WandAtk1,
        XI_FIR_WAND02 => WandAtkAir,
        XI_ITEM_RANGE_ATK1 | XI_ITEM_RANGE_ATK2 | XI_ITEM_RANGE_ATK3 | XI_ITEM_RANGE_ATK4 => {
            RangeAtk1
        }
        // `CreateObj.cpp:186-191`
        XI_NPC_RAN_SPITTL | XI_NPC_RAN_GAS | XI_NPC_RAN_MAGICBALL | XI_NPC_RAN_MAGICBLUE
        | XI_NPC_RAN_CARD | XI_NPC_RAN_CANNON => WandAtk1,
        _ => return None,
    })
}

/// The ids that name a visual-only or non-hitting effect: they are never a server projectile.
fn is_effect_only(sfx: u32) -> bool {
    matches!(
        sfx,
        XI_NPC_RAN_BOOM | XI_NPC_RAN_FOG | XI_NPCSP1RANBALL | XI_NPCSP1RANBOOM | XI_NPCSP1RANSPARK
    )
}

/// The projectile class of the SFX `sfx` a skill or an attack item names (Plan/17 §4.2):
///
/// * `0` and `NULL_ID` (no effect), the effect-only ids (`XI_NPC_RAN_BOOM`, `XI_NPC_RAN_FOG`, the ClockWorks
///   `XI_NPCSP1RAN*`): `None`;
/// * the wand ids → `WAND_ATK1` / `WAND_ATK_AIR`; the arrow ids → `RANGE_ATK1` (`CreateObj.cpp:513-529`, where the
///   bow skills fall through to the plain arrow), the two bow skills with their own class → `RANGE_ATK_JUNKBOW`;
/// * the magician missiles and winds, the wave and the hot air by their id;
/// * anything else → `SHOOT` (the `default` of `CreateShootSfx`).
///
/// `XI_ITEM_WAND_ATK2..4` are not referenced by the data; the original makes them other wand classes, the
/// rewrite treats them as `WAND_ATK1` (Plan/17 §4.2).
pub fn kind_for_sfx(sfx: u32) -> Option<ProjectileKind> {
    use ProjectileKind::*;
    if sfx == 0 || sfx == NULL_ID || is_effect_only(sfx) {
        return None;
    }
    if let Some(k) = generic_hitting_class(sfx) {
        return Some(k);
    }
    Some(match sfx {
        XI_SKILL_RAG_BOW_TRIPLESHOT02 | XI_SKILL_ACR_BOW_JUNKBOW02 => RangeAtkJunkbow,
        XI_SKILL_RAG_BOW_ICEARROW02
        | XI_SKILL_RAG_BOW_FLAMEARROW02
        | XI_SKILL_RAG_BOW_PIRCINGARROW02
        | XI_SKILL_RAG_BOW_POISONARROW02
        | XI_SKILL_RAG_BOW_SILENTARROW02
        | XI_SKILL_ACR_BOW_AIMEDSHOT02
        | XI_SKILL_ACR_BOW_SILENTSHOT02
        | XI_SKILL_ACR_BOW_ARROWRAIN02
        | XI_SKILL_ACR_BOW_AUTOSHOT02 => RangeAtk1,
        XI_SKILL_MAG_WIND_SWORDWIND01 => MagSwordWind,
        XI_SKILL_MAG_WIND_STRONGWIND01 => MagStrongWind,
        XI_SKILL_MAG_FIRE_BOOMERANG01 => MagFireBoomerang,
        XI_SKILL_MAG_FIRE_HOTAIR01 => MagHotAir,
        XI_SKILL_MAG_WATER_ICEMISSILE01 => MagIceMissile,
        XI_SKILL_MAG_ELECTRICITY_LIGHTINGBALL01 => MagLightningBall,
        XI_SKILL_MAG_EARTH_SPIKESTONE01 => MagSpikeStone,
        XI_SKILL_MER_ONE_GUILOTIN02 => ShootWave,
        _ => Shoot,
    })
}

/// The class of a **monster's** ranged attack with attack item SFX `sfx` (`OnAttackRange`, an NPC shooter:
/// `CreateSfx`): only the ids of the generic switch hit, every other id is a plain effect that never deals damage
/// (KQ-35, `XI_NPC_RAN_BOOM`: 38 items).
pub fn kind_for_monster_range(sfx: u32) -> Option<ProjectileKind> {
    generic_hitting_class(sfx)
}
