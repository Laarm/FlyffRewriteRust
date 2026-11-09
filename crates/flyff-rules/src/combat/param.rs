//! `DamageParam`: the packed `nParam` of a damage message (`MakeParamDamage`, Plan/17 §2.2; WP-COMBAT-01).
//!
//! ```text
//! bits 0-7   magic power (wand / bow charge 0..4) or skill level
//! bits 8-15  damage count (0 = a normal hit, >= 1 = a repeated tick: ATK x 0.1)
//! bits 16-31 skill id (skill attacks) or attack item id (NPC special attacks: `item << 16`)
//! ```
//! Skill attacks are encoded `(skill << 16) | level` (`Ctrl.cpp:1022`). The legacy value is a signed `int`, so a
//! right shift of the whole word is arithmetic ([`DamageParam::item_id`] keeps that).

use super::flags::{AtkFlags, Hand};
use crate::consts::NULL_ID;
use flyff_common::defs::skill::{SI_BIL_PST_ASALRAALAIKUM, SI_JST_YOYO_HITOFPENYA};

/// The packed damage parameter (see the module documentation).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DamageParam(pub u32);

impl DamageParam {
    /// `CActionMover::MakeParamDamage(nMagicPower, nSkill, nDmgCnt)`: `power | cnt << 8 | skill << 16`.
    pub const fn make(magic_power: u32, skill: u32, dmg_cnt: u32) -> DamageParam {
        DamageParam(magic_power | (dmg_cnt << 8) | (skill << 16))
    }

    /// A skill attack: `(skill << 16) | level` (`Ctrl.cpp:1022`).
    pub const fn skill_attack(skill: u32, level: u32) -> DamageParam {
        DamageParam((skill << 16) | level)
    }

    /// An NPC special attack that names an attack item: `item << 16` (`OnAttackSP`).
    pub const fn item_attack(item: u32) -> DamageParam {
        DamageParam(item << 16)
    }

    /// A precomputed (`AF_FORCE`) amount: the raw damage as the parameter (`SendDamage(AF_FORCE, ..., dmg)`).
    pub const fn force(amount: i32) -> DamageParam {
        DamageParam(amount as u32)
    }

    /// The raw word as the legacy signed `int`.
    pub const fn as_i32(self) -> i32 {
        self.0 as i32
    }

    /// `GetChargeLevel`: 0 for a skill attack, else `p & 0xFF`.
    pub const fn charge_level(self, f: AtkFlags) -> u32 {
        if f.is_skill_attack() {
            0
        } else {
            self.0 & 0xFF
        }
    }

    /// `GetSkillLevel`: `p & 0xFF`.
    pub const fn skill_level(self) -> u32 {
        self.0 & 0xFF
    }

    /// `GetAttackCount`: `(p >> 8) & 0xFF`; `> 0` means ATK x 0.1 (a dead path in this build, Plan/17 §4.4).
    pub const fn attack_count(self) -> u32 {
        (self.0 >> 8) & 0xFF
    }

    /// `GetSkill`: `(p >> 16) & 0xFFFF` for skill attacks, else 0.
    pub const fn skill(self, f: AtkFlags) -> u32 {
        if f.is_skill_attack() {
            (self.0 >> 16) & 0xFFFF
        } else {
            0
        }
    }

    /// The attack item an NPC hit names: `nParam >> 16` with the sign extension of the legacy `int`
    /// (`GetHitMinMax`). `None` when it is 0, `NULL_ID` or the truncated `0xFFFF` of a monster projectile
    /// (`MakeParamDamage(0, NULL_ID, 0)`), in which case the NPC falls back to its `dwAtk1`.
    pub const fn item_id(self) -> Option<u32> {
        let v = (self.as_i32() >> 16) as u32;
        if v == 0 || v == NULL_ID || v == 0xFFFF {
            None
        } else {
            Some(v)
        }
    }

    /// `GetHandFlag` (`AttackArbiter.cpp:1055`): bit 0 right, bit 1 left. A bow hit is always the right hand;
    /// an autoattack reads `p & 3` (0 = right); everything else is the right hand.
    pub const fn hand_flag(self, f: AtkFlags) -> u8 {
        if f.intersects(AtkFlags::RANGE) {
            return 1;
        }
        if f.intersects(AtkFlags::GENERIC) {
            let h = (self.0 & 3) as u8;
            return if h == 0 { 1 } else { h };
        }
        1
    }

    /// The hands of [`DamageParam::hand_flag`] in the order the arbiter loops over them (right, then left).
    pub fn hands(self, f: AtkFlags) -> impl Iterator<Item = Hand> {
        let flag = self.hand_flag(f);
        [(1u8, Hand::Right), (2u8, Hand::Left)]
            .into_iter()
            .filter(move |(bit, _)| flag & bit != 0)
            .map(|(_, h)| h)
    }

    /// `CanIgnoreDEF`: Asalraalaikum, Hit of Penya and `AF_FORCE` ignore the defense.
    pub const fn can_ignore_def(self, f: AtkFlags) -> bool {
        let skill = self.skill(f);
        skill == SI_BIL_PST_ASALRAALAIKUM
            || skill == SI_JST_YOYO_HITOFPENYA
            || f.intersects(AtkFlags::FORCE)
    }
}
