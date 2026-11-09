//! The registry of admin subscriptions (Plan/25 §3.15, §11.5, ADM-D4/D5; WP-ADMIN-06): who watches what, how often,
//! and the deterministic visit budget they share.
//!
//! * `GameInput::AdminView{conn, op}` ([`AdminViewOp`]) creates, changes and closes subscriptions; it is **not
//!   recorded** (`GameInput::is_recorded`) and must not touch world state. Nothing here takes `&mut Game`: the
//!   handlers get `&Game` and the registry's own state, so the borrow checker keeps ADM-D4 honest. A subscription
//!   never registers as a grid viewer and never wakes a monster (15 §5.6): it reads members, cells and services.
//! * The game thread only **collects plain structs** ([`AdminEvent`]) and moves them into
//!   the connection's [`AdminSink`](super::events::AdminSink); encoding and sending run on the connection's task.
//! * `AdminViews::run_post` runs in the tick's `post` phase, after the flush. It walks the subscriptions
//!   **round-robin** from a cursor, serves those that are due, and charges every read to a per-tick [`VisitBudget`]
//!   (`admin.map_visit_budget` = 30,000 entity reads; a cell costs 1/16). When the budget is out the rest waits for
//!   the next tick, which starts at the next subscription in the ring, so a heavy subscription cannot starve the
//!   others. At least one due subscription is served per tick.
//!
//! | view | cadence | content |
//! |---|---|---|
//! | `Roster` | every 2 s (30 ticks), only when something changed | `RosterDelta` (first and after a `Resync`: `full`) |
//! | `Layers` | at most 1 Hz, on change | `LayerList` of the live layers other than 0 |
//! | `Status` | 1 Hz | `StatusTick` |
//! | `Entity` | at most 3 Hz, on change | `EntityDetail`; the last one has `closed` |
//! | `Map` | every 3, 8 or 30 ticks by tier (WP-ADMIN-07, `map.rs`), only the latest frame matters | `MapFrame` |
//! | `Character`, `Chat` | WP-ADMIN-16, -19 (hooks in `watch.rs`, `chat_tap.rs`) | |

use super::chat_tap::{self, ChatFilter, ChatSub};
use super::events::{
    AdminEvent, EntityDetail, LayerInfo, Pushed, RosterDelta, RosterEntry, SharedSink, StatusTick,
    SubError, SubErrorCode,
};
use super::map::{self, MapSpec, MapSub, MapTuning};
use super::queries::{self, AdminQuery, AdminQueryResult, world as qworld};
use super::types::AdminConnId;
use super::watch::{self, CharacterSub};
use crate::tick::Game;
use flyff_common::rt::Reply;
use flyff_common::{CharacterId, ObjId, WorldId};
use std::collections::BTreeMap;

/// Ticks between two roster messages (2 s at 15 Hz, 25 §3.16 "Rates").
pub const ROSTER_EVERY_TICKS: u64 = 30;
/// Ticks between two layer-list checks (at most 1 Hz).
pub const LAYERS_EVERY_TICKS: u64 = 15;
/// Ticks between two status ticks (1 Hz).
pub const STATUS_EVERY_TICKS: u64 = 15;
/// Ticks between two entity-detail checks (3 Hz).
pub const ENTITY_EVERY_TICKS: u64 = 5;
/// A roster row is re-sent when the character moved more than this many metres since the last send.
pub const ROSTER_MOVE_EPS_M: f32 = 2.0;
/// The default of `admin.map_visit_budget`.
pub const DEFAULT_VISIT_BUDGET: u32 = 30_000;
/// Subscriptions per connection the game thread accepts (the gateway enforces the finer limits).
pub const MAX_SUBS_PER_CONN: usize = 32;

/// The 1/16 units a cell costs relative to an entity read (25 §3.15).
const CELL_UNITS: u64 = 1;
const ENTITY_UNITS: u64 = 16;

/// The limits of the registry (live: `admin.map_visit_budget`, sent by the gateway on start and on reload with
/// [`AdminViewOp::Limits`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewLimits {
    /// Entity reads per tick across all subscriptions; a cell visit costs 1/16 of one.
    pub visit_budget: u32,
    /// Subscriptions per connection.
    pub max_subs_per_conn: usize,
    /// The tier thresholds and the entity cap of the map collector (`admin.map_tier_a_m`, `map_tier_b_m`,
    /// `map_max_entities`).
    pub map: MapTuning,
}

