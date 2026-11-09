//! Cross-thread contract types of the game thread (Plan/01 §4.4, frozen at the end of M0):
//! everything that crosses C1 (`GameInput`) and C3 (`NetEvent`).
//!
//! Payload types owned by other documents (the GM/script admin payloads, `LiveConfig`) are minimal
//! placeholders here; they are extended by their owners without renaming. `Authority` is
//! `flyff_common::gm::Authority`.

use crate::admin::actions::{
    TargetResult,
    character::EditCharacter,
    combat::{DespawnArea, DespawnObject, Heal, KillObject, Resurrect},
    comm::{SendNotice, SetNoticeSchedule},
    grants::{CancelPendingGrant, CancelPendingRemoval, GrantBatch, GrantedReport},
    guilds::{EditGuild, GuildCreate, GuildDisband},
    items::{AdjustGold, PetAction, RemoveItem},
    moderation::{Mute, NotifyCharacter, SetModeFlags, Unmute},
    offline::OfflineCharacterEdit,
    server::RunJob,
    world::{DropItem, RespawnRegion, SetSpawnScale, SetWeather, SpawnMonsters, TeleportCharacter},
};
use crate::admin::types::{AdminConnId, AdminFailure};
use crate::admin::views::AdminViewOp;
use flyff_common::rt::{KickReason, LostCause, Reply, SessionSink};
use flyff_common::{AccountId, CharacterId, GuildId, ObjId, SaveGen, SessionId, UnixTime};
use flyff_db::CharacterLoad;
use flyff_lua::ReloadTarget;
use std::net::IpAddr;
use std::sync::Arc;

/// Decoded in-world client message (`pb::client_envelope::Msg` once Plan/11 is generated).
pub type ClientMsg = flyff_proto::ClientMsg;

/// Account authority level (the `AUTH_*` letter; Plan/23 §1): the real type of `flyff_common::gm` (WP-GM-01),
/// re-exported here because it is part of the C1 contract (`JoinRequest.authority`, `AdminCmd::AuthorityChanged`).
pub use flyff_common::gm::Authority;

/// Input to the game thread (C1, FIFO across all senders).
pub enum GameInput {
    /// Lobby, after `wait_flushed` + load: put the character into the world.
    Join(Box<JoinRequest>),
    /// Re-attach a new socket to a lingering avatar.
    Reattach {
        /// Account of the token.
        account: AccountId,
        /// New session.
        session: SessionId,
        /// Output of the new socket.
        sink: Box<dyn SessionSink>,
        /// Answer.
        reply: Reply<ReattachResult>,
    },
    /// In-world client message.
    Client {
        /// Sending session.
        session: SessionId,
        /// Decoded message.
        msg: ClientMsg,
    },
    /// The socket ended.
    SessionLost {
        /// Session.
        session: SessionId,
        /// Why.
        cause: LostCause,
    },
    /// Kick an account (GM, ban, takeover).
    KickAccount {
        /// Account.
        account: AccountId,
        /// Reason shown to the client.
        reason: KickReason,
        /// `true` if an avatar was found.
        reply: Option<Reply<bool>>,
    },
    /// Character create (validated by the lobby, committed here, R1).
    CreateCharacter {
        /// Account.
        account: AccountId,
        /// Request.
        req: Box<CreateCharacterRequest>,
        /// Answer.
        reply: Reply<Result<CharacterId, CharOpError>>,
    },
    /// Character delete (validated by the lobby, committed here, R1).
    DeleteCharacter {
        /// Account.
        account: AccountId,
        /// Character.
        character: CharacterId,
        /// Answer.
        reply: Reply<Result<(), CharOpError>>,
    },
    /// Console, `/admin` API or CLI command.
    Admin {
        /// Command.
        cmd: AdminCmd,
        /// Who issued it.
        actor: AdminActor,
        /// Answer (within the same tick).
        reply: Reply<AdminOutput>,
    },
    /// An admin console subscription or read-only query (Plan/25 §11.5, ADM-D4): **not recorded** in the replay log
    /// (see [`GameInput::is_recorded`]) and it never changes world state. The mutating console commands are
    /// [`GameInput::Admin`].
    AdminView {
        /// The admin WebSocket the operation belongs to.
        conn: AdminConnId,
        /// What to do.
        op: AdminViewOp,
    },
    /// `[rates]`/`[events]` hot reload.
    ReloadLive(Arc<LiveConfig>),
    /// Start (or shorten) the shutdown countdown.
    Shutdown {
        /// Seconds until everyone is logged out; `0` = now.
        countdown_s: u32,
        /// Who asked.
        reason: ShutdownReason,
        /// Operator message for the countdown notices.
        message: Option<String>,
    },
    /// The gateway's median client→server frame offset for a session (Plan/10 §4.6).
    ClockOffset {
        /// Session.
        session: SessionId,
        /// Median offset in frames.
        offset_frames: i64,
    },
}

