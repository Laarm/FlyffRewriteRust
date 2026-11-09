//! The data side of a system pet (Plan/19 §2.4, §13; src:_Common/pet.cpp:294-425): the methods of `CPet` that
//! need the pet tables of `pet.inc` / `s.txt` (`GameData.pet`).
//!
//! `PetState` itself is `item::model::PetState`; these are inherent methods added here so that the model
//! file stays free of table lookups.

use crate::item::model::{PetLevel, PetState};
use flyff_common::defs::item::II_PET_EGG;
use flyff_common::defs::obj::MI_PET_EGG;
use flyff_data::items_ext::pet::PetTables;
use flyff_proto::pb;
use flyff_rules::item::pet::{self, MAX_PET_EGG_EXP, MAX_PET_EXP, PET_AVAIL_SLOTS};

/// `MAX_PET_NAME - 1`: the most bytes of a pet name (`CPet::SetName`).
pub const MAX_PET_NAME_BYTES: usize = 16;

impl PetState {
    /// `CPet::InitEgg` on a fresh `CPet`: an egg of kind 0 with life 1 and the energy 1 of the egg grade.
    pub fn new_egg() -> PetState {
        PetState {
            kind: 0,
            level: PetLevel::Egg,
            exp: 0,
            energy: 1,
            life: 1,
            avail_levels: [0; PET_AVAIL_SLOTS],
            name: String::new(),
        }
    }

    /// The level as the `PL_*` byte.
    pub fn level_u8(&self) -> u8 {
        self.level as u8
    }

    /// `CPet::GetMaxEnergy`: `MaxEnergy` of the grade D to S, 1 for the egg.
    pub fn max_energy(&self, tables: &PetTables) -> u16 {
        u16::try_from(tables.max_energy(self.level_u8())).unwrap_or(u16::MAX)
    }

    /// `CPet::GetMaxExp`: 50 000 for the egg, 100 000 for the grades.
    pub fn max_exp(&self) -> u32 {
        if self.level == PetLevel::Egg {
            MAX_PET_EGG_EXP
        } else {
            MAX_PET_EXP
        }
    }

    /// `CPet::GetLife`: what the owner is told. With `features.no_pet_life` the life reads 1, else an egg reads 0.
    pub fn reported_life(&self, no_pet_life: bool) -> u16 {
        if no_pet_life {
            1
        } else if self.level == PetLevel::Egg {
            0
        } else {
            self.life
        }
    }

    /// `CPet::SetEnergy`: clamped to the maximum of the grade.
    pub fn set_energy(&mut self, tables: &PetTables, energy: u32) {
        self.energy = pet::clamp_energy(energy, u32::from(self.max_energy(tables)));
    }

    /// `CPet::SetExp`: clamped to the maximum of the grade.
    pub fn set_exp(&mut self, exp: u32) {
        self.exp = pet::clamp_exp(self.level_u8(), exp);
    }

    /// `CPet::GetItemId`: `II_PET_EGG` for the egg, else the item of the kind (0 when the kind has no row).
    pub fn item_id(&self, tables: &PetTables) -> u32 {
        if self.level == PetLevel::Egg {
            II_PET_EGG
        } else {
            tables.avail_param(self.kind).map_or(0, |a| a.item)
        }
    }

    /// `CPet::GetIndex`: the model of the pet (`MI_PET_EGG` for the egg; 0 when the kind has no row).
    pub fn mover_index(&self, tables: &PetTables) -> u32 {
        if self.level == PetLevel::Egg {
            return MI_PET_EGG;
        }
        tables
            .avail_param(self.kind)
            .and_then(|a| pet::pet_mover(MI_PET_EGG, &a.movers, self.level_u8()))
            .unwrap_or(0)
    }

    /// `CPet::GetAvailDestParam`: the `DST_*` of the kind and the sum of the rolled values of the grades D up to
    /// the current one. A kind without a row gives `(0, 0)`; an egg gives its kind's DST with a bonus of 0.
    pub fn avail_dst(&self, tables: &PetTables) -> (u32, i32) {
        match tables.avail_param(self.kind) {
            Some(a) => (
                a.dst,
                pet::pet_avail_bonus(&a.values, &self.avail_levels, self.level_u8()),
            ),
            None => (0, 0),
        }
    }

    /// `MAKELONG(GetIndex(), GetLevel())`: the word of `PetLevelup`.
    pub fn levelup_word(&self, tables: &PetTables) -> u32 {
        pet::make_long(self.mover_index(tables), u32::from(self.level_u8()))
    }

    /// `CPet::SetName`: at most 16 bytes, cut at a character boundary.
    pub fn set_name(&mut self, name: &str) {
        self.name = cut_name(name).to_owned();
    }

    /// The wire form (`CPet::Serialize`): the stored fields, not the reported life.
    pub fn to_pb(&self) -> pb::PetData {
        pb::PetData {
            kind: u32::from(self.kind),
            level: u32::from(self.level_u8()),
            exp: self.exp,
            energy: u32::from(self.energy),
            life: u32::from(self.life),
            avail_level: self.avail_levels.iter().map(|&v| u32::from(v)).collect(),
            name: self.name.clone(),
        }
    }
}

/// `name` cut to [`MAX_PET_NAME_BYTES`] bytes at a character boundary.
pub fn cut_name(name: &str) -> &str {
    cut_name_to(name, MAX_PET_NAME_BYTES)
}

/// `text` cut to at most `max` bytes at a character boundary.
pub fn cut_name_to(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or("")
}
