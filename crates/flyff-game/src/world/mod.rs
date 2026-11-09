//! Worlds, layers and objects (Plan/15): the world catalog, the object store, the per-tick phases, the link-map
//! grid with its visibility sets and the spawn serialization.
//!
//! | module | content | WP |
//! |---|---|---|
//! | [`catalog`], [`terrain`] | which worlds load, `WorldStatic` per folder, `GridGeom`, bounds, terrain queries | WORLD-01 |
//! | [`object`] | object header types, `ObjIdAllocator`, [`ObjectStore`] | WORLD-23, -02 |
//! | [`lifecycle`], [`phases`] | spawn / delete / `set_pos`, the destroy order, the per-tick phase driver and its hooks | WORLD-02 |
//! | [`grid`], [`visibility`] | the link-map cells, legacy range queries, add / modify / remove / resend view | WORLD-03 |
//! | [`spawn`] | `Spawn*` / `DelObj` builders, the encode-once cache, the `SpawnSink` over the `Outbox` | WORLD-06 |
//! | [`layer`], [`dyo`] | layers (ticket layers, `create_layer`, invalidate, GC, drive-out), `.dyo` NPC and ctrl placement, `NpcLocator` | WORLD-07, -08 |
//! | [`replace`], [`region`], [`continent`], [`revival`] | teleport and world change, region attributes and portals, continents, revival points | WORLD-10, -11, -15 |
//! | [`respawn`] | the respawn lines, `Respawner`, the P1 spawn tick | WORLD-14 |
//! | [`clock`], [`environment`] | the in-game clock, `GameTimer`, `Environment`, seasons | WORLD-12 |
//! | [`join`], [`logout`] | join placement and the join sequence driver, the re-attach view; `destroy_player` / `remove_player` | WORLD-09, -13 |
//! | [`item_drop`], [`fx`] | ground items and `spawn_ground_item`; the effect message helpers | WORLD-16, -17 |
//! | [`tele16`] | the TELE16 teleporter rules library: `validate`, `resolve_destination`, `Cooldowns` (no handler) | WORLD-19 |
//! | [`ctrl`] | the common ctrls: entity, `CCommonCtrl::Process`, the open flow, linked ctrls, colliders | WORLD-02, WORLD-18 |
//!
//! [`Worlds`] owns the worlds in ascending id order and the one [`ObjectStore`]; iteration in logic uses
//! `BTreeMap`s, the insertion-ordered sets of the layers and slot order, never a `HashMap` (15 §2).

pub mod catalog;
pub mod clock;
pub mod continent;
pub mod ctrl;
pub mod dyo;
pub mod environment;

pub mod fx;
pub mod grid;
pub mod item_drop;
pub mod join;
pub mod layer;
pub mod lifecycle;
pub mod logout;
pub mod object;
pub mod phases;
pub mod region;
pub mod replace;
pub mod respawn;
pub mod revival;
pub mod spawn;
pub mod tele16;
pub mod terrain;
pub mod visibility;

pub use catalog::{
    CatalogReport, CollisionSource, GridGeom, REFERENCE_WORLD_IDS, SkipReason, WLD_EXTRA_WIDTH,
    WorldCatalog, WorldEntry, WorldStatic, check_c_w01, n_view,
};
pub use clock::{GAME_CLOCK_BASE_MS, InGameTime, TIMESPEED, game_timer_msg};
pub use continent::Continents;
pub use ctrl::Ctrl;
pub use dyo::{ConfigEventFlags, EventFlagQuery, FixedEventFlags, NpcLocator, usable_dyo};
pub use environment::{EnvironmentState, SeasonTable};
pub use grid::{
    Cell, CellRect, Grid, LinkSet, Metric, aoe_n_range, cell_of, legacy_cells,
    skill_region_n_range, window,
};
pub use item_drop::{ITEM_EXPIRY_MS, ItemDrop};
pub use layer::{
    Layer, LayerDriveOut, LayerError, LayerKind, LayerQueues, LayerState, MadrigalDriveOut,
    OnDieEntry, TicketLayers,
};
pub use lifecycle::AddError;
pub use object::{
    CellIdx, CtrlKey, IdAlarmHook, IdShutdown, ItemKey, LinkState, MoverKey, OBJ_ID_CLIENT_LOCAL,
    OBJ_ID_WARN_THRESHOLD, ObjCore, ObjIdAllocator, ObjKind, ObjRef, ObjectStore, PlayerView,
    RespawnOrigin,
};
pub use phases::{FrameCtx, PhaseEnv, WorldHooks};
pub use region::{is_region_attr, is_region_move, pkpvp_region_attr, replace_inspection};
pub use replace::{ReplaceEntry, ReplaceError, ReplaceKind};
pub use respawn::{
    NoRespawnHooks, RespawnConfig, RespawnHooks, RespawnLoadReport, RespawnProto, RespawnState,
    RespawnTable, Respawner, SpawnEnv, SpawnReport,
};
pub use revival::{RevivalIndex, RevivalPoint};
pub use spawn::{SpawnCache, SpawnCacheStats, SpawnWriter};
pub use tele16::{
    Cooldowns as Tele16Cooldowns, Tele16Check, Tele16Refusal, Tele16Rules, Tele16Subject,
    Tele16Target,
};
pub use terrain::HeightAttr;
pub use visibility::{Phase, RecordingSink, SpawnEvent, SpawnSink};

