//! `flyff-rules` — tier S (Plan/16): pure formulas (DST engine, derived stats, damage/exp/drop math,
//! prices, upgrade odds, tooltip numbers) shared by the server and the browser (via `flyff-wasm`).
//! RNG is injected; no clocks, no I/O.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented
)]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]
#![deny(clippy::iter_over_hash_type)]
#![warn(missing_docs)]

pub mod appearance;
pub mod buff;
pub mod chat;
pub mod combat;
pub mod consts;
pub mod data;
pub mod dst;
pub mod equip_rules;
pub mod exp_share;
pub mod item;
pub mod jobs;
pub mod names;
pub mod pin;
pub mod progression;
pub mod regen;
pub mod sets;
pub mod skill;
pub mod skill_points;
pub mod sm;
pub mod social;
pub mod stats;

pub use data::{JobRow, RulesData};
pub use dst::{COMPOSITES, DstTable, OriginMax, SetOutcome, expand_composite};
pub use stats::{
    BaseStats, DerivedStats, EquipCache, MonsterRates, StatInputs, Statistics, UPGRADE_FLAT,
    WeaponView, derived_stats,
};

/// The projection of the runtime configuration the rules read (Plan/16 §5.2). The server converts the
/// `[features]` / `[gameplay]` sections with [`RulesConfig::from_config`]; the browser receives the same values
/// through the client feature bundle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RulesConfig {
    /// `features.bowdmg_propjob`.
    pub bowdmg_propjob: bool,
    /// `features.wand_autoattacks`.
    pub wand_autoattacks: bool,
    /// `features.better_armor_bonus`.
    pub better_armor_bonus: bool,
    /// `features.auto_skill_max`.
    pub auto_skill_max: bool,
    /// `features.fix_get_hp_mp_fp_point`.
    pub fix_get_hp_mp_fp_point: bool,
    /// `features.remove_max_recover_rate`.
    pub remove_max_recover_rate: bool,
    /// `features.heal_over_time`.
    pub heal_over_time: bool,
    /// `gameplay.set_max_speed`.
    pub set_max_speed: i32,
    /// `gameplay.set_max_cast`.
    pub set_max_cast: i32,
    /// `gameplay.set_max_crit`.
    pub set_max_crit: i32,
    /// `gameplay.max_block_value`.
    pub max_block_value: i32,
    /// `gameplay.max_block_value_gw`.
    pub max_block_value_gw: i32,
}

impl Default for RulesConfig {
    /// The repack defaults (Plan/03 §4: every switch on, caps 150 / 100 / 100 / 75 / 75).
    fn default() -> Self {
        RulesConfig {
            bowdmg_propjob: true,
            wand_autoattacks: true,
            better_armor_bonus: true,
            auto_skill_max: true,
            fix_get_hp_mp_fp_point: true,
            remove_max_recover_rate: true,
            heal_over_time: true,
            set_max_speed: 150,
            set_max_cast: 100,
            set_max_crit: 100,
            max_block_value: 75,
            max_block_value_gw: 75,
        }
    }
}

impl RulesConfig {
    /// The conversion from the configuration registry (`flyff_common::config`, WP-CORE-90). Caps above
    /// `i32::MAX` saturate.
    pub fn from_config(cfg: &flyff_common::config::RulesConfig) -> RulesConfig {
        let cap = |v: u32| i32::try_from(v).unwrap_or(i32::MAX);
        RulesConfig {
            bowdmg_propjob: cfg.features.bowdmg_propjob,
            wand_autoattacks: cfg.features.wand_autoattacks,
            better_armor_bonus: cfg.features.better_armor_bonus,
            auto_skill_max: cfg.features.auto_skill_max,
            fix_get_hp_mp_fp_point: cfg.features.fix_get_hp_mp_fp_point,
            remove_max_recover_rate: cfg.features.remove_max_recover_rate,
            heal_over_time: cfg.features.heal_over_time,
            set_max_speed: cap(cfg.gameplay.set_max_speed),
            set_max_cast: cap(cfg.gameplay.set_max_cast),
            set_max_crit: cap(cfg.gameplay.set_max_crit),
            max_block_value: cap(cfg.gameplay.max_block_value),
            max_block_value_gw: cap(cfg.gameplay.max_block_value_gw),
        }
    }
}

/// Crate marker used by the workspace smoke test and by `cargo xtask doctor`.
pub const CRATE_NAME: &str = "flyff-rules";
