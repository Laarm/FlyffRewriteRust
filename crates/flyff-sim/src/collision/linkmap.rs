//! The static link-map reproduction (Plan/31 §10.4; WP-SIM-03).
//!
//! "First OBB hit wins" in several world queries, so the candidate order of the original server link
//! map (`FOR_LINKMAP`, `WORLDSERVER/LinkMap.cpp`, `World.h:790-860`) is reproduced: the builder
//! (Plan/24 §10.7.6) stores each instance's link level, its cell and the in-cell list order in the chunk
//! (`LINK`); the runtime walk here visits the cells in the original order.
//!
//! [`link_placement`] is the single implementation of the placement rule, called by the chunk builder
//! of Plan/24 and by `testkit::scenes`.

use super::chunk::{
    LandChunk, MAP_SIZE, MAX_LINK_LEVEL, STATIC_GRID, StaticInstance, link_cell_size, link_width,
};
use super::world::{CollisionWorld, LandSlot};
use crate::num::Vec3;
use core::ops::ControlFlow;
use flyff_common::math::{D3dMatrix, ftoi_x86};

/// The original walks at most this many nodes of one cell list (`int __limit = 1000`).
pub const LINK_CELL_LIMIT: usize = 1000;

/// Precomputed geometry of the static link map of one world, plus the highest level in use
/// (`GetMaxLinkLevel`: the original iterates `0..max_level` only).
#[derive(Clone, Debug)]
pub struct StaticIndex {
    mpu: i32,
    lands_w: i32,
    lands_h: i32,
    max_level: usize,
}

impl StaticIndex {
    pub(crate) fn new(mpu: u32, lands_w: u32, lands_h: u32) -> StaticIndex {
        StaticIndex {
            mpu: mpu as i32,
            lands_w: lands_w as i32,
            lands_h: lands_h as i32,
            max_level: 0,
        }
    }

    /// Recomputes the number of levels in use from the loaded chunks.
    pub(crate) fn rebuild(&mut self, lands: &[LandSlot]) {
        self.max_level = lands
            .iter()
            .filter_map(|s| match s {
                LandSlot::Loaded(c) => Some(c.link_levels_in_use()),
                _ => None,
            })
            .max()
            .unwrap_or(0);
    }

    /// Number of link levels with at least one cell (`GetMaxLinkLevel`).
    pub fn max_level(&self) -> usize {
        self.max_level
    }
}

/// One candidate instance yielded by [`CollisionWorld::for_linkmap`].
#[derive(Clone, Copy)]
pub struct StaticRef<'a> {
    /// The chunk that stores the instance.
    pub chunk: &'a LandChunk,
    /// Land index (`lx + lz * lands_w`) of that chunk.
    pub land: u32,
    /// Index into `chunk.instances`.
    pub index: u32,
}

impl<'a> StaticRef<'a> {
    /// The instance (placement, matrices, OBB).
    #[inline]
    pub fn instance(&self) -> Option<&'a StaticInstance> {
        self.chunk.instances.get(self.index as usize)
    }

    /// The collision mesh of the instance.
    #[inline]
    pub fn mesh(&self) -> Option<&'a super::chunk::CollisionMesh> {
        self.chunk.meshes.get(self.instance()?.mesh as usize)
    }

    /// Stable id `land_index * 65536 + sobj_index` used by `ray_cast` (Plan/31 §10.11).
    #[inline]
    pub fn id(&self) -> u32 {
        self.land.wrapping_mul(65536).wrapping_add(self.index)
    }
}

impl CollisionWorld {
    /// `FOR_LINKMAP(this, pos, obj, range, linkStatic)`: visits the static instances of the 2x2 cell
    /// neighbourhood (more with `range > 0`) per level, level 0 first, z-major, in the stored in-cell
    /// order; `f` returns `ControlFlow::Break` to stop (the original's `goto`).
    pub fn for_linkmap<'w, F>(&'w self, pos: Vec3, range: i32, mut f: F)
    where
        F: FnMut(StaticRef<'w>) -> ControlFlow<()>,
    {
        let idx = &self.static_index;
        let mpu = idx.mpu;
        let mpu_f = mpu as f32;
        let lx = ftoi_x86(pos.x / mpu_f);
        let lz = ftoi_x86(pos.z / mpu_f);
        let px = ftoi_x86(pos.x);
        let pz = ftoi_x86(pos.z);
        for level in 0..idx.max_level.min(MAX_LINK_LEVEL) {
            let width = link_width(level, mpu as u32);
            if width <= 0 {
                continue;
            }
            let unit = (MAP_SIZE as i32) / width;
            if unit <= 0 {
                continue;
            }
            let max_w = width * idx.lands_w;
            let max_h = width * idx.lands_h;
            let ox = (lx / unit) * unit * mpu;
            let oz = (lz / unit) * unit * mpu;
            let d = unit * mpu / 2;
            let bx = i32::from(px - ox > d);
            let bz = i32::from(pz - oz > d);
            let x0 = ((lx - range) / unit + (bx - 1)).max(0);
            let z0 = ((lz - range) / unit + (bz - 1)).max(0);
            let x1 = ((lx + range) / unit + bx).min(max_w - 1);
            let z1 = ((lz + range) / unit + bz).min(max_h - 1);
            let mut cz = z0;
            while cz <= z1 {
                let mut cx = x0;
                while cx <= x1 {
                    if self.visit_cell(level, width, cx, cz, &mut f).is_break() {
                        return;
                    }
                    cx += 1;
                }
                cz += 1;
            }
        }
    }

    fn visit_cell<'w, F>(
        &'w self,
        level: usize,
        width: i32,
        cx: i32,
        cz: i32,
        f: &mut F,
    ) -> ControlFlow<()>
    where
        F: FnMut(StaticRef<'w>) -> ControlFlow<()>,
    {
        if cx < 0 || cz < 0 || width <= 0 {
            return ControlFlow::Continue(());
        }
        let (land_x, land_z) = ((cx / width) as u32, (cz / width) as u32);
        let Some(land_idx) = self.land_index(land_x, land_z) else {
            return ControlFlow::Continue(());
        };
        let Some(LandSlot::Loaded(chunk)) = self.slot_by_index(land_idx) else {
            return ControlFlow::Continue(());
        };
        let (Ok(level_u8), Ok(cx_u16), Ok(cz_u16)) =
            (u8::try_from(level), u16::try_from(cx), u16::try_from(cz))
        else {
            return ControlFlow::Continue(());
        };
        let key = (level_u8, cz_u16, cx_u16);
        let found = chunk
            .links
            .binary_search_by(|c| (c.level, c.cell_z, c.cell_x).cmp(&key));
        let Ok(i) = found else {
            return ControlFlow::Continue(());
        };
        let Some(cell) = chunk.links.get(i) else {
            return ControlFlow::Continue(());
        };
        for &inst in cell.instances.iter().take(LINK_CELL_LIMIT) {
            let r = StaticRef {
                chunk,
                land: land_idx as u32,
                index: inst,
            };
            f(r)?;
        }
        ControlFlow::Continue(())
    }
}

