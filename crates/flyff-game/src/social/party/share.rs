//! The party queries for the kill EXP and the loot share (Plan/21 §4.6, §4.7; WP-SOCIAL-05): who is in range of a
//! picker, the item share mode and the round-robin cursor.
//!
//! The pickup of an item runs inside the world phase, where only the object store is reachable. The queries
//! therefore exist in two forms: over the services (`members_in_range`, `loot_cursor`, ...) for handlers and tests,
//! and over the picker's [`PartyLink`] (`link_members_in_range`, `link_write_cursor`) for the pickup code. Both
//! count the same members: the online ones in the picker's world and layer with a squared 3-D distance of at most
//! `range²` (the legacy `IsValidArea`), in party order, the picker included.

use super::link::{PartyLink, write_cursor};
use super::model::ItemShareMode;
use crate::combat::duel::{DuelMember, PartyDuelView};
use crate::ctx::{GameCtx, Services};
use crate::world::{ObjectStore, Worlds};
use flyff_common::{CharacterId, ObjId, PartyId};
use smallvec::SmallVec;

/// The range of the item and gold share (`IsValidArea( .., 32.0f )`).
pub const SHARE_RANGE: f32 = 32.0;

/// A party member within reach: its character and its avatar.
pub type InRange = SmallVec<[(CharacterId, ObjId); 8]>;

/// The online members among `members` that stand in the world and layer of `picker` within `range_m` of it
/// (3-D, `d² <= range²`), in the given order. The picker counts when it passes (it always does).
pub fn in_range_of(
    store: &ObjectStore,
    members: impl IntoIterator<Item = (CharacterId, Option<ObjId>)>,
    picker: ObjId,
    range_m: f32,
) -> InRange {
    let Some(pc) = store.core(picker) else {
        return InRange::new();
    };
    let r2 = range_m * range_m;
    members
        .into_iter()
        .filter_map(|(c, o)| Some((c, o?)))
        .filter(|(_, o)| {
            store.core(*o).is_some_and(|m| {
                !m.deleted
                    && m.world == pc.world
                    && m.layer == pc.layer
                    && (m.pos - pc.pos).length_squared() <= r2
            })
        })
        .collect()
}

/// [`in_range_of`] over the members of a link (the pickup in the world phase).
pub fn link_members_in_range(
    store: &ObjectStore,
    link: &PartyLink,
    picker: ObjId,
    range_m: f32,
) -> InRange {
    in_range_of(
        store,
        link.members.iter().map(|m| (m.id, m.obj)),
        picker,
        range_m,
    )
}

/// `members_in_range(g, party, picker, range)` (21 §4.7): the online members of `party` in the picker's world and
/// layer within `range_m` (inclusive), in party order, the picker included.
pub fn members_in_range(
    svc: &Services,
    worlds: &Worlds,
    party: PartyId,
    picker: ObjId,
    range_m: f32,
) -> InRange {
    let Some(p) = svc.social.party.get(party) else {
        return InRange::new();
    };
    in_range_of(
        worlds.store(),
        p.members.iter().map(|m| (m.id, m.obj)),
        picker,
        range_m,
    )
}

/// `item_share(g, party)`: the party's item share mode (`Finder` for an unknown party).
pub fn item_share(svc: &Services, party: PartyId) -> ItemShareMode {
    svc.social
        .party
        .get(party)
        .map_or(ItemShareMode::Finder, |p| p.item_share)
}

/// `loot_cursor(g, party)`: the round-robin cursor (the leader of a new party). The hot copy is on the members'
/// links (a pickup moves it in the world phase).
pub fn loot_cursor(svc: &Services, worlds: &Worlds, party: PartyId) -> Option<CharacterId> {
    let p = svc.social.party.get(party)?;
    p.online_objs()
        .find_map(|o| {
            worlds
                .player(o)
                .and_then(|m| m.player())
                .and_then(|pl| pl.social.party_link.as_ref())
                .filter(|l| l.id == party)
                .map(|l| l.loot_cursor)
        })
        .or(Some(p.loot_cursor))
}

/// `set_loot_cursor(g, party, receiver)`: moves the cursor on every link and on the party's own copy.
pub fn set_loot_cursor(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    party: PartyId,
    receiver: CharacterId,
) {
    let Some(p) = svc.social.party.get_mut(party) else {
        return;
    };
    p.loot_cursor = receiver;
    let objs: SmallVec<[ObjId; 8]> = p.online_objs().collect();
    for o in objs {
        if let Some(l) = ctx
            .worlds
            .player_mut(o)
            .and_then(|m| m.player_mut())
            .and_then(|pl| pl.social.party_link.as_mut())
        {
            l.loot_cursor = receiver;
        }
    }
}

/// `set_loot_cursor` from the world phase: the same, over the object store and the picker's link.
pub fn link_write_cursor(store: &mut ObjectStore, link: &PartyLink, receiver: CharacterId) {
    write_cursor(store, link, receiver);
}

/// `same_party(a, b)` for the loot right (19 §6.6): both are members of one party.
pub fn same_party(svc: &Services, a: CharacterId, b: CharacterId) -> bool {
    svc.social.party.same_party(a, b)
}

/// The membership seam of the party duels (17 §8.7): installed in `Services.combat.duel.view`.
#[derive(Clone, Copy, Debug, Default)]
pub struct PartyDuelAdapter;

impl PartyDuelView for PartyDuelAdapter {
    fn members(&self, svc: &Services, party: PartyId) -> Option<Vec<DuelMember>> {
        svc.social.party.get(party).map(|p| {
            p.members
                .iter()
                .map(|m| DuelMember {
                    character: m.id,
                    obj: m.obj,
                })
                .collect()
        })
    }

    fn advanced_name(&self, svc: &Services, party: PartyId) -> Option<String> {
        svc.social
            .party
            .get(party)
            .filter(|p| p.kind == flyff_rules::social::PartyKind::Advanced)
            .map(|p| p.name.clone())
    }
}
