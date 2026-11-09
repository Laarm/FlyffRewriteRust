//! The messenger (Plan/21 §9, §3.4-§3.6, §4.11; WP-SOCIAL-19 and the friend half of WP-SOCIAL-08): friends, their
//! states, blocks, the login/logout presence, the join steps and the friend summon.
//!
//! | module | content |
//! |---|---|
//! | [`model`] | [`FriendState`], [`Messenger`], [`FriendView`], [`PendingSummon`], [`MAX_FRIEND`] |
//! | [`requests`] | request (by id and by name), cancel, accept, remove |
//! | [`state`] | the block toggle, `SetFriendState`, `GetFriendState` |
//! | [`presence`] | `AddFriendJoin` / `AddFriendLogout` at login and logout |
//! | [`join`] | join steps 7 (`QueryPlayerDataNtf` x N) and 8 (`AddFriendGameJoin`) |
//! | [`summon`] | the friend summon scroll: `open`, `SummonFriend`, confirm, cancel |
//! | [`seam`] | the privacy reads and the housing hook other tasks replace |
//! | [`notes`] | offline notes: `SendTag`, the `Tag` join step, `MarkNotesDelivered`, the cap of 20 (WP-SOCIAL-20) |
//! | [`block`] | `BlockNtf` notices, `BlockReq`, the friend-block predicate (WP-SOCIAL-20) |
//!
//! **One truth.** The lists of the characters in the world (the 10 s linger included) live in the
//! [`MessengerService`] (`Services.social.messenger`), built from `CharacterLoad.friends` and
//! `CharacterRow.messenger_state` at the join ([`on_join`]) and dropped at the final removal ([`on_leave`]). A friend
//! that is not in the world has no entry: its rows are in the database only, and what an online friend sees of it is
//! Offline. Memory changes first; the `DbOp`s go out in the same commit block (21 §2.4); a failed batch is logged at
//! error and the memory stays.
//!
//! **Identity (VR-001).** Every handler acts for the session's player; the legacy leading id fields do not exist in
//! the schema. A client-named character id is only a lookup key that must match a record.
//!
//! **Deviations from the plan, all deliberate.** (1) The lists are not on the avatar (`PlayerSocial.messenger` of the
//! sketch): `player_data::related` and the join contributors have no avatars to reach. (2) The state a friend SEES
//! is computed ([`MessengerService::seen_state`]), never cached (`FriendView.state` of the sketch). (3) A stored
//! messenger state loads as Online unless it is a user-set one ([`model::login_state`]). (4) `OneFriendState` is never
//! sent: its only call sites are in the non-`__RT_1025` branch of the original `OnAddFriend`.
//!
//! The privacy and ignore filters of the chat channels are `chat::route::filter` over `chat::privacy` (WP-GM-22).

pub mod block;
pub mod join;
pub mod model;
pub mod notes;
pub mod presence;
pub mod requests;
pub mod seam;
pub mod state;
pub mod summon;

pub use model::{
    FriendState, FriendView, MAX_FRIEND, MULTI_OFFLINE, MULTI_ONLINE, Messenger, PendingSummon,
    login_state,
};

use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::tickets::{Pending, ServiceTicket, TicketResult};
use flyff_common::{CharacterId, ObjId};
use flyff_db::{AtomicBatch, AtomicKind, BatchReply, CharacterLoad, DbOp};
use flyff_proto::pb;
use std::collections::BTreeMap;

/// The messengers of the characters in the world and the friend summons in flight (`Services.social.messenger`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MessengerService {
    /// The lists of the characters in the world, the 10 s linger included.
    online: BTreeMap<CharacterId, Messenger>,
    /// The summons in flight, by summoner (at most one each).
    summons: BTreeMap<CharacterId, PendingSummon>,
}

impl MessengerService {
    // ---- the read API other tasks call --------------------------------------------------------------------------

    /// The list of `c` while it is in the world.
    pub fn get(&self, c: CharacterId) -> Option<&Messenger> {
        self.online.get(&c)
    }

    /// `true` while `c` is in the world (has a list).
    pub fn in_world(&self, c: CharacterId) -> bool {
        self.online.contains_key(&c)
    }

    /// The friends of `c` in `CharacterId` order (empty while `c` is not in the world).
    pub fn friends_of(&self, c: CharacterId) -> Vec<CharacterId> {
        self.get(c)
            .map(|m| m.friends.keys().copied().collect())
            .unwrap_or_default()
    }

