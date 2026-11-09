//! Pets (Plan/19 §13; ref/13 §19).
//!
//! | module | content | WP |
//! |---|---|---|
//! | [`system`] | the system pet inside an egg item: summon and release, the 16-tick processing, EXP by kills, level-up, naming, the life and hatch scrolls | ITEM-16 |
//! | [`feed`] | feeding, the food mill, the tamer items, the resurrection fragment and the egg transform | ITEM-17 |
//! | [`eat`] | the eat pets: a server mover with the brain of `ai::pet`, summon and dismissal, the re-summon, the item side of the AI | ITEM-18 |
//! | [`vis`] | vis pets: the sockets, the charms and their prerequisites, the key, the transform scroll, the expiry | ITEM-18 |
//! | [`filter`] | the pet loot filter bits and `PetFilter` | ITEM-18 |
//! | [`awaken`] | the stat bonus of a summoned pet while its awakening lines are re-rolled (`UpgradeHooks.pet_options_*`) | ITEM-11 / ITEM-16 / ITEM-18 |
//!
//! The eat pets, the vis pets and the filter use the API of the system pets documented in
//! `crates/flyff-game/NOTES.md` ("Pets").

pub mod awaken;

pub mod eat;
pub mod feed;
pub mod filter;

pub mod system;
pub mod vis;
