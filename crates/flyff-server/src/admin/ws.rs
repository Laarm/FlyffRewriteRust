//! The admin WebSocket gateway `GET /admin/ws` (Plan/25 §3.15, §11.4, ADM-D3/D5; WP-ADMIN-05).
//!
//! **One socket per console tab**, binary frames, one `AdminClientMsg` or `AdminServerMsg` per frame (1 MiB limit,
//! `flyff_proto::admin`). Authentication is the layer of [`super::auth`]: the session cookie or the local owner rule,
//! `Origin == Host`, never the bearer token. This module owns what happens after the upgrade:
//!
//! * **Subscriptions** (25 §3.16): every `*Subscribe`/`*Watch` message is checked against the capabilities of the
//!   identity (`SubscriptionError{FORBIDDEN}`), validated (`BAD_SPEC`), counted against the limits
//!   (`admin.max_ws_per_identity` at the upgrade, `admin.max_map_subs_per_conn` / `max_map_subs_total` for maps:
//!   `LIMIT`) and forwarded to the game thread as `GameInput::AdminView` (never recorded, 25 ADM-D4).
//! * **The sink**: the connection registers a [`ConnSink`] with the game thread (`AdminViewOp::Attach`). The game
//!   hands it plain structs ([`AdminEvent`]); the sink never blocks it. Map frames, container snapshots, entity and
//!   character details, layer lists and status ticks are *complete states* and go to **latest-wins slots**; roster
//!   deltas, chat lines and errors go to a **bounded queue** (256). A full queue is dropped, the client gets
//!   `Resync{sub_id}` and the game restarts the stream from a full state ([`Pushed::Resync`]).
//! * **Encoding** happens here, on the connection's tokio task: the per-subscription [`MapEncoder`] (keyframe on
//!   subscribe, every 10 s and after a `Resync`; the diff is WP-ADMIN-07), plain conversions for everything else.
//! * **Backpressure**: encoded frames go through a small channel to a writer task that owns the socket. The bytes in
//!   flight are counted; above `send_cap` (2 MiB) nothing new is encoded, slots keep being replaced (memory stays
//!   flat), and the next delta is computed against the last frame *actually sent*.
//! * **Liveness**: the client pings every 15 s; 45 s of silence closes the socket (code 4002).
//! * **Session end**: [`AdminSessions::subscribe_ended`](super::session::AdminSessions::subscribe_ended) closes the sockets of the ended session with
//!   `SessionEnded{reason}` and code 4001 (a lagged receiver asks `is_live`).
//!
//! The pieces other work packages plug into: [`ws_map::new_encoder`] (WP-ADMIN-07), [`WsHub::set_status_extras`]
//! (alerts and maintenance of the status tick, WP-ADMIN-25), and the game-side hooks of `flyff_game::admin`.

use super::api::{ApiState, Routes};
use super::auth::Who;
use super::caps::Cap;
use super::error::ApiError;
use super::guard::audit_admin;
use super::identity::Identity;
use super::logs::{self, LogFilter};
use super::session::{EndReason, SessionEnded};
use super::shared::AdminShared;
use super::ws_map::{self, EncodePlan, MapEncoder};
use crate::audit::AuditFile;
use axum::Router;
use axum::extract::State;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use bytes::Bytes;
use flyff_common::gm::Authority;
use flyff_common::rt::{HealthState, Telemetry};
use flyff_common::{CharacterId, LayerId, ObjId, WorldId};
use flyff_game::GameInput;
use flyff_game::admin::chat_tap::{ChatChannel, ChatFilter};
use flyff_game::admin::events::{
    AdminEvent, AdminSink, Delivery, EntityDetail, LayerInfo, Pushed, RosterDelta, SlotKey,
    StatusTick, SubErrorCode,
};
use flyff_game::admin::map::{MapSpec, MapTuning, Tier};
use flyff_game::admin::types::AdminConnId;
use flyff_game::admin::views::{AdminViewOp, ViewLimits, ViewSpec};
use flyff_proto::admin as codec;
use flyff_proto::admin_pb as pb;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use parking_lot::{Mutex, RwLock};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::{Notify, broadcast, mpsc};

/// The close code of a socket whose session ended (after `SessionEnded`, 25 §11.4).
pub const CLOSE_SESSION_ENDED: u16 = 4001;
/// The close code of a socket that was silent for `ping_timeout` (the client reconnects).
pub const CLOSE_IDLE: u16 = 4002;
/// The standard "going away" close code (server stop).
pub const CLOSE_GOING_AWAY: u16 = 1001;
/// The standard "protocol error" close code (an undecodable frame, a text frame).
pub const CLOSE_PROTOCOL: u16 = 1002;
/// The standard "policy violation" close code (too many messages).
pub const CLOSE_POLICY: u16 = 1008;
/// The standard "internal error" close code (the game thread is gone).
pub const CLOSE_INTERNAL: u16 = 1011;

/// Subscriptions one socket may hold in total (the finer limits are the configured ones).
pub const MAX_SUBS_PER_CONN: usize = 32;
/// A map rectangle may not be larger than this in either direction (metres).
const MAX_RECT_M: f32 = 100_000.0;
/// The `kinds` bits a map subscription may carry (`1 << EntityKind`, 1..=6).
const KINDS_MASK: u32 = 0b111_1110;
/// Messages waiting for the writer task.
const OUT_CHANNEL: usize = 64;
/// Encoded messages the connection may hold back when the writer channel is full.
const OUTBOX_MAX: usize = 1024;
/// How long a message to the game thread may wait for room in its inbox.
const GAME_SEND_TIMEOUT: Duration = Duration::from_secs(2);

// ---- tuning, limits, the hub -----------------------------------------------------------------------------------------

/// The timing and size parameters of the gateway. The defaults are the plan's numbers; tests shorten them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WsTuning {
    /// Silence after which the socket is closed (25 §11.4: 45 s).
    pub ping_timeout: Duration,
    /// A full `MapKeyframe` is due this long after the last one (25 §3.15: 10 s).
    pub keyframe_every: Duration,
    /// Encoded bytes in flight above which nothing new is encoded (25 §11.4: 2 MiB).
    pub send_cap: usize,
    /// Items of the bounded per-connection queue (25 §11.4: 256).
    pub queue_cap: usize,
    /// How often the connection checks its timers.
    pub housekeeping: Duration,
    /// Client messages per second before the socket is closed (a flooding client).
    pub inbound_per_s: u32,
    /// A chat subscription with private channels writes a `gm.snoop` record this often while it is open
    /// (25 §4.9: 10 min).
    pub snoop_every: Duration,
}

impl Default for WsTuning {
    fn default() -> Self {
        Self {
            ping_timeout: Duration::from_secs(45),
            keyframe_every: Duration::from_secs(10),
            send_cap: 2 << 20,
            queue_cap: 256,
            housekeeping: Duration::from_secs(1),
            inbound_per_s: 200,
            snoop_every: Duration::from_secs(600),
        }
    }
}

/// The live limits of the gateway (`admin.max_ws_per_identity`, `max_map_subs_per_conn`, `max_map_subs_total`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WsLimits {
    /// Open sockets per identity.
    pub max_ws_per_identity: u32,
    /// Map subscriptions per socket.
    pub max_map_subs_per_conn: u32,
    /// Map subscriptions across all sockets.
    pub max_map_subs_total: u32,
}

impl Default for WsLimits {
    fn default() -> Self {
        Self {
            max_ws_per_identity: 4,
            max_map_subs_per_conn: 1,
            max_map_subs_total: 16,
        }
    }
}

/// Counters of the gateway (tests, `/metrics` later).
#[derive(Debug, Default)]
pub struct WsStats {
    /// Sockets opened.
    pub opened: AtomicU64,
    /// Map frames the game replaced in a slot before the connection took them (latest-wins drops).
    pub map_frames_replaced: AtomicU64,
    /// Map frames encoded and queued for the socket.
    pub map_frames_sent: AtomicU64,
    /// Bounded queues that overflowed and were dropped.
    pub queue_overflows: AtomicU64,
    /// The highest number of encoded bytes that were in flight on one socket.
    pub pending_high_water: AtomicUsize,
}

/// What the status tick shows besides the game's numbers: `(active alerts, maintenance mode)`.
pub type StatusExtras = Arc<dyn Fn() -> (u32, bool) + Send + Sync>;

/// Creates the encoder of a new map subscription (the argument is the `sub_id`); the default is
/// [`ws_map::new_encoder`].
pub type EncoderFactory = Arc<dyn Fn(u32) -> Box<dyn MapEncoder> + Send + Sync>;

