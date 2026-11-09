//! Friend requests, their cancel and accept, and the removal of a friend (Plan/21 §9.2; WP-SOCIAL-19; the original
//! `CDPSrvr::OnAddFriendReqest .. OnAddFriendCancel`, `CDPCacheSrvr::OnAddFriend`, `OnRemoveFriend`,
//! `CDPCoreClient::OnAddFriend`).
//!
//! **Trust (VR-001, VR-042, PL-090).** The actor is always the session's player. A friendship needs a live
//! `InviteKind::Friend` record `requester -> me` (TTL `gameplay.invite_ttl_s`, game clock): the original made the
//! friendship on whatever the accepting client wrote. An accept, a decline or a removal without the matching record
//! is ignored silently. Both lists are checked before anything is added (PL-098): the original could add one side.
//!
//! Message order of an accept (`CDPCoreClient::OnAddFriend`): `AddFriendNtf{acceptor}` to the requester,
//! `AddFriendNtf{requester}` to the acceptor, then `TID_GAME_MSGINVATECOM "<other name>"` to each.

use super::block::{BLOCK_MESSENGER, block_notice};
use super::seam::{accepts_friend_request, friendship_ended};
use super::{Actor, actor, persist_batch};
use crate::chat::ingress::notice;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::social::invites::{InviteKey, InviteKind, InviteKindTag, PendingInvite};
use crate::social::party::handlers::{avatar, gc1to1_participant, in_duel, in_guild_war_world};
use crate::social::party::now_secs;
use crate::social::party::wire::{name_of, to_one};
use flyff_common::defs::text::{
    TID_GAME_BATTLE_NOTFRIEND, TID_GAME_GUILDCOMBAT_CANNOT_FRIENDADD, TID_GAME_MSGINVATECOM,
    TID_GAME_MSGMAXUSER,
};
use flyff_common::{CharacterId, GameMs};
use flyff_db::{DbOp, FriendRecord};
use flyff_proto::pb;

/// `AddFriendError` code: the named character is already a friend.
pub const ERROR_ALREADY_FRIEND: u32 = 1;
/// `AddFriendError` code: no character has that name.
pub const ERROR_NO_SUCH_NAME: u32 = 2;

/// The sex and job the directory holds for `c` (`CPlayerDataCenter`), 0 and 0 for an unknown character.
fn facts(svc: &Services, c: CharacterId) -> (u32, i32) {
    svc.presence
        .get(c)
        .map_or((0, 0), |e| (u32::from(e.sex), i32::from(e.job)))
}

/// Records the request `from -> to` for `gameplay.invite_ttl_s` seconds (a repeated request renews it).
fn offer(svc: &mut Services, ctx: &GameCtx<'_>, from: CharacterId, to: CharacterId) {
    let ttl = u64::from(ctx.cfg.rules.gameplay.invite_ttl_s) * 1000;
    svc.social.invites.offer(PendingInvite {
        kind: InviteKind::Friend,
        from,
        to,
        expires: GameMs(ctx.clock.game_ms.0.saturating_add(ttl)),
    });
}

fn key(from: CharacterId, to: CharacterId) -> InviteKey {
    InviteKey {
        kind: InviteKindTag::Friend,
        from,
        to,
    }
}

/// `true` when `a` lists `b` or `b` lists `a`.
fn are_friends(svc: &Services, a: CharacterId, b: CharacterId) -> bool {
    let m = &svc.social.messenger;
    m.get(a).is_some_and(|x| x.is_friend(b)) || m.get(b).is_some_and(|x| x.is_friend(a))
}

// ---- AddFriendRequestReq -------------------------------------------------------------------------------------------

