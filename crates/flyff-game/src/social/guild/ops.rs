//! The trusted-caller API of the guild service (Plan/21 §5, 25 §6.2-§6.3; WP-SOCIAL-10..12): what the GM commands
//! (`/createguild`, `/sgn`, `/gstat`), the Helena dialog and the admin console call. A caller here has already been
//! authorised (the GM table, the NPC script, the console's capabilities): the NPC, quest, cost, party and online rules
//! of the player flow do not apply, the structural rules (rank caps, `max_members`, the war lock on roster edits,
//! name rules and uniqueness) do.
//!
//! * [`apply`] runs one [`GuildCmd`] and is **idempotent**: a repeat that finds the state already reached (the same
//!   name, a member already in with that rank, a member already out) is [`Applied::Unchanged`] with no write and no
//!   message; a conflicting repeat (the name is taken now, the member belongs elsewhere now) refuses.
//! * [`create_admin`] is the console's `GuildCreate` (ONE `AdminGuildCreate` batch).
//! * [`disband`] dissolves a guild like the master's request.
//!
//! Every function is ONE atomic persist: the admin console always writes `AdminGuildCreate` / `AdminGuildEdit`, a GM
//! or player change writes the player kinds. An OFFLINE target is accepted only for [`GuildActor::Admin`] whose
//! [`AdminGuildCtx::held`] holds the character's account (the route took `OnlineRegistry::admin_hold` first, as for
//! `OfflineCharacterEdit`; the game thread cannot take it): otherwise [`GuildRefusal::NeedsHold`] (or
//! [`GuildRefusal::Offline`] for a non-admin). The changes come back as [`GuildChange`]s whose
//! [`GuildChange::audit_data`] is the `data` of the `admin.guild` audit event (25 §14); the HTTP layer writes the
//! event with actor, ip and reason.

use super::commit::{self, Departure, Origin, PoolEdit};
use super::create::validate_guild_name;
use super::model::{Guild, GuildBank, GuildMemberState, GuildRank};
use crate::ctx::{GameCtx, Services};
use crate::econ::mail::commit::snapshot_for_batch;
use crate::input::AdminActor;
use crate::tickets::TicketResult;
use flyff_common::{AccountId, CharacterId, GuildId};
use flyff_db::{AtomicBatch, CharacterState, DbOp, GuildBankDoc};
use flyff_rules::names::NameError;
use flyff_rules::social::{GUILD_LOGO_MAX, GUILD_NOTICE_MAX, MAX_GUILD_LEVEL};
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub use crate::ctx::PlayerRef;

