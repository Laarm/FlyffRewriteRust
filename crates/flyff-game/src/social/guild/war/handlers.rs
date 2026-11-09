//! The five C2S handlers of a guild war (Plan/21 §6.2-§6.3; `CDPCacheSrvr::OnDeclWar .. OnAcptTruce`,
//! `DPCacheSrvr.cpp:2279-2598`): declare, accept, surrender, truce request and truce accept.
//!
//! **Trust (VR-001, VR-043, PL-090).** The actor is always the session's player. A war is declared master to master
//! and starts only with a LIVE declaration (`InviteKind::WarDeclaration`, `gameplay.invite_ttl_s`) from the CURRENT
//! master of the declaring guild; a truce is agreed only between the two masters of a running war and only against a
//! LIVE request (`InviteKind::Truce`) of the enemy master for THAT war. A forged or stale accept is ignored without a
//! word, so it learns nothing; a refusal keeps the record until its TTL (there is no decline message). Nothing here
//! reads `events.guild_war` (m26): the war works with the event off, combat and [`super::process()`] do not.

use super::model::{GuildWar, WarResult, WarSide};
use super::{end_war, wire};
use crate::chat::ingress::notice;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::social::guild::commit::{Origin, avatar_of, emit, now, submit};
use crate::social::invites::{InviteKey, InviteKind, InviteKindTag, PendingInvite};
use flyff_common::defs::text::{
    TID_GAME_COMDELNOTKINGPIN, TID_GAME_COMNOHAVECOM, TID_GAME_GUILDWARMASTEROFF,
    TID_GAME_GUILDWARMEMBER10, TID_GAME_GUILDWARNOETC, TID_GAME_GUILDWARNOFINDGUILD,
    TID_GAME_GUILDWARNOREQUEST, TID_GAME_GUILDWARNOTHINGGUILD, TID_GAME_GUILDWAROHTERLV6,
    TID_GAME_GUILDWAROTHERWAR, TID_GAME_GUILDWARREQLV6, TID_GAME_GUILDWARSTILLNOWAR,
};
use flyff_common::{CharacterId, GameMs, GuildId, GuildWarId, ObjId};
use flyff_db::audit::social as audit;
use flyff_db::{AtomicBatch, AtomicKind, DbOp};
use flyff_proto::pb;
use flyff_rules::social::{WAR_MIN_LEVEL, WAR_MIN_TARGET_MEMBERS, surrender_ends_war};

/// What a handler knows about its actor.
struct Me {
    character: CharacterId,
    name: String,
}

fn me(ctx: &GameCtx<'_>, who: PlayerRef) -> Option<Me> {
    let p = ctx.player(who)?.player()?;
    Some(Me {
        character: p.ident.character_id,
        name: p.ident.name.clone(),
    })
}

/// One `DefinedText` to the sender and nothing else.
fn refuse(ctx: &mut GameCtx<'_>, who: PlayerRef, tid: u32) {
    notice::defined(ctx, who, tid, Vec::new());
}

/// The game time at which a record made now expires (`gameplay.invite_ttl_s`).
fn expires(ctx: &GameCtx<'_>) -> GameMs {
    let ttl = u64::from(ctx.cfg.rules.gameplay.invite_ttl_s) * 1000;
    GameMs(ctx.clock.game_ms.0.saturating_add(ttl))
}

/// The master of `guild` and his avatar when he is online in the directory.
fn online_master(svc: &Services, guild: GuildId) -> Option<(CharacterId, ObjId)> {
    let master = svc.social.guild.get(guild)?.master()?;
    Some((master, avatar_of(svc, master)?))
}

/// The CURRENT master of `guild` when `c` is it.
fn is_master(svc: &Services, c: CharacterId, guild: GuildId) -> bool {
    svc.social
        .guild
        .get(guild)
        .is_some_and(|g| g.master() == Some(c))
}

// ---- DeclGuildWarReq -----------------------------------------------------------------------------------------------