impl Default for ViewLimits {
    fn default() -> Self {
        Self {
            visit_budget: DEFAULT_VISIT_BUDGET,
            max_subs_per_conn: MAX_SUBS_PER_CONN,
            map: MapTuning::default(),
        }
    }
}

/// The per-tick visit budget (25 §3.15 step 3): an entity read costs 1, a cell visit 1/16. It is plain integer
/// arithmetic in sixteenths, so the accounting is deterministic and exact.
#[derive(Clone, Copy, Debug)]
pub struct VisitBudget {
    left: u64,
    spent: u64,
}

impl VisitBudget {
    /// A budget of `entity_reads` reads.
    pub fn new(entity_reads: u32) -> VisitBudget {
        VisitBudget {
            left: u64::from(entity_reads) * ENTITY_UNITS,
            spent: 0,
        }
    }

    fn take(&mut self, units: u64) {
        let used = units.min(self.left);
        self.left -= used;
        self.spent += units;
    }

    /// Charges `n` entity reads.
    pub fn entities(&mut self, n: u32) {
        self.take(u64::from(n) * ENTITY_UNITS);
    }

    /// Charges `n` cell visits (1/16 of an entity read each).
    pub fn cells(&mut self, n: u32) {
        self.take(u64::from(n) * CELL_UNITS);
    }

    /// Cell visits that still fit (the budget in sixteenths of an entity read).
    pub fn cells_left(&self) -> u64 {
        self.left
    }

    /// `true` when nothing is left: the caller keeps its cursor and continues next tick.
    pub fn exhausted(&self) -> bool {
        self.left == 0
    }

    /// Entity reads still available (rounded down).
    pub fn remaining(&self) -> u64 {
        self.left / ENTITY_UNITS
    }

    /// Entity reads charged so far this tick, cells included at 1/16 (rounded down; may exceed the budget by the
    /// last charge, which is never refused).
    pub fn visits(&self) -> u64 {
        self.spent / ENTITY_UNITS
    }

    /// Everything charged so far in sixteenths of an entity read (exact; for counters that add up deltas).
    pub fn spent_sixteenths(&self) -> u64 {
        self.spent
    }
}

/// What a subscription watches (25 §11.5 `ViewSpec`, plus `Status`).
#[derive(Clone, Debug, PartialEq)]
pub enum ViewSpec {
    /// The live map (WP-ADMIN-07).
    Map(MapSpec),
    /// One object (`EntityWatch`).
    Entity(ObjId),
    /// One character (WP-ADMIN-16).
    Character(CharacterId),
    /// The chat tap (WP-ADMIN-19).
    Chat(ChatFilter),
    /// The online roster.
    Roster,
    /// The live layers of one world or of all.
    Layers(Option<WorldId>),
    /// The 1 Hz status.
    Status,
}

impl ViewSpec {
    /// A short name for logs and errors.
    pub fn name(&self) -> &'static str {
        match self {
            ViewSpec::Map(_) => "map",
            ViewSpec::Entity(_) => "entity",
            ViewSpec::Character(_) => "character",
            ViewSpec::Chat(_) => "chat",
            ViewSpec::Roster => "roster",
            ViewSpec::Layers(_) => "layers",
            ViewSpec::Status => "status",
        }
    }
}

/// An operation on the registry (25 §11.5 `AdminViewOp`; `Attach` and `Limits` are additions of WP-ADMIN-06).
pub enum AdminViewOp {
    /// A connection opened: from now on the game thread pushes its events into `sink`. The gateway sends it first.
    Attach {
        /// The connection's side of the channel.
        sink: SharedSink,
    },
    /// Starts subscription `sub` (an open one with the same id is replaced).
    Open {
        /// The client-chosen id.
        sub: u32,
        /// What to watch.
        spec: ViewSpec,
    },
    /// Changes a subscription (a map's rectangle, an entity to watch); a roster, layers or status stream restarts
    /// from a full state.
    Update {
        /// The id.
        sub: u32,
        /// The new specification (the same kind as the old).
        spec: ViewSpec,
    },
    /// Ends a subscription.
    Close {
        /// The id.
        sub: u32,
    },
    /// The connection is gone: drops all its subscriptions and its sink.
    CloseConn,
    /// A one-shot read, answered in the same tick.
    Query {
        /// The question.
        q: AdminQuery,
        /// The answer.
        reply: Reply<AdminQueryResult>,
    },
    /// New limits of the registry (`admin.map_visit_budget`).
    Limits(ViewLimits),
}

