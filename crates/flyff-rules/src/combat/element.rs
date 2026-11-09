//! The element system of autoattacks (Plan/17 §5.12; `GetDamagePropertyFactor`, `MoverAttack.cpp:1367-1515`;
//! WP-COMBAT-03). Magic skills use only the resists and the +-10 % of `magic_skill_factor`.

use super::{CombatCx, CombatantView};
use crate::consts::dst::DST_CHR_WEAEATKCHANGE;
use flyff_data::lua_config::ItemUpgradeConfig;

/// One row of `ItemUpgrade.lua` `AddAttribute(level, prob, damage, defense, addAtk)` (only the three rates the
/// damage formulas read; all `/10000`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Row {
    damage: i32,
    defense: i32,
    add_atk: i32,
}

/// The `m_mapAttributeEnchant` factors of `CItemUpgrade` (`GetAttributeDamageFactor`,
/// `GetAttributeDefenseFactor`, `GetAttributeAddAtkDmgFactor`): row `n` for `1 <= n <= rows`, the last row above
/// that, 0 for `n <= 0`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttributeTable {
    rows: Vec<Row>,
}

/// The 20 rows of the shipped `Resource/ItemUpgrade.lua`: `(damage = defense, add_atk)` for levels 1 to 20.
const SHIPPED: [(i32, i32); 20] = [
    (200, 500),
    (221, 522),
    (256, 560),
    (305, 612),
    (368, 680),
    (446, 763),
    (537, 860),
    (642, 973),
    (761, 1101),
    (895, 1244),
    (1042, 1402),
    (1203, 1575),
    (1379, 1763),
    (1568, 1966),
    (1772, 2184),
    (1989, 2417),
    (2221, 2665),
    (2467, 2998),
    (2726, 3207),
    (3000, 3500),
];

impl Default for AttributeTable {
    /// The shipped table.
    fn default() -> Self {
        AttributeTable {
            rows: SHIPPED
                .iter()
                .map(|&(d, a)| Row {
                    damage: d,
                    defense: d,
                    add_atk: a,
                })
                .collect(),
        }
    }
}

impl AttributeTable {
    /// The table of the loaded `ItemUpgrade.lua` (rows in file order, which is ascending by level).
    pub fn from_config(cfg: &ItemUpgradeConfig) -> AttributeTable {
        let mut rows: Vec<(i32, Row)> = cfg
            .attribute
            .iter()
            .map(|a| {
                (
                    a.level,
                    Row {
                        damage: a.damage_rate,
                        defense: a.defense_rate,
                        add_atk: a.add_atk_dmg_rate,
                    },
                )
            })
            .collect();
        rows.sort_by_key(|(level, _)| *level);
        AttributeTable {
            rows: rows.into_iter().map(|(_, r)| r).collect(),
        }
    }

    /// `GetMaxAttributeEnchantSize()`: the number of rows.
    pub fn max_level(&self) -> i32 {
        i32::try_from(self.rows.len()).unwrap_or(i32::MAX)
    }

    fn row(&self, n: i32) -> Option<&Row> {
        let n = n.min(self.max_level());
        let idx = usize::try_from(n.checked_sub(1)?).ok()?;
        self.rows.get(idx)
    }

    /// `GetAttributeDamageFactor(n)`.
    pub fn damage_factor(&self, n: i32) -> i32 {
        self.row(n).map_or(0, |r| r.damage)
    }

    /// `GetAttributeDefenseFactor(n)`.
    pub fn defense_factor(&self, n: i32) -> i32 {
        self.row(n).map_or(0, |r| r.defense)
    }

    /// `GetAttributeAddAtkDmgFactor(n)`.
    pub fn add_atk_factor(&self, n: i32) -> i32 {
        self.row(n).map_or(0, |r| r.add_atk)
    }
}

/// `table[atk][def]` of `GetDamagePropertyFactor` (element ids 0 none, 1 fire, 2 water, 3 electricity, 4 wind,
/// 5 earth): 0 no relation, 1 same, 2 the defender is superior, 3 the attacker is superior. Cycle: fire > wind >
/// earth > electricity > water > fire.
pub const ELEMENT_RELATION: [[u8; 6]; 6] = [
    [0, 0, 0, 0, 0, 0],
    [0, 1, 2, 0, 3, 0],
    [0, 3, 1, 2, 0, 0],
    [0, 0, 3, 1, 0, 2],
    [0, 2, 0, 0, 1, 3],
    [0, 0, 0, 3, 2, 1],
];

/// `GetDamagePropertyFactor` (`__EXT_ATTRIBUTE`): the `(ATK factor, DEF factor)` pair, both `10000 + F`.
///
/// The attacker's element is the weapon item's (level `option + DST_CHR_WEAEATKCHANGE`) or, without one, the hand
/// prop's `item_type` at level 0; an NPC's is its `element_type` at level `element_atk`. The defender's is the
/// suit item's (players) or the NPC's `element_type` / `element_atk` (sic: the attack value is the defense
/// level). Both elements none gives `(10000, 10000)`.
pub fn damage_property_factor(
    att: &dyn CombatantView,
    def: &dyn CombatantView,
    cx: &CombatCx<'_>,
) -> (i32, i32) {
    let neutral = (10_000, 10_000);
    let (atk_type, atk_level) = if att.is_player() {
        let w = att.weapon_element();
        if w.element != 0 {
            (
                w.element,
                w.level.wrapping_add(att.adj(DST_CHR_WEAEATKCHANGE)),
            )
        } else {
            (att.hand_prop().map_or(0, |p| p.item_type), 0)
        }
    } else {
        let p = att.mover_prop();
        (p.element_type, i32::from(p.element_atk))
    };
    let (def_type, def_level) = if def.is_player() {
        let s = def.suit_element();
        (s.element, s.level)
    } else {
        let p = def.mover_prop();
        (p.element_type, i32::from(p.element_atk))
    };
    if atk_type == 0 && def_type == 0 {
        return neutral;
    }
    let relation = ELEMENT_RELATION
        .get(atk_type as usize)
        .and_then(|row| row.get(def_type as usize))
        .copied()
        .unwrap_or(0);
    let t = cx.attr;
    let (mut factor, mut level) = (0_i32, 0_i32);
    match relation {
        0 | 1 => {
            if atk_level > 0 && def_level == 0 {
                factor = factor.wrapping_add(t.damage_factor(atk_level));
            } else if atk_level == 0 && def_level > 0 {
                factor = factor.wrapping_sub(t.defense_factor(def_level.wrapping_sub(3)));
            } else if atk_level > 0 && def_level > 0 {
                level = atk_level.wrapping_sub(def_level);
            }
        }
        2 => level = atk_level.wrapping_sub(5).wrapping_sub(def_level),
        3 => {
            level = atk_level.wrapping_sub(if def_level > 5 { def_level - 5 } else { 0 });
            if level > 0 {
                factor = factor.wrapping_add(t.add_atk_factor(level));
            }
        }
        _ => {}
    }
    if level != 0 {
        factor = factor.wrapping_add(if level > 0 {
            t.damage_factor(level)
        } else {
            t.defense_factor(level.wrapping_neg()).wrapping_neg()
        });
    }
    (
        10_000_i32.wrapping_add(factor),
        10_000_i32.wrapping_add(factor),
    )
}
