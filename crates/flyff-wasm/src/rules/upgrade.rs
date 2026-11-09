//! The upgrade exports of `RulesContext` (Plan/19 §12, WP-ITEM-11, WP-ITEM-12; task `upgrades-a`): what the
//! refine, element, accessory, awakening and safe-smelt windows show, computed by the functions the server's
//! handlers use (`flyff_rules::item::{refine, smelt}`).
//!
//! The probability tables of the server (`ItemUpgrade.lua`, `s.txt`) are not part of the client bundles (Plan/19
//! OQ8); [`upgrade_odds`](RulesCore::upgrade_odds) answers from the shipped tables of this data set
//! (`UpgradeTables::default`), which equal the loaded ones (`res_the_loaded_odds_tables_equal_the_shipped_
//! constants` in `flyff-game`). The Rust core has no `JsValue` and is tested natively (`tests/rules_upgrades_a.rs`);
//! the `#[wasm_bindgen]` block at the end only converts.
//!
//! | export | answer |
//! |---|---|
//! | `upgrade_odds({ kind, level })` | `{ p, space, chance, maxLevel }` or `null` at the cap; `kind` is `general`, `element`, `accessory` (legacy), `accessorySafe` or `collector` |
//! | `upgrade_route({ targetItemId, materialItemId, safe, ... })` | `{ branch, protection, smeltScroll, maxLevel }`: the branch the server dispatches to and the items the safe path asks for |
//! | `upgrade_constants()` | the costs, the caps, the bonus levels and the item ids of the upgrade windows |
//! | `element_card(element)` | the one card item that upgrades `element` (0 for none) |

use super::{RulesContext, RulesCore, from_js};
use flyff_rules::item::refine::{
    AWAKENING_COST, DESTROY_FROM_LEVEL, ELEMENT_BONUS_BELOW, GENERAL_BONUS_BELOW, MAX_ACCESSORY_AO,
    PIERCING_SIZE_COST, REMOVE_ELEMENT_COST, SMELTING_BONUS, SUPER_SMELTING_BONUS, UpgradeKind,
    UpgradeTables,
};
use flyff_rules::item::smelt::{Branch, RouteInput, element_card, route};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::{JsError, JsValue, wasm_bindgen};

/// The input of `upgrade_odds` (camelCase).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OddsInputJs {
    /// `general`, `element`, `accessory`, `accessorySafe` or `collector`.
    pub kind: String,
    /// The item's current level (`+N`, the element level, the collector level).
    pub level: i32,
}

/// The answer of `upgrade_odds` (camelCase).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OddsJs {
    /// The table value `p`.
    pub p: i32,
    /// The roll space (10 000, or 1000 for the collector).
    pub space: u32,
    /// The success chance in parts per `space` after the roll rule of the path.
    pub chance: u32,
    /// The cap of the path.
    pub max_level: i32,
}

/// The input of `upgrade_route` (camelCase; every flag defaults to `false` except the two switches).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RouteInputJs {
    /// The upgraded item.
    pub target_item_id: u32,
    /// The target is an accessory of `accessory.inc`.
    pub target_is_accessory: bool,
    /// The sockets the target has.
    pub target_piercing_size: u32,
    /// The target is a vis pet (`m_bTranformVisPet` included).
    pub target_is_vis_pet: bool,
    /// The material.
    pub material_item_id: u32,
    /// `SmeltSafetyReq` instead of `Enchant`.
    pub safe: bool,
    /// `features.better_safe_upgrades` (default `true`).
    pub better_safe_upgrades: bool,
    /// `features.no_piercing` (default `true`).
    pub no_piercing: bool,
}

impl Default for RouteInputJs {
    fn default() -> Self {
        RouteInputJs {
            target_item_id: 0,
            target_is_accessory: false,
            target_piercing_size: 0,
            target_is_vis_pet: false,
            material_item_id: 0,
            safe: false,
            better_safe_upgrades: true,
            no_piercing: true,
        }
    }
}

/// The answer of `upgrade_route` (camelCase).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteJs {
    /// `general`, `element`, `accessory`, `collector`, `piercingSize`, `ultimate` or `none`.
    pub branch: &'static str,
    /// The protection item of the safe path (0 = none).
    pub protection: u32,
    /// The smelting scroll the safe path accepts (0 = none).
    pub smelt_scroll: u32,
    /// The cap of the branch.
    pub max_level: i32,
}

/// The constants of the upgrade windows (camelCase).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConstantsJs {
    /// Penya for removing an element.
    pub remove_element_cost: i64,
    /// Penya for the awakening at the NPC.
    pub awakening_cost: i64,
    /// Penya for a piercing size.
    pub piercing_size_cost: i64,
    /// Penya for removing a socket card.
    pub piercing_remove_cost: i64,
    /// `+N` of the general refine.
    pub max_general: i32,
    /// The element cap.
    pub max_element: i32,
    /// `+N` of the accessory.
    pub max_accessory: i32,
    /// The collector cap.
    pub max_collector: i32,
    /// A legacy failure from this level on destroys an unprotected item.
    pub destroy_from_level: i32,
    /// A smelting scroll adds this to the odds ...
    pub smelting_bonus: i32,
    /// ... a super smelting scroll this.
    pub super_smelting_bonus: i32,
    /// The general refine takes the smelting bonuses below this `+N`.
    pub general_bonus_below: i32,
    /// The element upgrade takes `SMELTING2` below this level.
    pub element_bonus_below: i32,
}

