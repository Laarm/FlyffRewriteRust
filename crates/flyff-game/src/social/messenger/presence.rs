//! The login and logout notices of the friends (Plan/21 §3.4 join step 4 and leave step 4; ref/03 §9.2 "Login" and
//! "Logout"; `CORESERVER/Player.cpp:124-247`).
//!
//! * [`on_login`]: each friend in the world that lists the newcomer, whom the newcomer has NOT blocked and whose
//!   `privacy.messenger_join` is on gets `AddFriendJoin{id, state, 1}` (KEEP: also a friend who blocked the
//!   newcomer). Only when the directory entry was OFFLINE before the join: a re-attach during the linger runs the
//!   same hook and must not repeat a notice.
//! * [`on_logout`]: `AddFriendLogout{id}` to EVERY friend in the world that lists him (also those he blocked), with the
//!   same privacy gate. The `PresenceLogout` to the whole contact set follows in `player_data::announce_logout`, and
//!   [`super::on_leave`] drops the list last.

use super::seam::wants_join_notice;
use super::{MULTI_ONLINE, avatars_of, to_each};
use crate::ctx::{GameCtx, Services};
use flyff_common::{CharacterId, ObjId};
use flyff_proto::pb;

/// The friends of `c` in the world that list `c`, with the filter `keep(friend entry of c)`, whose privacy gate
/// lets a notice through, in `CharacterId` order.
fn audience(
    svc: &Services,
    ctx: &GameCtx<'_>,
    c: CharacterId,
    keep: impl Fn(bool) -> bool,
) -> Vec<ObjId> {
    let Some(mine) = svc.social.messenger.get(c) else {
        return Vec::new();
    };
    let friends = mine
        .friends
        .iter()
        .filter(|(_, v)| keep(v.blocked))
        .map(|(f, _)| *f)
        .filter(|f| svc.social.messenger.get(*f).is_some_and(|x| x.is_friend(c)));
    avatars_of(svc, ctx, friends)
        .into_iter()
        .filter(|o| wants_join_notice(ctx, *o))
        .collect()
}

/// 21 §3.4 join step 4, after the directory marked the newcomer online (`was_online` = the entry was online before).
pub fn on_login(svc: &mut Services, ctx: &mut GameCtx<'_>, c: CharacterId, was_online: bool) {
    if was_online {
        return;
    }
    let Some(state) = svc.social.messenger.get(c).map(|m| m.my_state) else {
        return;
    };
    let to = audience(svc, ctx, c, |blocked| !blocked);
    to_each(
        svc,
        ctx,
        &to,
        pb::AddFriendJoin {
            id_friend: c.0,
            state,
            multi_no: MULTI_ONLINE,
        },
    );
}

/// 21 §3.4 leave step 4, before `announce_logout` and while the list of the leaver still exists.
pub fn on_logout(svc: &mut Services, ctx: &mut GameCtx<'_>, c: CharacterId) {
    let to = audience(svc, ctx, c, |_| true);
    to_each(svc, ctx, &to, pb::AddFriendLogout { id_friend: c.0 });
}
