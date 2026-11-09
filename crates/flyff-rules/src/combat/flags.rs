//! Attack flags, enums and the attack-range table (Plan/17 §2.1, §2.3, §2.6; WP-COMBAT-01).
//!
//! `AF_*` live in the original `ActionMover.h` (not in `Resource/`), the enums in `Mover.h` /
//! `DefineCommon.h`; the `CHS_*` bits, ranks and `AR_*` ids come from [`crate::consts`] (generated defines).

use crate::consts::dst::{
    AR_FAR, AR_HRANGE, AR_HWAND, AR_LONG, AR_RANGE, AR_SHORT, AR_WAND, RANK_BOSS, RANK_GUARD,
    RANK_MATERIAL, RANK_MIDBOSS, RANK_SUPER,
};
use core::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, Not};

/// `dwAtkFlags`: the `AF_*` bit set of an attack (`src:_AIInterface/ActionMover.h:27-44`). The same word is the
/// `atk_flags` field of the `Damage` message.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct AtkFlags(pub u32);

impl AtkFlags {
    /// No flag.
    pub const NONE: AtkFlags = AtkFlags(0);
    /// `AF_GENERIC`: autoattack (melee, bow, monster).
    pub const GENERIC: AtkFlags = AtkFlags(0x0000_0001);
    /// `AF_MISS`.
    pub const MISS: AtkFlags = AtkFlags(0x0000_0002);
    /// `AF_MAGIC`: wand autoattack.
    pub const MAGIC: AtkFlags = AtkFlags(0x0000_0008);
    /// `AF_MELEESKILL` (`KT_SKILL` skill damage).
    pub const MELEESKILL: AtkFlags = AtkFlags(0x0000_0010);
    /// `AF_MAGICSKILL` (`KT_MAGIC` skill damage).
    pub const MAGICSKILL: AtkFlags = AtkFlags(0x0000_0020);
    /// `AF_CRITICAL1` (2.3x).
    pub const CRITICAL1: AtkFlags = AtkFlags(0x0000_0040);
    /// `AF_CRITICAL2` (2.6x).
    pub const CRITICAL2: AtkFlags = AtkFlags(0x0000_0080);
    /// `AF_CRITICAL`: both critical bits, set together.
    pub const CRITICAL: AtkFlags = AtkFlags(0x0000_00C0);
    /// `AF_PUSH` (never set by the server logic).
    pub const PUSH: AtkFlags = AtkFlags(0x0000_0100);
    /// `AF_PARRY` (never set by the server logic; the client renders it).
    pub const PARRY: AtkFlags = AtkFlags(0x0000_0200);
    /// `AF_RESIST` (never set by the server logic).
    pub const RESIST: AtkFlags = AtkFlags(0x0000_0400);
    /// `AF_STUN` (never set by the server logic).
    pub const STUN: AtkFlags = AtkFlags(0x0000_0800);
    /// `AF_BLOCKING`: the hit was blocked.
    pub const BLOCKING: AtkFlags = AtkFlags(0x0000_1000);
    /// `AF_FORCE`: precomputed damage (reflect, DoT, burn, steal-HP-imm, Psychic Wall PvP).
    pub const FORCE: AtkFlags = AtkFlags(0x0000_2000);
    /// `AF_RANGE`: player bow attack (charge multiplier).
    pub const RANGE: AtkFlags = AtkFlags(0x0000_4000);
    /// `AF_MONSTER_SP_CLIENT`: client only.
    pub const MONSTER_SP_CLIENT: AtkFlags = AtkFlags(0x0000_8000);
    /// `AF_FLYING`: knock-up; `Damage` carries `defender_pos` / `defender_angle`.
    pub const FLYING: AtkFlags = AtkFlags(0x1000_0000);

    /// The raw word.
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// `flags & other != 0`: at least one common bit (the C `if (dwAtkFlags & AF_X)`).
    pub const fn intersects(self, other: AtkFlags) -> bool {
        self.0 & other.0 != 0
    }

    /// Every bit of `other` is set.
    pub const fn contains(self, other: AtkFlags) -> bool {
        self.0 & other.0 == other.0
    }

    /// `IsSkillAttack(f)`: `f & (AF_MELEESKILL | AF_MAGICSKILL)`.
    pub const fn is_skill_attack(self) -> bool {
        self.0 & (Self::MELEESKILL.0 | Self::MAGICSKILL.0) != 0
    }

