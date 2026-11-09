//! Guild creation, first naming, the rename scroll and dissolution (Plan/21 §5.4; WP-SOCIAL-10).
//!
//! **Creation** is validate-then-commit (VR-047, PL-091, VR-018): [`check_create`] is read-only and is what
//! [`create_paid`] runs first, so a refusal changes nothing and the leader keeps his penya; the commit then debits
//! `charge`, takes the leader's snapshot AFTER the debit and writes ONE `GuildCreate` batch (snapshot, guild, members,
//! empty bank, counter), so the penya and the guild stand or fall together.
//!
//! The Helena dialog (doc 20, PL-149) is the caller of [`super::create_for_npc`]: the script's own
//! `RemoveGold(3 000 000)` already ran in the same handler, so it asks for `charge = 0` and the snapshot is the
//! post-debit one; a refusal after the debit loses the penya (KEEP, 20 §10.3 step 4).

use super::commit::{self, Origin, ScrollUse};
use super::model::{Guild, GuildMemberState, GuildRank};
use super::ops::GuildRefusal;
use super::wire;
use crate::chat::ingress::notice;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::econ::mail::commit::{can_snapshot, snapshot_for_batch};
use crate::item::container::StorageId;
use crate::social::party::Party;
use flyff_common::defs::item::II_SYS_SYS_SCR_GCHANAM;
use flyff_common::defs::text::{
    TID_DIAG_0020, TID_GAME_COMDELNOTKINGPIN, TID_GAME_COMNOHAVECOM, TID_GAME_COMOVERLAPNAME,
    TID_GAME_GUILDWARNODISMISS,
};
use flyff_common::{CharacterId, GuildId};
use flyff_db::AtomicBatch;
use flyff_proto::pb;
use flyff_rules::names::{NameError, NameKind, NameLimits, name_error_tid, validate_name};
use flyff_rules::social::GUILD_CREATE_MIN_PARTY;

/// Who becomes a member of a new guild.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CreateMembers {
    /// The leader's party, every member (the leader is the Master, the others Rookies): Helena.
    Party,
    /// The leader alone: the GM command `/createguild` (23 row 67).
    SelfOnly,
}

/// A request to create a guild (21 §5.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuildCreateReq {
    /// The leader and future master (an online avatar).
    pub leader: PlayerRef,
    /// Who joins.
    pub members: CreateMembers,
    /// The name; `None` creates an unnamed guild that the master names later (Helena).
    pub name: Option<String>,
    /// The penya taken from the leader in the same commit (0 when the caller already debited him).
    pub charge: u32,
}

/// Why a guild is not created. Nothing has changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum GuildCreateError {
    /// The leader does not lead a party.
    #[error("the player does not lead a party")]
    NotPartyLeader,
    /// The party has fewer than 3 members.
    #[error("the party is too small")]
    PartyTooSmall,
    /// A party member is not online.
    #[error("a party member is offline")]
    MemberOffline,
    /// A member already belongs to a guild.
    #[error("a member is in a guild")]
    MemberInGuild,
    /// A member is under a rejoin cooldown (only with `features.remove_guild_cooldown = false`).
    #[error("a member is under a rejoin cooldown")]
    RejoinCooldown,
    /// The leader has less than `charge` penya.
    #[error("not enough penya")]
    NotEnoughGold,
    /// The name breaks the name rules.
    #[error("invalid guild name")]
    InvalidName,
    /// Another guild has the name (ASCII case-folded).
    #[error("guild name taken")]
    NameTaken,
    /// The leader's session is tainted or gone: no snapshot can be written.
    #[error("the session cannot be saved")]
    Unavailable,
}

/// The name rules of a guild (`NameKind::Guild`, 3..=16 characters, UD-21) over the loaded rule data.
pub fn validate_guild_name(ctx: &GameCtx<'_>, raw: &str) -> Result<String, NameError> {
    validate_with(NameKind::Guild, ctx, raw)
}

/// The name rules of a member nickname (`NameKind::GuildNickname`, 2..=12).
pub fn validate_nickname(ctx: &GameCtx<'_>, raw: &str) -> Result<String, NameError> {
    validate_with(NameKind::GuildNickname, ctx, raw)
}