impl GameInput {
    /// `true` for the inputs the replay log records (Plan/10 §16.1): everything that can change the simulation.
    /// `AdminView` (admin subscriptions and read-only queries, 25 ADM-D4) is **not** recorded, and neither are the
    /// test hooks. `ReplayLog` (WP-OPS-13) must ask this function instead of listing variants.
    pub fn is_recorded(&self) -> bool {
        match self {
            GameInput::AdminView { .. } => false,

            _ => true,
        }
    }
}

/// Why a shutdown was requested.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShutdownReason {
    /// SIGINT/SIGTERM.
    Signal,
    /// stdin console.
    Console,
    /// `/admin/api/shutdown`.
    AdminApi,
    /// In-game GM command.
    Gm,
    /// Test.
    Test,
}

/// Everything the game needs to place a character into the world (Plan/12 §11).
pub struct JoinRequest {
    /// Session of the socket.
    pub session: SessionId,
    /// Account.
    pub account: AccountId,
    /// Account authority.
    pub authority: Authority,
    /// Loaded character.
    pub character: Box<CharacterLoad>,
    /// Output of the socket.
    pub sink: Box<dyn SessionSink>,
    /// `Hello.client_build`.
    pub client_build: String,
}

/// Result of a re-attach attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReattachResult {
    /// The avatar was found and bound to the new sink.
    Attached,
    /// No avatar (logout happened or the linger expired).
    NoAvatar,
}

/// Fields of a character-create request (Plan/12 §9.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateCharacterRequest {
    /// Slot `0..3`.
    pub slot: u8,
    /// Trimmed, validated name.
    pub name: String,
    /// Sex.
    pub sex: u8,
    /// Hair mesh.
    pub hair_mesh: u8,
    /// Head mesh.
    pub head_mesh: u8,
    /// Skin set.
    pub skin_set: u8,
    /// Bank PIN digits.
    pub bank_pin: [u8; 4],
}

/// Why a character operation failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CharOpError {
    /// Name already used.
    #[error("name taken")]
    NameTaken,
    /// Slot already used.
    #[error("slot taken")]
    SlotTaken,
    /// Unknown character.
    #[error("not found")]
    NotFound,
    /// Not allowed during guild war.
    #[error("wartime")]
    Wartime,
    /// Character is online.
    #[error("online")]
    Online,
    /// Server is shutting down.
    #[error("shutting down")]
    ShuttingDown,
    /// Internal error.
    #[error("internal error")]
    Internal,
}

/// Events from the game thread to the lobby/gateway (C3).
#[derive(Debug)]
pub enum NetEvent {
    /// A join could not be completed.
    JoinFailed {
        /// Session.
        session: SessionId,
        /// Why.
        reason: JoinFailReason,
    },
    /// An avatar left the world.
    Left {
        /// Session.
        session: SessionId,
        /// Account.
        account: AccountId,
        /// Character.
        character: CharacterId,
        /// Generation of the final save (the lobby waits for it to be flushed).
        final_gen: SaveGen,
        /// What the socket does next.
        next: AfterLeave,
    },
}

/// Why a join failed (Plan/12 §11).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JoinFailReason {
    /// The account is already in the world.
    AlreadyOnline,
    /// The saved world does not exist.
    InvalidWorld,
    /// The loaded character violates an invariant.
    LoadInvariant,
    /// Server shutting down.
    ShuttingDown,
    /// Internal error.
    Internal,
}

