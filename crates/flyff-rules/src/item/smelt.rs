//! Which item takes which upgrade (Plan/19 §12.1-§12.4; src:WORLDSERVER/ItemUpgrade.cpp:384-460, 1411-1429,
//! DPSrvr.cpp:6302-6432): the material classification of the `Enchant` and `SmeltSafetyReq` dispatchers, the
//! eligibility predicates of a target, the element cards and the commercial element stones. Pure functions
//! over item props, shared by the server handlers and the browser windows.

use flyff_common::defs::item::{
    II_CHR_SYS_SCR_DEEARTHYSTONE, II_CHR_SYS_SCR_DEFIREASTONE, II_CHR_SYS_SCR_DELIGHTINESTONE,
    II_CHR_SYS_SCR_DEWATEILSTONE, II_CHR_SYS_SCR_DEWINDYOSTONE, II_CHR_SYS_SCR_EARTHYSTONE,
    II_CHR_SYS_SCR_FIREASTONE, II_CHR_SYS_SCR_LIGHTINESTONE, II_CHR_SYS_SCR_TINEINEDSTONE,
    II_CHR_SYS_SCR_WATEILSTONE, II_CHR_SYS_SCR_WINDYOSTONE, II_GEN_MAT_ELE_CYCLON,
    II_GEN_MAT_ELE_DESERT, II_GEN_MAT_ELE_FLAME, II_GEN_MAT_ELE_GENERATOR, II_GEN_MAT_ELE_RIVER,
    II_GEN_MAT_MOONSTONE, II_GEN_MAT_MOONSTONE_1, II_GEN_MAT_ORICHALCUM01,
    II_GEN_MAT_ORICHALCUM01_1, II_GEN_MAT_ORICHALCUM02,
};
use flyff_common::defs::itemkind::{
    IK2_ARMOR, IK2_ARMORETC, IK2_SYSTEM, IK2_WEAPON_DIRECT, IK2_WEAPON_MAGIC, IK3_ELECARD,
    IK3_ENCHANT, IK3_PIERDICE, IK3_SMELT_ACCESSORY_MATERIAL, IK3_SMELT_GENERAL_MATERIAL,
    IK3_SMELT_ULTIMATE_MATERIAL, IK3_SUIT, WEAPON_ULTIMATE,
};
use flyff_data::props::item::ItemProp;

/// `FIRE` of `SAI79::ePropType`.
pub const ELEMENT_FIRE: u32 = 1;
/// `WATER`.
pub const ELEMENT_WATER: u32 = 2;
/// `ELECTRICITY`.
pub const ELEMENT_ELECTRICITY: u32 = 3;
/// `WIND`.
pub const ELEMENT_WIND: u32 = 4;
/// `EARTH`.
pub const ELEMENT_EARTH: u32 = 5;
/// `END_PROP`: the first value that is no element.
pub const ELEMENT_END: u32 = 6;

/// `IsDiceRefineryAble`: the general refine accepts suits, armour pieces and the two weapon kinds.
pub fn is_dice_refinery_able(p: &ItemProp) -> bool {
    matches!(
        p.item_kind2,
        IK2_ARMOR | IK2_ARMORETC | IK2_WEAPON_DIRECT | IK2_WEAPON_MAGIC
    )
}

/// `IsEleRefineryAble`: the element upgrade accepts suits and the two weapon kinds.
pub fn is_ele_refinery_able(p: &ItemProp) -> bool {
    p.item_kind3 == IK3_SUIT || matches!(p.item_kind2, IK2_WEAPON_MAGIC | IK2_WEAPON_DIRECT)
}

/// An ultimate weapon (`dwReferStat1 == WEAPON_ULTIMATE`): the general refine refuses it.
pub fn is_ultimate_weapon(p: &ItemProp) -> bool {
    p.refer_stat1 == WEAPON_ULTIMATE
}

/// `II_GEN_MAT_ORICHALCUM01` or `_1`: the only material of the general refine.
pub fn is_orichalcum(item_id: u32) -> bool {
    matches!(item_id, II_GEN_MAT_ORICHALCUM01 | II_GEN_MAT_ORICHALCUM01_1)
}

