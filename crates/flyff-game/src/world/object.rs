//! The shared object header types (Plan/15 §5.1–§5.2, WP-WORLD-23): types only.
//!
//! [`ObjCore`] is embedded as the first field `core` of `Mover` (doc 16), `ItemDrop` (15 §16) and `Ctrl`
//! (15 §14); [`PlayerView`] is part of the player extension of `Mover` (`PlayerExt.view`). The object
//! store, the queues and the phases are WP-WORLD-02, the grid is WP-WORLD-03; they use these types
//! unchanged (15 §22 Q5).
//!
//! ```
//! use flyff_common::{LayerId, ObjId, WorldId};
//! use flyff_game::world::{LinkState, ObjCore, ObjKind, PlayerView};
//!
//! /// Doc 16's `Mover` embeds the header and the player's view set (embedding check of WP-WORLD-23).
//! struct Mover {
//!     core: ObjCore,
//!     view: PlayerView,
//! }
//!
//! let m = Mover {
//!     core: ObjCore::new(ObjId(1), ObjKind::Mover, 11, WorldId(1), LayerId(0)),
//!     view: PlayerView::default(),
//! };
//! assert_eq!(m.core.link, LinkState::Detached);
//! assert!(m.view.seen.is_empty());
//! ```

use super::ctrl::Ctrl;
use super::item_drop::ItemDrop;
use crate::mover::Mover;
use flyff_common::det::{DetMap, DetSet};
use flyff_common::math::Vec3;
use flyff_common::{LayerId, ObjId, WorldId};

slotmap::new_key_type! {
    /// Key of the `movers` arena (players, NPCs, monsters, pets).
    pub struct MoverKey;
    /// Key of the `items` arena (ground items).
    pub struct ItemKey;
    /// Key of the `ctrls` arena (chests, doors, traps, triggers).
    pub struct CtrlKey;
}

/// A reference into one of the three arenas (15 §5.1).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObjRef {
    /// `OT_MOVER`.
    Mover(MoverKey),
    /// `OT_ITEM`.
    Item(ItemKey),
    /// `OT_CTRL`.
    Ctrl(CtrlKey),
}

/// The kind of a world object; the discriminant is the legacy `OT_*` wire value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum ObjKind {
    /// `OT_CTRL` (`CCommonCtrl`).
    Ctrl = 2,
    /// `OT_ITEM` (`CItem`).
    Item = 4,
    /// `OT_MOVER` (`CUser`, `CMover`).
    Mover = 5,
}

impl ObjKind {
    /// The legacy `OT_*` value.
    pub const fn ot(self) -> u8 {
        self as u8
    }
}

/// A cell of the link-map grid (15 §6.1): `x`, `z` in cell units.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct CellIdx {
    /// Cell column.
    pub x: i32,
    /// Cell row.
    pub z: i32,
}

/// Where an object stands in the add/link lifecycle (15 §5.3).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LinkState {
    /// Created but not yet queued (or already unlinked).
    #[default]
    Detached,
    /// Queued for the next `_add` of its layer.
    PendingAdd,
    /// Inserted into the grid cell `cell`.
    Linked {
        /// The linked cell.
        cell: CellIdx,
    },
}

/// The respawn entry an object came from (15 §15.5); `gm_index` is the legacy `m_nGMIndex`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RespawnOrigin {
    /// Index among the accepted respawn lines of the world.
    pub gm_index: u16,
}

/// The common header of every world object (15 §5.1; legacy `CObj`).
#[derive(Clone, Debug)]
pub struct ObjCore {
    /// Runtime id (never reused within a process run, 15 §5.2).
    pub id: ObjId,
    /// Arena kind.
    pub kind: ObjKind,
    /// `MoverProp` / `ItemProp` / `CtrlProp` id (legacy `m_dwIndex`).
    pub index: u32,
    /// World.
    pub world: WorldId,
    /// Layer.
    pub layer: LayerId,
    /// Add/link state.
    pub link: LinkState,
    /// Queued in the layer's modify-link queue.
    pub pending_relink: bool,
    /// Position; movers: a read-only copy of the sim position written back after each `step_frame` (15 §5.5).
    pub pos: Vec3,
    /// Yaw in degrees, quantised by `set_angle` (ref/06 §7.2); movers: copy of the sim's yaw.
    pub angle: f32,
    /// Pitch angle.
    pub angle_x: f32,
    /// Roll angle.
    pub angle_z: f32,
    /// Uniform scale (`m_vScale.x`).
    pub scale: f32,
    /// `OBJ_FLAG_POSCHANGED` (region processing, 15 §11.2).
    pub pos_changed: bool,
    /// `OBJ_FLAG_DELETE`.
    pub deleted: bool,
    /// Players that have this object spawned (legacy `m_2pc`); a linked player contains itself.
    pub viewers: DetSet<ObjId>,
    /// The respawn entry this object came from, if any (15 §15.5).
    pub respawn: Option<RespawnOrigin>,
    /// The position of the last write-back of the phase driver (15 §5.5): a debug check compares it with
    /// `pos` before the next write-back to find direct writes during phase 2.
    pub(crate) wb_pos: Vec3,
}

