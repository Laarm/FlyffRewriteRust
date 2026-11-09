//! The item, price, tooltip and statistics exports of `RulesContext` (Plan/16 §18.4 M4 rows, WP-MOVER-22).
//!
//! Every function here is a thin wrapper over a function of `flyff-rules` (`stats::statistics`,
//! `stats::item_min_max`, `item::tooltip::item_tooltip_numbers`, `item::price::*`, `item::trade::tradeable`):
//! the server handlers of doc 19 call the same code, so the browser shows exactly the server's numbers. The
//! Rust core ([`RulesCore`] methods) has no `JsValue` and is tested natively; the `#[wasm_bindgen]` block at the
//! end only converts.
//!
//! **Forms.** Prices are plain numbers (`f64`, exact below 2^53): `buy_unit_price` and `sell_unit_price` answer
//! `[unit, tax]`, `quick_sell_unit_price` answers `[unit, tax]` too (a superset of the plan's single number: the
//! window can show the tax line), `chip_cost` answers `-1` for an item without a chip price (the plan's `u32`
//! would be `0xFFFFFFFF`). A price input names either an explicit `cost` or an `itemId` with its `abilityOption`
//! (then `get_cost` of the item table decides). The tooltip call takes one object and answers one object, once
//! per tooltip build (Plan/42 §9.6); `null` stands for "none" in its answer.

use super::{RulesContext, RulesCore, StatInputJs, StatisticsJs, from_js, js_err};
use flyff_common::defs::item::II_SYS_SYS_SCR_PERIN;
use flyff_common::defs::itemkind::WEAPON_ULTIMATE;
use flyff_rules::item::random_option::{RandomOptKind, RandomOptWord};
use flyff_rules::item::tooltip::{
    ItemTooltipInput, ItemTooltipNumbers, RandomOptionOut, SetSummary, TooltipOwner, TooltipShop,
    TooltipTables, TooltipWorn, item_tooltip_numbers,
};
use flyff_rules::item::{
    TradeView, buy_unit_price, chip_cost, get_cost, hair_cost, is_taxable_item, mail_storage_fee,
    quick_sell_unit_price, sell_unit_price, tradeable,
};
use flyff_rules::stats::item_min_max;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::{JsError, JsValue, wasm_bindgen};

// ---- inputs ---------------------------------------------------------------------------------------------------

/// The viewer of a tooltip (camelCase).
#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TooltipOwnerJs {
    /// 0 male, 1 female.
    pub sex: u32,
    /// The job id.
    pub job: i32,
    /// The level.
    pub level: i32,
    /// The item in the right hand (0 = none).
    pub rweapon_item: u32,
}

/// One worn item of the viewer (camelCase).
#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TooltipWornJs {
    /// `PARTS_*`.
    pub part: u32,
    /// The item id.
    pub item_id: u32,
    /// The item is expired.
    pub expired: bool,
}

/// The shop context of a tooltip (camelCase); a missing field is the neutral value.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TooltipShopJs {
    /// `rates.shop_cost`; `null` = 1.0.
    pub shop_rate: Option<f32>,
    /// `EventLua.GetShopBuyFactor()`.
    pub buy_factor: f32,
    /// `EventLua.GetShopSellFactor()`.
    pub sell_factor: f32,
    /// The purchase tax rate of the continent the viewer stands on (0 = none).
    pub purchase_tax_rate: f32,
    /// The sales tax rate (0 = none).
    pub sales_tax_rate: f32,
}

impl Default for TooltipShopJs {
    fn default() -> Self {
        TooltipShopJs {
            shop_rate: None,
            buy_factor: 1.0,
            sell_factor: 1.0,
            purchase_tax_rate: 0.0,
            sales_tax_rate: 0.0,
        }
    }
}

