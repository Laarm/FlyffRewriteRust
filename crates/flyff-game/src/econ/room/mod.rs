//! The vendor chat rooms (Plan/19 §9.3, WP-ECON-11; ref/07b §10, ref/24 §7.6, ref/07c §5.1-§5.2; VR-074;
//! src:WORLDSERVER/DPSrvr.cpp:6472-6620, _Common/Chatting.cpp). Also reachable as `chat::vendor_room`, the path of
//! the plan.
//!
//! A shop with goods that opens creates a room keyed by the vendor's **character id** (`EnterChatting.room` is
//! that id) with the vendor as first member and `open = true`. Visitors join with `EnterChatting`, talk with
//! `ChattingReq`; the vendor toggles the chat with `OpenChattingRoom` / `CloseChattingRoom` (**VR-074:** only the
//! owner; the original accepted any member). Every shop close path deletes the room with `DELETECHATTING` to all
//! members, a visitor that leaves sends `REMOVEMEMBER`. [`ChatRooms`] is the [`VendorRoomHooks`] implementation
//! the vendor calls (WP-ECON-04's seam) and the default of `Services.econ.vendor`.
//!
//! | event | what the members get (`ChattingNtf` sub-type) |
//! |---|---|
//! | the shop opens | the vendor: `NEWCHATTING [vendor]` |
//! | `EnterChatting` | the members: `ENTERMEMBER`; the newcomer: `NEWCHATTING [all members]` |
//! | `ChattingReq` | everyone, the sender included: `CHATTING {sender, text}`, also while `open == false` (legacy); a muted sender gets the mute text and nothing is delivered; a sender whose room is gone gets `DELETECHATTING` |
//! | `OpenChattingRoom` / `CloseChattingRoom` | every member but the vendor: `CHATTINGROOMSTATE` |
//! | a visitor leaves or logs out | every member (the leaver included): `REMOVEMEMBER` |
//! | the shop closes | every member: `DELETECHATTING`, once |
//!
//! Sends skip members without a socket, like the legacy `IsValidObj`.

use super::vendor::VendorRoomHooks;
use crate::chat::ingress::markup::markup_text;
use crate::chat::ingress::mode::mute_text;
use crate::chat::ingress::notice;
use crate::ctx::{GameCtx, PlayerRef, Services};
use flyff_common::{CharacterId, ObjId};
use flyff_proto::pb;
use pb::chatting_ntf::Sub;
use std::collections::BTreeMap;

/// `MAX_CHATTINGMEMBER`: the members of one room, the vendor included.
pub const MAX_CHATTINGMEMBER: usize = 512;
/// The longest line (`ChattingReq.text`, 1023 bytes).
pub const MAX_CHAT_TEXT: usize = 1023;

/// One member of a room: kept with the name it joined under, so a member whose avatar is already gone can still be
/// announced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomMember {
    /// `idPlayer`, the id the wire uses.
    pub character: CharacterId,
    /// The avatar.
    pub obj: ObjId,
    /// The character name.
    pub name: String,
}

/// A vendor room (`CChatting`): the members in join order and the chat flag (`m_bState`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VendorRoom {
    /// The vendor's avatar: the only one who may open and close the chat.
    pub owner: ObjId,
    /// The members in join order, the vendor first.
    pub members: Vec<RoomMember>,
    /// `m_bState`: informational (a closed chat still delivers lines, legacy).
    pub open: bool,
}

/// The rooms of the shops and who is in which room.
#[derive(Debug, Default)]
pub struct ChatRooms {
    /// Key = the vendor's character id (the legacy room id).
    rooms: BTreeMap<CharacterId, VendorRoom>,
    /// `m_idChatting` of every member (absent = 0).
    member_of: BTreeMap<ObjId, CharacterId>,
    /// The room of each vendor avatar.
    by_owner: BTreeMap<ObjId, CharacterId>,
}

// ---- the ChattingNtf builders (ref/07b §10) ---------------------------------------------------------------------

/// `CHATTING_ENTERMEMBER (0)`.
pub fn enter_member(m: &RoomMember) -> pb::ChattingNtf {
    pb::ChattingNtf {
        sub: Some(Sub::EnterMember(pb::chatting_ntf::EnterMember {
            id_player: m.character.0,
            name: m.name.clone(),
        })),
    }
}

/// `CHATTING_NEWCHATTING (1)`: the member list.
pub fn new_chatting(members: &[RoomMember]) -> pb::ChattingNtf {
    pb::ChattingNtf {
        sub: Some(Sub::NewChatting(pb::chatting_ntf::NewChatting {
            members: members
                .iter()
                .map(|m| pb::chatting_ntf::Member {
                    id_player: m.character.0,
                    name: m.name.clone(),
                })
                .collect(),
        })),
    }
}

/// `CHATTING_DELETECHATTING (2)`.
pub fn delete_chatting() -> pb::ChattingNtf {
    pb::ChattingNtf {
        sub: Some(Sub::DeleteChatting(pb::chatting_ntf::DeleteChatting {})),
    }
}

