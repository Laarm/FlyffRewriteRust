//! The item rules of a trade window (Plan/19 §8.2, ref/13 §14; src:_Common/MoverItem.cpp:218-245).
//!
//! [`tradeable`] is the pure predicate behind `TradePutReq` and the re-check at commit (§8.3); the browser
//! uses it to grey out items in the trade window (45's `tradeable(item) -> 0 | TID`).

use super::IP_FLAG_BINDS;
use flyff_common::defs::itemkind::{IK2_WARP, IK3_CLOAK, IK3_QUEST};
use flyff_common::defs::neuz::PARTS_RIDE;
use flyff_common::defs::text::{TID_GAME_CANNOT_DO_USINGITEM, TID_GAME_CANNOTTRADE_ITEM};
use flyff_data::props::item::ItemProp;

/// The facts about one inventory item the trade rules read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TradeView {
    /// `m_nExtra != 0`: the item is already offered in a trade or registered in a vendor.
    pub reserved: bool,
    /// The item's flag byte (`EXPIRED 0x01`, `BINDS 0x02`).
    pub flags: u8,
    /// `m_dwKeepTime` (absolute unix seconds, 0 = permanent).
    pub keep_until: i64,
    /// The item is in an equipment slot.
    pub equipped: bool,
    /// The item is "in use" (summoned pet, active feed pocket or ticket, Plan/19 §2.6).
    pub in_use: bool,
    /// `m_idGuild` (guild cloaks).
    pub guild_id: u32,
    /// `IsBinds` already evaluated by the caller for the random-option part that needs the data tables
    /// (Blessing / EatPet words, level-down); the keep-time, prop-flag and item-flag parts are evaluated
    /// here.
    pub binds_by_option: bool,
}

/// `Some(tid)` = the refusal text of `TradePutReq`, `None` = the item may be put into a trade (and a
/// vendor, the same rule). The order is the original's: expired, reserved, equipped, quest, bound, in use,
/// guild cloak, vagrant ride.
pub fn tradeable(v: &TradeView, p: &ItemProp, now: i64) -> Option<u32> {
    let expired = v.flags & 0x01 != 0 || (v.keep_until != 0 && now > v.keep_until);
    if expired || v.reserved || v.equipped || p.item_kind3 == IK3_QUEST {
        return Some(TID_GAME_CANNOTTRADE_ITEM);
    }
    if binds(v, p) {
        return Some(TID_GAME_CANNOTTRADE_ITEM);
    }
    if v.in_use {
        return Some(TID_GAME_CANNOT_DO_USINGITEM);
    }
    if (p.item_kind3 == IK3_CLOAK && v.guild_id != 0)
        || (p.parts == PARTS_RIDE && p.item_job == flyff_common::defs::job::JOB_VAGRANT)
    {
        return Some(TID_GAME_CANNOTTRADE_ITEM);
    }
    None
}

/// `IsBinds` without the data-table part (see [`TradeView::binds_by_option`]).
fn binds(v: &TradeView, p: &ItemProp) -> bool {
    (v.keep_until != 0 && p.item_kind2 != IK2_WARP)
        || p.flag & IP_FLAG_BINDS != 0
        || v.flags & 0x02 != 0
        || v.binds_by_option
}
