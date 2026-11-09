//! Upgrade probability helpers shared with the browser's odds display (Plan/19 §12, §22; ref/13 §18).
//! The tables come from `ItemUpgrade.lua` (`flyff_data::lua_config::ItemUpgradeConfig`), the accessory
//! table and `pet.inc`; the functions take them as slices so they stay independent of the loaders.

/// The ENG penalty of the refine odds from +4 on (KQ-12): `p * 0.9f` truncated.
pub const REFINE_PENALTY_FROM_AO: i32 = 3;

/// `GetGeneralEnchantProb`: the chance, out of 10 000, to refine an item that currently has `+ao`; `table`
/// is `tGeneral` (index `ao`). `None` when `ao` is negative or at the end of the table (`+10`: the item
/// cannot be refined further). From `+3` (the third refine, KQ-12) the repack multiplies by `0.9f`.
pub fn general_refine_prob(table: &[i32], ao: i32) -> Option<i32> {
    let p = *table.get(usize::try_from(ao).ok()?)?;
    Some(if ao >= REFINE_PENALTY_FROM_AO {
        (p as f32 * 0.9_f32) as i32
    } else {
        p
    })
}

/// The success chance, out of 10 000, of the element upgrade to `level` (`tAttribute`, ascending by level);
/// `rows` are `(level, prob)` pairs.
pub fn element_refine_prob(rows: &[(i32, i32)], level: i32) -> Option<i32> {
    rows.iter().find(|r| r.0 == level).map(|r| r.1)
}

/// The accessory upgrade chance, out of 10 000, for an item that currently has `+ao` (`table[ao]`).
pub fn accessory_prob(table: &[i32], ao: i32) -> Option<i32> {
    table.get(usize::try_from(ao).ok()?).copied()
}

/// `pet_avail_bonus`: the stat bonus of a system pet. `values` is the kind's value row (`AvailDestParam`,
/// avail levels 1..9), `avail_levels` the rolled level per pet level D..S (index `level - 1`, 0 = not
/// rolled) and `level` the pet's current level (1 = D ... 5 = S, 0 = egg: no bonus). The sum covers every
/// level up to the current one.
pub fn pet_avail_bonus(values: &[i32], avail_levels: &[u8; 6], level: u8) -> i32 {
    let mut sum = 0_i32;
    for lvl in 1..=usize::from(level.min(5)) {
        let rolled = avail_levels.get(lvl - 1).copied().unwrap_or(0);
        if rolled == 0 {
            continue;
        }
        if let Some(v) = values.get(usize::from(rolled) - 1) {
            sum = sum.saturating_add(*v);
        }
    }
    sum
}