impl std::fmt::Debug for AdminViewOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AdminViewOp::Attach { .. } => f.write_str("Attach"),
            AdminViewOp::Open { sub, spec } => write!(f, "Open({sub}, {})", spec.name()),
            AdminViewOp::Update { sub, spec } => write!(f, "Update({sub}, {})", spec.name()),
            AdminViewOp::Close { sub } => write!(f, "Close({sub})"),
            AdminViewOp::CloseConn => f.write_str("CloseConn"),
            AdminViewOp::Query { q, .. } => write!(f, "Query({})", q.name()),
            AdminViewOp::Limits(l) => write!(f, "Limits({l:?})"),
        }
    }
}

// ---- subscriptions --------------------------------------------------------------------------------------------------

struct RosterSub {
    due: u64,
    /// The rows the client has, by character id.
    sent: BTreeMap<u32, RosterEntry>,
    /// The counts of the last message.
    counts: Option<(u32, u32)>,
    /// The next message replaces the client's table.
    full_next: bool,
}

struct LayersSub {
    world: Option<WorldId>,
    due: u64,
    last: Option<Vec<LayerInfo>>,
}

struct StatusSub {
    due: u64,
}

struct EntitySub {
    obj: ObjId,
    due: u64,
    last: Option<EntityDetail>,
    /// The `closed` message went out: nothing more follows.
    done: bool,
}

enum Sub {
    Map(Box<MapSub>),
    Entity(EntitySub),
    Character(Box<CharacterSub>),
    Chat(ChatSub),
    Roster(RosterSub),
    Layers(LayersSub),
    Status(StatusSub),
}

impl Sub {
    /// `Some(tick)`: the tick the subscription is next due; `None`: event-driven or finished.
    fn due(&self) -> Option<u64> {
        match self {
            Sub::Map(m) => Some(m.due_tick),
            Sub::Entity(e) => (!e.done).then_some(e.due),
            Sub::Character(c) => Some(c.due_tick),
            Sub::Chat(_) => None,
            Sub::Roster(r) => Some(r.due),
            Sub::Layers(l) => Some(l.due),
            Sub::Status(s) => Some(s.due),
        }
    }

    fn on_resync(&mut self) {
        if let Sub::Roster(r) = self {
            r.full_next = true;
        }
    }

    /// Serves the subscription (it is due): the events to push.
    fn collect(
        &mut self,
        g: &Game,
        tick: u64,
        budget: &mut VisitBudget,
        limits: &ViewLimits,
        shared: &mut TickShared,
    ) -> Vec<AdminEvent> {
        match self {
            Sub::Map(m) => map::collect(m, g, tick, budget, &limits.map)
                .map(AdminEvent::Map)
                .into_iter()
                .collect(),
            Sub::Character(c) => watch::collect(c, g, tick, budget),
            Sub::Chat(_) => Vec::new(),
            Sub::Roster(r) => collect_roster(r, g, tick, budget, shared),
            Sub::Layers(l) => collect_layers(l, g, tick, budget),
            Sub::Status(s) => {
                s.due = tick + STATUS_EVERY_TICKS;
                budget.entities(1);
                vec![AdminEvent::Status(status_tick(g))]
            }
            Sub::Entity(e) => collect_entity(e, g, tick, budget),
        }
    }
}

fn moved(a: &RosterEntry, b: &RosterEntry) -> bool {
    let dx = a.x - b.x;
    let dz = a.z - b.z;
    a.obj_id != b.obj_id
        || a.name != b.name
        || a.level != b.level
        || a.job != b.job
        || a.world != b.world
        || a.layer != b.layer
        || a.guild_id != b.guild_id
        || a.authority != b.authority
        || dx * dx + dz * dz > ROSTER_MOVE_EPS_M * ROSTER_MOVE_EPS_M
}

/// What the subscriptions of one tick share, built on first use: every roster of the tick reads the same rows.
#[derive(Default)]
struct TickShared {
    roster: Option<Vec<RosterEntry>>,
}