use flyff_common::math::Vec3;
use flyff_common::{LayerId, ObjId, WorldId};
use std::collections::BTreeMap;
use std::sync::Arc;

/// What the game thread is started with besides the persistence handle (`GameInit.world`): the loaded game
/// data and the built world catalog (15 §3.5 step 1). Both are optional so the M1 skeleton and most unit
/// tests run without them (`GameData::default()` and no world).
#[derive(Default)]
pub struct WorldInit {
    /// The loaded `GameData`; `None` = an empty one.
    pub data: Option<Arc<flyff_data::GameData>>,
    /// The worlds; `None` = no world at all.
    pub catalog: Option<WorldCatalog>,
    /// `layer.inc`: the ticket overflow layers of each world (15 §3.5 step 2).
    pub ticket_layers: TicketLayers,
    /// `EnvironmentEffect.txt`: the season of each real month (15 §17.2).
    pub seasons: SeasonTable,
    /// The `FLYK` ctrl collision models (`ServerDerived.ctrl_models`, 24 §10.8); `None` = no ctrl collides.
    pub ctrl_models: Option<Arc<flyff_sim::collision::CtrlModelSet>>,
}

impl std::fmt::Debug for WorldInit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorldInit")
            .field("data", &self.data.is_some())
            .field(
                "worlds",
                &self.catalog.as_ref().map_or(0, WorldCatalog::len),
            )
            .finish()
    }
}

/// One loaded world id: its static data and its layers (15 §3.5 step 2).
#[derive(Debug)]
pub struct World {
    /// The id.
    pub id: WorldId,
    /// The display name.
    pub title: String,
    /// The shared per-folder data.
    pub statics: Arc<WorldStatic>,
    /// The link-map geometry of this world id.
    pub geom: GridGeom,
    /// The layers, ascending; layer 0 always exists.
    pub layers: BTreeMap<LayerId, Layer>,
    /// The regions added at runtime (F-101, 15 §11.5): evaluated after the static ones.
    pub dynamic_regions: Vec<flyff_data::worlds::RegionDef>,
    /// The accepted respawn lines of the world's folder (15 §15.1; empty until [`Worlds::init_respawn`]).
    pub respawn: Arc<RespawnTable>,
}

/// Counters of the world driver (tests, `/metrics`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WorldStats {
    /// Objects destroyed at `_delete`.
    pub destroyed: u64,
    /// Relinks performed at `_modifylink`.
    pub relinks: u64,
    /// Mover write-backs of the phase driver.
    pub write_backs: u64,
    /// Write-backs that found `core.pos` changed by somebody else than the driver (15 §5.5).
    pub direct_core_writes: u64,
    /// Sim positions outside the world that the write-back refused.
    pub rejected_moves: u64,
    /// Movers skipped by the sleep rule (unobserved non-pets), per layer pass.
    pub slept: u64,
    /// Layers released by the garbage collection (15 §4.3).
    pub layers_released: u64,
}

