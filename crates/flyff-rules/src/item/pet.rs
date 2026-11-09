//! System-pet formulas shared by the server and the browser (Plan/19 §13, §22; ref/13 §19;
//! src:_Common/pet.cpp:51-115, _Common/Mover.cpp:11416-11513, WORLDSERVER/DPSrvr.cpp:10186-10358).
//!
//! Everything here is a pure function of plain numbers: the tables of `pet.inc` / `s.txt` are
//! `flyff_data::items_ext::pet::PetTables` (their roll helpers `hatch`, `levelup_avail_level`, `add_life` and
//! `feed_energy_range` take the dice as arguments), the dice and the state live in `flyff-game`. The
//! levels are the bytes of `PETLEVEL` (`PL_EGG` 0, `PL_D` 1 ... `PL_S` 5).

pub use super::upgrade::pet_avail_bonus;

/// `MAX_PET_EGG_EXP`: the experience an egg needs to hatch (`pet.h:86`).
pub const MAX_PET_EGG_EXP: u32 = 50_000;
/// `MAX_PET_EXP`: the experience a pet of grade D to A needs to level up (`pet.h:87`).
pub const MAX_PET_EXP: u32 = 100_000;
/// `MAX_PET_LIFE`: the most life a pet can have (`pet.h`).
pub const MAX_PET_LIFE: u16 = 99;
/// `PL_EGG`.
pub const PL_EGG: u8 = 0;
/// `PL_D`.
pub const PL_D: u8 = 1;
/// `PL_C`.
pub const PL_C: u8 = 2;
/// `PL_B`.
pub const PL_B: u8 = 3;
/// `PL_A`.
pub const PL_A: u8 = 4;
/// `PL_S`.
pub const PL_S: u8 = 5;
/// `PF_PET`: the first summon of a pet (`Pet.flag`).
pub const PF_PET: u32 = 0;
/// `PF_PET_LEVEL_UP`.
pub const PF_PET_LEVEL_UP: u32 = 1;
/// `PF_PET_LEVEL_DOWN`.
pub const PF_PET_LEVEL_DOWN: u32 = 2;
/// `PF_PET_GET_AVAIL`.
pub const PF_PET_GET_AVAIL: u32 = 3;
/// The default `gameplay.pet_base_exp` (`__PET_BASE_EXP`, `VersionCommon.h`).
pub const DEFAULT_PET_BASE_EXP: f32 = 50.0;
/// The size of `PetData.avail_level` (`PL_MAX`).
pub const PET_AVAIL_SLOTS: usize = 6;

/// `CPet::GetMaxExp`: 50 000 for an egg, 100 000 for every pet.
pub fn max_exp(level: u8) -> u32 {
    if level == PL_EGG {
        MAX_PET_EGG_EXP
    } else {
        MAX_PET_EXP
    }
}

/// `MAKELONG(lo, hi)` of the `PetLevelup` / `PetCall` words: `index | level << 16`.
pub fn make_long(lo: u32, hi: u32) -> u32 {
    (lo & 0xFFFF) | (hi << 16)
}

/// The EXP multiplier of one monster kill (`AddPetExpOnKill`): 10 for a super or boss monster, and `x 1.2`
/// when the player and the monster are four or more levels apart (in either direction - KEEP, the original
/// takes `abs`). `nMulti *= 1.2` is a float times a double: the product is rounded to `f32` once.
pub fn kill_multiplier(super_or_boss: bool, player_level: i32, monster_level: i32) -> f32 {
    let mut mult: f32 = if super_or_boss { 10.0 } else { 1.0 };
    if player_level.saturating_sub(monster_level).unsigned_abs() >= 4 {
        mult = (f64::from(mult) * 1.2_f64) as f32;
    }
    mult
}

/// The per-level factor of the base EXP: egg and D 10, C 7.5, B 5, A 2.5, S 0 (`nBaseExp *= ...`, a float times
/// a double rounded once). Unknown levels behave as the original's `switch` without a case (factor 1).
pub fn kill_base_exp(base_exp: f32, level: u8) -> f32 {
    let factor = match level {
        PL_EGG | PL_D => 10.0_f64,
        PL_C => 7.5,
        PL_B => 5.0,
        PL_A => 2.5,
        PL_S => 0.0,
        _ => return base_exp,
    };
    (f64::from(base_exp) * factor) as f32
}

/// What a kill does to the pet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KillExp {
    /// The experience reached the limit of the level: `PetLevelup()` (an egg hatches).
    LevelUp,
    /// Store this experience (clamped to the maximum of the level by `SetExp`) and tell the owner.
    Set(u32),
}