fn collect_roster(
    r: &mut RosterSub,
    g: &Game,
    tick: u64,
    budget: &mut VisitBudget,
    shared: &mut TickShared,
) -> Vec<AdminEvent> {
    r.due = tick + ROSTER_EVERY_TICKS;
    let rows: &[RosterEntry] = shared
        .roster
        .get_or_insert_with(|| qworld::roster_entries(g));
    budget.entities(u32::try_from(rows.len()).unwrap_or(u32::MAX));
    let in_world = u32::try_from(g.sessions.len()).unwrap_or(u32::MAX);
    let lobby = g.lobby_count();
    let full = std::mem::take(&mut r.full_next);
    if full {
        r.sent = rows.iter().map(|e| (e.character_id, e.clone())).collect();
        r.counts = Some((lobby, in_world));
        return vec![AdminEvent::Roster(RosterDelta {
            full: true,
            upserts: rows.to_vec(),
            removes: Vec::new(),
            lobby,
            in_world,
        })];
    }
    // an unchanged row keeps the one the client has, so slow drift accumulates until it passes the threshold
    let mut upserts = Vec::new();
    for e in rows {
        match r.sent.get(&e.character_id) {
            Some(old) if !moved(old, e) => {}
            _ => {
                upserts.push(e.clone());
                r.sent.insert(e.character_id, e.clone());
            }
        }
    }
    // the rows are ascending by character id; `sent` now holds every row, so it has more only if somebody left
    let removes: Vec<u32> = if r.sent.len() == rows.len() {
        Vec::new()
    } else {
        r.sent
            .keys()
            .filter(|id| rows.binary_search_by_key(*id, |e| e.character_id).is_err())
            .copied()
            .collect()
    };
    for id in &removes {
        r.sent.remove(id);
    }
    let counts = Some((lobby, in_world));
    if upserts.is_empty() && removes.is_empty() && r.counts == counts {
        return Vec::new();
    }
    r.counts = counts;
    vec![AdminEvent::Roster(RosterDelta {
        full: false,
        upserts,
        removes,
        lobby,
        in_world,
    })]
}

fn collect_layers(
    l: &mut LayersSub,
    g: &Game,
    tick: u64,
    budget: &mut VisitBudget,
) -> Vec<AdminEvent> {
    l.due = tick + LAYERS_EVERY_TICKS;
    let (infos, visited) = qworld::layer_infos_counted(g, l.world);
    budget.entities(visited);
    if l.last.as_ref() == Some(&infos) {
        return Vec::new();
    }
    l.last = Some(infos.clone());
    vec![AdminEvent::Layers(infos)]
}

fn collect_entity(
    e: &mut EntitySub,
    g: &Game,
    tick: u64,
    budget: &mut VisitBudget,
) -> Vec<AdminEvent> {
    e.due = tick + ENTITY_EVERY_TICKS;
    budget.entities(1);
    match qworld::entity_detail(g, e.obj) {
        Some(d) => {
            if e.last.as_ref() == Some(&d) {
                return Vec::new();
            }
            e.last = Some(d.clone());
            vec![AdminEvent::Entity(d)]
        }
        None => {
            e.done = true;
            // the last message: what we last knew, marked closed
            let mut d = e.last.take().unwrap_or_else(|| closed_stub(e.obj));
            d.closed = true;
            vec![AdminEvent::Entity(d)]
        }
    }
}

fn closed_stub(obj: ObjId) -> EntityDetail {
    EntityDetail {
        obj_id: obj.0,
        kind: map::EntityKind::Monster,
        prop: 0,
        name: String::new(),
        world: 0,
        layer: 0,
        x: 0.0,
        y: 0.0,
        z: 0.0,
        level: 0,
        hp: 0,
        hp_max: 0,
        mp: 0,
        mp_max: 0,
        fp: 0,
        fp_max: 0,
        flags: 0,
        ai_state: String::new(),
        target: 0,
        hate: Vec::new(),
        spawn_region: 0,
        respawn_tick: 0,
        character_id: 0,
        owner: 0,
        mode_flags: 0,
        closed: true,
    }
}

/// The 1 Hz status of the game (`alerts` and `maintenance` are the gateway's to fill).
fn status_tick(g: &Game) -> StatusTick {
    let s = g.status_snapshot();
    StatusTick {
        in_world: s.in_world,
        lobby: s.lobby,
        tick_p50_us: s.tick_p50_us,
        tick_p99_us: s.tick_p99_us,
        db_degraded: s.db_degraded,
        db_queue: s.db_queue,
        alerts: 0,
        maintenance: false,
        shutdown_in_s: s.shutdown_in_s.unwrap_or(0),
    }
}

