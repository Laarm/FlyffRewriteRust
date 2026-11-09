//! The membership and permission handlers (Plan/21 §5.5, §5.6; WP-SOCIAL-11): the invitation, its decline and
//! accept, leave and kick, rank, class, nickname, authority, salary setting and the master transfer, plus the
//! `/guildinvite` command (23 row 11). The creation, first-naming, rename-scroll and dissolution handlers are in
//! [`super::create`], the contribution, logo and notice handlers in [`super::contribution`].
//!
//! **Trust (VR-001, VR-041, VR-044).** The actor is always the session's player: a client never names the actor. A
//! join needs a LIVE invitation from exactly the named inviter for his CURRENT guild, and the inviter must still be a
//! member holding `INVITATION` (the original let anybody join any online member's guild). The texts and their order
//! are those of `CDPCacheSrvr` (`DPCacheSrvr.cpp:1185-1858`) and `CDPSrvr::InviteCompany`
//! (`DPSrvr.cpp:9926-10040`); each handler checks, then calls ONE commit primitive.

use super::commit::{self, Departure, Origin, avatar_of};
use super::create::validate_nickname;
use super::model::{GuildPower, GuildRank};
use super::wire;
use crate::chat::ingress::notice;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::gm::{CmdArgs, CmdCtx, CmdDone, CmdError, CmdResult};
use crate::social::invites::{InviteKey, InviteKind, InviteKindTag, PendingInvite};
use flyff_common::defs::text::{
    TID_DIAG_0020, TID_DIAG_0060, TID_DIAG_0061, TID_GAME_BATTLE_NOTGUILD, TID_GAME_COMACCEPTDENY,
    TID_GAME_COMACCEPTHAVECOM, TID_GAME_COMACCEPTKINGPIN, TID_GAME_COMHAVECOM,
    TID_GAME_COMLEAVEKINGPIN, TID_GAME_COMLEAVENOCOM, TID_GAME_COMLEAVENOKINGPIN,
    TID_GAME_COMOVERMEMBER, TID_GAME_GUILDAPPNOTWARRANT, TID_GAME_GUILDAPPNUMOVER,
    TID_GAME_GUILDAPPOVER, TID_GAME_GUILDCHROFFLINE, TID_GAME_GUILDCOMBAT_NOT_INVITATION_GUILD,
    TID_GAME_GUILDCOMBAT_NOT_LEAVE_GUILD, TID_GAME_GUILDCOMBAT1TO1_NOT_INVITATION_GUILD,
    TID_GAME_GUILDCOMBAT1TO1_NOTLEAVEGUILD, TID_GAME_GUILDINVAITNOTWARR, TID_GAME_GUILDNOTINCLUDE,
    TID_GAME_GUILDNOTLEVEL, TID_GAME_GUILDNOTMEMBER, TID_GAME_GUILDWARNOINVATE,
    TID_GAME_GUILDWARNOMEMBER, TID_GAME_GUILDWARRANTREGOVER,
    TID_GAME_SECRETROOM_NOT_INVITATION_GUILD,
};
use flyff_common::{CharacterId, GameMs, GuildId, ObjId};
use flyff_proto::pb;
use flyff_rules::names::{NameKind, name_error_tid};
use flyff_rules::social::{GUILD_NICKNAME_MIN_LEVEL, GUILD_SALARY_LIMIT};

/// What a handler knows about its actor.
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

/// The refusal of a request without a guild: `TID_GAME_COMLEAVENOCOM`.
fn no_guild(ctx: &mut GameCtx<'_>, who: PlayerRef) {
    notice::defined(ctx, who, TID_GAME_COMLEAVENOCOM, Vec::new());
}

/// The war lock of every roster and management action: `TID_GAME_GUILDWARNOMEMBER`.
fn war_locked(ctx: &mut GameCtx<'_>, who: PlayerRef) {
    notice::defined(ctx, who, TID_GAME_GUILDWARNOMEMBER, Vec::new());
}

// ---- GuildInviteReq ----------------------------------------------------------------------------------------------