fn validate_with(kind: NameKind, ctx: &GameCtx<'_>, raw: &str) -> Result<String, NameError> {
    let g = &ctx.cfg.rules.gameplay;
    validate_name(
        kind,
        raw,
        &ctx.data.name_rules,
        NameLimits {
            min_name_letters: g.min_name_letters,
            max_name_letters: g.max_name_letters,
        },
        None,
    )
}

// ---- the party rules ---------------------------------------------------------------------------------------------

/// The first problem among the members of `party`, in party order (`IsPartyGuild`): a member that is not in the
/// world, one that belongs to a guild, one under a rejoin cooldown.
fn member_problem(svc: &Services, ctx: &GameCtx<'_>, party: &Party) -> Option<GuildCreateError> {
    let at = commit::now(ctx);
    let cooldowns = !ctx.cfg.rules.features.remove_guild_cooldown;
    for m in &party.members {
        let in_world = commit::avatar_of(svc, m.id).is_some_and(|o| ctx.worlds.player(o).is_some());
        if !in_world {
            return Some(GuildCreateError::MemberOffline);
        }
        if svc.social.guild.guild_of(m.id).is_some() {
            return Some(GuildCreateError::MemberInGuild);
        }
        if cooldowns && svc.social.guild.in_cooldown(m.id, at) {
            return Some(GuildCreateError::RejoinCooldown);
        }
    }
    None
}

/// The problem of `c`'s party for a guild creation, ignoring who leads it and its size (the dialog helper
/// `IsPartyGuild`): no party counts as `NotPartyLeader`.
pub(crate) fn party_members_error(
    svc: &Services,
    ctx: &GameCtx<'_>,
    c: CharacterId,
) -> Option<GuildCreateError> {
    match svc.social.party.party_record_of(c) {
        Some(p) => member_problem(svc, ctx, p),
        None => Some(GuildCreateError::NotPartyLeader),
    }
}

/// Everything [`create_paid`] needs to know, read-only: the members in roster order and the validated name.
struct Plan {
    leader: CharacterId,
    members: Vec<CharacterId>,
    name: String,
}

fn plan(svc: &Services, ctx: &GameCtx<'_>, req: &GuildCreateReq) -> Result<Plan, GuildCreateError> {
    let leader = super::character_of(ctx, req.leader).ok_or(GuildCreateError::Unavailable)?;
    if !can_snapshot(ctx, req.leader) {
        return Err(GuildCreateError::Unavailable);
    }
    let members: Vec<CharacterId> = match req.members {
        CreateMembers::Party => {
            let party = svc
                .social
                .party
                .party_record_of(leader)
                .ok_or(GuildCreateError::NotPartyLeader)?;
            if party.leader_id() != Some(leader) {
                return Err(GuildCreateError::NotPartyLeader);
            }
            if party.size() < GUILD_CREATE_MIN_PARTY {
                return Err(GuildCreateError::PartyTooSmall);
            }
            if let Some(e) = member_problem(svc, ctx, party) {
                return Err(e);
            }
            party.members.iter().map(|m| m.id).collect()
        }
        CreateMembers::SelfOnly => {
            if svc.social.guild.guild_of(leader).is_some() {
                return Err(GuildCreateError::MemberInGuild);
            }
            vec![leader]
        }
    };
    let name = match &req.name {
        Some(raw) => {
            let name = validate_guild_name(ctx, raw).map_err(|_| GuildCreateError::InvalidName)?;
            if svc.social.guild.name_taken(&name) {
                return Err(GuildCreateError::NameTaken);
            }
            name
        }
        None => String::new(),
    };
    let gold = ctx
        .player(req.leader)
        .and_then(|m| m.player())
        .map_or(0, |p| i64::from(p.items.gold));
    if gold < i64::from(req.charge) {
        return Err(GuildCreateError::NotEnoughGold);
    }
    Ok(Plan {
        leader,
        members,
        name,
    })
}

/// The read-only validation of a creation (21 §5.4 step 1): never mutates anything. The dialog helper of doc 20 and
/// [`create_paid`] call it.
pub fn check_create(
    svc: &Services,
    ctx: &GameCtx<'_>,
    req: &GuildCreateReq,
) -> Result<(), GuildCreateError> {
    plan(svc, ctx, req).map(|_| ())
}

