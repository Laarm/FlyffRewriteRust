//! Guilds (Plan/21 §5; WP-SOCIAL-09..12): the model and its loading, the roster messages and the login fan-out,
//! creation, naming, the rename scroll and dissolution, membership and permissions, contribution, levels, logo,
//! notice and the kill contribution, and the trusted-caller API for GM commands, NPCs and the admin console.
//!
//! | module | content | WP |
//! |---|---|---|
//! | [`model`] | `Guild`, `GuildMemberState`, `GuildRank`, `GuildPower`, the database record builders | 09 |
//! | [`wire`] | `GuildBrief` / `GuildFull`, the senders | 09 |
//! | [`join`] | join steps `AllGuilds` and `Guild` | 09 |
//! | [`create`] | `GuildCreateReq`, `check_create`, `create_paid`, first naming, the rename scroll, dissolution | 10 |
//! | [`handlers`] | the 16 C2S handlers and `/guildinvite` | 10, 11 |
//! | [`contribution`] | `add_contribution`, the contribution request, logo, notice, the kill contribution | 12 |
//! | [`ops`] | the trusted-caller API (`apply`, `create_admin`, `disband`, `GuildChange`) | 10..12 |
//! | `commit` | the commit primitives every module shares (memory, mirrors, one persist, messages, audit) | 10..12 |
//! | [`bank`] | the 42-slot bank with its log, the guild cloak and the GM removals (P13) | 13 |
//! | [`salary`], [`buffs`] | the 21:00 wall job and the guild-level buffs | 14 |
//!
//! **One truth.** The guilds live in [`GuildService`] (`Services.social.guild`); `PlayerSocial.guild` and
//! `PlayerSocial.war` of the online members are mirrors kept by [`GuildService::sync_member`] and [`attach`].
//! **One commit path per mutation:** a handler does the player's checks, the `commit` primitive changes memory,
//! writes ONE persist (a single `DbOp` for single-entity rows, one `AtomicBatch` for everything that moves items or
//! penya or several rows), sends the messages (they never wait for `DbReply::Committed`, 21 §2.4) and audits, so a
//! player, a GM and the admin console produce identical messages, ops and audit lines. A `Failed` result of a social
//! batch is a bug: it is logged at error with the batch kind and the memory is kept.
//!
//! **Logout and login.** The other online members learn of a login with `GuildGameLogin` only when the directory
//! entry was not online before (a re-attach keeps it online); the joiner always gets `GuildGameJoin` ([`on_login`],
//! [`on_logout`]).

pub mod bank;
pub mod buffs;
pub mod contribution;
pub mod create;
pub mod handlers;
pub mod join;
pub mod model;
pub mod ops;
pub mod salary;
pub mod war;
pub mod wire;

mod commit;

pub use model::{Guild, GuildBank, GuildMemberState, GuildPower, GuildRank, max_members};

use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::input::CharOpError;
use crate::presence::{NameKey, PlayerDirectory};
use flyff_common::{CharacterId, GuildId, ObjId, UnixTime};
use flyff_data::GameData;
use flyff_db::{DbOp, StartupState};
use std::collections::{BTreeMap, BTreeSet};

