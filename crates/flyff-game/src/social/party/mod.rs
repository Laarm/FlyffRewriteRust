//! Parties (Plan/21 §4; WP-SOCIAL-03..07): the party model, invitations, membership, advanced parties, EXP and
//! points, share modes, party skills, the finder, the navigator marks and the party summon.
//!
//! | module | content | WP |
//! |---|---|---|
//! | [`model`] | `Party`, `PartySlot`, the share modes, `PartyNames`, [`PartyService`] (pure membership operations) | 03 |
//! | [`link`] | [`PartyLink`]: the mirror of the party on each online member for the world phase | 03, 05 |
//! | [`wire`] | `PartyInfo`, `PartyMember`, the senders and the `ErrorParty` codes | 03 |
//! | [`handlers`] | `MemberRequest`, `MemberRequestCancel`, `AddPartyMember`, `RemovePartyMember`, `PartyChangeLeader`, `/partyinvite` | 03 |
//! | [`names`] | `ChangeTroup`, `ChanPartyName`, the share-mode requests, the party-name rule | 04 |
//! | [`points`] | [`points::party_points`] (`CParty::GetPoint`) and the `/plv` level setter | 04 |
//! | [`share`] | the queries of docs 16/19: `members_in_range`, `item_share`, the loot cursor | 05 |
//! | [`skills`] | party skills, mode timers, PARSKILL and the effect queries | 06 |
//! | [`finder`] | the finder list, the allow-join flag, `SetNaviPointReq`, the party map info | 07 |
//! | [`summon`] | the party summon scroll | 07 |
//!
//! **One truth.** The membership lives in [`PartyService`] (`Services.social.party`); `PlayerSocial.party` and
//! `PlayerSocial.party_link` on the online members are mirrors rebuilt by [`sync_links`] after every change.
//! Parties are never persisted (KQ-21); the stored advanced-party names are (`party_names`).
//!
//! **Logout and login.** A member that leaves the world stays in its party as an offline slot for 600 s
//! ([`on_logout`], [`worker`]); a member that joins again is reconnected before the join batch is written
//! ([`on_reconnect`]), so its own `PartyMember` lists it as online.

pub mod finder;

pub mod handlers;
pub mod join;
pub mod link;
pub mod model;
pub mod names;

pub mod points;

pub mod share;
pub mod skills;

pub mod summon;

pub mod wire;

pub use link::{
    LinkMember, PartyLink, Stretching, build_link, clear_link, sync_links, write_cursor,
};
pub use model::{
    ExpShareMode, ItemShareMode, MAX_PARTYMODE, MODE_FORTUNECIRCLE, MODE_GIFTBOX, MODE_LINKATTACK,
    MODE_PARSKILL, MODE_STRETCHING, Party, PartyKind, PartyNames, PartyService, PartySlot, Removal,
};

use crate::ctx::{GameCtx, PlayerRef, Services};
use flyff_common::{CharacterId, ObjId, UnixTime};
use flyff_proto::pb;
use flyff_rules::social::MAX_PARTY_MEMBERS;
use wire::{name_of, party_info, party_member_msg, to_list, to_members, to_one};

/// A non-leader member offline for more than this many seconds is removed (`60 * 10`, ref/14 §4.2).
pub const OFFLINE_PURGE_S: i64 = 600;
/// The party map info acts on every this many calls (`PARTY_MAP_SEC`, 10 s).
pub const MAP_INFO_EVERY: u32 = 10;
/// A member farther than this (squared) from another one is shown on its map (`32 m`).
pub const MAP_INFO_FAR_SQ: f32 = 32.0 * 32.0;
/// The most members a party can have.
pub const PARTY_CAP: usize = MAX_PARTY_MEMBERS;

/// The wall-clock seconds of this tick (`CTime::GetCurrentTime()`).
pub fn now_secs(ctx: &GameCtx<'_>) -> UnixTime {
    UnixTime(ctx.clock.unix_ms.0.div_euclid(1000))
}

/// The character and avatar of an online player.
pub fn identity(ctx: &GameCtx<'_>, who: PlayerRef) -> Option<CharacterId> {
    ctx.player(who)?.player().map(|p| p.ident.character_id)
}

// ---- membership changes with their messages ---------------------------------------------------------------------

