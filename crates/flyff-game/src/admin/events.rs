//! What the game thread hands to an admin connection (Plan/25 §3.15 "sinks", §11.4, ADM-D5; WP-ADMIN-06).
//!
//! The game thread only **collects plain structs**: [`AdminEvent`] carries the game-side value of every
//! `AdminServerMsg` payload that a subscription produces. Diffing, protobuf encoding and sending run on the
//! connection's tokio task (`flyff-server::admin::ws`), which implements [`AdminSink`]. A sink never blocks the game
//! thread: [`AdminEvent::delivery`] says which events replace their predecessor in a *latest-wins slot* (map frames,
//! container snapshots, entity and character details, layer lists, status ticks: complete states) and which go
//! through the connection's *bounded queue* (roster deltas, chat lines, errors); a full queue is dropped, the client
//! gets `Resync{sub_id}` and the sink answers [`Pushed::Resync`] so the producer restarts the stream from a full state.

use super::chat_tap::ChatLine;
use super::map::{EntityKind, MapFrame};
use super::watch::{CharacterDetail, ContainerSnapshot};
use std::sync::Arc;

/// Why a subscription was refused or ended (the `SubscriptionErrorCode` of the protocol).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SubErrorCode {
    /// The identity lacks the capability (decided by the gateway, never by the game).
    Forbidden,
    /// A limit of subscriptions is reached.
    Limit,
    /// The specification is malformed or the feature is not implemented.
    BadSpec,
    /// The watched thing does not exist (any more).
    NotFound,
}

/// A refused subscription.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubError {
    /// Why.
    pub code: SubErrorCode,
    /// For the operator.
    pub message: String,
}

impl SubError {
    /// An error with a code and a message.
    pub fn new(code: SubErrorCode, message: impl Into<String>) -> SubError {
        SubError {
            code,
            message: message.into(),
        }
    }

    /// The scaffolding answer of a view whose work package has not landed.
    pub fn not_implemented(what: &str, wp: &str) -> SubError {
        SubError::new(
            SubErrorCode::BadSpec,
            format!("{what} is not implemented yet ({wp})"),
        )
    }
}

/// One row of a roster (`RosterEntry`): an online character.
#[derive(Clone, Debug, PartialEq)]
pub struct RosterEntry {
    /// `characters.id`.
    pub character_id: u32,
    /// The avatar's object id.
    pub obj_id: u32,
    /// The name.
    pub name: String,
    /// Level.
    pub level: u32,
    /// Job id.
    pub job: u32,
    /// World id (0 while the avatar is not placed in a world).
    pub world: u32,
    /// Layer id.
    pub layer: i32,
    /// Position x.
    pub x: f32,
    /// Position z.
    pub z: f32,
    /// Guild id (0 = none).
    pub guild_id: u32,
    /// The authority byte of the session: the ASCII code of its letter (`L` = 76), as in `HelloAck.authority`.
    pub authority: u32,
}

/// A roster message (`RosterDelta`): `full` replaces the client's table, otherwise the upserts and removes are
/// applied to it.
#[derive(Clone, Debug, PartialEq)]
pub struct RosterDelta {
    /// `true`: the first message of a stream, or the restart after a [`Pushed::Resync`].
    pub full: bool,
    /// New or changed rows.
    pub upserts: Vec<RosterEntry>,
    /// Characters that left.
    pub removes: Vec<u32>,
    /// Connected sockets that are not in the world.
    pub lobby: u32,
    /// Avatars in the world.
    pub in_world: u32,
}

/// One live layer other than layer 0 (`LayerInfo`, 25 §3.14).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerInfo {
    /// World id.
    pub world: u32,
    /// Layer id (`< 0` ticket copy, `> 0` instance, housing room or guild house).
    pub layer: i32,
    /// `ticket`, `instance`, `mini_room` or `guild_house`.
    pub kind: String,
    /// The owner as text: a guild id, a character name; empty when the layer has none.
    pub owner: String,
    /// Players in the layer.
    pub members: u32,
    /// Monsters in the layer.
    pub monsters: u32,
    /// The tick the layer was created; `0` = unknown (the layer does not record it yet).
    pub created_tick: u64,
    /// Seconds until the layer closes; `0` = no expiry known.
    pub seconds_left: u32,
}

/// The 1 Hz status of a `StatusSubscribe` (`StatusTick`). `alerts` and `maintenance` are filled by the gateway
/// (the game knows neither); the game sends `0` / `false`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StatusTick {
    /// Avatars in the world.
    pub in_world: u32,
    /// Connected sockets that are not in the world.
    pub lobby: u32,
    /// Tick duration p50 in µs.
    pub tick_p50_us: u32,
    /// Tick duration p99 in µs.
    pub tick_p99_us: u32,
    /// Persistence in retry mode.
    pub db_degraded: bool,
    /// Persistence queue depth.
    pub db_queue: u32,
    /// Active alerts (gateway).
    pub alerts: u32,
    /// Maintenance mode is on (gateway).
    pub maintenance: bool,
    /// Seconds until shutdown; `0` = no countdown.
    pub shutdown_in_s: u32,
}

/// One entry of a monster's hate list (`HateEntry`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HateEntry {
    /// The enemy.
    pub obj_id: u32,
    /// Its name.
    pub name: String,
    /// The summed damage.
    pub hate: u32,
}

