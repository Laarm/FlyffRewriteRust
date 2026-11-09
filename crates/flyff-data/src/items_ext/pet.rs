//! `pet.inc`: the pet tables (`CPetProperty::LoadScript`, Plan/14 §6.13, ref/17 §8.7; src:_Common/pet.cpp:124-259).
//!
//! ```text
//! AvailDestParam '{' (DST P1 .. P9 ITEM MOVER1 MOVER2 MOVER3 PROB)* '}'     bonus per pet level and kind
//! LevelupAvail '{' (PROB{9})* '}'
//! FeedEnergy N '{' (COSTMIN COSTMAX ENERGYMIN ENERGYMAX)* '}'               N = 0 (NPC), 1 (item)
//! IncrementExp '{' NUM* '}'   MaxEnergy '{' NUM* '}'   AddLifeProbability '{' NUM* '}'
//! Penalty '{' (EXP% ENERGY)* '}'
//! ```
//!
//! The client shows the bonus table, the maximum energy and the feed energy; the other blocks are read and
//! kept (raw), `s.txt` overrides some of them on the server (WP-DATA-11).

use serde::{Deserialize, Serialize};

/// `MAX_PET_AVAIL_LEVEL`.
pub const MAX_PET_AVAIL_LEVEL: usize = 9;

/// One row of `AvailDestParam`: the bonus of a pet kind.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PetAvailParam {
    /// `DST_*`.
    pub dst: u32,
    /// The bonus at each of the nine levels.
    pub values: Vec<i32>,
    /// The pet egg item of the kind.
    pub item: u32,
    /// The three mover ids (the three growth stages).
    pub movers: Vec<u32>,
    /// The hatch probability (server-side).
    pub prob: u32,
}

/// One row of `Penalty`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PetPenalty {
    /// The experience percentage lost.
    pub exp: f32,
    /// The energy lost.
    pub energy: u32,
}

/// The pet tables.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PetTables {
    /// `AvailDestParam` rows.
    pub avail: Vec<PetAvailParam>,
    /// `LevelupAvail` rows of nine probabilities (the first row is pet level D).
    pub levelup_avail: Vec<Vec<u32>>,
    /// `FeedEnergy 0` and `FeedEnergy 1`: `[cost min, cost max, energy min, energy max]` rows.
    pub feed_energy: [Vec<[i32; 4]>; 2],
    /// `IncrementExp` per pet level (D, C, B, A).
    pub increment_exp: Vec<u32>,
    /// `MaxEnergy` per pet level (D, C, B, A, S).
    pub max_energy: Vec<u32>,
    /// `AddLifeProbability`.
    pub add_life_probability: Vec<u32>,
    /// `Penalty` rows per pet level.
    pub penalty: Vec<PetPenalty>,
}

/// `PK_MAX`: the number of pet kinds (tiger, lion, rabbit, fox, dragon, griffin, unicorn).
pub const PK_MAX: usize = 7;
/// `PL_D`..`PL_S`: the pet levels D, C, B, A, S are 1..=5 (`PL_EGG` is 0).
pub const PL_S: u8 = 5;

impl PetTables {
    /// `GetAvailParam( kind )`: the bonus row of a pet kind (`PK_TIGER`..`PK_UNICORN` = 0..6).
    pub fn avail_param(&self, kind: u8) -> Option<&PetAvailParam> {
        self.avail
            .get(usize::from(kind))
            .filter(|_| usize::from(kind) < PK_MAX)
    }

    /// `GetMaxEnergy( level )`: D..S read `MaxEnergy`, anything else (the egg) is 1.
    pub fn max_energy(&self, level: u8) -> u32 {
        if (1..=PL_S).contains(&level) {
            self.max_energy
                .get(usize::from(level) - 1)
                .copied()
                .unwrap_or(0)
        } else {
            1
        }
    }

    /// `GetIncrementExp( level )`: the experience per minute (out of 100,000) for D..A, else 0.
    pub fn increment_exp(&self, level: u8) -> u32 {
        if (1..=4).contains(&level) {
            self.increment_exp
                .get(usize::from(level) - 1)
                .copied()
                .unwrap_or(0)
        } else {
            0
        }
    }

    /// `GetPenalty( level )`: the release penalty of D..S (`None` for the egg or above S).
    pub fn penalty(&self, level: u8) -> Option<&PetPenalty> {
        if (1..=PL_S).contains(&level) {
            self.penalty.get(usize::from(level) - 1)
        } else {
            None
        }
    }

    /// The nine `LevelupAvail` probabilities of a pet level (D..S; `None` otherwise).
    pub fn levelup_avail_row(&self, level: u8) -> Option<&[u32]> {
        if (1..=PL_S).contains(&level) {
            self.levelup_avail
                .get(usize::from(level) - 1)
                .map(Vec::as_slice)
        } else {
            None
        }
    }

    /// `m_anLevelupAvailLevelMax[level]`: the highest index `i >= 1` with a non-zero probability (0 if none).
    pub fn levelup_avail_level_max(&self, level: u8) -> usize {
        self.levelup_avail_row(level).map_or(0, |row| {
            row.iter()
                .enumerate()
                .skip(1)
                .filter(|(_, p)| **p > 0)
                .map(|(i, _)| i)
                .next_back()
                .unwrap_or(0)
        })
    }