/// `II_GEN_MAT_ORICHALCUM02` (Shining Orichalcum): the material of the ultimate paths.
pub fn is_shining_orichalcum(item_id: u32) -> bool {
    item_id == II_GEN_MAT_ORICHALCUM02
}

/// `II_GEN_MAT_MOONSTONE` or `_1`: the material of the accessory, collector and piercing-size paths.
pub fn is_moonstone(item_id: u32) -> bool {
    matches!(item_id, II_GEN_MAT_MOONSTONE | II_GEN_MAT_MOONSTONE_1)
}

/// The branch of the legacy `Enchant` dispatcher (`CItemUpgrade::OnEnchant`) a material selects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnchantBranch {
    /// `IK3_ELECARD`: the element upgrade.
    Element,
    /// `IK3_ENCHANT` (and, with `features.better_safe_upgrades`, the two repack smelt materials): general.
    General,
    /// Anything else: the accessory or collector refine when the target is one.
    Other,
}

/// `OnEnchant`'s `switch ( dwItemKind3 )`.
pub fn enchant_branch(material: &ItemProp, better_safe_upgrades: bool) -> EnchantBranch {
    match material.item_kind3 {
        IK3_ELECARD => EnchantBranch::Element,
        IK3_ENCHANT => EnchantBranch::General,
        IK3_SMELT_ULTIMATE_MATERIAL | IK3_SMELT_GENERAL_MATERIAL if better_safe_upgrades => {
            EnchantBranch::General
        }
        _ => EnchantBranch::Other,
    }
}

/// The first switch of `OnSmeltSafety` (`CItemUpgrade::OnSmeltSafety`, ItemUpgrade.cpp:424-460).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SafeBranch {
    /// Orichalcum kinds: `SmeltSafetyNormal` (general by id, the Shining Orichalcum is the ultimate path).
    Normal,
    /// Moonstone kinds: the accessory path, else the piercing size, else the element case (which answers 0).
    Moonstone,
    /// `IK3_ELECARD`: the element path.
    Element,
    /// Anything else answers 0.
    None,
}

/// `OnSmeltSafety`'s `switch ( dwItemKind3 )`.
pub fn safe_branch(material: &ItemProp, better_safe_upgrades: bool) -> SafeBranch {
    match material.item_kind3 {
        IK3_ENCHANT => SafeBranch::Normal,
        IK3_SMELT_ULTIMATE_MATERIAL | IK3_SMELT_GENERAL_MATERIAL if better_safe_upgrades => {
            SafeBranch::Normal
        }
        IK3_PIERDICE => SafeBranch::Moonstone,
        IK3_SMELT_ACCESSORY_MATERIAL if better_safe_upgrades => SafeBranch::Moonstone,
        IK3_ELECARD => SafeBranch::Element,
        _ => SafeBranch::None,
    }
}

/// `WhatEleCard( element )`: the one card that upgrades each element (`II_GEN_MAT_ELE_FLAME` ... ); `None`
/// for no element.
pub fn element_card(element: u32) -> Option<u32> {
    match element {
        ELEMENT_FIRE => Some(II_GEN_MAT_ELE_FLAME),
        ELEMENT_WATER => Some(II_GEN_MAT_ELE_RIVER),
        ELEMENT_ELECTRICITY => Some(II_GEN_MAT_ELE_GENERATOR),
        ELEMENT_EARTH => Some(II_GEN_MAT_ELE_DESERT),
        ELEMENT_WIND => Some(II_GEN_MAT_ELE_CYCLON),
        _ => None,
    }
}

/// `pItemMaterial->dwID == WhatEleCard( eItemType )`: the card is **the** card of its element (the old
/// `ELE_TOUCH`, `ELE_LAKE`, ... cards are rejected).
pub fn is_the_element_card(card: &ItemProp) -> bool {
    element_card(card.item_type) == Some(card.id)
}

/// What a commercial element stone does (`OnCommercialElem`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoneKind {
    /// An attack stone: weapons only.
    Attack,
    /// A defence stone: suits only.
    Defence,
    /// `II_CHR_SYS_SCR_TINEINEDSTONE`: clears the element.
    Remove,
}

