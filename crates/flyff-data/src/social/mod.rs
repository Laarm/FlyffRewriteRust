//! The social tables (Plan/14 §6.15): honor titles, lord skills, the couple table, guild levels and buffs.
//! Loaded so far: [`guild`] (levels and buffs), [`honor`], [`lord`] (skills) and [`couple`]; the guild combat
//! settings and the lord event/election tables are WP-DATA-12.

pub mod couple;
pub mod guild;
pub mod honor;
pub mod lord;