/// `GuildInviteReq{target_obj}` (`CDPSrvr::InviteCompany`).
pub fn on_guild_invite(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::GuildInviteReq,
) {
    invite(svc, ctx, who, ObjId(m.target_obj));
}

/// `InviteCompany(inviter = who, target)`, shared with `/guildinvite`. In order (21 §5.5): the three bidder locks of
/// `GuildSeams` (a player without a guild is silently refused), the target is a player in the world (else silent), the
/// inviter holds `INVITATION`, the target is in a guild (`COMACCEPTHAVECOM "name"`), the target is in a duel (silent),
/// the inviter's guild is at war, the target is in attack mode; then the invitation is recorded for
/// `gameplay.invite_ttl_s`, the target gets `GuildInviteNtf` and the inviter `COMACCEPTKINGPIN "name"`.
pub fn invite(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, target: ObjId) {
    let Some(a) = me(ctx, who) else { return };
    let Some(gid) = svc.social.guild.guild_of(a.character) else {
        return;
    };
    let seams = svc.social.guild.seams;
    for (locked, tid) in [
        (
            seams.siege_invite_locked,
            TID_GAME_GUILDCOMBAT_NOT_INVITATION_GUILD,
        ),
        (
            seams.gc1to1_invite_locked,
            TID_GAME_GUILDCOMBAT1TO1_NOT_INVITATION_GUILD,
        ),
        (
            seams.secret_room_invite_locked,
            TID_GAME_SECRETROOM_NOT_INVITATION_GUILD,
        ),
    ] {
        if locked(svc, gid) {
            notice::defined(ctx, who, tid, Vec::new());
            return;
        }
    }
    let Some((t_char, t_name)) = ctx
        .worlds
        .player(target)
        .filter(|mv| !mv.core.deleted)
        .and_then(|mv| mv.player())
        .map(|p| (p.ident.character_id, p.ident.name.clone()))
    else {
        return;
    };
    let Some(g) = svc.social.guild.get(gid) else {
        return;
    };
    if !g.has_power(a.character, GuildPower::INVITATION) {
        notice::defined(ctx, who, TID_GAME_GUILDINVAITNOTWARR, Vec::new());
        return;
    }
    if svc.social.guild.guild_of(t_char).is_some() {
        notice::defined(ctx, who, TID_GAME_COMACCEPTHAVECOM, vec![t_name]);
        return;
    }
    if ctx
        .player(PlayerRef(target))
        .is_some_and(|mv| mv.combat.duel.duel > 0)
    {
        return;
    }
    if g.at_war() {
        war_locked(ctx, who);
        return;
    }
    if ctx
        .player(PlayerRef(target))
        .is_some_and(crate::stats::tick::is_attack_mode)
    {
        notice::defined(ctx, who, TID_GAME_BATTLE_NOTGUILD, Vec::new());
        return;
    }
    let ttl = u64::from(ctx.cfg.rules.gameplay.invite_ttl_s) * 1000;
    svc.social.invites.offer(PendingInvite {
        kind: InviteKind::Guild { guild: gid },
        from: a.character,
        to: t_char,
        expires: GameMs(ctx.clock.game_ms.0.saturating_add(ttl)),
    });
    wire::to_one(
        ctx,
        target,
        pb::GuildInviteNtf {
            id_guild: gid.0,
            id_inviter: a.character.0,
        },
    );
    notice::defined(ctx, who, TID_GAME_COMACCEPTKINGPIN, vec![t_name]);
}

/// `/guildinvite <name>` (row 11): the name goes through the directory; an offline character answers
/// `TID_DIAG_0060 "name"`, an unknown one `TID_DIAG_0061 "name"` (like `/partyinvite`), an online one is invited like
/// a `GuildInviteReq`.
pub fn cmd_invite(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let me = cx.self_player.ok_or(CmdError::NeedsSelf)?;
    let name = args.token().to_owned();
    match cx.svc.presence.by_name(&name).cloned() {
        Some(e) if e.online.is_some() => {
            if let Some(o) = e.online {
                invite(cx.svc, cx.ctx, me, o.obj);
            }
        }
        Some(_) => notice::defined(cx.ctx, me, TID_DIAG_0060, vec![format!("\"{name}\"")]),
        None => notice::defined(cx.ctx, me, TID_DIAG_0061, vec![format!("\"{name}\"")]),
    }
    Ok(CmdDone::on(name))
}