/// `DeclGuildWarReq{target_name}` (`OnDeclWar`, `DPCacheSrvr.cpp:2430-2505`). Checks in this order, each refusal ONE
/// `DefinedText` to the sender and nothing else: in a guild `COMNOHAVECOM`; master `COMDELNOTKINGPIN`; own level >= 6
/// `GUILDWARREQLV6`; own guild not at war `GUILDWARSTILLNOWAR`; the target found by name (case-insensitive, named
/// guilds only) `GUILDWARNOTHINGGUILD`; target level >= 6 `GUILDWAROHTERLV6`; target master online `GUILDWARMASTEROFF`;
/// the target has >= 10 members, offline ones included, `GUILDWARMEMBER10` (the declarer has no minimum); target not at
/// war `GUILDWAROTHERWAR`; the target is the own guild: silent. Success records the declaration (`from` the declaring
/// master, `to` the target master, `gameplay.invite_ttl_s`; a repeat renews it) and sends `DeclGuildWarNtf` to the
/// target master only. Nothing is stored.
pub fn on_decl_guild_war(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::DeclGuildWarReq,
) {
    let Some(a) = me(ctx, who) else { return };
    let guilds = &svc.social.guild;
    let Some(own_id) = guilds.guild_of(a.character) else {
        refuse(ctx, who, TID_GAME_COMNOHAVECOM);
        return;
    };
    let Some(own) = guilds.get(own_id) else {
        return;
    };
    if own.master() != Some(a.character) {
        refuse(ctx, who, TID_GAME_COMDELNOTKINGPIN);
        return;
    }
    if own.level < WAR_MIN_LEVEL {
        refuse(ctx, who, TID_GAME_GUILDWARREQLV6);
        return;
    }
    if own.at_war() {
        refuse(ctx, who, TID_GAME_GUILDWARSTILLNOWAR);
        return;
    }
    let Some((target_id, target)) = guilds
        .by_name(&m.target_name)
        .and_then(|id| guilds.get(id).map(|g| (id, g)))
    else {
        refuse(ctx, who, TID_GAME_GUILDWARNOTHINGGUILD);
        return;
    };
    if target.level < WAR_MIN_LEVEL {
        refuse(ctx, who, TID_GAME_GUILDWAROHTERLV6);
        return;
    }
    let Some((t_master, t_obj)) = online_master(svc, target_id) else {
        refuse(ctx, who, TID_GAME_GUILDWARMASTEROFF);
        return;
    };
    if target.members.len() < WAR_MIN_TARGET_MEMBERS {
        refuse(ctx, who, TID_GAME_GUILDWARMEMBER10);
        return;
    }
    if target.at_war() {
        refuse(ctx, who, TID_GAME_GUILDWAROTHERWAR);
        return;
    }
    if target_id == own_id {
        return;
    }
    svc.social.invites.offer(PendingInvite {
        kind: InviteKind::WarDeclaration { war_from: own_id },
        from: a.character,
        to: t_master,
        expires: expires(ctx),
    });
    wire::declaration(ctx, t_obj, own_id, a.name);
}

// ---- AcptGuildWarReq -----------------------------------------------------------------------------------------------

/// `AcptGuildWarReq{id_decl_guild}` (`OnAcptWar`, `DPCacheSrvr.cpp:2507-2598`). In order: in a guild
/// `COMNOHAVECOM`; master `COMDELNOTKINGPIN`; FIX VR-043: a LIVE declaration from the CURRENT master of the guild
/// `id_decl_guild` to the sender whose payload names that guild, else the request is ignored silently, before any
/// further text (a forged accept learns not even whether the guild exists); the own guild is not at war
/// `GUILDWARNOREQUEST` (the original's text); the declaring guild is not the own one (silent); its master is online
/// `GUILDWARMASTEROFF`; it is not at war `GUILDWAROTHERWAR`. The record is consumed only on success. The war starts
/// in ONE commit block (`start_war`).
pub fn on_acpt_guild_war(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::AcptGuildWarReq,
) {
    let Some(a) = me(ctx, who) else { return };
    let Some(own_id) = svc.social.guild.guild_of(a.character) else {
        refuse(ctx, who, TID_GAME_COMNOHAVECOM);
        return;
    };
    if !is_master(svc, a.character, own_id) {
        refuse(ctx, who, TID_GAME_COMDELNOTKINGPIN);
        return;
    }
    // VR-043: the declaration of the CURRENT master of the named guild, alive, for exactly this guild
    let decl_id = GuildId(m.id_decl_guild);
    let Some(decl_master) = svc.social.guild.get(decl_id).and_then(|g| g.master()) else {
        return;
    };
    let key = InviteKey {
        kind: InviteKindTag::WarDeclaration,
        from: decl_master,
        to: a.character,
    };
    let live = svc
        .social
        .invites
        .get(key, ctx.clock.game_ms)
        .is_some_and(|i| i.kind == InviteKind::WarDeclaration { war_from: decl_id });
    if !live {
        return;
    }
    if svc.social.guild.get(own_id).is_some_and(|g| g.at_war()) {
        refuse(ctx, who, TID_GAME_GUILDWARNOREQUEST);
        return;
    }
    let Some(decl) = svc.social.guild.get(decl_id) else {
        refuse(ctx, who, TID_GAME_GUILDWARNOFINDGUILD);
        return;
    };
    if decl_id == own_id {
        return;
    }
    if online_master(svc, decl_id).is_none() {
        refuse(ctx, who, TID_GAME_GUILDWARMASTEROFF);
        return;
    }
    if decl.at_war() {
        refuse(ctx, who, TID_GAME_GUILDWAROTHERWAR);
        return;
    }
    svc.social.invites.take(key, ctx.clock.game_ms);
    start_war(svc, ctx, who, decl_id, own_id);
}

