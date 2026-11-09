//! Layers: the independent copies of a world (Plan/15 §4, WP-WORLD-02 types, WP-WORLD-07 behaviour).
//!
//! A world holds `BTreeMap<LayerId, Layer>`; layer 0 always exists. Every layer has its own grid, member set
//! and deferred queues, so objects of different layers never see each other (V4).
//!
//! * layer 0: the normal world, created at startup; layers `-1 .. -n`: the ticket copies of `layer.inc`
//!   ([`TicketLayers`], created at startup, never released); layers `> 0`: instances, housing rooms and guild
//!   houses created by doc 22 through [`Worlds::create_layer`] and released by [`Worlds::invalidate`] + the
//!   garbage collection after the last layer of a world's tick (`Worlds::layer_gc`);
//! * every layer gets its **own copy** of the `.dyo` NPCs and ctrls ([`Worlds::load_dyo`]), so the players of
//!   two layers never see each other;
//! * [`LayerDriveOut`] moves the players out of an invalidated layer: the default sends them to Madrigal
//!   (6983, 0, 3330), layer 0, with `Force`; doc 22 installs its own for housing, instances and guild houses
//!   (`GuildHouse` just `revalidate`s).
//!
//! The respawner (WP-WORLD-14) and the ctrl colliders and keys (WP-WORLD-18) are part of [`Layer`].

use super::Worlds;
use super::grid::Grid;
use super::replace::{ReplaceEntry, ReplaceKind};
use flyff_common::defs::world::WI_WORLD_MADRIGAL;
use flyff_common::det::DetSet;
use flyff_common::{LayerId, ObjId, WorldId};
use flyff_sim::collision::dynamic::DynamicColliders;
use std::collections::BTreeMap;
use std::sync::Arc;

/// What a layer is (15 §4.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerKind {
    /// Layer 0, and the GM layers of `/CreateLayer` (ids > 0).
    Normal,
    /// A ticket copy (`layer.inc`, ids `-1 ..= -n`).
    Ticket,
    /// An instance dungeon (layer = dungeon id).
    Instance,
    /// A housing room (layer = character id).
    MiniRoom,
    /// A guild house (layer = guild id).
    GuildHouse,
}

/// Whether a layer is in use or waits for its garbage collection (15 §4.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerState {
    /// In use.
    Live,
    /// `invalidate` was called: it is released once nobody is in it.
    Invalidated,
}

/// A queued death for the guild-siege point accounting (`m_aOnDie`; filled by doc 17, drained by doc 21).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OnDieEntry {
    /// The mover that died.
    pub victim: ObjId,
    /// Its killer, if known.
    pub attacker: Option<ObjId>,
}

/// The deferred queues of one layer (15 §5.3), drained by the phases of the tick.
#[derive(Clone, Debug, Default)]
pub struct LayerQueues {
    /// `m_apAddObjs`: objects waiting for `_add`.
    pub add: Vec<ObjId>,
    /// `m_apModifyLink`: objects whose cell may have changed (deduplicated by `ObjCore.pending_relink`).
    pub modify_link: Vec<ObjId>,
    /// `m_aOnDie`.
    pub on_die: Vec<OnDieEntry>,
    /// `m_aReplaceObj`.
    pub replace: Vec<ReplaceEntry>,
    /// `m_apDeleteObjs` (deduplicated by `ObjCore.deleted`).
    pub delete: Vec<ObjId>,
}

impl LayerQueues {
    /// `true` when every queue is empty.
    pub fn is_empty(&self) -> bool {
        self.add.is_empty()
            && self.modify_link.is_empty()
            && self.on_die.is_empty()
            && self.replace.is_empty()
            && self.delete.is_empty()
    }
}