/// The input of `item_tooltip_numbers` (camelCase; Plan/42 §9.6 `ItemTooltipInput`). The 64-bit random-option
/// word travels as two 32-bit halves (never through a JavaScript number).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ItemTooltipInputJs {
    /// The item id.
    pub item_id: u32,
    /// The upgrade level.
    pub ability_option: i32,
    /// The item is expired.
    pub expired: bool,
    /// The element (0 = none).
    pub element: u8,
    /// The element level.
    pub element_level: i32,
    /// High half of the random-option word.
    pub random_option_hi: u32,
    /// Low half of the random-option word.
    pub random_option_lo: u32,
    /// The item is an egg whose pet is level C or better.
    pub egg_level_c: bool,
    /// The card item of each socket (0 = empty).
    pub piercing: Vec<u32>,
    /// The ability item of each ultimate socket (0 = empty).
    pub ultimate_gems: Vec<u32>,
    /// The viewer, for the requirement lines.
    pub owner: Option<TooltipOwnerJs>,
    /// The viewer's equipment, for the set count.
    pub worn: Vec<TooltipWornJs>,
    /// The shop context.
    pub shop: Option<TooltipShopJs>,
}

impl ItemTooltipInputJs {
    fn to_rules(&self) -> ItemTooltipInput {
        ItemTooltipInput {
            item_id: self.item_id,
            ability_option: self.ability_option,
            expired: self.expired,
            element: self.element,
            element_level: self.element_level,
            random_option: RandomOptWord(
                u64::from(self.random_option_hi) << 32 | u64::from(self.random_option_lo),
            ),
            egg_level_c: self.egg_level_c,
            piercing: self.piercing.clone(),
            ultimate_gems: self.ultimate_gems.clone(),
            owner: self.owner.map(|o| TooltipOwner {
                sex: o.sex,
                job: o.job,
                level: o.level,
                rweapon_item: o.rweapon_item,
            }),
            worn: self
                .worn
                .iter()
                .map(|w| TooltipWorn {
                    part: w.part,
                    item_id: w.item_id,
                    expired: w.expired,
                })
                .collect(),
            shop: self.shop.map(|s| TooltipShop {
                shop_rate: s.shop_rate,
                buy_factor: s.buy_factor,
                sell_factor: s.sell_factor,
                purchase_tax_rate: s.purchase_tax_rate,
                sales_tax_rate: s.sales_tax_rate,
            }),
        }
    }
}

/// The input of `buy_unit_price`, `sell_unit_price` and `quick_sell_unit_price` (camelCase).
#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PriceInputJs {
    /// `GetCost()` of the item, when the caller has it; else it is computed from `itemId` and
    /// `abilityOption`.
    pub cost: Option<i32>,
    /// The item (price and tax exemption); 0 = none.
    pub item_id: u32,
    /// The upgrade level of the item.
    pub ability_option: i32,
    /// `rates.shop_cost`; `null` = 1.0 (buy only).
    pub shop_rate: Option<f32>,
    /// The event factor; `null` = 1.0.
    pub event_factor: Option<f32>,
    /// The Perin price rule; `null` = `itemId` is the Perin.
    pub perin: Option<bool>,
    /// The tax rate that applies; `null` = not taxable. The caller has applied `IsApplyTaxRate`'s place half;
    /// the item half (Perin, Red Chip, the awakening scrolls) is applied here for a named `itemId`.
    pub tax_rate: Option<f32>,
    /// The item is bound (quick sell pays 1).
    pub binds: bool,
}

/// The input of `hair_cost` (camelCase).
#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HairCostInputJs {
    /// The current hair mesh.
    pub cur_mesh: u32,
    /// The current stored colour (ARGB).
    pub cur_color: u32,
    /// The requested `[r, g, b]`.
    pub rgb: [u8; 3],
    /// The requested hair mesh.
    pub hair: u8,
}

