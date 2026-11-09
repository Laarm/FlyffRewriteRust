//! The block toggle, the own state and the state query (Plan/21 §9.2; `CDPCacheSrvr::OnFriendInterceptState`,
//! `OnSetFrinedState`, `OnGetFriendState`, `SendFriendState`, `SendSetFriendState`).
//!
//! **The state a friend sees.** `GetFriendStateNtf`, `FriendNoIntercept` and the join list carry
//! [`MessengerService::seen_state`](super::MessengerService::seen_state): Offline when the friend is not in the world,
//! does not list the viewer or has blocked him, else the friend's own state. The client polls `GetFriendStateReq`;
//! `multi_no` is 1 for a friend in the world and 100 for one that is not.
//!
//! **`OneFriendState` is never sent**: its only call sites are in the non-`__RT_1025` branch of the original
//! `OnAddFriend`, which this build does not use.

use super::model::{FriendState, MULTI_ONLINE};
use super::{actor, avatars_of, to_each};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::social::party::handlers::avatar;
use crate::social::party::now_secs;
use crate::social::party::wire::to_one;
use flyff_common::CharacterId;
use flyff_db::{DbOp, FriendRecord};
use flyff_proto::pb;

/// `FriendInterceptStateReq{id_friend}`: toggles MY block flag on a friend (a non-friend is ignored); the friend's
/// own flag is never touched. Block: `FriendInterceptStateNtf{me, friend}` to me and to the friend if he is in the
/// world. Unblock: me `FriendNoIntercept{friend, state}`, then the friend - if in the world - `AddFriendJoin{me, my
/// state, 1}` (also when he blocked me, as the original). Both write `UpsertFriend` of my row (the SQL keeps
/// `created_at`).
pub fn on_friend_intercept_state(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::FriendInterceptStateReq,
) {
    let Some(a) = actor(ctx, who) else { return };
    let friend = CharacterId(m.id_friend);
    let Some(view) = svc
        .social
        .messenger
        .get_mut(a.character)
        .and_then(|x| x.friends.get_mut(&friend))
    else {
        return;
    };
    view.blocked = !view.blocked;
    let blocked = view.blocked;
    let my_state = svc
        .social
        .messenger
        .get(a.character)
        .map_or(FriendState::Online.wire(), |x| x.my_state);
    ctx.persist.op(DbOp::UpsertFriend(FriendRecord {
        character_id: a.character,
        friend_id: friend,
        blocked,
        created_at: now_secs(ctx),
    }));
    let f_obj = avatar(svc, ctx, friend);
    if blocked {
        let ntf = pb::FriendInterceptStateNtf {
            id_player: a.character.0,
            id_friend: friend.0,
        };
        to_one(ctx, a.obj, ntf);
        if let Some(o) = f_obj {
            to_one(ctx, o, ntf);
        }
    } else {
        let state = svc.social.messenger.seen_state(a.character, friend);
        to_one(
            ctx,
            a.obj,
            pb::FriendNoIntercept {
                id_friend: friend.0,
                state: i32::try_from(state).unwrap_or(0),
            },
        );
        if let Some(o) = f_obj {
            to_one(
                ctx,
                o,
                pb::AddFriendJoin {
                    id_friend: a.character.0,
                    state: my_state,
                    multi_no: MULTI_ONLINE,
                },
            );
        }
    }
}

/// `SetFriendStateReq{state}`: the own state. A value above 11 (or negative) is ignored (FIX: unchecked in the
/// original). `my_state = state`; `Player.misc.messenger_state` takes it - the next snapshot stores it - unless it is
/// `AutoAbsent`, which is relayed and never stored. Me `SetFriendStateNtf{me, state}`, then every friend in the
/// world that lists me and that I have NOT blocked, in `CharacterId` order, gets the same message.
pub fn on_set_friend_state(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::SetFriendStateReq,
) {
    let Some(a) = actor(ctx, who) else { return };
    let Some(state) = u32::try_from(m.state)
        .ok()
        .filter(|s| FriendState::from_wire(*s).is_some())
    else {
        return;
    };
    let Some(mine) = svc.social.messenger.get_mut(a.character) else {
        return;
    };
    mine.my_state = state;
    if state != FriendState::AutoAbsent.wire()
        && let Some(p) = ctx.worlds.player_mut(a.obj).and_then(|mv| mv.player_mut())
    {
        p.misc.messenger_state = state;
    }
    let ntf = pb::SetFriendStateNtf {
        id_player: a.character.0,
        state,
    };
    to_one(ctx, a.obj, ntf);
    let listeners: Vec<CharacterId> = svc
        .social
        .messenger
        .get(a.character)
        .map(|me| {
            me.friends
                .iter()
                .filter(|(_, v)| !v.blocked)
                .map(|(f, _)| *f)
                .filter(|f| {
                    svc.social
                        .messenger
                        .get(*f)
                        .is_some_and(|x| x.is_friend(a.character))
                })
                .collect()
        })
        .unwrap_or_default();
    let objs = avatars_of(svc, ctx, listeners);
    to_each(svc, ctx, &objs, ntf);
}

/// `GetFriendStateReq`: one `GetFriendStateNtf` with the friends I have not blocked and the ones I have, each
/// `{id, seen state, multi}` in `CharacterId` order.
pub fn on_get_friend_state(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    _m: pb::GetFriendStateReq,
) {
    let Some(a) = actor(ctx, who) else { return };
    let Some(mine) = svc.social.messenger.get(a.character) else {
        return;
    };
    let presence = |f: &CharacterId| pb::get_friend_state_ntf::FriendPresence {
        id_friend: f.0,
        state: svc.social.messenger.seen_state(a.character, *f),
        multi_no: svc.social.messenger.multi_of(*f),
    };
    let friends = mine
        .friends
        .iter()
        .filter(|(_, v)| !v.blocked)
        .map(|(f, _)| presence(f))
        .collect();
    let blocked = mine
        .friends
        .iter()
        .filter(|(_, v)| v.blocked)
        .map(|(f, _)| presence(f))
        .collect();
    to_one(ctx, a.obj, pb::GetFriendStateNtf { friends, blocked });
}