// ---- the registry ---------------------------------------------------------------------------------------------------

struct Conn {
    sink: SharedSink,
    subs: BTreeMap<u32, Sub>,
}

/// Counters of the registry (tests, `/metrics` later).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViewStats {
    /// Events pushed into sinks.
    pub pushed: u64,
    /// Entity reads charged to the budget (cells at 1/16).
    pub visits: u64,
    /// Ticks on which the budget ran out before every due subscription was served.
    pub budget_cuts: u64,
    /// Operations of a connection that was never attached (dropped).
    pub orphan_ops: u64,
    /// Queries answered.
    pub queries: u64,
    /// What the map collector charged, in sixteenths of an entity read (`flyff_admin_map_visits_total` is this
    /// divided by 16).
    pub map_units: u64,
    /// Map frames completed and pushed.
    pub map_frames: u64,
    /// The nanoseconds the last `post` phase of the registry took (the `admin_views` bench reads it).
    pub last_post_ns: u64,
}

/// The registry of admin subscriptions (see the module documentation).
#[derive(Default)]
pub struct AdminViews {
    conns: BTreeMap<AdminConnId, Conn>,
    limits: ViewLimits,
    /// The subscription served last: the next round starts after it.
    cursor: Option<(AdminConnId, u32)>,
    stats: ViewStats,
}

impl std::fmt::Debug for AdminViews {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdminViews")
            .field("conns", &self.conns.len())
            .field("subs", &self.sub_count())
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

impl AdminViews {
    /// Open connections.
    pub fn conn_count(&self) -> usize {
        self.conns.len()
    }

    /// Open subscriptions of all connections.
    pub fn sub_count(&self) -> usize {
        self.conns.values().map(|c| c.subs.len()).sum()
    }

    /// The ids of the subscriptions of `conn`, ascending.
    pub fn subs_of(&self, conn: AdminConnId) -> Vec<u32> {
        self.conns
            .get(&conn)
            .map(|c| c.subs.keys().copied().collect())
            .unwrap_or_default()
    }

    /// The limits in force.
    pub fn limits(&self) -> ViewLimits {
        self.limits
    }

    /// The counters.
    pub fn stats(&self) -> ViewStats {
        self.stats
    }

    /// The chat subscriptions as `(connection, sub id, filter, sink)`, for the chat tap (WP-ADMIN-19).
    pub fn chat_subs(
        &self,
    ) -> impl Iterator<Item = (AdminConnId, u32, &ChatFilter, &SharedSink)> + '_ {
        self.conns.iter().flat_map(|(cid, c)| {
            c.subs.iter().filter_map(move |(sid, s)| match s {
                Sub::Chat(chat) => Some((*cid, *sid, &chat.filter, &c.sink)),
                _ => None,
            })
        })
    }

    fn error(conn: &Conn, sub: u32, e: SubError) -> Pushed {
        conn.sink.push(sub, AdminEvent::Error(e))
    }

    /// Applies one operation (`Query` is answered by the caller; see [`Game::on_admin_view`]).
    fn apply(&mut self, g: &Game, id: AdminConnId, op: AdminViewOp) {
        let tick = g.clock.tick.0;
        match op {
            AdminViewOp::Attach { sink } => {
                self.conns.insert(
                    id,
                    Conn {
                        sink,
                        subs: BTreeMap::new(),
                    },
                );
            }
            AdminViewOp::CloseConn => {
                self.conns.remove(&id);
            }
            AdminViewOp::Limits(l) => self.limits = l,
            AdminViewOp::Close { sub } => {
                if let Some(c) = self.conns.get_mut(&id) {
                    c.subs.remove(&sub);
                }
            }
            AdminViewOp::Open { sub, spec } => {
                let max = self.limits.max_subs_per_conn;
                let Some(c) = self.conns.get_mut(&id) else {
                    self.stats.orphan_ops += 1;
                    return;
                };
                c.subs.remove(&sub);
                if c.subs.len() >= max {
                    let e = SubError::new(
                        SubErrorCode::Limit,
                        format!("at most {max} subscriptions per connection"),
                    );
                    if Self::error(c, sub, e) == Pushed::Closed {
                        self.conns.remove(&id);
                    }
                    return;
                }
                match open_sub(g, sub, spec, tick, &self.limits) {
                    Ok(s) => {
                        c.subs.insert(sub, s);
                    }
                    Err(e) => {
                        if Self::error(c, sub, e) == Pushed::Closed {
                            self.conns.remove(&id);
                        }
                    }
                }
            }
            AdminViewOp::Update { sub, spec } => {
                let Some(c) = self.conns.get_mut(&id) else {
                    self.stats.orphan_ops += 1;
                    return;
                };
                let outcome = match c.subs.get_mut(&sub) {
                    None => Err(SubError::new(
                        SubErrorCode::NotFound,
                        format!("subscription {sub} is not open"),
                    )),
                    Some(s) => update_sub(g, s, spec, tick, &self.limits),
                };
                if let Err(e) = outcome
                    && Self::error(c, sub, e) == Pushed::Closed
                {
                    self.conns.remove(&id);
                }
            }
            AdminViewOp::Query { .. } => {}
        }
    }