impl ObjCore {
    /// A detached header at the origin with scale 1.
    pub fn new(id: ObjId, kind: ObjKind, index: u32, world: WorldId, layer: LayerId) -> Self {
        Self {
            id,
            kind,
            index,
            world,
            layer,
            link: LinkState::Detached,
            pending_relink: false,
            pos: Vec3::ZERO,
            angle: 0.0,
            angle_x: 0.0,
            angle_z: 0.0,
            scale: 1.0,
            pos_changed: false,
            deleted: false,
            viewers: DetSet::default(),
            respawn: None,
            wb_pos: Vec3::ZERO,
        }
    }
}

/// The player's view of the world (legacy `m_2npc`); part of the player extension of `Mover` (15 §5.1).
#[derive(Clone, Debug, Default)]
pub struct PlayerView {
    /// Non-player objects this player has spawned.
    pub seen: DetSet<ObjId>,
}

/// Ids at or above this value are client-local movers (`0xFF00_0000 + n`: system pet, previews); the
/// server never issues one (15 §5.2, 40 OQ7).
pub const OBJ_ID_CLIENT_LOCAL: u32 = 0xFF00_0000;

/// From this id on the allocator asks for a scheduled graceful restart (once).
pub const OBJ_ID_WARN_THRESHOLD: u32 = 0xFE00_0000;

/// What the allocator asks the runtime for (15 §5.2).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IdShutdown {
    /// The warning threshold was reached: a scheduled graceful restart with the normal countdown.
    Scheduled,
    /// The id space is exhausted: shut down at once (no countdown; the final saves still run).
    Immediate,
}

/// The one request hook of the allocator: both the warning threshold and the exhaustion go through it
/// (the runtime turns a request into a `Shutdown` input; tests install a recorder).
pub trait IdAlarmHook: Send + 'static {
    /// Asks the runtime to shut down.
    fn request_shutdown(&mut self, kind: IdShutdown);
}

/// Allocator of [`ObjId`]s: starts at 1, strictly increasing, never wraps, never reuses (15 §5.2).
///
/// Ids are allocated when an object is **created**. The allocator never yields `0` or any id at or above
/// [`OBJ_ID_CLIENT_LOCAL`]; at [`OBJ_ID_WARN_THRESHOLD`] it logs an error once and asks for a scheduled
/// restart; exhaustion below the bound is fatal: [`ObjIdAllocator::alloc`] returns `None`, logs the error
/// and requests an immediate shutdown exactly once.
pub struct ObjIdAllocator {
    next: u32,
    warned: bool,
    exhausted: bool,
    hook: Option<Box<dyn IdAlarmHook>>,
}

impl Default for ObjIdAllocator {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for ObjIdAllocator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ObjIdAllocator")
            .field("next", &self.next)
            .field("warned", &self.warned)
            .field("exhausted", &self.exhausted)
            .finish_non_exhaustive()
    }
}

impl ObjIdAllocator {
    /// An allocator whose first id is 1.
    pub fn new() -> Self {
        Self::starting_at(1)
    }

    /// An allocator whose first id is `next` (clamped to `1..=OBJ_ID_CLIENT_LOCAL`); tests seed it near
    /// the thresholds.
    pub fn starting_at(next: u32) -> Self {
        Self {
            next: next.clamp(1, OBJ_ID_CLIENT_LOCAL),
            warned: false,
            exhausted: false,
            hook: None,
        }
    }

    /// Installs the shutdown-request hook (the runtime wires it to a `Shutdown` input).
    pub fn set_hook(&mut self, hook: Box<dyn IdAlarmHook>) {
        self.hook = Some(hook);
    }