/// What the socket does after the avatar left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AfterLeave {
    /// Back to the character list.
    CharSelect,
    /// Close the socket.
    Close,
}

/// A short ASCII label kept inline, so [`AdminActor`] stays `Copy` (account names are at most 16 bytes, 12 §3.1).
/// Longer text is cut on a character boundary.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ActorName {
    len: u8,
    bytes: [u8; ActorName::CAP],
}

impl ActorName {
    /// Capacity in bytes.
    pub const CAP: usize = 24;

    /// The label `text` (cut to [`ActorName::CAP`] bytes on a character boundary).
    pub fn new(text: &str) -> Self {
        let mut end = text.len().min(Self::CAP);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let mut bytes = [0u8; Self::CAP];
        if let (Some(dst), Some(src)) = (bytes.get_mut(..end), text.as_bytes().get(..end)) {
            dst.copy_from_slice(src);
        }
        Self {
            // `end <= CAP = 24`
            len: u8::try_from(end).unwrap_or(0),
            bytes,
        }
    }

    /// The text.
    pub fn as_str(&self) -> &str {
        self.bytes
            .get(..usize::from(self.len))
            .and_then(|b| std::str::from_utf8(b).ok())
            .unwrap_or("")
    }
}

impl std::fmt::Debug for ActorName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self.as_str(), f)
    }
}

impl std::fmt::Display for ActorName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl serde::Serialize for ActorName {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for ActorName {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        Ok(ActorName::new(&text))
    }
}

/// How an owner identity of the admin console got in (Plan/25 §1.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum OwnerVia {
    /// The implicit local owner of a loopback request (`admin.owner_loopback`).
    Local,
    /// The bearer token or an owner-token cookie session.
    Token,
}

/// Who issued an admin command (carried once, never repeated inside [`AdminCmd`]; Plan/10 §13.1, 25 §1.4). Serde,
/// replay-visible, `Copy`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum AdminActor {
    /// stdin console.
    Console,
    /// An owner of the admin API: the local owner, or the bearer token / owner session. Acts as ADMINISTRATOR.
    Owner {
        /// How it got in.
        via: OwnerVia,
        /// The client address.
        remote: IpAddr,
    },
    /// A staff member of the admin console: a game account whose resolved account-level authority is `>= L`.
    /// Its GM lines run with `authority` (capped at ADMINISTRATOR), not as administrator.
    Staff {
        /// The account.
        account_id: AccountId,
        /// The account name.
        account: ActorName,
        /// The resolved account-level authority (`[gm]` grants with an `ip` selector ignored, 25 §1.4).
        authority: Authority,
        /// The client address.
        remote: IpAddr,
    },
    /// CLI subcommand.
    Cli,
}

impl AdminActor {
    /// The audit actor of 25 §1.1: `console`, `cli`, `owner:local`, `owner:token`, `staff:<account>`.
    pub fn label(&self) -> String {
        match self {
            AdminActor::Console => "console".to_owned(),
            AdminActor::Cli => "cli".to_owned(),
            AdminActor::Owner {
                via: OwnerVia::Local,
                ..
            } => "owner:local".to_owned(),
            AdminActor::Owner {
                via: OwnerVia::Token,
                ..
            } => "owner:token".to_owned(),
            AdminActor::Staff { account, .. } => format!("staff:{account}"),
        }
    }

    /// The `runtime_override.set_by` label (13 §5.1): `admin:` and the audit actor.
    pub fn set_by(&self) -> String {
        format!("admin:{}", self.label())
    }

    /// The authority GM lines run with: ADMINISTRATOR for the console, the CLI and the owners, the staff
    /// member's own letter (capped at ADMINISTRATOR) otherwise (23 §3.8, 25 §1.4).
    pub fn authority(&self) -> Authority {
        match self {
            AdminActor::Staff { authority, .. } => (*authority).min(Authority::ADMINISTRATOR),
            _ => Authority::ADMINISTRATOR,
        }
    }