/// The state shared by all admin sockets: connection ids, the per-identity and map-subscription counts, the limits.
pub struct WsHub {
    next_conn: AtomicU64,
    open: Mutex<BTreeMap<String, u32>>,
    map_subs: AtomicU32,
    limits: RwLock<WsLimits>,
    /// The game-side limits (`admin.map_*`), pushed to the game thread by every connection (see `sync_view_limits`).
    view: RwLock<ViewLimits>,
    /// Bumped whenever `view` changes.
    view_gen: AtomicU64,
    tuning: RwLock<WsTuning>,
    extras: RwLock<Option<StatusExtras>>,
    encoder: RwLock<EncoderFactory>,
    stop: tokio::sync::watch::Sender<bool>,
    /// The counters.
    pub stats: WsStats,
}

impl std::fmt::Debug for WsHub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WsHub")
            .field("limits", &*self.limits.read())
            .finish_non_exhaustive()
    }
}

/// A socket slot of one identity, released on drop.
pub struct SocketPermit {
    hub: Arc<WsHub>,
    key: String,
}

impl Drop for SocketPermit {
    fn drop(&mut self) {
        let mut open = self.hub.open.lock();
        if let Some(n) = open.get_mut(&self.key) {
            *n = n.saturating_sub(1);
            if *n == 0 {
                open.remove(&self.key);
            }
        }
    }
}

/// A map-subscription slot, released on drop.
pub struct MapPermit {
    hub: Arc<WsHub>,
}

impl Drop for MapPermit {
    fn drop(&mut self) {
        let _ = self
            .hub
            .map_subs
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
                Some(v.saturating_sub(1))
            });
    }
}

impl WsHub {
    /// A hub with the given limits.
    pub fn new(limits: WsLimits) -> Arc<WsHub> {
        Arc::new(WsHub {
            next_conn: AtomicU64::new(1),
            open: Mutex::new(BTreeMap::new()),
            map_subs: AtomicU32::new(0),
            limits: RwLock::new(limits),
            view: RwLock::new(ViewLimits::default()),
            view_gen: AtomicU64::new(1),
            tuning: RwLock::new(WsTuning::default()),
            extras: RwLock::new(None),
            encoder: RwLock::new(Arc::new(ws_map::new_encoder)),
            stop: tokio::sync::watch::channel(false).0,
            stats: WsStats::default(),
        })
    }

    /// The limits of a configuration.
    pub fn limits_of(cfg: &crate::config::Config) -> WsLimits {
        WsLimits {
            max_ws_per_identity: cfg.admin.max_ws_per_identity,
            max_map_subs_per_conn: cfg.admin.max_map_subs_per_conn,
            max_map_subs_total: cfg.admin.max_map_subs_total,
        }
    }

    /// The game-side limits of a configuration: the visit budget and the map tiers (25 §11.8).
    pub fn view_limits_of(cfg: &crate::config::Config) -> ViewLimits {
        ViewLimits {
            visit_budget: cfg.admin.map_visit_budget,
            map: MapTuning {
                tier_a_m: cfg.admin.map_tier_a_m,
                tier_b_m: cfg.admin.map_tier_b_m,
                max_entities: cfg.admin.map_max_entities,
            },
            ..ViewLimits::default()
        }
    }

    /// Applies the live limits of a reloaded configuration (every open socket pushes the new game-side limits to the
    /// game thread within its next housekeeping step).
    pub fn apply_config(&self, cfg: &crate::config::Config) {
        *self.limits.write() = Self::limits_of(cfg);
        self.set_view_limits(Self::view_limits_of(cfg));
    }

    /// Sets the game-side limits (`admin.map_visit_budget`, `map_tier_a_m`, `map_tier_b_m`, `map_max_entities`).
    pub fn set_view_limits(&self, v: ViewLimits) {
        let mut cur = self.view.write();
        if *cur != v {
            *cur = v;
            self.view_gen.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// The game-side limits in force.
    pub fn view_limits(&self) -> ViewLimits {
        *self.view.read()
    }

    /// The limits in force.
    pub fn limits(&self) -> WsLimits {
        *self.limits.read()
    }

    /// The timing parameters in force.
    pub fn tuning(&self) -> WsTuning {
        *self.tuning.read()
    }

    /// Changes the timing parameters (tests; sockets opened afterwards use them).
    pub fn set_tuning(&self, tuning: WsTuning) {
        *self.tuning.write() = tuning;
    }

    /// Installs the source of the status tick's `alerts` and `maintenance` (WP-ADMIN-25 / WP-AUTH-18).
    pub fn set_status_extras(&self, extras: StatusExtras) {
        *self.extras.write() = Some(extras);
    }

    /// Replaces the factory of map encoders (WP-ADMIN-07 installs its diffing encoder here at start; tests install
    /// recorders). Subscriptions opened afterwards use it.
    pub fn set_map_encoder(&self, factory: EncoderFactory) {
        *self.encoder.write() = factory;
    }

    /// The server stops: every open socket gets `SessionEnded{SHUTDOWN}` and is closed with 1001 (the owner on
    /// loopback has no session that `AdminSessions::end_all` could end).
    pub fn stop(&self) {
        let _ = self.stop.send(true);
    }

    fn new_encoder(&self, sub: u32) -> Box<dyn MapEncoder> {
        (self.encoder.read())(sub)
    }

    fn status_extras(&self) -> (u32, bool) {
        self.extras.read().as_ref().map_or((0, false), |f| f())
    }

    /// Open sockets of the identity with this limit key.
    pub fn open_sockets(&self, key: &str) -> u32 {
        self.open.lock().get(key).copied().unwrap_or(0)
    }

    /// Map subscriptions open on all sockets.
    pub fn map_subscriptions(&self) -> u32 {
        self.map_subs.load(Ordering::Relaxed)
    }

    fn alloc_conn(&self) -> AdminConnId {
        AdminConnId(self.next_conn.fetch_add(1, Ordering::Relaxed))
    }

    /// A socket slot for `key`, or `None` at `admin.max_ws_per_identity`.
    pub fn try_open(self: &Arc<Self>, key: &str) -> Option<SocketPermit> {
        let max = self.limits().max_ws_per_identity;
        let mut open = self.open.lock();
        let n = open.entry(key.to_owned()).or_insert(0);
        if *n >= max {
            return None;
        }
        *n += 1;
        Some(SocketPermit {
            hub: Arc::clone(self),
            key: key.to_owned(),
        })
    }

    /// A map-subscription slot, or `None` at `admin.max_map_subs_total`.
    pub fn try_map(self: &Arc<Self>) -> Option<MapPermit> {
        let max = self.limits().max_map_subs_total;
        self.map_subs
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
                (v < max).then_some(v + 1)
            })
            .ok()
            .map(|_| MapPermit {
                hub: Arc::clone(self),
            })
    }
}

// ---- the environment and the route ------------------------------------------------------------------------------------

/// Everything a socket needs from the server.
#[derive(Clone)]
pub struct WsEnv {
    /// Sessions, capabilities, the hub, the counters.
    pub shared: Arc<AdminShared>,
    /// C1: the game thread's inbox.
    pub game_tx: flume::Sender<GameInput>,
    /// The health state of the process.
    pub telemetry: Arc<Telemetry>,
    /// The audit log: private chat subscriptions write `gm.snoop` (25 §4.9). `None` writes nothing (tests).
    pub audit: Option<Arc<AuditFile>>,
}

impl WsEnv {
    /// The environment of the admin API state.
    pub fn of(st: &ApiState) -> WsEnv {
        WsEnv {
            shared: Arc::clone(&st.admin),
            game_tx: st.ops.game_tx.clone(),
            telemetry: Arc::clone(&st.ops.telemetry),
            audit: Some(Arc::clone(&st.ops.audit)),
        }
    }
}

/// Mounts `GET /admin/ws` (catalogued as `Authenticated`: the capability of each subscription is checked at subscribe).
pub fn mount(r: Routes<ApiState>) -> Routes<ApiState> {
    r.get("/admin/ws", route)
}

async fn route(
    State(st): State<ApiState>,
    Who(identity): Who,
    upgrade: WebSocketUpgrade,
) -> Response {
    accept(WsEnv::of(&st), identity, upgrade)
}