// ---- IgnoreGuildInvite -------------------------------------------------------------------------------------------

/// `IgnoreGuildInvite{inviter}` (`OnIgnoreGuildInvite`): the invitee declines. The live record from that inviter is
/// consumed and the inviter, when online, hears `TID_GAME_COMACCEPTDENY "name"`; a decline without a record is
/// ignored (a forged request learns nothing).
pub fn on_ignore_guild_invite(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::IgnoreGuildInvite,
) {
    let Some(a) = me(ctx, who) else { return };
    let inviter = CharacterId(m.inviter);
    let key = InviteKey {
        kind: InviteKindTag::Guild,
        from: inviter,
        to: a.character,
    };
    if svc.social.invites.decline(key).is_none() {
        return;
    }
    if let Some(o) = avatar_of(svc, inviter) {
        notice::defined(ctx, PlayerRef(o), TID_GAME_COMACCEPTDENY, vec![a.name]);
    }
}

// ---- AddGuildMemberReq -------------------------------------------------------------------------------------------

/// `AddGuildMemberReq{inviter}` (`CDPCacheSrvr::OnAddGuildMember`, VR-041, VR-044): the invitee accepts. In order: a
/// LIVE record from exactly `inviter` for his CURRENT guild, and the inviter still a member holding `INVITATION`
/// (else silent, the forged request learns nothing); the inviter online (`GUILDCHROFFLINE`); the rejoin cooldown
/// (only with `features.remove_guild_cooldown = false`, GMs exempt: `GUILDNOTINCLUDE` to both); the guild at war
/// (`GUILDWARNOINVATE`); the sender already in a guild (`COMHAVECOM`); the guild full (`COMOVERMEMBER`). The record is
/// consumed only on success; the newcomer is a Rookie of class 0.
pub fn on_add_guild_member(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::AddGuildMemberReq,
) {
    let Some(a) = me(ctx, who) else { return };
    let inviter = CharacterId(m.inviter);
    let key = InviteKey {
        kind: InviteKindTag::Guild,
        from: inviter,
        to: a.character,
    };
    let Some(InviteKind::Guild { guild }) = svc
        .social
        .invites
        .get(key, ctx.clock.game_ms)
        .map(|i| i.kind)
    else {
        return;
    };
    let Some(g) = svc.social.guild.get(guild) else {
        return;
    };
    if svc.social.guild.guild_of(inviter) != Some(guild)
        || !g.has_power(inviter, GuildPower::INVITATION)
    {
        return;
    }
    let Some(inviter_obj) = avatar_of(svc, inviter).filter(|o| ctx.worlds.player(*o).is_some())
    else {
        notice::defined(ctx, who, TID_GAME_GUILDCHROFFLINE, Vec::new());
        return;
    };
    let is_gm = ctx
        .player(who)
        .and_then(|mv| mv.player())
        .is_some_and(|p| p.ident.authority.is_gm());
    if !ctx.cfg.rules.features.remove_guild_cooldown
        && !is_gm
        && svc.social.guild.in_cooldown(a.character, commit::now(ctx))
    {
        notice::defined(
            ctx,
            PlayerRef(inviter_obj),
            TID_GAME_GUILDNOTINCLUDE,
            Vec::new(),
        );
        notice::defined(ctx, who, TID_GAME_GUILDNOTINCLUDE, Vec::new());
        return;
    }
    if g.at_war() {
        notice::defined(ctx, who, TID_GAME_GUILDWARNOINVATE, Vec::new());
        return;
    }
    if svc.social.guild.guild_of(a.character).is_some() {
        notice::defined(ctx, who, TID_GAME_COMHAVECOM, Vec::new());
        return;
    }
    if g.is_full(ctx.data) {
        notice::defined(ctx, who, TID_GAME_COMOVERMEMBER, Vec::new());
        return;
    }
    let _ = svc.social.invites.take(key, ctx.clock.game_ms);
    if let Err(e) = commit::add_member(
        svc,
        ctx,
        Origin::Player(who),
        guild,
        a.character,
        GuildRank::Rookie,
        None,
    ) {
        tracing::debug!(?e, "a guild join was refused after its checks");
    }
}