impl RulesCore {
    /// `upgrade_odds(input)`: the odds of one path at a level (`None` at the cap or for an unknown kind).
    pub fn upgrade_odds(&self, i: &OddsInputJs) -> Option<OddsJs> {
        let kind = match i.kind.as_str() {
            "general" => UpgradeKind::General,
            "element" => UpgradeKind::Element,
            "accessory" => UpgradeKind::Accessory,
            "accessorySafe" => UpgradeKind::AccessorySafe,
            "collector" => UpgradeKind::Collector,
            _ => return None,
        };
        let o = kind.odds(&UpgradeTables::default(), i.level)?;
        Some(OddsJs {
            p: o.p,
            space: o.space,
            chance: o.chance,
            max_level: o.max_level,
        })
    }

    /// `upgrade_route(input)`: the branch of the server's dispatcher for a pair of items. An unknown item
    /// answers `none`.
    pub fn upgrade_route(&self, i: &RouteInputJs) -> RouteJs {
        let (Some(target), Some(material)) = (
            self.items.get(i.target_item_id),
            self.items.get(i.material_item_id),
        ) else {
            return RouteJs {
                branch: "none",
                protection: 0,
                smelt_scroll: 0,
                max_level: 0,
            };
        };
        let r = route(&RouteInput {
            target,
            target_is_accessory: i.target_is_accessory,
            target_is_collector: i.target_item_id
                == flyff_common::defs::item::II_GEN_TOO_COL_NORMAL,
            target_is_vis_pet: i.target_is_vis_pet,
            target_piercing_size: i.target_piercing_size as usize,
            material,
            safe: i.safe,
            better_safe_upgrades: i.better_safe_upgrades,
            no_piercing: i.no_piercing,
        });
        RouteJs {
            branch: match r.branch {
                Branch::General => "general",
                Branch::Element => "element",
                Branch::Accessory => "accessory",
                Branch::Collector => "collector",
                Branch::PiercingSize => "piercingSize",
                Branch::Ultimate => "ultimate",
                Branch::None => "none",
            },
            protection: r.protection,
            smelt_scroll: r.smelt_scroll,
            max_level: r.max_level,
        }
    }

    /// `upgrade_constants()`.
    pub fn upgrade_constants(&self) -> ConstantsJs {
        let t = UpgradeTables::default();
        ConstantsJs {
            remove_element_cost: REMOVE_ELEMENT_COST,
            awakening_cost: AWAKENING_COST,
            piercing_size_cost: PIERCING_SIZE_COST,
            piercing_remove_cost: 1_000_000,
            max_general: t.max_general(),
            max_element: t.max_attribute(),
            max_accessory: MAX_ACCESSORY_AO,
            max_collector: t.max_collector(),
            destroy_from_level: DESTROY_FROM_LEVEL,
            smelting_bonus: SMELTING_BONUS,
            super_smelting_bonus: SUPER_SMELTING_BONUS,
            general_bonus_below: GENERAL_BONUS_BELOW,
            element_bonus_below: ELEMENT_BONUS_BELOW,
        }
    }

    /// `element_card(element)`: the card item of an element (1 fire ... 5 earth), 0 for anything else.
    pub fn element_card(&self, element: u32) -> u32 {
        element_card(element).unwrap_or(0)
    }
}

fn to_js_null<T: Serialize>(v: &T) -> Result<JsValue, JsError> {
    v.serialize(&serde_wasm_bindgen::Serializer::new().serialize_missing_as_null(true))
        .map_err(|e| JsError::new(&e.to_string()))
}

#[wasm_bindgen]
impl RulesContext {
    /// The odds of one upgrade path at a level: `input` is `{ kind: 'general' | 'element' | 'accessory' |
    /// 'accessorySafe' | 'collector', level }`, the result `{ p, space, chance, maxLevel }` (`chance` in parts per
    /// `space`) or `null` at the cap of the path or for another kind.
    pub fn upgrade_odds(&self, input: JsValue) -> Result<JsValue, JsError> {
        let i: OddsInputJs = from_js(input, "upgrade_odds input")?;
        to_js_null(&self.core.upgrade_odds(&i))
    }

    /// The branch the server's `Enchant` / `SmeltSafetyReq` takes for a pair of items: `input` is `{
    /// targetItemId, materialItemId, safe, targetIsAccessory, targetPiercingSize, targetIsVisPet,
    /// betterSafeUpgrades (default true), noPiercing (default true) }`, the result `{ branch: 'general' |
    /// 'element' | 'accessory' | 'collector' | 'piercingSize' | 'ultimate' | 'none', protection, smeltScroll,
    /// maxLevel }` (the protection item and the smelting scroll the safe path asks for, 0 = none).
    pub fn upgrade_route(&self, input: JsValue) -> Result<JsValue, JsError> {
        let i: RouteInputJs = from_js(input, "upgrade_route input")?;
        to_js_null(&self.core.upgrade_route(&i))
    }

    /// The costs, caps, bonus levels of the upgrade windows (see `ConstantsJs`).
    pub fn upgrade_constants(&self) -> Result<JsValue, JsError> {
        to_js_null(&self.core.upgrade_constants())
    }

    /// The card item that upgrades an element (1 fire, 2 water, 3 electricity, 4 wind, 5 earth); 0 for none.
    pub fn element_card(&self, element: u32) -> u32 {
        self.core.element_card(element)
    }
}
