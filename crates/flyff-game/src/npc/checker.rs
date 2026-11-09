//! `NpcChecker`: the NPC-menu proximity helper, the single implementation of VR-012 and VR-013 (Plan/20 §8.1,
//! D20-10, WP-NPC-19; src:WORLDSERVER/npchecker.cpp:56-72, `MAX_LEN_MOVER_MENU 1024`).
//!
//! * [`NpcChecker::close_npc`] (VR-012): is there an NPC that offers the menu `menu` within
//!   `gameplay.npc_menu_range` (32 units) of `pos` — `‖Δ‖² < range²`, strict, **3-D**, in the **same world and
//!   layer** (the original checked the world only; the layer is the rewrite's addition)? Like the original,
//!   *any* NPC offering the menu suffices, not necessarily the one whose window is open. Returns the first one
//!   in registration order.
//! * [`NpcChecker::any_menu_npc_within_xz`] (VR-013): is there an NPC with at least one menu in the same world
//!   and layer whose **XZ** distance² to `pos` is `< radius²` (strict; the personal-shop exclusion radius
//!   `gameplay.vendor_npc_min_dist`, 3 units, whatever the height)?
//!
//! The index has one source: the world driver's placement hooks. `Worlds` calls
//! [`NpcChecker::on_npc_added`] once at `_add` for an NPC whose `NpcDef.menus` is non-empty, and
//! [`NpcChecker::on_npc_removed`] at the object's destruction or the release of its layer, so an NPC without a
//! menu is never registered and never returned. NPC positions are read when asked ([`MoverView`]): NPCs walk.
//!
//! Every NPC-service handler of docs 17, 19, 21 and 22 calls these two functions first (call-site table in
//! Plan/20 §8.1); `Worlds::close_npc` and `Worlds::any_menu_npc_within_xz` are the shorthands that pass the
//! object store.

use crate::world::object::ObjectStore;
use flyff_common::det::{DetMap, DetSet};
use flyff_common::math::Vec3;
use flyff_common::{LayerId, ObjId, WorldId};
use flyff_data::types::MenuId;

/// `MAX_MOVER_MENU`: the menu ids are below this (`neuz.MAX_MOVER_MENU` = 175).
pub const MAX_MOVER_MENU: usize = flyff_common::defs::neuz::MAX_MOVER_MENU as usize;

/// `gameplay.npc_menu_range` default (32 units; `MAX_LEN_MOVER_MENU 1024` squared).
pub const DEFAULT_NPC_MENU_RANGE: f32 = 32.0;

/// Where an NPC is right now.
pub trait MoverView {
    /// The current position of the object, `None` when it does not exist.
    fn position(&self, id: ObjId) -> Option<Vec3>;
}

impl MoverView for ObjectStore {
    fn position(&self, id: ObjId) -> Option<Vec3> {
        self.core(id).map(|c| c.pos)
    }
}

/// A fixed table of positions (tests, tools).
impl MoverView for std::collections::BTreeMap<ObjId, Vec3> {
    fn position(&self, id: ObjId) -> Option<Vec3> {
        self.get(&id).copied()
    }
}

#[derive(Clone, Debug)]
struct Entry {
    world: WorldId,
    layer: LayerId,
    menus: Vec<MenuId>,
}

/// The NPC-menu index (see the module documentation).
#[derive(Clone, Debug)]
pub struct NpcChecker {
    /// Per menu id: the NPCs that offer it, per `(world, layer)`, in registration order.
    by_menu: Vec<DetMap<(WorldId, LayerId), DetSet<ObjId>>>,
    /// Every registered NPC (the NPCs with at least one menu), in registration order.
    npcs: DetMap<ObjId, Entry>,
    /// `gameplay.npc_menu_range`.
    range: f32,
}

impl Default for NpcChecker {
    fn default() -> Self {
        Self::new(DEFAULT_NPC_MENU_RANGE)
    }
}

impl NpcChecker {
    /// An empty index with the NPC-menu range `range` (`gameplay.npc_menu_range`).
    pub fn new(range: f32) -> NpcChecker {
        NpcChecker {
            by_menu: (0..MAX_MOVER_MENU).map(|_| DetMap::default()).collect(),
            npcs: DetMap::default(),
            range,
        }
    }