// ---- RemoveGuildMemberReq ----------------------------------------------------------------------------------------

/// `RemoveGuildMemberReq{id_player}` (`OnRemoveGuildMember`): the sender leaves (its own id; the master cannot:
/// `COMLEAVEKINGPIN`) or the MASTER (by rank, not by a power bit: `COMLEAVENOKINGPIN`) removes a member (a
/// non-member answers `GUILDNOTMEMBER "guild"`). No guild is `COMLEAVENOCOM`, a running war `GUILDWARNOMEMBER`, a
/// siege bidder `GUILDCOMBAT_NOT_LEAVE_GUILD`. The effects are those of `commit::remove_member`.
pub fn on_remove_guild_member(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::RemoveGuildMemberReq,
) {
    let Some(a) = me(ctx, who) else { return };
    let Some(gid) = svc.social.guild.guild_of(a.character) else {
        no_guild(ctx, who);
        return;
    };
    let Some(g) = svc.social.guild.get(gid) else {
        return;
    };
    if g.at_war() {
        war_locked(ctx, who);
        return;
    }
    let target = CharacterId(m.id_player);
    let is_master = g.master() == Some(a.character);
    if target != a.character {
        if !g.members.contains_key(&target) {
            notice::defined(ctx, who, TID_GAME_GUILDNOTMEMBER, vec![g.name.clone()]);
            return;
        }
        if !is_master {
            notice::defined(ctx, who, TID_GAME_COMLEAVENOKINGPIN, Vec::new());
            return;
        }
    } else if is_master {
        notice::defined(ctx, who, TID_GAME_COMLEAVEKINGPIN, Vec::new());
        return;
    }
    let locked = svc.social.guild.seams.siege_leave_locked;
    if locked(svc, gid) {
        notice::defined(ctx, who, TID_GAME_GUILDCOMBAT_NOT_LEAVE_GUILD, Vec::new());
        return;
    }
    let locked = svc.social.guild.seams.gc1to1_leave_locked;
    if locked(svc, gid) {
        notice::defined(ctx, who, TID_GAME_GUILDCOMBAT1TO1_NOTLEAVEGUILD, Vec::new());
        return;
    }
    let why = if target == a.character {
        Departure::Leave
    } else {
        Departure::Kick(a.character)
    };
    if let Err(e) = commit::remove_member(svc, ctx, Origin::Player(who), gid, target, why, None) {
        tracing::debug!(?e, "a guild removal was refused after its checks");
    }
}

// ---- GuildMemberLevelReq / GuildClassReq -------------------------------------------------------------------------

/// The prefix of the rank and class requests: the actor's guild (else `COMLEAVENOCOM`), no war (`GUILDWARNOMEMBER`),
/// the target a member (else `GUILDNOTMEMBER "guild"`).
fn member_request(
    svc: &Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    actor: CharacterId,
    target: CharacterId,
) -> Option<GuildId> {
    let Some(gid) = svc.social.guild.guild_of(actor) else {
        no_guild(ctx, who);
        return None;
    };
    let g = svc.social.guild.get(gid)?;
    if g.at_war() {
        war_locked(ctx, who);
        return None;
    }
    if !g.members.contains_key(&target) {
        notice::defined(ctx, who, TID_GAME_GUILDNOTMEMBER, vec![g.name.clone()]);
        return None;
    }
    Some(gid)
}