/// The route over a bare [`WsEnv`] (tests mount it behind the real authentication layer).
pub fn router(env: WsEnv) -> Router {
    async fn test_route(
        State(env): State<WsEnv>,
        Who(identity): Who,
        upgrade: WebSocketUpgrade,
    ) -> Response {
        accept(env, identity, upgrade)
    }
    Router::new()
        .route("/admin/ws", get(test_route))
        .with_state(env)
}

/// The checks before the upgrade and the upgrade itself.
pub fn accept(env: WsEnv, identity: Identity, upgrade: WebSocketUpgrade) -> Response {
    if !matches!(env.telemetry.health.state(), HealthState::Running) {
        return ApiError::unavailable("the server is not serving").into_response();
    }
    let hub = &env.shared.ws;
    let Some(permit) = hub.try_open(&identity.limit_key()) else {
        return ApiError::rate_limited(2, "too many open admin connections").into_response();
    };
    upgrade
        .max_message_size(codec::MAX_FRAME_BYTES)
        .max_frame_size(codec::MAX_FRAME_BYTES)
        .on_upgrade(move |socket| serve(env, identity, socket, permit))
}

// ---- the sink: what the game thread talks to ------------------------------------------------------------------------

#[derive(Default)]
struct QueueState {
    items: VecDeque<(u32, AdminEvent)>,
    /// Subscriptions that lost deltas and accept nothing but a full state.
    awaiting_full: BTreeSet<u32>,
}

/// The state shared between the connection task and its sink.
struct ConnShared {
    notify: Notify,
    slots: Mutex<BTreeMap<(u32, SlotKey), AdminEvent>>,
    queue: Mutex<QueueState>,
    /// Subscriptions that lost queue items: the connection sends them `Resync`.
    resync: Mutex<BTreeSet<u32>>,
    closed: AtomicBool,
    queue_cap: usize,
    hub: Arc<WsHub>,
}

/// The game thread's side of one admin socket (see the module documentation).
pub struct ConnSink(Arc<ConnShared>);

impl AdminSink for ConnSink {
    fn push(&self, sub: u32, event: AdminEvent) -> Pushed {
        let s = &self.0;
        if s.closed.load(Ordering::Relaxed) {
            return Pushed::Closed;
        }
        match event.delivery() {
            Delivery::Latest(key) => {
                let is_map = matches!(event, AdminEvent::Map(_));
                let replaced = s.slots.lock().insert((sub, key), event).is_some();
                if replaced && is_map {
                    s.hub
                        .stats
                        .map_frames_replaced
                        .fetch_add(1, Ordering::Relaxed);
                }
                s.notify.notify_one();
                Pushed::Accepted
            }
            Delivery::Queued => {
                let mut q = s.queue.lock();
                if q.awaiting_full.contains(&sub) && event.needs_restart_after_loss() {
                    if !event.is_full_state() {
                        return Pushed::Resync;
                    }
                    q.awaiting_full.remove(&sub);
                }
                if q.items.len() >= s.queue_cap {
                    // overflow: drop the queue, tell every subscription that lost something, restart the streams
                    let mut lost: BTreeSet<u32> = q.items.iter().map(|(sub, _)| *sub).collect();
                    lost.insert(sub);
                    let dropped: Vec<(u32, AdminEvent)> = q.items.drain(..).collect();
                    for (dropped_sub, ev) in &dropped {
                        if ev.needs_restart_after_loss() {
                            q.awaiting_full.insert(*dropped_sub);
                        }
                    }
                    let restart = event.needs_restart_after_loss();
                    if restart {
                        // a delta without its baseline is useless: the stream restarts with a full state
                        q.awaiting_full.insert(sub);
                    } else {
                        q.items.push_back((sub, event));
                    }
                    drop(q);
                    s.resync.lock().extend(lost);
                    s.hub.stats.queue_overflows.fetch_add(1, Ordering::Relaxed);
                    s.notify.notify_one();
                    return if restart {
                        Pushed::Resync
                    } else {
                        Pushed::Accepted
                    };
                }
                q.items.push_back((sub, event));
                drop(q);
                s.notify.notify_one();
                Pushed::Accepted
            }
        }
    }

    fn is_closed(&self) -> bool {
        self.0.closed.load(Ordering::Relaxed)
    }
}

impl ConnShared {
    fn new(hub: Arc<WsHub>, queue_cap: usize) -> Arc<ConnShared> {
        Arc::new(ConnShared {
            notify: Notify::new(),
            slots: Mutex::new(BTreeMap::new()),
            queue: Mutex::new(QueueState::default()),
            resync: Mutex::new(BTreeSet::new()),
            closed: AtomicBool::new(false),
            queue_cap,
            hub,
        })
    }

    fn pop_queue(&self) -> Option<(u32, AdminEvent)> {
        self.queue.lock().items.pop_front()
    }

    fn take_resync(&self) -> BTreeSet<u32> {
        std::mem::take(&mut *self.resync.lock())
    }

    fn take_slots(&self) -> BTreeMap<(u32, SlotKey), AdminEvent> {
        std::mem::take(&mut *self.slots.lock())
    }

    /// Puts events back that were not sent; a newer event in the slot wins.
    fn put_back(&self, events: Vec<((u32, SlotKey), AdminEvent)>) {
        let mut slots = self.slots.lock();
        for (key, ev) in events {
            slots.entry(key).or_insert(ev);
        }
    }

    /// Forgets everything pending for a subscription that ended.
    fn purge(&self, sub: u32) {
        self.slots.lock().retain(|(s, _), _| *s != sub);
        let mut q = self.queue.lock();
        q.items.retain(|(s, _)| *s != sub);
        q.awaiting_full.remove(&sub);
        drop(q);
        self.resync.lock().remove(&sub);
    }
}

// ---- conversions: game events to wire messages ---------------------------------------------------------------------------

fn msg(m: pb::admin_server_msg::Msg) -> pb::AdminServerMsg {
    pb::AdminServerMsg { msg: Some(m) }
}

fn sub_error_code(c: SubErrorCode) -> i32 {
    match c {
        SubErrorCode::Forbidden => pb::SubscriptionErrorCode::Forbidden as i32,
        SubErrorCode::Limit => pb::SubscriptionErrorCode::Limit as i32,
        SubErrorCode::BadSpec => pb::SubscriptionErrorCode::BadSpec as i32,
        SubErrorCode::NotFound => pb::SubscriptionErrorCode::NotFound as i32,
    }
}

fn error_msg(sub: u32, code: SubErrorCode, message: impl Into<String>) -> pb::AdminServerMsg {
    msg(pb::admin_server_msg::Msg::SubscriptionError(
        pb::SubscriptionError {
            sub_id: sub,
            code: sub_error_code(code),
            message: message.into(),
        },
    ))
}

fn entity_msg(sub: u32, d: &EntityDetail) -> pb::EntityDetail {
    pb::EntityDetail {
        sub_id: sub,
        obj_id: d.obj_id,
        kind: ws_map::kind_number(d.kind),
        prop: d.prop,
        name: d.name.clone(),
        world: d.world,
        layer: d.layer,
        x: d.x,
        y: d.y,
        z: d.z,
        level: d.level,
        hp: d.hp,
        hp_max: d.hp_max,
        mp: d.mp,
        mp_max: d.mp_max,
        fp: d.fp,
        fp_max: d.fp_max,
        flags: d.flags,
        ai_state: d.ai_state.clone(),
        target: d.target,
        hate: d
            .hate
            .iter()
            .map(|h| pb::HateEntry {
                obj_id: h.obj_id,
                name: h.name.clone(),
                hate: h.hate,
            })
            .collect(),
        spawn_region: d.spawn_region,
        respawn_tick: d.respawn_tick,
        character_id: u64::from(d.character_id),
        owner: d.owner,
        mode_flags: d.mode_flags,
        closed: d.closed,
    }
}

fn roster_msg(sub: u32, d: &RosterDelta) -> pb::RosterDelta {
    pb::RosterDelta {
        sub_id: sub,
        full: d.full,
        upserts: d
            .upserts
            .iter()
            .map(|e| pb::RosterEntry {
                character_id: u64::from(e.character_id),
                obj_id: e.obj_id,
                name: e.name.clone(),
                level: e.level,
                job: e.job,
                world: e.world,
                layer: e.layer,
                x: e.x,
                z: e.z,
                guild_id: e.guild_id,
                authority: e.authority,
            })
            .collect(),
        removes: d.removes.iter().map(|id| u64::from(*id)).collect(),
        lobby: d.lobby,
        in_world: d.in_world,
    }
}