/// Creates the guild (validate-then-commit): [`check_create`] first, then ONE commit block. Returns the new id.
pub fn create_paid(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    req: GuildCreateReq,
) -> Result<GuildId, GuildCreateError> {
    let p = plan(svc, ctx, &req)?;
    // ---- the commit block (nothing below can fail) ----
    let at = commit::now(ctx);
    if req.charge > 0 {
        crate::item::gold::add_gold(svc, ctx, req.leader, -i64::from(req.charge), true);
    }
    let id = svc.social.guild.alloc_id();
    let mut guild = Guild::new(id, p.name, at);
    for c in &p.members {
        let rank = if *c == p.leader {
            GuildRank::Master
        } else {
            GuildRank::Rookie
        };
        guild
            .members
            .insert(*c, GuildMemberState::new(*c, rank, at));
    }
    let record = guild.to_record();
    let rows: Vec<_> = guild.members.values().map(|m| m.to_record(id)).collect();
    // the master row first: the schema's unique master index does not care, a reader of the batch does
    let mut rows = rows;
    rows.sort_by_key(|r| (r.rank != 0, r.character_id));
    svc.social.guild.insert(guild);
    if let Some((snap, generation)) = snapshot_for_batch(svc, ctx, req.leader) {
        let batch = AtomicBatch::guild_create(snap, record, rows);
        commit::submit(
            svc,
            ctx,
            "GuildCreate",
            batch,
            &[(req.leader, generation)],
            None,
        );
    }
    for c in &p.members {
        svc.social.guild.sync_member(&svc.presence, ctx, *c);
    }
    commit::announce_created(svc, ctx, id);
    commit::audit_created(ctx, Origin::Player(req.leader), svc, id);
    Ok(id)
}

// ---- the requests ------------------------------------------------------------------------------------------------

struct Me {
    obj: flyff_common::ObjId,
    character: CharacterId,
}

fn me(ctx: &GameCtx<'_>, who: PlayerRef) -> Option<Me> {
    let p = ctx.player(who)?.player()?;
    Some(Me {
        obj: who.0,
        character: p.ident.character_id,
    })
}

/// `GuildSetNameReq{name}` (`OnGuildSetName`, 21 §5.4 first naming): the master of an unnamed guild names it. A
/// non-master (or a player without a guild) hears `TID_GAME_COMDELNOTKINGPIN`, a guild that has a name ignores the
/// request, a name that breaks the rules answers with its text, a taken name `GuildError{1}`.
pub fn on_guild_set_name(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::GuildSetNameReq,
) {
    let Some(a) = me(ctx, who) else { return };
    let Some(gid) = svc.social.guild.guild_of(a.character) else {
        notice::defined(ctx, who, TID_GAME_COMDELNOTKINGPIN, Vec::new());
        return;
    };
    let Some(g) = svc.social.guild.get(gid) else {
        return;
    };
    if g.master() != Some(a.character) {
        notice::defined(ctx, who, TID_GAME_COMDELNOTKINGPIN, Vec::new());
        return;
    }
    if g.is_named() {
        return;
    }
    let name = match validate_guild_name(ctx, &m.name) {
        Ok(n) => n,
        Err(e) => {
            let tid = u32::try_from(name_error_tid(NameKind::Guild, e)).unwrap_or(TID_DIAG_0020);
            notice::defined(ctx, who, tid, Vec::new());
            return;
        }
    };
    if svc.social.guild.name_taken(&name) {
        wire::to_one(ctx, a.obj, pb::GuildError { code: 1 });
        return;
    }
    let _ = commit::rename(svc, ctx, Origin::Player(who), gid, &name, None, None);
}

