//! The commit primitives of the guild service (Plan/21 §2.4, §5; WP-SOCIAL-10..12). Every mutation of a guild goes
//! through exactly one function here, whoever asks (a player's handler, a GM command, an NPC, the admin console, a
//! character deletion), so that all of them produce the same messages, the same persistence and the same audit.
//!
//! A primitive validates first (a refusal changes nothing), then runs ONE commit block without a fallible step:
//! memory, the `PlayerSocial` mirrors, ONE persist (a single `DbOp` for the single-entity rows of 21 §5.5, one
//! `AtomicBatch` for everything that moves items or penya or changes several rows), the client messages (which never
//! wait for `DbReply::Committed`) and the audit. A `Failed` batch is a bug: it is logged at error with the batch
//! kind and the memory is kept (21 §2.4).

use super::model::{GuildMemberState, GuildRank};
use super::ops::{CommitCb, GuildActor, GuildChange, GuildRefusal};
use super::set_mirror;
use super::wire;
use crate::audit::actor_of;
use crate::buff;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::econ::mail::commit::{can_snapshot, snapshot_for_batch};
use crate::item::container::StorageId;
use crate::item::ops::{RemoveReason, remove_item};
use crate::social::party::wire::name_of;
use crate::tickets::{Pending, ServiceTicket, TicketResult};
use flyff_common::defs::itemkind::IK3_GUILD_BUFF;
use flyff_common::{CharacterId, GuildId, ObjId, SaveGen, UnixTime};
use flyff_db::audit::social as audit;
use flyff_db::{
    AtomicBatch, AtomicKind, AuditEvent, BatchReply, DbOp, GuildBankLogKind, GuildBankLogRecord,
    SaveReason,
};
use flyff_proto::pb;
use flyff_rules::social::GUILD_REJOIN_COOLDOWN_S;
use serde_json::{Value, json};

// ---- who acts -----------------------------------------------------------------------------------------------------

/// The acting side of a commit, reduced to what the commit needs: the avatar for the audit line, whether the admin
/// batch kind applies, and `System` for the deletion of a character.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Origin {
    /// A player's own request.
    Player(PlayerRef),
    /// A GM command or an NPC acting for the avatar.
    Gm(PlayerRef),
    /// The admin console.
    Admin,
    /// The server itself (a character deletion).
    System,
}

impl From<&GuildActor> for Origin {
    fn from(a: &GuildActor) -> Origin {
        match a {
            GuildActor::Player(p) => Origin::Player(*p),
            GuildActor::Gm(p) => Origin::Gm(*p),
            GuildActor::Admin(_) => Origin::Admin,
        }
    }
}

impl Origin {
    /// The avatar behind the action, if any.
    pub(crate) fn avatar(&self) -> Option<PlayerRef> {
        match self {
            Origin::Player(p) | Origin::Gm(p) => Some(*p),
            Origin::Admin | Origin::System => None,
        }
    }

    /// The character behind the action (0 for the admin console and the server).
    pub(crate) fn character(&self, ctx: &GameCtx<'_>) -> CharacterId {
        self.avatar()
            .and_then(|p| super::character_of(ctx, p))
            .unwrap_or(CharacterId(0))
    }

    fn is_admin(&self) -> bool {
        matches!(self, Origin::Admin)
    }
}

// ---- small helpers -----------------------------------------------------------------------------------------------

/// The wall-clock seconds of this tick.
pub(crate) fn now(ctx: &GameCtx<'_>) -> UnixTime {
    crate::social::party::now_secs(ctx)
}

/// Emits an audit event with the avatar of the origin as the actor context.
pub(crate) fn emit(ctx: &GameCtx<'_>, origin: Origin, event: AuditEvent) {
    let event = match origin.avatar().and_then(|p| actor_of(ctx, p)) {
        Some(a) => event.by(a),
        None => event,
    };
    ctx.audit.emit(event);
}

/// The directory name of `c` (empty when unknown).
fn name(svc: &Services, c: CharacterId) -> String {
    name_of(svc, c)
}

/// The online avatar of `c`, when it is in the world.
pub(crate) fn avatar_of(svc: &Services, c: CharacterId) -> Option<ObjId> {
    svc.presence.get(c).and_then(|e| e.online).map(|o| o.obj)
}

/// Logs the answer of a social batch: a failure is a bug (21 §2.4), the memory is kept.
fn log_result(label: &'static str, result: &TicketResult) {
    match result {
        TicketResult::Committed(_) => {}
        TicketResult::Failed(e) => {
            tracing::error!(batch = label, error = %e, "a guild batch failed: the memory is kept (a bug, 21 §2.4)");
        }
        other => tracing::error!(
            batch = label,
            ?other,
            "a guild batch completed with an unexpected result"
        ),
    }
}

