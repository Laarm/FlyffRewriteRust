//! The numbers of one item tooltip, composed in one call (Plan/42 §9.6, Plan/16 §18.4 M4 row
//! `itemTooltipNumbers`, WP-MOVER-22; src:_Interface/WndManager.cpp:4986-5160, 6684-6880).
//!
//! The tooltip composer of the browser (doc 45) never computes a formula: it asks [`item_tooltip_numbers`]
//! once per tooltip build. The function only *composes* the rule functions that already exist, so the numbers
//! the client shows are the ones the server uses:
//!
//! | output | source |
//! |---|---|
//! | `multiplier`, `ability` | [`crate::stats::item_multiplier`], [`crate::stats::item_min_max`] (row 7a) |
//! | `equip_denied` | [`crate::equip_rules::check_equip`] (the red sex / job / level lines) |
//! | `prices` | [`super::price`]: `get_cost`, `buy_unit_price`, `sell_unit_price`, `chip_cost` (row 15) |
//! | `random_options` | [`RandomOptWord`] decoded with the kind table of [`random_option_kind`] (rows 7a, 19) |
//! | `set` | the named sets of the `item-ext` bundle (`CWndMgr::PutSetItemOpt`, row 16) |
//! | `piercing_dsts` | `CItemElem::GetPiercingAvail` over the socketed cards (row 18) |
//! | `gem_dsts` | `CUltimateWeapon::GetDestParamUltimate` over the ultimate sockets (row 20) |
//! | `element_resist_dsts` | [`crate::sets::element_resist_dsts`] (row 7a) |
//!
//! Everything is pure and panic-free; an unknown item answers `known: false` and zeros. The structs are plain
//! Rust types: the wasm shell converts them to and from JavaScript objects.

use super::price::{buy_unit_price, chip_cost, get_cost, is_taxable_item, sell_unit_price};
use super::random_option::{RandomOptKind, RandomOptWord, random_option_kind};
use crate::data::RulesData;
use crate::equip_rules::{Wearer, check_equip};
use crate::sets::element_resist_dsts;
use crate::stats::{item_min_max, item_multiplier};
use flyff_common::defs::item::II_SYS_SYS_SCR_PERIN;
use flyff_common::defs::itemkind::{IK3_BOW, WEAPON_ULTIMATE};
use flyff_data::bundle::types::ItemExtBundle;
use flyff_data::items_ext::item_etc::SetItemDef;
use flyff_data::items_ext::ultimate::GemAbilityDef;
use flyff_data::props::item::ItemProp;
use flyff_data::types::IdTable;

/// The immutable data the composition reads.
#[derive(Clone, Copy, Debug)]
pub struct TooltipTables<'a> {
    /// The rule tables (`expUpitem`, jobs, ...).
    pub rules: &'a RulesData,
    /// The item props.
    pub items: &'a IdTable<ItemProp>,
    /// The `item-ext` tables: named sets, piercing materials, gem abilities, random options and items.
    pub ext: &'a ItemExtBundle,
    /// `features.only_armor_awakes` (which items have a random-option kind).
    pub only_armor_awakes: bool,
}

/// The viewer, for the red lines of a requirement (`IsEquipAble`'s sex, job and level checks).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TooltipOwner {
    /// `SEX_MALE` 0 / `SEX_FEMALE` 1.
    pub sex: u32,
    /// The job id.
    pub job: i32,
    /// The level.
    pub level: i32,
    /// The item in the right hand (an arrow needs a bow), 0 = none.
    pub rweapon_item: u32,
}

/// One worn item of the viewer (`m_aEquipInfo[part]`), for the set count.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TooltipWorn {
    /// `PARTS_*`.
    pub part: u32,
    /// The item id.
    pub item_id: u32,
    /// `CItemElem::expired` (an expired piece does not count).
    pub expired: bool,
}

