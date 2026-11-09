//! The personal vendor ("private shop", Plan/19 §9, WP-ECON-04; ref/13 §15, ref/07c §5, ref/07b §20; VR-013,
//! VR-019, VR-046, PL-064, PL-155; src:WORLDSERVER/DPSrvr.cpp:8928-9325, _Common/MoverItem.cpp:535-704).
//!
//! A vendor registers goods while its shop is closed (`RegisterPVendorItemReq`: the item gets `reserved = n`
//! and `vendor_price`, the slot mirrors them), opens it with a title (`PVendorOpenReq`: every check runs
//! **before** the title is set, **FIX**), and visitors browse it (`QueryPVendorItem`) and buy (`BuyPVendorItem`:
//! one atomic [`purchase`], `AtomicBatch{VendorPurchase, [snap(buyer), snap(seller)]}`). The shop closes by
//! request, logout, disconnect, death and teleport ([`close_vendor`]); every close calls
//! [`VendorRoomHooks::on_vendor_close`] exactly once.
//!
//! | module | content |
//! |---|---|
//! | [`hooks`] | [`VendorRoomHooks`] and the [`NoVendorRooms`] default (the seam of WP-ECON-11) |
//! | [`handlers`] | register, unregister, open, query, close |
//! | [`purchase`] | `BuyPVendorItem` |
//! | [`close`] | [`close_vendor`], [`leave_browse`] and the per-tick [`process`] |
//!
//! State: [`VendorState`] on the player (`EconState.vendor`, with `EconState.vending` kept in step), the registry of
//! open shops and of their visitors in [`VendorService`].

pub mod close;
pub mod handlers;
pub mod hooks;
pub mod purchase;

pub use close::{close_vendor, leave_browse, process};
pub use handlers::{on_close, on_open, on_query, on_register, on_unregister};
pub use hooks::{NoVendorRooms, VendorRoomHooks};
pub use purchase::on_buy;

use crate::econ::room::ChatRooms;
use crate::item::container::StorageId;
use crate::mover::Mover;
use flyff_common::{GameMs, ItemSerial, ObjId};
use flyff_data::types::ItemId;
use std::collections::{BTreeMap, BTreeSet};

/// `MAX_VENDITEM`: the slots a shop has (the buy and unregister indices).
pub const MAX_VENDITEM: usize = 30;
/// `MAX_VENDOR_REVISION`: the slots a registration may use (`iIndex < 20`).
pub const MAX_VENDOR_REVISION: usize = 20;
/// The longest title the client sends (`MAX_VENDORNAME` 48 with the terminator).
pub const MAX_VENDOR_TITLE: usize = 47;

/// One registered good: identified by the storage entry **and** the serial, so a sale never moves another item
/// that sits in the entry now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VendorSlot {
    /// The inventory storage entry (the `obj_id` of the wire).
    pub obj_id: StorageId,
    /// The serial of the registered stack.
    pub serial: ItemSerial,
    /// The prop id.
    pub item_id: ItemId,
    /// The count for sale (the reservation on the item).
    pub count: u16,
    /// The unit price (`m_nCost`).
    pub price: i32,
}

/// The stall of a player (Plan/19 §9.1).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VendorState {
    /// The shop title; the shop is open exactly when it is set (`IsVendorOpen`). A title with no goods leaves the
    /// shop "open" and blocks further registration until it is closed (**KEEP-QUIRK**).
    pub title: Option<String>,
    /// The slots: registration uses `0..20`, buying and unregistering `0..30`.
    pub slots: [Option<VendorSlot>; MAX_VENDITEM],
    /// When the shop opened (honor "vendor time", doc 21).
    pub opened_at: GameMs,
}

impl VendorState {
    /// `IsVendorOpen`: the title is set.
    pub fn is_open(&self) -> bool {
        self.title.is_some()
    }

    /// `VendorIsVendor`: at least one good is registered.
    pub fn is_vendor(&self) -> bool {
        self.slots.iter().any(Option::is_some)
    }

    /// The slot the storage entry is registered in.
    pub fn slot_of(&self, obj_id: StorageId) -> Option<usize> {
        self.slots
            .iter()
            .position(|s| s.as_ref().is_some_and(|s| s.obj_id == obj_id))
    }

    /// `sum(price * count)` over the registered goods (the VR-019 total).
    pub fn registered_value(&self) -> i64 {
        self.slots
            .iter()
            .flatten()
            .map(|s| i64::from(s.price) * i64::from(s.count))
            .sum()
    }

    /// The registered goods as `(slot, good)` in slot order.
    pub fn goods(&self) -> impl Iterator<Item = (usize, &VendorSlot)> + '_ {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.as_ref().map(|s| (i, s)))
    }
}

/// Keeps `EconState.vending` in step with the stall: set while the shop is open or goods are registered.
pub(crate) fn sync_vending(p: &mut crate::player::PlayerExt) {
    p.econ.vending = p.econ.vendor.is_open() || p.econ.vendor.is_vendor();
}

/// Counters of the vendor service.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VendorStats {
    /// Shops that opened with goods.
    pub opened: u64,
    /// Shops that closed (any path).
    pub closed: u64,
    /// Purchases that went through.
    pub sales: u64,
}

/// `Services.econ.vendor`: the installed room hooks, the shops that are open or have goods, and their visitors.
#[derive(Debug)]
pub struct VendorService {
    /// The vendor-room implementation: [`ChatRooms`] by default (WP-ECON-11), [`NoVendorRooms`] without rooms.
    pub rooms: Box<dyn VendorRoomHooks>,
    /// The vendors with a title or registered goods.
    pub(crate) active: BTreeSet<ObjId>,
    /// The visitors of each vendor (`partner == VendorBrowse{vendor}`).
    pub(crate) browsers: BTreeMap<ObjId, BTreeSet<ObjId>>,
    /// Counters.
    pub stats: VendorStats,
}

impl Default for VendorService {
    fn default() -> Self {
        VendorService {
            rooms: Box::new(ChatRooms::default()),
            active: BTreeSet::new(),
            browsers: BTreeMap::new(),
            stats: VendorStats::default(),
        }
    }
}

impl VendorService {
    /// The vendors with a title or goods, ascending.
    pub fn active(&self) -> impl Iterator<Item = ObjId> + '_ {
        self.active.iter().copied()
    }

    /// The visitors of `vendor`, ascending.
    pub fn browsers_of(&self, vendor: ObjId) -> Vec<ObjId> {
        self.browsers
            .get(&vendor)
            .map(|b| b.iter().copied().collect())
            .unwrap_or_default()
    }

    /// `true` while `vendor` is registered as an active shop.
    pub fn is_active(&self, vendor: ObjId) -> bool {
        self.active.contains(&vendor)
    }
}

/// A player that is in a world (the legacy `IsValidObj`).
pub(crate) fn live_player<'a>(ctx: &'a crate::ctx::GameCtx<'_>, obj: ObjId) -> Option<&'a Mover> {
    ctx.worlds.player(obj).filter(|m| !m.core.deleted)
}