    /// `ATTACK_INFO::GetAtkType`, tested in the original order: melee skill, magic skill, wand, force, else generic.
    pub const fn atk_type(self) -> AtkType {
        if self.0 & Self::MELEESKILL.0 != 0 {
            AtkType::MeleeSkill
        } else if self.0 & Self::MAGICSKILL.0 != 0 {
            AtkType::MagicSkill
        } else if self.0 & Self::MAGIC.0 != 0 {
            AtkType::Magic
        } else if self.0 & Self::FORCE.0 != 0 {
            AtkType::Force
        } else {
            AtkType::Generic
        }
    }
}

impl BitOr for AtkFlags {
    type Output = AtkFlags;
    fn bitor(self, rhs: AtkFlags) -> AtkFlags {
        AtkFlags(self.0 | rhs.0)
    }
}

impl BitOrAssign for AtkFlags {
    fn bitor_assign(&mut self, rhs: AtkFlags) {
        self.0 |= rhs.0;
    }
}

impl BitAnd for AtkFlags {
    type Output = AtkFlags;
    fn bitand(self, rhs: AtkFlags) -> AtkFlags {
        AtkFlags(self.0 & rhs.0)
    }
}

impl BitAndAssign for AtkFlags {
    fn bitand_assign(&mut self, rhs: AtkFlags) {
        self.0 &= rhs.0;
    }
}

impl Not for AtkFlags {
    type Output = AtkFlags;
    fn not(self) -> AtkFlags {
        AtkFlags(!self.0)
    }
}

impl From<u32> for AtkFlags {
    fn from(v: u32) -> AtkFlags {
        AtkFlags(v)
    }
}

impl From<AtkFlags> for u32 {
    fn from(v: AtkFlags) -> u32 {
        v.0
    }
}

/// `ATK_TYPE` (`DefineCommon.h:30`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum AtkType {
    /// `ATK_FORCE`: precomputed damage.
    Force = 0,
    /// `ATK_MELEESKILL`.
    MeleeSkill = 1,
    /// `ATK_MAGICSKILL`.
    MagicSkill = 2,
    /// `ATK_MAGIC`: wand autoattack.
    Magic = 3,
    /// `ATK_GENERIC`.
    Generic = 4,
}

/// `HITTYPE` (`Mover.h:320`): whether and how a hit is allowed (Plan/17 §8.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum HitType {
    /// `HITTYPE_FAIL`: the hit is ignored.
    Fail = 0,
    /// `HITTYPE_GENERIC`: PvE.
    Generic = 1,
    /// `HITTYPE_PVP`: duel.
    Pvp = 2,
    /// `HITTYPE_WAR`: guild war.
    War = 3,
    /// `HITTYPE_SCHOOL`: school event.
    School = 4,
    /// `HITTYPE_PK`.
    Pk = 5,
    /// `HITTYPE_GUILDCOMBAT`.
    GuildCombat = 6,
    /// `HITTYPE_ARENA`.
    Arena = 7,
}

/// The hand a hit comes from (`ATTACK_INFO::nParts`: `PARTS_RWEAPON` / `PARTS_LWEAPON`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Hand {
    /// `PARTS_RWEAPON`.
    #[default]
    Right,
    /// `PARTS_LWEAPON` (off hand: x0.75 damage).
    Left,
}

/// `JOB_PROP_TYPE` of `GetJobPropFactor`: the `propJob.inc` columns the damage formulas read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobFactor {
    /// `fMeleeSWD`.
    Swd,
    /// `fMeleeAXE`.
    Axe,
    /// `fMeleeSTAFF`.
    Staff,
    /// `fMeleeSTICK`.
    Stick,
    /// `fMeleeKNUCKLE`.
    Knuckle,
    /// `fMagicWAND`.
    Wand,
    /// `fMeleeYOYO`.
    Yoyo,
    /// `fMeleeBOW` (`__BOWDMG_PROPJOB`).
    Bow,
    /// `fBlocking`.
    Blocking,
    /// `fCritical`.
    Critical,
}

