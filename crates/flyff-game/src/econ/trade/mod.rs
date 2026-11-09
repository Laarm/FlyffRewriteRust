//! Player trade with an atomic commit (Plan/19 §8, WP-ECON-03; ref/13 §14, ref/07b §19, VR-017, PL-062, PL-063;
//! src:_Common/MoverItem.cpp:351-518, WORLDSERVER/DPSrvr.cpp:8602-8926).
//!
//! The flow, with the original's duplication and ordering bugs fixed:
//!
//! 1. `ConfirmTradeReq{target}` records a [`TradeRequest`] (TTL `gameplay.invite_ttl_s`, one per requester) and
//!    asks the target (`ConfirmTradeNtf`); `TradeReq{requester}` consumes a pending, unexpired request and opens
//!    the [`TradeSession`] (`TradeNtf` to both, **without** the partner inventory); `ConfirmTradeCancelReq`
//!    declines (VR-017: the original had no request record at all).
//! 2. In the `Item` step both sides put items (`TradePutReq`: `reserved = n` on the item, the offer echoed with
//!    the full item to both), pull them back and add penya (`TradePutGoldReq` **accumulates**, PL-062; the penya
//!    stays in the wallet as a reservation, E5, so a cancel or a crash can never lose it).
//! 3. `TradeOkReq` -> `Ok`, `TradeConfirm` -> `Confirm`; when both are confirmed the **commit** runs
//!    (`commit.rs`): validate everything without mutating, simulate the exchange on clones of the two
//!    inventories, install them, move the penya, send the explicit deltas and `TradeConsent` (PL-063), and queue
//!    exactly one `AtomicBatch{Trade, [snap(a), snap(b)]}` with the audit lines.
//! 4. `cancel.rs`: `TradeCancelReq`, and the per-tick [`process`] pass that ends a session when a participant dies,
//!    teleports, changes world, logs out or disconnects, opens a vendor, the bank or the guild bank, or when the
//!    **content** of an inventory changes (a pickup, a mail, a quest reward; VR-017, [`PlayerItems::content_rev`]).
//!
//! Sessions live in [`TradeService`] (`Services.econ.trade`); the per-player link is
//! `EconState.partner = TradePartner::Player{other}` on both sides (the legacy `m_vtInfo.GetOther()`).
//!
//! [`PlayerItems::content_rev`]: crate::item::inventory::PlayerItems::content_rev

mod cancel;
mod commit;
mod handlers;
pub mod moves;
mod rev;
pub mod snap;

pub use cancel::{Cancel, cancel_for_player, cancel_session, process};
pub use commit::CommitError;

pub(crate) use handlers::guild_combat_1to1_participant;
pub use handlers::{
    on_confirm_trade_cancel_req, on_confirm_trade_req, on_trade_cancel_req, on_trade_confirm,
    on_trade_ok_req, on_trade_pull_req, on_trade_put_gold_req, on_trade_put_req, on_trade_req,
};
pub use rev::item_hash;

use crate::ctx::{GameCtx, PlayerRef};
use crate::item::container::StorageId;
use flyff_common::{CharacterId, GameMs, ItemSerial, LayerId, ObjId, WorldId};
use flyff_data::types::ItemId;
use flyff_proto::pb;
use std::collections::BTreeMap;

/// `MAX_TRADE`: the offer slots of one side.
pub const MAX_TRADE: usize = 25;

/// The id of a running trade.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TradeId(pub u64);

/// `m_state` of `CVTInfo`: `TRADE_STEP_ITEM`, `TRADE_STEP_OK`, `TRADE_STEP_CONFIRM`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TradeStep {
    /// 0: still editing the offer.
    #[default]
    Item,
    /// 1: pressed OK.
    Ok,
    /// 2: pressed the final confirmation.
    Confirm,
}

/// A pending `ConfirmTradeReq` (Plan/19 §8.1): the record `TradeReq` must match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TradeRequest {
    /// The asking player.
    pub from: ObjId,
    /// The asked player.
    pub to: ObjId,
    /// The game time from which the request no longer counts.
    pub expires: GameMs,
}

/// One offered stack: the item is identified by its storage entry **and** its serial, so a commit never moves
/// another item that happens to sit in the entry now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TradeOffer {
    /// The inventory storage entry (the `obj_id` of the wire).
    pub obj_id: StorageId,
    /// The serial of the offered stack.
    pub serial: ItemSerial,
    /// The prop id.
    pub item_id: ItemId,
    /// The offered count (the reservation on the item).
    pub count: u16,
}

/// One side of a trade (`TradeSide`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TradeSide {
    /// The 25 offer slots.
    pub slots: [Option<TradeOffer>; MAX_TRADE],
    /// The penya offered: reserved, **not** removed from the wallet until the commit (E5, PL-062).
    pub gold: i64,
    /// Where this side is in the handshake.
    pub step: TradeStep,
}

impl TradeSide {
    /// The number of occupied slots.
    pub fn offered(&self) -> usize {
        self.slots.iter().flatten().count()
    }

    /// The occupied slots as `(slot, offer)` in slot order.
    pub fn offers(&self) -> impl Iterator<Item = (usize, &TradeOffer)> + '_ {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(i, o)| o.as_ref().map(|o| (i, o)))
    }
}