fn layers_msg(sub: u32, layers: &[LayerInfo]) -> pb::LayerList {
    pb::LayerList {
        sub_id: sub,
        layers: layers
            .iter()
            .map(|l| pb::LayerInfo {
                world: l.world,
                layer: l.layer,
                kind: l.kind.clone(),
                owner: l.owner.clone(),
                members: l.members,
                monsters: l.monsters,
                created_tick: l.created_tick,
                seconds_left: l.seconds_left,
            })
            .collect(),
    }
}

fn status_msg(sub: u32, t: &StatusTick, extras: (u32, bool)) -> pb::StatusTick {
    pb::StatusTick {
        sub_id: sub,
        in_world: t.in_world,
        lobby: t.lobby,
        tick_p50_us: t.tick_p50_us,
        tick_p99_us: t.tick_p99_us,
        db_degraded: t.db_degraded,
        db_queue: t.db_queue,
        alerts: t.alerts.max(extras.0),
        maintenance: t.maintenance || extras.1,
        shutdown_in_s: t.shutdown_in_s,
    }
}

fn chat_channel(c: ChatChannel) -> i32 {
    match c {
        ChatChannel::Say => pb::ChatChannel::Say as i32,
        ChatChannel::Shout => pb::ChatChannel::Shout as i32,
        ChatChannel::Party => pb::ChatChannel::Party as i32,
        ChatChannel::Guild => pb::ChatChannel::Guild as i32,
        ChatChannel::Whisper => pb::ChatChannel::Whisper as i32,
        ChatChannel::Messenger => pb::ChatChannel::Messenger as i32,
        ChatChannel::System => pb::ChatChannel::System as i32,
    }
}

/// `true` for the channels only `view.chat.private` may see (party, guild, whisper and messenger; 25 §4.9).
fn private_channel(c: ChatChannel) -> bool {
    matches!(
        c,
        ChatChannel::Party | ChatChannel::Guild | ChatChannel::Whisper | ChatChannel::Messenger
    )
}

/// The channel bits only `view.chat.private` may see.
const PRIVATE_BITS: u8 = ChatChannel::Party.bit()
    | ChatChannel::Guild.bit()
    | ChatChannel::Whisper.bit()
    | ChatChannel::Messenger.bit();

/// The names of the channels in a filter's bitset (the `gm.snoop` record).
fn channel_names(bits: u8) -> Vec<&'static str> {
    [
        (ChatChannel::Say, "say"),
        (ChatChannel::Shout, "shout"),
        (ChatChannel::Party, "party"),
        (ChatChannel::Guild, "guild"),
        (ChatChannel::Whisper, "whisper"),
        (ChatChannel::Messenger, "messenger"),
        (ChatChannel::System, "system"),
    ]
    .into_iter()
    .filter(|(c, _)| bits & c.bit() != 0)
    .map(|(_, n)| n)
    .collect()
}

/// The game's channel of a protocol channel number.
fn channel_of(v: i32) -> Option<ChatChannel> {
    use pb::ChatChannel as P;
    Some(match P::try_from(v).ok()? {
        P::Say => ChatChannel::Say,
        P::Shout => ChatChannel::Shout,
        P::Party => ChatChannel::Party,
        P::Guild => ChatChannel::Guild,
        P::Whisper => ChatChannel::Whisper,
        P::Messenger => ChatChannel::Messenger,
        P::System => ChatChannel::System,
        P::ChatUnspecified => return None,
    })
}

fn end_reason(r: EndReason) -> pb::SessionEndReason {
    match r {
        EndReason::Logout | EndReason::Revoked => pb::SessionEndReason::Logout,
        EndReason::Banned => pb::SessionEndReason::Ban,
        EndReason::Authority => pb::SessionEndReason::Authority,
        EndReason::Password => pb::SessionEndReason::Password,
        EndReason::Shutdown => pb::SessionEndReason::Shutdown,
        EndReason::Expired => pb::SessionEndReason::Expired,
    }
}

// ---- the connection --------------------------------------------------------------------------------------------------

/// Why the connection ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CloseReq {
    code: u16,
    reason: &'static str,
}