/// `GuildMemberLevelReq{id_player, rank}` (`OnGuildMemberLv`). After the common prefix: the actor's rank must be
/// higher than the target's (`GUILDAPPOVER`) and than the new rank (`GUILDWARRANTREGOVER`), the actor needs
/// `MEMBERLEVEL` (`GUILDAPPNOTWARRANT`), the new rank must be `0..5` (`GUILDWARRANTREGOVER`) and have room under its
/// cap (`GUILDAPPNUMOVER`). The target gets the rank and class 0.
pub fn on_guild_member_level(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::GuildMemberLevelReq,
) {
    let Some(a) = me(ctx, who) else { return };
    let target = CharacterId(m.id_player);
    let Some(gid) = member_request(svc, ctx, who, a.character, target) else {
        return;
    };
    let Some(g) = svc.social.guild.get(gid) else {
        return;
    };
    let (Some(actor_rank), Some(target_rank)) = (
        g.members.get(&a.character).map(|x| x.rank),
        g.members.get(&target).map(|x| x.rank),
    ) else {
        return;
    };
    if actor_rank >= target_rank {
        notice::defined(ctx, who, TID_GAME_GUILDAPPOVER, Vec::new());
        return;
    }
    if i32::from(actor_rank.as_u8()) >= m.rank {
        notice::defined(ctx, who, TID_GAME_GUILDWARRANTREGOVER, Vec::new());
        return;
    }
    if !g.has_power(a.character, GuildPower::MEMBERLEVEL) {
        notice::defined(ctx, who, TID_GAME_GUILDAPPNOTWARRANT, Vec::new());
        return;
    }
    let Some(new) = GuildRank::from_i32(m.rank) else {
        notice::defined(ctx, who, TID_GAME_GUILDWARRANTREGOVER, Vec::new());
        return;
    };
    if g.count_rank(new) + 1 > g.rank_cap(ctx.data, new) {
        notice::defined(ctx, who, TID_GAME_GUILDAPPNUMOVER, Vec::new());
        return;
    }
    if let Err(e) = commit::set_rank(svc, ctx, Origin::Player(who), gid, target, new, None) {
        tracing::debug!(?e, "a rank change was refused after its checks");
    }
}

/// `GuildClassReq{up, id_player}` (`OnGuildClass`). After the common prefix the actor needs `LEVEL`
/// (`GUILDAPPNOTWARRANT`); the class moves one step up or down and a result outside `0..=2` is ignored silently (the
/// class change has no rank ordering, KEEP).
pub fn on_guild_class(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::GuildClassReq,
) {
    let Some(a) = me(ctx, who) else { return };
    let target = CharacterId(m.id_player);
    let Some(gid) = member_request(svc, ctx, who, a.character, target) else {
        return;
    };
    let Some(g) = svc.social.guild.get(gid) else {
        return;
    };
    if !g.has_power(a.character, GuildPower::LEVEL) {
        notice::defined(ctx, who, TID_GAME_GUILDAPPNOTWARRANT, Vec::new());
        return;
    }
    let Some(class) = g.members.get(&target).map(|x| i32::from(x.class)) else {
        return;
    };
    let new = if m.up { class + 1 } else { class - 1 };
    let Ok(new) = u8::try_from(new) else {
        return;
    };
    if new > 2 {
        return;
    }
    if let Err(e) = commit::set_class(svc, ctx, Origin::Player(who), gid, target, new) {
        tracing::debug!(?e, "a class change was refused after its checks");
    }
}

// ---- GuildNicknameReq --------------------------------------------------------------------------------------------

/// `GuildNicknameReq{id_player, alias}` (`OnGuildNickName`): no guild `COMLEAVENOCOM`, level below 10
/// `GUILDNOTLEVEL`, a running war `GUILDWARNOMEMBER`, anybody but the master is ignored; the nickname follows
/// `NameKind::GuildNickname` (2..=12 characters) and a breach answers with its text; a target that is no member is
/// ignored.
pub fn on_guild_nickname(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::GuildNicknameReq,
) {
    let Some(a) = me(ctx, who) else { return };
    let Some(gid) = svc.social.guild.guild_of(a.character) else {
        no_guild(ctx, who);
        return;
    };
    let Some(g) = svc.social.guild.get(gid) else {
        return;
    };
    if g.level < GUILD_NICKNAME_MIN_LEVEL {
        notice::defined(ctx, who, TID_GAME_GUILDNOTLEVEL, Vec::new());
        return;
    }
    if g.at_war() {
        war_locked(ctx, who);
        return;
    }
    if g.master() != Some(a.character) {
        return;
    }
    let alias = match validate_nickname(ctx, &m.alias) {
        Ok(n) => n,
        Err(e) => {
            let tid =
                u32::try_from(name_error_tid(NameKind::GuildNickname, e)).unwrap_or(TID_DIAG_0020);
            notice::defined(ctx, who, tid, Vec::new());
            return;
        }
    };
    let target = CharacterId(m.id_player);
    if !g.members.contains_key(&target) {
        return;
    }
    if let Err(e) = commit::set_alias(svc, ctx, Origin::Player(who), gid, target, &alias) {
        tracing::debug!(?e, "a nickname was refused after its checks");
    }
}

