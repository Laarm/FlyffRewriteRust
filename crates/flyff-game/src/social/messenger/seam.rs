//! The seams of the messenger (Plan/21 §10.6, doc 22): the privacy reads and the housing hook that later tasks replace
//! without touching the handlers.
//!
//! * [`accepts_friend_request`] / [`wants_join_notice`]: the stored `privacy.messenger` / `privacy.messenger_join`
//!   flags of the receiving player, answered by the `chat::privacy` queries of WP-GM-22 (absent = true).
//! * [`friendship_ended`]: doc 22 turns it into `SetHousingVisitor{allowed: false}` both ways (WP-EVENT-16).

use crate::chat::privacy;
use crate::ctx::{GameCtx, Services};
use flyff_common::{CharacterId, ObjId};

/// `privacy.messenger` of the avatar `o` (PL-095): `false` refuses friend requests (the requester gets
/// `BlockNtf{2}`). An avatar that is not a live player accepts. The query of `chat::privacy` (WP-GM-22).
pub fn accepts_friend_request(ctx: &GameCtx<'_>, o: ObjId) -> bool {
    privacy::accepts_friend_request(&*ctx.worlds, o)
}

/// `privacy.messenger_join` of the avatar `o` (PL-095): `false` means no `AddFriendJoin` / `AddFriendLogout` is
/// sent to this player. The query of `chat::privacy` (WP-GM-22).
pub fn wants_join_notice(ctx: &GameCtx<'_>, o: ObjId) -> bool {
    privacy::wants_friend_login_notice(&*ctx.worlds, o)
}

/// A friendship ended (`RemoveFriendReq`): the housing visit permission is revoked both ways in doc 22
/// (`CHousingMng::ReqSetAllowVisit( .., FALSE )`): `event::housing::friendship_ended` (WP-EVENT-16).
pub fn friendship_ended(svc: &mut Services, ctx: &mut GameCtx<'_>, a: CharacterId, b: CharacterId) {
    crate::event::housing::friendship_ended(svc, ctx, a, b);
}