/// The input of `tradeable` (camelCase).
#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TradeableInputJs {
    /// The item id.
    pub item_id: u32,
    /// The item is already offered in a trade or registered in a stall.
    pub reserved: bool,
    /// The item's flag byte.
    pub flags: u8,
    /// `m_dwKeepTime` (absolute unix seconds, 0 = permanent).
    pub keep_until: f64,
    /// The item is worn.
    pub equipped: bool,
    /// The item is in use (summoned pet, active ticket / feed pocket).
    pub in_use: bool,
    /// `m_idGuild`.
    pub guild_id: u32,
    /// `IsBinds` evaluated for the random-option part.
    pub binds_by_option: bool,
    /// The current unix time in seconds.
    pub now: f64,
}

// ---- outputs --------------------------------------------------------------------------------------------------

/// `{min, max}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AbilityJs {
    /// Minimum.
    pub min: i32,
    /// Maximum.
    pub max: i32,
}

/// The price line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PricesJs {
    /// `GetCost()`, `-1` unpriced.
    pub cost: i32,
    /// Unit price in a penya shop before tax.
    pub buy_unit: i32,
    /// Purchase tax per piece.
    pub buy_tax: i32,
    /// Unit price when sold to an NPC.
    pub sell_unit: i32,
    /// Sales tax per piece.
    pub sell_tax: i32,
    /// The Red Chip price, `-1` none.
    pub chip_cost: i32,
}

/// One random-option line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RandomOptionJs {
    /// `awakening` | `blessing` | `systemPet` | `eatPet`, `null` for an item without a kind.
    pub kind: Option<&'static str>,
    /// `DST_*`.
    pub dst: u32,
    /// The signed value.
    pub adj: i32,
    /// The table maximum for `(kind, dst)` (0 = unlisted).
    pub max_adj: i32,
}

/// The stat-bonus item of the low byte of the word.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RandomStatJs {
    /// The stat id.
    pub id: i32,
    /// The prefix name.
    pub name: String,
    /// `[dst, adj]` pairs.
    pub dsts: Vec<(u32, i32)>,
}

/// One piece of a set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetMemberJs {
    /// The piece.
    pub item_id: u32,
    /// `PARTS_*`.
    pub parts: i32,
    /// The viewer wears it.
    pub worn: bool,
}

/// The named set of the item.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetSummaryJs {
    /// The set id.
    pub set_id: i32,
    /// The set name.
    pub name: String,
    /// Pieces worn.
    pub worn: u32,
    /// Pieces in the set.
    pub total: u32,
    /// The pieces in set order.
    pub members: Vec<SetMemberJs>,
    /// `[dst, adj]` of the active bonuses.
    pub active_dsts: Vec<(u32, i32)>,
}

/// Everything `item_tooltip_numbers` answers (Plan/42 §9.6 `ItemTooltipNumbers`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemTooltipNumbersJs {
    /// The item exists.
    pub known: bool,
    /// `GetItemMultiplier` (an `f32`: compare thresholds with `Math.fround`).
    pub multiplier: f32,
    /// The ability line.
    pub ability: AbilityJs,
    /// 0, a `TID_*`, or `0xFFFFFFFF` (refused without a text).
    pub equip_denied: u32,
    /// The prices.
    pub prices: PricesJs,
    /// A protected awakening is pending.
    pub awakening_protected: bool,
    /// The random-option lines.
    pub random_options: Vec<RandomOptionJs>,
    /// The stat-bonus item, `null` for none.
    pub random_stat: Option<RandomStatJs>,
    /// The named set, `null` for none.
    pub set: Option<SetSummaryJs>,
    /// `[dst, adj]` of the socketed cards, merged.
    pub piercing_dsts: Vec<(u32, i32)>,
    /// `[dst, adj]` of the ultimate gems.
    pub gem_dsts: Vec<(u32, i32)>,
    /// `[dst, adj]` of the element upgrade.
    pub element_resist_dsts: Vec<(u32, i32)>,
}

fn kind_name(k: RandomOptKind) -> &'static str {
    match k {
        RandomOptKind::Awakening => "awakening",
        RandomOptKind::Blessing => "blessing",
        RandomOptKind::SystemPet => "systemPet",
        RandomOptKind::EatPet => "eatPet",
    }
}

