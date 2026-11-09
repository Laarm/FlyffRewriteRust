//! The party handlers of the core (Plan/21 §4.2, §4.3; WP-SOCIAL-03): the invitation, its decline and accept, the
//! leave and kick, the leader change, and the `/partyinvite` command.
//!
//! **Trust (VR-001, VR-040, VR-044).** The actor is always the session's player: `MemberRequest` makes the sender
//! the leader, `AddPartyMember` adds the sender and never a client-named member, `MemberRequestCancel` needs a
//! live invitation from that leader to the sender. An accept needs the invitation (or a recruiting leader for the
//! finder). The original forwarded and honoured whatever the client wrote.

use super::model::{Party, PartySlot};
use super::wire::{
    ERROR_DIFFRENTPARTY, ERROR_FULLPARTY, ERROR_NOLEADER, ERROR_NOTMAMBER, ERROR_SAMLEADER, error,
    name_of, party_info, party_member_msg, to_members, to_one,
};
use super::{identity, remove_member, skills, sync_links};
use crate::chat::ingress::notice;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::gm::{CmdArgs, CmdCtx, CmdDone, CmdError, CmdResult};
use crate::social::invites::{InviteKey, InviteKind, InviteKindTag, PendingInvite};
use flyff_common::defs::text::{
    TID_DIAG_0060, TID_DIAG_0061, TID_GAME_BATTLE_NOTPARTY, TID_GAME_GUILDCOMBAT_CANNOT_PARTY,
};
use flyff_common::defs::world::{
    WI_WORLD_GUILDWAR, WI_WORLD_GUILDWAR1TO1_0, WI_WORLD_GUILDWAR1TO1_3,
};
use flyff_common::{CharacterId, GameMs, ObjId, PartyId};
use flyff_proto::pb;

/// The text a joiner gets when the leader of the party does not recruit (KEEP-QUIRK (a), 21 §4.2 step 9).
pub const NOT_RECRUITING_TEXT: &str =
    "You have joined a group whose leader does not want to recruit new members.";

/// What a handler knows about its actor.
#[derive(Clone, Debug)]
struct Me {
    obj: ObjId,
    character: CharacterId,
    name: String,
}

fn me(ctx: &GameCtx<'_>, who: PlayerRef) -> Option<Me> {
    let p = ctx.player(who)?.player()?;
    Some(Me {
        obj: who.0,
        character: p.ident.character_id,
        name: p.ident.name.clone(),
    })
}

/// The live avatar of a character the directory lists as online.
pub(crate) fn avatar(svc: &Services, ctx: &GameCtx<'_>, c: CharacterId) -> Option<ObjId> {
    let o = svc.presence.get(c)?.online?.obj;
    ctx.worlds
        .player(o)
        .is_some_and(|m| !m.core.deleted)
        .then_some(o)
}

/// The leaders' `__PARTY_FIND` flag (`CMover::m_bPartyJoin`): per online player, default `true`.
fn allows_join(ctx: &GameCtx<'_>, leader: ObjId) -> bool {
    ctx.player(PlayerRef(leader))
        .and_then(|m| m.player())
        .is_none_or(|p| p.social.party_allow_join)
}

/// A player in a duel (solo or party) neither invites nor is invited.
pub(crate) fn in_duel(ctx: &GameCtx<'_>, o: ObjId) -> bool {
    ctx.player(PlayerRef(o))
        .is_some_and(|m| m.combat.duel.duel > 0)
}

/// Whether the player stands in the guild-war world.
pub(crate) fn in_guild_war_world(ctx: &GameCtx<'_>, o: ObjId) -> bool {
    ctx.worlds
        .core(o)
        .is_some_and(|c| c.world.0 == WI_WORLD_GUILDWAR)
}

/// A participant of the 1:1 guild combat (`IsPossibleUser`, WP-SOCIAL-26): his guild is a tender with a stage and he
/// stands in that arena's world.
pub(crate) fn gc1to1_participant(svc: &Services, ctx: &GameCtx<'_>, o: ObjId) -> bool {
    crate::social::gc1to1::is_participant(svc, ctx, o)
}