    /// The friends of `c` that are in the world, in `CharacterId` order.
    pub fn online_friends(&self, c: CharacterId) -> Vec<CharacterId> {
        self.get(c)
            .map(|m| {
                m.friends
                    .keys()
                    .copied()
                    .filter(|f| self.online.contains_key(f))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// `IsBlock`: whether `of` has blocked `other` - **true for a character that is not a friend of `of`** and for
    /// one whose list does not exist (KEEP, ref/14 §9.1; the friend summon and the friend-block whisper read it).
    pub fn is_blocked(&self, of: CharacterId, other: CharacterId) -> bool {
        self.get(of).is_none_or(|m| m.is_blocked(other))
    }

    /// The messengers in the world that contain `c` (the audience of a character that is itself not in the world:
    /// a deletion), in `CharacterId` order.
    pub fn lists(&self, c: CharacterId) -> Vec<CharacterId> {
        self.online
            .iter()
            .filter(|(k, m)| **k != c && m.friends.contains_key(&c))
            .map(|(k, _)| *k)
            .collect()
    }

    /// The state `viewer` sees of `friend` (`SendFriendState`, ref/03 §9.2; also the fix of the uninitialised value
    /// of a friend without an entry): Offline when the friend is not in the world, has no entry for the viewer or
    /// has the viewer blocked, else the friend's own state.
    pub fn seen_state(&self, viewer: CharacterId, friend: CharacterId) -> u32 {
        match self.online.get(&friend) {
            Some(f) if f.friends.get(&viewer).is_some_and(|v| !v.blocked) => f.my_state,
            _ => FriendState::Offline.wire(),
        }
    }

    /// The channel number `c` shows: [`MULTI_ONLINE`] in the world, [`MULTI_OFFLINE`] otherwise.
    pub fn multi_of(&self, c: CharacterId) -> u32 {
        if self.in_world(c) {
            MULTI_ONLINE
        } else {
            MULTI_OFFLINE
        }
    }

    /// The friend summon in flight of `summoner`.
    pub fn summon_of(&self, summoner: CharacterId) -> Option<&PendingSummon> {
        self.summons.get(&summoner)
    }

    /// Number of characters with a list.
    pub fn len(&self) -> usize {
        self.online.len()
    }

    /// `true` when nobody has a list.
    pub fn is_empty(&self) -> bool {
        self.online.is_empty()
    }

    // ---- the mutations (inside a commit block) -------------------------------------------------------------------

    /// The list of `c`, mutable.
    pub(crate) fn get_mut(&mut self, c: CharacterId) -> Option<&mut Messenger> {
        self.online.get_mut(&c)
    }

    /// Gives `c` the list `m` (a fresh join replaces whatever stale entry exists).
    pub(crate) fn insert(&mut self, c: CharacterId, m: Messenger) {
        self.online.insert(c, m);
    }

    /// Adds `b` to the list of `a` and `a` to the list of `b` (blocked false); an entry that does not exist (the
    /// character is not in the world) is skipped. Returns nothing: the caller checked the lists.
    pub(crate) fn link(&mut self, a: CharacterId, b: CharacterId) {
        if let Some(m) = self.online.get_mut(&a) {
            m.friends.insert(b, FriendView::default());
        }
        if let Some(m) = self.online.get_mut(&b) {
            m.friends.insert(a, FriendView::default());
        }
    }

    /// Removes `b` from the list of `a` and `a` from the list of `b`, where those lists exist.
    pub(crate) fn unlink(&mut self, a: CharacterId, b: CharacterId) {
        if let Some(m) = self.online.get_mut(&a) {
            m.friends.remove(&b);
        }
        if let Some(m) = self.online.get_mut(&b) {
            m.friends.remove(&a);
        }
    }

    /// Records the summon of `s.summoner` (replaces nothing: the caller checked that none is in flight).
    pub(crate) fn begin_summon(&mut self, s: PendingSummon) {
        self.summons.insert(s.summoner, s);
    }

    /// Drops the summon of `summoner`.
    pub(crate) fn end_summon(&mut self, summoner: CharacterId) -> Option<PendingSummon> {
        self.summons.remove(&summoner)
    }

    /// Drops every summon in which `c` is summoner or target.
    pub(crate) fn drop_summons_of(&mut self, c: CharacterId) {
        self.summons.retain(|_, s| s.summoner != c && s.target != c);
    }
}

// ---- the life cycle ------------------------------------------------------------------------------------------------

/// The join: the list of the character is built from its rows and its stored state (21 §9.1). The stored state is the
/// last one the user set: 0 and 3..=7 are kept, everything else loads as Online ([`login_state`]). Called from
/// `WorldRules::join` before the join batch is written, so the batch (`AddFriendGameJoin`) and the presence of the
/// friends find the list.
pub fn on_join(svc: &mut Services, load: &CharacterLoad) {
    let c = load.identity.id;
    let mut m = Messenger::new(login_state(load.row.messenger_state));
    for f in &load.friends {
        if f.character_id == c && f.friend_id != c {
            m.friends
                .insert(f.friend_id, FriendView { blocked: f.blocked });
        }
    }
    svc.social.messenger.drop_summons_of(c);
    svc.social.messenger.insert(c, m);
}

/// The final removal of the avatar (21 §3.4 leave, after `announce_logout`): the list and the pending summons of the
/// character go; the friends keep their entries for it (they see it offline).
pub fn on_leave(svc: &mut Services, c: CharacterId) {
    svc.social.messenger.online.remove(&c);
    svc.social.messenger.drop_summons_of(c);
}

/// The messenger part of a character deletion (21 §3.6; the commit part, the character is offline): the rows of
/// both directions go with the delete batch (`DeleteAllFriendsOf`), every online character whose list contains `c`
/// hears `RemoveFriendNtf{c}` and loses the entry, the invitations of `c` and its pending summons are dropped. The
/// delete batch can still fail afterwards (the lobby restores the directory): accepted, as the party part does.
pub fn on_character_deleted(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    c: CharacterId,
    ops: &mut Vec<DbOp>,
) {
    ops.push(DbOp::DeleteAllFriendsOf { character_id: c });
    let listeners = svc.social.messenger.lists(c);
    for f in listeners {
        if let Some(m) = svc.social.messenger.get_mut(f) {
            m.friends.remove(&c);
        }
        if let Some(o) = crate::social::party::handlers::avatar(svc, ctx, f) {
            crate::social::party::wire::to_one(ctx, o, pb::RemoveFriendNtf { id_player: c.0 });
        }
    }
    svc.social.messenger.online.remove(&c);
    svc.social.messenger.drop_summons_of(c);
    svc.social.invites.drop_player(c);
}

// ---- helpers the modules share ------------------------------------------------------------------------------------

/// What a handler knows about its actor (identity only from the session, VR-001).
#[derive(Clone, Debug)]
pub(crate) struct Actor {
    /// The avatar.
    pub obj: ObjId,
    /// The character.
    pub character: CharacterId,
    /// Its name.
    pub name: String,
}

/// The actor of `who`, when it is a live player.
pub(crate) fn actor(ctx: &GameCtx<'_>, who: PlayerRef) -> Option<Actor> {
    let p = ctx.player(who)?.player()?;
    Some(Actor {
        obj: who.0,
        character: p.ident.character_id,
        name: p.ident.name.clone(),
    })
}

/// Sends `msg` to the avatars in `list`, encoded once (nothing for an empty list).
pub(crate) fn to_each(
    svc: &Services,
    ctx: &mut GameCtx<'_>,
    list: &[ObjId],
    msg: impl Into<pb::server_msg::Msg>,
) {
    match list {
        [] => {}
        [one] => crate::social::party::wire::to_one(ctx, *one, msg),
        many => crate::social::party::wire::to_list(svc, ctx, many, msg),
    }
}

/// The avatars of the characters `ids` that are in the world, in the order given.
pub(crate) fn avatars_of(
    svc: &Services,
    ctx: &GameCtx<'_>,
    ids: impl IntoIterator<Item = CharacterId>,
) -> Vec<ObjId> {
    ids.into_iter()
        .filter_map(|c| crate::social::party::handlers::avatar(svc, ctx, c))
        .collect()
}

/// Queues `ops` as ONE batch of the given kind; a failed commit is logged at error and the memory stays (21 §2.4).
pub(crate) fn persist_batch(ctx: &mut GameCtx<'_>, kind: &'static str, ops: Vec<DbOp>) {
    let ticket = ctx.tickets.issue(Pending::Service(ServiceTicket::new(
        move |_svc, _ctx, result| match result {
            TicketResult::Committed(_) => {}
            TicketResult::Failed(e) => {
                tracing::error!(batch = kind, error = %e, "a messenger batch failed: the memory is kept (a bug, 21 §2.4)");
            }
            other => {
                tracing::error!(batch = kind, ?other, "a messenger batch completed with an unexpected result");
            }
        },
    )));
    ctx.persist.atomic(AtomicBatch {
        reply: BatchReply::Game(ticket),
        ..AtomicBatch::of_ops(AtomicKind::Other(kind), ops)
    });
}