/// `AddPetExpOnKill` for a pet of `level` holding `exp`: `exp' = (DWORD)(exp + base x mult)` in `f32`, then the
/// egg hatches at 50 000 and the grades D to A level up at 100 000; S gains nothing (its base is 0) but is told.
pub fn exp_after_kill(
    level: u8,
    exp: u32,
    base_exp: f32,
    super_or_boss: bool,
    player_level: i32,
    monster_level: i32,
) -> KillExp {
    let mult = kill_multiplier(super_or_boss, player_level, monster_level);
    let base = kill_base_exp(base_exp, level);
    // `(DWORD)( pPet->GetExp() + ( nBaseExp * nMulti ) )`: DWORD + float is a float sum, truncated
    let gain = base * mult;
    let sum = exp as f32 + gain;
    let total = if sum.is_nan() || sum < 0.0 {
        0
    } else {
        sum as u32
    };
    if level == PL_EGG {
        if total >= MAX_PET_EGG_EXP {
            return KillExp::LevelUp;
        }
    } else if level != PL_S && total >= MAX_PET_EXP {
        return KillExp::LevelUp;
    }
    KillExp::Set(total.min(max_exp(level)))
}

/// `CPet::SetExp`: the experience is clamped to the maximum of the level.
pub fn clamp_exp(level: u8, exp: u32) -> u32 {
    exp.min(max_exp(level))
}

/// `CPet::SetEnergy`: the energy is clamped to the maximum of the level.
pub fn clamp_energy(energy: u32, max_energy: u32) -> u16 {
    u16::try_from(energy.min(max_energy)).unwrap_or(u16::MAX)
}

/// The most feed units one `UsePetFeed` can use (`nMaxNum`): an egg `50 000 - exp`, a pet
/// `(maxEnergy - energy) / 2` (one unit gives two points of energy). Never negative (FIX, VR-025: the original
/// let a pet above its maximum use a negative count).
pub fn feed_need(level: u8, exp: u32, energy: u32, max_energy: u32) -> u32 {
    if level == PL_EGG {
        MAX_PET_EGG_EXP.saturating_sub(exp)
    } else {
        max_energy.saturating_sub(energy) / 2
    }
}

/// The units a feed stack of `stack` pieces gives to a pet that needs `need`: the whole stack, capped.
pub fn feed_units(stack: u32, need: u32) -> u32 {
    stack.min(need)
}

/// The energy of a pet after `units` of feed (`SetEnergy( energy + n * 2 )`, clamped).
pub fn energy_after_feed(energy: u32, units: u32, max_energy: u32) -> u16 {
    clamp_energy(energy.saturating_add(units.saturating_mul(2)), max_energy)
}

/// One `GetFeedEnergy` roll: a uniform draw in `[min, max + 1)` of the row's energy range; `draw(lo, hi)` is
/// `xRandom(lo, hi)`. A row that does not exist gives 0.
pub fn feed_roll(range: Option<[i32; 2]>, mut draw: impl FnMut(u32, u32) -> u32) -> u32 {
    let Some([lo, hi]) = range else { return 0 };
    let lo = u32::try_from(lo.max(0)).unwrap_or(0);
    let hi = u32::try_from(hi.max(0)).unwrap_or(0);
    draw(lo, hi.saturating_add(1))
}

/// The food mill's loop (`OnMakePetFeed`): `units` materials are ground one by one, `roll` gives the feed of
/// one unit; the loop stops before the running total would exceed `pack_max` (the pieces ground so far are the
/// ones that are consumed). Returns `(feed total, materials consumed)`.
pub fn mill(units: u32, pack_max: u32, mut roll: impl FnMut() -> u32) -> (u32, u32) {
    let mut total = 0_u32;
    for i in 0..units {
        let feed = roll();
        if total.saturating_add(feed) > pack_max {
            return (total, i);
        }
        total += feed;
    }
    (total, units)
}

/// `CPet::GetIndex`: the mover (model) of a pet - `egg_mover` for the egg, then the first of the three
/// per-kind movers for D and C, the second for B and A and the third for S. `None` when the kind has no row.
pub fn pet_mover(egg_mover: u32, movers: &[u32], level: u8) -> Option<u32> {
    if level == PL_EGG {
        return Some(egg_mover);
    }
    let slot = match level {
        PL_D | PL_C => 0,
        PL_B | PL_A => 1,
        _ => 2,
    };
    movers.get(slot).copied()
}

/// The percent of the experience bar (`GetExpPercent`, 0 when the maximum is 0).
pub fn exp_percent(level: u8, exp: u32) -> u32 {
    let max = u64::from(max_exp(level));
    u32::try_from(u64::from(exp) * 100 / max.max(1)).unwrap_or(u32::MAX)
}

/// The percent of the energy bar (`GetEnergyPercent`, 0 when the maximum is 0).
pub fn energy_percent(energy: u32, max_energy: u32) -> u32 {
    u32::try_from(u64::from(energy) * 100 / u64::from(max_energy).max(1)).unwrap_or(u32::MAX)
}