/// The world gate of the join: guild-war worlds and the combat state keep a player out of a party
/// (`__NO_PARTY_GW_JOIN`, VR-048). The 1:1 worlds are 241..=244.
fn join_blocked_by_war(ctx: &GameCtx<'_>, o: ObjId) -> bool {
    let in_world = ctx.worlds.core(o).is_some_and(|c| {
        c.world.0 == WI_WORLD_GUILDWAR
            || (WI_WORLD_GUILDWAR1TO1_0..=WI_WORLD_GUILDWAR1TO1_3).contains(&c.world.0)
    });
    in_world
        || ctx
            .player(PlayerRef(o))
            .and_then(|m| m.player())
            .is_some_and(|p| p.social.guild_combat_state == 1)
}

// ---- MemberRequest ----------------------------------------------------------------------------------------------

/// `MemberRequest{member_id, troupe}` (`InviteParty`, `DPSrvr.cpp:9874-9924`): the sender invites `member_id`.
pub fn on_member_request(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::MemberRequest,
) {
    invite(svc, ctx, who, CharacterId(m.member_id), m.troupe);
}

/// `InviteParty(leader = who, member, troupe)`, shared with `/partyinvite`. The refusals in order (21 §4.2):
/// offline target (cancel 4), a guild-war world or a 1:1 participant (`TID_GAME_GUILDCOMBAT_CANNOT_PARTY`), a duel
/// (silent), the target in a party (cancel 1), the target in attack mode (`TID_GAME_BATTLE_NOTPARTY`), the target
/// refusing invitations (cancel 3); otherwise the invitation is recorded and the target gets `PartyRequest`.
pub fn invite(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    target: CharacterId,
    troupe: bool,
) {
    let Some(a) = me(ctx, who) else { return };
    if target == a.character {
        return; // nobody invites himself
    }
    let cancel = |ctx: &mut GameCtx<'_>, mode: i32| {
        to_one(
            ctx,
            a.obj,
            pb::PartyRequestCancel {
                leader_id: a.character.0,
                member_id: target.0,
                mode,
            },
        );
    };
    let Some(t_obj) = avatar(svc, ctx, target) else {
        cancel(ctx, 4);
        return;
    };
    if in_guild_war_world(ctx, a.obj)
        || in_guild_war_world(ctx, t_obj)
        || gc1to1_participant(svc, ctx, t_obj)
    {
        notice::defined(ctx, who, TID_GAME_GUILDCOMBAT_CANNOT_PARTY, Vec::new());
        return;
    }
    if in_duel(ctx, a.obj) || in_duel(ctx, t_obj) {
        return;
    }
    if svc.social.party.party_of(target).is_some() {
        cancel(ctx, 1);
        return;
    }
    if ctx
        .player(PlayerRef(t_obj))
        .is_some_and(crate::stats::tick::is_attack_mode)
    {
        notice::defined(ctx, who, TID_GAME_BATTLE_NOTPARTY, Vec::new());
        return;
    }
    if !crate::chat::privacy::accepts_party_invite(&*ctx.worlds, t_obj) {
        cancel(ctx, 3);
        return;
    }
    let ttl = u64::from(ctx.cfg.rules.gameplay.invite_ttl_s) * 1000;
    svc.social.invites.offer(PendingInvite {
        kind: InviteKind::Party { troupe },
        from: a.character,
        to: target,
        expires: GameMs(ctx.clock.game_ms.0.saturating_add(ttl)),
    });
    let facts = |o: ObjId| {
        ctx.player(PlayerRef(o)).map(|m| {
            (
                m.mv.level,
                m.mv.job,
                m.player().map_or(0, |p| u32::from(p.appearance.sex)),
            )
        })
    };
    let (Some((ll, lj, ls)), Some((ml, mj, ms))) = (facts(a.obj), facts(t_obj)) else {
        return;
    };
    to_one(
        ctx,
        t_obj,
        pb::PartyRequest {
            leader_id: a.character.0,
            leader_level: ll,
            leader_job: lj,
            leader_sex: ls,
            member_id: target.0,
            member_level: ml,
            member_job: mj,
            member_sex: ms,
            leader_name: a.name,
            troupe,
        },
    );
}