/// The continuation of a persisted change: it gets the ticket result of the batch (the admin console answers its
/// HTTP request from it).
pub type CommitCb = Box<dyn FnOnce(&mut Services, &mut GameCtx<'_>, TicketResult) + Send>;

/// What the admin route took before calling: the console actor and the accounts it holds under
/// `OnlineRegistry::admin_hold` (an offline character may be changed only while its account is held).
#[derive(Clone, Debug)]
pub struct AdminGuildCtx {
    /// The console actor (for the audit lines the HTTP layer writes).
    pub actor: AdminActor,
    /// The held accounts.
    pub held: BTreeSet<AccountId>,
}

/// Who asks.
#[derive(Clone, Debug)]
pub enum GuildActor {
    /// A player acting through his own request (the handlers use the commit primitives directly; this variant is
    /// for callers that act for a player).
    Player(PlayerRef),
    /// A game master's command or an NPC script acting for the avatar.
    Gm(PlayerRef),
    /// The admin console.
    Admin(AdminGuildCtx),
}

/// One change of a guild (the admin `GuildEdit` without `BankRemove`, which belongs to the guild bank).
#[derive(Clone, Debug, PartialEq)]
pub enum GuildCmd {
    /// Rename (the rules of the rename scroll without the scroll).
    Rename(String),
    /// The logo `0..=27`, replaceable at any guild level.
    Logo(u32),
    /// Hand the guild to a member; the old master becomes a Rookie.
    Master(CharacterId),
    /// Set the level (clamped to 1..=50).
    Level(u32),
    /// Set the contribution points.
    Pxp(u32),
    /// Add (positive) or take away (negative) penya of the guild pool.
    Gold(i64),
    /// Set the notice (at most 127 bytes).
    Notice(String),
    /// Add a member with a rank.
    AddMember(CharacterId, GuildRank),
    /// Remove a member.
    RemoveMember(CharacterId),
    /// Change a member's rank.
    Rank(CharacterId, GuildRank),
}

/// What a trusted change did, for the `admin.guild` audit event (25 §14): `action` is one of `create`, `rename`,
/// `logo`, `master`, `level` (the level / PXP / penya pools), `member`, `bank`, `disband` or `notice`.
#[derive(Clone, Debug, PartialEq)]
pub struct GuildChange {
    /// The action name.
    pub action: &'static str,
    /// The guild.
    pub guild: GuildId,
    /// The character concerned (a member change, the new master), if any.
    pub character: Option<CharacterId>,
    /// The state before (`null` for an addition).
    pub before: Option<Value>,
    /// The state after (`null` for a removal).
    pub after: Option<Value>,
}

impl GuildChange {
    /// The `data` of the `admin.guild` audit event: `{action, guild, character?, before, after}`. A member addition
    /// has `before: null` and `after: <rank name>`, a removal `before: <rank name>` and `after: null` (the console's
    /// Undo reads exactly this).
    pub fn audit_data(&self) -> Value {
        let mut m = serde_json::Map::new();
        m.insert("action".to_owned(), Value::from(self.action));
        m.insert("guild".to_owned(), Value::from(self.guild.0));
        if let Some(c) = self.character {
            m.insert("character".to_owned(), Value::from(c.0));
        }
        m.insert(
            "before".to_owned(),
            self.before.clone().unwrap_or(Value::Null),
        );
        m.insert(
            "after".to_owned(),
            self.after.clone().unwrap_or(Value::Null),
        );
        Value::Object(m)
    }
}

/// The result of [`apply`].
#[derive(Clone, Debug, PartialEq)]
pub enum Applied {
    /// The guild changed.
    Changed(Vec<GuildChange>),
    /// The target state was already reached: nothing was written or sent.
    Unchanged,
}

/// Why a trusted change is refused. Nothing has changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum GuildRefusal {
    /// The guild (or the character) does not exist.
    #[error("not found")]
    NotFound,
    /// The character is not a member of the guild.
    #[error("not a member")]
    NotMember,
    /// The character belongs to a guild already.
    #[error("already in a guild")]
    InGuild,
    /// The character is under a rejoin cooldown.
    #[error("under a rejoin cooldown")]
    Cooldown,
    /// The guild has reached its member limit.
    #[error("the guild is full")]
    Full,
    /// The rank has reached its cap (1 / 5 / 10 / 20).
    #[error("the rank is full")]
    RankFull,
    /// A war runs: roster edits and the dissolution are refused.
    #[error("the guild is at war")]
    AtWar,
    /// The name breaks the name rules.
    #[error("invalid guild name: {0}")]
    InvalidName(NameError),
    /// Another guild has the name.
    #[error("guild name taken")]
    NameTaken,
    /// The request is malformed.
    #[error("invalid: {0}")]
    Invalid(&'static str),
    /// The character is offline and the caller may not change offline characters.
    #[error("the character is offline")]
    Offline,
    /// The character is offline and its account is not held: the route takes `admin_hold` and calls again.
    #[error("the account of character {0:?} must be held first")]
    NeedsHold(CharacterId),
    /// The session of an online character cannot be saved (tainted).
    #[error("the session cannot be saved")]
    Unavailable,
}

/// The guild the console creates (25 §6.2).
#[derive(Clone, Debug, PartialEq)]
pub struct AdminGuildSpec {
    /// The name (the name rules and uniqueness apply).
    pub name: String,
    /// The master (online or, with a held account, offline).
    pub master: CharacterId,
    /// The other members with their ranks (no Master).
    pub members: Vec<(CharacterId, GuildRank)>,
    /// Level `1..=50`.
    pub level: u32,
    /// The guild penya (at most `u32::MAX`).
    pub gold: u64,
    /// The contribution points.
    pub pxp: u32,
    /// The logo `0..=27`.
    pub logo: u32,
    /// The notice (at most 127 bytes).
    pub notice: String,
    /// The bank document (the caller builds the optional guild cloak with the bank's cloak builder).
    pub bank: Option<GuildBankDoc>,
    /// Ignore the rejoin cooldown of the master and the members.
    pub override_cooldown: bool,
}

// ---- helpers -----------------------------------------------------------------------------------------------------

/// May `actor` change the character `c`? An online avatar always; an offline one only for an admin that holds its
/// account.
fn target_access(
    svc: &Services,
    ctx: &GameCtx<'_>,
    c: CharacterId,
    held: Option<&AdminGuildCtx>,
) -> Result<(), GuildRefusal> {
    let e = svc
        .presence
        .get(c)
        .filter(|e| e.state != CharacterState::Deleted)
        .ok_or(GuildRefusal::NotFound)?;
    if e.online.is_some_and(|o| ctx.worlds.player(o.obj).is_some()) {
        return Ok(());
    }
    match held {
        Some(a) if a.held.contains(&e.account) => Ok(()),
        Some(_) => Err(GuildRefusal::NeedsHold(c)),
        None => Err(GuildRefusal::Offline),
    }
}

fn admin_of(actor: &GuildActor) -> Option<&AdminGuildCtx> {
    match actor {
        GuildActor::Admin(a) => Some(a),
        _ => None,
    }
}

fn changed(c: GuildChange) -> Result<Applied, GuildRefusal> {
    Ok(Applied::Changed(vec![c]))
}

// ---- apply -------------------------------------------------------------------------------------------------------

/// Runs one trusted change of `guild` (see the module documentation for the rules). `cb` gets the result of the
/// persisted batch.
pub fn apply(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    guild: GuildId,
    cmd: GuildCmd,
    actor: &GuildActor,
    cb: Option<CommitCb>,
) -> Result<Applied, GuildRefusal> {
    let origin = Origin::from(actor);
    let g = svc.social.guild.get(guild).ok_or(GuildRefusal::NotFound)?;
    match cmd {
        GuildCmd::Rename(raw) => {
            let name = validate_guild_name(ctx, &raw).map_err(GuildRefusal::InvalidName)?;
            if g.name == name {
                return Ok(Applied::Unchanged);
            }
            if svc.social.guild.by_name(&name).is_some_and(|o| o != guild) {
                return Err(GuildRefusal::NameTaken);
            }
            changed(commit::rename(svc, ctx, origin, guild, &name, None, cb)?)
        }
        GuildCmd::Logo(logo) => {
            if logo > GUILD_LOGO_MAX {
                return Err(GuildRefusal::Invalid("logo"));
            }
            if g.logo == logo {
                return Ok(Applied::Unchanged);
            }
            changed(commit::set_logo(svc, ctx, origin, guild, logo, cb)?)
        }
        GuildCmd::Notice(text) => {
            if text.len() > GUILD_NOTICE_MAX {
                return Err(GuildRefusal::Invalid("notice"));
            }
            if g.notice == text {
                return Ok(Applied::Unchanged);
            }
            changed(commit::set_notice(svc, ctx, origin, guild, &text, cb)?)
        }
        GuildCmd::Level(level) => {
            let level = i32::try_from(level)
                .unwrap_or(MAX_GUILD_LEVEL)
                .clamp(1, MAX_GUILD_LEVEL);
            if g.level == level {
                return Ok(Applied::Unchanged);
            }
            changed(commit::set_pools(
                svc,
                ctx,
                origin,
                guild,
                PoolEdit {
                    level: Some(level),
                    ..PoolEdit::default()
                },
                cb,
            )?)
        }
        GuildCmd::Pxp(pxp) => {
            if g.pxp == pxp {
                return Ok(Applied::Unchanged);
            }
            changed(commit::set_pools(
                svc,
                ctx,
                origin,
                guild,
                PoolEdit {
                    pxp: Some(pxp),
                    ..PoolEdit::default()
                },
                cb,
            )?)
        }
        GuildCmd::Gold(delta) => {
            let new = i64::from(g.gold)
                .saturating_add(delta)
                .clamp(0, i64::from(u32::MAX));
            if delta == 0 || new == i64::from(g.gold) {
                return Ok(Applied::Unchanged);
            }
            changed(commit::set_pools(
                svc,
                ctx,
                origin,
                guild,
                PoolEdit {
                    gold_delta: Some(delta),
                    ..PoolEdit::default()
                },
                cb,
            )?)
        }
        GuildCmd::Master(c) => {
            if !g.members.contains_key(&c) {
                return Err(GuildRefusal::NotMember);
            }
            if g.master() == Some(c) {
                return Ok(Applied::Unchanged);
            }
            if g.at_war() {
                return Err(GuildRefusal::AtWar);
            }
            changed(commit::change_master(svc, ctx, origin, guild, c, cb)?)
        }
        GuildCmd::AddMember(c, rank) => {
            if g.at_war() {
                return Err(GuildRefusal::AtWar);
            }
            match svc.social.guild.guild_of(c) {
                Some(gid) if gid == guild => {
                    return match g.members.get(&c) {
                        Some(m) if m.rank == rank => Ok(Applied::Unchanged),
                        _ => Err(GuildRefusal::InGuild),
                    };
                }
                Some(_) => return Err(GuildRefusal::InGuild),
                None => {}
            }
            target_access(svc, ctx, c, admin_of(actor))?;
            let at = commit::now(ctx);
            if !ctx.cfg.rules.features.remove_guild_cooldown && svc.social.guild.in_cooldown(c, at)
            {
                return Err(GuildRefusal::Cooldown);
            }
            changed(commit::add_member(svc, ctx, origin, guild, c, rank, cb)?)
        }
        GuildCmd::RemoveMember(c) => {
            if !g.members.contains_key(&c) {
                return Ok(Applied::Unchanged);
            }
            if g.at_war() {
                return Err(GuildRefusal::AtWar);
            }
            target_access(svc, ctx, c, admin_of(actor))?;
            let why = match origin {
                Origin::Admin | Origin::System => Departure::Admin,
                Origin::Player(_) | Origin::Gm(_) => Departure::Kick(origin.character(ctx)),
            };
            changed(commit::remove_member(svc, ctx, origin, guild, c, why, cb)?)
        }
        GuildCmd::Rank(c, rank) => {
            let m = g.members.get(&c).ok_or(GuildRefusal::NotMember)?;
            if g.at_war() {
                return Err(GuildRefusal::AtWar);
            }
            if rank == GuildRank::Master {
                if m.rank == GuildRank::Master {
                    return Ok(Applied::Unchanged);
                }
                return changed(commit::change_master(svc, ctx, origin, guild, c, cb)?);
            }
            if m.rank == GuildRank::Master {
                return Err(GuildRefusal::Invalid(
                    "demote the master with a master change",
                ));
            }
            if m.rank == rank {
                return Ok(Applied::Unchanged);
            }
            if g.count_rank(rank) >= g.rank_cap(ctx.data, rank) {
                return Err(GuildRefusal::RankFull);
            }
            changed(commit::set_rank(svc, ctx, origin, guild, c, rank, cb)?)
        }
    }
}

/// Dissolves `guild` like the master's request (members told, buffs removed, the bank lost): refused at war.
pub fn disband(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    guild: GuildId,
    actor: &GuildActor,
    cb: Option<CommitCb>,
) -> Result<GuildChange, GuildRefusal> {
    commit::disband(svc, ctx, Origin::from(actor), guild, cb)
}

// ---- the console's creation --------------------------------------------------------------------------------------

/// The console's `GuildCreate` (25 §6.2): no NPC, quest, cost, party or online rules; the master and the members may
/// be offline when their accounts are held; rank caps and `max_members` still apply. ONE `AdminGuildCreate` batch
/// (guild, members, the bank, `RaiseCounter(next_guild_id)`, and the master's snapshot only when he is online).
/// Returns the new id and the `create` change.
pub fn create_admin(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    spec: AdminGuildSpec,
    admin: &AdminGuildCtx,
    cb: Option<CommitCb>,
) -> Result<(GuildId, Vec<GuildChange>), GuildRefusal> {
    // ---- validation: nothing changes on a refusal ----
    let name = validate_guild_name(ctx, &spec.name).map_err(GuildRefusal::InvalidName)?;
    if svc.social.guild.name_taken(&name) {
        return Err(GuildRefusal::NameTaken);
    }
    let level = i32::try_from(spec.level).map_err(|_| GuildRefusal::Invalid("level"))?;
    if !(1..=MAX_GUILD_LEVEL).contains(&level) {
        return Err(GuildRefusal::Invalid("level"));
    }
    if spec.logo > GUILD_LOGO_MAX {
        return Err(GuildRefusal::Invalid("logo"));
    }
    if spec.notice.len() > GUILD_NOTICE_MAX {
        return Err(GuildRefusal::Invalid("notice"));
    }
    let gold = u32::try_from(spec.gold).map_err(|_| GuildRefusal::Invalid("gold"))?;
    let mut seen = BTreeSet::new();
    let mut roster: Vec<(CharacterId, GuildRank)> = vec![(spec.master, GuildRank::Master)];
    for (c, r) in &spec.members {
        if *r == GuildRank::Master {
            return Err(GuildRefusal::Invalid("a second master"));
        }
        roster.push((*c, *r));
    }
    let at = commit::now(ctx);
    let cooldowns = !ctx.cfg.rules.features.remove_guild_cooldown;
    for (c, _) in &roster {
        if !seen.insert(*c) {
            return Err(GuildRefusal::Invalid("a character twice"));
        }
        target_access(svc, ctx, *c, Some(admin))?;
        if svc.social.guild.guild_of(*c).is_some() {
            return Err(GuildRefusal::InGuild);
        }
        if cooldowns && !spec.override_cooldown && svc.social.guild.in_cooldown(*c, at) {
            return Err(GuildRefusal::Cooldown);
        }
    }
    let max = super::model::max_members(ctx.data, level);
    if roster.len() > max {
        return Err(GuildRefusal::Full);
    }
    for rank in GuildRank::ALL {
        let n = roster.iter().filter(|(_, r)| *r == rank).count();
        if n > flyff_rules::social::guild_rank_cap(rank.as_u8(), max) {
            return Err(GuildRefusal::RankFull);
        }
    }
    let master_snapshot = commit::avatar_of(svc, spec.master)
        .map(PlayerRef)
        .filter(|p| crate::econ::mail::commit::can_snapshot(ctx, *p));
    // ---- the commit block ----
    let id = svc.social.guild.alloc_id();
    let mut guild = Guild::new(id, name.clone(), at);
    guild.level = level;
    guild.gold = gold;
    guild.pxp = spec.pxp;
    guild.logo = spec.logo;
    guild.notice = spec.notice.clone();
    if let Some(doc) = &spec.bank {
        guild.bank = GuildBank::from_doc(doc.clone());
    }
    for (c, r) in &roster {
        guild.members.insert(*c, GuildMemberState::new(*c, *r, at));
    }
    let record = guild.to_record();
    let rows: Vec<_> = roster
        .iter()
        .filter_map(|(c, _)| guild.members.get(c))
        .map(|m| m.to_record(id))
        .collect();
    let bank_doc = guild.bank.to_doc();
    let mut ops = DbOp::guild_create_ops(record, rows);
    for op in &mut ops {
        if let DbOp::SetGuildBank { doc, .. } = op {
            *doc = bank_doc.clone();
        }
    }
    let after = json!({
        "name": name, "master": spec.master.0, "level": level, "gold": gold, "pxp": spec.pxp,
        "logo": spec.logo, "notice": spec.notice,
        "members": roster.iter().map(|(c, r)| json!([c.0, r.audit_name()])).collect::<Vec<_>>(),
    });
    svc.social.guild.insert(guild);
    let snap =
        master_snapshot.and_then(|p| snapshot_for_batch(svc, ctx, p).map(|(s, g)| (p, s, g)));
    let (snapshot, saved) = match snap {
        Some((p, s, g)) => (Some(s), vec![(p, g)]),
        None => (None, Vec::new()),
    };
    let batch = AtomicBatch::admin_guild_create(snapshot, id, ops);
    commit::submit(svc, ctx, "AdminGuildCreate", batch, &saved, cb);
    for (c, _) in &roster {
        svc.social.guild.sync_member(&svc.presence, ctx, *c);
    }
    commit::announce_created(svc, ctx, id);
    commit::audit_created(ctx, Origin::Admin, svc, id);
    Ok((
        id,
        vec![GuildChange {
            action: "create",
            guild: id,
            character: Some(spec.master),
            before: None,
            after: Some(after),
        }],
    ))
}