const fn close(code: u16, reason: &'static str) -> CloseReq {
    CloseReq { code, reason }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SubKind {
    Map,
    Entity,
    Character,
    Chat,
    Roster,
    Layers,
    Status,
    /// The server log: served from the ring buffer of `admin::logs`, not by the game thread (WP-ADMIN-26).
    Log,
}

/// Log lines a new log subscription starts with (the tail of the ring).
const LOG_TAIL: usize = 200;
/// Log subscriptions one socket may hold.
const MAX_LOG_SUBS: usize = 2;

/// The cursor of one log subscription.
struct LogSub {
    filter: LogFilter,
    /// The sequence number of the next line to send.
    next: u64,
}

struct MapState {
    encoder: Box<dyn MapEncoder>,
    last_key: Option<Instant>,
    /// The tier (metrics index) of the last frame; counted in `flyff_admin_map_subscriptions{tier}`.
    tier: Option<usize>,
    _permit: MapPermit,
}

struct SubState {
    kind: SubKind,
    map: Option<MapState>,
    log: Option<LogSub>,
    /// A chat subscription that includes private channels: when the next `gm.snoop` record is due and the filter
    /// it records (25 §4.9).
    snoop: Option<Snoop>,
}

/// The `gm.snoop` bookkeeping of one private chat subscription.
struct Snoop {
    next: Instant,
    filter: ChatFilter,
}

enum Out {
    Data(Bytes),
    Close(CloseReq),
}

struct Conn {
    id: AdminConnId,
    env: WsEnv,
    identity: Identity,
    shared: Arc<ConnShared>,
    tuning: WsTuning,
    subs: BTreeMap<u32, SubState>,
    outbox: VecDeque<Bytes>,
    out_tx: mpsc::Sender<Out>,
    pending: Arc<AtomicUsize>,
    last_rx: Instant,
    window: (Instant, u32),
    /// The `WsHub` generation of the game-side limits this connection last sent.
    view_gen: u64,
}

impl Conn {
    fn caps(&self) -> &super::identity::CapTable {
        &self.env.shared.caps
    }

    fn has(&self, cap: Cap) -> bool {
        self.identity.has(self.caps(), cap)
    }

    fn congested(&self) -> bool {
        self.pending.load(Ordering::Relaxed) > self.tuning.send_cap
            || self.outbox.len() >= OUT_CHANNEL
    }

    /// Encodes one message and queues it for the writer.
    fn queue_msg(&mut self, m: &pb::AdminServerMsg) -> usize {
        let bytes = Bytes::from(codec::encode_server(m));
        let n = bytes.len();
        let now = self.pending.fetch_add(n, Ordering::Relaxed) + n;
        self.env
            .shared
            .ws
            .stats
            .pending_high_water
            .fetch_max(now, Ordering::Relaxed);
        self.outbox.push_back(bytes);
        n
    }

    /// Moves queued messages into the writer channel while it has room. `Err` once the writer is gone.
    fn flush_outbox(&mut self) -> Result<(), CloseReq> {
        while let Some(bytes) = self.outbox.pop_front() {
            match self.out_tx.try_send(Out::Data(bytes)) {
                Ok(()) => {}
                Err(mpsc::error::TrySendError::Full(Out::Data(b))) => {
                    self.outbox.push_front(b);
                    break;
                }
                Err(mpsc::error::TrySendError::Full(_)) => break,
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    return Err(close(CLOSE_GOING_AWAY, "writer gone"));
                }
            }
        }
        if self.outbox.len() > OUTBOX_MAX {
            return Err(close(CLOSE_POLICY, "the client does not read"));
        }
        Ok(())
    }

    fn error(&mut self, sub: u32, code: SubErrorCode, message: impl Into<String>) {
        let m = error_msg(sub, code, message);
        self.queue_msg(&m);
    }

    /// Sends an operation to the game thread (bounded wait; `Err` when it is gone).
    async fn game(&mut self, op: AdminViewOp) -> Result<(), CloseReq> {
        let input = GameInput::AdminView { conn: self.id, op };
        match tokio::time::timeout(GAME_SEND_TIMEOUT, self.env.game_tx.send_async(input)).await {
            Ok(Ok(())) => Ok(()),
            _ => Err(close(CLOSE_INTERNAL, "game thread unavailable")),
        }
    }

    /// The inbound rate limit: more than `inbound_per_s` messages in a second closes the socket.
    fn rate_ok(&mut self) -> bool {
        let now = Instant::now();
        if now.duration_since(self.window.0) >= Duration::from_secs(1) {
            self.window = (now, 0);
        }
        self.window.1 += 1;
        self.window.1 <= self.tuning.inbound_per_s
    }

    async fn on_frame(&mut self, bytes: &[u8]) -> Result<(), CloseReq> {
        self.last_rx = Instant::now();
        if !self.rate_ok() {
            return Err(close(CLOSE_POLICY, "too many messages"));
        }
        let m = codec::decode_client(bytes).map_err(|_| close(CLOSE_PROTOCOL, "bad frame"))?;
        let Some(m) = m.msg else { return Ok(()) };
        self.on_client(m).await
    }

    async fn on_client(&mut self, m: pb::admin_client_msg::Msg) -> Result<(), CloseReq> {
        use pb::admin_client_msg::Msg as C;
        match m {
            C::Ping(p) => {
                self.queue_msg(&msg(pb::admin_server_msg::Msg::Pong(pb::Pong { t: p.t })));
                Ok(())
            }
            C::MapSubscribe(s) => self.map_subscribe(s).await,
            C::MapUpdate(u) => self.map_update(u).await,
            C::Unsubscribe(u) => self.unsubscribe(u.sub_id).await,
            C::EntityWatch(w) => {
                if !self.allow(w.sub_id, Cap::ViewMap) {
                    return Ok(());
                }
                if w.obj_id == 0 {
                    self.error(w.sub_id, SubErrorCode::BadSpec, "obj_id 0 is not an object");
                    return Ok(());
                }
                self.view(w.sub_id, SubKind::Entity, ViewSpec::Entity(ObjId(w.obj_id)))
                    .await
            }
            C::CharacterWatch(w) => {
                if !self.allow(w.sub_id, Cap::ViewPlayers) {
                    return Ok(());
                }
                let Ok(id) = u32::try_from(w.character_id) else {
                    self.error(w.sub_id, SubErrorCode::BadSpec, "no such character id");
                    return Ok(());
                };
                self.view(
                    w.sub_id,
                    SubKind::Character,
                    ViewSpec::Character(CharacterId(id)),
                )
                .await
            }
            C::ChatSubscribe(c) => self.chat_subscribe(c).await,
            C::RosterSubscribe(r) => {
                if !self.allow(r.sub_id, Cap::ViewPlayers) {
                    return Ok(());
                }
                self.view(r.sub_id, SubKind::Roster, ViewSpec::Roster).await
            }
            C::LayersSubscribe(l) => {
                if !self.allow(l.sub_id, Cap::ViewMap) {
                    return Ok(());
                }
                let world = (l.world != 0).then_some(WorldId(l.world));
                self.view(l.sub_id, SubKind::Layers, ViewSpec::Layers(world))
                    .await
            }
            C::LogSubscribe(l) => {
                self.log_subscribe(&l);
                Ok(())
            }
            C::StatusSubscribe(s) => {
                if !self.allow(s.sub_id, Cap::ViewDashboard) {
                    return Ok(());
                }
                self.view(s.sub_id, SubKind::Status, ViewSpec::Status).await
            }
        }
    }

    /// `LogSubscribe` (`act.server`): the server log from the ring buffer of `admin::logs` (25 §9.8): the last
    /// [`LOG_TAIL`] lines that match, then every new one. The stream is the server's own, not a game view.
    fn log_subscribe(&mut self, l: &pb::LogSubscribe) {
        let sub = l.sub_id;
        if !self.allow(sub, Cap::ActServer) {
            return;
        }
        // 1 error .. 5 trace; 0 (unset) means info
        let max_level = if l.min_level == 0 { 3 } else { l.min_level };
        if max_level > 5 {
            self.error(
                sub,
                SubErrorCode::BadSpec,
                "min_level is 1 (error) to 5 (trace)",
            );
            return;
        }
        if l.target_prefix.len() > 128 {
            self.error(sub, SubErrorCode::BadSpec, "the target prefix is too long");
            return;
        }
        match self.subs.get(&sub) {
            Some(s) if s.kind != SubKind::Log => {
                self.error(
                    sub,
                    SubErrorCode::BadSpec,
                    "the sub_id belongs to a subscription of another kind",
                );
                return;
            }
            Some(_) => {}
            None => {
                let mine = self
                    .subs
                    .values()
                    .filter(|s| s.kind == SubKind::Log)
                    .count();
                if self.subs.len() >= MAX_SUBS_PER_CONN || mine >= MAX_LOG_SUBS {
                    self.error(
                        sub,
                        SubErrorCode::Limit,
                        format!("at most {MAX_LOG_SUBS} log subscriptions per connection"),
                    );
                    return;
                }
            }
        }
        let filter = LogFilter {
            max_level: u8::try_from(max_level).unwrap_or(5),
            target_prefix: l.target_prefix.clone(),
        };
        let next = self.env.shared.logs.tail_start(&filter, LOG_TAIL);
        // a changed filter restarts the stream: the client drops what it shows
        self.shared.purge(sub);
        self.subs.insert(
            sub,
            SubState {
                kind: SubKind::Log,
                map: None,
                log: Some(LogSub { filter, next }),
                snoop: None,
            },
        );
    }

    /// Sends the log lines that arrived since the last call to every log subscription, as far as the socket keeps
    /// up. A subscription that fell behind the ring (lines overwritten) gets `Resync` and goes on with what is left.
    fn pump_logs(&mut self) -> Result<(), CloseReq> {
        if !self.subs.values().any(|s| s.log.is_some()) {
            return Ok(());
        }
        let ring = Arc::clone(&self.env.shared.logs);
        let ids: Vec<u32> = self
            .subs
            .iter()
            .filter(|(_, s)| s.log.is_some())
            .map(|(id, _)| *id)
            .collect();
        for sub in ids {
            // up to 2,000 lines per subscription and call: a burst is worked off in a few calls, one subscription
            // cannot starve the rest of the socket
            for _round in 0..10 {
                if self.congested() {
                    return Ok(());
                }
                let Some((filter, next)) = self
                    .subs
                    .get(&sub)
                    .and_then(|s| s.log.as_ref())
                    .map(|l| (l.filter.clone(), l.next))
                else {
                    break;
                };
                let batch = ring.since(next, &filter, logs::BATCH);
                if batch.lost {
                    self.queue_msg(&msg(pb::admin_server_msg::Msg::Resync(pb::Resync {
                        sub_id: sub,
                    })));
                }
                for line in &batch.lines {
                    self.queue_msg(&msg(pb::admin_server_msg::Msg::LogLine(pb::LogLine {
                        sub_id: sub,
                        ts_ms: u64::try_from(line.ts_ms).unwrap_or(0),
                        level: u32::from(line.level),
                        target: line.target.clone(),
                        text: line.text.clone(),
                    })));
                }
                if let Some(l) = self.subs.get_mut(&sub).and_then(|s| s.log.as_mut()) {
                    l.next = batch.next;
                }
                self.flush_outbox()?;
                if batch.lines.len() < logs::BATCH {
                    break; // caught up
                }
            }
        }
        Ok(())
    }

    /// The capability check of a subscription: a refusal is a `SubscriptionError{FORBIDDEN}`.
    fn allow(&mut self, sub: u32, cap: Cap) -> bool {
        if self.has(cap) {
            return true;
        }
        self.error(
            sub,
            SubErrorCode::Forbidden,
            format!("the capability {} is required", cap.name()),
        );
        false
    }

    /// Opens (or restarts) a subscription of a kind the game implements.
    async fn view(&mut self, sub: u32, kind: SubKind, spec: ViewSpec) -> Result<(), CloseReq> {
        let op = match self.subs.get(&sub) {
            Some(s) if s.kind == kind => AdminViewOp::Update { sub, spec },
            Some(_) => {
                self.error(
                    sub,
                    SubErrorCode::BadSpec,
                    "the sub_id belongs to a subscription of another kind",
                );
                return Ok(());
            }
            None => {
                if self.subs.len() >= MAX_SUBS_PER_CONN {
                    self.error(
                        sub,
                        SubErrorCode::Limit,
                        format!("at most {MAX_SUBS_PER_CONN} subscriptions per connection"),
                    );
                    return Ok(());
                }
                self.subs.insert(
                    sub,
                    SubState {
                        kind,
                        map: None,
                        log: None,
                        snoop: None,
                    },
                );
                AdminViewOp::Open { sub, spec }
            }
        };
        self.game(op).await
    }

    async fn chat_subscribe(&mut self, c: pb::ChatSubscribe) -> Result<(), CloseReq> {
        let sub = c.sub_id;
        if !self.allow(sub, Cap::ViewChat) {
            return Ok(());
        }
        let private = self.has(Cap::ViewChatPrivate);
        let mut channels = 0_u8;
        for raw in &c.channels {
            let Some(ch) = channel_of(*raw) else {
                self.error(sub, SubErrorCode::BadSpec, "unknown chat channel");
                return Ok(());
            };
            if private_channel(ch) && !private {
                self.error(
                    sub,
                    SubErrorCode::Forbidden,
                    format!(
                        "the capability {} is required for party, guild, whisper and messenger chat",
                        Cap::ViewChatPrivate.name()
                    ),
                );
                return Ok(());
            }
            channels |= ch.bit();
        }
        if channels == 0 {
            // no channel named: everything this identity may see
            channels =
                ChatChannel::Say.bit() | ChatChannel::Shout.bit() | ChatChannel::System.bit();
            if private {
                for ch in [
                    ChatChannel::Party,
                    ChatChannel::Guild,
                    ChatChannel::Whisper,
                    ChatChannel::Messenger,
                ] {
                    channels |= ch.bit();
                }
            }
        }
        let character = u32::try_from(c.character_id)
            .ok()
            .filter(|id| *id != 0)
            .map(CharacterId);
        if c.character_id != 0 && character.is_none() {
            self.error(sub, SubErrorCode::BadSpec, "no such character id");
            return Ok(());
        }
        if c.text.len() > 64 {
            self.error(sub, SubErrorCode::BadSpec, "the text filter is too long");
            return Ok(());
        }
        let filter = ChatFilter {
            channels,
            world: (c.world != 0).then_some(WorldId(c.world)),
            character,
            text: c.text,
        };
        // 25 §4.9: reading private channels is audited (`gm.snoop`), once now and every `snoop_every` while the
        // subscription is open; a record the log cannot acknowledge refuses the subscription (`audit.admin_required`)
        let private = filter.channels & PRIVATE_BITS != 0;
        if private && !self.snoop_record(sub, "subscribe", &filter) {
            self.error(
                sub,
                SubErrorCode::Forbidden,
                "private chat is audited and the audit record could not be written",
            );
            return Ok(());
        }
        let watched = filter.clone();
        let was_private = self
            .subs
            .get(&sub)
            .and_then(|s| s.snoop.as_ref())
            .map(|k| k.filter.clone());
        self.view(sub, SubKind::Chat, ViewSpec::Chat(filter))
            .await?;
        if let (false, Some(old)) = (private, was_private) {
            // a restart with public channels only ends the private reading
            let _ = self.snoop_record(sub, "end", &old);
        }
        let every = self.tuning.snoop_every;
        if let Some(state) = self.subs.get_mut(&sub).filter(|s| s.kind == SubKind::Chat) {
            state.snoop = private.then(|| Snoop {
                next: Instant::now() + every,
                filter: watched,
            });
        }
        Ok(())
    }

    /// Writes one `gm.snoop` record for a private chat subscription (`state`: `subscribe`, `renew`, `end`). `false`
    /// when the record was required and could not be acknowledged. Without an audit log (tests) nothing is written.
    fn snoop_record(&self, sub: u32, state: &str, filter: &ChatFilter) -> bool {
        let Some(audit) = &self.env.audit else {
            return true;
        };
        let data = serde_json::json!({
            "sub": sub,
            "via": "admin_console",
            "channels": channel_names(filter.channels),
            "world": filter.world.map_or(0, |w| w.0),
            "character": filter.character.map_or(0, |c| c.0),
            "text": filter.text,
        });
        audit_admin(audit, "gm.snoop", &self.identity, state, None, data).is_ok()
    }

    /// The renewal of the `gm.snoop` records of the open private chat subscriptions. A subscription whose record
    /// cannot be written any more is closed: the console must not go on reading unaudited.
    async fn snoop_renewals(&mut self) -> Result<(), CloseReq> {
        let now = Instant::now();
        let due: Vec<(u32, ChatFilter)> = self
            .subs
            .iter()
            .filter_map(|(id, s)| {
                s.snoop
                    .as_ref()
                    .filter(|k| k.next <= now)
                    .map(|k| (*id, k.filter.clone()))
            })
            .collect();
        let every = self.tuning.snoop_every;
        for (sub, filter) in due {
            if self.snoop_record(sub, "renew", &filter) {
                if let Some(k) = self.subs.get_mut(&sub).and_then(|s| s.snoop.as_mut()) {
                    k.next = now + every;
                }
            } else {
                self.error(
                    sub,
                    SubErrorCode::Forbidden,
                    "private chat is audited and the audit record could not be written: the subscription was closed",
                );
                self.unsubscribe(sub).await?;
            }
        }
        Ok(())
    }

    fn map_spec(
        &mut self,
        sub: u32,
        rect: Option<&pb::Rect>,
        kinds: u32,
        max_tier: i32,
        world: u32,
        layer: i32,
    ) -> Option<MapSpec> {
        let Some(r) = rect else {
            self.error(sub, SubErrorCode::BadSpec, "the rectangle is missing");
            return None;
        };
        let finite = [r.x0, r.z0, r.x1, r.z1].iter().all(|v| v.is_finite());
        if !finite
            || r.x1 <= r.x0
            || r.z1 <= r.z0
            || r.x1 - r.x0 > MAX_RECT_M
            || r.z1 - r.z0 > MAX_RECT_M
        {
            self.error(sub, SubErrorCode::BadSpec, "the rectangle is not valid");
            return None;
        }
        if kinds & !KINDS_MASK != 0 || kinds & KINDS_MASK == 0 {
            self.error(sub, SubErrorCode::BadSpec, "the entity kinds are not valid");
            return None;
        }
        let tier = match pb::Tier::try_from(max_tier) {
            Ok(pb::Tier::Unspecified | pb::Tier::A) => Tier::A,
            Ok(pb::Tier::B) => Tier::B,
            Ok(pb::Tier::C) => Tier::C,
            Err(_) => {
                self.error(sub, SubErrorCode::BadSpec, "the tier is not valid");
                return None;
            }
        };
        if world == 0 {
            self.error(sub, SubErrorCode::BadSpec, "the world is missing");
            return None;
        }
        Some(MapSpec {
            world: WorldId(world),
            layer: LayerId(layer),
            rect: [r.x0, r.z0, r.x1, r.z1],
            kinds,
            max_tier: tier,
            // invisible GMs are shown to an authority of N or higher (25 §3.5)
            see_invisible: self.identity.authority >= Authority::GAMEMASTER3,
        })
    }

    async fn map_subscribe(&mut self, s: pb::MapSubscribe) -> Result<(), CloseReq> {
        let sub = s.sub_id;
        if !self.allow(sub, Cap::ViewMap) {
            return Ok(());
        }
        if self.subs.contains_key(&sub) {
            self.error(
                sub,
                SubErrorCode::BadSpec,
                "the sub_id is in use (use MapUpdate to change a map subscription)",
            );
            return Ok(());
        }
        let Some(spec) = self.map_spec(sub, s.rect.as_ref(), s.kinds, s.max_tier, s.world, s.layer)
        else {
            return Ok(());
        };
        let limits = self.env.shared.ws.limits();
        let mine = self
            .subs
            .values()
            .filter(|s| s.kind == SubKind::Map)
            .count();
        if self.subs.len() >= MAX_SUBS_PER_CONN
            || u32::try_from(mine).unwrap_or(u32::MAX) >= limits.max_map_subs_per_conn
        {
            self.error(
                sub,
                SubErrorCode::Limit,
                format!(
                    "at most {} map subscription(s) per connection",
                    limits.max_map_subs_per_conn
                ),
            );
            return Ok(());
        }
        let Some(permit) = self.env.shared.ws.try_map() else {
            self.error(
                sub,
                SubErrorCode::Limit,
                format!(
                    "at most {} map subscriptions on the server",
                    limits.max_map_subs_total
                ),
            );
            return Ok(());
        };
        self.subs.insert(
            sub,
            SubState {
                kind: SubKind::Map,
                map: Some(MapState {
                    encoder: self.env.shared.ws.new_encoder(sub),
                    last_key: None,
                    tier: None,
                    _permit: permit,
                }),
                log: None,
                snoop: None,
            },
        );
        self.game(AdminViewOp::Open {
            sub,
            spec: ViewSpec::Map(spec),
        })
        .await
    }

    async fn map_update(&mut self, u: pb::MapUpdate) -> Result<(), CloseReq> {
        let sub = u.sub_id;
        if !self.allow(sub, Cap::ViewMap) {
            return Ok(());
        }
        if self.subs.get(&sub).is_none_or(|s| s.kind != SubKind::Map) {
            self.error(sub, SubErrorCode::NotFound, "no such map subscription");
            return Ok(());
        }
        let Some(spec) = self.map_spec(sub, u.rect.as_ref(), u.kinds, u.max_tier, u.world, u.layer)
        else {
            return Ok(());
        };
        if let Some(m) = self.subs.get_mut(&sub).and_then(|s| s.map.as_mut()) {
            // the picture changes: the next frame is a keyframe
            m.encoder.reset();
            m.last_key = None;
        }
        self.game(AdminViewOp::Update {
            sub,
            spec: ViewSpec::Map(spec),
        })
        .await
    }

    async fn unsubscribe(&mut self, sub: u32) -> Result<(), CloseReq> {
        // a log subscription lives here, the game thread never heard of it
        let log = self.subs.get(&sub).is_some_and(|s| s.kind == SubKind::Log);
        if self.drop_sub(sub) && !log {
            self.game(AdminViewOp::Close { sub }).await?;
        }
        Ok(())
    }

    /// Forgets a subscription (counters, pending events); `true` when it existed.
    fn drop_sub(&mut self, sub: u32) -> bool {
        let Some(state) = self.subs.remove(&sub) else {
            return false;
        };
        if let Some(tier) = state.map.as_ref().and_then(|m| m.tier) {
            self.env.shared.metrics.map_unsubscribed(tier);
        }
        if let Some(k) = &state.snoop {
            // the end of the reading, for the audit trail (best effort)
            let _ = self.snoop_record(sub, "end", &k.filter);
        }
        self.shared.purge(sub);
        true
    }

    // ---- outbound ----------------------------------------------------------------------------------------------------

    /// Renders one event of the game into messages.
    fn render(&mut self, sub: u32, ev: AdminEvent) {
        use pb::admin_server_msg::Msg as S;
        match ev {
            AdminEvent::Map(frame) => self.render_map(sub, &frame),
            AdminEvent::Entity(d) => {
                let m = msg(S::EntityDetail(entity_msg(sub, &d)));
                self.queue_msg(&m);
            }
            AdminEvent::Roster(d) => {
                let m = msg(S::RosterDelta(roster_msg(sub, &d)));
                self.queue_msg(&m);
            }
            AdminEvent::Layers(l) => {
                let m = msg(S::LayerList(layers_msg(sub, &l)));
                self.queue_msg(&m);
            }
            AdminEvent::Status(t) => {
                let extras = self.env.shared.ws.status_extras();
                let m = msg(S::StatusTick(status_msg(sub, &t, extras)));
                self.queue_msg(&m);
            }
            AdminEvent::Error(e) => {
                let m = error_msg(sub, e.code, e.message);
                self.queue_msg(&m);
            }
            AdminEvent::Chat(line) => {
                // defence in depth: the private channels never reach an identity without the capability, and the
                // text before the filter changed it is for those sessions only (25 §4.9)
                let private = self.has(Cap::ViewChatPrivate);
                if private_channel(line.channel) && !private {
                    return;
                }
                let m = msg(S::ChatLine(pb::ChatLine {
                    sub_id: sub,
                    ts_ms: line.ts_ms,
                    channel: chat_channel(line.channel),
                    from_character_id: u64::from(line.from_character_id),
                    from: line.from,
                    to: line.to,
                    text: line.text,
                    original_text: if private {
                        line.original_text
                    } else {
                        String::new()
                    },
                    world: line.world,
                }));
                self.queue_msg(&m);
            }
            AdminEvent::Character(d) => {
                let m = msg(S::CharacterDetail(character_msg(sub, &d)));
                self.queue_msg(&m);
            }
            AdminEvent::Container(c) => {
                if !self.has(Cap::ViewInventory) {
                    return;
                }
                let m = msg(S::ContainerSnapshot(container_msg(sub, &c)));
                self.queue_msg(&m);
            }
            // a variant added later: drop it rather than guess a wire shape
            _ => {}
        }
    }

    fn render_map(&mut self, sub: u32, frame: &flyff_game::admin::map::MapFrame) {
        let keyframe_every = self.tuning.keyframe_every;
        let Some(state) = self.subs.get_mut(&sub).and_then(|s| s.map.as_mut()) else {
            return; // a late frame of a subscription that ended
        };
        let now = Instant::now();
        let due = state
            .last_key
            .is_none_or(|t| now.duration_since(t) >= keyframe_every);
        let encoded = state.encoder.encode(frame, EncodePlan { keyframe: due });
        if encoded.keyframe {
            state.last_key = Some(now);
        }
        let tier = ws_map::tier_index(frame.tier);
        let metrics = &self.env.shared.metrics;
        if state.tier != Some(tier) {
            if let Some(old) = state.tier {
                metrics.map_unsubscribed(old);
            }
            metrics.map_subscribed(tier);
            state.tier = Some(tier);
        }
        let mut bytes = 0_u64;
        for m in &encoded.msgs {
            bytes += self.queue_msg(m) as u64;
        }
        self.env.shared.metrics.map_frame(tier, bytes);
        self.env
            .shared
            .ws
            .stats
            .map_frames_sent
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Drains what the game left for this connection into the writer, as far as the socket keeps up.
    fn pump(&mut self) -> Result<(), CloseReq> {
        self.flush_outbox()?;
        for sub in self.shared.take_resync() {
            let m = msg(pb::admin_server_msg::Msg::Resync(pb::Resync {
                sub_id: sub,
            }));
            self.queue_msg(&m);
        }
        while !self.congested() {
            let Some((sub, ev)) = self.shared.pop_queue() else {
                break;
            };
            self.render(sub, ev);
            self.flush_outbox()?;
        }
        self.pump_logs()?;
        if self.congested() {
            return self.flush_outbox();
        }
        let batch = self.shared.take_slots();
        if batch.is_empty() {
            return Ok(());
        }
        let mut kept = Vec::new();
        for (key, ev) in batch {
            if self.congested() {
                kept.push((key, ev));
                continue;
            }
            self.render(key.0, ev);
            self.flush_outbox()?;
        }
        if !kept.is_empty() {
            self.shared.put_back(kept);
        }
        Ok(())
    }
}

fn character_msg(sub: u32, d: &flyff_game::admin::watch::CharacterDetail) -> pb::CharacterDetail {
    pb::CharacterDetail {
        sub_id: sub,
        character_id: u64::from(d.character_id),
        name: d.name.clone(),
        level: d.level,
        job: d.job,
        exp_permille: d.exp_permille,
        str: d.str,
        sta: d.sta,
        dex: d.dex,
        int: d.int,
        stat_points: d.stat_points,
        skill_points: d.skill_points,
        hp: d.hp,
        hp_max: d.hp_max,
        mp: d.mp,
        mp_max: d.mp_max,
        fp: d.fp,
        fp_max: d.fp_max,
        skills: d
            .skills
            .iter()
            .map(|s| pb::SkillLevel {
                skill: s.skill,
                level: s.level,
            })
            .collect(),
        buffs: d
            .buffs
            .iter()
            .map(|b| pb::BuffState {
                buff: b.buff,
                level: b.level,
                remaining_ms: b.remaining_ms,
            })
            .collect(),
        quests: d
            .quests
            .iter()
            .map(|q| pb::QuestState {
                quest: q.quest,
                state: q.state,
            })
            .collect(),
        mode_flags: d.mode_flags,
        mute_until: d.mute_until,
        world: d.world,
        layer: d.layer,
        x: d.x,
        z: d.z,
        guild_id: d.guild_id,
        party_id: d.party_id,
        online: d.online,
    }
}

/// The wire form of a container snapshot (also the `pb` of the REST container views, `dto_players.rs`).
pub(crate) fn container_msg(
    sub: u32,
    c: &flyff_game::admin::watch::ContainerSnapshot,
) -> pb::ContainerSnapshot {
    use flyff_game::admin::watch::ContainerKind as K;
    let kind = match c.container.kind {
        K::Equipment => pb::ContainerKind::Equipment,
        K::Inventory => pb::ContainerKind::Inventory,
        K::Pocket => pb::ContainerKind::Pocket,
        K::Bank => pb::ContainerKind::Bank,
        K::GuildBank => pb::ContainerKind::GuildBank,
        K::Mailbox => pb::ContainerKind::Mailbox,
    };
    pb::ContainerSnapshot {
        sub_id: sub,
        container: Some(pb::ContainerRef {
            kind: kind as i32,
            index: c.container.index,
            owner_character_id: u64::from(c.container.owner_character_id),
        }),
        rev: c.rev,
        capacity: c.capacity,
        items: c.items.clone(),
        gold: c.gold,
        live: c.live,
        saved_at: c.saved_at,
        live_via_character_id: u64::from(c.live_via_character_id),
    }
}

/// The task that owns the socket's write half: it sends what the connection hands over, one message at a time, and
/// wakes the connection after each completed send so it can encode the next one.
async fn writer(
    mut sink: SplitSink<WebSocket, Message>,
    mut rx: mpsc::Receiver<Out>,
    pending: Arc<AtomicUsize>,
    shared: Arc<ConnShared>,
) {
    while let Some(out) = rx.recv().await {
        match out {
            Out::Data(bytes) => {
                let n = bytes.len();
                if sink.send(Message::Binary(bytes)).await.is_err() {
                    break;
                }
                let _ = pending.try_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
                    Some(v.saturating_sub(n))
                });
                shared.notify.notify_one();
            }
            Out::Close(c) => {
                let _ = sink
                    .send(Message::Close(Some(CloseFrame {
                        code: c.code,
                        reason: c.reason.into(),
                    })))
                    .await;
                break;
            }
        }
    }
}