    /// The `post` phase: serves the due subscriptions round-robin within the visit budget (see the module
    /// documentation).
    fn run_post(&mut self, g: &Game) {
        if self.conns.is_empty() {
            return;
        }
        let tick = g.clock.tick.0;
        let started_ns = g.telemetry.now_ns();
        let limits = self.limits;
        self.conns.retain(|_, c| !c.sink.is_closed());
        // the ring of (connection, sub) starting after the cursor
        let mut ring: Vec<(AdminConnId, u32)> = self
            .conns
            .iter()
            .flat_map(|(cid, c)| c.subs.keys().map(move |sid| (*cid, *sid)))
            .collect();
        if let Some(cur) = self.cursor {
            let split = ring.partition_point(|k| *k <= cur);
            ring.rotate_left(split);
        }
        let mut budget = VisitBudget::new(self.limits.visit_budget);
        let mut shared = TickShared::default();
        let mut served = 0_u32;
        let mut cut = false;
        let mut dead: Vec<AdminConnId> = Vec::new();
        for (cid, sid) in ring {
            let Some(conn) = self.conns.get_mut(&cid) else {
                continue;
            };
            let Some(sub) = conn.subs.get_mut(&sid) else {
                continue;
            };
            if !sub.due().is_some_and(|d| d <= tick) {
                continue;
            }
            if served > 0 && budget.exhausted() {
                cut = true;
                break;
            }
            served += 1;
            self.cursor = Some((cid, sid));
            let before = budget.spent_sixteenths();
            let events = sub.collect(g, tick, &mut budget, &limits, &mut shared);
            if matches!(sub, Sub::Map(_)) {
                self.stats.map_units += budget.spent_sixteenths() - before;
                self.stats.map_frames += events.len() as u64;
            }
            for ev in events {
                self.stats.pushed += 1;
                match conn.sink.push(sid, ev) {
                    Pushed::Accepted => {}
                    Pushed::Resync => sub.on_resync(),
                    Pushed::Closed => {
                        dead.push(cid);
                        break;
                    }
                }
            }
        }
        for cid in dead {
            self.conns.remove(&cid);
        }
        if cut {
            self.stats.budget_cuts += 1;
        }
        self.stats.visits += budget.visits();
        g.telemetry.admin_map_visits.store(
            self.stats.map_units / 16,
            std::sync::atomic::Ordering::Relaxed,
        );
        self.stats.last_post_ns = g.telemetry.now_ns().saturating_sub(started_ns);
        g.telemetry
            .admin_views_hist
            .observe_ns(self.stats.last_post_ns);
    }
}

fn open_sub(
    g: &Game,
    sub: u32,
    spec: ViewSpec,
    tick: u64,
    limits: &ViewLimits,
) -> Result<Sub, SubError> {
    Ok(match spec {
        ViewSpec::Map(m) => Sub::Map(Box::new(map::open(g, sub, m, tick, &limits.map)?)),
        ViewSpec::Entity(obj) => {
            if qworld::entity_detail(g, obj).is_none() {
                return Err(SubError::new(
                    SubErrorCode::NotFound,
                    format!("object {obj} does not exist"),
                ));
            }
            Sub::Entity(EntitySub {
                obj,
                due: tick,
                last: None,
                done: false,
            })
        }
        ViewSpec::Character(c) => Sub::Character(Box::new(watch::open(g, c, tick)?)),
        ViewSpec::Chat(f) => Sub::Chat(chat_tap::open(f)?),
        ViewSpec::Roster => Sub::Roster(RosterSub {
            due: tick,
            sent: BTreeMap::new(),
            counts: None,
            full_next: true,
        }),
        ViewSpec::Layers(world) => Sub::Layers(LayersSub {
            world,
            due: tick,
            last: None,
        }),
        ViewSpec::Status => Sub::Status(StatusSub { due: tick }),
    })
}