/// The worlds in their fixed order with their layers and the object store (15 §3, §5).
#[derive(Debug, Default)]
pub struct Worlds {
    worlds: BTreeMap<WorldId, World>,
    pub(crate) store: ObjectStore,
    /// Driver counters.
    pub stats: WorldStats,
    /// The movement pipeline's shared data and counters (Plan/18; installed by `Game::new` from
    /// `Handlers.movement`).
    pub movement: crate::movement::MovementState,
    /// The once-per-second sample of the in-game clock (`g_GameTimer.Compute()`, 15 §17.1).
    pub game_time: InGameTime,
    /// The weather effect as the clients know it (15 §17.2).
    pub environment: EnvironmentState,
    /// `EnvironmentEffect.txt`.
    pub seasons: SeasonTable,
    /// The server continents of Madrigal (15 §12.4).
    pub continents: Continents,
    /// The revival points of every world (15 §12.1).
    pub revival: RevivalIndex,
    /// Character key to place of the startup placement (15 §13.3).
    pub npc_locator: NpcLocator,
    /// The NPC-menu index of VR-012 / VR-013 (doc 20 §8.1, WP-NPC-19), fed by the add and destroy hooks of the
    /// phase driver.
    pub npc_checker: crate::npc::NpcChecker,
    /// Players that entered a layer by a world change and still need their `SpawnSelf` at `_add`.
    pub(crate) pending_self: std::collections::BTreeSet<ObjId>,
    /// Players whose world id changed: the save pass turns them into `WorldChange` snapshots (15 §10.4).
    pub(crate) save_requests: Vec<ObjId>,
    /// The drive-out of invalidated layers (15 §4.3).
    pub(crate) drive_out: layer::DriveOutSlot,
    /// The seams of the respawn tick (doc 17's DPS marker).
    pub(crate) respawn_hooks: respawn::RespawnHookSlot,
    /// The `FLYK` ctrl collision models every layer's colliders pose their ctrls with (24 §10.8).
    pub(crate) ctrl_models: Arc<flyff_sim::collision::CtrlModelSet>,
    /// The seams of the ctrl code (trap skills, the psychic wall: doc 17).
    pub(crate) ctrl_hooks: ctrl::CtrlHookSlot,
    /// The TELE16 cool-downs per `(character, row)` (VR-063, 15 §10.7; in memory, not persisted).
    pub tele16_cooldowns: tele16::Cooldowns,
    /// The lap timer of the world frame (`flyff_world_phase_seconds`, WP-OPS-10): unarmed (a no-op) until the game
    /// thread hands it the telemetry.
    pub(crate) phase_times: phases::PhaseTimes,
}

impl Worlds {
    /// No world at all (the M1 behaviour and most unit tests).
    pub fn empty() -> Worlds {
        Worlds::default()
    }

    /// The startup sequence of 15 §3.5 steps 1-2 for the layer-0 part: one [`World`] per catalog entry, in
    /// ascending id, each with its layer 0 (`LayerKind::Normal`). Placement of the `.dyo` objects, ticket
    /// layers and the respawner are the world-flow work packages.
    pub fn from_catalog(catalog: &WorldCatalog) -> Worlds {
        let mut w = Worlds::default();
        for e in catalog.entries() {
            w.insert_world(e);
        }
        w
    }

    /// The startup sequence of 15 §3.5 steps 1-3: [`Worlds::from_catalog`], then per world ascending the `.dyo`
    /// placement of layer 0 and of the ticket layers `-1 .. -n` (each its own copy, recorded in the
    /// [`NpcLocator`]), the revival index and the continents. Initial populations appear at the first respawn
    /// tick (WP-WORLD-14).
    pub fn start(
        catalog: &WorldCatalog,
        data: &flyff_data::GameData,
        tickets: &TicketLayers,
        flags: &dyn EventFlagQuery,
        rng: &mut crate::rng::RngSet,
    ) -> Worlds {
        let mut w = Worlds::from_catalog(catalog);
        w.revival = RevivalIndex::new(&data.worlds.revival);
        w.continents = Continents::from_registry(&data.worlds);
        let ids: Vec<WorldId> = w.world_ids().collect();
        for id in ids {
            w.load_dyo(id, LayerId(0), data, flags, rng.world(id), true);
            for n in 1..=tickets.expanded(id) {
                let l = LayerId(-n);
                if w.add_layer(id, l, LayerKind::Ticket) {
                    w.load_dyo(id, l, data, flags, rng.world(id), true);
                }
            }
        }
        w
    }

    /// Installs the `FLYK` ctrl models: every layer's colliders pose their ctrls with them (colliders already
    /// registered are re-posed; those whose model is missing are dropped).
    pub fn set_ctrl_models(&mut self, models: Arc<flyff_sim::collision::CtrlModelSet>) {
        self.ctrl_models = Arc::clone(&models);
        for w in self.worlds.values_mut() {
            for l in w.layers.values_mut() {
                Arc::make_mut(&mut l.dyn_colliders).set_models(Arc::clone(&models));
            }
        }
    }