/// `AddFriendRequestReq{target}` (`OnAddFriendReqest`): the sender asks the online character `target`. Refusals in
/// order: the target is the sender, unknown or offline (silent); either side in a duel (silent); either in the
/// guild-war world or a GC1:1 participant (`TID_GAME_GUILDCOMBAT_CANNOT_FRIENDADD` to the requester); already
/// friends (silent); the target in attack mode (`TID_GAME_BATTLE_NOTFRIEND`); the target refusing requests
/// (`BlockNtf{2, target}`). Otherwise the request is recorded and the target gets `AddFriendRequestNtf`.
pub fn on_add_friend_request(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::AddFriendRequestReq,
) {
    let Some(a) = actor(ctx, who) else { return };
    let target = CharacterId(m.target);
    if target == a.character {
        return; // FIX: nobody asks himself (the database would answer Conflict)
    }
    let Some(t_obj) = avatar(svc, ctx, target) else {
        return;
    };
    if in_duel(ctx, a.obj) || in_duel(ctx, t_obj) {
        return;
    }
    if in_guild_war_world(ctx, a.obj)
        || in_guild_war_world(ctx, t_obj)
        || gc1to1_participant(svc, ctx, a.obj)
        || gc1to1_participant(svc, ctx, t_obj)
    {
        notice::defined(ctx, who, TID_GAME_GUILDCOMBAT_CANNOT_FRIENDADD, Vec::new());
        return;
    }
    if are_friends(svc, a.character, target) {
        return;
    }
    if ctx
        .player(PlayerRef(t_obj))
        .is_some_and(crate::stats::tick::is_attack_mode)
    {
        notice::defined(ctx, who, TID_GAME_BATTLE_NOTFRIEND, Vec::new());
        return;
    }
    if !accepts_friend_request(ctx, t_obj) {
        refuse_by_privacy(svc, ctx, &a, target);
        return;
    }
    offer(svc, ctx, a.character, target);
    let (sex, job) = facts(svc, a.character);
    to_one(
        ctx,
        t_obj,
        pb::AddFriendRequestNtf {
            requester: a.character.0,
            sex,
            job,
            name: a.name,
        },
    );
}

/// `BlockNtf{2, <the refusing player>}` to the requester (PL-095).
fn refuse_by_privacy(svc: &Services, ctx: &mut GameCtx<'_>, a: &Actor, target: CharacterId) {
    block_notice(
        ctx,
        PlayerRef(a.obj),
        BLOCK_MESSENGER,
        &name_of(svc, target),
    );
}

// ---- AddFriendNameRequestReq ---------------------------------------------------------------------------------------

/// `AddFriendNameRequestReq{name}` (`OnAddFriendNameReqest` + the Core's `OnAddFriendNameReqest`): the name goes
/// through the directory (case-insensitive). Unknown: `AddFriendError{2, name}`; the sender: silent; already a
/// friend: `AddFriendError{1, name}`; offline: `AddFriendNameNotFound{name}`; refusing requests: `BlockNtf{2}`;
/// otherwise the request is recorded and the target gets `AddFriendNameRequestNtf`. The original has no duel, attack
/// or war-world check on this path and neither has this port.
pub fn on_add_friend_name_request(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::AddFriendNameRequestReq,
) {
    let Some(a) = actor(ctx, who) else { return };
    let Some(target) = svc.presence.by_name(&m.name).map(|e| e.id) else {
        to_one(
            ctx,
            a.obj,
            pb::AddFriendError {
                error: ERROR_NO_SUCH_NAME,
                name: m.name,
            },
        );
        return;
    };
    if target == a.character {
        return;
    }
    if are_friends(svc, a.character, target) {
        to_one(
            ctx,
            a.obj,
            pb::AddFriendError {
                error: ERROR_ALREADY_FRIEND,
                name: m.name,
            },
        );
        return;
    }
    let Some(t_obj) = avatar(svc, ctx, target) else {
        to_one(ctx, a.obj, pb::AddFriendNameNotFound { name: m.name });
        return;
    };
    if !accepts_friend_request(ctx, t_obj) {
        refuse_by_privacy(svc, ctx, &a, target);
        return;
    }
    offer(svc, ctx, a.character, target);
    let (sex, job) = facts(svc, a.character);
    to_one(
        ctx,
        t_obj,
        pb::AddFriendNameRequestNtf {
            requester: a.character.0,
            target: target.0,
            job,
            sex,
            requester_name: a.name,
        },
    );
}

// ---- AddFriendCancelReq --------------------------------------------------------------------------------------------

/// `AddFriendCancelReq{requester}`: the asked player declines. It needs the live request `requester -> me`, which it
/// consumes (a decline by anybody else leaves the real request alone); the requester, if still in the world, gets
/// `AddFriendCancelNtf`.
pub fn on_add_friend_cancel(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::AddFriendCancelReq,
) {
    let Some(a) = actor(ctx, who) else { return };
    let requester = CharacterId(m.requester);
    if svc
        .social
        .invites
        .take(key(requester, a.character), ctx.clock.game_ms)
        .is_none()
    {
        return;
    }
    if let Some(o) = avatar(svc, ctx, requester) {
        to_one(ctx, o, pb::AddFriendCancelNtf::default());
    }
}