/// A running trade between two players.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TradeSession {
    /// The player that asked.
    pub a: ObjId,
    /// The player that accepted.
    pub b: ObjId,
    /// The characters `[a, b]` (kept for the cancel message and the audit when an avatar is already gone).
    pub chars: [CharacterId; 2],
    /// The world both stood in when the trade opened (a teleport or world change ends it).
    pub world: WorldId,
    /// Their layer.
    pub layer: LayerId,
    /// The offers `[a, b]`.
    pub side: [TradeSide; 2],
    /// `content_rev` of both inventories when the trade opened (19 §8.4).
    pub revs: [u64; 2],
    /// The game time the trade opened.
    pub opened_at: GameMs,
}

impl TradeSession {
    /// `0` for the requester, `1` for the accepter, `None` for anybody else.
    pub fn index_of(&self, who: ObjId) -> Option<usize> {
        if who == self.a {
            Some(0)
        } else if who == self.b {
            Some(1)
        } else {
            None
        }
    }

    /// The object id of side `i`.
    pub fn obj(&self, i: usize) -> ObjId {
        if i == 0 { self.a } else { self.b }
    }
}

/// Counters of the trade service (tests, `/metrics` later).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TradeStats {
    /// Trades that were committed.
    pub completed: u64,
    /// Trades that ended by a cancel (request, trigger or error).
    pub cancelled: u64,
    /// Commits that failed validation (a subset of `cancelled`).
    pub errors: u64,
}

/// `Services.econ.trade`: the pending requests and the running sessions (Plan/19 §8.1).
#[derive(Debug, Default)]
pub struct TradeService {
    requests: Vec<TradeRequest>,
    sessions: BTreeMap<TradeId, TradeSession>,
    by_player: BTreeMap<ObjId, TradeId>,
    next_id: u64,
    /// Counters.
    pub stats: TradeStats,
}

impl TradeService {
    /// Records `from -> to`, replacing an older request of the same requester.
    pub fn request(&mut self, from: ObjId, to: ObjId, expires: GameMs) {
        self.requests.retain(|r| r.from != from);
        self.requests.push(TradeRequest { from, to, expires });
    }

    /// Consumes the request `from -> to` if it is there and not expired at `now`.
    pub fn take_request(&mut self, from: ObjId, to: ObjId, now: GameMs) -> bool {
        let Some(i) = self
            .requests
            .iter()
            .position(|r| r.from == from && r.to == to)
        else {
            return false;
        };
        self.requests.remove(i).expires > now
    }

    /// `true` while the request `from -> to` is pending and not expired at `now` (it is not consumed).
    pub fn has_request(&self, from: ObjId, to: ObjId, now: GameMs) -> bool {
        self.requests
            .iter()
            .any(|r| r.from == from && r.to == to && r.expires > now)
    }

    /// Drops every request that `p` made or received (logout, a trade that opened).
    pub fn drop_requests_of(&mut self, p: ObjId) {
        self.requests.retain(|r| r.from != p && r.to != p);
    }

    /// The pending requests (expired ones included until the next pass).
    pub fn requests(&self) -> &[TradeRequest] {
        &self.requests
    }

    /// Drops the requests that expired at `now` or whose `keep` says no.
    pub fn retain_requests(&mut self, now: GameMs, mut keep: impl FnMut(&TradeRequest) -> bool) {
        self.requests.retain(|r| r.expires > now && keep(r));
    }

    /// Opens a session and links both players.
    pub fn open(&mut self, session: TradeSession) -> TradeId {
        self.next_id += 1;
        let id = TradeId(self.next_id);
        self.by_player.insert(session.a, id);
        self.by_player.insert(session.b, id);
        self.sessions.insert(id, session);
        id
    }

    /// The running trade of `p`.
    pub fn session_id_of(&self, p: ObjId) -> Option<TradeId> {
        self.by_player.get(&p).copied()
    }

    /// A session.
    pub fn session(&self, id: TradeId) -> Option<&TradeSession> {
        self.sessions.get(&id)
    }

    /// A session, mutably.
    pub fn session_mut(&mut self, id: TradeId) -> Option<&mut TradeSession> {
        self.sessions.get_mut(&id)
    }

    /// Removes a session and its links.
    pub fn remove(&mut self, id: TradeId) -> Option<TradeSession> {
        let s = self.sessions.remove(&id)?;
        for p in [s.a, s.b] {
            if self.by_player.get(&p) == Some(&id) {
                self.by_player.remove(&p);
            }
        }
        Some(s)
    }

    /// The ids of the running sessions, ascending.
    pub fn ids(&self) -> Vec<TradeId> {
        self.sessions.keys().copied().collect()
    }

    /// The number of running sessions.
    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    /// `true` when no trade is running.
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }
}

/// Sends `body` to the bound session of `to` with `header` as the wire object id (nothing for a lingering or gone
/// player).
pub(crate) fn send(
    ctx: &mut GameCtx<'_>,
    to: ObjId,
    header: ObjId,
    body: impl Into<pb::server_msg::Msg>,
) {
    if let Some(sid) = ctx.session_of(PlayerRef(to)) {
        ctx.outbox.to_session(sid, header.0, body);
    }
}