    /// The client address of an HTTP identity.
    pub fn remote(&self) -> Option<IpAddr> {
        match self {
            AdminActor::Owner { remote, .. } | AdminActor::Staff { remote, .. } => Some(*remote),
            AdminActor::Console | AdminActor::Cli => None,
        }
    }

    /// `true` for the identities that come through the HTTP admin API (owner or staff).
    pub fn is_http(&self) -> bool {
        self.remote().is_some()
    }
}

/// What changed on an account that ends its admin-console sessions (25 §1.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountChangeKind {
    /// A ban was stored (`/ban`).
    Banned,
    /// The account-level authority changed (`/setauth ... account`).
    Authority,
}

/// One change of an account made on the game thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccountChange {
    /// The account.
    pub account: AccountId,
    /// What changed.
    pub kind: AccountChangeKind,
}

/// The game thread's side channel to the admin console's `AccountEvents` (25 §1.8): the `/ban` and `/setauth`
/// handlers [`notify`](AccountEventSink::notify) it, the server forwards the changes to the admin sessions. It never
/// blocks (a full or disconnected channel drops the change; the console polls the database every 10 s as well). The
/// default sink is disconnected.
#[derive(Clone, Debug, Default)]
pub struct AccountEventSink(Option<flume::Sender<AccountChange>>);

impl AccountEventSink {
    /// A sink over `tx`.
    pub fn new(tx: flume::Sender<AccountChange>) -> Self {
        Self(Some(tx))
    }

    /// Reports `change`; never blocks.
    pub fn notify(&self, change: AccountChange) {
        if let Some(tx) = &self.0 {
            let _ = tx.try_send(change);
        }
    }
}

/// Target of a GM kick (Plan/23 §8.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdminTarget {
    /// A character by name.
    Character(String),
    /// An account by name.
    Account(String),
    /// Every player below GM authority.
    AllPlayers,
}