/// `/partyinvite <name>` (row 10): the name goes through the directory; an offline character answers
/// `TID_DIAG_0060 "name"`, an unknown one `TID_DIAG_0061 "name"` (ref/24 §6.1), an online one is invited like a
/// `MemberRequest` for a normal party.
pub fn cmd_invite(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let me = cx.self_player.ok_or(CmdError::NeedsSelf)?;
    let name = args.token().to_owned();
    match cx.svc.presence.by_name(&name).cloned() {
        Some(e) if e.online.is_some() => {
            invite(cx.svc, cx.ctx, me, e.id, false);
        }
        Some(_) => notice::defined(cx.ctx, me, TID_DIAG_0060, vec![format!("\"{name}\"")]),
        None => notice::defined(cx.ctx, me, TID_DIAG_0061, vec![format!("\"{name}\"")]),
    }
    Ok(CmdDone::on(name))
}

// ---- MemberRequestCancel ----------------------------------------------------------------------------------------

/// `MemberRequestCancel{leader_id, mode}`: the invitee declines (FIX VR-001: the sender must hold a live invitation
/// from `leader_id`). The record is consumed and the leader gets `PartyRequestCancel` with the mode clamped to
/// `{0, 1, 3, 4}` (anything else counts as 0).
pub fn on_member_request_cancel(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::MemberRequestCancel,
) {
    let Some(a) = me(ctx, who) else { return };
    let leader = CharacterId(m.leader_id);
    let key = InviteKey {
        kind: InviteKindTag::Party,
        from: leader,
        to: a.character,
    };
    if svc.social.invites.decline(key).is_none() {
        return; // forged: nobody invited the sender
    }
    let mode = match m.mode {
        1 | 3 | 4 => m.mode,
        _ => 0,
    };
    if let Some(lo) = avatar(svc, ctx, leader) {
        to_one(
            ctx,
            lo,
            pb::PartyRequestCancel {
                leader_id: leader.0,
                member_id: a.character.0,
                mode,
            },
        );
    }
}

// ---- AddPartyMember ---------------------------------------------------------------------------------------------

/// `AddPartyMember{leader_id}` (`CDPCacheSrvr::OnAddPartyMember` + `CDPCoreClient::OnAddPartyMember`): the sender
/// joins the party of `leader_id`, or forms one with it. Steps in order (21 §4.2): the war-world gate
/// (`features.no_party_gw_join`, VR-048), the invitation (consumed) or the finder rule (VR-040), the leader online,
/// the sender not yet in a party (`ErrorParty` 203 to the leader), then for an existing party the leader must lead
/// it (200) and the party have room (201) and not duel; a new party gets the leader at index 0. Every online member
/// gets `PartyMember`.
pub fn on_add_party_member(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::AddPartyMember,
) {
    let Some(a) = me(ctx, who) else { return };
    let leader_c = CharacterId(m.leader_id);
    if leader_c == a.character {
        return;
    }
    if ctx.cfg.rules.features.no_party_gw_join && join_blocked_by_war(ctx, a.obj) {
        return;
    }
    let key = InviteKey {
        kind: InviteKindTag::Party,
        from: leader_c,
        to: a.character,
    };
    let invited = svc.social.invites.take(key, ctx.clock.game_ms).is_some();
    let Some(leader_obj) = avatar(svc, ctx, leader_c) else {
        return;
    };
    if !invited && !finder_join_allowed(svc, ctx, leader_c, leader_obj) {
        return;
    }
    if svc.social.party.party_of(a.character).is_some() {
        error(ctx, leader_obj, ERROR_DIFFRENTPARTY, 0);
        return;
    }
    let leader_name = name_of(svc, leader_c);
    let slot = PartySlot::online(a.character, a.obj);
    let party_id = match svc.social.party.party_of(leader_c) {
        Some(pid) => {
            let Some(p) = svc.social.party.get(pid) else {
                return;
            };
            if p.leader_id() != Some(leader_c) {
                error(ctx, leader_obj, ERROR_NOLEADER, 0);
                return;
            }
            if p.is_full() {
                error(ctx, leader_obj, ERROR_FULLPARTY, 0);
                return;
            }
            if svc.combat.duel.opposing(pid).is_some() {
                return;
            }
            // KEEP-QUIRK (a): joining a party whose leader does not recruit works, with a warning first
            if !allows_join(ctx, leader_obj) {
                notice::text(ctx, who, NOT_RECRUITING_TEXT);
            }
            if !svc.social.party.add_member(pid, slot) {
                return;
            }
            pid
        }
        None => {
            let leader_slot = PartySlot::online(leader_c, leader_obj);
            svc.social.party.create(leader_slot, slot)
        }
    };
    announce_join(svc, ctx, party_id, a.character, &leader_name, &a.name);
}