/// The kind of a commercial element stone item; `None` for any other item.
pub fn commercial_stone(item_id: u32) -> Option<StoneKind> {
    match item_id {
        II_CHR_SYS_SCR_FIREASTONE
        | II_CHR_SYS_SCR_WATEILSTONE
        | II_CHR_SYS_SCR_WINDYOSTONE
        | II_CHR_SYS_SCR_LIGHTINESTONE
        | II_CHR_SYS_SCR_EARTHYSTONE => Some(StoneKind::Attack),
        II_CHR_SYS_SCR_DEFIREASTONE
        | II_CHR_SYS_SCR_DEWATEILSTONE
        | II_CHR_SYS_SCR_DEWINDYOSTONE
        | II_CHR_SYS_SCR_DELIGHTINESTONE
        | II_CHR_SYS_SCR_DEEARTHYSTONE => Some(StoneKind::Defence),
        II_CHR_SYS_SCR_TINEINEDSTONE => Some(StoneKind::Remove),
        _ => None,
    }
}

/// The target kinds of `OnCommercialElem`: a weapon (`IK2_WEAPON_MAGIC` / `IK2_WEAPON_DIRECT`) or an armour
/// suit (`IK2_ARMOR` / `IK2_ARMORETC` with `IK3_SUIT`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoneTarget {
    /// A weapon.
    Weapon,
    /// A suit.
    Suit,
}

/// The stone target class of `p`, `None` when the item takes no stone.
pub fn stone_target(p: &ItemProp) -> Option<StoneTarget> {
    if matches!(p.item_kind2, IK2_WEAPON_MAGIC | IK2_WEAPON_DIRECT) {
        Some(StoneTarget::Weapon)
    } else if matches!(p.item_kind2, IK2_ARMOR | IK2_ARMORETC) && p.item_kind3 == IK3_SUIT {
        Some(StoneTarget::Suit)
    } else {
        None
    }
}

/// `dwItemKind2 == IK2_SYSTEM` (the stone prop check of `OnCommercialElem`).
pub fn is_system_kind(p: &ItemProp) -> bool {
    p.item_kind2 == IK2_SYSTEM
}

// ---- the route of a pair (the browser's windows ask the server's dispatcher) ----------------------------------

pub use super::piercing::{MAX_PIERCING_SUIT, MAX_PIERCING_WEAPON, MAX_VIS};
use flyff_common::defs::item::{
    II_SYS_SYS_SCR_PIEPROT, II_SYS_SYS_SCR_SMELPROT, II_SYS_SYS_SCR_SMELPROT3,
    II_SYS_SYS_SCR_SMELPROT4, II_SYS_SYS_SCR_SMELTING, II_SYS_SYS_SCR_SMELTING2,
    II_SYS_SYS_SCR_SUPERSMELTING,
};

/// `IsPierceAble( NULL_ID, TRUE )` (Item.cpp:739-787): whether the item takes one more socket. With
/// `features.no_piercing` (the repack) the gear branches do not exist and the vis `else` binds to
/// `if ( bSize )`, so the answer is always `false`. One truth table with [`super::piercing::is_pierce_able`]
/// (the integrator folded the two copies into one).
pub fn can_take_socket(p: &ItemProp, vis_pet: bool, size: usize, no_piercing: bool) -> bool {
    super::piercing::is_pierce_able(p, size, vis_pet, None, true, no_piercing)
}

/// The branch a pair of items takes in the server's `Enchant` / `SmeltSafetyReq` dispatcher.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Branch {
    /// The general refine +1..+10.
    General,
    /// The element upgrade.
    Element,
    /// The accessory upgrade.
    Accessory,
    /// The collector upgrade (legacy path only).
    Collector,
    /// A socket for gear or a vis pet (safe path, `features.no_piercing = false`).
    PiercingSize,
    /// The Shining Orichalcum: the ultimate enchant (doc 19 §12.7).
    Ultimate,
    /// No branch: the request is refused.
    None,
}

/// What the dispatcher needs to know about a pair of items.
#[derive(Clone, Copy, Debug)]
pub struct RouteInput<'a> {
    /// The upgraded item.
    pub target: &'a ItemProp,
    /// The target is an accessory of `accessory.inc`.
    pub target_is_accessory: bool,
    /// The target is `II_GEN_TOO_COL_NORMAL`.
    pub target_is_collector: bool,
    /// The target is a vis pet.
    pub target_is_vis_pet: bool,
    /// The sockets the target has.
    pub target_piercing_size: usize,
    /// The material.
    pub material: &'a ItemProp,
    /// The safe smelt (`SmeltSafetyReq`) instead of the legacy `Enchant`.
    pub safe: bool,
    /// `features.better_safe_upgrades`.
    pub better_safe_upgrades: bool,
    /// `features.no_piercing`.
    pub no_piercing: bool,
}