/// The ONE admin command union (Plan/10 §13.1): runtime variants (10), GM variants (23 §8.2),
/// script reloads (20 §3.9). Executed on the game thread from [`GameInput::Admin`].
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum AdminCmd {
    // ---- runtime (10) ----
    /// → [`AdminOutput::Status`].
    Status,
    /// Urgent snapshots for every online character, then a persistence flush.
    SaveAll,
    /// Change a runtime-class key (console `set`).
    SetRuntime {
        /// Dotted key.
        key: String,
        /// New value.
        value: serde_json::Value,
    },
    /// Remove a runtime override.
    UnsetRuntime {
        /// Dotted key.
        key: String,
    },
    /// State digest (Plan/10 §16.4).
    Digest,
    /// Cancel a running shutdown countdown.
    CancelShutdown,
    /// Run the daily maintenance now (console `db maintenance now`, Plan/13 §23): the game selects the purge
    /// candidates from its directory and queues the retention job. Answers [`AdminOutput::Done`] with the number
    /// of characters selected for the purge; the writer's report follows in `PersistHandle::last_maintenance`.
    RunMaintenance,
    // ---- GM (23) ----
    /// Run a GM chat line, optionally as another character.
    RunLine {
        /// The line (with or without leading `/`).
        line: String,
        /// `[as <character>]` prefix.
        as_character: Option<String>,
    },
    /// List online players.
    ListOnline {
        /// Maximum rows.
        limit: Option<u32>,
    },
    /// Kick (and optionally ban).
    Kick {
        /// Who.
        target: AdminTarget,
        /// Reason text.
        reason: Option<String>,
        /// Also ban the account.
        banned: bool,
    },
    /// Server-wide notice.
    Broadcast {
        /// Text.
        text: String,
    },
    /// Read the rates.
    GetRates,
    /// Set (or reset, with `None`) a rate.
    SetRate {
        /// Rate key.
        key: String,
        /// New value; `None` = data default.
        value: Option<f64>,
    },
    /// Read the event flags.
    GetEvents,
    /// Set an event flag.
    SetEvent {
        /// Event id.
        id: u16,
        /// New state.
        state: u8,
    },
    /// An account's authority changed in the database.
    AuthorityChanged {
        /// Account.
        account: AccountId,
        /// Specific character, if any.
        character: Option<CharacterId>,
        /// New authority.
        authority: Authority,
    },
    // ---- scripts (20) ----
    /// Hot-reload Lua scripts.
    ReloadScripts {
        /// What to reload.
        target: ReloadTarget,
        /// Pre-read sources.
        npc_sources: Option<flyff_lua::ScriptSources>,
    },
    // ---- the admin console (Plan/25 §11.5): payloads and bodies live in `admin::actions::<area>` ----
    /// Move characters to a point (WP-ADMIN-12).
    TeleportCharacter(TeleportCharacter),
    /// Heal movers (WP-ADMIN-12).
    Heal(Heal),
    /// Resurrect a character (WP-ADMIN-12).
    Resurrect(Resurrect),
    /// Kill objects, with or without drops (WP-ADMIN-12).
    KillObject(KillObject),
    /// Remove objects without a death (WP-ADMIN-12).
    DespawnObject(DespawnObject),
    /// Remove the objects of some kinds inside a rectangle (WP-ADMIN-12).
    DespawnArea(DespawnArea),
    /// Spawn monsters (WP-ADMIN-12) → [`AdminOutput::Spawned`].
    SpawnMonsters(SpawnMonsters),
    /// Respawn a region now (WP-ADMIN-12).
    RespawnRegion(RespawnRegion),
    /// Drop an item on the ground (WP-ADMIN-12).
    DropItem(DropItem),
    /// Mute a character (WP-ADMIN-12).
    Mute(Mute),
    /// Lift a mute (WP-ADMIN-12).
    Unmute(Unmute),
    /// Change the mode flags of a character (WP-ADMIN-12).
    SetModeFlags(SetModeFlags),
    /// Message characters (WP-ADMIN-12).
    NotifyCharacter(NotifyCharacter),
    /// Edit an online character: level, job, stats, skills, quests, rename (WP-ADMIN-12, rename: -16).
    EditCharacter(EditCharacter),
    /// Edit an offline character under an online-registry hold (WP-ADMIN-16).
    OfflineCharacterEdit(OfflineCharacterEdit),
    /// Remove an item from a container (WP-ADMIN-16).
    RemoveItem(RemoveItem),
    /// Add or remove penya (WP-ADMIN-16).
    AdjustGold(AdjustGold),
    /// Act on a pet (WP-ADMIN-16).
    PetAction(PetAction),
    /// Deliver items and penya to online characters (WP-ADMIN-20) → [`AdminOutput::Granted`].
    GrantBatch(GrantBatch),
    /// Cancel a pending grant (WP-ADMIN-20).
    CancelPendingGrant(CancelPendingGrant),
    /// Cancel a pending removal (WP-ADMIN-20).
    CancelPendingRemoval(CancelPendingRemoval),
    /// Create a guild (WP-ADMIN-23) → [`AdminOutput::Guild`].
    GuildCreate(GuildCreate),
    /// Edit a guild (WP-ADMIN-23).
    GuildEdit(EditGuild),
    /// Disband a guild (WP-ADMIN-23).
    GuildDisband(GuildDisband),
    /// Replace the schedule of notices (WP-ADMIN-27).
    SetNoticeSchedule(SetNoticeSchedule),
    /// Send a notice now to everybody, a world or some characters (WP-ADMIN-27) → [`AdminOutput::Done`].
    SendNotice(SendNotice),
    /// Set the weather of a world (WP-ADMIN-30).
    SetWeather(SetWeather),
    /// Run a scheduler job now (WP-ADMIN-26).
    RunJob(RunJob),
    /// Scale a respawn line for a while (WP-ADMIN-31, MAY).
    SetSpawnScale(SetSpawnScale),
}

