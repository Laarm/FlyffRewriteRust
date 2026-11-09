//! Pending invitations (Plan/21 §2.3, VR-040..044, PL-090): every join or accept needs a server-side record of the
//! invitation, keyed by `(kind, from, to)`.
//!
//! The record lives `gameplay.invite_ttl_s` seconds (60 by default). An accept or decline without a live record is
//! ignored silently (an honest client never sends one); a decline consumes the record; logging out of either side
//! drops its records; expiry is silent and runs in the body of `party::worker` (10 §5.4 item 15).
//!
//! Time is the game clock (`GameMs`): invitations do not outlive a restart and never read the wall clock.

use flyff_common::{CharacterId, GameMs, GuildId, GuildWarId};
use std::collections::BTreeMap;

/// What was offered (the payload a later accept needs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InviteKind {
    /// A party invitation; `troupe` asks for an advanced (named) party.
    Party {
        /// The invitation is for an advanced party.
        troupe: bool,
    },
    /// A guild invitation.
    Guild {
        /// The inviting guild.
        guild: GuildId,
    },
    /// A friend request.
    Friend,
    /// A campus invitation.
    Campus,
    /// A guild-war declaration (from master to master).
    WarDeclaration {
        /// The declaring guild.
        war_from: GuildId,
    },
    /// A truce request during a war.
    Truce {
        /// The war.
        war: GuildWarId,
    },
}

impl InviteKind {
    /// The discriminant only, as it appears in [`InviteKey`].
    pub const fn tag(self) -> InviteKindTag {
        match self {
            InviteKind::Party { .. } => InviteKindTag::Party,
            InviteKind::Guild { .. } => InviteKindTag::Guild,
            InviteKind::Friend => InviteKindTag::Friend,
            InviteKind::Campus => InviteKindTag::Campus,
            InviteKind::WarDeclaration { .. } => InviteKindTag::WarDeclaration,
            InviteKind::Truce { .. } => InviteKindTag::Truce,
        }
    }
}

/// The kind of an invitation without its payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InviteKindTag {
    /// [`InviteKind::Party`].
    Party,
    /// [`InviteKind::Guild`].
    Guild,
    /// [`InviteKind::Friend`].
    Friend,
    /// [`InviteKind::Campus`].
    Campus,
    /// [`InviteKind::WarDeclaration`].
    WarDeclaration,
    /// [`InviteKind::Truce`].
    Truce,
}

/// The identity of an invitation: at most one live record per `(kind, from, to)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InviteKey {
    /// The kind.
    pub kind: InviteKindTag,
    /// The inviter.
    pub from: CharacterId,
    /// The invitee.
    pub to: CharacterId,
}

/// One live invitation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingInvite {
    /// What was offered.
    pub kind: InviteKind,
    /// The inviter.
    pub from: CharacterId,
    /// The invitee.
    pub to: CharacterId,
    /// The game time from which the record no longer counts.
    pub expires: GameMs,
}

impl PendingInvite {
    /// The key of this record.
    pub const fn key(&self) -> InviteKey {
        InviteKey {
            kind: self.kind.tag(),
            from: self.from,
            to: self.to,
        }
    }
}

/// The table of live invitations of every kind (`Services.social.invites`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PendingInvites {
    by_key: BTreeMap<InviteKey, PendingInvite>,
}

impl PendingInvites {
    /// Records `inv`, replacing a live record of the same `(kind, from, to)` (a repeated invitation renews it).
    pub fn offer(&mut self, inv: PendingInvite) {
        self.by_key.insert(inv.key(), inv);
    }

    /// Consumes the record of `key` and returns it; `None` when it is absent or already expired at `now`.
    pub fn take(&mut self, key: InviteKey, now: GameMs) -> Option<PendingInvite> {
        let inv = self.by_key.remove(&key)?;
        (now < inv.expires).then_some(inv)
    }

    /// Whether a live record of `key` exists at `now` (nothing is consumed).
    pub fn is_live(&self, key: InviteKey, now: GameMs) -> bool {
        self.by_key.get(&key).is_some_and(|i| now < i.expires)
    }

    /// The live record of `key`, if any.
    pub fn get(&self, key: InviteKey, now: GameMs) -> Option<&PendingInvite> {
        self.by_key.get(&key).filter(|i| now < i.expires)
    }

    /// A decline: removes the record and returns it (expired or not; the answer to an expired invitation is
    /// still harmless, the caller checks the returned record's time when it matters).
    pub fn decline(&mut self, key: InviteKey) -> Option<PendingInvite> {
        self.by_key.remove(&key)
    }

    /// Logout of `who`: every record from or to it disappears silently.
    pub fn drop_player(&mut self, who: CharacterId) {
        self.by_key.retain(|k, _| k.from != who && k.to != who);
    }

    /// Drops every record whose time is up (silent; `party::worker`).
    pub fn expire(&mut self, now: GameMs) {
        self.by_key.retain(|_, i| now < i.expires);
    }

    /// Number of records (live or not yet expired by [`PendingInvites::expire`]).
    pub fn len(&self) -> usize {
        self.by_key.len()
    }

    /// `true` when no record exists.
    pub fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }
}