    /// The `FLYK` ctrl models in use.
    pub fn ctrl_models(&self) -> &Arc<flyff_sim::collision::CtrlModelSet> {
        &self.ctrl_models
    }

    /// The dynamic ctrl colliders of `(world, layer)` (cheap `Arc` clone; an empty set for an unknown layer).
    pub fn dynamic_of(
        &self,
        world: WorldId,
        layer: LayerId,
    ) -> Arc<flyff_sim::collision::dynamic::DynamicColliders> {
        self.layer(world, layer)
            .map(|l| Arc::clone(&l.dyn_colliders))
            .unwrap_or_default()
    }

    /// A fresh, empty set of ctrl colliders for a world (its link grid) with the installed models.
    fn new_colliders(
        &self,
        statics: &WorldStatic,
    ) -> Arc<flyff_sim::collision::dynamic::DynamicColliders> {
        let c = &statics.collision;
        let mut d =
            flyff_sim::collision::dynamic::DynamicColliders::for_world(c.mpu, c.lands_w, c.lands_h);
        d.set_models(Arc::clone(&self.ctrl_models));
        Arc::new(d)
    }

    /// Adds (or replaces) a world with an empty layer 0.
    pub fn insert_world(&mut self, e: &WorldEntry) {
        let geom = e.statics.grid_geom(e.id);
        let mut layers = BTreeMap::new();
        let mut layer0 = Layer::new(LayerId(0), LayerKind::Normal, Grid::new(geom));
        layer0.dyn_colliders = self.new_colliders(&e.statics);
        layers.insert(LayerId(0), layer0);
        self.worlds.insert(
            e.id,
            World {
                id: e.id,
                title: e.title.clone(),
                statics: Arc::clone(&e.statics),
                geom,
                layers,
                dynamic_regions: Vec::new(),
                respawn: Arc::default(),
            },
        );
    }

    /// Adds an empty layer to a loaded world (the plain mechanism of `create_layer`, 15 §4.2; WP-WORLD-07
    /// adds its checks, the `.dyo` placement and the respawner). `false` when the world is missing or the
    /// layer exists.
    pub fn add_layer(&mut self, world: WorldId, id: LayerId, kind: LayerKind) -> bool {
        let Some(w) = self.worlds.get_mut(&world) else {
            return false;
        };
        if w.layers.contains_key(&id) {
            return false;
        }
        let grid = Grid::new(w.geom);
        let mut layer = Layer::new(id, kind, grid);
        layer.respawner = Respawner::fresh(w.respawn.len());
        let statics = Arc::clone(&w.statics);
        w.layers.insert(id, layer);
        let colliders = self.new_colliders(&statics);
        if let Some(l) = self.layer_mut(world, id) {
            l.dyn_colliders = colliders;
        }
        true
    }