impl From<RandomOptionOut> for RandomOptionJs {
    fn from(o: RandomOptionOut) -> Self {
        RandomOptionJs {
            kind: o.kind.map(kind_name),
            dst: o.dst,
            adj: o.adj,
            max_adj: o.max_adj,
        }
    }
}

impl From<SetSummary> for SetSummaryJs {
    fn from(s: SetSummary) -> Self {
        SetSummaryJs {
            set_id: s.set_id,
            name: s.name,
            worn: s.worn,
            total: s.total,
            members: s
                .members
                .into_iter()
                .map(|m| SetMemberJs {
                    item_id: m.item_id,
                    parts: m.parts,
                    worn: m.worn,
                })
                .collect(),
            active_dsts: s.active_dsts,
        }
    }
}

impl From<ItemTooltipNumbers> for ItemTooltipNumbersJs {
    fn from(n: ItemTooltipNumbers) -> Self {
        ItemTooltipNumbersJs {
            known: n.known,
            multiplier: n.multiplier,
            ability: AbilityJs {
                min: n.ability.min,
                max: n.ability.max,
            },
            equip_denied: n.equip_denied,
            prices: PricesJs {
                cost: n.prices.cost,
                buy_unit: n.prices.buy_unit,
                buy_tax: n.prices.buy_tax,
                sell_unit: n.prices.sell_unit,
                sell_tax: n.prices.sell_tax,
                chip_cost: n.prices.chip_cost,
            },
            awakening_protected: n.awakening_protected,
            random_options: n.random_options.into_iter().map(Into::into).collect(),
            random_stat: n.random_stat.map(|r| RandomStatJs {
                id: r.id,
                name: r.name,
                dsts: r.dsts,
            }),
            set: n.set.map(Into::into),
            piercing_dsts: n.piercing_dsts,
            gem_dsts: n.gem_dsts,
            element_resist_dsts: n.element_resist_dsts,
        }
    }
}

// ---- the core -------------------------------------------------------------------------------------------------

impl RulesCore {
    /// `statistics(obj, input)`: the `__STATISTICS` lines of the character window (Plan/16 §5.12) for the
    /// replica of `obj`: `speed_run` and `spell_cast_speed` clamped to `set_max_speed` / `set_max_cast`, the
    /// block rates to `max_block_value`, `pvp_dmg` floored at 0. The numbers are those of
    /// [`RulesCore::derived_stats`] (one function, `flyff_rules::stats::statistics`).
    pub fn statistics(&self, obj: u32, input: &StatInputJs) -> StatisticsJs {
        self.derived_with(self.table(obj), input).statistics.into()
    }

    /// `item_min_max(item, ability_option, expired)`: the ability line `[min, max]` of a tooltip; `[0, 0]`
    /// for an unknown item.
    pub fn item_min_max(&self, item_id: u32, ability_option: i32, expired: bool) -> [i32; 2] {
        let Some(prop) = self.items.get(item_id) else {
            return [0, 0];
        };
        let (min, max) = item_min_max(
            &self.rules,
            prop,
            ability_option,
            expired,
            prop.refer_stat1 == WEAPON_ULTIMATE,
        );
        [min, max]
    }

    /// `item_tooltip_numbers(input)`: every number of one tooltip in one call
    /// ([`flyff_rules::item::tooltip::item_tooltip_numbers`]).
    pub fn item_tooltip_numbers(&self, input: &ItemTooltipInputJs) -> ItemTooltipNumbersJs {
        let tables = TooltipTables {
            rules: &self.rules,
            items: &self.items,
            ext: &self.ext,
            only_armor_awakes: self.only_armor_awakes,
        };
        item_tooltip_numbers(&tables, &input.to_rules()).into()
    }

    /// The `GetCost()` a price input stands for.
    fn cost_of(&self, i: &PriceInputJs) -> i32 {
        i.cost.unwrap_or_else(|| {
            self.items
                .get(i.item_id)
                .map_or(-1, |p| get_cost(p, i.ability_option))
        })
    }

