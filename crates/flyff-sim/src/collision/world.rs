//! `CollisionWorld`: one per world folder, shared by all world ids and layers that use it
//! (Plan/31 §10.1; WP-SIM-02).
//!
//! The world owns one [`LandSlot`] per land: `Missing` (no `.lnd`: height 0, attribute NONE, no
//! objects — the server's memset-0 rule), `Pending` (client: not loaded yet) or `Loaded`. Terrain
//! queries live in [`super::terrain`], the static link-map walk in [`super::linkmap`], world queries in
//! [`super::query`].

use super::chunk::{LandChunk, MAP_SIZE, decode_chunk};
use super::linkmap::StaticIndex;
use crate::types::SimError;
use core::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// `world_max_height`: the flight ceiling (`src:_Common/World.cpp:90`).
pub const WORLD_MAX_HEIGHT: f32 = 200.0;
/// `world_min_height`: the cloud floor (`src:_Common/World.cpp:91`).
pub const WORLD_MIN_HEIGHT: f32 = 85.0;
/// `WLD_EXTRA_WIDTH`: the border that `clip_x`/`clip_z` keep movers away from (`World.h:34`).
pub const WLD_EXTRA_WIDTH: f32 = 10.0;
/// Largest supported world edge in lands (keeps every link cell coordinate within `u16`).
pub const MAX_WORLD_LANDS: u32 = 256;

/// The state of one land of a [`CollisionWorld`].
#[derive(Clone, Debug)]
pub enum LandSlot {
    /// No `.lnd` file: height 0, attribute NONE, no objects (server view; Plan/24 §10.7).
    Missing,
    /// Client: the chunk has not been loaded yet. Queries behave as "flat at the querying mover's y"
    /// and raise `SimEvent::ChunkMissing` once (Plan/31 §10.1).
    Pending,
    /// A decoded chunk.
    Loaded(Arc<LandChunk>),
}

/// Static description of a world (`set_world`): the `.wld` values the sim needs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WorldDesc {
    /// World units per heightmap cell (`MPU`: 1, 2 or 4).
    pub mpu: u32,
    /// World width in lands.
    pub lands_w: u32,
    /// World height in lands.
    pub lands_h: u32,
    /// `.wld` `fly` flag: flying allowed.
    pub fly_allowed: bool,
    /// `WI_WORLD_MINIROOM` or a guild-house world: ctrls take part in under-collision.
    pub housing: bool,
}

/// Initial state of the lands of a new world.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InitialLands {
    /// Server: lands without a loaded chunk have no file ([`LandSlot::Missing`]).
    Missing,
    /// Client: lands stream in ([`LandSlot::Pending`]).
    Pending,
}

/// The collision world (Plan/31 §10.1).
#[derive(Debug)]
pub struct CollisionWorld {
    /// World units per heightmap cell.
    pub mpu: u32,
    /// World width in lands.
    pub lands_w: u32,
    /// World height in lands.
    pub lands_h: u32,
    /// `.wld` `fly` flag.
    pub fly_allowed: bool,
    /// Flight ceiling (`200.0`).
    pub max_height: f32,
    /// Cloud floor (`85.0`).
    pub min_height: f32,
    /// Housing world: ctrls take part in under-collision.
    pub housing: bool,
    pub(crate) lands: Vec<LandSlot>,
    /// "A `ChunkMissing` event was already raised for this pending land" latch, per land.
    pub(crate) pending_notified: Vec<AtomicBool>,
    pub(crate) static_index: StaticIndex,
}

impl CollisionWorld {
    /// Creates a world whose lands are all in the `initial` state.
    pub fn new(desc: WorldDesc, initial: InitialLands) -> Result<CollisionWorld, SimError> {
        if !matches!(desc.mpu, 1 | 2 | 4) {
            return Err(SimError::BadWorld("mpu must be 1, 2 or 4"));
        }
        if desc.lands_w == 0
            || desc.lands_h == 0
            || desc.lands_w > MAX_WORLD_LANDS
            || desc.lands_h > MAX_WORLD_LANDS
        {
            return Err(SimError::BadWorld("land counts must be 1..=256"));
        }
        let n = (desc.lands_w * desc.lands_h) as usize;
        let slot = match initial {
            InitialLands::Missing => LandSlot::Missing,
            InitialLands::Pending => LandSlot::Pending,
        };
        let mut w = CollisionWorld {
            mpu: desc.mpu,
            lands_w: desc.lands_w,
            lands_h: desc.lands_h,
            fly_allowed: desc.fly_allowed,
            max_height: WORLD_MAX_HEIGHT,
            min_height: WORLD_MIN_HEIGHT,
            housing: desc.housing,
            lands: vec![slot; n],
            pending_notified: (0..n).map(|_| AtomicBool::new(false)).collect(),
            static_index: StaticIndex::new(desc.mpu, desc.lands_w, desc.lands_h),
        };
        w.static_index.rebuild(&w.lands);
        Ok(w)
    }

    /// The description this world was created from.
    pub fn desc(&self) -> WorldDesc {
        WorldDesc {
            mpu: self.mpu,
            lands_w: self.lands_w,
            lands_h: self.lands_h,
            fly_allowed: self.fly_allowed,
            housing: self.housing,
        }
    }

