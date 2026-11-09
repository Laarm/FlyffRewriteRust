//! Item queries (Plan/25 §5.5, §11.5; **WP-ADMIN-20**): `FindItems`, the live half of the ownership search.
//!
//! The game thread answers for everything that lives in memory: the inventories, equipment, pockets and banks of the
//! **online** characters (a bank once per online account: banks are account-scoped, every online sibling holds the
//! same three) and the ground items of every world. The route streams the database half (`character_docs`, `banks`,
//! guild banks, open mails, pending grants) next to it and drops the database copies of the accounts listed in
//! [`FindItemsResult::online_accounts`]: **live hits override the DB copies** (an online character's DB row can be
//! 90 s old).
//!
//! Like every query it takes `&Game`: nothing here changes, wakes or schedules anything (ADM-D4).

use super::AdminQueryResult;
use crate::tick::Game;
use crate::world::ObjKind;
use flyff_common::{AccountId, CharacterId, LayerId, WorldId};
use flyff_db::ItemRecord;
use std::collections::BTreeSet;

/// The most hits one answer carries (the rest is `truncated`).
pub const LIVE_HITS_MAX: usize = 5_000;

/// What `FindItems` looks for (25 §5.5): an item id, optionally an upgrade floor and an element, or a serial.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ItemQuery {
    /// An item id.
    pub item_id: Option<u32>,
    /// An item serial.
    pub serial: Option<u64>,
    /// Only items with this `+N` or more.
    pub min_upgrade: Option<i32>,
    /// Only items with this element (`1..=5`).
    pub element: Option<u8>,
}

impl ItemQuery {
    /// Does `item` match?
    pub fn matches(&self, item: &ItemRecord) -> bool {
        self.item_id.is_none_or(|id| item.item_id == id)
            && self.serial.is_none_or(|s| item.serial == s)
            && self.min_upgrade.is_none_or(|n| item.upgrade_level >= n)
            && self.element.is_none_or(|e| item.element == e)
    }

    /// A query without an item id or a serial would list the whole game.
    pub fn is_empty(&self) -> bool {
        self.item_id.is_none() && self.serial.is_none()
    }
}

/// Where a live item is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LiveSite {
    /// A worn item.
    Equipment,
    /// The bag.
    Inventory,
    /// A pocket `0..3`.
    Pocket(u8),
    /// A bank compartment of the account (the character slot).
    Bank,
    /// On the ground.
    Ground,
}

/// Where a ground item lies.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroundAt {
    /// The object id of the drop.
    pub obj_id: u32,
    /// World.
    pub world: WorldId,
    /// Layer.
    pub layer: LayerId,
    /// Position (metres).
    pub pos: [f32; 3],
}

/// One live item.
#[derive(Clone, Debug, PartialEq)]
pub struct LiveItemHit {
    /// Where.
    pub site: LiveSite,
    /// The online character holding it (`None` on the ground).
    pub character_id: Option<CharacterId>,
    /// Its name.
    pub character_name: Option<String>,
    /// Its account.
    pub account_id: Option<AccountId>,
    /// The character slot of a bank compartment.
    pub bank_slot: Option<u8>,
    /// The visible slot in the container.
    pub slot: Option<u16>,
    /// Where a ground item lies.
    pub ground: Option<GroundAt>,
    /// The item.
    pub item: ItemRecord,
}

/// The answer of `FindItems`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FindItemsResult {
    /// The live hits, characters in id order, then the ground.
    pub hits: Vec<LiveItemHit>,
    /// The accounts that have an online character: their database copies are stale.
    pub online_accounts: Vec<AccountId>,
    /// More hits exist than [`LIVE_HITS_MAX`].
    pub truncated: bool,
}

/// `FindItems`: see the module documentation.
pub(super) fn find_items(g: &Game, query: &ItemQuery) -> AdminQueryResult {
    if query.is_empty() {
        return AdminQueryResult::Failed(crate::admin::types::AdminFailure::new(
            crate::admin::types::AdminErrorCode::Invalid,
            "an item search needs an item id or a serial",
        ));
    }
    let mut out = FindItemsResult::default();
    let mut accounts = BTreeSet::new();
    let mut banks_done = BTreeSet::new();
    let mut online: Vec<_> = g
        .services
        .presence
        .iter()
        .filter_map(|e| e.online.map(|o| (e.id, e.account, e.name.clone(), o.obj)))
        .collect();
    online.sort_by_key(|(id, ..)| *id);
    'players: for (character, account, name, obj) in online {
        let Some(p) = g.worlds.player(obj).and_then(|m| m.player()) else {
            continue;
        };
        accounts.insert(account);
        let push = |out: &mut FindItemsResult,
                    site: LiveSite,
                    slot: Option<u16>,
                    bank_slot: Option<u8>,
                    item: &crate::item::model::Item,
                    obj_id: u16| {
            let rec = item.to_record(obj_id);
            if !query.matches(&rec) {
                return true;
            }
            if out.hits.len() >= LIVE_HITS_MAX {
                out.truncated = true;
                return false;
            }
            out.hits.push(LiveItemHit {
                site,
                character_id: Some(character),
                character_name: Some(name.clone()),
                account_id: Some(account),
                bank_slot,
                slot,
                ground: None,
                item: rec,
            });
            true
        };
        let inv = &p.items.inventory;
        for (id, item) in inv.iter() {
            let site = if inv.is_equip(id) {
                LiveSite::Equipment
            } else {
                LiveSite::Inventory
            };
            let slot = inv.slot_of(id);
            if !push(&mut out, site, slot, None, item, id) {
                break 'players;
            }
        }
        for (i, pocket) in p.items.pockets.iter().enumerate() {
            let n = u8::try_from(i).unwrap_or(0);
            for (id, item) in pocket.items.iter() {
                let slot = pocket.items.slot_of(id);
                if !push(&mut out, LiveSite::Pocket(n), slot, None, item, id) {
                    break 'players;
                }
            }
        }
        if banks_done.insert(account) {
            for (i, bank) in p.items.banks.iter().enumerate() {
                let Some(bank) = bank else { continue };
                let n = u8::try_from(i).unwrap_or(0);
                for (id, item) in bank.items.iter() {
                    let slot = bank.items.slot_of(id);
                    if !push(&mut out, LiveSite::Bank, slot, Some(n), item, id) {
                        break 'players;
                    }
                }
            }
        }
    }
    if !out.truncated {
        for drop in g.worlds.store().items.values() {
            if drop.core.deleted || drop.core.kind != ObjKind::Item || !query.matches(&drop.item) {
                continue;
            }
            if out.hits.len() >= LIVE_HITS_MAX {
                out.truncated = true;
                break;
            }
            out.hits.push(LiveItemHit {
                site: LiveSite::Ground,
                character_id: None,
                character_name: None,
                account_id: None,
                bank_slot: None,
                slot: None,
                ground: Some(GroundAt {
                    obj_id: drop.core.id.0,
                    world: drop.core.world,
                    layer: drop.core.layer,
                    pos: [drop.core.pos.x, drop.core.pos.y, drop.core.pos.z],
                }),
                item: (*drop.item).clone(),
            });
        }
    }
    out.online_accounts = accounts.into_iter().collect();
    AdminQueryResult::Items(Box::new(out))
}