/// `D3DXVec3Transform` of a point by a row-major matrix, without the `w` divide:
/// `x*m00 + y*m10 + z*m20 + m30`, summed left to right.
#[inline]
fn transform_point_nodiv(v: Vec3, m: &D3dMatrix) -> Vec3 {
    let g = |r: usize, c: usize| m.0.get(r * 4 + c).copied().unwrap_or(0.0);
    Vec3::new(
        ((v.x * g(0, 0) + v.y * g(1, 0)) + v.z * g(2, 0)) + g(3, 0),
        ((v.x * g(0, 1) + v.y * g(1, 1)) + v.z * g(2, 1)) + g(3, 1),
        ((v.x * g(0, 2) + v.y * g(1, 2)) + v.z * g(2, 2)) + g(3, 2),
    )
}

/// The link level of an object (`CObj::CalcLinkLevel` + `CLinkMap::CalcLinkLevel`, static grid): from the
/// two transformed AABB corners only (Plan/24 §10.7.6 rule 1).
pub fn link_level(aabb_min: Vec3, aabb_max: Vec3, world: &D3dMatrix, mpu: u32) -> u8 {
    let null_box = aabb_min == aabb_max || aabb_min.x == 65535.0;
    let width = if null_box {
        0.0_f32
    } else {
        let vmin = transform_point_nodiv(aabb_min, world);
        let vmax = transform_point_nodiv(aabb_max, world);
        let dx = vmax.x - vmin.x;
        let dz = vmax.z - vmin.z;
        let w = if dx.abs() > dz.abs() { dx } else { dz };
        w.abs()
    };
    let mul = 0.25_f32 * (mpu as f32);
    let mut level = 0usize;
    while level < MAX_LINK_LEVEL {
        let grid = STATIC_GRID.get(level).copied().unwrap_or(256.0);
        if width < grid * mul {
            break;
        }
        level += 1;
    }
    level.min(MAX_LINK_LEVEL - 1) as u8 // BOUNDS: <= 6
}

/// The link placement of a static object (Plan/24 §10.7.6; `CLinkMap::GetObjPtr` + `CalcLinkLevel`):
/// `Some((level, cell_x, cell_z))` in global cell coordinates at that level, or `None` when the linear
/// position `nPos` is outside `0..nMaxWidth^2` (the object is not linked and never collides). KEEP-QUIRK:
/// `nPos` is linear, so a position past the east edge wraps into the next cell row.
pub fn link_placement(
    aabb_min: Vec3,
    aabb_max: Vec3,
    world: &D3dMatrix,
    pos: Vec3,
    mpu: u32,
    lands_w: u32,
) -> Option<(u8, u32, u32)> {
    let level = link_level(aabb_min, aabb_max, world, mpu);
    let width = link_width(usize::from(level), mpu);
    if width <= 0 || lands_w == 0 {
        return None;
    }
    let lands_w = i32::try_from(lands_w).ok()?;
    let max_w = width.checked_mul(lands_w)?;
    let unit = (MAP_SIZE as i32).checked_mul(lands_w)? / max_w;
    if unit <= 0 {
        return None;
    }
    let mpu_f = mpu as f32;
    let unit_f = unit as f32;
    let nz = ftoi_x86(pos.z / mpu_f / unit_f);
    let nx = ftoi_x86(pos.x / mpu_f / unit_f);
    let n_pos = nz.wrapping_mul(max_w).wrapping_add(nx);
    let limit = max_w.checked_mul(max_w)?;
    if n_pos < 0 || n_pos >= limit {
        return None;
    }
    Some((level, (n_pos % max_w) as u32, (n_pos / max_w) as u32)) // BOUNDS: 0 <= n_pos < max_w^2
}

/// Cell size in world units of `level` (re-export of [`link_cell_size`] for callers of this module).
pub fn cell_size(level: usize, mpu: u32) -> i32 {
    link_cell_size(level, mpu)
}