/// The shop the tooltip is shown for (`APP_SHOP_` / an open shop while the bag is shown): the rates of the
/// server's price functions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TooltipShop {
    /// `rates.shop_cost` (`prj.m_fShopCost`); `None` = 1.0.
    pub shop_rate: Option<f32>,
    /// `EventLua.GetShopBuyFactor()`.
    pub buy_factor: f32,
    /// `EventLua.GetShopSellFactor()`.
    pub sell_factor: f32,
    /// `CTax::GetPurchaseTaxRate` of the continent the viewer stands on (0.0 on none).
    pub purchase_tax_rate: f32,
    /// `CTax::GetSalesTaxRate` (0.0 on none).
    pub sales_tax_rate: f32,
}

impl TooltipShop {
    /// No shop: the plain prices (rate and factors 1.0, no tax).
    pub const NEUTRAL: TooltipShop = TooltipShop {
        shop_rate: None,
        buy_factor: 1.0,
        sell_factor: 1.0,
        purchase_tax_rate: 0.0,
        sales_tax_rate: 0.0,
    };
}

impl Default for TooltipShop {
    fn default() -> Self {
        TooltipShop::NEUTRAL
    }
}

/// What the composition needs to know about one item instance and its viewer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemTooltipInput {
    /// The item id.
    pub item_id: u32,
    /// `GetAbilityOption()`: the upgrade level.
    pub ability_option: i32,
    /// `CItemElem::expired`.
    pub expired: bool,
    /// The element (`SAI79::ePropType`, 0 = none).
    pub element: u8,
    /// `m_nResistAbilityOption`: the element level.
    pub element_level: i32,
    /// The 64-bit random-option word.
    pub random_option: RandomOptWord,
    /// The item is an egg whose pet is level C or better (a system-pet word).
    pub egg_level_c: bool,
    /// The card item of each socket, 0 = empty.
    pub piercing: Vec<u32>,
    /// The ability item of each ultimate socket, 0 = empty.
    pub ultimate_gems: Vec<u32>,
    /// The viewer, when the requirement lines are wanted.
    pub owner: Option<TooltipOwner>,
    /// The viewer's equipment, for the set count.
    pub worn: Vec<TooltipWorn>,
    /// The shop context; `None` = neutral.
    pub shop: Option<TooltipShop>,
}

/// `{min, max}` of the ability line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AbilityRange {
    /// `(int)(ability_min * f) + flat(opt)`.
    pub min: i32,
    /// `(int)(ability_max * f) + flat(opt)`.
    pub max: i32,
}

/// The prices of the price line (row 15).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TooltipPrices {
    /// `GetCost()` with the upgrade level, `-1` when the item has no price.
    pub cost: i32,
    /// The price of one piece in a penya shop (shop rate and event factor applied, at least 1, before tax).
    pub buy_unit: i32,
    /// The purchase tax on one piece.
    pub buy_tax: i32,
    /// What one piece pays when sold to an NPC (after the sales tax).
    pub sell_unit: i32,
    /// The sales tax withheld per piece.
    pub sell_tax: i32,
    /// The Red Chip price, `-1` when absent.
    pub chip_cost: i32,
}

/// One decoded random-option line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RandomOptionOut {
    /// Which table the item's word belongs to; `None` for an item without a kind.
    pub kind: Option<RandomOptKind>,
    /// `DST_*`.
    pub dst: u32,
    /// The signed value.
    pub adj: i32,
    /// The largest positive value listed for `(kind, dst)` in the random-option tables, 0 when unlisted
    /// (`features.max_awake_color`: the line is the maximum when `max_adj != 0 && max_adj <= adj`).
    pub max_adj: i32,
}

/// The stat-bonus item of the low byte of the word (`RandomOptItem`, the "of the Tiger" prefix).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RandomStatOut {
    /// The stat id.
    pub id: i32,
    /// The prefix name.
    pub name: String,
    /// `(DST, adj)` pairs.
    pub dsts: Vec<(u32, i32)>,
}