fn update_sub(
    g: &Game,
    sub: &mut Sub,
    spec: ViewSpec,
    tick: u64,
    limits: &ViewLimits,
) -> Result<(), SubError> {
    match (sub, spec) {
        (Sub::Map(m), ViewSpec::Map(spec)) => map::update(g, m, spec, tick, &limits.map),
        (Sub::Character(c), ViewSpec::Character(who)) => watch::update(g, c, who, tick),
        (Sub::Chat(c), ViewSpec::Chat(f)) => chat_tap::update(c, f),
        (Sub::Entity(e), ViewSpec::Entity(obj)) => {
            if qworld::entity_detail(g, obj).is_none() {
                return Err(SubError::new(
                    SubErrorCode::NotFound,
                    format!("object {obj} does not exist"),
                ));
            }
            *e = EntitySub {
                obj,
                due: tick,
                last: None,
                done: false,
            };
            Ok(())
        }
        (Sub::Roster(r), ViewSpec::Roster) => {
            r.due = tick;
            r.full_next = true;
            Ok(())
        }
        (Sub::Layers(l), ViewSpec::Layers(world)) => {
            *l = LayersSub {
                world,
                due: tick,
                last: None,
            };
            Ok(())
        }
        (Sub::Status(s), ViewSpec::Status) => {
            s.due = tick;
            Ok(())
        }
        (_, spec) => Err(SubError::new(
            SubErrorCode::BadSpec,
            format!("a {} update does not fit that subscription", spec.name()),
        )),
    }
}

impl Game {
    /// `GameInput::AdminView` (25 ADM-D4): a registry operation or a read-only query. Not a recorded input and never
    /// a change of world state; a query borrows the whole game immutably.
    pub(crate) fn on_admin_view(&mut self, conn: AdminConnId, op: AdminViewOp) {
        match op {
            AdminViewOp::Query { q, reply } => {
                let answer = queries::run(self, q);
                self.admin_views.stats.queries += 1;
                let _ = reply.send(answer);
            }
            other => {
                // the registry leaves the game for the call, so the handlers can hold `&Game`
                let mut views = std::mem::take(&mut self.admin_views);
                views.apply(self, conn, other);
                // the chat routes record only while somebody watches (25 §4.9)
                self.services
                    .chat
                    .tap
                    .set_enabled(views.chat_subs().next().is_some());
                self.admin_views = views;
            }
        }
    }

    /// The `post` phase of the admin views (see [`AdminViews`]).
    pub(crate) fn run_admin_views(&mut self) {
        if self.admin_views.conns.is_empty() {
            self.services.chat.tap.set_enabled(false);
            return;
        }
        let mut views = std::mem::take(&mut self.admin_views);
        // the lines the chat routes recorded this tick go to the matching subscriptions (WP-ADMIN-19)
        let lines = self.services.chat.tap.take();
        chat_tap::deliver(&views, lines);
        views.run_post(self);
        // a connection that went away in `run_post` takes its chat subscriptions with it
        self.services
            .chat
            .tap
            .set_enabled(views.chat_subs().next().is_some());
        self.admin_views = views;
    }

    /// The registry of admin subscriptions (inspection).
    pub fn admin_views(&self) -> &AdminViews {
        &self.admin_views
    }

    /// Connected sockets that are not in the world (the `lobby` of `StatusSnapshot`).
    pub(crate) fn lobby_count(&self) -> u32 {
        let in_world = u32::try_from(self.sessions.len()).unwrap_or(u32::MAX);
        u32::try_from(
            self.telemetry
                .sessions
                .load(std::sync::atomic::Ordering::Relaxed),
        )
        .unwrap_or(u32::MAX)
        .saturating_sub(in_world)
    }
}