/// Queues `batch` with a ticket whose continuation logs a failure and then calls `cb`; the snapshots in
/// `saved` (avatar and generation) restart the periodic save timer of their avatars.
pub(crate) fn submit(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    label: &'static str,
    batch: AtomicBatch,
    saved: &[(PlayerRef, SaveGen)],
    cb: Option<CommitCb>,
) {
    let ticket = ctx.tickets.issue(Pending::Service(ServiceTicket::new(
        move |svc, ctx, result| {
            log_result(label, &result);
            if let Some(cb) = cb {
                cb(svc, ctx, result);
            }
        },
    )));
    ctx.persist.atomic(batch.with_ticket(ticket));
    for (p, g) in saved {
        svc.saves
            .record(p.0, *g, ctx.clock.game_ms, SaveReason::Atomic);
    }
}

/// Persists `ops` the way the origin's kind says: the admin console always writes an `AdminGuildEdit` batch; a
/// player or GM change of ONE row is a plain `ctx.persist.op` (21 §5.5) unless a continuation wants the result;
/// several rows go as one `kind` batch.
pub(crate) fn persist(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    origin: Origin,
    kind: AtomicKind,
    ops: Vec<DbOp>,
    cb: Option<CommitCb>,
) {
    if ops.is_empty() {
        return;
    }
    let label = kind.label();
    if origin.is_admin() {
        let batch = AtomicBatch::admin_guild_edit(Vec::new(), ops);
        submit(svc, ctx, "AdminGuildEdit", batch, &[], cb);
        return;
    }
    if cb.is_none() && ops.len() == 1 {
        if let Some(op) = ops.into_iter().next() {
            ctx.persist.op(op);
        }
        return;
    }
    let batch = AtomicBatch {
        kind,
        snapshots: Vec::new(),
        ops,
        reply: BatchReply::None,
    };
    submit(svc, ctx, label, batch, &[], cb);
}

/// The `guilds` row of `guild` as an op.
fn guild_op(svc: &Services, guild: GuildId) -> Option<DbOp> {
    svc.social
        .guild
        .get(guild)
        .map(|g| DbOp::UpsertGuild(g.to_record()))
}

/// The `guild_members` row of `c` as an op.
fn member_op(svc: &Services, guild: GuildId, c: CharacterId) -> Option<DbOp> {
    svc.social
        .guild
        .get(guild)
        .and_then(|g| g.members.get(&c))
        .map(|m| DbOp::UpsertGuildMember(m.to_record(guild)))
}

fn members_ids(svc: &Services, guild: GuildId) -> Vec<CharacterId> {
    svc.social
        .guild
        .get(guild)
        .map(|g| g.members.keys().copied().collect())
        .unwrap_or_default()
}

fn online(svc: &Services, guild: GuildId) -> Vec<ObjId> {
    svc.social.guild.online_objs(&svc.presence, guild)
}

// ---- creation ----------------------------------------------------------------------------------------------------

/// The messages of a guild that was just created, in the order of 21 §5.4: `CreateGuild` to everybody, then to each
/// online member (roster order) the whole guild and `SetGuild` near it.
pub(crate) fn announce_created(svc: &Services, ctx: &mut GameCtx<'_>, guild: GuildId) {
    let Some(g) = svc.social.guild.get(guild) else {
        return;
    };
    let master = g.master().unwrap_or(CharacterId(0));
    wire::to_all(
        svc,
        ctx,
        pb::CreateGuild {
            id_player: master.0,
            id_guild: guild.0,
            master_name: name(svc, master),
            guild_name: g.name.clone(),
        },
    );
    let full = wire::full(g);
    for obj in online(svc, guild) {
        wire::to_one(
            ctx,
            obj,
            pb::Guild {
                guild: Some(full.clone()),
            },
        );
        wire::set_guild_near(svc, ctx, obj, Some(guild));
    }
}

/// The audit line of a created guild.
pub(crate) fn audit_created(ctx: &GameCtx<'_>, origin: Origin, svc: &Services, guild: GuildId) {
    if let Some(g) = svc.social.guild.get(guild) {
        let master = g.master().unwrap_or(CharacterId(0));
        emit(ctx, origin, audit::guild_create(guild, &g.name, master));
    }
}

// ---- dissolution -------------------------------------------------------------------------------------------------

/// What a dissolution left to persist.
pub(crate) struct Dissolved {
    /// The rejoin cooldowns that were set (empty with `features.remove_guild_cooldown = true`).
    pub cooldowns: Vec<(CharacterId, UnixTime)>,
}

