//! [`PartyLink`]: the party facts the world phase reads from the mover (Plan/21 §2.8, §4.7, §4.8.5).
//!
//! The pickup of a ground item, the damage pipeline and the sitting regeneration run inside the world driver,
//! where only the object store is reachable - no `Services`. The party service therefore mirrors, for every
//! online member, the few facts those code paths need into `PlayerSocial.party_link`:
//! the members in party order with their avatars, the kind, the item share mode, the round-robin cursor, which
//! modes run and the stretching factor of the sitting regeneration. The service stays the only writer of the
//! membership; a link is rebuilt by [`sync_links`] after every change of the party.
//!
//! The **cursor** is the one exception: a pickup moves it inside the world phase, so while a member is online its
//! links carry the authoritative value (all links of a party always agree); `Party.loot_cursor` is the copy a
//! rebuilt link starts from when nobody holds a link yet.

use super::model::{ItemShareMode, MAX_PARTYMODE, Party};
use crate::ctx::{GameCtx, PlayerRef, Services};
use flyff_common::{CharacterId, ObjId, PartyId};
use flyff_rules::social::PartyKind;
use smallvec::SmallVec;

/// One member as the world phase sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkMember {
    /// The character.
    pub id: CharacterId,
    /// The avatar while the member is online.
    pub obj: Option<ObjId>,
}

/// The mirror of a party on one online member (see the module documentation).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartyLink {
    /// The party.
    pub id: PartyId,
    /// Normal or advanced.
    pub kind: PartyKind,
    /// The members in party order, index 0 the leader.
    pub members: SmallVec<[LinkMember; 8]>,
    /// `m_nTroupeShareItem`.
    pub item_share: ItemShareMode,
    /// `m_nGetItemPlayerId`: the round-robin cursor (see the module documentation).
    pub loot_cursor: CharacterId,
    /// The modes that run: timers 0..=3 positive, mode 4 the PARSKILL flag.
    pub modes: [bool; MAX_PARTYMODE],
    /// The sitting regeneration factor the party grants this member right now (`None` = 1.0), refreshed every
    /// second by `party::worker` and at every change of the party.
    pub stretching: Option<Stretching>,
}

/// The factor of `PARTY_STRETCHING_MODE` on the sitting regeneration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stretching {
    /// 1.5 (the leader's job is anything else).
    X15,
    /// 1.8 (the leader is an Assist, Ringmaster or Billposter).
    X18,
}

impl Stretching {
    /// The multiplier.
    pub const fn factor(self) -> f32 {
        match self {
            Stretching::X15 => 1.5,
            Stretching::X18 => 1.8,
        }
    }
}

impl PartyLink {
    /// The leader (index 0).
    pub fn leader(&self) -> Option<CharacterId> {
        self.members.first().map(|m| m.id)
    }

    /// Whether `c` leads the party.
    pub fn is_leader(&self, c: CharacterId) -> bool {
        self.leader() == Some(c)
    }

    /// Number of members, online or not (`m_nSizeofMember`).
    pub fn size(&self) -> usize {
        self.members.len()
    }

    /// The online members' avatars in party order.
    pub fn online_objs(&self) -> impl Iterator<Item = ObjId> + '_ {
        self.members.iter().filter_map(|m| m.obj)
    }

    /// `true` while `mode` runs.
    pub fn mode_running(&self, mode: usize) -> bool {
        self.modes.get(mode).copied().unwrap_or(false)
    }
}

/// Builds the link of `p` with the round-robin cursor `cursor`; `stretching` is filled in by the caller.
pub fn build_link(p: &Party, cursor: CharacterId) -> PartyLink {
    let mut modes = [false; MAX_PARTYMODE];
    for (m, running) in modes.iter_mut().enumerate() {
        *running = p.mode_running(m);
    }
    PartyLink {
        id: p.id,
        kind: p.kind,
        members: p
            .members
            .iter()
            .map(|s| LinkMember {
                id: s.id,
                obj: s.obj,
            })
            .collect(),
        item_share: p.item_share,
        loot_cursor: cursor,
        modes,
        stretching: None,
    }
}

/// The cursor a party's links currently carry: the first online member's, else the party's own copy.
fn current_cursor(ctx: &GameCtx<'_>, p: &Party) -> CharacterId {
    p.online_objs()
        .find_map(|o| {
            ctx.worlds
                .player(o)
                .and_then(|m| m.player())
                .and_then(|pl| pl.social.party_link.as_ref())
                .filter(|l| l.id == p.id)
                .map(|l| l.loot_cursor)
        })
        .unwrap_or(p.loot_cursor)
}

/// Rebuilds the links of every online member of `id` (after any change of the party) and keeps `Party.loot_cursor`
/// in step with the hot copy. A member that is not (or no longer) in the party is cleared by [`clear_link`].
pub fn sync_links(svc: &mut Services, ctx: &mut GameCtx<'_>, id: PartyId) {
    let Some(p) = svc.social.party.get(id) else {
        return;
    };
    let cursor = current_cursor(ctx, p);
    let link = build_link(p, cursor);
    let objs: SmallVec<[ObjId; 8]> = p.online_objs().collect();
    if let Some(p) = svc.social.party.get_mut(id) {
        p.loot_cursor = cursor;
    }
    for o in objs {
        let stretching = super::skills::stretching_factor_in(&*svc, ctx, id, o);
        let mut l = link.clone();
        l.stretching = stretching;
        if let Some(pl) = ctx.worlds.player_mut(o).and_then(|m| m.player_mut()) {
            pl.social.party = Some(id);
            pl.social.party_link = Some(l);
        }
    }
}

/// Clears the party mirror of an avatar (it left the party or logged out).
pub fn clear_link(ctx: &mut GameCtx<'_>, who: PlayerRef) {
    if let Some(pl) = ctx.worlds.player_mut(who.0).and_then(|m| m.player_mut()) {
        pl.social.party = None;
        pl.social.party_link = None;
    }
}

/// Moves the round-robin cursor of `link`'s party to `receiver` on every online member's link (a pickup in the
/// world phase, where the object store is all there is): `set_loot_cursor`.
pub fn write_cursor(
    store: &mut crate::world::ObjectStore,
    link: &PartyLink,
    receiver: CharacterId,
) {
    for o in link.online_objs() {
        if let Some(pl) = store.mover_mut(o).and_then(|m| m.player_mut())
            && let Some(l) = pl.social.party_link.as_mut()
            && l.id == link.id
        {
            l.loot_cursor = receiver;
        }
    }
}