/// `CHATTING_REMOVEMEMBER (3)`.
pub fn remove_member(character: CharacterId) -> pb::ChattingNtf {
    pb::ChattingNtf {
        sub: Some(Sub::RemoveMember(pb::chatting_ntf::RemoveMember {
            id_player: character.0,
        })),
    }
}

/// `CHATTING_CHATTING (4)`.
pub fn chatting(speaker: CharacterId, text: &str) -> pb::ChattingNtf {
    pb::ChattingNtf {
        sub: Some(Sub::Chatting(pb::chatting_ntf::Chatting {
            id_player: speaker.0,
            text: text.to_owned(),
        })),
    }
}

/// `CHATTING_CHATTINGROOMSTATE (5)`.
pub fn room_state(open: bool) -> pb::ChattingNtf {
    pb::ChattingNtf {
        sub: Some(Sub::ChattingRoomState(
            pb::chatting_ntf::ChattingRoomState { open },
        )),
    }
}

/// Sends a `ChattingNtf` to an avatar (its own session, wire object id 0; nothing without a socket).
fn push(ctx: &mut GameCtx<'_>, to: ObjId, msg: pb::ChattingNtf) {
    if let Some(sid) = ctx.session_of(PlayerRef(to)) {
        ctx.outbox.to_session(sid, 0, msg);
    }
}

/// The character id and name of an online avatar.
fn identity(ctx: &GameCtx<'_>, obj: ObjId) -> Option<(CharacterId, String)> {
    let p = ctx.worlds.player(obj)?.player()?;
    Some((p.ident.character_id, p.ident.name.clone()))
}

