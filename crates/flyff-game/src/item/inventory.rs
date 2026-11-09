//! The per-player item state (Plan/19 §3.3, WP-ITEM-02): the inventory with its equipment slots, the account
//! banks, the pockets, the wallet and the runtime-only item state (cast, cooldowns, pet and ticket handles).
//!
//! `PlayerExt.items` is a [`PlayerItems`] (doc 15/16 keep the field name). The persisted documents are
//! unchanged: [`PlayerItems::from_load`] builds the runtime containers from the `CharacterLoad` and
//! [`PlayerItems::inventory_doc`] / [`PlayerItems::pockets_doc`] / [`PlayerItems::bank_snapshots`] write them
//! back for the snapshot (reserved items are saved as ordinary items, 13 §17.3 rule 4).

use super::container::{ContainerKind, ItemContainer, StorageId};
use flyff_common::{CharacterId, GameMs, ObjId};
use flyff_db::{
    BankDoc, BankLoad, BankSnapshot, CharacterLoad, InventoryDoc, ItemRecord, PocketsDoc,
};
use flyff_rules::consts::MAX_HUMAN_PARTS;
use std::collections::BTreeMap;

/// `MAX_INVENTORY` with `features.inventory_168`.
pub const BAG_SLOTS_168: u16 = 168;
/// `MAX_INVENTORY` without it (the vanilla 42).
pub const BAG_SLOTS_42: u16 = 42;
/// `MAX_BANK`: slots of one bank.
pub const BANK_SLOTS: u16 = 42;
/// Accounts have at most this many characters, and so many banks and pockets.
pub const ACCOUNT_SLOTS: usize = 3;

/// A bank of the account, owned by the character in `slot` (`m_idPlayerBank[slot]`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountBank {
    /// The owner of this bank.
    pub character_id: CharacterId,
    /// The owner's slot `0..3`.
    pub slot: u8,
    /// `banks.pin`: four ASCII digits.
    pub pin: [u8; 4],
    /// `banks.gold`.
    pub gold: i64,
    /// The 42 slots.
    pub items: ItemContainer,
}

/// One pocket (`CPocket`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pocket {
    /// The pocket can be used.
    pub available: bool,
    /// `bExpired`.
    pub expired: bool,
    /// `tExpirationDate` (absolute unix seconds), 0 = none.
    pub expires_at: i64,
    /// The pocket's slots.
    pub items: ItemContainer,
}

/// The cooldown end of the three groups (19 §5.2; not persisted).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CooldownTimers {
    /// `until[g - 1]`: the group may be used again once `now > until`.
    pub until: [GameMs; 3],
}

/// A use that waits for its cast time (19 §5.2, `m_nReadyTime`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CastPending {
    /// The game time the use fires.
    pub ready_at: GameMs,
    /// The item being used (`m_dwUseItemId` with `m_bItemFind`).
    pub obj_id: StorageId,
    /// The item prop of a virtual use (the escape wing; `m_bItemFind == FALSE`).
    pub virtual_item: Option<u32>,
    /// The scheduler id of the due-timer, to cancel it.
    pub timer: Option<crate::sched::TimerId>,
}

/// The set bonuses currently applied to the player (16 §13.5; runtime only).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SetState {
    /// The `n` of the applied armour `+N` bonus (0 = none).
    pub armor_n: i32,
    /// Applied named sets: set id -> worn piece count.
    pub named: BTreeMap<u32, usize>,
}