// ---- GuildAuthorityReq / GuildPenyaReq ---------------------------------------------------------------------------

/// `GuildAuthorityReq{power[5]}` (`OnGuildAuthority`): exactly five entries, master only (anybody else is ignored),
/// a running war `GUILDWARNOMEMBER`; the values are stored RAW (a master may zero his own entry: PL-092 keeps his
/// rights).
pub fn on_guild_authority(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::GuildAuthorityReq,
) {
    let Some(a) = me(ctx, who) else { return };
    let Ok(power) = <[u32; 5]>::try_from(m.power.as_slice()) else {
        return;
    };
    let Some(g) = svc.social.guild.guild_record_of(a.character) else {
        return;
    };
    if g.master() != Some(a.character) {
        return;
    }
    if g.at_war() {
        war_locked(ctx, who);
        return;
    }
    let gid = g.id;
    if let Err(e) = commit::set_powers(svc, ctx, Origin::Player(who), gid, power) {
        tracing::debug!(?e, "an authority change was refused after its checks");
    }
}

/// `GuildPenyaReq{rank, penya}` (`OnGuildPenya`): the salary of a rank. Master only (anybody else is ignored), a rank
/// above 4 is ignored (FIX VR-004: the original wrote out of bounds), a salary of 1 000 000 or more answers
/// `GuildError{2}`. There is no war lock (KEEP).
pub fn on_guild_penya(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::GuildPenyaReq,
) {
    let Some(a) = me(ctx, who) else { return };
    let Some(g) = svc.social.guild.guild_record_of(a.character) else {
        return;
    };
    if g.master() != Some(a.character) {
        return;
    }
    let Some(rank) = u8::try_from(m.rank).ok().and_then(GuildRank::from_u8) else {
        return;
    };
    if m.penya >= GUILD_SALARY_LIMIT {
        wire::to_one(ctx, a.obj, pb::GuildError { code: 2 });
        return;
    }
    let gid = g.id;
    if let Err(e) = commit::set_salary(svc, ctx, Origin::Player(who), gid, rank, m.penya) {
        tracing::debug!(?e, "a salary change was refused after its checks");
    }
}

// ---- ChgMasterReq ------------------------------------------------------------------------------------------------

/// `ChgMasterReq{new_master}` (`OnChgMaster`): the new master must be ONLINE and another player (a request naming
/// itself or an offline character is ignored); either side without membership in the same guild answers
/// `COMLEAVENOCOM`, a running war `GUILDWARNOMEMBER`; only the master may hand over (anybody else is ignored). There
/// is no level requirement. ONE batch writes both rows.
pub fn on_chg_master(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::ChgMasterReq,
) {
    let Some(a) = me(ctx, who) else { return };
    let new = CharacterId(m.new_master);
    if new == a.character {
        return;
    }
    if avatar_of(svc, new)
        .filter(|o| ctx.worlds.player(*o).is_some())
        .is_none()
    {
        return;
    }
    let gid = svc.social.guild.guild_of(a.character);
    if gid.is_none() || svc.social.guild.guild_of(new) != gid {
        no_guild(ctx, who);
        return;
    }
    let Some(gid) = gid else { return };
    let Some(g) = svc.social.guild.get(gid) else {
        return;
    };
    if g.at_war() {
        war_locked(ctx, who);
        return;
    }
    if g.master() != Some(a.character) {
        return;
    }
    if let Err(e) = commit::change_master(svc, ctx, Origin::Player(who), gid, new, None) {
        tracing::debug!(?e, "a master change was refused after its checks");
    }
}
