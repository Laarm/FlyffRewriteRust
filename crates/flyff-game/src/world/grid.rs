//! The link-map grid of a layer and its range queries (Plan/15 §6.1, §6.4, WP-WORLD-03).
//!
//! Only level 0 of the original 7-level link map exists: every dynamic object and every player is level 0
//! (`fDynamicGrid` is 512 m everywhere, ref/06 §8.1); static scenery lives in the `CollisionWorld`. A cell is
//! `8 * mpu` metres wide; each cell holds two lists, the players (`linkPlayer`) and everything else
//! (`linkDynamic`).
//!
//! **List order** (KQ-13 "first match", 24 §10.7.6 rule 3): the lists reproduce the legacy intrusive list of
//! `InsertObjLink`: the head stays first and every later insertion goes directly after it. Inserting A, B, C, D
//! yields A, D, C, B; removing the head makes its successor the head (D, C, B); inserting E then gives
//! D, E, C, B. Target scans that take the "first match" (18 OQ7) depend on this order.
//!
//! **Storage**: a dense `u32` index per cell (4 B, 0 = empty) pointing into a slab of [`Cell`]s that exist
//! only while a cell holds an object, so Madrigal's 230 400 cells cost under 1 MB per layer (15 §6.1 allows
//! any storage with the legacy iteration order).

use super::catalog::{CELL_UNITS, GridGeom};
use super::object::CellIdx;
use flyff_common::ObjId;
use flyff_common::math::Vec3;
use smallvec::SmallVec;

/// An inclusive rectangle of cells; empty when `xmin > xmax` or `zmin > zmax`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellRect {
    /// First column.
    pub xmin: i32,
    /// Last column (inclusive).
    pub xmax: i32,
    /// First row.
    pub zmin: i32,
    /// Last row (inclusive).
    pub zmax: i32,
}

impl CellRect {
    /// `true` when the rectangle contains no cell.
    pub fn is_empty(&self) -> bool {
        self.xmin > self.xmax || self.zmin > self.zmax
    }

    /// `true` when `(x, z)` lies inside.
    pub fn contains(&self, x: i32, z: i32) -> bool {
        (self.xmin..=self.xmax).contains(&x) && (self.zmin..=self.zmax).contains(&z)
    }

    /// Number of cells.
    pub fn area(&self) -> usize {
        if self.is_empty() {
            0
        } else {
            ((self.xmax - self.xmin + 1) as usize) * ((self.zmax - self.zmin + 1) as usize)
        }
    }
}

/// Which link set a range callback walks (15 §6.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkSet {
    /// `linkPlayer`: the players.
    Players,
    /// `linkDynamic`: NPCs, monsters, pets, items, ctrls.
    Others,
    /// Players first, then the others, per cell.
    Both,
}

/// The distance a radius query measures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Metric {
    /// Euclidean distance in 3-D.
    D3,
    /// Distance in the x/z plane.
    Xz,
}

/// One cell: the two lists in the legacy link-list order (see the module docs).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cell {
    /// Players (`linkPlayer`).
    pub players: SmallVec<[ObjId; 2]>,
    /// Everything else (`linkDynamic`).
    pub others: SmallVec<[ObjId; 4]>,
}

impl Cell {
    fn is_empty(&self) -> bool {
        self.players.is_empty() && self.others.is_empty()
    }
}

/// Inserts `id` into a legacy-ordered list: first element if empty, else directly after the head.
fn link_insert<A: smallvec::Array<Item = ObjId>>(list: &mut SmallVec<A>, id: ObjId) {
    if list.is_empty() {
        list.push(id);
    } else {
        list.insert(1, id);
    }
}

/// `CObj::GetObjPtr` cell of a position (ref/06 §8.3): `int(x / mpu / 8)`, `int(z / mpu / 8)`.
pub fn cell_of(g: &GridGeom, p: Vec3) -> CellIdx {
    let mpu = g.mpu as f32;
    let unit = CELL_UNITS as f32;
    CellIdx {
        x: ((p.x / mpu) / unit) as i32,
        z: ((p.z / mpu) / unit) as i32,
    }
}

/// The window `W(c)` of a cell (15 §6.1): `|x - c.x| <= R`, `|z - c.z| <= R` clamped to the map. The window is
/// computed in 2-D, so it never wraps into the previous or next row (FIX W8, PL-152).
pub fn window(g: &GridGeom, c: CellIdx) -> CellRect {
    CellRect {
        xmin: c.x.saturating_sub(g.radius).max(0),
        xmax: c.x.saturating_add(g.radius).min(g.cells.0 - 1),
        zmin: c.z.saturating_sub(g.radius).max(0),
        zmax: c.z.saturating_add(g.radius).min(g.cells.1 - 1),
    }
}