/// Inventory, banks, pockets and gold of a player (19 §3.3).
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerItems {
    /// The inventory: the bag grid plus the 31 equipment slots (`slot 168 + part`).
    pub inventory: ItemContainer,
    /// The three pockets.
    pub pockets: [Pocket; ACCOUNT_SLOTS],
    /// The banks of the account's active characters by character slot.
    pub banks: [Option<AccountBank>; ACCOUNT_SLOTS],
    /// `m_dwGold`, `0..=i32::MAX` (`characters.gold` is an `i64` column the game clamps).
    pub gold: i32,
    /// Storage id of the summoned system pet item (`m_dwPetId`); `None` = `NULL_ID`.
    pub pet_obj_id: Option<u16>,
    /// The summoned eat pet: its item and the spawned mover (runtime).
    pub summoned_eat_pet: Option<(StorageId, ObjId)>,
    /// `savedEatPetId`: the eat pet item a dismissal wants summoned again (`features.reactivate_eatpet`; runtime,
    /// used by `item::pets::eat::tick`).
    pub saved_eat_pet: Option<StorageId>,
    /// The active ticket item (runtime).
    pub active_ticket: Option<StorageId>,
    /// The active timed feed pocket item (runtime).
    pub active_feed_pocket: Option<StorageId>,
    /// The cooldown groups.
    pub cooldowns: CooldownTimers,
    /// The pending cast.
    pub cast: Option<CastPending>,
    /// `PETFILTER` bits (client resends after login; not persisted).
    pub pet_filter: u32,
    /// `m_szInput`: the text of a `DoUseItemInput` while its item is being used (`SetInput` / `ResetInput`,
    /// 19 §5.9); empty otherwise. Runtime only.
    pub input: String,
    /// The applied set bonuses.
    pub set_state: SetState,
    /// Some inventory item was created with serial 0 and waits for the item job to number it.
    pub serial_pending: bool,
    /// The pacing of the upgrade attempts (PL-076; runtime only).
    pub upgrade_limiter: super::upgrade::UpgradeLimiter,
    /// The collecting counter and the spot it started on (Plan/19 §14; runtime only).
    pub collect: super::collect::CollectState,
}

impl Default for PlayerItems {
    /// An empty player with the 168-slot inventory and the permanent 18/42/42 pockets.
    fn default() -> Self {
        PlayerItems::empty(BAG_SLOTS_168, PocketsDoc::CAPACITIES_BIG)
    }
}

impl PlayerItems {
    /// An empty player: `bag_slots` bag slots, three available pockets of the given capacities.
    pub fn empty(bag_slots: u16, pocket_capacities: [u16; 3]) -> PlayerItems {
        let pockets = std::array::from_fn(|i| Pocket {
            available: true,
            expired: false,
            expires_at: 0,
            items: ItemContainer::new(
                ContainerKind::Pocket(i as u8),
                pocket_capacities.get(i).copied().unwrap_or(0),
                0,
            ),
        });
        PlayerItems {
            inventory: ItemContainer::new(
                ContainerKind::Inventory,
                bag_slots,
                MAX_HUMAN_PARTS as u16,
            ),
            pockets,
            banks: [None, None, None],
            gold: 0,
            pet_obj_id: None,
            summoned_eat_pet: None,
            saved_eat_pet: None,
            active_ticket: None,
            active_feed_pocket: None,
            cooldowns: CooldownTimers::default(),
            cast: None,
            pet_filter: 0,
            input: String::new(),
            set_state: SetState::default(),
            serial_pending: false,
            upgrade_limiter: super::upgrade::UpgradeLimiter::default(),
            collect: super::collect::CollectState::default(),
        }
    }