/// `OBJMSG_*` ids that travel on the wire in the attack messages (ref/12 §4.4, Plan/17 §2.3).
pub mod objmsg {
    /// `OBJMSG_ATK1`.
    pub const ATK1: u32 = 29;
    /// `OBJMSG_ATK2`.
    pub const ATK2: u32 = 30;
    /// `OBJMSG_ATK3`.
    pub const ATK3: u32 = 31;
    /// `OBJMSG_ATK4`.
    pub const ATK4: u32 = 32;
    /// `OBJMSG_SP_ATK1`.
    pub const SP_ATK1: u32 = 33;
    /// `OBJMSG_ATK_RANGE1`.
    pub const ATK_RANGE1: u32 = 35;
    /// `OBJMSG_ATK_MAGIC1`.
    pub const ATK_MAGIC1: u32 = 36;
    /// `OBJMSG_MELEESKILL`.
    pub const MELEESKILL: u32 = 37;
    /// `OBJMSG_MAGICSKILL`.
    pub const MAGICSKILL: u32 = 38;
    /// `OBJMSG_MAGICCASTING`.
    pub const MAGICCASTING: u32 = 39;
    /// `OBJMSG_DIE`.
    pub const DIE: u32 = 40;
    /// `OBJMSG_DAMAGE`.
    pub const DAMAGE: u32 = 41;
    /// `OBJMSG_DAMAGE_FORCE`.
    pub const DAMAGE_FORCE: u32 = 42;
}

/// `MAX_CHARGE_LEVEL` (`DefineCommon.h:25`): the highest wand / bow charge.
pub const MAX_CHARGE_LEVEL: i32 = 4;
/// `MAX_ACTMSG`: capacity of the melee and magic attack queues (`Action.h`).
pub const MAX_ACTMSG: usize = 255;
/// `MAX_SKILLBUFF_COUNT`.
pub const MAX_SKILLBUFF_COUNT: usize = 28;
/// `MAX_SLOT_QUEUE`: the action-slot (skill queue) length.
pub const MAX_SLOT_QUEUE: usize = 5;
/// Server safety timeout of a projectile in 60 Hz frames (Plan/17 §4.1).
pub const PROJECTILE_TIMEOUT_FRAMES: u32 = 600;
/// `NEXT_TICK_DUEL`: duel countdown step in ms (`Mover.h:119`).
pub const NEXT_TICK_DUEL_MS: u32 = 1_000;
/// `NEXT_TICK_ENDDUEL`: duel timeout refresh in ms (`Mover.h:120`).
pub const NEXT_TICK_ENDDUEL_MS: u32 = 180_000;
/// Post-revival invulnerability in ticks (`DPSrvr.cpp:1013`): 5 s.
pub const REVIVE_INVULNERABLE_TICKS: i32 = 75;
/// Post-duel invulnerability in ticks (`MoverParam.cpp:801`): 2 s.
pub const DUEL_END_INVULNERABLE_TICKS: i32 = 30;
/// The "use all MP/FP" sentinel of the skill costs (`MoverAttack.cpp:2318-2332`).
pub const REQ_ALL: i32 = 9_999_999;

/// `CMover::GetAttackRange(ar)` (`MoverMsg.cpp:140-166`): the range in metres of an `AR_*` id (2, 3, 4, 10, 15, 6,
/// 18; unknown ids 0), times `(100 + hawkeye_rate) / 100` for a positive `DST_HAWKEYE_RATE`.
pub fn attack_range(ar: u32, hawkeye_rate: i32) -> f32 {
    let mut range = match ar {
        AR_SHORT => 2.0_f32,
        AR_LONG => 3.0_f32,
        AR_FAR => 4.0_f32,
        AR_RANGE => 10.0_f32,
        AR_WAND => 15.0_f32,
        AR_HRANGE => 6.0_f32,
        AR_HWAND => 18.0_f32,
        _ => 0.0_f32,
    };
    if hawkeye_rate > 0 {
        range = range * hawkeye_rate.wrapping_add(100) as f32 / 100.0_f32;
    }
    range
}

/// KQ-05 (`DoAttackMagic`, `MoverSkill.cpp:3224-3302`): a wand shot pays `GetReqMp(weapon.dwReqMp)` **only at charge 3**
/// ("the 4th stage costs 5 MP", the comment of the original) - not at 0, 1, 2 or 4.
pub const fn wand_pays_mp(charge: u32) -> bool {
    charge == 3
}

/// Ranks that can never be knocked up (`CanFlyByAttack`): `SUPER`, `BOSS`, `MATERIAL`, `MIDBOSS`.
pub const fn rank_resists_knock_up(rank: u32) -> bool {
    matches!(rank, RANK_SUPER | RANK_BOSS | RANK_MATERIAL | RANK_MIDBOSS)
}

/// `RANK_GUARD` / `RANK_SUPER`: the ranks that skip the level-gap penalty when they attack
/// (`GetDamageMultiplier`).
pub const fn rank_skips_level_gap(rank: u32) -> bool {
    matches!(rank, RANK_GUARD | RANK_SUPER)
}
