//! Read-only admin queries (Plan/25 §11.5; WP-ADMIN-06 lands every variant, the area files implement them).
//!
//! `GameInput::AdminView{conn, op: Query{q, reply}}` is answered **in the same tick** by [`run`], which borrows the
//! whole [`Game`] **immutably**: a query cannot change world state, the borrow checker enforces ADM-D4. It is not
//! recorded in the replay log and never wakes a monster (it reads the object store, the layers and the services;
//! it never calls a visibility function).
//!
//! | area file | queries | work package |
//! |---|---|---|
//! | [`world`] | `InspectObject`, `LayerList`, `OnlineSiblings`, `Find` | WP-ADMIN-06 (implemented) |
//! | [`characters`] | `InspectCharacter`, `Containers` | WP-ADMIN-16 |
//! | [`mail`] | `Mailbox` | WP-ADMIN-16 |
//! | [`moderation`] | `ListMutes` | WP-ADMIN-17 |
//! | [`items`] | `FindItems` | WP-ADMIN-20 |
//! | [`guilds`] | `GuildList`, `GuildDetail`, `GuildNameCheck`, `PartyMembers`; the guild-bank snapshot of `Containers` | WP-ADMIN-23 |
//! | [`spawns`] | `SpawnStatus`, `Census`, `EventInfo` | WP-ADMIN-30 |
//! | [`server`] | `ListJobs` | WP-ADMIN-26 |
//! | [`comm`] | `SystemMails` | WP-ADMIN-27 |
//!
//! **Adding a query body** (every later work package): replace the stub in your area file; if the answer needs a
//! result type, add its variant to [`AdminQueryResult`] next to the others (one line) and keep the type in your area
//! file. [`run`] already routes your query to your function.

pub mod characters;
pub mod comm;
pub mod guilds;
pub mod items;
pub mod mail;
pub mod moderation;
pub mod server;
pub mod spawns;
pub mod world;

use super::events::{EntityDetail, LayerInfo};
use super::types::{AdminErrorCode, AdminFailure, ContainerLoc};
use crate::tick::Game;
use flyff_common::{AccountId, CharacterId, GuildId, ObjId, PartyId, WorldId};

/// A read-only question to the game thread (25 §11.5 `AdminQuery`).
#[derive(Clone, Debug, PartialEq)]
pub enum AdminQuery {
    /// The detail of one world object (`EntityDetail`).
    InspectObject(ObjId),
    /// The live state of one character (WP-ADMIN-16).
    InspectCharacter(CharacterId),
    /// One container of a character (WP-ADMIN-16).
    Containers {
        /// The character.
        character: CharacterId,
        /// Which container.
        container: ContainerLoc,
    },
    /// The mailbox of a character (WP-ADMIN-16).
    Mailbox(CharacterId),
    /// The online players, monsters and NPCs matching a text, a character id or a prop id (25 §3.9).
    Find(String),
    /// Where an item id or serial lives right now (WP-ADMIN-20).
    FindItems(items::ItemQuery),
    /// The active mutes (WP-ADMIN-17).
    ListMutes,
    /// The respawn lines of a world (WP-ADMIN-30).
    SpawnStatus(WorldId),
    /// The monster population of a world by prop (WP-ADMIN-30).
    Census(WorldId),
    /// The Event.lua modifiers, the running Lua events and the weather effect (WP-ADMIN-30).
    EventInfo,
    /// The newest system mails, at most `limit`, of everybody or of one character (WP-ADMIN-27).
    SystemMails {
        /// The most rows.
        limit: usize,
        /// Only the mails of this character.
        receiver: Option<CharacterId>,
    },
    /// The guilds (WP-ADMIN-23).
    GuildList(guilds::GuildFilter),
    /// One guild (WP-ADMIN-23).
    GuildDetail(GuildId),
    /// Whether a text is a usable guild name now: the name rules and the case-insensitive uniqueness (WP-ADMIN-23).
    GuildNameCheck(String),
    /// The members of a party, for the grant dialog's "party of …" (WP-ADMIN-23).
    PartyMembers(PartyId),
    /// The live layers other than layer 0 (`LayerInfo`s), of one world or of all.
    LayerList(Option<WorldId>),
    /// The scheduler's jobs (WP-ADMIN-26).
    ListJobs,
    /// The online characters of an account: the siblings through which account-scoped banks are live (25 ADM-D8).
    OnlineSiblings(AccountId),
}