/// `QuerySetGuildName{item_obj_id, name}` (`OnQuerySetGuildName`, the rename scroll, 21 §5.4): an invalid name
/// answers `TID_DIAG_0020` and STOPS (VR-046); the item in that slot must be a usable guild rename scroll
/// (`II_SYS_SYS_SCR_GCHANAM`; the original consumed ANY usable item) and no rename may be in flight; then in order no
/// guild `TID_GAME_COMNOHAVECOM`, not the master `TID_GAME_COMDELNOTKINGPIN`, a taken name
/// `TID_GAME_COMOVERLAPNAME`. The success consumes that scroll in the commit block: ONE `GuildRename` batch carries
/// the master's snapshot (this differs from the character scroll of `player_data.rs`, which consumes after the
/// commit).
pub fn on_query_set_guild_name(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::QuerySetGuildName,
) {
    let Some(a) = me(ctx, who) else { return };
    let name = match validate_guild_name(ctx, &m.name) {
        Ok(n) => n,
        Err(_) => {
            notice::defined(ctx, who, TID_DIAG_0020, Vec::new());
            return;
        }
    };
    let Ok(slot) = StorageId::try_from(m.item_obj_id) else {
        return;
    };
    let scroll_ok = ctx.player(who).and_then(|mv| mv.player()).is_some_and(|p| {
        !p.social.rename_pending
            && p.items
                .inventory
                .get_at_id(slot)
                .is_some_and(|it| it.is_usable() && it.item_id == II_SYS_SYS_SCR_GCHANAM)
    });
    if !scroll_ok {
        return;
    }
    let Some(gid) = svc.social.guild.guild_of(a.character) else {
        notice::defined(ctx, who, TID_GAME_COMNOHAVECOM, Vec::new());
        return;
    };
    if svc.social.guild.get(gid).and_then(Guild::master) != Some(a.character) {
        notice::defined(ctx, who, TID_GAME_COMDELNOTKINGPIN, Vec::new());
        return;
    }
    let own = svc.social.guild.get(gid).map(|g| g.name.clone());
    let taken = svc.social.guild.by_name(&name).is_some_and(|o| o != gid)
        || own.as_deref() == Some(name.as_str());
    if taken {
        notice::defined(ctx, who, TID_GAME_COMOVERLAPNAME, Vec::new());
        return;
    }
    match commit::rename(
        svc,
        ctx,
        Origin::Player(who),
        gid,
        &name,
        Some(ScrollUse { holder: who, slot }),
        None,
    ) {
        Ok(_) => {}
        Err(GuildRefusal::Unavailable) => {
            tracing::debug!("a guild rename was refused: the session cannot be saved");
        }
        Err(e) => tracing::debug!(?e, "a guild rename was refused"),
    }
}

/// `DestroyGuildReq` (`OnDestroyGuild`, 21 §5.4): no guild `TID_GAME_COMNOHAVECOM`, not the master
/// `TID_GAME_COMDELNOTKINGPIN`, a running war `TID_GAME_GUILDWARNODISMISS`, a siege bidder the seam's text; then ONE
/// `GuildDisband` batch (the bank contents are lost, KEEP).
pub fn on_destroy_guild(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    _m: pb::DestroyGuildReq,
) {
    let Some(a) = me(ctx, who) else { return };
    let Some(gid) = svc.social.guild.guild_of(a.character) else {
        notice::defined(ctx, who, TID_GAME_COMNOHAVECOM, Vec::new());
        return;
    };
    let Some(g) = svc.social.guild.get(gid) else {
        return;
    };
    if g.master() != Some(a.character) {
        notice::defined(ctx, who, TID_GAME_COMDELNOTKINGPIN, Vec::new());
        return;
    }
    if g.at_war() {
        notice::defined(ctx, who, TID_GAME_GUILDWARNODISMISS, Vec::new());
        return;
    }
    let locked = svc.social.guild.seams.siege_dissolve_locked;
    if locked(svc, gid) {
        notice::defined(
            ctx,
            who,
            flyff_common::defs::text::TID_GAME_GUILDCOMBAT_NOT_DISSOLVE_GUILD,
            Vec::new(),
        );
        return;
    }
    // the siege's text: no GC 1:1 text exists (WP-SOCIAL-25)
    let locked = svc.social.guild.seams.gc1to1_dissolve_locked;
    if locked(svc, gid) {
        notice::defined(
            ctx,
            who,
            flyff_common::defs::text::TID_GAME_GUILDCOMBAT_NOT_DISSOLVE_GUILD,
            Vec::new(),
        );
        return;
    }
    let _ = commit::disband(svc, ctx, Origin::Player(who), gid, None);
}