    /// Heightmap cells along x (`WORLD_WIDTH = 128 * lands_w`).
    #[inline]
    pub fn world_width_cells(&self) -> i32 {
        (MAP_SIZE as i32) * self.lands_w as i32
    }

    /// Heightmap cells along z (`WORLD_HEIGHT`).
    #[inline]
    pub fn world_height_cells(&self) -> i32 {
        (MAP_SIZE as i32) * self.lands_h as i32
    }

    /// Index of land `(lx, lz)` (`lx + lz * lands_w`) or `None` when outside the world.
    #[inline]
    pub fn land_index(&self, lx: u32, lz: u32) -> Option<usize> {
        if lx < self.lands_w && lz < self.lands_h {
            Some((lx + lz * self.lands_w) as usize)
        } else {
            None
        }
    }

    /// The slot of land `(lx, lz)`.
    #[inline]
    pub fn slot(&self, lx: u32, lz: u32) -> Option<&LandSlot> {
        self.lands.get(self.land_index(lx, lz)?)
    }

    /// The slot with index `idx` (`lx + lz * lands_w`).
    #[inline]
    pub(crate) fn slot_by_index(&self, idx: usize) -> Option<&LandSlot> {
        self.lands.get(idx)
    }

    /// The loaded chunk of land `(lx, lz)`.
    pub fn chunk(&self, lx: u32, lz: u32) -> Option<&Arc<LandChunk>> {
        match self.slot(lx, lz)? {
            LandSlot::Loaded(c) => Some(c),
            _ => None,
        }
    }

    /// Number of loaded lands.
    pub fn loaded_count(&self) -> usize {
        self.lands
            .iter()
            .filter(|s| matches!(s, LandSlot::Loaded(_)))
            .count()
    }

    /// Checks a chunk's header against the world description (Plan/31 §10.2 C1).
    fn check_header(&self, c: &LandChunk) -> Result<usize, SimError> {
        if u32::from(c.lands_w) != self.lands_w
            || u32::from(c.lands_h) != self.lands_h
            || u32::from(c.mpu) != self.mpu
        {
            return Err(SimError::ChunkMismatch(
                "lands_w/lands_h/mpu differ from the world",
            ));
        }
        self.land_index(u32::from(c.land_x), u32::from(c.land_z))
            .ok_or(SimError::LandOutOfRange)
    }

    /// Installs an already decoded chunk into its land (replacing any previous slot content).
    pub fn insert_chunk(&mut self, chunk: Arc<LandChunk>) -> Result<(), SimError> {
        let idx = self.check_header(&chunk)?;
        if let Some(slot) = self.lands.get_mut(idx) {
            *slot = LandSlot::Loaded(chunk);
        }
        self.reset_latch(idx);
        self.static_index.rebuild(&self.lands);
        Ok(())
    }

    /// Decodes `bytes` and installs the chunk as land `(lx, lz)` (`SimWorld.load_chunk`): rejects a chunk
    /// whose `land_x/land_z` differ from `(lx, lz)` or whose `lands_w/lands_h/mpu` differ from the world.
    pub fn load_chunk(&mut self, lx: u32, lz: u32, bytes: &[u8]) -> Result<(), SimError> {
        let chunk = decode_chunk(bytes)?;
        if u32::from(chunk.land_x) != lx || u32::from(chunk.land_z) != lz {
            return Err(SimError::ChunkMismatch(
                "land_x/land_z differ from the requested land",
            ));
        }
        self.insert_chunk(Arc::new(chunk))
    }

    /// Replaces land `(lx, lz)` by `Missing` (no file) or `Pending` (client streaming) and drops its
    /// chunk (`unload_chunk`).
    pub fn set_slot(&mut self, lx: u32, lz: u32, slot: InitialLands) -> Result<(), SimError> {
        let idx = self.land_index(lx, lz).ok_or(SimError::LandOutOfRange)?;
        if let Some(s) = self.lands.get_mut(idx) {
            *s = match slot {
                InitialLands::Missing => LandSlot::Missing,
                InitialLands::Pending => LandSlot::Pending,
            };
        }
        self.reset_latch(idx);
        self.static_index.rebuild(&self.lands);
        Ok(())
    }

    fn reset_latch(&self, idx: usize) {
        if let Some(l) = self.pending_notified.get(idx) {
            l.store(false, Ordering::Relaxed);
        }
    }

    /// The first touch of a pending land returns `true` once per land (until the slot changes).
    pub(crate) fn first_pending_touch(&self, idx: usize) -> bool {
        match self.pending_notified.get(idx) {
            Some(l) => !l.swap(true, Ordering::Relaxed),
            None => false,
        }
    }

    /// `(lx, lz)` of land index `idx`.
    pub(crate) fn land_coords(&self, idx: usize) -> (u16, u16) {
        let w = self.lands_w.max(1) as usize;
        // BOUNDS: lands_w and lands_h are at most MAX_WORLD_LANDS = 256, so both fit u16
        ((idx % w) as u16, (idx / w) as u16)
    }
}