// ---- AddFriendReq (the accept) -------------------------------------------------------------------------------------

/// `AddFriendReq{requester}`: the asked player accepts (VR-042, PL-090). The live request `requester -> me` is
/// consumed (absent, expired after 60 s, another requester, a repeated accept: silent, no database write); the
/// requester must be in the world; two characters that are already friends (crossed requests) drop silently; when a
/// list is full each FULL side gets `TID_GAME_MSGMAXUSER` and nothing is added (PL-098, FIX: the original returned
/// silently in the core and could add one side only). Otherwise both views are added (not blocked), ONE `FriendAdd`
/// batch of two `UpsertFriend` is queued and the four messages of the module documentation go out.
pub fn on_add_friend(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::AddFriendReq,
) {
    let Some(a) = actor(ctx, who) else { return };
    let requester = CharacterId(m.requester);
    if requester == a.character {
        return;
    }
    if svc
        .social
        .invites
        .take(key(requester, a.character), ctx.clock.game_ms)
        .is_none()
    {
        return;
    }
    let Some(r_obj) = avatar(svc, ctx, requester) else {
        return;
    };
    let messenger = &svc.social.messenger;
    let (Some(mine), Some(theirs)) = (messenger.get(a.character), messenger.get(requester)) else {
        return;
    };
    if mine.is_friend(requester) || theirs.is_friend(a.character) {
        // crossed requests: the other one is moot, too
        svc.social.invites.decline(key(a.character, requester));
        return;
    }
    let (requester_full, acceptor_full) = (theirs.is_full(), mine.is_full());
    if requester_full || acceptor_full {
        if requester_full {
            notice::defined(ctx, PlayerRef(r_obj), TID_GAME_MSGMAXUSER, Vec::new());
        }
        if acceptor_full {
            notice::defined(ctx, who, TID_GAME_MSGMAXUSER, Vec::new());
        }
        return;
    }
    svc.social.messenger.link(requester, a.character);
    // crossed requests: the request the other way is moot once the friendship exists
    svc.social.invites.decline(key(a.character, requester));
    let requester_name = name_of(svc, requester);
    persist_batch(
        ctx,
        "FriendAdd",
        FriendRecord::pair(requester, a.character, now_secs(ctx))
            .into_iter()
            .map(DbOp::UpsertFriend)
            .collect(),
    );
    to_one(
        ctx,
        r_obj,
        pb::AddFriendNtf {
            id_player: a.character.0,
            name: a.name.clone(),
        },
    );
    to_one(
        ctx,
        a.obj,
        pb::AddFriendNtf {
            id_player: requester.0,
            name: requester_name.clone(),
        },
    );
    notice::defined(ctx, PlayerRef(r_obj), TID_GAME_MSGINVATECOM, vec![a.name]);
    notice::defined(ctx, who, TID_GAME_MSGINVATECOM, vec![requester_name]);
}

// ---- RemoveFriendReq -----------------------------------------------------------------------------------------------

/// `RemoveFriendReq{id_friend}` (`OnRemoveFriend` + `CDPCoreClient::OnRemovefriend`): the named character must be a
/// friend (anything else is ignored). Both views go, `DeleteFriendPair` is queued, the remover gets
/// `RemoveFriendNtf{friend}`, the friend - if in the world - `RemoveFriendState{remover}` and `RemoveFriendNtf{remover}`;
/// then the housing seam runs. An offline friend only needs the rows gone.
pub fn on_remove_friend(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::RemoveFriendReq,
) {
    let Some(a) = actor(ctx, who) else { return };
    let friend = CharacterId(m.id_friend);
    if !svc
        .social
        .messenger
        .get(a.character)
        .is_some_and(|x| x.is_friend(friend))
    {
        return;
    }
    svc.social.messenger.unlink(a.character, friend);
    ctx.persist.op(DbOp::DeleteFriendPair {
        a: a.character,
        b: friend,
    });
    to_one(
        ctx,
        a.obj,
        pb::RemoveFriendNtf {
            id_player: friend.0,
        },
    );
    if let Some(f_obj) = avatar(svc, ctx, friend) {
        to_one(
            ctx,
            f_obj,
            pb::RemoveFriendState {
                remover: a.character.0,
            },
        );
        to_one(
            ctx,
            f_obj,
            pb::RemoveFriendNtf {
                id_player: a.character.0,
            },
        );
    }
    friendship_ended(svc, ctx, a.character, friend);
}