    /// `GetLevelupAvailLevel( level )` for a roll in `1..=10_000`: the avail level (1..=9) gained, 0 if the
    /// cumulative sum never reaches the roll.
    pub fn levelup_avail_level(&self, level: u8, roll: u32) -> u8 {
        let Some(row) = self.levelup_avail_row(level) else {
            return 0;
        };
        let mut total = 0_u32;
        for i in 0..=self.levelup_avail_level_max(level) {
            total = total.wrapping_add(row.get(i).copied().unwrap_or(0));
            if total >= roll {
                return u8::try_from(i + 1).unwrap_or(0);
            }
        }
        0
    }

    /// `Hatch()` for a roll in `1..=10_000`: the kind whose cumulative hatch weight reaches the roll (0 on the
    /// original's "error" path).
    pub fn hatch(&self, roll: u32) -> u8 {
        let mut total = 0_u32;
        for (kind, a) in self.avail.iter().enumerate().take(PK_MAX) {
            total = total.wrapping_add(a.prob);
            if total >= roll {
                return u8::try_from(kind).unwrap_or(0);
            }
        }
        0
    }

    /// `GetAddLife()` for a roll in `1..=100`: the life added (`1..=n`), 1 when the table is exhausted.
    pub fn add_life(&self, roll: u32) -> u32 {
        let mut total = 0_u32;
        for (i, p) in self.add_life_probability.iter().enumerate() {
            total = total.wrapping_add(*p);
            if total >= roll {
                return u32::try_from(i + 1).unwrap_or(1);
            }
        }
        1
    }

    /// The `[energy min, energy max]` range of a feed-energy table (`0` NPC mill, `1` feed-maker tool) for a
    /// material cost; the caller picks `xRandom( min, max + 1 )` (`GetFeedEnergy`).
    pub fn feed_energy_range(&self, cost: u32, table: usize) -> Option<[i32; 2]> {
        let rows = self.feed_energy.get(table)?;
        let cost = i64::from(cost);
        rows.iter()
            .find(|r| cost >= i64::from(r[0]) && cost <= i64::from(r[1]))
            .map(|r| [r[2], r[3]])
    }
}

#[cfg(feature = "load")]
pub(crate) use parse::parse_pet;

#[cfg(feature = "load")]
mod parse {
    use super::{MAX_PET_AVAIL_LEVEL, PetAvailParam, PetPenalty, PetTables};
    use flyff_script::{DiagCode, Diagnostics, ScannerOptions, Script, ScriptCtx, Severity};
    use std::sync::Arc;

    fn open(sc: &Script<'_>) -> bool {
        !sc.token().starts_with('}') && !sc.finished()
    }

    /// `CPetProperty::LoadScript` over decoded `text`.
    pub(crate) fn parse_pet(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> PetTables {
        let mut t = PetTables::default();
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        sc.get_token();
        while !sc.finished() {
            match sc.token().to_owned().as_str() {
                "AvailDestParam" => {
                    sc.get_token(); // {
                    let mut dst = sc.get_number();
                    while open(&sc) {
                        let mut values = Vec::with_capacity(MAX_PET_AVAIL_LEVEL);
                        for _ in 0..MAX_PET_AVAIL_LEVEL {
                            values.push(sc.get_number_i32());
                        }
                        let item = sc.get_number();
                        let movers = vec![sc.get_number(), sc.get_number(), sc.get_number()];
                        let prob = sc.get_number();
                        t.avail.push(PetAvailParam {
                            dst,
                            values,
                            item,
                            movers,
                            prob,
                        });
                        dst = sc.get_number();
                    }
                }
                "LevelupAvail" => {
                    sc.get_token(); // {
                    let mut first = sc.get_number();
                    while open(&sc) {
                        let mut row = vec![first];
                        for _ in 1..MAX_PET_AVAIL_LEVEL {
                            row.push(sc.get_number());
                        }
                        t.levelup_avail.push(row);
                        first = sc.get_number();
                    }
                }
                "FeedEnergy" => {
                    let index = sc.get_number();
                    sc.get_token(); // {
                    let mut rows = Vec::new();
                    let mut min = sc.get_number_i32();
                    while open(&sc) {
                        rows.push([
                            min,
                            sc.get_number_i32(),
                            sc.get_number_i32(),
                            sc.get_number_i32(),
                        ]);
                        min = sc.get_number_i32();
                    }
                    match t.feed_energy.get_mut(index as usize) {
                        Some(slot) => *slot = rows,
                        None => sc.report(
                            Severity::Error,
                            DiagCode::IndexOutOfRange,
                            format!("pet.inc: FeedEnergy {index} is not 0 or 1; ignored"),
                        ),
                    }
                }
                "IncrementExp" | "MaxEnergy" | "AddLifeProbability" => {
                    let which = sc.token().to_owned();
                    sc.get_token(); // {
                    let mut list = Vec::new();
                    let mut v = sc.get_number();
                    while open(&sc) {
                        list.push(v);
                        v = sc.get_number();
                    }
                    match which.as_str() {
                        "IncrementExp" => t.increment_exp = list,
                        "MaxEnergy" => t.max_energy = list,
                        _ => t.add_life_probability = list,
                    }
                }
                "Penalty" => {
                    sc.get_token(); // {
                    let mut exp = sc.get_float();
                    while open(&sc) {
                        t.penalty.push(PetPenalty {
                            exp,
                            energy: sc.get_number(),
                        });
                        exp = sc.get_float();
                    }
                }
                _ => {}
            }
            sc.get_token();
        }
        t
    }
}
