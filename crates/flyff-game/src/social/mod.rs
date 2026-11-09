//! Social systems (Plan/21): party, guild, war, siege, messenger, couple, campus, honor. WP-SOCIAL-01 provides
//! the container and the empty hooks every other document calls from M3 on; the services fill in with their
//! work packages (WP-SOCIAL-02 .. 30).
//!
//! * [`Social`]: the container of the services (empty until their WPs land; the player directory is
//!   [`crate::presence::PlayerDirectory`] in `Services.presence`);
//! * [`PlayerSocial`]: the social state of one online player (stand-in of WP-MOVER-02, now with the cadence
//!   counter of 21 §2.2);
//! * [`hooks`]: the functions of 21 §2.9 (the directory bookkeeping of join/leave/level/job and the honor feeds
//!   are real; the kill, siege and lord hooks are stubs of their owners);
//! * [`couple`], [`campus`], [`honor`] and the per-player pass [`relation`] (WP-SOCIAL-21..23);
//! * the bodies of the 1 s slot items 4, 5, 10, 11, 12, 15: `siege::process` and [`gc1to1::process`],
//!   `guild::process` (in [`guild`]), the guild wars' [`guild::war::process()`] (WP-SOCIAL-15), `party::map_info`,
//!   `party::worker`. `sched::registry` points at them; each owning WP replaces the body of its function. Periodic
//!   job P6 is [`couple::expire`].
//!
//! Time is read from the `TickClock` only (21 §2.2): no clock type of its own.

pub mod campus;
pub mod cheer;
pub mod couple;
pub mod gc1to1;
pub mod guild;
pub mod honor;
pub mod hooks;
pub mod invites;
pub mod messenger;
pub mod party;
pub mod player_data;
pub mod relation;
pub mod siege;

use flyff_common::{CharacterId, GameMs, GuildId, PartyId};
use flyff_db::HonorDoc;

pub use messenger::block::block_notice;

/// Every service of doc 21, owned by `Services.social`. The party service and the pending invitations exist
/// (WP-SOCIAL-03); the guild, siege, messenger, couple and campus services are added by their work packages
/// without renaming this type.
#[derive(Debug, Default)]
pub struct Social {
    /// The parties and the stored advanced-party names (21 §4).
    pub party: party::PartyService,
    /// The pending invitations of every kind (21 §2.3).
    pub invites: invites::PendingInvites,
    /// The guilds, their rosters, pools and settings (21 §5; WP-SOCIAL-09).
    pub guild: guild::GuildService,
    /// The running guild wars (21 §6; WP-SOCIAL-15).
    pub guild_war: guild::war::GuildWarService,
    /// The guild siege: schedule, script, bids (21 §7; WP-SOCIAL-16).
    pub siege: siege::SiegeService,
    /// The 1:1 guild combat: tenders, line-ups, arenas (21 §8; WP-SOCIAL-25, -26).
    pub gc1to1: gc1to1::Gc1to1Service,
    /// The couples and the proposal cooldowns (21 §11, WP-SOCIAL-21).
    pub couple: couple::CoupleService,
    /// The campuses and the campus points (21 §12, WP-SOCIAL-22).
    pub campus: campus::CampusService,
    /// The friend lists of the characters in the world and the friend summons in flight (21 §9, WP-SOCIAL-19). There
    /// is no startup data: the lists arrive with `CharacterLoad`.
    pub messenger: messenger::MessengerService,
    /// The undelivered offline-note counts and the batches shown at the join (21 §9.6, WP-SOCIAL-20); seeded from
    /// `StartupState.note_counts`.
    pub notes: messenger::notes::NoteBox,
}

impl Social {
    /// The services at start: the stored advanced-party names of `StartupState.party_names` (parties themselves
    /// are never persisted, KQ-21).
    pub fn from_startup(startup: &flyff_db::StartupState) -> Social {
        let mut social = Social::default();
        social.party.names = party::PartyNames::load(
            startup
                .party_names
                .iter()
                .map(|r| (r.character_id, r.name.as_str())),
        );
        // the loads run guild, war, siege, couple, campus (B8 shared-file protocol, 6)
        social.guild = guild::GuildService::load(startup);
        social.guild_war = guild::war::GuildWarService::load(
            &startup.running_wars,
            startup.counters.next_guild_war_id,
            &mut social.guild,
        );
        // the siege and its bidder locks on the guilds (WP-SOCIAL-16)
        social.siege = siege::SiegeService::load(&startup.sieges, &startup.counters);
        social.guild.seams.siege_invite_locked = siege::bids::invite_locked;
        social.guild.seams.siege_leave_locked = siege::bids::leave_locked;
        social.guild.seams.siege_dissolve_locked = siege::bids::dissolve_locked;
        // the guild house: the teardown of a dissolved guild and the membership changes (doc 22 §12.5, task guild-house)
        social.guild.seams.on_disband = crate::event::guild_house::on_guild_disbanded;
        social.guild.seams.on_member_joined = crate::event::guild_house::on_member_joined;
        social.guild.seams.on_member_left = crate::event::guild_house::on_member_left;
        // the 1:1 guild combat and its tender locks on the guilds (WP-SOCIAL-25)
        social.gc1to1 = gc1to1::Gc1to1Service::load(&startup.gc1to1, &startup.counters);
        social.siege.seams.gc1to1_tenderer = gc1to1::is_tenderer;
        social.guild.seams.gc1to1_invite_locked = gc1to1::invite_locked;
        social.guild.seams.gc1to1_leave_locked = gc1to1::leave_locked;
        social.guild.seams.gc1to1_dissolve_locked = gc1to1::dissolve_locked;
        social.couple = couple::CoupleService::load(startup);
        social.campus = campus::CampusService::load(startup);
        social.notes = messenger::notes::NoteBox::from_counts(&startup.note_counts);
        social
    }