/// `Message` to the frames the connection cares about.
enum Inbound {
    Frame(Bytes),
    Activity,
    /// A text frame: the protocol is binary only.
    Text,
    Closed,
}

fn inbound(m: Option<Result<Message, axum::Error>>) -> Inbound {
    match m {
        Some(Ok(Message::Binary(b))) => Inbound::Frame(b),
        Some(Ok(Message::Ping(_) | Message::Pong(_))) => Inbound::Activity,
        Some(Ok(Message::Text(_))) => Inbound::Text,
        Some(Ok(Message::Close(_))) | Some(Err(_)) | None => Inbound::Closed,
    }
}

async fn serve(env: WsEnv, identity: Identity, socket: WebSocket, _permit: SocketPermit) {
    let hub = Arc::clone(&env.shared.ws);
    let tuning = hub.tuning();
    let id = hub.alloc_conn();
    hub.stats.opened.fetch_add(1, Ordering::Relaxed);
    env.shared.metrics.ws_opened();
    let shared = ConnShared::new(Arc::clone(&hub), tuning.queue_cap);
    let (sink_half, stream_half) = socket.split();
    let (out_tx, out_rx) = mpsc::channel(OUT_CHANNEL);
    let pending = Arc::new(AtomicUsize::new(0));
    let mut writer_task = tokio::spawn(writer(
        sink_half,
        out_rx,
        Arc::clone(&pending),
        Arc::clone(&shared),
    ));
    let mut conn = Conn {
        id,
        env: env.clone(),
        identity,
        shared: Arc::clone(&shared),
        tuning,
        subs: BTreeMap::new(),
        outbox: VecDeque::new(),
        out_tx,
        pending,
        last_rx: Instant::now(),
        window: (Instant::now(), 0),
        view_gen: 0,
    };
    let outcome = conn.run(stream_half, &mut writer_task).await;
    // end of the connection: no more events, no more subscriptions
    shared.closed.store(true, Ordering::Relaxed);
    if let Some(c) = outcome {
        let _ = conn.flush_outbox();
        let _ = tokio::time::timeout(Duration::from_secs(1), conn.out_tx.send(Out::Close(c))).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), &mut writer_task).await;
    }
    writer_task.abort();
    let subs: Vec<u32> = conn.subs.keys().copied().collect();
    for sub in subs {
        conn.drop_sub(sub);
    }
    let _ = tokio::time::timeout(
        Duration::from_secs(1),
        env.game_tx.send_async(GameInput::AdminView {
            conn: id,
            op: AdminViewOp::CloseConn,
        }),
    )
    .await;
    env.shared.metrics.ws_closed();
}