/// The cells `FOR_LINKMAP` scans for a position and an `nRange` in heightmap units at level 0 (ref/06 §8.4,
/// `World.h:790-863`); all integer arithmetic truncates toward zero like C:
///
/// ```text
/// link = int(pos / mpu); cell_start = (link / 8) * 8 * mpu; near_upper = int(pos) - cell_start > 4 * mpu
/// min = max(0, (link - n_range) / 8 + (near_upper - 1));  max = min(cells - 1, (link + n_range) / 8 + near_upper)
/// ```
pub fn legacy_cells(g: &GridGeom, pos: Vec3, n_range: i32) -> CellRect {
    let mpu = g.mpu as i32;
    let unit = CELL_UNITS as i32;
    let axis = |p: f32, cells: i32| -> (i32, i32) {
        let link = (p / g.mpu as f32) as i32;
        let start = (link / unit) * unit * mpu;
        let upper = i32::from((p as i32) - start > unit * mpu / 2);
        let min = ((link - n_range) / unit + (upper - 1)).max(0);
        let max = ((link + n_range) / unit + upper).min(cells - 1);
        (min, max)
    };
    let (xmin, xmax) = axis(pos.x, g.cells.0);
    let (zmin, zmax) = axis(pos.z, g.cells.1);
    CellRect {
        xmin,
        xmax,
        zmin,
        zmax,
    }
}

/// `SendDamageAround`: the link range for an AoE radius in metres (`<= 4 -> 4, <= 8 -> 8, <= 16 -> 16,
/// <= 32 -> 32, else 64`, ref/06 §8.4).
pub fn aoe_n_range(radius: f32) -> i32 {
    match radius {
        r if r <= 4.0 => 4,
        r if r <= 8.0 => 8,
        r if r <= 16.0 => 16,
        r if r <= 32.0 => 32,
        _ => 64,
    }
}

/// `ApplySkillRegion`: the link range for a skill-region radius (`<= 4 -> 4, <= 8 -> 8, <= 16 -> 16, else 32`).
pub fn skill_region_n_range(radius: f32) -> i32 {
    match radius {
        r if r <= 4.0 => 4,
        r if r <= 8.0 => 8,
        r if r <= 16.0 => 16,
        _ => 32,
    }
}

/// The grid of one layer (15 §6.1).
#[derive(Clone, Debug)]
pub struct Grid {
    geom: GridGeom,
    /// Row-major (`z` outer): 0 = no cell, else `slab` index + 1.
    index: Vec<u32>,
    slab: Vec<Cell>,
    free: Vec<u32>,
    objects: usize,
}

impl Grid {
    /// An empty grid of the given geometry.
    pub fn new(geom: GridGeom) -> Grid {
        Grid {
            index: vec![0; geom.cell_count()],
            slab: Vec::new(),
            free: Vec::new(),
            objects: 0,
            geom,
        }
    }

    /// The geometry.
    pub fn geom(&self) -> &GridGeom {
        &self.geom
    }

    /// Number of linked objects (players and others).
    pub fn len(&self) -> usize {
        self.objects
    }

    /// `true` when nothing is linked.
    pub fn is_empty(&self) -> bool {
        self.objects == 0
    }

    /// Approximate heap bytes (the dense index plus the slab).
    pub fn heap_bytes(&self) -> usize {
        self.index.len() * size_of::<u32>() + self.slab.capacity() * size_of::<Cell>()
    }

    fn pos(&self, c: CellIdx) -> Option<usize> {
        if c.x < 0 || c.z < 0 || c.x >= self.geom.cells.0 || c.z >= self.geom.cells.1 {
            return None;
        }
        Some((c.z as usize) * (self.geom.cells.0 as usize) + c.x as usize)
    }

    /// The cell at `c` (`None` when it holds nothing or lies outside the map).
    pub fn cell(&self, c: CellIdx) -> Option<&Cell> {
        let p = self.pos(c)?;
        let i = *self.index.get(p)?;
        self.slab.get((i as usize).checked_sub(1)?)
    }

    /// Links `id` into cell `c` (a player into the player list). `false` when `c` is outside the map.
    pub fn insert(&mut self, c: CellIdx, id: ObjId, player: bool) -> bool {
        let Some(p) = self.pos(c) else {
            return false;
        };
        let Some(slot) = self.index.get(p).copied() else {
            return false;
        };
        let slab_index = if slot == 0 {
            let idx = match self.free.pop() {
                Some(i) => i,
                None => {
                    self.slab.push(Cell::default());
                    self.slab.len() as u32 - 1
                }
            };
            if let Some(e) = self.index.get_mut(p) {
                *e = idx + 1;
            }
            idx
        } else {
            slot - 1
        };
        if let Some(cell) = self.slab.get_mut(slab_index as usize) {
            if player {
                link_insert(&mut cell.players, id);
            } else {
                link_insert(&mut cell.others, id);
            }
            self.objects += 1;
            true
        } else {
            false
        }
    }

