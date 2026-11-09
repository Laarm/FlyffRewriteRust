//! The seam between the personal vendor and the vendor chat rooms (Plan/19 §9.3, WP-ECON-04 / WP-ECON-11).
//!
//! The vendor calls [`VendorRoomHooks`] at four places: the shop opens (`on_vendor_open`), **every** way a shop
//! closes (`on_vendor_close`, exactly once per shop: the request, the logout and the disconnect, the death, the
//! teleport), a visitor closes the vendor window or leaves (`on_member_leave`), and a `PVendorItem` reply asks
//! for the chat flag (`room_open_state`). [`NoVendorRooms`] does nothing, so the vendor is built and tested
//! without rooms; [`crate::econ::room::ChatRooms`] is the real implementation (WP-ECON-11) and the default of
//! [`super::VendorService`].
//!
//! **Deviation from the plan's sketch:** the hooks take `(&mut GameCtx, PlayerRef)` and not
//! `(&mut GameCtx, CharacterId)`: feature code addresses an online player by its `ObjId` (the room members are
//! online avatars, and a vendor that is already gone when its shop closes is still known by its `ObjId`). The
//! four room requests (`EnterChatting`, `ChattingReq`, `OpenChattingRoom`, `CloseChattingRoom`) are provided
//! methods too, so the handlers of `chat::vendor_room` reach the installed implementation through the trait
//! object; the default of every method is "no rooms": nothing happens and nothing is sent.
//! `room_open_state` defaults to `true` (a vendor without a room, legacy `bChatting = TRUE`; the plan's sketch
//! says `false`, but its acceptance criterion and the legacy both say `true`).

use crate::ctx::{GameCtx, PlayerRef};
use flyff_common::CharacterId;

/// The vendor-room hooks. Every method has the "no rooms" default.
pub trait VendorRoomHooks: std::fmt::Debug + Send {
    /// A shop with registered goods just opened (`PVendorOpenNtf` is out): create the room.
    fn on_vendor_open(&mut self, _ctx: &mut GameCtx<'_>, _vendor: PlayerRef) {}

    /// The shop of `vendor` closed (every close path, exactly once): delete the room.
    fn on_vendor_close(&mut self, _ctx: &mut GameCtx<'_>, _vendor: PlayerRef) {}

    /// `who` left a vendor window, logged out or was removed: leave the room it is in, if any.
    fn on_member_leave(&mut self, _ctx: &mut GameCtx<'_>, _who: PlayerRef) {}

    /// `PVendorItem.chatting_state` of the shop of `vendor`: the room's `open` flag, `true` without a room.
    fn room_open_state(&self, _vendor: PlayerRef) -> bool {
        true
    }

    /// `EnterChatting{room}`: `who` asks to join the room of the vendor with character id `room`.
    fn on_enter(&mut self, _ctx: &mut GameCtx<'_>, _who: PlayerRef, _room: CharacterId) {}

    /// `ChattingReq{text}`: `who` says `text` in its room.
    fn on_chatting(&mut self, _ctx: &mut GameCtx<'_>, _who: PlayerRef, _text: &str) {}

    /// `OpenChattingRoom`: the owner opens the chat.
    fn on_open_room(&mut self, _ctx: &mut GameCtx<'_>, _who: PlayerRef) {}

    /// `CloseChattingRoom`: the owner closes the chat.
    fn on_close_room(&mut self, _ctx: &mut GameCtx<'_>, _who: PlayerRef) {}

    /// Once per tick, after the worlds: drop members whose avatar is gone or has no socket any more.
    fn on_tick(&mut self, _ctx: &mut GameCtx<'_>) {}
}

/// No vendor rooms: nothing is ever created and no `ChattingNtf` is ever sent.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoVendorRooms;

impl VendorRoomHooks for NoVendorRooms {}