/// Removes `removed` from its party and tells everybody (`OnRemovePartyMember` of the core and
/// `CDPCoreClient::OnRemovePartyMember` of the world), in the order of the original:
///
/// 1. the party is deleted when fewer than two members are left: its remaining member gets the no-party form of
///    `PartyMember` with `changed = 0`; a running party duel is cancelled first (21 §4.3, doc 17);
///    else every remaining member gets `PartyMember{changed: removed, party}`;
/// 2. the removed member (when online) gets the no-party form with its own id;
/// 3. a surviving party whose **leader** left zeroes every running mode 0..=3 with `SetPartyMode` off and announces
///    the new leader with `AddPartyChangeLeader`; the PARSKILL flag is evaluated again.
///
/// `leader_name` is the name the message carries as the leader's: the acting member (the kicker or the leaver),
/// or the leader of the party for the purge.
pub fn remove_member(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    removed: CharacterId,
    leader_name: &str,
) -> Option<Removal> {
    let party_id = svc.social.party.party_of(removed)?;
    let dissolves = svc
        .social
        .party
        .get(party_id)
        .is_some_and(|p| p.members.len() <= 2);
    if dissolves {
        // doc 17 reads the membership of the party it cancels: before the record disappears
        crate::combat::duel::on_party_dissolved(svc, ctx, party_id);
    }
    let member_name = name_of(svc, removed);
    let removed_obj = svc
        .social
        .party
        .get(party_id)
        .and_then(|p| p.slot(removed))
        .and_then(|s| s.obj);
    let removal = svc.social.party.remove_member(removed)?;
    // doc 22 §8 (task instances): `CParty::DeleteMember` starts the 3-minute kick of a leaver who stands in a dungeon
    crate::event::on_party_left(svc, ctx, removed, party_id);
    if removal.dissolved {
        for m in &removal.former {
            if let Some(o) = m.obj {
                clear_link(ctx, PlayerRef(o));
                if m.is_online() {
                    to_one(ctx, o, party_member_msg(0, leader_name, &member_name, None));
                }
            }
        }
    } else if let Some(p) = svc.social.party.get(party_id) {
        let info = party_info(svc, p);
        let targets: Vec<ObjId> = p.online_objs().collect();
        let msg = party_member_msg(removed.0, leader_name, &member_name, Some(info));
        to_list(svc, ctx, &targets, msg);
    }
    if let Some(o) = removed_obj {
        clear_link(ctx, PlayerRef(o));
        to_one(
            ctx,
            o,
            party_member_msg(removed.0, leader_name, &member_name, None),
        );
    }
    if !removal.dissolved {
        if removal.was_leader {
            skills::zero_modes_with_messages(svc, ctx, party_id);
            if let Some(new) = removal.new_leader {
                to_members(
                    svc,
                    ctx,
                    party_id,
                    pb::AddPartyChangeLeader { new_leader: new.0 },
                );
            }
        }
        sync_links(svc, ctx, party_id);
        skills::reevaluate_parskill(svc, ctx, party_id);
    }
    Some(removal)
}

// ---- logout and reconnect ---------------------------------------------------------------------------------------

/// `CPartyMng::RemoveConnection` (21 §4.3, `on_player_leaving`): the member is flagged offline, the online
/// members get `SetPartyMemberParam{id, 0, 1}`. The leader's logout passes the leadership to the first online
/// member and zeroes every mode except PARSKILL **without any `SetPartyMode`** (KEEP-QUIRK (b)); the new leader is
/// announced with `AddPartyChangeLeader` so the clients' member order follows (the original left them stale);
/// a party nobody is online in is deleted at once.
pub fn on_logout(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let Some(character) = identity(ctx, who) else {
        return;
    };
    let Some(party_id) = svc.social.party.party_of(character) else {
        return;
    };
    let now = now_secs(ctx);
    if !svc.social.party.set_offline(character, now) {
        return;
    }
    let was_leader = svc.social.party.get(party_id).and_then(|p| p.leader_id()) == Some(character);
    clear_link(ctx, who);
    to_members(
        svc,
        ctx,
        party_id,
        pb::SetPartyMemberParam {
            id_player: character.0,
            param: 0,
            value: 1,
        },
    );
    if was_leader {
        let next = svc.social.party.get(party_id).and_then(|p| {
            p.members
                .iter()
                .skip(1)
                .find(|m| m.is_online())
                .map(|m| m.id)
        });
        // every mode but PARSKILL is zeroed silently
        if let Some(p) = svc.social.party.get_mut(party_id) {
            for m in 0..MODE_PARSKILL {
                p.mode_ms[m] = 0;
            }
        }
        match next {
            Some(new) => {
                svc.social.party.swap_leader(party_id, new);
                to_members(
                    svc,
                    ctx,
                    party_id,
                    pb::AddPartyChangeLeader { new_leader: new.0 },
                );
            }
            None => {
                // nobody online: the party is deleted at once (a duel in progress ends first)
                crate::combat::duel::on_party_dissolved(svc, ctx, party_id);
                svc.social.party.delete(party_id);
                return;
            }
        }
    }
    sync_links(svc, ctx, party_id);
    skills::reevaluate_parskill(svc, ctx, party_id);
}