    /// Unlinks `id` from cell `c`. `false` when it was not there.
    pub fn remove(&mut self, c: CellIdx, id: ObjId, player: bool) -> bool {
        let Some(p) = self.pos(c) else {
            return false;
        };
        let Some(slot) = self.index.get(p).copied() else {
            return false;
        };
        let Some(si) = slot.checked_sub(1) else {
            return false;
        };
        let Some(cell) = self.slab.get_mut(si as usize) else {
            return false;
        };
        let found = if player {
            cell.players.iter().position(|&x| x == id).map(|i| {
                cell.players.remove(i);
            })
        } else {
            cell.others.iter().position(|&x| x == id).map(|i| {
                cell.others.remove(i);
            })
        };
        if found.is_none() {
            return false;
        }
        self.objects -= 1;
        if cell.is_empty() {
            if let Some(e) = self.index.get_mut(p) {
                *e = 0;
            }
            self.free.push(si);
        }
        true
    }

    /// Calls `f(cell index, cell)` for every non-empty cell of `rect` in scan order: rows `z` ascending, then
    /// `x` ascending. The rectangle is clamped to the map.
    pub fn for_each_cell(&self, rect: CellRect, mut f: impl FnMut(CellIdx, &Cell)) {
        let r = CellRect {
            xmin: rect.xmin.max(0),
            xmax: rect.xmax.min(self.geom.cells.0 - 1),
            zmin: rect.zmin.max(0),
            zmax: rect.zmax.min(self.geom.cells.1 - 1),
        };
        if r.is_empty() {
            return;
        }
        for z in r.zmin..=r.zmax {
            let row = (z as usize) * (self.geom.cells.0 as usize);
            for x in r.xmin..=r.xmax {
                let slot = self.index.get(row + x as usize).copied().unwrap_or(0);
                if let Some(cell) = slot.checked_sub(1).and_then(|i| self.slab.get(i as usize)) {
                    f(CellIdx { x, z }, cell);
                }
            }
        }
    }

    /// The non-empty cells of row `z` with `xmin <= x <= xmax` (clamped to the map), as `(x, cell)` in ascending `x`.
    /// A row is contiguous in the dense index, so this is a straight scan over `u32`s that skips the empty cells
    /// without touching the slab (the admin map's collector walks rows with it, 25 §3.15).
    pub fn row_cells(
        &self,
        z: i32,
        xmin: i32,
        xmax: i32,
    ) -> impl Iterator<Item = (i32, &Cell)> + '_ {
        let xmin = xmin.max(0);
        let xmax = xmax.min(self.geom.cells.0 - 1);
        let span = if z >= 0 && z < self.geom.cells.1 && xmin <= xmax {
            let row = (z as usize) * (self.geom.cells.0 as usize);
            self.index.get(row + xmin as usize..=row + xmax as usize)
        } else {
            None
        };
        span.unwrap_or(&[])
            .iter()
            .enumerate()
            .filter_map(move |(i, &slot)| {
                let cell = self.slab.get(usize::try_from(slot.checked_sub(1)?).ok()?)?;
                Some((xmin + i as i32, cell))
            })
    }

    /// The window of `c` (15 §6.1).
    pub fn window(&self, c: CellIdx) -> CellRect {
        window(&self.geom, c)
    }

    /// Exact port of `FOR_LINKMAP` at level 0 (15 §6.4): calls `f` for every object of the cells
    /// [`legacy_cells`] selects, in the legacy order (cell scan order, inside a cell the link-list order;
    /// players before others when both sets are asked for). The caller applies the original exact
    /// predicate; the original's 1000-nodes-per-cell walk limit is dropped.
    pub fn for_each_legacy(&self, pos: Vec3, n_range: i32, set: LinkSet, mut f: impl FnMut(ObjId)) {
        let rect = legacy_cells(&self.geom, pos, n_range);
        self.for_each_cell(rect, |_, cell| {
            if matches!(set, LinkSet::Players | LinkSet::Both) {
                for &id in &cell.players {
                    f(id);
                }
            }
            if matches!(set, LinkSet::Others | LinkSet::Both) {
                for &id in &cell.others {
                    f(id);
                }
            }
        });
    }
}
