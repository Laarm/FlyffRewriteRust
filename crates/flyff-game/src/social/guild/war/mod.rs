//! Guild wars (Plan/21 §6, F-272; WP-SOCIAL-15): the declared war between two guilds, NOT the siege
//! (`WI_WORLD_GUILDWAR` / "guild combat", tasks siege-a/b).
//!
//! | module | content |
//! |---|---|
//! | [`model`] | `GuildWar`, `WarSide`, `WarRole`, `WarResult` (the ten `WR_*` codes), the wire and database builders |
//! | [`handlers`] | the five C2S handlers: declare, accept (creates the war), surrender, truce request, truce accept |
//! | [`death`] | `on_war_death`, the `GuildWar` arm of `hooks::on_player_killed` |
//! | [`result`] | [`end_war`]: win points, clean-up, `WarEnd`, the end batch |
//! | [`process()`] | slot item 11 (1 Hz, gate `events.guild_war`): master absence, the 2 h timeout, the 10 s counter writes |
//! | [`join`] | join step 13 `War` (also part of the re-attach replay) |
//! | [`wire`] | the senders |
//!
//! **One truth.** [`GuildWarService`] holds the RUNNING wars; `Guild.war` / `Guild.enemy` and the members' `war_id`
//! (guild-core's model) say who fights, and `PlayerSocial.war` of an online member is the mirror combat reads
//! ([`GuildService::sync_member`] sets it wherever `war_id` changes). An ended war is only a database row.
//!
//! **Trust (VR-043, PL-090).** The war needs a live declaration from the CURRENT master of the declaring guild, a
//! truce needs a live request of the enemy master for THAT war; anything else is ignored without a word. A refusal
//! never changes anything. **m26 / `events.guild_war` (Plan/61 m26, 21 OQ17).** With the event off declaring, accepting,
//! surrendering and the truce still work (the war exists, `PlayerSocial.war` and `SetWar` are set, the guild
//! management locks apply) but [`process()`] does not run (no timeout, no absence) and combat ignores the war; a war
//! older than two hours ends at the first [`process()`] after the event is on again. No handler reads the event.
//!
//! **Persistence.** Every state change is ONE write: the start is `AtomicBatch::guild_war_start`, the end
//! `AtomicBatch::guild_war_end`, a surrender an `Other("GuildWarSurrender")` batch (the member row and the war row),
//! the counters of the deaths and of the master's absence one `UpsertGuildWar` per war at most every
//! [`COUNTER_PERSIST_MS`] (a crash loses at most that, 13 §8). A `Failed` batch is a bug and logged; the memory is kept.

pub mod death;
pub mod handlers;
pub mod join;
pub mod model;
pub mod process;
pub mod result;
pub mod wire;

pub use death::on_war_death;
pub use model::{FLAG_WARTIME, GuildWar, WarResult, WarRole, WarSide};
pub use process::process;
pub use result::end_war;

use super::GuildService;
use crate::ctx::{GameCtx, PlayerRef};
use flyff_common::{GuildId, GuildWarId};
use flyff_db::GuildWarRecord;
use std::collections::BTreeMap;

/// The counters of a war are written at most this often (13 §8: a crash loses at most this much absence).
pub const COUNTER_PERSIST_MS: u64 = 10_000;

/// How long a member may not be attacked (`SetPKTargetLimit`) after a war: 10 seconds.
pub const PK_LIMIT_S: u32 = 10;

/// The running wars (21 §6.1): `Services.social.guild_war`.
#[derive(Debug, Default)]
pub struct GuildWarService {
    wars: BTreeMap<GuildWarId, GuildWar>,
    next_id: u32,
    /// Stored running wars that could not be loaded (a guild is missing, a guild already fights): the first
    /// [`process()`] closes each of them as a draw.
    orphans: Vec<GuildWarRecord>,
}

