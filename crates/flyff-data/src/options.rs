//! `DataOptions`: the configuration values that change loading (Plan/14 §5.5).
//!
//! The `[features]`/`[gameplay]` keys are the canonical names of Plan/03 §4.2-§4.5; `[data]` and
//! `[paths]` are proposed to doc 10. Defaults reproduce this repack. The data formats themselves (the
//! propJob BOW column `__BOWDMG_PROPJOB`, the `Setitem` 4th column `__BETTER_ARMOR_BONUS`) are **not**
//! switchable: the files contain those columns.

use flyff_common::config::RulesConfig;

/// Config values that change what `load_all` produces.
#[derive(Clone, Debug, PartialEq)]
pub struct DataOptions {
    /// `gameplay.max_pack_item` = 9999 (`__MAX_PACK_ITEM`; init-only, 03 §4.5): every prop with
    /// `pack_max > 1` is forced to this stack size.
    pub max_pack_item: u32,
    /// `gameplay.all_cs_price` = 25,000,000 (`__ALL_CS_PRICE`): `cost` of `IK3_HAT/CLOTH/GLOVE/SHOES`.
    pub cs_item_price: u32,
    /// `gameplay.all_pet_price` = 25,000,000 (`__ALL_PET_PRICE`): `cost` of `IK3_PET`.
    pub pet_item_price: u32,
    /// `gameplay.guild_rate` = 200 (`__GUILD_RATE`): divisor of every guild-level PXP requirement.
    pub guild_exp_divisor: u32,
    /// `gameplay.couple_rate` = 200 (`__COUPLE_RATE`): divisor of every couple-level EXP requirement.
    pub couple_exp_divisor: i32,
    /// `features.improved_pets` = true gives 2.80, false 1.0 (`__IMPROVED_PETS`): factor of the
    /// `speed` of `AII_PET`/`AII_EGG` movers.
    pub pet_speed_factor: f32,
    /// `features.addshopitem` = true (`__ADDSHOPITEM`): `AddShopItem(slot, item, price)` is honoured.
    pub add_shop_item: bool,
    /// `[data] cp1252_lossy` = false (14 §2.6, ledger PL-132): leftover Korean text in UTF-16 files
    /// becomes `?` as it did on the original server.
    pub cp1252_lossy: bool,
    /// `[data] strict` = false (14 §8): every Error diagnostic fails the start-up.
    pub strict: bool,
    /// `true` for serve / prepare / check; `false` for unit tests (skips P7 and its required files).
    pub load_client_data: bool,
}

impl Default for DataOptions {
    fn default() -> Self {
        Self {
            max_pack_item: 9_999,
            cs_item_price: 25_000_000,
            pet_item_price: 25_000_000,
            guild_exp_divisor: 200,
            couple_exp_divisor: 200,
            pet_speed_factor: 2.80,
            add_shop_item: true,
            cp1252_lossy: false,
            strict: false,
            load_client_data: true,
        }
    }
}

impl DataOptions {
    /// Options for unit tests: the defaults without client data.
    pub fn for_tests() -> Self {
        Self {
            load_client_data: false,
            ..Self::default()
        }
    }

    /// The loading-relevant values of a [`RulesConfig`]; `[data]` values stay at their defaults.
    pub fn from_rules(rules: &RulesConfig) -> Self {
        Self {
            max_pack_item: rules.gameplay.max_pack_item,
            cs_item_price: rules.gameplay.all_cs_price,
            pet_item_price: rules.gameplay.all_pet_price,
            guild_exp_divisor: rules.gameplay.guild_rate,
            couple_exp_divisor: i32::try_from(rules.gameplay.couple_rate).unwrap_or(i32::MAX),
            pet_speed_factor: if rules.features.improved_pets {
                2.80
            } else {
                1.0
            },
            add_shop_item: rules.features.addshopitem,
            ..Self::default()
        }
    }
}
