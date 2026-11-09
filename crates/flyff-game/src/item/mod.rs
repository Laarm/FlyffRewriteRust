//! Items of a player and of the ground (Plan/19, tasks `items-core`: WP-ITEM-01..05, 07, 26).
//!
//! | module | content | WP |
//! |---|---|---|
//! | [`model`] | `Item`, `ItemFlags`, `Element`, `PiercingSet`, `PetState`, the predicates, record/wire conversions | ITEM-01 |
//! | [`container`] | `ItemContainer` (storage + slot index + inverse), `AddPlan`, the serial sources | ITEM-02 |
//! | [`inventory`] | `PlayerItems`, `AccountBank`, `Pocket`, construction from the `CharacterLoad` | ITEM-02 |
//! | [`mod@env`], [`fx`] | the read-only environment, messages + audit lines of an operation | ITEM-03 |
//! | [`ops`] | `create_item`, `remove_item`, `update_item`, `UiParam`, `ItemOrigin` | ITEM-03 |
//! | [`gold`] | `add_gold`, the Perin converter | ITEM-03 |
//! | [`handlers`] | `MoveItemReq`, `DoEquipReq`, `RemoveInvenItem`, `DropItem` | ITEM-04 |
//! | [`usage`] | the `DoUseItem` pipeline: cast time, cooldown groups, food | ITEM-05 |
//! | [`mod@drop`] | monster drop generation: factors, tables, `DropKind`, the stat-option table, the seams | ITEM-06 |
//! | [`loot`] | loot rights, pickup, the multiple item picker, the pet-loot API | ITEM-07 |
//! | [`tick`] | the per-tick job: due casts, deferred serials | ITEM-05 |
//! | [`pets`] | system pets, feeding, the food mill, tamer items, egg transform | ITEM-16, ITEM-17 |
//!
//! The equipment rule engine is `stats::equip` (doc 16, WP-MOVER-11); the pure rules are in `flyff-rules`
//! (`item`, `equip_rules`).

pub mod buffitems;
mod cloak;
pub mod collect;
pub mod container;
pub mod drop;
pub mod env;
pub mod expiry;

pub mod fx;
pub mod gold;
pub mod handlers;
pub mod inventory;
pub mod loot;
pub mod model;
pub mod ops;
pub mod pets;
pub mod piercing;
pub mod pockets;

pub mod summon;

pub mod tick;
pub mod ticket;

pub mod ultimate;
pub mod upgrade;
pub mod usage;
pub mod wire;

pub use cloak::remove_guild_cloaks;
pub use container::{
    AddError, AddPlan, ContainerKind, DeferSerials, ItemContainer, NoSerials, SeqSerials,
    SerialSource, SlotIdx, StorageId, SwapOutcome,
};
pub use env::{ItemConfig, ItemCore, ItemEnv, ItemService};
pub use fx::{AuditLine, Fx};
pub use inventory::{AccountBank, CastPending, CooldownTimers, PlayerItems, Pocket};
pub use loot::{GroundItemMeta, ItemArrival, LootResult};
pub use model::{Element, Item, ItemFlags, PetLevel, PetState, PiercingSet};
pub use ops::{ItemError, ItemOrigin, RemoveReason, UiParam};
/// The plan's module names for the angel and the monster-summon handler (Plan/19 §1.3, §5.10).
pub use summon::{angel, create_monster};