/// One layer of a world (15 §4.1).
#[derive(Debug)]
pub struct Layer {
    /// The id.
    pub id: LayerId,
    /// The kind.
    pub kind: LayerKind,
    /// Live or invalidated.
    pub state: LayerState,
    /// The link-map grid.
    pub grid: Grid,
    /// The linked objects, iterated in insertion order by `_process`.
    pub members: DetSet<ObjId>,
    /// The deferred queues.
    pub queues: LayerQueues,
    /// The respawn state of this layer, parallel to the world's [`RespawnTable`](super::RespawnTable)
    /// (15 §15.1). A fresh layer starts at zero: its first respawn tick fills it.
    pub respawner: super::Respawner,
    /// The dynamic `OT_CTRL` colliders of this layer (15 §14.7, Plan/31 §10.10): registered at the `_add` of a
    /// ctrl, re-posed by [`Worlds::process_ctrl`](super::Worlds), removed at its destroy. Shared (`Arc`) with
    /// the sim calls of the layer's movers; only the ctrl code mutates it, with no clone outstanding.
    pub dyn_colliders: Arc<DynamicColliders>,
    /// `g_MapStrToObjId`, per layer (15 §14.6, FIX PL-151): ctrl key to the linked ctrl's id.
    pub ctrl_keys: BTreeMap<String, ObjId>,
}

impl Layer {
    /// A fresh live layer with an empty grid.
    pub fn new(id: LayerId, kind: LayerKind, grid: Grid) -> Layer {
        Layer {
            id,
            kind,
            state: LayerState::Live,
            grid,
            members: DetSet::default(),
            queues: LayerQueues::default(),
            respawner: super::Respawner::default(),
            dyn_colliders: Arc::new(DynamicColliders::new()),
            ctrl_keys: BTreeMap::new(),
        }
    }
}

/// `layer.inc`: how many ticket overflow layers (`-1 .. -n`) each world gets (15 §3.5 step 2, ref/06 §10). The
/// shipped file has Kebaras (2) and Cisland (3) with one layer each.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TicketLayers {
    layers: BTreeMap<WorldId, i32>,
}

impl TicketLayers {
    /// A table of `(world, n)` pairs.
    pub fn from_pairs(pairs: impl IntoIterator<Item = (WorldId, i32)>) -> TicketLayers {
        TicketLayers {
            layers: pairs.into_iter().collect(),
        }
    }

    /// The number of ticket layers of `world` (`CTicketProperty::GetExpanedLayer`), 0 when absent.
    pub fn expanded(&self, world: WorldId) -> i32 {
        self.layers.get(&world).copied().unwrap_or(0).max(0)
    }

    /// `true` without an entry.
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    /// Parses the text of `layer.inc`: `<WI_* define or number> <count>` lines; `//` comments. A define is
    /// resolved with `resolve` (the loaded define table); an unknown define or a bad number skips the line
    /// with a warning.
    pub fn parse(text: &str, resolve: impl Fn(&str) -> Option<u32>) -> TicketLayers {
        let mut out = TicketLayers::default();
        let mut toks: Vec<&str> = Vec::new();
        for line in text.lines() {
            let line = line.split("//").next().unwrap_or("");
            toks.extend(line.split_whitespace());
        }
        for pair in toks.chunks(2) {
            let [w, n] = pair else {
                tracing::warn!("layer.inc: a trailing token without a count");
                break;
            };
            let id = w.parse::<u32>().ok().or_else(|| resolve(w));
            match (id, n.parse::<i32>()) {
                (Some(id), Ok(n)) => {
                    out.layers.insert(WorldId(id), n);
                }
                _ => tracing::warn!("layer.inc: cannot read '{w} {n}'"),
            }
        }
        out
    }

    /// Reads `layer.inc` through the VFS; a missing file is an empty table.
    pub fn load(vfs: &flyff_vfs::Vfs, resolve: impl Fn(&str) -> Option<u32>) -> TicketLayers {
        match vfs.read_text_auto("layer.inc") {
            Ok(d) => TicketLayers::parse(&d.text, resolve),
            Err(_) => TicketLayers::default(),
        }
    }
}

/// Why [`Worlds::create_layer`] refused (15 §4.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LayerError {
    /// Layer 0 is created at startup and never again.
    #[error("layer 0 is reserved")]
    Reserved,
    /// The layer exists.
    #[error("the layer already exists")]
    Exists,
    /// The world is not loaded.
    #[error("the world is not loaded")]
    WorldMissing,
}

