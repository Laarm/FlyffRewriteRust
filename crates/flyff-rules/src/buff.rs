//! Pure buff rules shared by the server and the browser (Plan/17 §6.8-§6.9, §7; WP-SKILL-01/02): the skill-buff
//! cap, the refer-stat terms of a buff duration and of a heal, and the weapon-bound validity of skill buffs
//! (`CBuffSkill::IsInvalid`, `src:_Common/buff.cpp:389-456`). The state (`BuffSet`) is `flyff_game::buff`.

use crate::consts::NULL_ID;
use crate::consts::dst::{DST_DEX, DST_INT, DST_STA};
use flyff_common::defs::attribute::{
    HD_TWO, RT_HEAL, RT_TIME, WT_MELEE_AXE, WT_MELEE_KNUCKLE, WT_MELEE_SWD, WT_MELEE_YOYO,
    WT_RANGE_BOW,
};
use flyff_common::defs::itemkind::IK3_SHIELD;
use flyff_common::defs::skill::*;
use flyff_data::props::item::ItemProp;

/// `MAX_SKILLBUFF_COUNT`: skill buffs held at once; the 29th evicts the lowest-id beneficial one (KQ-06).
pub const MAX_SKILL_BUFFS: usize = 28;

/// The load cap of a common buff's remaining time (`MIN(60)`, 13 §6.9, 17 §7.9).
pub const BUFF_LOAD_CAP_MS: u32 = 3_600_000;

/// `skill_time` of an item buff that never expires (`CBuffItem::IsRemovable`).
pub const ITEM_BUFF_PERMANENT: u32 = 999_999_999;

/// The base stats of the caster that the refer terms read (`GetSta/GetDex/GetInt`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReferStats {
    /// `GetSta()`.
    pub sta: i32,
    /// `GetDex()`.
    pub dex: i32,
    /// `GetInt()`.
    pub int: i32,
}

impl ReferStats {
    /// The stat a `dwReferStat` names (STA, DEX or INT; anything else reads 0, like the `switch` of the original).
    fn of(&self, dst: u32) -> u32 {
        let v = match dst {
            DST_STA => self.sta,
            DST_DEX => self.dex,
            DST_INT => self.int,
            _ => 0,
        };
        v as u32
    }
}

/// One refer slot: `(DWORD)(((value / 10.0f) * stat) + (lvl * (stat / 50.0f)))` in `f32`.
fn refer_term(value: u32, stat: u32, lvl: u32) -> u32 {
    (((value as f32 / 10.0_f32) * stat as f32) + (lvl as f32 * (stat as f32 / 50.0_f32))) as u32
}

/// `CMover::SubReferTime` (`MoverActEvent.cpp:171-197`): the extra duration in ms of each of the two refer
/// slots with `RT_TIME`, from the caster's stats and the skill level; a slot with another refer target is 0.
pub fn refer_time(prop: &ItemProp, skill_lvl: u32, caster: ReferStats) -> (u32, u32) {
    let slot = |target: u32, stat: u32, value: u32| {
        if target == RT_TIME {
            refer_term(value, caster.of(stat), skill_lvl)
        } else {
            0
        }
    };
    (
        slot(prop.refer_target1, prop.refer_stat1, prop.refer_value1),
        slot(prop.refer_target2, prop.refer_stat2, prop.refer_value2),
    )
}

/// The refer terms of `ApplyParam`'s `DST_HP` branch (`MoverActEvent.cpp:341-367`): `dwHp1 + dwHp2` over the
/// slots with `RT_HEAL` (DWORD arithmetic). A caster that is not a mover contributes nothing (the caller passes
/// `None`).
pub fn refer_heal(prop: &ItemProp, skill_lvl: u32, caster: Option<ReferStats>) -> u32 {
    let Some(c) = caster else { return 0 };
    let slot = |target: u32, stat: u32, value: u32| {
        if target == RT_HEAL {
            refer_term(value, c.of(stat), skill_lvl)
        } else {
            0
        }
    };
    slot(prop.refer_target1, prop.refer_stat1, prop.refer_value1).wrapping_add(slot(
        prop.refer_target2,
        prop.refer_stat2,
        prop.refer_value2,
    ))
}

/// What `GetActiveHandItemProp` reads of the worn weapon (the bare-hand prop for an empty hand).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandView {
    /// `dwWeaponType` of the right-hand prop (`WT_*`).
    pub weapon_type: u32,
    /// `dwHanded` of the right-hand prop (`HD_*`).
    pub handed: u32,
    /// `dwItemKind3` of the shield-slot prop ([`NULL_ID`] without a shield).
    pub shield_kind3: u32,
    /// A weapon is worn in both hands (`IsDualWeapon`).
    pub dual: bool,
    /// The right weapon has an element (`m_bItemResist != NO_PROP`); `None` = no right weapon.
    pub right_element: Option<bool>,
    /// The left weapon has an element; `None` = no left weapon.
    pub left_element: Option<bool>,
}

impl HandView {
    /// Bare hands: no weapon, no shield.
    pub const BARE: HandView = HandView {
        weapon_type: NULL_ID,
        handed: NULL_ID,
        shield_kind3: NULL_ID,
        dual: false,
        right_element: None,
        left_element: None,
    };
}

/// `CBuffSkill::IsInvalid` (players only): the weapon-bound skill buffs end when the weapon no longer fits.
/// `better_impowerweapon` is `features.better_impowerweapon` (true: Impower Weapon never needs an element).
pub fn skill_buff_invalid(skill: u32, hand: &HandView, better_impowerweapon: bool) -> bool {
    match skill {
        SI_JST_SUP_POISON
        | SI_JST_SUP_BLEEDING
        | SI_JST_SUP_ABSORB
        | SI_ACR_SUP_YOYOMASTER
        | SI_JST_YOYO_CRITICALSWING => hand.weapon_type != WT_MELEE_YOYO,
        SI_RAG_SUP_FASTATTACK | SI_ACR_SUP_BOWMASTER => hand.weapon_type != WT_RANGE_BOW,
        SI_BLD_MASTER_ONEHANDMASTER => !hand.dual,
        SI_KNT_MASTER_TWOHANDMASTER => hand.handed != HD_TWO,
        SI_MER_SUP_IMPOWERWEAPON if !better_impowerweapon => {
            // both hands armed: released when neither weapon has an element; the right hand only: that weapon;
            // no weapon at all: always. A left weapon alone is not released (the original's `if` chain has
            // no such branch, `bRelease` stays FALSE).
            match (hand.right_element, hand.left_element) {
                (Some(r), Some(l)) => !r && !l,
                (Some(r), None) => !r,
                (None, None) => true,
                (None, Some(_)) => false,
            }
        }
        SI_MER_SUP_BLAZINGSWORD | SI_MER_SUP_SWORDMASTER => hand.weapon_type != WT_MELEE_SWD,
        SI_MER_SUP_SMITEAXE | SI_MER_SUP_AXEMASTER => hand.weapon_type != WT_MELEE_AXE,
        SI_BIL_PST_BARAQIJALESNA | SI_BIL_PST_ASMODEUS => hand.weapon_type != WT_MELEE_KNUCKLE,
        SI_MER_SHIELD_PROTECTION | SI_MER_SHIELD_PANBARRIER => hand.shield_kind3 != IK3_SHIELD,
        _ => false,
    }
}