/// The memory side of a dissolution with its messages (21 §5.4): the guild leaves the service, the rejoin
/// cooldowns are set (only with `features.remove_guild_cooldown = false`, never for `except`), `DestroyGuildNtf`
/// goes to everybody, every online member loses the guild (mirror, bank window, guild buffs) and gets `SetGuild{0}`
/// near it, the guild-house teardown seam runs and the audit line is written. The caller persists
/// ([`DbOp::guild_disband_ops`]).
pub(crate) fn dissolve(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    origin: Origin,
    guild: GuildId,
    except: Option<CharacterId>,
) -> Option<Dissolved> {
    let members = members_ids(svc, guild);
    let objs: Vec<(CharacterId, ObjId)> = members
        .iter()
        .filter_map(|c| avatar_of(svc, *c).map(|o| (*c, o)))
        .collect();
    let g = svc.social.guild.remove(guild)?;
    let master = g.master();
    let master_name = master.map(|m| name(svc, m)).unwrap_or_default();
    let at = now(ctx);
    let mut cooldowns = Vec::new();
    if !ctx.cfg.rules.features.remove_guild_cooldown {
        let until = UnixTime(at.0.saturating_add(GUILD_REJOIN_COOLDOWN_S));
        for c in &members {
            if Some(*c) != except {
                svc.social.guild.set_rejoin(*c, Some(until), at);
                cooldowns.push((*c, until));
            }
        }
    }
    wire::to_all(
        svc,
        ctx,
        pb::DestroyGuildNtf {
            master_name,
            id_guild: guild.0,
        },
    );
    for (_, obj) in objs {
        // ---- guild house (doc 22 §12.5, task guild-house): the member's rest points and furniture DSTs go; the house is
        // still in the service
        let on_member_left = svc.social.guild.seams.on_member_left;
        on_member_left(svc, ctx, guild, obj, true);
        set_mirror(ctx, obj, None, 0);
        buff::api::remove_by_ik3(svc, ctx, obj, IK3_GUILD_BUFF);
        crate::item::remove_guild_cloaks(svc, ctx, PlayerRef(obj));
        wire::set_guild_near(svc, ctx, obj, None);
    }
    let on_disband = svc.social.guild.seams.on_disband;
    on_disband(svc, ctx, guild);
    emit(
        ctx,
        origin,
        audit::guild_disband(guild, &g.name, master.unwrap_or(CharacterId(0))),
    );
    Some(Dissolved { cooldowns })
}

/// `DestroyGuildReq` and the admin `Disband`: refuses at war, then ONE `GuildDisband` batch.
pub(crate) fn disband(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    origin: Origin,
    guild: GuildId,
    cb: Option<CommitCb>,
) -> Result<GuildChange, GuildRefusal> {
    let g = svc.social.guild.get(guild).ok_or(GuildRefusal::NotFound)?;
    if g.at_war() {
        return Err(GuildRefusal::AtWar);
    }
    let change = GuildChange {
        action: "disband",
        guild,
        character: g.master(),
        before: Some(json!({
            "name": g.name, "level": g.level, "members": g.members.len(),
            "gold": g.gold, "pxp": g.pxp,
        })),
        after: None,
    };
    let Some(d) = dissolve(svc, ctx, origin, guild, None) else {
        return Err(GuildRefusal::NotFound);
    };
    if origin.is_admin() {
        let ops = DbOp::guild_disband_ops(guild, d.cooldowns);
        persist(svc, ctx, origin, AtomicKind::GuildDisband, ops, cb);
    } else {
        let batch = AtomicBatch::guild_disband(guild, d.cooldowns);
        submit(svc, ctx, "GuildDisband", batch, &[], cb);
    }
    Ok(change)
}

/// The guild part of a character deletion (21 §3.6; the character is offline): a master's guild is disbanded
/// (`guild_disband_ops` are appended to the `CharacterDelete` ops), any other member leaves (`DeleteGuildMember` is
/// appended); the online guild-mates get the messages.
pub(crate) fn on_character_deleted(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    c: CharacterId,
    ops: &mut Vec<DbOp>,
) {
    let Some(guild) = svc.social.guild.guild_of(c) else {
        return;
    };
    let is_master = svc
        .social
        .guild
        .get(guild)
        .is_some_and(|g| g.master() == Some(c));
    if is_master {
        if let Some(d) = dissolve(svc, ctx, Origin::System, guild, Some(c)) {
            ops.extend(DbOp::guild_disband_ops(guild, d.cooldowns));
        }
        return;
    }
    let gname = svc
        .social
        .guild
        .get(guild)
        .map(|g| g.name.clone())
        .unwrap_or_default();
    let rank = svc
        .social
        .guild
        .get(guild)
        .and_then(|g| g.members.get(&c))
        .map_or(GuildRank::Rookie, |m| m.rank);
    let recipients = online(svc, guild);
    if svc.social.guild.remove_member(guild, c).is_none() {
        return;
    }
    ops.push(DbOp::DeleteGuildMember { character_id: c });
    wire::to_list(
        svc,
        ctx,
        &recipients,
        pb::RemoveGuildMemberNtf {
            id_player: c.0,
            id_guild: guild.0,
        },
    );
    emit(
        ctx,
        Origin::System,
        audit::guild_member_leave(guild, &gname, c, rank.as_u8()),
    );
}