/// Moves a player out of an invalidated layer that is still occupied (15 §4.3; doc 22 implements the housing,
/// instance and guild-house variants). Called every tick while the layer is invalidated and occupied;
/// duplicate `replace` requests collapse.
pub trait LayerDriveOut: Send {
    /// Drives `player` out of `(world, layer)` of kind `kind`.
    fn drive_out(
        &mut self,
        worlds: &mut Worlds,
        world: WorldId,
        kind: LayerKind,
        layer: LayerId,
        player: ObjId,
    );
}

/// The default drive-out: `replace(WI_WORLD_MADRIGAL, (6983, 0, 3330), Force, layer 0)`; a guild house cancels
/// the garbage collection (`Invalidate(n, FALSE)`).
#[derive(Clone, Copy, Debug, Default)]
pub struct MadrigalDriveOut;

impl LayerDriveOut for MadrigalDriveOut {
    fn drive_out(
        &mut self,
        worlds: &mut Worlds,
        world: WorldId,
        kind: LayerKind,
        layer: LayerId,
        player: ObjId,
    ) {
        if kind == LayerKind::GuildHouse {
            worlds.revalidate(world, layer);
            return;
        }
        if let Err(e) = worlds.replace(
            player,
            WorldId(WI_WORLD_MADRIGAL),
            super::revival::MADRIGAL_DRIVE_OUT,
            LayerId(0),
            ReplaceKind::Force,
        ) {
            tracing::debug!(player = player.0, "layer drive-out refused: {e}");
        }
    }
}

/// The holder of the installed [`LayerDriveOut`] (`Worlds` is `Debug`, trait objects are not).
pub(crate) struct DriveOutSlot(pub Option<Box<dyn LayerDriveOut>>);

impl Default for DriveOutSlot {
    fn default() -> Self {
        DriveOutSlot(Some(Box::new(MadrigalDriveOut)))
    }
}

impl std::fmt::Debug for DriveOutSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DriveOutSlot(..)")
    }
}

impl Worlds {
    /// Installs doc 22's drive-out (replaces the Madrigal default).
    pub fn set_drive_out(&mut self, d: Box<dyn LayerDriveOut>) {
        self.drive_out.0 = Some(d);
    }

    /// `CWorld::CreateLayer(n)` (15 §4.2): `id == 0` is `Reserved`, an existing layer `Exists`, a missing world
    /// `WorldMissing`. A fresh grid, a fresh copy of every accepted `.dyo` object with the event flags **as
    /// they are now**, a fresh respawner (zero counters: the first respawn tick fills the layer), then the
    /// layer is live.
    pub fn create_layer(
        &mut self,
        world: WorldId,
        id: LayerId,
        kind: LayerKind,
        data: &flyff_data::GameData,
        flags: &dyn super::dyo::EventFlagQuery,
        rng: &mut dyn flyff_common::rng::GameRng,
    ) -> Result<(), LayerError> {
        if id.0 == 0 {
            return Err(LayerError::Reserved);
        }
        let w = self.world(world).ok_or(LayerError::WorldMissing)?;
        if w.layers.contains_key(&id) {
            return Err(LayerError::Exists);
        }
        if !self.add_layer(world, id, kind) {
            return Err(LayerError::Exists);
        }
        self.load_dyo(world, id, data, flags, rng, false);
        Ok(())
    }

    /// `CLayeredLinkMap::Invalidate(n)`: the layer is released by the garbage collection once nobody is in it.
    /// Only layers `> 0` can be invalidated; returns `false` otherwise.
    pub fn invalidate(&mut self, world: WorldId, id: LayerId) -> bool {
        if id.0 <= 0 {
            return false;
        }
        match self.layer_mut(world, id) {
            Some(l) => {
                l.state = LayerState::Invalidated;
                true
            }
            None => false,
        }
    }

    /// `Invalidate(n, FALSE)`: cancels the garbage collection.
    pub fn revalidate(&mut self, world: WorldId, id: LayerId) -> bool {
        match self.layer_mut(world, id) {
            Some(l) => {
                l.state = LayerState::Live;
                true
            }
            None => false,
        }
    }