    /// The tax rate of an input after the item half of `IsApplyTaxRate`.
    fn tax_of(i: &PriceInputJs) -> Option<f32> {
        i.tax_rate
            .filter(|_| i.item_id == 0 || is_taxable_item(i.item_id))
    }

    /// `buy_unit_price(input)`: `[unit, tax]` of one piece in a penya shop.
    pub fn buy_unit_price(&self, i: &PriceInputJs) -> [f64; 2] {
        let p = buy_unit_price(
            self.cost_of(i),
            i.shop_rate,
            i.event_factor.unwrap_or(1.0),
            i.perin.unwrap_or(i.item_id == II_SYS_SYS_SCR_PERIN),
            Self::tax_of(i),
        );
        [f64::from(p.unit), f64::from(p.tax)]
    }

    /// `sell_unit_price(input)`: `[unit, tax]` of one piece sold to an NPC.
    pub fn sell_unit_price(&self, i: &PriceInputJs) -> [f64; 2] {
        let p = sell_unit_price(
            self.cost_of(i),
            i.event_factor.unwrap_or(1.0),
            Self::tax_of(i),
        );
        [f64::from(p.unit), f64::from(p.tax)]
    }

    /// `quick_sell_unit_price(input)`: `[unit, tax]` of one piece of a quick sale.
    pub fn quick_sell_unit_price(&self, i: &PriceInputJs) -> [f64; 2] {
        let p = quick_sell_unit_price(self.cost_of(i), Self::tax_of(i), i.binds);
        [f64::from(p.unit), f64::from(p.tax)]
    }

    /// `chip_cost(item)`: the Red Chip price, `-1` without one or for an unknown item.
    pub fn chip_cost(&self, item_id: u32) -> i32 {
        self.items.get(item_id).map_or(-1, chip_cost)
    }

    /// `mail_storage_fee(cost, elapsed_s)`: the fee for taking an item out of a mail (Plan/19 §11.4); `cost`
    /// is the item's `GetCost()` (truncated to `int`), `elapsed_s` the age of the mail.
    pub fn mail_storage_fee(&self, cost: f64, elapsed_s: f64) -> f64 {
        f64::from(mail_storage_fee(cost as i32, elapsed_s as i64))
    }

    /// `hair_cost(input)`: the price of a beauty-shop hair change against the current look.
    pub fn hair_cost(&self, i: &HairCostInputJs) -> f64 {
        let cur = [
            (i.cur_color >> 16) as u8,
            (i.cur_color >> 8) as u8,
            i.cur_color as u8,
        ];
        f64::from(hair_cost(i.cur_mesh, cur, i.rgb, i.hair))
    }

    /// `tradeable(input)`: `0` when the item may be put into a trade or a stall, else the `TID_*` of the
    /// refusal (an unknown item is `0`: the server decides).
    pub fn tradeable(&self, i: &TradeableInputJs) -> u32 {
        let Some(prop) = self.items.get(i.item_id) else {
            return 0;
        };
        let view = TradeView {
            reserved: i.reserved,
            flags: i.flags,
            keep_until: i.keep_until as i64,
            equipped: i.equipped,
            in_use: i.in_use,
            guild_id: i.guild_id,
            binds_by_option: i.binds_by_option,
        };
        tradeable(&view, prop, i.now as i64).unwrap_or(0)
    }
}

/// `to_js` with `None` as `null` (the tooltip answer has explicit nulls).
fn to_js_null<T: Serialize>(v: &T) -> Result<JsValue, JsError> {
    v.serialize(&serde_wasm_bindgen::Serializer::new().serialize_missing_as_null(true))
        .map_err(js_err)
}

// ---- the wasm shell -------------------------------------------------------------------------------------------

#[wasm_bindgen]
impl RulesContext {
    /// `__STATISTICS` lines for the replica of `obj`; `input` is the `derived_stats` input (`undefined` = a
    /// level-1 vagrant), the result `{ speedRun, criticalRate, spellCastSpeed, hitRate, blockMelee,
    /// blockRange, pvmDmg, pvpDmg }`.
    pub fn statistics(&self, obj: u32, input: JsValue) -> Result<JsValue, JsError> {
        let input: StatInputJs = if input.is_undefined() || input.is_null() {
            StatInputJs::default()
        } else {
            from_js(input, "statistics input")?
        };
        to_js_null(&self.core.statistics(obj, &input))
    }

