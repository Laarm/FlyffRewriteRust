//! The eat-pet loot filter (Plan/19 §6.8, `features.pet_filter`; WP-ITEM-18; src:_AIInterface/AIPet.cpp:145-184
//! `__PET_FILTER`, Resource/defineFilter.h).
//!
//! The client sends `PetFilter{filter}`: a bit mask of the loot categories its pet must **skip** (a set bit means
//! skip). The mask lives in `PlayerItems.pet_filter` (memory only; the client resends it after every login) and
//! is evaluated **only** inside `item::loot::pet_can_loot`, once per candidate of the pet's scan - the AI of
//! doc 18 has no filter code. [`pet_filter_allows`] is the whole table of §6.8:
//!
//! | bit | name | skips the items with |
//! |---|---|---|
//! | 1 | `HPFOOD` | `ik2 == IK2_FOOD` |
//! | 2 | `MPFOOD` | `ik3 == IK3_REFRESHER` |
//! | 4 | `FPFOOD` | `ik2 == IK2_POTION`, or `ik1 == IK3_DRINK` (**KEEP-QUIRK**, KQ-36: an IK3 constant compared with the item kind 1) |
//! | 8 | `WEAPON` | `ik1 == IK1_WEAPON` |
//! | 16 | `ARMOR` | `ik1 == IK1_ARMOR` |
//! | 32 | `JEWELRY` | `ik2 == IK2_JEWELRY` |
//! | 64 | `EGG` | `id == II_PET_EGG` |
//! | 128 | `SUNSTONE` | `id` in `II_GEN_MAT_ORICHALCUM01`, `_02`, `_01_1`, `II_SYS_SYS_SCR_SCRAPORICHALCUM` |
//! | 256 | `MOONSTONE` | `id` in `II_GEN_MAT_MOONSTONE`, `_1`, `II_SYS_SYS_SCR_SCRAPMOONSTONE` |
//! | 512 | `QUESTITEM` | `ik2 == IK2_GEM`, or `ik2 == IK3_GEM` (**KEEP-QUIRK**, KQ-36: an IK3 constant compared with the item kind 2) |
//! | 1024 | `ELECARDS` | `ik3 == IK3_ELECARD` |
//! | 2048 | `SOCKETCARD` | `ik3 == IK3_SOCKETCARD2` |
//!
//! Bits above 2048 do nothing. The switch `features.pet_filter = false` removes the message handler and the
//! evaluation (the original compiled both out): every item the owner may loot is fetched.

use crate::ctx::{GameCtx, PlayerRef, Services};
use flyff_common::defs::item::{
    II_GEN_MAT_MOONSTONE, II_GEN_MAT_MOONSTONE_1, II_GEN_MAT_ORICHALCUM01,
    II_GEN_MAT_ORICHALCUM01_1, II_GEN_MAT_ORICHALCUM02, II_PET_EGG, II_SYS_SYS_SCR_SCRAPMOONSTONE,
    II_SYS_SYS_SCR_SCRAPORICHALCUM,
};
use flyff_common::defs::itemkind::{
    IK1_ARMOR, IK1_WEAPON, IK2_FOOD, IK2_GEM, IK2_JEWELRY, IK2_POTION, IK3_DRINK, IK3_ELECARD,
    IK3_GEM, IK3_REFRESHER, IK3_SOCKETCARD2,
};
use flyff_data::props::item::ItemProp;
use flyff_proto::pb;

/// `FILTER_HPFOOD`.
pub const FILTER_HPFOOD: u32 = 1;
/// `FILTER_MPFOOD`.
pub const FILTER_MPFOOD: u32 = 2;
/// `FILTER_FPFOOD`.
pub const FILTER_FPFOOD: u32 = 4;
/// `FILTER_WEAPON`.
pub const FILTER_WEAPON: u32 = 8;
/// `FILTER_ARMOR`.
pub const FILTER_ARMOR: u32 = 16;
/// `FILTER_JEWELRY`.
pub const FILTER_JEWELRY: u32 = 32;
/// `FILTER_EGG`.
pub const FILTER_EGG: u32 = 64;
/// `FILTER_SUNSTONE`.
pub const FILTER_SUNSTONE: u32 = 128;
/// `FILTER_MOONSTONE`.
pub const FILTER_MOONSTONE: u32 = 256;
/// `FILTER_QUESTITEM`.
pub const FILTER_QUESTITEM: u32 = 512;
/// `FILTER_ELECARDS`.
pub const FILTER_ELECARDS: u32 = 1024;
/// `FILTER_SOCKETCARD`.
pub const FILTER_SOCKETCARD: u32 = 2048;