impl Conn {
    /// The connection loop. Returns the close to send, or `None` when the socket is already gone.
    async fn run(
        &mut self,
        mut stream: SplitStream<WebSocket>,
        writer_task: &mut tokio::task::JoinHandle<()>,
    ) -> Option<CloseReq> {
        let sink = Arc::new(ConnSink(Arc::clone(&self.shared)));
        if self.game(AdminViewOp::Attach { sink }).await.is_err() {
            return Some(close(CLOSE_INTERNAL, "game thread unavailable"));
        }
        if let Err(c) = self.sync_view_limits().await {
            return Some(c);
        }
        let mut ended = self.env.shared.sessions.subscribe_ended();
        let mut stop = self.env.shared.ws.stop.subscribe();
        let my_session = self.identity.session.as_ref().map(|s| s.id.clone());
        let mut tick = tokio::time::interval(self.tuning.housekeeping);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // log lines have no notifier: a subscription polls the ring buffer at 4 Hz
        let mut log_tick = tokio::time::interval(Duration::from_millis(250));
        log_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let shared = Arc::clone(&self.shared);
        loop {
            let step: Result<(), CloseReq> = tokio::select! {
                _ = &mut *writer_task => return None,
                m = stream.next() => match inbound(m) {
                    Inbound::Closed => return None,
                    Inbound::Activity => { self.last_rx = Instant::now(); Ok(()) }
                    Inbound::Frame(b) => self.on_frame(&b).await,
                    Inbound::Text => Err(close(CLOSE_PROTOCOL, "text frames are not part of the protocol")),
                },
                _ = shared.notify.notified() => Ok(()),
                ev = ended.recv() => self.on_session_event(ev, my_session.as_deref()),
                _ = stop.changed() => self.on_stop(),
                _ = tick.tick() => match self.housekeeping() {
                    Ok(()) => self.snoop_renewals().await,
                    err => err,
                },
                _ = log_tick.tick() => Ok(()),
            };
            let step = step.and_then(|()| self.pump());
            if let Err(c) = step {
                return Some(c);
            }
            if let Err(c) = self.sync_view_limits().await {
                return Some(c);
            }
        }
    }