// ---- membership --------------------------------------------------------------------------------------------------

/// Why a member leaves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Departure {
    /// The member left by itself.
    Leave,
    /// The master removed the member.
    Kick(CharacterId),
    /// The admin console removed the member.
    Admin,
}

/// A character joins `guild` with `rank` (class 0): the invitation accepted by a player (Rookie), or an admin /
/// GM addition. Validates the guild, that the character is free, the member limit and the rank cap.
pub(crate) fn add_member(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    origin: Origin,
    guild: GuildId,
    c: CharacterId,
    rank: GuildRank,
    cb: Option<CommitCb>,
) -> Result<GuildChange, GuildRefusal> {
    let g = svc.social.guild.get(guild).ok_or(GuildRefusal::NotFound)?;
    if svc.social.guild.guild_of(c).is_some() {
        return Err(GuildRefusal::InGuild);
    }
    if g.is_full(ctx.data) {
        return Err(GuildRefusal::Full);
    }
    if g.count_rank(rank) >= g.rank_cap(ctx.data, rank) {
        return Err(GuildRefusal::RankFull);
    }
    let entry = svc.presence.get(c).ok_or(GuildRefusal::NotFound)?;
    let member_name = entry.name.clone();
    let obj = entry.online.map(|o| o.obj);
    let gname = g.name.clone();
    // ---- the commit block ----
    let state = GuildMemberState::new(c, rank, now(ctx));
    let record = state.to_record(guild);
    svc.social.guild.add_member(guild, state);
    persist(
        svc,
        ctx,
        origin,
        AtomicKind::Other("GuildMemberAdd"),
        vec![DbOp::UpsertGuildMember(record)],
        cb,
    );
    svc.social.guild.sync_member(&svc.presence, ctx, c);
    let others: Vec<ObjId> = online(svc, guild)
        .into_iter()
        .filter(|o| Some(*o) != obj)
        .collect();
    if let Some(o) = obj
        && let Some(g) = svc.social.guild.get(guild)
    {
        wire::to_one(
            ctx,
            o,
            pb::Guild {
                guild: Some(wire::full(g)),
            },
        );
    }
    wire::to_list(
        svc,
        ctx,
        &others,
        pb::AddGuildMemberNtf {
            info: Some(pb::GuildMemberInfo {
                id_player: c.0,
                multi_no: if obj.is_some() { 1 } else { 100 },
            }),
            id_guild: guild.0,
            name: member_name,
        },
    );
    if let Some(o) = obj {
        wire::set_guild_near(svc, ctx, o, Some(guild));
        // ---- guild house (doc 22 §12.5, task guild-house): `GuildHouseAllInfo` and the furniture DSTs for the new member
        let on_member_joined = svc.social.guild.seams.on_member_joined;
        on_member_joined(svc, ctx, guild, o);
    }
    emit(
        ctx,
        origin,
        audit::guild_member_join(guild, &gname, c, rank.as_u8()),
    );
    Ok(GuildChange {
        action: "member",
        guild,
        character: Some(c),
        before: None,
        after: Some(Value::from(rank.audit_name())),
    })
}