    /// Sets `gameplay.npc_menu_range` (startup class: `Game::new` does it once).
    pub fn set_range(&mut self, range: f32) {
        self.range = range;
    }

    /// The NPC-menu range in force.
    pub fn range(&self) -> f32 {
        self.range
    }

    /// Doc 15's placement hook (WP-WORLD-08): `npc` was linked into `(world, layer)` and offers `menus`. Menu
    /// ids outside `0..MAX_MOVER_MENU` are ignored; an NPC without any menu is not registered. Registering an
    /// NPC again replaces its entry.
    pub fn on_npc_added(&mut self, npc: ObjId, world: WorldId, layer: LayerId, menus: &[MenuId]) {
        self.on_npc_removed(npc);
        let mut valid: Vec<MenuId> = menus
            .iter()
            .copied()
            .filter(|m| (*m as usize) < MAX_MOVER_MENU)
            .collect();
        valid.sort_unstable();
        valid.dedup();
        if valid.is_empty() {
            return;
        }
        for &m in &valid {
            if let Some(slot) = self.by_menu.get_mut(m as usize) {
                slot.entry((world, layer)).or_default().insert(npc);
            }
        }
        self.npcs.insert(
            npc,
            Entry {
                world,
                layer,
                menus: valid,
            },
        );
    }

    /// Doc 15's removal hook: the object was destroyed, or its layer was released.
    pub fn on_npc_removed(&mut self, npc: ObjId) {
        let Some(entry) = self.npcs.shift_remove(&npc) else {
            return;
        };
        for m in entry.menus {
            let Some(slot) = self.by_menu.get_mut(m as usize) else {
                continue;
            };
            let key = (entry.world, entry.layer);
            if let Some(set) = slot.get_mut(&key) {
                set.shift_remove(&npc);
                if set.is_empty() {
                    slot.shift_remove(&key);
                }
            }
        }
    }

    /// VR-012: the first NPC (in registration order) that offers `menu` in `(world, layer)` and stands closer
    /// than the range to `pos` (`‖Δ‖² < range²`, 3-D, strict). `None` when there is none, when `menu` is not a
    /// valid menu id, or when the NPCs are in another world or layer.
    pub fn close_npc(
        &self,
        menu: MenuId,
        world: WorldId,
        layer: LayerId,
        pos: Vec3,
        movers: &dyn MoverView,
    ) -> Option<ObjId> {
        let ids = self.by_menu.get(menu as usize)?.get(&(world, layer))?;
        let limit = self.range * self.range;
        ids.iter().copied().find(|&id| {
            movers
                .position(id)
                .is_some_and(|p| (p - pos).length_squared() < limit)
        })
    }

    /// VR-013: `true` when an NPC with at least one menu stands in `(world, layer)` with an XZ distance² to
    /// `pos` below `radius²` (strict; the height is ignored).
    pub fn any_menu_npc_within_xz(
        &self,
        world: WorldId,
        layer: LayerId,
        pos: Vec3,
        radius: f32,
        movers: &dyn MoverView,
    ) -> bool {
        let limit = radius * radius;
        self.npcs.iter().any(|(&id, e)| {
            e.world == world
                && e.layer == layer
                && movers.position(id).is_some_and(|p| {
                    let (dx, dz) = (p.x - pos.x, p.z - pos.z);
                    dx * dx + dz * dz < limit
                })
        })
    }

    /// `true` when `npc` is registered (it has a menu and was added).
    pub fn contains(&self, npc: ObjId) -> bool {
        self.npcs.contains_key(&npc)
    }

    /// The registered NPCs.
    pub fn len(&self) -> usize {
        self.npcs.len()
    }

    /// `true` when no NPC is registered.
    pub fn is_empty(&self) -> bool {
        self.npcs.is_empty()
    }

    /// How many NPCs offer `menu` in `(world, layer)`.
    pub fn count_with_menu(&self, menu: MenuId, world: WorldId, layer: LayerId) -> usize {
        self.by_menu
            .get(menu as usize)
            .and_then(|m| m.get(&(world, layer)))
            .map_or(0, DetSet::len)
    }
}