/// Status line of `flyff status` / `/admin/api/status` (Plan/23 §8.2).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StatusSnapshot {
    /// Last tick index.
    pub tick: u64,
    /// Seconds since the process started.
    pub uptime_s: u64,
    /// Avatars in the world.
    pub in_world: u32,
    /// Sessions that are connected but not in the world.
    pub lobby: u32,
    /// Tick duration p50 in µs (0 until measured).
    pub tick_p50_us: u32,
    /// Tick duration p99 in µs (0 until measured).
    pub tick_p99_us: u32,
    /// Persistence in retry mode.
    pub db_degraded: bool,
    /// Persistence queue depth.
    pub db_queue: u32,
    /// Seconds until shutdown, if a countdown runs.
    pub shutdown_in_s: Option<u32>,
}

/// One online player (Plan/23 §8.2 `OnlineEntry`). `ip` is empty when the game thread builds the entry: the
/// session's address is the gateway's; the server fills it from the online registry.
#[derive(Clone, Debug, PartialEq)]
pub struct OnlineEntry {
    /// Session.
    pub session: SessionId,
    /// Account.
    pub account_id: AccountId,
    /// Character.
    pub character_id: CharacterId,
    /// Character name.
    pub name: String,
    /// Level.
    pub level: u16,
    /// Job id (`-1` clamps to 0).
    pub job: u8,
    /// World id.
    pub world: u32,
    /// Layer id.
    pub layer: i32,
    /// Position.
    pub pos: [f32; 3],
    /// The authority letter of the session (`F`, `L`, `P`, ...).
    pub authority: char,
    /// Client address (filled by the server).
    pub ip: String,
    /// In the world since.
    pub in_world_since: UnixTime,
    /// `true` while the avatar lingers without a socket.
    pub lingering: bool,
}

/// One row of `GET /admin/api/rates` (Plan/23 §8.2 `RateEntry`).
#[derive(Clone, Debug, PartialEq)]
pub struct RateEntry {
    /// The short key (`exp`, `drop`, `gold`, `hit`, `shop`, `party`, `aggressive`).
    pub key: &'static str,
    /// The `flyff.toml` key.
    pub config_key: &'static str,
    /// What the data files give.
    pub data_default: f32,
    /// The value of `flyff.toml` (`None` = unset).
    pub toml: Option<f32>,
    /// The runtime override in force, if any.
    pub overridden: Option<f32>,
    /// The value the game uses now.
    pub effective: f32,
}

/// One row of `GET /admin/api/events` (Plan/23 §8.2 `EventEntry`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventEntry {
    /// Event id.
    pub id: u16,
    /// The `[events]` key of the id, when it has one.
    pub key: Option<&'static str>,
    /// The state in force.
    pub state: u8,
    /// A runtime override is in force.
    pub overridden: bool,
}

/// Answer to an [`AdminCmd`] (Plan/10 §13.1).
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum AdminOutput {
    /// Free-form text lines.
    Lines(Vec<String>),
    /// Online players.
    Online(Vec<OnlineEntry>),
    /// Status.
    Status(StatusSnapshot),
    /// The rates (`GetRates`, `SetRate`).
    Rates(Vec<RateEntry>),
    /// The event flags (`GetEvents`, `SetEvent`).
    Events(Vec<EventEntry>),
    /// A command that changed `affected` things.
    Done {
        /// Number of affected objects.
        affected: u32,
    },
    /// State digest.
    Digest {
        /// Tick of the digest.
        tick: u64,
        /// Digest value.
        digest: u128,
    },
    /// The command failed.
    Error(String),
    /// A console command failed with a code (`NotImplemented` for the scaffolding stubs of WP-ADMIN-06, `StaleTarget`,
    /// `NotFound`, ...): the server maps the code to the HTTP status.
    Failed(AdminFailure),
    /// `GrantBatch`: what happened per recipient (WP-ADMIN-20).
    Granted(GrantedReport),
    /// `SpawnMonsters`: the objects that were created (WP-ADMIN-12).
    Spawned {
        /// The new objects.
        ids: Vec<ObjId>,
    },
    /// A group command: the outcome per target (WP-ADMIN-12).
    Report {
        /// One entry per target, in request order.
        per_target: Vec<TargetResult>,
    },
    /// `GuildCreate`: the new guild (WP-ADMIN-23).
    Guild {
        /// Its id.
        id: GuildId,
    },
}

pub use crate::live::LiveConfig;