/// A member leaves, is kicked or is removed by the console: `DeleteGuildMember` (+ the rejoin cooldown with
/// `features.remove_guild_cooldown = false`), `RemoveGuildMemberNtf` to the online members INCLUDING the removed one,
/// the removed member loses the mirrors, the bank window and the guild buffs and gets `SetGuild{0}` near it.
pub(crate) fn remove_member(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    origin: Origin,
    guild: GuildId,
    c: CharacterId,
    why: Departure,
    cb: Option<CommitCb>,
) -> Result<GuildChange, GuildRefusal> {
    let g = svc.social.guild.get(guild).ok_or(GuildRefusal::NotFound)?;
    let m = g.members.get(&c).ok_or(GuildRefusal::NotMember)?;
    if m.rank == GuildRank::Master {
        return Err(GuildRefusal::Invalid("the master cannot be removed"));
    }
    let rank = m.rank;
    let gname = g.name.clone();
    // the recipients are listed before the removal: the removed member is told as well
    let recipients = online(svc, guild);
    let obj = avatar_of(svc, c);
    // ---- the commit block ----
    svc.social.guild.remove_member(guild, c);
    let mut ops = vec![DbOp::DeleteGuildMember { character_id: c }];
    if !ctx.cfg.rules.features.remove_guild_cooldown {
        let at = now(ctx);
        let until = UnixTime(at.0.saturating_add(GUILD_REJOIN_COOLDOWN_S));
        svc.social.guild.set_rejoin(c, Some(until), at);
        ops.push(DbOp::SetGuildRejoinCooldown {
            character_id: c,
            until: Some(until),
        });
    }
    persist(
        svc,
        ctx,
        origin,
        AtomicKind::Other("GuildMemberRemove"),
        ops,
        cb,
    );
    wire::to_list(
        svc,
        ctx,
        &recipients,
        pb::RemoveGuildMemberNtf {
            id_player: c.0,
            id_guild: guild.0,
        },
    );
    if let Some(o) = obj {
        // ---- guild house (doc 22 §12.5, task guild-house): rest points, furniture DSTs and the way out of the house
        let on_member_left = svc.social.guild.seams.on_member_left;
        on_member_left(svc, ctx, guild, o, false);
        set_mirror(ctx, o, None, 0);
        buff::api::remove_by_ik3(svc, ctx, o, IK3_GUILD_BUFF);
        crate::item::remove_guild_cloaks(svc, ctx, PlayerRef(o));
        wire::set_guild_near(svc, ctx, o, None);
    }
    let event = match why {
        Departure::Leave => audit::guild_member_leave(guild, &gname, c, rank.as_u8()),
        Departure::Kick(by) => audit::guild_member_kick(guild, &gname, c, rank.as_u8(), by),
        Departure::Admin => {
            audit::guild_member_kick(guild, &gname, c, rank.as_u8(), CharacterId(0))
        }
    };
    emit(ctx, origin, event);
    Ok(GuildChange {
        action: "member",
        guild,
        character: Some(c),
        before: Some(Value::from(rank.audit_name())),
        after: None,
    })
}

/// A rank change (`GuildMemberLevelReq`): the new rank, class 0, `GuildMemberLevelNtf` to the members. The caller
/// checked the caps; a promotion to Master is [`change_master`].
pub(crate) fn set_rank(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    origin: Origin,
    guild: GuildId,
    c: CharacterId,
    rank: GuildRank,
    cb: Option<CommitCb>,
) -> Result<GuildChange, GuildRefusal> {
    let g = svc.social.guild.get(guild).ok_or(GuildRefusal::NotFound)?;
    let old = g.members.get(&c).ok_or(GuildRefusal::NotMember)?.rank;
    let gname = g.name.clone();
    // ---- the commit block ----
    if let Some(m) = svc
        .social
        .guild
        .get_mut(guild)
        .and_then(|g| g.members.get_mut(&c))
    {
        m.rank = rank;
        m.class = 0;
    }
    if let Some(op) = member_op(svc, guild, c) {
        persist(
            svc,
            ctx,
            origin,
            AtomicKind::Other("GuildMemberRank"),
            vec![op],
            cb,
        );
    }
    wire::to_members(
        svc,
        ctx,
        guild,
        pb::GuildMemberLevelNtf {
            id_player: c.0,
            rank: i32::from(rank.as_u8()),
        },
    );
    let mut event = audit::guild_rank(
        guild,
        &gname,
        c,
        old.as_u8(),
        rank.as_u8(),
        origin.character(ctx),
    );
    if let Value::Object(map) = &mut event.data {
        map.insert("class".to_owned(), Value::from(0));
    }
    emit(ctx, origin, event);
    Ok(GuildChange {
        action: "member",
        guild,
        character: Some(c),
        before: Some(Value::from(old.audit_name())),
        after: Some(Value::from(rank.audit_name())),
    })
}

/// A class change (`GuildClassReq`): the class `class` (0..=2), `GuildClassNtf` to the members.
pub(crate) fn set_class(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    origin: Origin,
    guild: GuildId,
    c: CharacterId,
    class: u8,
) -> Result<(), GuildRefusal> {
    let g = svc.social.guild.get(guild).ok_or(GuildRefusal::NotFound)?;
    let m = g.members.get(&c).ok_or(GuildRefusal::NotMember)?;
    let rank = m.rank;
    let gname = g.name.clone();
    if class > 2 {
        return Err(GuildRefusal::Invalid("class"));
    }
    if let Some(m) = svc
        .social
        .guild
        .get_mut(guild)
        .and_then(|g| g.members.get_mut(&c))
    {
        m.class = class;
    }
    if let Some(op) = member_op(svc, guild, c) {
        persist(
            svc,
            ctx,
            origin,
            AtomicKind::Other("GuildMemberClass"),
            vec![op],
            None,
        );
    }
    wire::to_members(
        svc,
        ctx,
        guild,
        pb::GuildClassNtf {
            id_player: c.0,
            class: i32::from(class),
        },
    );
    let mut event = audit::guild_rank(
        guild,
        &gname,
        c,
        rank.as_u8(),
        rank.as_u8(),
        origin.character(ctx),
    );
    if let Value::Object(map) = &mut event.data {
        map.insert("class".to_owned(), Value::from(class));
    }
    emit(ctx, origin, event);
    Ok(())
}