/// One piece of the set the item belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SetMemberOut {
    /// The piece.
    pub item_id: u32,
    /// `PARTS_*` it is worn in.
    pub parts: i32,
    /// The viewer wears it (right id in its slot, not expired).
    pub worn: bool,
}

/// The named set of the item (`PutSetItemOpt`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetSummary {
    /// `CSetItem::m_nId`.
    pub set_id: i32,
    /// The set name.
    pub name: String,
    /// Pieces worn (`GetEquipedSetItemNumber`).
    pub worn: u32,
    /// Pieces in the set.
    pub total: u32,
    /// The pieces in set order.
    pub members: Vec<SetMemberOut>,
    /// The active bonuses: every row with `equiped <= worn`, merged by DST in row order.
    pub active_dsts: Vec<(u32, i32)>,
}

/// Everything [`item_tooltip_numbers`] answers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemTooltipNumbers {
    /// The item exists; `false` zeroes everything else.
    pub known: bool,
    /// `GetItemMultiplier`.
    pub multiplier: f32,
    /// The ability line.
    pub ability: AbilityRange,
    /// 0 = the viewer meets the requirements, else the `TID_*` (`u32::MAX`: refused without a text).
    pub equip_denied: u32,
    /// The prices.
    pub prices: TooltipPrices,
    /// The protected awakening is pending (the lines are drawn struck out).
    pub awakening_protected: bool,
    /// The lines of the random-option word.
    pub random_options: Vec<RandomOptionOut>,
    /// The stat-bonus item of the word's low byte.
    pub random_stat: Option<RandomStatOut>,
    /// The named set of the item.
    pub set: Option<SetSummary>,
    /// The merged parameters of the socketed cards.
    pub piercing_dsts: Vec<(u32, i32)>,
    /// The merged parameters of the ultimate gems.
    pub gem_dsts: Vec<(u32, i32)>,
    /// `(weak, -2 * level)`, `(strong, +2 * level)` of an element upgrade; empty without an element.
    pub element_resist_dsts: Vec<(u32, i32)>,
}

/// Merges `(dst, adj)` pairs: a repeated DST adds to the first entry, new DSTs keep the order of appearance
/// (the `nFind` loops of `GetPiercingAvail` and `GetItemAvail`).
fn merge_into(out: &mut Vec<(u32, i32)>, dst: u32, adj: i32) {
    match out.iter_mut().find(|(d, _)| *d == dst) {
        Some(entry) => entry.1 = entry.1.wrapping_add(adj),
        None => out.push((dst, adj)),
    }
}

/// `CItemElem::GetPiercingAvail`: the parameters of the socketed cards, merged by DST in first-appearance
/// order. An empty socket or a card without a piercing definition adds nothing.
pub fn piercing_dsts(ext: &ItemExtBundle, cards: &[u32]) -> Vec<(u32, i32)> {
    let mut out = Vec::new();
    for &card in cards {
        let Some(def) = ext.piercing.iter().find(|p| p.item == card) else {
            continue;
        };
        for &[dst, adj] in &def.params {
            merge_into(&mut out, dst as u32, adj);
        }
    }
    out
}