    /// The ability line `[min, max]` of an item at an upgrade level (`[0, 0]` for an unknown item).
    pub fn item_min_max(&self, item_id: u32, ability_option: i32, expired: bool) -> Vec<i32> {
        self.core
            .item_min_max(item_id, ability_option, expired)
            .to_vec()
    }

    /// Every number of one item tooltip in one call (Plan/42 §9.6): `input` is `{ itemId, abilityOption,
    /// expired, element, elementLevel, randomOptionHi, randomOptionLo, eggLevelC, piercing: number[],
    /// ultimateGems: number[], owner: { sex, job, level, rweaponItem }, worn: [{ part, itemId, expired }],
    /// shop: { shopRate, buyFactor, sellFactor, purchaseTaxRate, salesTaxRate } }` (everything optional but
    /// `itemId`), the result `ItemTooltipNumbers` in camelCase with `null` for "none".
    pub fn item_tooltip_numbers(&self, input: JsValue) -> Result<JsValue, JsError> {
        let input: ItemTooltipInputJs = from_js(input, "item_tooltip_numbers input")?;
        to_js_null(&self.core.item_tooltip_numbers(&input))
    }

    /// `[unit, tax]` of one piece in a penya shop; `input` is `{ cost | itemId + abilityOption, shopRate,
    /// eventFactor, perin, taxRate }`.
    pub fn buy_unit_price(&self, input: JsValue) -> Result<Vec<f64>, JsError> {
        let i: PriceInputJs = from_js(input, "buy_unit_price input")?;
        Ok(self.core.buy_unit_price(&i).to_vec())
    }

    /// `[unit, tax]` of one piece sold to an NPC; `input` is `{ cost | itemId + abilityOption, eventFactor,
    /// taxRate }`.
    pub fn sell_unit_price(&self, input: JsValue) -> Result<Vec<f64>, JsError> {
        let i: PriceInputJs = from_js(input, "sell_unit_price input")?;
        Ok(self.core.sell_unit_price(&i).to_vec())
    }

    /// `[unit, tax]` of one piece of a quick sale; `input` is `{ cost | itemId + abilityOption, taxRate,
    /// binds }`.
    pub fn quick_sell_unit_price(&self, input: JsValue) -> Result<Vec<f64>, JsError> {
        let i: PriceInputJs = from_js(input, "quick_sell_unit_price input")?;
        Ok(self.core.quick_sell_unit_price(&i).to_vec())
    }

    /// The Red Chip price of an item, `-1` for none.
    pub fn chip_cost(&self, item_id: u32) -> i32 {
        self.core.chip_cost(item_id)
    }

    /// The fee for taking an item out of a mail (Plan/19 §11.4).
    pub fn mail_storage_fee(&self, cost: f64, elapsed_s: f64) -> f64 {
        self.core.mail_storage_fee(cost, elapsed_s)
    }

    /// The price of a beauty-shop hair change; `input` is `{ curMesh, curColor (ARGB), rgb: [r, g, b], hair }`.
    pub fn hair_cost(&self, input: JsValue) -> Result<f64, JsError> {
        let i: HairCostInputJs = from_js(input, "hair_cost input")?;
        Ok(self.core.hair_cost(&i))
    }

    /// `0` when the item may be put into a trade or a stall, else the `TID_*` of the refusal; `input` is
    /// `{ itemId, reserved, flags, keepUntil, equipped, inUse, guildId, bindsByOption, now }`.
    pub fn tradeable(&self, input: JsValue) -> Result<u32, JsError> {
        let i: TradeableInputJs = from_js(input, "tradeable input")?;
        Ok(self.core.tradeable(&i))
    }
}
