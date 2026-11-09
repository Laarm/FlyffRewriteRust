//! Block notices (Plan/21 §9.4, §10.6; WP-SOCIAL-20; PL-095, VR-001): `BlockNtf{gu, name}` tells a player that
//! another refused or cannot take part in something, `BlockReq{gu, target_name}` is the client-generated half.
//!
//! | `gu` | meaning | origin |
//! |---|---|---|
//! | 1 | whisper/`/say` refused by `privacy.whisper` (`TID_GAME_MSGCHATDENY`) | server ([`block_notice`]) |
//! | 2 | friend request refused by `privacy.messenger` (`TID_GAME_MSGDENY`) | server |
//! | 3 | trade refused by `privacy.trade` (`TID_GAME_TRADEDENY`) | server |
//! | 4 repairing, 5 already trading, 6 friend confirm open, 7 party confirm open, 8 guild bank / angel window open | the refusing client's UI state, which the server cannot know | client ([`on_block_req`]) |
//!
//! The legacy client sent codes 1-3 itself (the receiving client refused and told the sender); a modified client
//! could therefore forge "X refused your whisper/trade". The server generates 1-3 now and ignores them in a
//! `BlockReq` (FIX). `BlockReq` has no pending-request check (as in the plan): a modified client can still send 4-8
//! to any online player.

use super::actor;
use crate::chat::privacy;
use crate::chat::send_to;
use crate::ctx::{GameCtx, PlayerRef, Services};
use flyff_common::CharacterId;
use flyff_proto::pb;

/// `BlockNtf` code: whisper or messenger IM refused by `privacy.whisper`.
pub const BLOCK_WHISPER: u32 = 1;
/// `BlockNtf` code: friend request refused by `privacy.messenger`.
pub const BLOCK_MESSENGER: u32 = 2;
/// `BlockNtf` code: trade request refused by `privacy.trade`.
pub const BLOCK_TRADE: u32 = 3;
/// First `gu` a client may send in a `BlockReq` (4 repairing).
pub const BLOCK_CLIENT_FIRST: u32 = 4;
/// Last `gu` a client may send in a `BlockReq` (8 guild bank / angel window open).
pub const BLOCK_CLIENT_LAST: u32 = 8;

/// `BlockNtf{gu, name: refuser_name}` to `to`: the player `refuser_name` refused what `to` asked for (21 §9.4). The
/// one helper of the whisper, friend-request and trade refusals.
pub fn block_notice(ctx: &mut GameCtx<'_>, to: PlayerRef, gu: u32, refuser_name: &str) {
    send_to(
        ctx,
        to,
        pb::BlockNtf {
            gu,
            name: refuser_name.to_owned(),
        },
    );
}

/// Whether `by` has blocked its friend `other` (the block flag of its own friend entry). **False for a stranger**
/// and for a character without a list: unlike `MessengerService::is_blocked`, which is TRUE for a non-friend (the
/// summon rule), a whisper from a stranger must still arrive.
pub fn friend_blocks(svc: &Services, by: CharacterId, other: CharacterId) -> bool {
    svc.social
        .messenger
        .get(by)
        .and_then(|m| m.friends.get(&other))
        .is_some_and(|v| v.blocked)
}

/// `BlockReq{gu, target_name}` (`CDPClient::SendBlock`): the sender is the session's player (the legacy `szNameTo`
/// does not exist, VR-001). A `gu` outside 4..=8 is ignored (1-3 are server-generated). The target is resolved
/// through the player directory; when it is online and not the sender, it gets `BlockNtf{gu, name: sender name}`.
pub fn on_block_req(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, m: pb::BlockReq) {
    if !(BLOCK_CLIENT_FIRST..=BLOCK_CLIENT_LAST).contains(&m.gu) {
        return;
    }
    let Some(a) = actor(ctx, who) else { return };
    let Some(target) = svc.presence.by_name(&m.target_name).map(|e| e.id) else {
        return;
    };
    if target == a.character {
        return;
    }
    let Some(obj) = privacy::by_character(svc, ctx.worlds, target) else {
        return;
    };
    block_notice(ctx, PlayerRef(obj), m.gu, &a.name);
}