/// The questions and actions of other documents that guild management depends on. Every field is a plain function
/// pointer with a harmless default (the lock is off, the teardown does nothing); siege-a/b, the GC 1:1 and secret
/// room work and the guild house install theirs.
#[derive(Clone, Copy)]
pub struct GuildSeams {
    /// The guild is a bidder of the guild siege while the siege is not closed: it may not invite
    /// (`TID_GAME_GUILDCOMBAT_NOT_INVITATION_GUILD`; doc 21 §7.7, WP-SOCIAL-16).
    pub siege_invite_locked: fn(&Services, GuildId) -> bool,
    /// The guild is a tenderer of the 1:1 guild combat while it is not closed
    /// (`TID_GAME_GUILDCOMBAT1TO1_NOT_INVITATION_GUILD`; WP-SOCIAL-25).
    pub gc1to1_invite_locked: fn(&Services, GuildId) -> bool,
    /// The guild is a tenderer of the secret room (`TID_GAME_SECRETROOM_NOT_INVITATION_GUILD`; doc 22).
    pub secret_room_invite_locked: fn(&Services, GuildId) -> bool,
    /// The guild may not lose a member by leave or kick (a siege bidder outside the bidding window; FIX of a rule the
    /// original enforced only in the client; siege-a installs it; `TID_GAME_GUILDCOMBAT_NOT_LEAVE_GUILD`).
    pub siege_leave_locked: fn(&Services, GuildId) -> bool,
    /// The guild may not be dissolved (a siege bidder; `TID_GAME_GUILDCOMBAT_NOT_DISSOLVE_GUILD`).
    pub siege_dissolve_locked: fn(&Services, GuildId) -> bool,
    /// The guild may not lose a member by leave or kick while it is a tenderer of the 1:1 guild combat and the event is
    /// not closed (FIX of a rule the original enforced only in the client, `WndGuildTabMember.cpp:645-652`;
    /// `TID_GAME_GUILDCOMBAT1TO1_NOTLEAVEGUILD`; WP-SOCIAL-25).
    pub gc1to1_leave_locked: fn(&Services, GuildId) -> bool,
    /// The guild may not be dissolved while it is a tenderer of the 1:1 guild combat (the siege's
    /// `TID_GAME_GUILDCOMBAT_NOT_DISSOLVE_GUILD`; WP-SOCIAL-25).
    pub gc1to1_dissolve_locked: fn(&Services, GuildId) -> bool,
    /// A guild was dissolved: the guild-house teardown (doc 22). Runs after the members were told.
    pub on_disband: fn(&mut Services, &mut GameCtx<'_>, GuildId),
    // ---- guild house (doc 22 §12.5, task guild-house) ----
    /// An online character joined the guild (after `SetGuild` went out): the guild house tells him (`GuildHouseAllInfo`)
    /// and applies the DSTs of its furniture.
    pub on_member_joined: fn(&mut Services, &mut GameCtx<'_>, GuildId, ObjId),
    /// An online member leaves the guild (`disband`: the guild is being dissolved), right before his mirror is cleared:
    /// the rest points go, the furniture DSTs are reset, a member inside the house is sent out.
    pub on_member_left: fn(&mut Services, &mut GameCtx<'_>, GuildId, ObjId, bool),
}

fn not_locked(_: &Services, _: GuildId) -> bool {
    false
}

fn no_disband(_: &mut Services, _: &mut GameCtx<'_>, _: GuildId) {}

fn no_member_joined(_: &mut Services, _: &mut GameCtx<'_>, _: GuildId, _: ObjId) {}

fn no_member_left(_: &mut Services, _: &mut GameCtx<'_>, _: GuildId, _: ObjId, _: bool) {}

impl Default for GuildSeams {
    fn default() -> Self {
        GuildSeams {
            siege_invite_locked: not_locked,
            gc1to1_invite_locked: not_locked,
            secret_room_invite_locked: not_locked,
            siege_leave_locked: not_locked,
            siege_dissolve_locked: not_locked,
            gc1to1_leave_locked: not_locked,
            gc1to1_dissolve_locked: not_locked,
            on_disband: no_disband,
            on_member_joined: no_member_joined,
            on_member_left: no_member_left,
        }
    }
}

impl std::fmt::Debug for GuildSeams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GuildSeams(..)")
    }
}

/// The guilds (21 §5.1): `Services.social.guild`, the one truth of the roster, the settings and the pools.
///
/// The three indexes (`by_member`, `by_name`, the map itself) are kept in step by [`GuildService::insert`],
/// [`GuildService::remove`], [`GuildService::add_member`], [`GuildService::remove_member`] and
/// [`GuildService::rename`]; [`GuildService::get_mut`] is for the fields (pools, settings, war state) and must be
/// followed by the persistence of the change in the same commit block.
#[derive(Debug, Default)]
pub struct GuildService {
    guilds: BTreeMap<GuildId, Guild>,
    by_member: BTreeMap<CharacterId, GuildId>,
    by_name: BTreeMap<NameKey, GuildId>,
    rejoin: BTreeMap<CharacterId, UnixTime>,
    next_id: u32,
    /// The seams of other documents (default: no lock, no teardown).
    pub seams: GuildSeams,
    /// The pay-once guard of the 21:00 salary (21 §5.10, PL-094; [`salary`]).
    pub salary: salary::SalaryGuard,
    /// The guild-bank removal rows this run has applied (the P13 poll, [`bank::removal`]).
    pub removals: bank::removal::RemovalGuard,
}

impl GuildService {
    /// The service of `StartupState.guilds` (13 §16.9). Tolerant, never panics: `next_id` is the larger of the
    /// counter and the highest id plus one; a member the directory does not know is dropped with an error log
    /// (nothing is written); a guild without a master is kept and logged; a second guild with a taken name or a
    /// second guild of a member is logged and left out of the index; levels, ranks and classes are clamped.
    pub fn load(startup: &StartupState) -> GuildService {
        let known: BTreeSet<CharacterId> = startup.directory.iter().map(|d| d.id).collect();
        let mut svc = GuildService {
            next_id: startup.counters.next_guild_id.max(1),
            ..GuildService::default()
        };
        for l in &startup.guilds {
            let mut g = Guild::from_load(l);
            let id = g.id;
            g.members.retain(|c, _| {
                let keep = known.contains(c) && !svc.by_member.contains_key(c);
                if !keep {
                    tracing::error!(
                        guild = id.0,
                        character = c.0,
                        "a guild member is unknown to the directory or belongs to another guild: dropped from memory"
                    );
                }
                keep
            });
            if g.master().is_none() {
                tracing::error!(guild = id.0, name = %g.name, "a guild without a master is kept");
            }
            if svc.guilds.contains_key(&id) {
                tracing::error!(
                    guild = id.0,
                    "a guild id was loaded twice: the later row is ignored"
                );
                continue;
            }
            svc.next_id = svc.next_id.max(id.0.saturating_add(1));
            svc.insert(g);
        }
        for (c, until) in &startup.rejoin_cooldowns {
            svc.rejoin.insert(*c, *until);
        }
        svc.salary = salary::SalaryGuard::load(&startup.server_kv);
        svc
    }

    // ---- queries ----------------------------------------------------------------------------------------------

    /// The guild `id`.
    pub fn get(&self, id: GuildId) -> Option<&Guild> {
        self.guilds.get(&id)
    }

    /// The guild `id`, mutable: for the fields only (never the member map: use [`GuildService::add_member`] and
    /// [`GuildService::remove_member`]); persist the change in the same commit block.
    pub fn get_mut(&mut self, id: GuildId) -> Option<&mut Guild> {
        self.guilds.get_mut(&id)
    }

    /// Every guild id in id order (a copy, so the caller may change the service while it walks).
    pub fn ids(&self) -> Vec<GuildId> {
        self.guilds.keys().copied().collect()
    }

    /// Every guild in id order.
    pub fn guilds(&self) -> impl Iterator<Item = &Guild> {
        self.guilds.values()
    }

    /// The number of guilds.
    pub fn len(&self) -> usize {
        self.guilds.len()
    }

    /// `true` without any guild.
    pub fn is_empty(&self) -> bool {
        self.guilds.is_empty()
    }

    /// The id the next guild gets (`AllGuilds.next_id`).
    pub fn next_id(&self) -> u32 {
        self.next_id
    }

    /// The guild of a character.
    pub fn guild_of(&self, c: CharacterId) -> Option<GuildId> {
        self.by_member.get(&c).copied()
    }

    /// The guild record of a character.
    pub fn guild_record_of(&self, c: CharacterId) -> Option<&Guild> {
        self.guild_of(c).and_then(|id| self.guilds.get(&id))
    }

    /// The named guild called `name` (ASCII case-folded, PL-058).
    pub fn by_name(&self, name: &str) -> Option<GuildId> {
        self.by_name.get(&NameKey::new(name)).copied()
    }

    /// `true` when a named guild already has this name (ASCII case-folded).
    pub fn name_taken(&self, name: &str) -> bool {
        self.by_name(name).is_some()
    }

    /// `true` when `c` is the master (rank 0) of its guild.
    pub fn is_master(&self, c: CharacterId) -> bool {
        self.guild_record_of(c)
            .is_some_and(|g| g.master() == Some(c))
    }

    /// `true` when `c` may use `p` in its guild (the master always may, PL-092).
    pub fn has_power(&self, c: CharacterId, p: GuildPower) -> bool {
        self.guild_record_of(c).is_some_and(|g| g.has_power(c, p))
    }

    /// The OTHER members of the guild of `c` in `CharacterId` order (the contact set of `UpdatePlayerData`,
    /// `PresenceLogout` and the join batch's `QueryPlayerDataNtf`); empty without a guild.
    pub fn members_of(&self, c: CharacterId) -> Vec<CharacterId> {
        self.guild_record_of(c)
            .map(|g| g.members.keys().copied().filter(|m| *m != c).collect())
            .unwrap_or_default()
    }

    /// The avatars of the online members of `guild` in `CharacterId` order (`Audience::Guild`, 21 §2.8). A
    /// lingering avatar counts here; the audience resolver skips it because it has no session.
    pub fn online_objs(&self, dir: &PlayerDirectory, guild: GuildId) -> Vec<ObjId> {
        self.guilds
            .get(&guild)
            .map(|g| {
                g.members
                    .keys()
                    .filter_map(|c| dir.get(*c).and_then(|e| e.online).map(|o| o.obj))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The member limit of a guild level (`levels[level - 1].max_member`).
    pub fn max_members(&self, data: &GameData, level: i32) -> usize {
        max_members(data, level)
    }

    /// The end of the rejoin cooldown of `c`, if one runs (only ever set with `features.remove_guild_cooldown =
    /// false`). Compare it with the clock: an expired entry may still be stored.
    pub fn rejoin_until(&self, c: CharacterId) -> Option<UnixTime> {
        self.rejoin.get(&c).copied()
    }

    /// `true` while the rejoin cooldown of `c` runs at `now`.
    pub fn in_cooldown(&self, c: CharacterId, now: UnixTime) -> bool {
        self.rejoin_until(c).is_some_and(|t| t > now)
    }

    // ---- mutations (inside a commit block, with their persistence) ---------------------------------------------

    /// Takes the next guild id.
    pub fn alloc_id(&mut self) -> GuildId {
        let id = GuildId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// Adds a guild with its members and name to the indexes (a name that is taken is left out of the name index).
    pub fn insert(&mut self, g: Guild) {
        let id = g.id;
        for c in g.members.keys() {
            self.by_member.insert(*c, id);
        }
        if g.is_named() {
            let key = NameKey::new(&g.name);
            match self.by_name.entry(key) {
                std::collections::btree_map::Entry::Occupied(_) => {
                    tracing::error!(guild = id.0, name = %g.name, "a guild name is taken twice: left out of the name index");
                }
                std::collections::btree_map::Entry::Vacant(slot) => {
                    slot.insert(id);
                }
            }
        }
        self.next_id = self.next_id.max(id.0.saturating_add(1));
        self.guilds.insert(id, g);
    }

    /// Removes a guild and its index entries; returns it.
    pub fn remove(&mut self, id: GuildId) -> Option<Guild> {
        let g = self.guilds.remove(&id)?;
        for c in g.members.keys() {
            if self.by_member.get(c) == Some(&id) {
                self.by_member.remove(c);
            }
        }
        let key = NameKey::new(&g.name);
        if self.by_name.get(&key) == Some(&id) {
            self.by_name.remove(&key);
        }
        Some(g)
    }

    /// Adds `m` to `guild`; `false` (and nothing changes) for an unknown guild or a character that already belongs
    /// to a guild.
    pub fn add_member(&mut self, guild: GuildId, m: GuildMemberState) -> bool {
        if self.by_member.contains_key(&m.id) {
            return false;
        }
        let Some(g) = self.guilds.get_mut(&guild) else {
            return false;
        };
        self.by_member.insert(m.id, guild);
        g.members.insert(m.id, m);
        true
    }

    /// Removes the member `c` from `guild`.
    pub fn remove_member(&mut self, guild: GuildId, c: CharacterId) -> Option<GuildMemberState> {
        let m = self.guilds.get_mut(&guild)?.members.remove(&c)?;
        if self.by_member.get(&c) == Some(&guild) {
            self.by_member.remove(&c);
        }
        Some(m)
    }

    /// Gives `guild` the name `name` (the caller checked uniqueness; `false` when another guild has it or the
    /// guild is unknown). A change of letter case of the guild's own name works.
    pub fn rename(&mut self, guild: GuildId, name: &str) -> bool {
        let key = NameKey::new(name);
        if self.by_name.get(&key).is_some_and(|g| *g != guild) {
            return false;
        }
        let Some(g) = self.guilds.get_mut(&guild) else {
            return false;
        };
        let old = NameKey::new(&g.name);
        if g.is_named() && self.by_name.get(&old) == Some(&guild) {
            self.by_name.remove(&old);
        }
        g.name = name.to_owned();
        if !name.is_empty() {
            self.by_name.insert(key, guild);
        }
        true
    }

    /// Sets (`Some`) or clears (`None`) the rejoin cooldown of `c`; expired entries of others are dropped.
    pub fn set_rejoin(&mut self, c: CharacterId, until: Option<UnixTime>, now: UnixTime) {
        self.rejoin.retain(|_, t| *t > now);
        match until {
            Some(t) => {
                self.rejoin.insert(c, t);
            }
            None => {
                self.rejoin.remove(&c);
            }
        }
    }

    /// Brings the mirrors of the ONLINE character `c` in step with the model: `PlayerSocial.guild`, `.war` (the
    /// member's `war_id`, 0 without one) and, for a character without a guild, `guild_bank_open = false`. Nothing for
    /// an offline character (its next join runs [`attach`]).
    pub fn sync_member(&self, dir: &PlayerDirectory, ctx: &mut GameCtx<'_>, c: CharacterId) {
        let Some(obj) = dir.get(c).and_then(|e| e.online).map(|o| o.obj) else {
            return;
        };
        let guild = self.guild_of(c);
        let war = guild
            .and_then(|id| self.guilds.get(&id))
            .and_then(|g| g.members.get(&c))
            .and_then(|m| m.war_id)
            .map_or(0, |w| w.0);
        set_mirror(ctx, obj, guild, war);
    }
}

/// Writes the guild mirrors of an avatar.
pub(crate) fn set_mirror(ctx: &mut GameCtx<'_>, obj: ObjId, guild: Option<GuildId>, war: u32) {
    if let Some(p) = ctx.worlds.player_mut(obj).and_then(|m| m.player_mut()) {
        p.social.guild = guild;
        p.social.war = war;
        if guild.is_none() {
            p.social.guild_bank_open = false;
        }
    }
}

// ---- the 1 s slot item ------------------------------------------------------------------------------------------

/// Slot item 10 (gate `events.guild_bank`): V15 has no per-second guild work besides the salary wall job
/// (21 §2.2, §5.10), so the body is empty and the slot owner stays explicit.
pub fn process(_svc: &mut Services, _ctx: &mut GameCtx<'_>) {}

// ---- join, login, logout -----------------------------------------------------------------------------------------

/// The character of an online avatar.
pub(crate) fn character_of(ctx: &GameCtx<'_>, p: PlayerRef) -> Option<CharacterId> {
    ctx.player(p)?.player().map(|pl| pl.ident.character_id)
}

/// Before the join batch is written (next to `party::on_reconnect`): the mirrors of the avatar are set from the
/// model, so that `SpawnSelf` / `SpawnPlayer` carry `id_guild` and `id_war`; the guild bank window is closed.
pub fn attach(svc: &mut Services, ctx: &mut GameCtx<'_>, p: PlayerRef) {
    let Some(c) = character_of(ctx, p) else {
        return;
    };
    let guild = svc.social.guild.guild_of(c);
    let war = guild
        .and_then(|id| svc.social.guild.get(id))
        .and_then(|g| g.members.get(&c))
        .and_then(|m| m.war_id)
        .map_or(0, |w| w.0);
    set_mirror(ctx, p.0, guild, war);
    if let Some(pl) = ctx.worlds.player_mut(p.0).and_then(|m| m.player_mut()) {
        pl.social.guild_bank_open = false;
    }
}

/// The avatars of the online members of `c`'s guild (including `c`) with their characters, in `CharacterId` order.
fn online_members(svc: &Services, c: CharacterId) -> Vec<(CharacterId, ObjId)> {
    let Some(g) = svc.social.guild.guild_record_of(c) else {
        return Vec::new();
    };
    g.members
        .keys()
        .filter_map(|m| {
            svc.presence
                .get(*m)
                .and_then(|e| e.online)
                .map(|o| (*m, o.obj))
        })
        .collect()
}

/// 21 §3.4 join step 3, after `announce_login`: the OTHER online members get `GuildGameLogin{login, id, multi 1}`
/// only when the directory entry was not online before this join (`was_online`; a re-attach keeps it online, so no
/// second notice); the newcomer ALWAYS gets `GuildGameJoin` listing all online members including itself.
pub fn on_login(svc: &mut Services, ctx: &mut GameCtx<'_>, p: PlayerRef, was_online: bool) {
    use flyff_proto::pb;
    let Some(c) = character_of(ctx, p) else {
        return;
    };
    let online = online_members(svc, c);
    if online.is_empty() {
        return;
    }
    if !was_online {
        let others: Vec<ObjId> = online
            .iter()
            .filter(|(m, _)| *m != c)
            .map(|(_, o)| *o)
            .collect();
        if !others.is_empty() {
            wire::to_list(
                svc,
                ctx,
                &others,
                pb::GuildGameLogin {
                    login: true,
                    id_player: c.0,
                    multi_no: 1,
                },
            );
        }
    }
    wire::to_one(
        ctx,
        p.0,
        pb::GuildGameJoin {
            ids: online.iter().map(|(m, _)| m.0).collect(),
            multis: vec![1; online.len()],
        },
    );
}

/// 21 §3.4 leave step 3: the other online members get `GuildGameLogin{login: false, id, multi 100}`.
pub fn on_logout(svc: &mut Services, ctx: &mut GameCtx<'_>, p: PlayerRef) {
    use flyff_proto::pb;
    let Some(c) = character_of(ctx, p) else {
        return;
    };
    let others: Vec<ObjId> = online_members(svc, c)
        .into_iter()
        .filter(|(m, _)| *m != c)
        .map(|(_, o)| o)
        .collect();
    if others.is_empty() {
        return;
    }
    wire::to_list(
        svc,
        ctx,
        &others,
        pb::GuildGameLogin {
            login: false,
            id_player: c.0,
            multi_no: 100,
        },
    );
}

// ---- the seams of the NPC dialog (Helena, 20 §6.7, PL-149) --------------------------------------------------------

/// The guild facts the NPC dialog helpers ask for (`IsGuild`, `IsGuildMaster`): `None` for a player without a guild.
pub fn npc_facts(svc: &Services, ctx: &GameCtx<'_>, who: ObjId) -> Option<crate::npc::GuildFacts> {
    let c = ctx.worlds.player(who)?.player()?.ident.character_id;
    let g = svc.social.guild.guild_record_of(c)?;
    Some(crate::npc::GuildFacts {
        master: g.master() == Some(c),
    })
}

/// `IsPartyGuild` (`ScriptLib.cpp:737-780`): 1 without a party or when a member is offline or in a guild, 2 for a
/// running rejoin cooldown (only with `features.remove_guild_cooldown = false`), else 0.
pub fn is_party_guild(svc: &Services, ctx: &GameCtx<'_>, who: ObjId) -> i64 {
    let Some(c) = ctx
        .worlds
        .player(who)
        .and_then(|m| m.player())
        .map(|p| p.ident.character_id)
    else {
        return 1;
    };
    match create::party_members_error(svc, ctx, c) {
        None => 0,
        Some(create::GuildCreateError::RejoinCooldown) => 2,
        Some(_) => 1,
    }
}

/// `CreateGuild()` of the dialog (`create_paid` with the party, no name, no charge: the script's own
/// `RemoveGold(3 000 000)` already ran in the same handler, PL-091, PL-149). `true` when a guild was created.
pub fn create_for_npc(svc: &mut Services, ctx: &mut GameCtx<'_>, who: ObjId) -> bool {
    let req = create::GuildCreateReq {
        leader: PlayerRef(who),
        members: create::CreateMembers::Party,
        name: None,
        charge: 0,
    };
    match create::create_paid(svc, ctx, req) {
        Ok(_) => true,
        Err(e) => {
            tracing::debug!(?e, "the guild creation of the NPC dialog was refused");
            false
        }
    }
}

// ---- character deletion (21 §3.6) --------------------------------------------------------------------------------

/// The guild part of the delete veto: `Err(Wartime)` while the character's guild is at war (12 §10.2 step 6, PL-040).
pub fn check_character_delete(svc: &Services, c: CharacterId) -> Result<(), CharOpError> {
    match svc.social.guild.guild_record_of(c) {
        Some(g) if g.at_war() => Err(CharOpError::Wartime),
        _ => Ok(()),
    }
}

/// The guild part of a character deletion (the commit part, the character is offline): a member leaves
/// (`DeleteGuildMember` is appended to `ops`, `RemoveGuildMemberNtf` goes to the online members); the guild of a
/// master is disbanded (the ops of [`DbOp::guild_disband_ops`] are appended, the dissolution messages go out).
pub fn on_character_deleted(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    c: CharacterId,
    ops: &mut Vec<DbOp>,
) {
    commit::on_character_deleted(svc, ctx, c, ops);
}