/// `CUltimateWeapon::GetDestParamUltimate` (src:_Common/UltimateWeapon.cpp:828-884): only an ultimate weapon
/// has gem abilities. The sockets are read up to the first empty one; the number of sockets holding the same
/// ability item picks the value `values[n - 1]` of that item's row in the `ABILITY` block of the item's
/// `refer_stat2` (a missing block, row or value gives 0); values of one DST add. The result is ordered by DST.
pub fn gem_dst_values(
    items: &IdTable<ItemProp>,
    gem_ability: &[GemAbilityDef],
    weapon: &ItemProp,
    sockets: &[u32],
) -> Vec<(u32, i32)> {
    if weapon.refer_stat1 != WEAPON_ULTIMATE {
        return Vec::new();
    }
    // `map<DWORD, int> mapItem`: ordered by the ability item id
    let mut counts: Vec<(u32, i32)> = Vec::new();
    for &gem in sockets.iter().take_while(|g| **g != 0) {
        match counts.iter_mut().find(|(g, _)| *g == gem) {
            Some(c) => c.1 += 1,
            None => counts.push((gem, 1)),
        }
    }
    counts.sort_unstable_by_key(|(g, _)| *g);
    // `map<int, int> mapDst`: ordered by DST
    let mut by_dst: Vec<(u32, i32)> = Vec::new();
    for (gem, n) in counts {
        let Some(dst) = items.get(gem).map(|p| p.refer_stat2) else {
            continue;
        };
        let value = gem_ability
            .iter()
            .find(|a| a.dst == dst)
            .and_then(|a| a.rows.iter().find(|r| r.gem06 == gem))
            .and_then(|r| usize::try_from(n - 1).ok().and_then(|i| r.values.get(i)))
            .copied()
            .unwrap_or(0);
        match by_dst.iter_mut().find(|(d, _)| *d == dst) {
            Some(e) => e.1 = e.1.wrapping_add(value),
            None => by_dst.push((dst, value)),
        }
    }
    by_dst.sort_unstable_by_key(|(d, _)| *d);
    by_dst
}

/// `CSetItem::GetItemAvail(bAll = TRUE)`: the rows up to the first one that needs more pieces than `worn`
/// (the rows are sorted by `equiped`), merged by DST.
fn set_active_dsts(set: &SetItemDef, worn: i32) -> Vec<(u32, i32)> {
    let mut out = Vec::new();
    for a in &set.avail {
        if a.equiped > worn {
            break;
        }
        merge_into(&mut out, a.dst as u32, a.adj);
    }
    out
}

/// The named set that has `item_id` as a piece (`CSetItemFinder::GetSetItemByItemId`: the first set in
/// the table order).
fn set_of_item(ext: &ItemExtBundle, item_id: u32) -> Option<&SetItemDef> {
    ext.set_items
        .iter()
        .find(|s| s.elems.iter().any(|e| e.item == item_id))
}

fn set_summary(set: &SetItemDef, worn: &[TooltipWorn]) -> SetSummary {
    let members: Vec<SetMemberOut> = set
        .elems
        .iter()
        .map(|e| SetMemberOut {
            item_id: e.item,
            parts: e.parts,
            worn: worn.iter().any(|w| {
                i64::from(w.part) == i64::from(e.parts) && w.item_id == e.item && !w.expired
            }),
        })
        .collect();
    let n = members.iter().filter(|m| m.worn).count();
    let n_i32 = i32::try_from(n).unwrap_or(i32::MAX);
    SetSummary {
        set_id: set.id,
        name: set.name.clone(),
        worn: u32::try_from(n).unwrap_or(u32::MAX),
        total: u32::try_from(members.len()).unwrap_or(u32::MAX),
        members,
        active_dsts: set_active_dsts(set, n_i32),
    }
}

/// The table name of a random-option kind in `randomoption.inc`.
fn kind_keyword(k: RandomOptKind) -> &'static str {
    match k {
        RandomOptKind::Awakening => "Awakening",
        RandomOptKind::Blessing => "Blessing",
        RandomOptKind::SystemPet => "SystemPet",
        RandomOptKind::EatPet => "EatPet",
    }
}

/// The largest positive value listed for `(kind, dst)` in the random-option tables (0 when none).
fn max_adj(ext: &ItemExtBundle, kind: Option<RandomOptKind>, dst: u32) -> i32 {
    let Some(kind) = kind else { return 0 };
    ext.random_options
        .iter()
        .filter(|g| g.kind == kind_keyword(kind))
        .flat_map(|g| g.dsts.iter())
        .filter(|d| d.dst == dst)
        .flat_map(|d| d.values.iter().map(|v| v[0]))
        .filter(|v| *v > 0)
        .max()
        .unwrap_or(0)
}