/// A nickname (`GuildNicknameReq`), already validated: `GuildNicknameNtf` to the members.
pub(crate) fn set_alias(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    origin: Origin,
    guild: GuildId,
    c: CharacterId,
    alias: &str,
) -> Result<(), GuildRefusal> {
    let g = svc.social.guild.get(guild).ok_or(GuildRefusal::NotFound)?;
    if !g.members.contains_key(&c) {
        return Err(GuildRefusal::NotMember);
    }
    if let Some(m) = svc
        .social
        .guild
        .get_mut(guild)
        .and_then(|g| g.members.get_mut(&c))
    {
        m.alias = alias.to_owned();
    }
    if let Some(op) = member_op(svc, guild, c) {
        persist(
            svc,
            ctx,
            origin,
            AtomicKind::Other("GuildMemberAlias"),
            vec![op],
            None,
        );
    }
    wire::to_members(
        svc,
        ctx,
        guild,
        pb::GuildNicknameNtf {
            id_player: c.0,
            alias: alias.to_owned(),
        },
    );
    Ok(())
}

/// The rank authorities (`GuildAuthorityReq`), stored raw: `GuildAuthorityNtf` to the members.
pub(crate) fn set_powers(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    origin: Origin,
    guild: GuildId,
    powers: [u32; 5],
) -> Result<(), GuildRefusal> {
    let g = svc
        .social
        .guild
        .get_mut(guild)
        .ok_or(GuildRefusal::NotFound)?;
    g.powers = powers;
    if let Some(op) = guild_op(svc, guild) {
        persist(
            svc,
            ctx,
            origin,
            AtomicKind::Other("GuildPowers"),
            vec![op],
            None,
        );
    }
    wire::to_members(
        svc,
        ctx,
        guild,
        pb::GuildAuthorityNtf {
            power: powers.to_vec(),
        },
    );
    Ok(())
}

/// The salary of a rank (`GuildPenyaReq`), already range-checked: `GuildPenyaNtf` to the members.
pub(crate) fn set_salary(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    origin: Origin,
    guild: GuildId,
    rank: GuildRank,
    penya: u32,
) -> Result<(), GuildRefusal> {
    let g = svc
        .social
        .guild
        .get_mut(guild)
        .ok_or(GuildRefusal::NotFound)?;
    if let Some(slot) = g.salaries.get_mut(usize::from(rank.as_u8())) {
        *slot = penya;
    }
    if let Some(op) = guild_op(svc, guild) {
        persist(
            svc,
            ctx,
            origin,
            AtomicKind::Other("GuildSalary"),
            vec![op],
            None,
        );
    }
    wire::to_members(
        svc,
        ctx,
        guild,
        pb::GuildPenyaNtf {
            rank: u32::from(rank.as_u8()),
            penya,
        },
    );
    Ok(())
}

/// The master hands the guild to `new` (`ChgMasterReq`, the admin `Master`): the old master becomes a Rookie
/// (class 0), the new one the Master (class 0); both rows go in ONE batch with the demotion first (the unique master
/// index); `ChgMasterNtf` to the members.
pub(crate) fn change_master(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    origin: Origin,
    guild: GuildId,
    new: CharacterId,
    cb: Option<CommitCb>,
) -> Result<GuildChange, GuildRefusal> {
    let g = svc.social.guild.get(guild).ok_or(GuildRefusal::NotFound)?;
    let new_state = g.members.get(&new).ok_or(GuildRefusal::NotMember)?;
    let old = g.master();
    if old == Some(new) {
        return Err(GuildRefusal::Invalid("already the master"));
    }
    let gname = g.name.clone();
    let new_row = new_state.to_record(guild);
    let old_row = old
        .and_then(|o| g.members.get(&o))
        .map(|m| m.to_record(guild));
    // ---- the commit block ----
    let ops: Vec<DbOp> = match old_row {
        Some(old_row) => DbOp::guild_master_change_ops(old_row, new_row).into(),
        None => {
            let mut promoted = new_row;
            promoted.rank = 0;
            promoted.class = 0;
            vec![DbOp::UpsertGuildMember(promoted)]
        }
    };
    if let Some(g) = svc.social.guild.get_mut(guild) {
        if let Some(m) = old.and_then(|o| g.members.get_mut(&o)) {
            m.rank = GuildRank::Rookie;
            m.class = 0;
        }
        if let Some(m) = g.members.get_mut(&new) {
            m.rank = GuildRank::Master;
            m.class = 0;
        }
    }
    persist(
        svc,
        ctx,
        origin,
        AtomicKind::Other("GuildMasterChange"),
        ops,
        cb,
    );
    let old_id = old.unwrap_or(CharacterId(0));
    wire::to_members(
        svc,
        ctx,
        guild,
        pb::ChgMasterNtf {
            old_master: old_id.0,
            new_master: new.0,
        },
    );
    emit(ctx, origin, audit::guild_master(guild, &gname, old_id, new));
    Ok(GuildChange {
        action: "master",
        guild,
        character: Some(new),
        before: Some(Value::from(old_id.0)),
        after: Some(Value::from(new.0)),
    })
}