/// The branch and the items the safe path asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Route {
    /// The branch.
    pub branch: Branch,
    /// The protection item the safe path consumes (`II_SYS_SYS_SCR_SMELPROT` ...); 0 for none.
    pub protection: u32,
    /// The smelting scroll the safe path accepts as the optional fourth item; 0 for none.
    pub smelt_scroll: u32,
    /// The highest level of the branch's own counter (the cap): `+10` general, `20` element and accessory, `5`
    /// collector, 0 where it does not apply.
    pub max_level: i32,
}

/// The legacy protection and smelting **buffs** (`II_SYS_SYS_SCR_*`) a branch reads: `(protection,
/// smelting scrolls)`.
pub fn legacy_buffs(b: Branch) -> (u32, &'static [u32]) {
    match b {
        Branch::General => (
            II_SYS_SYS_SCR_SMELPROT,
            &[II_SYS_SYS_SCR_SMELTING, II_SYS_SYS_SCR_SUPERSMELTING],
        ),
        Branch::Element => (
            II_SYS_SYS_SCR_SMELPROT,
            &[II_SYS_SYS_SCR_SMELTING2, II_SYS_SYS_SCR_SUPERSMELTING],
        ),
        Branch::Accessory => (II_SYS_SYS_SCR_SMELPROT4, &[]),
        _ => (0, &[]),
    }
}

/// The route of a pair: which branch the dispatcher takes and, for the safe path, which protection item and
/// smelting scroll that branch asks for (**VR-022**: the element branch needs `SMELPROT`). The branch itself
/// may still refuse the pair (a target that cannot take the upgrade); this is only the dispatch.
pub fn route(i: &RouteInput<'_>) -> Route {
    let none = Route {
        branch: Branch::None,
        protection: 0,
        smelt_scroll: 0,
        max_level: 0,
    };
    let general = Route {
        branch: Branch::General,
        protection: II_SYS_SYS_SCR_SMELPROT,
        smelt_scroll: II_SYS_SYS_SCR_SMELTING,
        max_level: 10,
    };
    let accessory = Route {
        branch: Branch::Accessory,
        protection: II_SYS_SYS_SCR_SMELPROT4,
        smelt_scroll: 0,
        max_level: 20,
    };
    let element = Route {
        branch: Branch::Element,
        protection: II_SYS_SYS_SCR_SMELPROT,
        smelt_scroll: II_SYS_SYS_SCR_SMELTING2,
        max_level: 20,
    };
    if !i.safe {
        return match enchant_branch(i.material, i.better_safe_upgrades) {
            EnchantBranch::Element => element,
            EnchantBranch::General => general,
            EnchantBranch::Other if i.target_is_accessory => accessory,
            EnchantBranch::Other if i.target_is_collector => Route {
                branch: Branch::Collector,
                protection: 0,
                smelt_scroll: 0,
                max_level: 5,
            },
            EnchantBranch::Other => none,
        };
    }
    match safe_branch(i.material, i.better_safe_upgrades) {
        SafeBranch::Normal if is_orichalcum(i.material.id) => general,
        SafeBranch::Normal if is_shining_orichalcum(i.material.id) => Route {
            branch: Branch::Ultimate,
            protection: II_SYS_SYS_SCR_SMELPROT3,
            smelt_scroll: 0,
            max_level: 10,
        },
        SafeBranch::Moonstone if i.target_is_accessory => accessory,
        SafeBranch::Moonstone
            if can_take_socket(
                i.target,
                i.target_is_vis_pet,
                i.target_piercing_size,
                i.no_piercing,
            ) =>
        {
            Route {
                branch: Branch::PiercingSize,
                protection: II_SYS_SYS_SCR_PIEPROT,
                smelt_scroll: 0,
                max_level: 0,
            }
        }
        // the moonstone falls through into the element case, which refuses it (KEEP)
        SafeBranch::Moonstone => none,
        SafeBranch::Element => element,
        _ => none,
    }
}