/// The finder join (VR-040): `features.party_find`, `leader` leads a party (or none yet), its leader recruits and
/// the party has room.
fn finder_join_allowed(
    svc: &Services,
    ctx: &GameCtx<'_>,
    leader: CharacterId,
    leader_obj: ObjId,
) -> bool {
    if !ctx.cfg.rules.features.party_find || !allows_join(ctx, leader_obj) {
        return false;
    }
    match svc.social.party.party_record_of(leader) {
        Some(p) => p.leader_id() == Some(leader) && !p.is_full(),
        None => false,
    }
}

/// The messages of a member that joined (or formed) `party`: `PartyMember` to every online member, the links, and
/// PARSKILL evaluated again (the original reset it at every join).
fn announce_join(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    party: PartyId,
    joined: CharacterId,
    leader_name: &str,
    member_name: &str,
) {
    if let Some(p) = svc.social.party.get(party) {
        let info = party_info(svc, p);
        to_members(
            svc,
            ctx,
            party,
            party_member_msg(joined.0, leader_name, member_name, Some(info)),
        );
    }
    sync_links(svc, ctx, party);
    skills::reevaluate_parskill(svc, ctx, party);
}

// ---- RemovePartyMember ------------------------------------------------------------------------------------------

/// `RemovePartyMember{member_id}` (`CDPCacheSrvr::OnRemovePartyMember`): the sender leaves (its own id) or the
/// leader kicks a member. The sender must be in a party that is not dueling; kicking needs the leadership and a
/// member (anything else is ignored).
pub fn on_remove_party_member(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::RemovePartyMember,
) {
    let Some(a) = me(ctx, who) else { return };
    let Some(pid) = svc.social.party.party_of(a.character) else {
        return;
    };
    if svc.combat.duel.opposing(pid).is_some() {
        return;
    }
    let target = CharacterId(m.member_id);
    if target != a.character {
        let Some(p) = svc.social.party.get(pid) else {
            return;
        };
        if p.leader_id() != Some(a.character) || !p.is_member(target) {
            return;
        }
    }
    remove_member(svc, ctx, target, &a.name);
}

// ---- PartyChangeLeader ------------------------------------------------------------------------------------------

/// `PartyChangeLeader{new_leader}` (`CDPCacheSrvr::OnPartyChangeLeader`): the sender hands the party to a member.
/// Naming itself is `ErrorParty` 209; a sender that is not the leader is ignored; a non-member is 208; an offline
/// member is ignored. Every running mode 0..=3 is zeroed with `SetPartyMode` off, the members swap and everybody
/// gets `AddPartyChangeLeader`.
pub fn on_party_change_leader(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::PartyChangeLeader,
) {
    let Some(a) = me(ctx, who) else { return };
    let Some(pid) = svc.social.party.party_of(a.character) else {
        return;
    };
    let new = CharacterId(m.new_leader);
    if new == a.character {
        error(ctx, a.obj, ERROR_SAMLEADER, 0);
        return;
    }
    let Some(p) = svc.social.party.get(pid) else {
        return;
    };
    if p.leader_id() != Some(a.character) {
        return;
    }
    if !p.is_member(new) {
        error(ctx, a.obj, ERROR_NOTMAMBER, 0);
        return;
    }
    if p.slot(new).is_none_or(|s| !s.is_online()) {
        return;
    }
    skills::zero_modes_with_messages(svc, ctx, pid);
    svc.social.party.swap_leader(pid, new);
    to_members(
        svc,
        ctx,
        pid,
        pb::AddPartyChangeLeader { new_leader: new.0 },
    );
    sync_links(svc, ctx, pid);
}

/// The party record of `who`, if the avatar is a member.
pub fn party_of_player<'s>(
    svc: &'s Services,
    ctx: &GameCtx<'_>,
    who: PlayerRef,
) -> Option<&'s Party> {
    let c = identity(ctx, who)?;
    svc.social.party.party_record_of(c)
}