impl GuildWarService {
    /// The service of `StartupState.running_wars` (13 §16.9), after the guilds were loaded. Tolerant, never panics:
    /// a record whose two guilds exist (and do not fight another war) becomes a war with its counters and
    /// `started_at` from the row and sets `Guild.war` / `Guild.enemy`; a member keeps its stored `war_id` only when it
    /// equals its guild's war (otherwise it is `None` in memory, nothing is written); any other record is an
    /// orphan and is logged. `next_id` is the larger of `counter` and the highest stored id plus one.
    pub fn load(
        running: &[GuildWarRecord],
        counter: u32,
        guilds: &mut GuildService,
    ) -> GuildWarService {
        let mut svc = GuildWarService {
            next_id: counter.max(1),
            ..GuildWarService::default()
        };
        let mut records: Vec<&GuildWarRecord> = running.iter().collect();
        records.sort_by_key(|r| r.id);
        for r in records {
            svc.next_id = svc.next_id.max(r.id.0.saturating_add(1));
            if r.result.is_some() {
                continue;
            }
            let usable = r.declarer != r.acceptor
                && !svc.wars.contains_key(&r.id)
                && [r.declarer, r.acceptor]
                    .iter()
                    .all(|g| guilds.get(*g).is_some_and(|g| g.war.is_none()));
            if !usable {
                tracing::error!(
                    war = r.id.0,
                    declarer = r.declarer.0,
                    acceptor = r.acceptor.0,
                    "a stored running guild war cannot be loaded (a guild is missing or already fights): it is closed as a draw by the first process"
                );
                svc.orphans.push(r.clone());
                continue;
            }
            for (own, enemy) in [(r.declarer, r.acceptor), (r.acceptor, r.declarer)] {
                if let Some(g) = guilds.get_mut(own) {
                    g.war = Some(r.id);
                    g.enemy = Some(enemy);
                }
            }
            svc.wars.insert(r.id, GuildWar::from_record(r));
        }
        // a stored `war_id` counts only while it is the war of the member's guild
        for id in guilds.ids() {
            if let Some(g) = guilds.get_mut(id) {
                let war = g.war;
                for m in g.members.values_mut() {
                    if m.war_id.is_some() && m.war_id != war {
                        m.war_id = None;
                    }
                }
            }
        }
        svc
    }

    /// The running war `id`.
    pub fn get(&self, id: GuildWarId) -> Option<&GuildWar> {
        self.wars.get(&id)
    }

    /// The running war `id`, mutable (the counters).
    pub fn get_mut(&mut self, id: GuildWarId) -> Option<&mut GuildWar> {
        self.wars.get_mut(&id)
    }

    /// Every running war in id order (the admin console, the GM views).
    pub fn running(&self) -> impl Iterator<Item = &GuildWar> {
        self.wars.values()
    }

    /// The ids of the running wars in id order (a copy, so the caller may end wars while it walks).
    pub fn ids(&self) -> Vec<GuildWarId> {
        self.wars.keys().copied().collect()
    }

    /// The running war of `guild`, if it fights one.
    pub fn of_guild(&self, guild: GuildId) -> Option<&GuildWar> {
        self.wars.values().find(|w| w.role_of(guild).is_some())
    }

    /// The number of running wars.
    pub fn len(&self) -> usize {
        self.wars.len()
    }

    /// `true` without a running war.
    pub fn is_empty(&self) -> bool {
        self.wars.is_empty()
    }

    /// The id the next war gets (`meta.next_guild_war_id`).
    pub fn next_id(&self) -> u32 {
        self.next_id
    }

    /// The stored running wars that were not loaded and wait for the first [`process()`].
    pub fn orphans(&self) -> &[GuildWarRecord] {
        &self.orphans
    }

    /// Takes the next war id.
    pub(crate) fn alloc_id(&mut self) -> GuildWarId {
        let id = GuildWarId(self.next_id.max(1));
        self.next_id = id.0.saturating_add(1);
        id
    }

    /// Adds a war.
    pub(crate) fn insert(&mut self, war: GuildWar) {
        self.next_id = self.next_id.max(war.id.0.saturating_add(1));
        self.wars.insert(war.id, war);
    }

    /// Removes a war (the end of it).
    pub(crate) fn remove(&mut self, id: GuildWarId) -> Option<GuildWar> {
        self.wars.remove(&id)
    }

    /// Takes the orphans.
    pub(crate) fn take_orphans(&mut self) -> Vec<GuildWarRecord> {
        std::mem::take(&mut self.orphans)
    }
}

/// The 10 s protection after a war (`pUser->SetPKTargetLimit( 10 )`, `guildwar.cpp:244` and `:260`). A SEAM: neither
/// the combat crate nor the C++ tree defines `SetPKTargetLimit` (the original's two call sites are all there is, so
/// the real limit is unspecified); the call is made once for every online member of both guilds, the declarer's
/// guild first, and does nothing yet.
pub fn on_pk_limit(_ctx: &mut GameCtx<'_>, _who: PlayerRef, _seconds: u32) {}
