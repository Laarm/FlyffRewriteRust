//! Pet rule functions for the browser (Plan/19 §13, §22; Plan/45 §18.7, WP-ITEM-16/17): FREE `#[wasm_bindgen]`
//! functions over `flyff_rules::item::pet`, so the pet status window, the feed confirmation and the tooltips use
//! the very numbers the server computes. They need no bundle and no `RulesContext`: the tables (the `pet` part of
//! the `item-ext` bundle) are passed as plain arrays.
//!
//! * [`pet_avail_bonus`]`(values, availLevels, level) -> number`: the stat bonus of a pet, the sum of the rolled
//!   values of the grades D up to `level` (`values` = the kind's nine values);
//! * [`pet_max_exp`], [`pet_exp_percent`], [`pet_energy_percent`]: the bars of the status window;
//! * [`pet_kill_exp`]`(level, exp, baseExp, superOrBoss, playerLevel, monsterLevel) -> [kind, value]`: `kind` 0 = the
//!   new experience `value`, 1 = the pet levels up (an egg hatches), `value` 0;
//! * [`pet_feed_units`]`(level, exp, energy, maxEnergy, stack) -> number`: how many pieces of feed one `UsePetFeed`
//!   uses;
//! * [`pet_mover`]`(eggMover, movers, level) -> number`: the model of the pet, -1 without a row;
//! * [`pet_levelup_word`]`(mover, level) -> number`: `MAKELONG( mover, level )` of `PetLevelup` / `PlayerOther.pet_id`.
//!
//! Nothing here panics; the levels are clamped to `0..=5` where a byte is expected.

use flyff_rules::item::pet;
use wasm_bindgen::prelude::wasm_bindgen;

/// A `PL_*` level from a JavaScript number (anything above `PL_S` reads as S).
fn level_of(level: u32) -> u8 {
    u8::try_from(level.min(u32::from(pet::PL_S))).unwrap_or(pet::PL_S)
}

/// The six stat rolls of a pet from a JavaScript array (missing entries are 0, extra ones ignored).
fn rolls_of(avail_levels: &[u8]) -> [u8; pet::PET_AVAIL_SLOTS] {
    let mut out = [0_u8; pet::PET_AVAIL_SLOTS];
    for (slot, v) in out.iter_mut().zip(avail_levels) {
        *slot = *v;
    }
    out
}

/// The stat bonus of a pet: the sum of the rolled values (`values` = the nine values of its kind, `avail_levels`
/// = the roll per grade D..S) of the grades D up to `level`.
#[wasm_bindgen(js_name = pet_avail_bonus)]
pub fn pet_avail_bonus(values: &[i32], avail_levels: &[u8], level: u32) -> i32 {
    pet::pet_avail_bonus(values, &rolls_of(avail_levels), level_of(level))
}

/// `CPet::GetMaxExp`: 50 000 for the egg, 100 000 for the grades.
#[wasm_bindgen(js_name = pet_max_exp)]
pub fn pet_max_exp(level: u32) -> u32 {
    pet::max_exp(level_of(level))
}

/// `CPet::GetExpPercent`.
#[wasm_bindgen(js_name = pet_exp_percent)]
pub fn pet_exp_percent(level: u32, exp: u32) -> u32 {
    pet::exp_percent(level_of(level), exp)
}

/// `CPet::GetEnergyPercent`.
#[wasm_bindgen(js_name = pet_energy_percent)]
pub fn pet_energy_percent(energy: u32, max_energy: u32) -> u32 {
    pet::energy_percent(energy, max_energy)
}

/// The core of [`pet_kill_exp`].
pub fn kill_exp_core(
    level: u32,
    exp: u32,
    base_exp: f32,
    super_or_boss: bool,
    player_level: i32,
    monster_level: i32,
) -> [u32; 2] {
    match pet::exp_after_kill(
        level_of(level),
        exp,
        base_exp,
        super_or_boss,
        player_level,
        monster_level,
    ) {
        pet::KillExp::LevelUp => [1, 0],
        pet::KillExp::Set(v) => [0, v],
    }
}

/// `AddPetExpOnKill`: `[0, new exp]` or `[1, 0]` (levels up).
#[wasm_bindgen(js_name = pet_kill_exp)]
pub fn pet_kill_exp(
    level: u32,
    exp: u32,
    base_exp: f32,
    super_or_boss: bool,
    player_level: i32,
    monster_level: i32,
) -> Vec<u32> {
    kill_exp_core(
        level,
        exp,
        base_exp,
        super_or_boss,
        player_level,
        monster_level,
    )
    .to_vec()
}

/// The pieces of feed one `UsePetFeed` uses: the whole stack, capped by what the pet needs.
#[wasm_bindgen(js_name = pet_feed_units)]
pub fn pet_feed_units(level: u32, exp: u32, energy: u32, max_energy: u32, stack: u32) -> u32 {
    pet::feed_units(
        stack,
        pet::feed_need(level_of(level), exp, energy, max_energy),
    )
}

/// `CPet::GetIndex`: the model of the pet for `level` (-1 when the kind has no mover row).
#[wasm_bindgen(js_name = pet_mover)]
pub fn pet_mover(egg_mover: u32, movers: &[u32], level: u32) -> i32 {
    pet::pet_mover(egg_mover, movers, level_of(level))
        .map_or(-1, |m| i32::try_from(m).unwrap_or(-1))
}

/// `MAKELONG( mover, level )`.
#[wasm_bindgen(js_name = pet_levelup_word)]
pub fn pet_levelup_word(mover: u32, level: u32) -> u32 {
    pet::make_long(mover, level)
}