    /// The database purge (13 §14.2) in memory: couples and campuses that contain a purged character vanish. The
    /// report lists whom to tell (`relation::purge` sends the messages).
    pub fn purge(&mut self, ids: &[CharacterId]) -> PurgeReport {
        let campus = self.campus.purge(ids);
        PurgeReport {
            decoupled: self.couple.purge(ids),
            dissolved: campus.dissolved,
            updated: campus.updated,
        }
    }

    /// The sessions of the online members of `party` in member order (`Audience::Party`, 21 §2.8): the membership
    /// trait of doc 15's audience resolver.
    pub fn party_members_online(&self, party: PartyId) -> Vec<flyff_common::ObjId> {
        self.party
            .get(party)
            .map(|p| p.online_objs().collect())
            .unwrap_or_default()
    }

    /// Whether two characters are members of the same party (`same_party`, the loot right of 19 §6.6).
    pub fn same_party(&self, a: CharacterId, b: CharacterId) -> bool {
        self.party.same_party(a, b)
    }
}

/// What [`Social::purge`] removed that online players have to hear about.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PurgeReport {
    /// The surviving partners of the couples that went.
    pub decoupled: Vec<CharacterId>,
    /// The campuses that vanished with the members that are left in them.
    pub dissolved: Vec<(u32, Vec<CharacterId>)>,
    /// The campuses that lost a pupil and go on: their online members get a fresh `CampusUpdate`.
    pub updated: Vec<u32>,
}

/// Ticks per per-player cadence step: the couple, campus and campus-point work of a player runs when its
/// `cadence & 15 == 0`, i.e. every 16 ticks = 1.0667 s (21 §2.2, ref/14 §3.3, `src:WORLDSERVER/User.cpp:417-447`).
pub const CADENCE_PERIOD: u32 = 16;

/// Guild, war and party membership ids of a player (0 = none), plus the per-player counters of 21 §2.2.
/// Created at join, dropped at removal, never persisted except through the snapshots (`honor_doc`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlayerSocial {
    /// Ticks in the world; see [`PlayerSocial::advance`].
    pub cadence: u32,
    /// `m_idGuild`.
    pub guild: Option<GuildId>,
    /// `m_idWar`.
    pub war: u32,
    /// `m_idGuildCloak`.
    pub guild_cloak: u32,
    /// `m_idparty`.
    pub party: Option<PartyId>,
    /// The party facts the world phase reads (pickups, damage, sitting regeneration); `Some` exactly while
    /// `party` is, maintained by [`party::sync_links`] (21 §4).
    pub party_link: Option<party::PartyLink>,
    /// `m_idDuelParty`.
    pub duel_party: u32,
    /// `m_nGuildCombatState`.
    pub guild_combat_state: i32,
    /// `m_idCampus`.
    pub campus: u32,
    /// `m_nCampusPoint`.
    pub campus_point: i32,
    /// Selected honor title (`m_nHonor`), `None` = none (wire -1).
    pub honor: Option<i32>,
    /// The honor progress counters (`m_aHonorTitle[150]`, 13 §6.11), kept as the persisted document.
    pub honor_doc: HonorDoc,
    /// Parties accept invitations unless the player switched it off (21 §4.1).
    pub party_allow_join: bool,
    /// The guild bank window is open (21 §5.9).
    pub guild_bank_open: bool,
    /// Couple counter (21 §11).
    pub couple_counter: u32,
    /// The character that proposed to this player (21 §11).
    pub proposer: Option<CharacterId>,
    /// Campus timer (21 §12.4).
    pub campus_tick: Option<GameMs>,
    /// Cheer points (21 §14).
    pub cheer_points: i32,
    /// Cheer regeneration mark (21 §14): the game time at which the next point is due.
    pub tick_cheer: GameMs,
    /// A rename scroll is being committed (`m_bQuery` of the scroll, 21 §3.3): no second request meanwhile.
    pub rename_pending: bool,
    /// `CheckHonorTime` mark (21 §13.3, [`honor::check_time`]).
    pub honor_time_mark: GameMs,
    /// The 90 s pass of the per-player timer raised this (`honor::HonorSecondHooks`, which only has the mover):
    /// `relation::process` runs [`honor::check_time`] and clears it.
    pub honor_check_due: bool,
}

impl PlayerSocial {
    /// A fresh state for a player that just joined: nothing pending, parties open.
    pub fn joined() -> PlayerSocial {
        PlayerSocial {
            party_allow_join: true,
            ..PlayerSocial::default()
        }
    }

    /// One tick in the world: increments the counter and returns `true` when this tick is a cadence step
    /// (`cadence & 15 == 0`, the 16th, 32nd, ... tick after the join).
    pub fn advance(&mut self) -> bool {
        self.cadence = self.cadence.wrapping_add(1);
        self.cadence.is_multiple_of(CADENCE_PERIOD)
    }
}