/// `text` cut to `max` bytes at a character boundary.
fn cut(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

impl ChatRooms {
    /// The room of the vendor with character id `room`.
    pub fn room(&self, room: CharacterId) -> Option<&VendorRoom> {
        self.rooms.get(&room)
    }

    /// The room an avatar is in (`m_idChatting`).
    pub fn room_of(&self, member: ObjId) -> Option<CharacterId> {
        self.member_of.get(&member).copied()
    }

    /// The room a vendor avatar owns.
    pub fn room_of_owner(&self, vendor: ObjId) -> Option<&VendorRoom> {
        self.by_owner.get(&vendor).and_then(|c| self.rooms.get(c))
    }

    /// The number of rooms.
    pub fn len(&self) -> usize {
        self.rooms.len()
    }

    /// `true` without rooms.
    pub fn is_empty(&self) -> bool {
        self.rooms.is_empty()
    }

    /// Deletes the room `chr`: every member gets `DELETECHATTING` and loses its membership.
    fn delete_room(&mut self, ctx: &mut GameCtx<'_>, chr: CharacterId) {
        let Some(room) = self.rooms.remove(&chr) else {
            return;
        };
        self.by_owner.remove(&room.owner);
        for m in &room.members {
            if self.member_of.get(&m.obj) == Some(&chr) {
                self.member_of.remove(&m.obj);
            }
            push(ctx, m.obj, delete_chatting());
        }
    }

    /// `who` leaves its room: every member (the leaver included) gets `REMOVEMEMBER`, then the member goes.
    fn remove(&mut self, ctx: &mut GameCtx<'_>, who: ObjId) {
        let Some(chr) = self.member_of.get(&who).copied() else {
            return;
        };
        let Some(room) = self.rooms.get(&chr) else {
            self.member_of.remove(&who);
            return;
        };
        if room.owner == who {
            // the owner never leaves its own room: the shop closing deletes it
            self.delete_room(ctx, chr);
            return;
        }
        let Some(leaver) = room.members.iter().find(|m| m.obj == who).cloned() else {
            self.member_of.remove(&who);
            return;
        };
        let targets: Vec<ObjId> = room.members.iter().map(|m| m.obj).collect();
        for t in targets {
            push(ctx, t, remove_member(leaver.character));
        }
        if let Some(room) = self.rooms.get_mut(&chr) {
            room.members.retain(|m| m.obj != who);
        }
        self.member_of.remove(&who);
    }
}

impl VendorRoomHooks for ChatRooms {
    fn on_vendor_open(&mut self, ctx: &mut GameCtx<'_>, vendor: PlayerRef) {
        let Some((chr, name)) = identity(ctx, vendor.0) else {
            return;
        };
        // a stale room of the same vendor cannot exist; delete it rather than keep two
        self.delete_room(ctx, chr);
        let me = RoomMember {
            character: chr,
            obj: vendor.0,
            name,
        };
        let list = [me.clone()];
        self.rooms.insert(
            chr,
            VendorRoom {
                owner: vendor.0,
                members: vec![me],
                open: true,
            },
        );
        self.member_of.insert(vendor.0, chr);
        self.by_owner.insert(vendor.0, chr);
        push(ctx, vendor.0, new_chatting(&list));
    }

    fn on_vendor_close(&mut self, ctx: &mut GameCtx<'_>, vendor: PlayerRef) {
        if let Some(chr) = self.by_owner.get(&vendor.0).copied() {
            self.delete_room(ctx, chr);
        }
    }

    fn on_member_leave(&mut self, ctx: &mut GameCtx<'_>, who: PlayerRef) {
        self.remove(ctx, who.0);
    }

    fn room_open_state(&self, vendor: PlayerRef) -> bool {
        self.room_of_owner(vendor.0).is_none_or(|r| r.open)
    }

    fn on_enter(&mut self, ctx: &mut GameCtx<'_>, who: PlayerRef, room: CharacterId) {
        // the legacy does not even read the payload when the sender is in a room already
        if self.member_of.contains_key(&who.0) {
            return;
        }
        let Some((chr, name)) = identity(ctx, who.0) else {
            return;
        };
        let Some(r) = self.rooms.get_mut(&room) else {
            return;
        };
        if r.members.len() >= MAX_CHATTINGMEMBER || r.members.iter().any(|m| m.obj == who.0) {
            return;
        }
        let newcomer = RoomMember {
            character: chr,
            obj: who.0,
            name,
        };
        let existing: Vec<ObjId> = r.members.iter().map(|m| m.obj).collect();
        r.members.push(newcomer.clone());
        let all = r.members.clone();
        self.member_of.insert(who.0, room);
        for t in existing {
            push(ctx, t, enter_member(&newcomer));
        }
        push(ctx, who.0, new_chatting(&all));
    }

    fn on_chatting(&mut self, ctx: &mut GameCtx<'_>, who: PlayerRef, text: &str) {
        let Some(m) = ctx.worlds.player(who.0) else {
            return;
        };
        if let Some(tid) = mute_text(m) {
            notice::defined(ctx, who, tid, Vec::new());
            return;
        }
        let line = markup_text(m, cut(text, MAX_CHAT_TEXT));
        let Some((chr, _)) = identity(ctx, who.0) else {
            return;
        };
        let room = self
            .member_of
            .get(&who.0)
            .and_then(|c| self.rooms.get(c))
            .filter(|r| r.members.iter().any(|x| x.obj == who.0));
        match room {
            None => {
                // the room is gone: the membership ends and the sender is told
                self.member_of.remove(&who.0);
                push(ctx, who.0, delete_chatting());
            }
            Some(r) => {
                let targets: Vec<ObjId> = r.members.iter().map(|m| m.obj).collect();
                for t in targets {
                    push(ctx, t, chatting(chr, &line));
                }
            }
        }
    }

    fn on_open_room(&mut self, ctx: &mut GameCtx<'_>, who: PlayerRef) {
        self.set_open(ctx, who.0, true);
    }

    fn on_close_room(&mut self, ctx: &mut GameCtx<'_>, who: PlayerRef) {
        self.set_open(ctx, who.0, false);
    }

    fn on_tick(&mut self, ctx: &mut GameCtx<'_>) {
        let gone: Vec<ObjId> = self
            .member_of
            .keys()
            .copied()
            .filter(|&o| ctx.worlds.player(o).is_none() || ctx.session_of(PlayerRef(o)).is_none())
            .collect();
        for o in gone {
            self.remove(ctx, o);
        }
    }
}

impl ChatRooms {
    /// `OpenChattingRoom` / `CloseChattingRoom` (**VR-074**): only the owner of the sender's room changes the
    /// flag; every other member gets `CHATTINGROOMSTATE`.
    fn set_open(&mut self, ctx: &mut GameCtx<'_>, who: ObjId, open: bool) {
        let Some(chr) = self.member_of.get(&who).copied() else {
            return;
        };
        let Some(room) = self.rooms.get_mut(&chr) else {
            return;
        };
        if room.owner != who {
            return;
        }
        room.open = open;
        let others: Vec<ObjId> = room
            .members
            .iter()
            .map(|m| m.obj)
            .filter(|&o| o != who)
            .collect();
        for t in others {
            push(ctx, t, room_state(open));
        }
    }
}

// ---- the handlers (chat::vendor_room::{on_enter, on_chatting, on_open_room, on_close_room}) ----------------------

/// `OnEnterChattingRoom`: forwards to the installed rooms.
pub fn on_enter(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, msg: pb::EnterChatting) {
    svc.econ
        .vendor
        .rooms
        .on_enter(ctx, who, CharacterId(msg.room));
}

/// `OnChatting`: forwards to the installed rooms.
pub fn on_chatting(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::ChattingReq,
) {
    svc.econ.vendor.rooms.on_chatting(ctx, who, &msg.text);
}

/// `OnOpenChattingRoom` (VR-074: owner only).
pub fn on_open_room(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    _msg: pb::OpenChattingRoom,
) {
    svc.econ.vendor.rooms.on_open_room(ctx, who);
}

/// `OnCloseChattingRoom` (VR-074: owner only).
pub fn on_close_room(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    _msg: pb::CloseChattingRoom,
) {
    svc.econ.vendor.rooms.on_close_room(ctx, who);
}
