//! Buffs and abnormal states (Plan/17 §7, WP-SKILL-01/02/03).
//!
//! | module | content |
//! |---|---|
//! | [`set`] | `BuffSet`, `BuffKey`, `BuffEntry`: the container, the add / refresh / replace / 28-cap planner (KQ-06) |
//! | [`core`] | the mover-level operations: `add`, `process`, release with the exact DST revert, removal helpers, weapon invalidation |
//! | [`api`] | the public API of 17 §7.13 (`apply_item`, `apply_skill`, `has_*`, `remove_*`, ...) |
//! | [`status`] | `StatusEffects` and the `Set*` functions of the abnormal states (the per-tick work is WP-SKILL-07) |
//! | [`persist`] | the `buffs` document: snapshot, load with the 60-minute cap, the KQ-17 strip |
//! | [`npc`] | `on_npc_buff` and the class buff table |
//! | [`tick`] | the per-tick pass over the movers with buffs |
//!
//! The effects of a buff are DST operations under `DstSource::Buff { kind, id }`; the application of a skill
//! or item is `skill::apply`.

pub mod api;
pub mod core;
pub mod npc;
pub mod persist;
pub mod set;
pub mod status;

pub mod tick;

pub use self::core::{BuffCx, RemoveFlags};
pub use api::{
    BuffRefusal, add, apply_item, apply_skill, find_ik3, has, has_ik3, has_item, has_skill, remove,
    remove_all, remove_by_ik3, remove_common, remove_debufs, remove_flags, remove_item,
    remove_skill,
};
pub use set::{AddPlan, AddRequest, Buff, BuffEntry, BuffKey, BuffKind, BuffSet};
pub use status::{Dot, StatusEffects};

/// `BUFF_ITEM` as the legacy `WORD`.
pub const BUFF_ITEM: u16 = 0;
/// `BUFF_SKILL`.
pub const BUFF_SKILL: u16 = 1;
/// `BUFF_PET`.
pub const BUFF_PET: u16 = 2;
/// `BUFF_ITEM2` (the only kind with an absolute expiry).
pub const BUFF_ITEM2: u16 = 3;
/// `BUFF_EQUIP`.
pub const BUFF_EQUIP: u16 = 4;