    /// Sends the game-side limits (`admin.map_visit_budget`, the map tiers and cap) to the game thread when they
    /// changed since this connection last did (at attach and after a reload).
    async fn sync_view_limits(&mut self) -> Result<(), CloseReq> {
        let hub = Arc::clone(&self.env.shared.ws);
        let current = hub.view_gen.load(Ordering::Relaxed);
        if current == self.view_gen {
            return Ok(());
        }
        self.view_gen = current;
        self.game(AdminViewOp::Limits(hub.view_limits())).await
    }

    fn on_session_event(
        &mut self,
        ev: Result<SessionEnded, broadcast::error::RecvError>,
        my_session: Option<&str>,
    ) -> Result<(), CloseReq> {
        let reason = match ev {
            Ok(e) if my_session == Some(e.id.as_str()) => end_reason(e.reason),
            Ok(_) => return Ok(()),
            // events were lost: the session is gone when the table no longer knows it
            Err(broadcast::error::RecvError::Lagged(_)) => match my_session {
                Some(id) if !self.env.shared.sessions.is_live(id) => pb::SessionEndReason::Expired,
                _ => return Ok(()),
            },
            Err(broadcast::error::RecvError::Closed) => return Ok(()),
        };
        self.queue_msg(&msg(pb::admin_server_msg::Msg::SessionEnded(
            pb::SessionEnded {
                reason: reason as i32,
            },
        )));
        Err(close(CLOSE_SESSION_ENDED, "session ended"))
    }

    /// The server stops (see [`WsHub::stop`]).
    fn on_stop(&mut self) -> Result<(), CloseReq> {
        self.queue_msg(&msg(pb::admin_server_msg::Msg::SessionEnded(
            pb::SessionEnded {
                reason: pb::SessionEndReason::Shutdown as i32,
            },
        )));
        Err(close(CLOSE_GOING_AWAY, "server stopped"))
    }

    fn housekeeping(&mut self) -> Result<(), CloseReq> {
        if self.last_rx.elapsed() >= self.tuning.ping_timeout {
            return Err(close(CLOSE_IDLE, "no message within the ping timeout"));
        }
        // the game thread ended without the server's teardown having run yet
        if matches!(
            self.env.telemetry.health.state(),
            HealthState::Stopped | HealthState::Panicked
        ) {
            return self.on_stop();
        }
        Ok(())
    }
}