// ---- name, logo, notice ------------------------------------------------------------------------------------------

/// The rename scroll that a [`rename`] consumes in the same commit block.
pub(crate) struct ScrollUse {
    /// The holder (the master).
    pub holder: PlayerRef,
    /// The inventory slot of the scroll.
    pub slot: StorageId,
}

/// The first naming, the rename scroll, a GM / admin rename: the memory and the `by_name` index take the name, ONE
/// persist writes it (the scroll variant is the `GuildRename` batch with the master's snapshot after the scroll was
/// consumed, so a clash rolls the scroll back), `GuildSetNameNtf` goes to everybody. The caller validated the name and
/// checked that no other guild has it.
pub(crate) fn rename(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    origin: Origin,
    guild: GuildId,
    new_name: &str,
    scroll: Option<ScrollUse>,
    cb: Option<CommitCb>,
) -> Result<GuildChange, GuildRefusal> {
    let g = svc.social.guild.get(guild).ok_or(GuildRefusal::NotFound)?;
    if svc
        .social
        .guild
        .by_name(new_name)
        .is_some_and(|o| o != guild)
    {
        return Err(GuildRefusal::NameTaken);
    }
    if let Some(s) = &scroll
        && !can_snapshot(ctx, s.holder)
    {
        return Err(GuildRefusal::Unavailable);
    }
    let old_name = g.name.clone();
    let master = g.master().unwrap_or(CharacterId(0));
    // ---- the commit block ----
    svc.social.guild.rename(guild, new_name);
    match scroll {
        Some(s) => {
            remove_item(svc, ctx, s.holder, s.slot, 1, RemoveReason::Consume);
            if let (Some((snap, generation)), Some(g)) = (
                snapshot_for_batch(svc, ctx, s.holder),
                svc.social.guild.get(guild),
            ) {
                let batch = AtomicBatch::guild_rename(snap, g.to_record());
                submit(
                    svc,
                    ctx,
                    "GuildRename",
                    batch,
                    &[(s.holder, generation)],
                    cb,
                );
            }
        }
        None => {
            if let Some(op) = guild_op(svc, guild) {
                persist(svc, ctx, origin, AtomicKind::GuildRename, vec![op], cb);
            }
        }
    }
    wire::to_all(
        svc,
        ctx,
        pb::GuildSetNameNtf {
            id_guild: guild.0,
            name: new_name.to_owned(),
        },
    );
    emit(
        ctx,
        origin,
        audit::guild_rename(guild, &old_name, new_name, master),
    );
    Ok(GuildChange {
        action: "rename",
        guild,
        character: None,
        before: Some(Value::from(old_name)),
        after: Some(Value::from(new_name)),
    })
}

/// The logo (`GuildLogoReq`, the admin `Logo`, `/gstat logo`): `GuildLogoNtf` to everybody.
pub(crate) fn set_logo(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    origin: Origin,
    guild: GuildId,
    logo: u32,
    cb: Option<CommitCb>,
) -> Result<GuildChange, GuildRefusal> {
    let g = svc
        .social
        .guild
        .get_mut(guild)
        .ok_or(GuildRefusal::NotFound)?;
    let old = g.logo;
    g.logo = logo;
    if let Some(op) = guild_op(svc, guild) {
        persist(
            svc,
            ctx,
            origin,
            AtomicKind::Other("GuildLogo"),
            vec![op],
            cb,
        );
    }
    wire::to_all(
        svc,
        ctx,
        pb::GuildLogoNtf {
            id_guild: guild.0,
            logo,
        },
    );
    Ok(GuildChange {
        action: "logo",
        guild,
        character: None,
        before: Some(Value::from(old)),
        after: Some(Value::from(logo)),
    })
}