    /// The worlds in ascending id order.
    pub fn worlds_iter(&self) -> impl Iterator<Item = &World> + '_ {
        self.worlds.values()
    }

    /// The loaded world ids in ascending order.
    pub fn world_ids(&self) -> impl Iterator<Item = WorldId> + '_ {
        self.worlds.keys().copied()
    }

    /// The number of loaded worlds.
    pub fn len(&self) -> usize {
        self.worlds.len()
    }

    /// `true` when no world is loaded.
    pub fn is_empty(&self) -> bool {
        self.worlds.is_empty()
    }

    /// The world `id`.
    pub fn world(&self, id: WorldId) -> Option<&World> {
        self.worlds.get(&id)
    }

    /// The world `id`, mutably.
    pub fn world_mut(&mut self, id: WorldId) -> Option<&mut World> {
        self.worlds.get_mut(&id)
    }

    /// The layer `(world, layer)`.
    pub fn layer(&self, world: WorldId, layer: LayerId) -> Option<&Layer> {
        self.worlds.get(&world)?.layers.get(&layer)
    }

    /// The layer `(world, layer)`, mutably.
    pub fn layer_mut(&mut self, world: WorldId, layer: LayerId) -> Option<&mut Layer> {
        self.worlds.get_mut(&world)?.layers.get_mut(&layer)
    }

    /// Disjoint borrows for combat code (Plan/17): the object store, the movement state (which carries the combat
    /// runtime) and the link-map grid of the layer `(world, layer)`. `None` when the layer does not exist.
    pub(crate) fn split_for_combat(
        &mut self,
        world: WorldId,
        layer: LayerId,
    ) -> Option<(&mut ObjectStore, &mut crate::movement::MovementState, &Grid)> {
        let Worlds {
            worlds,
            store,
            movement,
            ..
        } = self;
        let l = worlds.get(&world)?.layers.get(&layer)?;
        Some((store, movement, &l.grid))
    }

    /// The object store.
    pub fn store(&self) -> &ObjectStore {
        &self.store
    }

    /// VR-012: an NPC that offers `menu` within `gameplay.npc_menu_range` of `pos` in `(world, layer)`
    /// ([`crate::npc::NpcChecker::close_npc`] over the live NPC positions). Every NPC-service handler calls
    /// this first (doc 20 §8.1).
    pub fn close_npc(
        &self,
        menu: flyff_data::types::MenuId,
        world: WorldId,
        layer: LayerId,
        pos: Vec3,
    ) -> Option<ObjId> {
        self.npc_checker
            .close_npc(menu, world, layer, pos, &self.store)
    }

    /// VR-013: an NPC with a menu within `radius` (XZ) of `pos` in `(world, layer)` — the personal-shop
    /// exclusion; `radius` is `gameplay.vendor_npc_min_dist`.
    pub fn any_menu_npc_within_xz(
        &self,
        world: WorldId,
        layer: LayerId,
        pos: Vec3,
        radius: f32,
    ) -> bool {
        self.npc_checker
            .any_menu_npc_within_xz(world, layer, pos, radius, &self.store)
    }

    /// The object store, mutably (for the in-crate phase code and the object helpers below).
    pub fn store_mut(&mut self) -> &mut ObjectStore {
        &mut self.store
    }

    /// The mover `id` (any kind).
    pub fn mover(&self, id: ObjId) -> Option<&crate::mover::Mover> {
        self.store.mover(id)
    }

    /// The mover `id`, mutably.
    pub fn mover_mut(&mut self, id: ObjId) -> Option<&mut crate::mover::Mover> {
        self.store.mover_mut(id)
    }

    /// The player mover `id` (`None` for other kinds).
    pub fn player(&self, id: ObjId) -> Option<&crate::mover::Mover> {
        self.store.mover(id).filter(|m| m.is_player())
    }

    /// The player mover `id`, mutably.
    pub fn player_mut(&mut self, id: ObjId) -> Option<&mut crate::mover::Mover> {
        self.store.mover_mut(id).filter(|m| m.is_player())
    }

    /// The header of any object.
    pub fn core(&self, id: ObjId) -> Option<&ObjCore> {
        self.store.core(id)
    }

    /// The static data of the world an object is in.
    pub fn statics_of(&self, id: ObjId) -> Option<&Arc<WorldStatic>> {
        let c = self.store.core(id)?;
        self.worlds.get(&c.world).map(|w| &w.statics)
    }

    /// Candidates of the cells that cover a radius around `c`, filtered by the exact distance (`< r`), in
    /// the legacy scan order (15 §6.4 `query_radius`; a convenience for new code: legacy call sites use
    /// [`Grid::for_each_legacy`] with their original `nRange` and predicate).
    pub fn query_radius(
        &self,
        world: WorldId,
        layer: LayerId,
        c: Vec3,
        r: f32,
        set: LinkSet,
        metric: Metric,
    ) -> smallvec::SmallVec<[ObjId; 32]> {
        let mut out = smallvec::SmallVec::new();
        let Some(l) = self.layer(world, layer) else {
            return out;
        };
        // the covering range in heightmap units, rounded up (a radius larger than the legacy buckets works)
        let mpu = l.grid.geom().mpu as f32;
        let n_range = ((r / mpu).ceil() as i32).saturating_add(1);
        l.grid.for_each_legacy(c, n_range, set, |id| {
            if let Some(core) = self.store.core(id) {
                let d = core.pos - c;
                let dist2 = match metric {
                    Metric::D3 => d.x * d.x + d.y * d.y + d.z * d.z,
                    Metric::Xz => d.x * d.x + d.z * d.z,
                };
                if dist2 < r * r {
                    out.push(id);
                }
            }
        });
        out
    }
}