/// The prices of the price line: the cost at the upgrade level, the purchase price with the shop's rate,
/// factor and tax, the sell price, the chip price.
pub fn tooltip_prices(prop: &ItemProp, ability_option: i32, shop: &TooltipShop) -> TooltipPrices {
    let cost = get_cost(prop, ability_option);
    let taxable = is_taxable_item(prop.id);
    let buy = buy_unit_price(
        cost,
        shop.shop_rate,
        shop.buy_factor,
        prop.id == II_SYS_SYS_SCR_PERIN,
        taxable.then_some(shop.purchase_tax_rate),
    );
    let sell = sell_unit_price(
        cost,
        shop.sell_factor,
        taxable.then_some(shop.sales_tax_rate),
    );
    TooltipPrices {
        cost,
        buy_unit: buy.unit,
        buy_tax: buy.tax,
        sell_unit: sell.unit,
        sell_tax: sell.tax,
        chip_cost: chip_cost(prop),
    }
}

/// The batched call: every number of one tooltip (see the module documentation).
pub fn item_tooltip_numbers(t: &TooltipTables<'_>, input: &ItemTooltipInput) -> ItemTooltipNumbers {
    let Some(prop) = t.items.get(input.item_id) else {
        return ItemTooltipNumbers::default();
    };
    let ultimate = prop.refer_stat1 == WEAPON_ULTIMATE;
    let multiplier = item_multiplier(t.rules, prop, input.ability_option, input.expired, ultimate);
    let (min, max) = item_min_max(t.rules, prop, input.ability_option, input.expired, ultimate);
    let word = input.random_option;
    let equip_denied = input.owner.map_or(0, |o| {
        let bow = t
            .items
            .get(o.rweapon_item)
            .is_some_and(|p| p.item_kind3 == IK3_BOW);
        let wearer = Wearer {
            sex: o.sex,
            job: o.job,
            level: o.level,
        };
        match check_equip(t.rules, &wearer, prop, word, "", bow, false) {
            Ok(()) => 0,
            Err(d) => d.tid.unwrap_or(u32::MAX),
        }
    });
    let kind = random_option_kind(
        prop.parts,
        prop.item_kind3,
        input.egg_level_c,
        t.only_armor_awakes,
    );
    let random_options = (0..word.view_line_count())
        .filter_map(|i| word.line(i))
        .map(|l| RandomOptionOut {
            kind,
            dst: u32::from(l.dst),
            adj: i32::from(l.adj),
            max_adj: max_adj(t.ext, kind, u32::from(l.dst)),
        })
        .collect();
    let stat_id = i32::from(word.stat_id());
    let random_stat = (stat_id > 0)
        .then(|| t.ext.random_opt_items.iter().find(|r| r.id == stat_id))
        .flatten()
        .map(|r| RandomStatOut {
            id: r.id,
            name: r.name.clone(),
            dsts: r.params.iter().map(|&[d, a]| (d as u32, a)).collect(),
        });
    let element_resist = if input.element != 0 && input.element_level != 0 {
        element_resist_dsts(input.element, input.element_level).to_vec()
    } else {
        Vec::new()
    };
    ItemTooltipNumbers {
        known: true,
        multiplier,
        ability: AbilityRange { min, max },
        equip_denied,
        prices: tooltip_prices(prop, input.ability_option, &input.shop.unwrap_or_default()),
        awakening_protected: word.safe_flag(),
        random_options,
        random_stat,
        set: set_of_item(t.ext, input.item_id).map(|s| set_summary(s, &input.worn)),
        piercing_dsts: piercing_dsts(t.ext, &input.piercing),
        gem_dsts: gem_dst_values(t.items, &t.ext.gem_ability, prop, &input.ultimate_gems),
        element_resist_dsts: element_resist,
    }
}