/// What `EntityWatch` and `InspectObject` answer (`EntityDetail`). Positions are metres; `flags` is a
/// [`MapFlags`](super::map::MapFlags) word.
#[derive(Clone, Debug, PartialEq)]
pub struct EntityDetail {
    /// The object.
    pub obj_id: u32,
    /// What it is.
    pub kind: EntityKind,
    /// `MoverProp` / `ItemProp` / `CtrlProp` id (players: `MI_MALE` / `MI_FEMALE`).
    pub prop: u32,
    /// Display name (players: the character name; empty when unknown).
    pub name: String,
    /// World id.
    pub world: u32,
    /// Layer id.
    pub layer: i32,
    /// Position x.
    pub x: f32,
    /// Position y.
    pub y: f32,
    /// Position z.
    pub z: f32,
    /// Level (movers; 0 for items and ctrls).
    pub level: u32,
    /// Hit points.
    pub hp: u32,
    /// Maximum hit points.
    pub hp_max: u32,
    /// Mana points.
    pub mp: u32,
    /// Maximum mana points.
    pub mp_max: u32,
    /// Fatigue points.
    pub fp: u32,
    /// Maximum fatigue points.
    pub fp_max: u32,
    /// `MapFlags` bits.
    pub flags: u32,
    /// The AI state as text (`idle`, `stand`, `chase`, `attack`, `return`, `rage`, `asleep`, `dead`, ...); empty for
    /// players, items and ctrls.
    pub ai_state: String,
    /// The current target (monsters), `0` = none.
    pub target: u32,
    /// The top of the hate list, most damage first (at most 5).
    pub hate: Vec<HateEntry>,
    /// The respawn line the object came from (`gm_index`), `0` = none.
    pub spawn_region: u32,
    /// Tick of the next respawn of its line; `0` = unknown.
    pub respawn_tick: u64,
    /// The character id (players), `0` otherwise.
    pub character_id: u32,
    /// The owner of a pet or a summon, the owner-only holder of a ground item; `0` = none.
    pub owner: u32,
    /// `m_dwMode` of a player.
    pub mode_flags: u32,
    /// `true`: the object no longer exists (the last message of a watch).
    pub closed: bool,
}

/// What a subscription produced; the sink turns it into one `AdminServerMsg`.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum AdminEvent {
    /// A completed map frame (`MapKeyframe` / `MapDelta`, encoded by the connection).
    Map(MapFrame),
    /// The state of a watched entity.
    Entity(EntityDetail),
    /// The state of a watched character (WP-ADMIN-16).
    Character(Box<CharacterDetail>),
    /// The state of a watched container (WP-ADMIN-16).
    Container(Box<ContainerSnapshot>),
    /// A chat line (WP-ADMIN-19).
    Chat(Box<ChatLine>),
    /// A roster message.
    Roster(RosterDelta),
    /// The live layers.
    Layers(Vec<LayerInfo>),
    /// The 1 Hz status.
    Status(StatusTick),
    /// A subscription was refused or ended by the game.
    Error(SubError),
}

/// How an [`AdminEvent`] reaches the client when the client is slow.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Delivery {
    /// Replaces the pending event with the same key of the same subscription (a complete state).
    Latest(SlotKey),
    /// Goes through the connection's bounded queue (a delta, a line, an error).
    Queued,
}

/// The key of a latest-wins slot inside one subscription.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SlotKey {
    /// The event class (`0` map, `1` entity, `2` character, `3` layers, `4` status, `5` container).
    pub class: u8,
    /// Distinguishes several slots of one class (a container: kind, index and owner).
    pub extra: u64,
}

impl AdminEvent {
    /// Latest-wins slot or bounded queue (see the module documentation).
    pub fn delivery(&self) -> Delivery {
        let slot = |class, extra| Delivery::Latest(SlotKey { class, extra });
        match self {
            AdminEvent::Map(_) => slot(0, 0),
            AdminEvent::Entity(_) => slot(1, 0),
            AdminEvent::Character(_) => slot(2, 0),
            AdminEvent::Layers(_) => slot(3, 0),
            AdminEvent::Status(_) => slot(4, 0),
            AdminEvent::Container(c) => slot(5, c.container.slot_key()),
            AdminEvent::Roster(_) | AdminEvent::Chat(_) | AdminEvent::Error(_) => Delivery::Queued,
        }
    }

    /// `true` for the events that restart a stream from a full state: after a dropped queue the sink accepts nothing
    /// else of that subscription until one of these arrives (a `full` roster).
    pub fn is_full_state(&self) -> bool {
        matches!(self, AdminEvent::Roster(d) if d.full)
    }

    /// `true` when losing this event leaves the client with a wrong picture that only a full state repairs (the
    /// delta streams); a lost chat line is just lost.
    pub fn needs_restart_after_loss(&self) -> bool {
        matches!(self, AdminEvent::Roster(_))
    }
}

/// The sink's answer to [`AdminSink::push`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pushed {
    /// Taken (a latest-wins slot may have replaced an older event).
    Accepted,
    /// The bounded queue overflowed and was dropped: the client got `Resync{sub_id}`. The producer sends its next
    /// event as a full state ([`AdminEvent::is_full_state`]); a delta pushed before that is refused with the same
    /// answer.
    Resync,
    /// The connection is gone: the game drops its subscriptions.
    Closed,
}

/// The game thread's side of one admin connection. Implemented by the gateway task of the server; the game thread
/// calls it from the tick and **it must never block** (a mutex held for a few instructions, a non-blocking channel
/// send and a `Notify::notify_one` are fine).
pub trait AdminSink: Send + Sync + 'static {
    /// Hands over an event of subscription `sub`.
    fn push(&self, sub: u32, event: AdminEvent) -> Pushed;

    /// `true` once the connection is gone.
    fn is_closed(&self) -> bool;
}

/// A shared sink.
pub type SharedSink = Arc<dyn AdminSink>;