    /// The id the next successful [`ObjIdAllocator::alloc`] returns.
    pub fn peek(&self) -> u32 {
        self.next
    }

    /// Returns the next id and advances; `None` when the id space below [`OBJ_ID_CLIENT_LOCAL`] is used up
    /// (the spawn then fails with `AddError::IdsExhausted`; the counter never wraps).
    pub fn alloc(&mut self) -> Option<ObjId> {
        if self.next >= OBJ_ID_CLIENT_LOCAL {
            if !self.exhausted {
                self.exhausted = true;
                tracing::error!(
                    next = self.next,
                    "object ids exhausted below the client-local range: requesting an immediate shutdown"
                );
                self.request(IdShutdown::Immediate);
            }
            return None;
        }
        let id = self.next;
        self.next += 1;
        if id >= OBJ_ID_WARN_THRESHOLD && !self.warned {
            self.warned = true;
            tracing::error!(
                id,
                "object id space is nearly exhausted: requesting a scheduled restart"
            );
            self.request(IdShutdown::Scheduled);
        }
        Some(ObjId(id))
    }

    fn request(&mut self, kind: IdShutdown) {
        if let Some(hook) = self.hook.as_mut() {
            hook.request_shutdown(kind);
        }
    }
}

/// The three arenas and the global `ObjId` index (15 §5.1). No ECS (D2): typed structs in `slotmap`s.
///
/// Iteration over objects in logic uses the arena (slot) order or the insertion-ordered sets of the grid and
/// the layers, never a `HashMap`. Every object is addressed by its `ObjId`; the keys are an implementation
/// detail of the store and of the phase driver.
pub struct ObjectStore {
    /// Players, NPCs, monsters and pets.
    pub movers: slotmap::SlotMap<MoverKey, Mover>,
    /// Ground items.
    pub items: slotmap::SlotMap<ItemKey, ItemDrop>,
    /// Chests, doors, traps and triggers.
    pub ctrls: slotmap::SlotMap<CtrlKey, Ctrl>,
    by_id: DetMap<ObjId, ObjRef>,
    ids: ObjIdAllocator,
}

impl Default for ObjectStore {
    fn default() -> Self {
        ObjectStore::new()
    }
}

impl std::fmt::Debug for ObjectStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ObjectStore")
            .field("movers", &self.movers.len())
            .field("items", &self.items.len())
            .field("ctrls", &self.ctrls.len())
            .field("next_id", &self.ids.peek())
            .finish()
    }
}

impl ObjectStore {
    /// An empty store whose first id is 1.
    pub fn new() -> ObjectStore {
        ObjectStore::with_allocator(ObjIdAllocator::new())
    }

    /// An empty store with a prepared allocator (tests seed it near the thresholds, the runtime installs the
    /// shutdown hook).
    pub fn with_allocator(ids: ObjIdAllocator) -> ObjectStore {
        ObjectStore {
            movers: slotmap::SlotMap::with_key(),
            items: slotmap::SlotMap::with_key(),
            ctrls: slotmap::SlotMap::with_key(),
            by_id: DetMap::default(),
            ids,
        }
    }

    /// The id allocator (the runtime sets its shutdown hook).
    pub fn ids_mut(&mut self) -> &mut ObjIdAllocator {
        &mut self.ids
    }

    /// The id the next created object receives.
    pub fn next_id(&self) -> u32 {
        self.ids.peek()
    }

    /// Number of created, not yet destroyed objects.
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// `true` when no object exists.
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// The arena reference of `id` (every created, not yet destroyed object, `prj.m_objmap`).
    pub fn lookup(&self, id: ObjId) -> Option<ObjRef> {
        self.by_id.get(&id).copied()
    }

    /// `true` while `id` names an object (also one flagged for deletion until `_delete` ran).
    pub fn contains(&self, id: ObjId) -> bool {
        self.by_id.contains_key(&id)
    }