/// The notice (`GuildNoticeReq`, the admin `Notice`, `/gstat notice`): `GuildNoticeNtf` to the members.
pub(crate) fn set_notice(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    origin: Origin,
    guild: GuildId,
    notice: &str,
    cb: Option<CommitCb>,
) -> Result<GuildChange, GuildRefusal> {
    let g = svc
        .social
        .guild
        .get_mut(guild)
        .ok_or(GuildRefusal::NotFound)?;
    let old = std::mem::replace(&mut g.notice, notice.to_owned());
    if let Some(op) = guild_op(svc, guild) {
        persist(
            svc,
            ctx,
            origin,
            AtomicKind::Other("GuildNotice"),
            vec![op],
            cb,
        );
    }
    wire::to_members(
        svc,
        ctx,
        guild,
        pb::GuildNoticeNtf {
            id_guild: guild.0,
            notice: notice.to_owned(),
        },
    );
    Ok(GuildChange {
        action: "notice",
        guild,
        character: None,
        before: Some(Value::from(old)),
        after: Some(Value::from(notice)),
    })
}

// ---- the pools (admin level / PXP / penya edits) -----------------------------------------------------------------

/// `GuildContributionNtf` to the members of a named guild (KQ-39 c: an unnamed guild changes state but sends
/// nothing). `forced` sends it anyway (an admin edit).
pub(crate) fn announce_pools(
    svc: &Services,
    ctx: &mut GameCtx<'_>,
    guild: GuildId,
    member: CharacterId,
    pxp_delta: u32,
    penya_delta: u32,
    forced: bool,
) {
    let Some(g) = svc.social.guild.get(guild) else {
        return;
    };
    if !g.is_named() && !forced {
        return;
    }
    wire::to_members(
        svc,
        ctx,
        guild,
        pb::GuildContributionNtf {
            id_guild: guild.0,
            id_player: member.0,
            pxp_delta,
            penya_delta,
            guild_pxp: g.pxp,
            guild_penya: g.gold,
            guild_level: u32::try_from(g.level).unwrap_or(1),
        },
    );
}

/// What [`set_pools`] changes (a field left `None` stays).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PoolEdit {
    /// The new level.
    pub level: Option<i32>,
    /// The new contribution points.
    pub pxp: Option<u32>,
    /// The change of the penya pool.
    pub gold_delta: Option<i64>,
}

/// The level, the PXP or the penya pool set by the console (`Level`, `Pxp`, `Gold`): the new values, ONE persist
/// (the penya variant carries the `R` log row), the members learn the pools with a contribution notice that names
/// nobody. `gold_delta` is the change of the pool (negative takes penya away, clamped at 0 and `u32::MAX`).
pub(crate) fn set_pools(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    origin: Origin,
    guild: GuildId,
    edit: PoolEdit,
    cb: Option<CommitCb>,
) -> Result<GuildChange, GuildRefusal> {
    let PoolEdit {
        level,
        pxp,
        gold_delta,
    } = edit;
    let g = svc
        .social
        .guild
        .get_mut(guild)
        .ok_or(GuildRefusal::NotFound)?;
    let before = json!({"level": g.level, "pxp": g.pxp, "gold": g.gold});
    let old_gold = g.gold;
    if let Some(l) = level {
        g.level = l.clamp(1, flyff_rules::social::MAX_GUILD_LEVEL);
    }
    if let Some(p) = pxp {
        g.pxp = p;
    }
    if let Some(d) = gold_delta {
        let new = i64::from(g.gold)
            .saturating_add(d)
            .clamp(0, i64::from(u32::MAX));
        g.gold = u32::try_from(new).unwrap_or(u32::MAX);
    }
    let after = json!({"level": g.level, "pxp": g.pxp, "gold": g.gold});
    let moved = i64::from(g.gold) - i64::from(old_gold);
    let ops: Vec<DbOp> = match (
        gold_delta.is_some() && moved != 0,
        svc.social.guild.get(guild),
    ) {
        (true, Some(g)) => {
            let log = GuildBankLogRecord::new(
                guild,
                origin.character(ctx),
                GuildBankLogKind::GmRemoval,
                now(ctx),
            )
            .with_penya(moved);
            DbOp::guild_gold_removal_ops(g.to_record(), log).into()
        }
        _ => guild_op(svc, guild).into_iter().collect(),
    };
    persist(svc, ctx, origin, AtomicKind::Other("GuildPools"), ops, cb);
    announce_pools(svc, ctx, guild, CharacterId(0), 0, 0, true);
    Ok(GuildChange {
        action: "level",
        guild,
        character: None,
        before: Some(before),
        after: Some(after),
    })
}