impl AdminQuery {
    /// A short name for logs and errors.
    pub fn name(&self) -> &'static str {
        match self {
            AdminQuery::InspectObject(_) => "inspect object",
            AdminQuery::InspectCharacter(_) => "inspect character",
            AdminQuery::Containers { .. } => "containers",
            AdminQuery::Mailbox(_) => "mailbox",
            AdminQuery::Find(_) => "find",
            AdminQuery::FindItems(_) => "find items",
            AdminQuery::ListMutes => "list mutes",
            AdminQuery::SpawnStatus(_) => "spawn status",
            AdminQuery::Census(_) => "census",
            AdminQuery::EventInfo => "event info",
            AdminQuery::SystemMails { .. } => "system mails",
            AdminQuery::GuildList(_) => "guild list",
            AdminQuery::GuildDetail(_) => "guild detail",
            AdminQuery::GuildNameCheck(_) => "guild name check",
            AdminQuery::PartyMembers(_) => "party members",
            AdminQuery::LayerList(_) => "layer list",
            AdminQuery::ListJobs => "list jobs",
            AdminQuery::OnlineSiblings(_) => "online siblings",
        }
    }
}

/// The answer to an [`AdminQuery`].
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum AdminQueryResult {
    /// The query failed (`NotImplemented` for a stub, `NotFound` for a missing target).
    Failed(AdminFailure),
    /// `InspectObject`.
    Object(Box<EntityDetail>),
    /// `Find`.
    Find(Vec<world::FindHit>),
    /// `LayerList`.
    Layers(Vec<LayerInfo>),
    /// `OnlineSiblings`.
    Siblings(Vec<world::Sibling>),
    /// `InspectCharacter` (WP-ADMIN-16): the live detail, or the directory's facts with `online = false`.
    Character(Box<super::watch::CharacterDetail>),
    /// `Containers` (WP-ADMIN-16).
    Container(Box<super::watch::ContainerSnapshot>),
    /// `Mailbox` (WP-ADMIN-16).
    Mailbox(Vec<mail::MailView>),
    /// `ListMutes` (WP-ADMIN-17).
    Mutes(Vec<moderation::MuteRow>),
    /// `ListJobs` (WP-ADMIN-26).
    Jobs(Vec<server::JobRow>),
    /// `FindItems` (WP-ADMIN-20).
    Items(Box<items::FindItemsResult>),
    /// `SpawnStatus` (WP-ADMIN-30).
    Spawns(Box<spawns::SpawnStatusResult>),
    /// `Census` (WP-ADMIN-30).
    Census(Box<spawns::CensusResult>),
    /// `EventInfo` (WP-ADMIN-30).
    EventInfo(Box<spawns::EventInfo>),
    /// `SystemMails` (WP-ADMIN-27).
    SystemMails(Vec<comm::SystemMailRow>),
    /// `GuildList` (WP-ADMIN-23): every matching guild in id order.
    Guilds(Vec<guilds::GuildRow>),
    /// `GuildDetail` (WP-ADMIN-23).
    Guild(Box<guilds::GuildDetail>),
    /// `GuildNameCheck` (WP-ADMIN-23).
    GuildName(guilds::GuildNameCheck),
    /// `PartyMembers` (WP-ADMIN-23).
    Party(Vec<CharacterId>),
}

impl AdminQueryResult {
    /// A `NotFound` failure.
    pub fn not_found(message: impl Into<String>) -> AdminQueryResult {
        AdminQueryResult::Failed(AdminFailure::new(AdminErrorCode::NotFound, message))
    }

    /// The scaffolding answer of a query whose work package has not landed.
    pub fn not_implemented(q: &str, wp: &str) -> AdminQueryResult {
        AdminQueryResult::Failed(AdminFailure::not_implemented(q, wp))
    }
}

/// Answers `q` from the state of `g` (see the module documentation). Cheap: queries run on the game thread.
pub fn run(g: &Game, q: AdminQuery) -> AdminQueryResult {
    match q {
        AdminQuery::InspectObject(id) => world::inspect_object(g, id),
        AdminQuery::InspectCharacter(c) => characters::inspect_character(g, c),
        AdminQuery::Containers {
            character,
            container,
        } => characters::containers(g, character, container),
        AdminQuery::Mailbox(c) => mail::mailbox(g, c),
        AdminQuery::Find(text) => world::find(g, &text),
        AdminQuery::FindItems(query) => items::find_items(g, &query),
        AdminQuery::ListMutes => moderation::list_mutes(g),
        AdminQuery::SpawnStatus(w) => spawns::spawn_status(g, w),
        AdminQuery::Census(w) => spawns::census(g, w),
        AdminQuery::EventInfo => spawns::event_info(g),
        AdminQuery::SystemMails { limit, receiver } => comm::system_mails(g, limit, receiver),
        AdminQuery::GuildList(f) => guilds::guild_list(g, &f),
        AdminQuery::GuildDetail(id) => guilds::guild_detail(g, id),
        AdminQuery::GuildNameCheck(raw) => guilds::guild_name_check(g, &raw),
        AdminQuery::PartyMembers(p) => guilds::party_members(g, p),
        AdminQuery::LayerList(w) => world::layer_list(g, w),
        AdminQuery::ListJobs => server::list_jobs(g),
        AdminQuery::OnlineSiblings(a) => world::online_siblings(g, a),
    }
}