    /// The runtime state of a loaded character: the inventory (its capacity decides 168 / 42 bag slots; an
    /// empty document gets the 168-slot default), the pockets, the banks of the account, the gold. Items that
    /// could not be placed by the container repair are returned (the caller logs / mails them).
    pub fn from_load(load: &CharacterLoad, gold: i32) -> (PlayerItems, Vec<ItemRecord>) {
        let mut orphans = Vec::new();
        let docs = &load.docs;
        let cap = docs.inventory.container.capacity;
        let extra = MAX_HUMAN_PARTS as u16;
        let inventory = if cap == 0 {
            ItemContainer::new(ContainerKind::Inventory, BAG_SLOTS_168, extra)
        } else {
            let (c, o) = ItemContainer::from_record(
                &docs.inventory.container,
                ContainerKind::Inventory,
                cap.saturating_sub(extra),
                extra,
            );
            orphans.extend(o);
            c
        };
        let pockets = std::array::from_fn(|i| {
            let rec = docs.pockets.pockets.get(i).cloned().unwrap_or_default();
            let default_cap = PocketsDoc::CAPACITIES_BIG.get(i).copied().unwrap_or(0);
            let (items, available) = if rec.container.capacity == 0 {
                (
                    ItemContainer::new(ContainerKind::Pocket(i as u8), default_cap, 0),
                    true,
                )
            } else {
                let (c, o) = ItemContainer::from_record(
                    &rec.container,
                    ContainerKind::Pocket(i as u8),
                    rec.container.capacity,
                    0,
                );
                orphans.extend(o);
                (c, rec.available)
            };
            Pocket {
                available,
                expired: rec.expired,
                expires_at: rec.expires_at,
                items,
            }
        });
        let mut banks: [Option<AccountBank>; ACCOUNT_SLOTS] = [None, None, None];
        for b in &load.banks {
            let (items, o) = ItemContainer::from_record(
                &b.doc.container,
                ContainerKind::Bank,
                if b.doc.container.capacity == 0 {
                    BANK_SLOTS
                } else {
                    b.doc.container.capacity
                },
                0,
            );
            orphans.extend(o);
            if let Some(slot) = banks.get_mut(usize::from(b.slot)) {
                *slot = Some(AccountBank {
                    character_id: b.character_id,
                    slot: b.slot,
                    pin: b.pin,
                    gold: b.gold,
                    items,
                });
            }
        }
        (
            PlayerItems {
                inventory,
                pockets,
                banks,
                gold,
                pet_obj_id: load.row.summoned_pet_obj_id,
                summoned_eat_pet: None,
                saved_eat_pet: None,
                active_ticket: None,
                active_feed_pocket: None,
                cooldowns: CooldownTimers::default(),
                cast: None,
                pet_filter: 0,
                input: String::new(),
                set_state: SetState::default(),
                serial_pending: false,
                upgrade_limiter: super::upgrade::UpgradeLimiter::default(),
                collect: super::collect::CollectState::default(),
            },
            orphans,
        )
    }

    /// `InventoryDoc` for the snapshot.
    pub fn inventory_doc(&self) -> InventoryDoc {
        InventoryDoc {
            container: self.inventory.to_record(),
            ..InventoryDoc::default()
        }
    }

    /// `PocketsDoc` for the snapshot.
    pub fn pockets_doc(&self) -> PocketsDoc {
        let mut doc = PocketsDoc::default();
        for (rec, p) in doc.pockets.iter_mut().zip(&self.pockets) {
            *rec = flyff_db::model::PocketRecord {
                available: p.available,
                expired: p.expired,
                expires_at: p.expires_at,
                container: p.items.to_record(),
            };
        }
        doc
    }

    /// The banks loaded at enter-world as snapshot parts (gold clamped by the caller).
    pub fn bank_snapshots(&self) -> Vec<BankSnapshot> {
        self.banks
            .iter()
            .flatten()
            .map(|b| BankSnapshot {
                character_id: b.character_id,
                gold: b.gold,
                doc: BankDoc {
                    container: b.items.to_record(),
                    ..BankDoc::default()
                },
            })
            .collect()
    }

    /// The `BankLoad` records of the banks (tests, the persistence round trip).
    pub fn bank_loads(&self) -> Vec<BankLoad> {
        self.banks
            .iter()
            .flatten()
            .map(|b| BankLoad {
                character_id: b.character_id,
                slot: b.slot,
                pin: b.pin,
                gold: b.gold,
                doc: BankDoc {
                    container: b.items.to_record(),
                    ..BankDoc::default()
                },
            })
            .collect()
    }

    /// The bank of character slot `slot`.
    pub fn bank(&self, slot: usize) -> Option<&AccountBank> {
        self.banks.get(slot)?.as_ref()
    }

    /// "In use" (19 §2.6): the summoned eat pet, the summoned system pet, the active feed pocket, the
    /// active ticket. Such items cannot be traded, sold, banked, mailed, dropped, destroyed or vended.
    pub fn is_using(&self, obj_id: StorageId) -> bool {
        self.pet_obj_id == Some(obj_id)
            || self.summoned_eat_pet.is_some_and(|(id, _)| id == obj_id)
            || self.active_feed_pocket == Some(obj_id)
            || self.active_ticket == Some(obj_id)
    }

    /// `GetItemNum` for quest conditions (KQ-27): the pieces of `item_id` in the inventory, **0 while any
    /// inventory item is reserved** (an anti-abuse rule of the original).
    pub fn quest_item_count(&self, item_id: u32) -> u32 {
        if self.inventory.iter().any(|(_, it)| !it.is_usable()) {
            return 0;
        }
        self.inventory.count_of(item_id)
    }
}
