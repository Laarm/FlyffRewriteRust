//! The messenger steps of the join batch (Plan/21 §3.5, 11 §2.5 items 7 and 8). Both are also part of the re-attach
//! replay.
//!
//! * Step 7 [`QueryPlayerDataStep`]: one `QueryPlayerDataNtf{id, name, data}` for each friend (id order, an offline one
//!   with `login = 0`) and then for each guild mate (guild-core, id order), every id once, the character itself
//!   excluded, deleted or unknown characters skipped.
//! * Step 8 [`AddFriendGameJoinStep`]: ALWAYS one `AddFriendGameJoin{my_state, friends[{id, blocked, state}]}`, also for
//!   a character without friends (the client needs `my_state`). `state` is what the character sees
//!   ([`MessengerService::seen_state`](super::MessengerService::seen_state)).
//!
//! A character without friends and guild therefore changes the join batch by that one message.

use crate::outbox::Outbox;
use crate::social::player_data::ntf_of;
use crate::world::join::{JoinContributor, JoinCtx, JoinStep};
use flyff_common::CharacterId;
use flyff_proto::pb;
use std::collections::BTreeSet;

/// The character of the joiner.
fn character_of(cx: &JoinCtx<'_>) -> Option<CharacterId> {
    cx.worlds
        .player(cx.player)
        .and_then(|m| m.player())
        .map(|p| p.ident.character_id)
}

/// Step 7: the player data of the friends, then of the guild mates.
pub struct QueryPlayerDataStep;

impl JoinContributor for QueryPlayerDataStep {
    fn step(&self) -> JoinStep {
        JoinStep::QueryPlayerData
    }

    fn write(&self, cx: &JoinCtx<'_>, out: &mut Outbox) {
        let Some(me) = character_of(cx) else { return };
        let mut seen: BTreeSet<CharacterId> = BTreeSet::new();
        seen.insert(me);
        let friends = cx.svc.social.messenger.friends_of(me);
        let guild = cx.svc.social.guild.members_of(me);
        for id in friends.into_iter().chain(guild) {
            if !seen.insert(id) {
                continue;
            }
            if let Some(n) = ntf_of(cx.svc, id) {
                out.to_session(cx.session, 0, n);
            }
        }
    }
}

/// Step 8: the own state and the friend list.
pub struct AddFriendGameJoinStep;

impl JoinContributor for AddFriendGameJoinStep {
    fn step(&self) -> JoinStep {
        JoinStep::AddFriendGameJoin
    }

    fn write(&self, cx: &JoinCtx<'_>, out: &mut Outbox) {
        let Some(me) = character_of(cx) else { return };
        let messenger = &cx.svc.social.messenger;
        let (my_state, friends) = match messenger.get(me) {
            Some(m) => (
                m.my_state,
                m.friends
                    .iter()
                    .map(|(f, v)| pb::FriendEntry {
                        id_friend: f.0,
                        blocked: v.blocked,
                        state: messenger.seen_state(me, *f),
                    })
                    .collect(),
            ),
            None => (0, Vec::new()),
        };
        out.to_session(
            cx.session,
            0,
            pb::AddFriendGameJoin {
                messenger: Some(pb::Messenger { my_state, friends }),
            },
        );
    }
}