/// `CPartyMng::AddConnection` (21 §4.3, the join step before the join batch): the slot is online again with the
/// new avatar, the other members get `SetPartyMemberParam{id, 0, 0}`, the links are rebuilt and PARSKILL is
/// evaluated again. A character that is in no party changes nothing. The joiner's own `PartyMember` is the
/// join-batch contributor's.
pub fn on_reconnect(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let Some(character) = identity(ctx, who) else {
        return;
    };
    let Some(party_id) = svc.social.party.party_of(character) else {
        return;
    };
    if !svc.social.party.set_online(character, who.0) {
        return;
    }
    let others: Vec<ObjId> = svc
        .social
        .party
        .get(party_id)
        .map(|p| p.online_objs().filter(|o| *o != who.0).collect())
        .unwrap_or_default();
    to_list(
        svc,
        ctx,
        &others,
        pb::SetPartyMemberParam {
            id_player: character.0,
            param: 0,
            value: 0,
        },
    );
    sync_links(svc, ctx, party_id);
    skills::reevaluate_parskill(svc, ctx, party_id);
}

/// A character was deleted while it was an offline party member (21 §3.6): it leaves like a purged member.
pub fn on_character_deleted(svc: &mut Services, ctx: &mut GameCtx<'_>, c: CharacterId) {
    if svc.social.party.party_of(c).is_none() {
        return;
    }
    let leader = leader_name_of(svc, c);
    remove_member(svc, ctx, c, &leader);
}

/// The name of the leader of the party `c` belongs to.
fn leader_name_of(svc: &Services, c: CharacterId) -> String {
    svc.social
        .party
        .party_record_of(c)
        .and_then(Party::leader_id)
        .map(|l| name_of(svc, l))
        .unwrap_or_default()
}

/// The party facts the NPC dialog helpers ask for (`IsParty`, `IsPartyMaster`, `GetPartyNum`, `GetPartyLevel`;
/// Plan/20 §6.7): `None` for a player without a party.
pub fn npc_facts(svc: &Services, ctx: &GameCtx<'_>, who: ObjId) -> Option<crate::npc::PartyFacts> {
    let c = ctx.worlds.player(who)?.player()?.ident.character_id;
    let p = svc.social.party.party_record_of(c)?;
    Some(crate::npc::PartyFacts {
        size: u32::try_from(p.size()).unwrap_or(u32::MAX),
        leader: p.leader_id() == Some(c),
        level: u32::try_from(p.level).unwrap_or(0),
    })
}

// ---- the 1 s slot items -----------------------------------------------------------------------------------------

/// Slot item 15 (21 §2.2, §4.3, §4.8.3), once per second and in this order: the offline purge, the mode timers,
/// the expiry of the pending invitations. A last step refreshes the sitting-regeneration factor of the links.
pub fn worker(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    purge_offline(svc, ctx);
    skills::tick_modes(svc, ctx);
    svc.social.invites.expire(ctx.clock.game_ms);
    skills::refresh_stretching(svc, ctx);
    crate::social::cheer::process(svc, ctx);
}

/// The offline purge: a non-leader slot that has been offline for more than [`OFFLINE_PURGE_S`] seconds is removed
/// as a kick by the system (`PartyMember` to the others); a party below two members is deleted.
fn purge_offline(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    let now = now_secs(ctx).0;
    for id in svc.social.party.ids() {
        let due: Vec<CharacterId> = svc
            .social
            .party
            .get(id)
            .map(|p| {
                p.members
                    .iter()
                    .skip(1)
                    .filter(|m| {
                        m.offline_since
                            .is_some_and(|t| now.saturating_sub(t.0) > OFFLINE_PURGE_S)
                    })
                    .map(|m| m.id)
                    .collect()
            })
            .unwrap_or_default();
        for c in due {
            // the original sends the leader as the "leader" of the message
            let leader = svc
                .social
                .party
                .get(id)
                .and_then(Party::leader_id)
                .map(|l| name_of(svc, l))
                .unwrap_or_default();
            if remove_member(svc, ctx, c, &leader).is_some_and(|r| r.dissolved) {
                break;
            }
        }
    }
}

/// Slot item 12 (21 §4.9): the party map info, acting on every [`MAP_INFO_EVERY`]th call. For each member whose
/// position changed since the last send, `PartyMapInfo{index, pos}` goes to every other online member that is
/// farther than 32 m (squared 1024; the worlds are not compared, KEEP).
pub fn map_info(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    finder::map_info(svc, ctx);
}