/// The war between `decl` and `acpt` starts, in ONE commit block (21 §6.2): the id, the member counts as the sizes,
/// `Guild.war` / `Guild.enemy` crossed, every member of both guilds `war_id = Some(id)`, every online member
/// `PlayerSocial.war = id`, ONE start batch (`UpsertGuildWar`, `RaiseCounter`, `SetGuildWarOfMembers`), then
/// `AcptGuildWarNtf` to every player in a world and `SetWar{id}` near each online member (the declarer's guild first,
/// `CharacterId` order) and the audit line `guild.war_start`.
fn start_war(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    decl: GuildId,
    acpt: GuildId,
) {
    let size = |svc: &Services, g: GuildId| {
        svc.social
            .guild
            .get(g)
            .map_or(0, |g| i32::try_from(g.members.len()).unwrap_or(i32::MAX))
    };
    let id = svc.social.guild_war.alloc_id();
    let war = GuildWar::start(
        id,
        WarSide::new(decl, size(svc, decl)),
        WarSide::new(acpt, size(svc, acpt)),
        now(ctx),
        ctx.clock.game_ms,
    );
    let record = war.record();
    // memory and mirrors
    let mut roster: Vec<CharacterId> = Vec::new();
    for (own, enemy) in [(decl, acpt), (acpt, decl)] {
        if let Some(g) = svc.social.guild.get_mut(own) {
            g.war = Some(id);
            g.enemy = Some(enemy);
            for m in g.members.values_mut() {
                m.war_id = Some(id);
            }
            roster.extend(g.members.keys().copied());
        }
    }
    svc.social.guild_war.insert(war);
    for c in &roster {
        svc.social.guild.sync_member(&svc.presence, ctx, *c);
    }
    // ONE batch
    submit(
        svc,
        ctx,
        "GuildWarStart",
        AtomicBatch::guild_war_start(record, [decl, acpt]),
        &[],
        None,
    );
    // the messages: everybody hears of it, the members' neighbours see the new flag
    if let Some(war) = svc.social.guild_war.get(id) {
        wire::started(svc, ctx, war);
    }
    for c in roster {
        if let Some(obj) = avatar_of(svc, c) {
            wire::set_war_near(svc, ctx, obj, id.0);
        }
    }
    emit(
        ctx,
        Origin::Player(who),
        audit::guild_war_start(id, decl, acpt),
    );
}

// ---- SurrenderReq --------------------------------------------------------------------------------------------------