    /// `CWorld::DeleteLayerControls(n)`: deletes every ctrl of the layer (`_delete` removes them).
    pub fn delete_layer_ctrls(&mut self, world: WorldId, id: LayerId) {
        let ctrls: Vec<ObjId> = self
            .layer(world, id)
            .map(|l| {
                l.members
                    .iter()
                    .copied()
                    .chain(l.queues.add.iter().copied())
                    .filter(|&o| self.store.ctrl(o).is_some())
                    .collect()
            })
            .unwrap_or_default();
        for o in ctrls {
            self.delete(o);
        }
    }

    /// `HasNobody(n)` (15 §4.3): no player linked in the layer, none in its add queue, and no replace entry of
    /// **any** world's queues targets `(world, id)`.
    pub fn has_nobody(&self, world: WorldId, id: LayerId) -> bool {
        let Some(l) = self.layer(world, id) else {
            return true;
        };
        if l.members.iter().any(|&o| self.store.is_player(o))
            || l.queues.add.iter().any(|&o| self.store.is_player(o))
        {
            return false;
        }
        !self.worlds_iter().any(|w| {
            w.layers.values().any(|l| {
                l.queues
                    .replace
                    .iter()
                    .any(|e| e.world == world && e.layer == id)
            })
        })
    }

    /// The players inside a layer (linked or pending add), ascending by insertion order.
    fn players_in(&self, world: WorldId, id: LayerId) -> Vec<ObjId> {
        let Some(l) = self.layer(world, id) else {
            return Vec::new();
        };
        l.members
            .iter()
            .copied()
            .chain(l.queues.add.iter().copied())
            .filter(|&o| self.store.is_player(o))
            .collect()
    }

    /// `ReleaseLayer(n)`: destroys every object of the layer at once (the normal destroy path minus `DelObj`:
    /// nobody can see them), drops the grid and the queues and removes the layer. Registered NPCs are
    /// unregistered through the hook.
    pub(crate) fn release_layer(
        &mut self,
        world: WorldId,
        id: LayerId,
        env: &mut super::PhaseEnv<'_>,
    ) {
        if id.0 == 0 {
            return;
        }
        let ids = self.store.ids_in_layer(world, id);
        for obj in ids {
            let registered = self
                .store
                .core(obj)
                .is_some_and(|c| matches!(c.link, super::LinkState::Linked { .. }))
                && self
                    .store
                    .mover(obj)
                    .and_then(|m| m.npc())
                    .is_some_and(|n| !n.menus.is_empty());
            if registered {
                self.npc_checker.on_npc_removed(obj);
                env.hooks.on_npc_removed(obj);
            }
            self.pending_self.remove(&obj);
            self.store.remove(obj);
        }
        if let Some(w) = self.world_mut(world) {
            w.layers.remove(&id);
        }
        self.stats.layers_released += 1;
    }

    /// The layer garbage collection of one world, after its last layer's `_delete` (15 §4.3, 01 §6.3): for
    /// each invalidated layer `> 0`, ascending, release it when nobody is in or entering it, else drive the
    /// players out (retried every tick).
    pub(crate) fn layer_gc(&mut self, world: WorldId, env: &mut super::PhaseEnv<'_>) {
        let candidates: Vec<(LayerId, LayerKind)> = self
            .world(world)
            .map(|w| {
                w.layers
                    .iter()
                    .filter(|(id, l)| id.0 > 0 && l.state == LayerState::Invalidated)
                    .map(|(id, l)| (*id, l.kind))
                    .collect()
            })
            .unwrap_or_default();
        for (id, kind) in candidates {
            if self.has_nobody(world, id) {
                self.release_layer(world, id, env);
                continue;
            }
            let players = self.players_in(world, id);
            let Some(mut d) = self.drive_out.0.take() else {
                continue;
            };
            for p in players {
                d.drive_out(self, world, kind, id, p);
            }
            self.drive_out.0 = Some(d);
        }
    }
}