    /// Every id in creation order.
    pub fn ids(&self) -> impl Iterator<Item = ObjId> + '_ {
        self.by_id.keys().copied()
    }

    /// The ids of every object that belongs to `(world, layer)` (linked, pending add or flagged for deletion),
    /// in creation order (the layer release of 15 §4.3).
    pub fn ids_in_layer(&self, world: WorldId, layer: LayerId) -> Vec<ObjId> {
        self.by_id
            .keys()
            .copied()
            .filter(|&id| {
                self.core(id)
                    .is_some_and(|c| c.world == world && c.layer == layer)
            })
            .collect()
    }

    /// The header of `id`.
    pub fn core(&self, id: ObjId) -> Option<&ObjCore> {
        match self.lookup(id)? {
            ObjRef::Mover(k) => self.movers.get(k).map(|m| &m.core),
            ObjRef::Item(k) => self.items.get(k).map(|i| &i.core),
            ObjRef::Ctrl(k) => self.ctrls.get(k).map(|c| &c.core),
        }
    }

    /// The header of `id`, mutably.
    pub fn core_mut(&mut self, id: ObjId) -> Option<&mut ObjCore> {
        match self.by_id.get(&id).copied()? {
            ObjRef::Mover(k) => self.movers.get_mut(k).map(|m| &mut m.core),
            ObjRef::Item(k) => self.items.get_mut(k).map(|i| &mut i.core),
            ObjRef::Ctrl(k) => self.ctrls.get_mut(k).map(|c| &mut c.core),
        }
    }

    /// The mover `id`.
    pub fn mover(&self, id: ObjId) -> Option<&Mover> {
        match self.lookup(id)? {
            ObjRef::Mover(k) => self.movers.get(k),
            _ => None,
        }
    }

    /// The mover `id`, mutably.
    pub fn mover_mut(&mut self, id: ObjId) -> Option<&mut Mover> {
        match self.by_id.get(&id).copied()? {
            ObjRef::Mover(k) => self.movers.get_mut(k),
            _ => None,
        }
    }

    /// The ground item `id`.
    pub fn item(&self, id: ObjId) -> Option<&ItemDrop> {
        match self.lookup(id)? {
            ObjRef::Item(k) => self.items.get(k),
            _ => None,
        }
    }

    /// The ctrl `id`.
    pub fn ctrl(&self, id: ObjId) -> Option<&Ctrl> {
        match self.lookup(id)? {
            ObjRef::Ctrl(k) => self.ctrls.get(k),
            _ => None,
        }
    }

    /// The ctrl `id`, mutably.
    pub fn ctrl_mut(&mut self, id: ObjId) -> Option<&mut Ctrl> {
        match self.by_id.get(&id).copied()? {
            ObjRef::Ctrl(k) => self.ctrls.get_mut(k),
            _ => None,
        }
    }

    /// `true` when `id` is a player mover.
    pub fn is_player(&self, id: ObjId) -> bool {
        self.mover(id).is_some_and(Mover::is_player)
    }

    /// The arena key of mover `id` (the phase driver works with keys).
    pub fn mover_key(&self, id: ObjId) -> Option<MoverKey> {
        match self.lookup(id)? {
            ObjRef::Mover(k) => Some(k),
            _ => None,
        }
    }

    /// Allocates the next id (`None` when the space below [`OBJ_ID_CLIENT_LOCAL`] is used up, 15 §5.2).
    pub(crate) fn alloc_id(&mut self) -> Option<ObjId> {
        self.ids.alloc()
    }

    /// Inserts a created mover (its `core.id` is already stamped).
    pub(crate) fn insert_mover(&mut self, m: Mover) -> ObjId {
        let id = m.core.id;
        let key = self.movers.insert(m);
        self.by_id.insert(id, ObjRef::Mover(key));
        id
    }

    /// Inserts a created ground item.
    pub(crate) fn insert_item(&mut self, d: ItemDrop) -> ObjId {
        let id = d.core.id;
        let key = self.items.insert(d);
        self.by_id.insert(id, ObjRef::Item(key));
        id
    }

    /// Inserts a created ctrl.
    pub(crate) fn insert_ctrl(&mut self, c: Ctrl) -> ObjId {
        let id = c.core.id;
        let key = self.ctrls.insert(c);
        self.by_id.insert(id, ObjRef::Ctrl(key));
        id
    }

    /// Removes the object from its arena and from the index; the id stays burned (15 §5.4 step 6). `false`
    /// when there was no such object.
    pub(crate) fn remove(&mut self, id: ObjId) -> bool {
        match self.by_id.shift_remove(&id) {
            Some(ObjRef::Mover(k)) => self.movers.remove(k).is_some(),
            Some(ObjRef::Item(k)) => self.items.remove(k).is_some(),
            Some(ObjRef::Ctrl(k)) => self.ctrls.remove(k).is_some(),
            None => false,
        }
    }
}