/// `SurrenderReq` (`OnSurrender`, `DPCacheSrvr.cpp:2279-2337`). The war is the sender's `PlayerSocial.war`; none, or a
/// war that does not run, answers `GUILDWARNOETC` and nothing else (so does a second surrender of the same member:
/// his flag is gone). The member's `surrenders + 1`, his side's `surrenders + 1`, his `war_id` and flag cleared,
/// `SetWar{0}` near him, `SurrenderNtf{id_war, id_player, name, decl}` to the online members of both guilds (the
/// declarer's first) and ONE `Other("GuildWarSurrender")` batch (his member row and the war row). If he is the master
/// of his guild, or [`surrender_ends_war`] holds for his side (`surrenders * 100 / size > 70` on the size at the START
/// of the war), the other side wins: [`WarResult::AcptSr`] when the declarer's side surrendered,
/// [`WarResult::DeclSr`] for the acceptor's.
pub fn on_surrender(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    _m: pb::SurrenderReq,
) {
    let Some(a) = me(ctx, who) else { return };
    let flag = ctx
        .player(who)
        .and_then(|m| m.player())
        .map_or(0, |p| p.social.war);
    let id = GuildWarId(flag);
    let guilds = &svc.social.guild;
    let situation = (flag != 0)
        .then(|| svc.social.guild_war.get(id))
        .flatten()
        .zip(guilds.guild_of(a.character))
        .and_then(|(war, gid)| {
            let role = war.role_of(gid)?;
            let fights = guilds.get(gid)?.member(a.character)?.war_id == Some(id);
            Some((role, gid, fights))
        });
    let Some((role, gid, fights)) = situation else {
        refuse(ctx, who, TID_GAME_GUILDWARNOETC);
        return;
    };
    if !fights {
        refuse(ctx, who, TID_GAME_GUILDWARNOETC);
        return;
    }
    let was_master = is_master(svc, a.character, gid);
    // memory
    let member_row = {
        let Some(g) = svc.social.guild.get_mut(gid) else {
            return;
        };
        let Some(member) = g.members.get_mut(&a.character) else {
            return;
        };
        member.surrenders = member.surrenders.saturating_add(1);
        member.war_id = None;
        member.to_record(gid)
    };
    let (war_row, decl_guild, acpt_guild, ends) = {
        let Some(war) = svc.social.guild_war.get_mut(id) else {
            return;
        };
        let side = war.side_mut(role);
        side.surrenders = side.surrenders.saturating_add(1);
        let ends = was_master || surrender_ends_war(side.surrenders, side.size);
        war.last_persist = ctx.clock.game_ms;
        war.dirty = false;
        (war.record(), war.decl.guild, war.acpt.guild, ends)
    };
    svc.social
        .guild
        .sync_member(&svc.presence, ctx, a.character);
    // the messages
    wire::set_war_near(svc, ctx, who.0, 0);
    wire::to_both_guilds(
        svc,
        ctx,
        decl_guild,
        acpt_guild,
        pb::SurrenderNtf {
            id_war: id.0,
            id_player: a.character.0,
            name: a.name,
            decl: role.is_decl(),
        },
    );
    // ONE batch
    submit(
        svc,
        ctx,
        "GuildWarSurrender",
        AtomicBatch::of_ops(
            AtomicKind::Other("GuildWarSurrender"),
            vec![
                DbOp::UpsertGuildMember(member_row),
                DbOp::UpsertGuildWar(war_row),
            ],
        ),
        &[],
        None,
    );
    if ends {
        end_war(svc, ctx, id, WarResult::surrender(role.other()));
    }
}

// ---- QueryTruceReq / AcptTruce -------------------------------------------------------------------------------------

/// The war of the master `c` of a guild with a running war and the enemy master: `None` for a player who is not the
/// master of a guild at war (both truce handlers use `Guild.war`, not the player's flag).
fn truce_parties(svc: &Services, c: CharacterId) -> Option<(GuildWarId, CharacterId)> {
    let gid = svc.social.guild.guild_of(c)?;
    let g = svc.social.guild.get(gid)?;
    if g.master() != Some(c) {
        return None;
    }
    let war = svc.social.guild_war.get(g.war?)?;
    let enemy = svc.social.guild.get(war.guild(war.role_of(gid)?.other()))?;
    Some((war.id, enemy.master()?))
}

/// `QueryTruceReq` (`OnQueryTruce`, `DPCacheSrvr.cpp:2342-2385`): the sender is the master of a guild with a running
/// war and the enemy master is online, else silent (as the original). The request is recorded (`Truce{war}` from the
/// sender to the enemy master, `gameplay.invite_ttl_s`; a repeat renews it) and the enemy master gets `QueryTruceNtf`.
pub fn on_query_truce(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    _m: pb::QueryTruceReq,
) {
    let Some(a) = me(ctx, who) else { return };
    let Some((war, enemy)) = truce_parties(svc, a.character) else {
        return;
    };
    let Some(obj) = avatar_of(svc, enemy) else {
        return;
    };
    svc.social.invites.offer(PendingInvite {
        kind: InviteKind::Truce { war },
        from: a.character,
        to: enemy,
        expires: expires(ctx),
    });
    wire::truce_request(ctx, obj);
}

/// `AcptTruce` (`OnAcptTruce`, `DPCacheSrvr.cpp:2387-2412`, FIX VR-043): the sender is the master of a guild with a
/// running war AND holds a live `Truce{war}` request of the enemy master for THAT war (consumed); anything else is
/// ignored without text. The requester cannot accept his own request, a record of an older war never matches. The
/// war ends as [`WarResult::Truce`].
pub fn on_acpt_truce(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, _m: pb::AcptTruce) {
    let Some(a) = me(ctx, who) else { return };
    let Some((war, enemy)) = truce_parties(svc, a.character) else {
        return;
    };
    let key = InviteKey {
        kind: InviteKindTag::Truce,
        from: enemy,
        to: a.character,
    };
    let matches = svc
        .social
        .invites
        .get(key, ctx.clock.game_ms)
        .is_some_and(|i| i.kind == InviteKind::Truce { war });
    if !matches {
        return;
    }
    svc.social.invites.take(key, ctx.clock.game_ms);
    end_war(svc, ctx, war, WarResult::Truce);
}