/// One test of the table: what an item must have to be skipped by the bit.
#[derive(Clone, Copy, Debug)]
pub enum Test {
    /// `dwItemKind1 == k` (`k` may be an `IK3_*` constant: the quirk of the FP-food row).
    Kind1(u32),
    /// `dwItemKind2 == k` (`k` may be an `IK3_*` constant: the quirk of the quest row).
    Kind2(u32),
    /// `dwItemKind3 == k`.
    Kind3(u32),
    /// `dwID` is one of these.
    Id(&'static [u32]),
}

impl Test {
    fn matches(self, p: &ItemProp) -> bool {
        match self {
            Test::Kind1(k) => p.item_kind1 == k,
            Test::Kind2(k) => p.item_kind2 == k,
            Test::Kind3(k) => p.item_kind3 == k,
            Test::Id(ids) => ids.contains(&p.id),
        }
    }
}

/// The table of §6.8: a bit and the tests that skip an item when the bit is set (any one matching skips).
pub const TABLE: &[(u32, &[Test])] = &[
    (FILTER_HPFOOD, &[Test::Kind2(IK2_FOOD)]),
    (FILTER_MPFOOD, &[Test::Kind3(IK3_REFRESHER)]),
    (
        FILTER_FPFOOD,
        &[Test::Kind2(IK2_POTION), Test::Kind1(IK3_DRINK)],
    ),
    (FILTER_WEAPON, &[Test::Kind1(IK1_WEAPON)]),
    (FILTER_ARMOR, &[Test::Kind1(IK1_ARMOR)]),
    (FILTER_JEWELRY, &[Test::Kind2(IK2_JEWELRY)]),
    (FILTER_EGG, &[Test::Id(&[II_PET_EGG])]),
    (
        FILTER_SUNSTONE,
        &[Test::Id(&[
            II_GEN_MAT_ORICHALCUM01,
            II_GEN_MAT_ORICHALCUM02,
            II_GEN_MAT_ORICHALCUM01_1,
            II_SYS_SYS_SCR_SCRAPORICHALCUM,
        ])],
    ),
    (
        FILTER_MOONSTONE,
        &[Test::Id(&[
            II_GEN_MAT_MOONSTONE,
            II_GEN_MAT_MOONSTONE_1,
            II_SYS_SYS_SCR_SCRAPMOONSTONE,
        ])],
    ),
    (
        FILTER_QUESTITEM,
        &[Test::Kind2(IK2_GEM), Test::Kind2(IK3_GEM)],
    ),
    (FILTER_ELECARDS, &[Test::Kind3(IK3_ELECARD)]),
    (FILTER_SOCKETCARD, &[Test::Kind3(IK3_SOCKETCARD2)]),
];

/// `pet_filter_allows(filter, item)` (19 §6.8): `false` when a set bit of `filter` skips the item.
pub fn pet_filter_allows(filter: u32, prop: &ItemProp) -> bool {
    !TABLE
        .iter()
        .any(|(bit, tests)| filter & bit != 0 && tests.iter().any(|t| t.matches(prop)))
}

/// `PetFilter{filter}` (`CDPSrvr::OnSetPetfilter`): the mask is stored as sent. With `features.pet_filter` off
/// the message is not handled (the original compiled the handler out).
pub fn on_pet_filter(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, m: pb::PetFilter) {
    if !svc.items.handle().cfg.pet_filter {
        return;
    }
    if let Some(p) = ctx.worlds.player_mut(who.0).and_then(|m| m.player_mut()) {
        p.items.pet_filter = m.filter;
    }
}
