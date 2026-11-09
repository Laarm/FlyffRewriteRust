//! Terrain queries (Plan/31 §10.3; WP-SIM-02), exact transcriptions of `CWorld::GetLandHeight`,
//! `GetLandTri`, `GetLandTris`, `GetHeightAttribute`, `GetWaterHeight`, `VecInWorld` and `ClipX/ClipZ`
//! over the *server heightmap view*: heights are the stripped values of the chunks, the 129th row and
//! column of a land equal the neighbour's row/column 0 (applied by the builder, Plan/24 §10.7.2).
//!
//! Queries read the four vertices of a cell from the cell's *own* chunk (local index up to 128), which
//! equals the neighbour's local 0 by that rule.

use super::chunk::{MAP_SIZE, VERTS_PER_SIDE, vertex_index};
use super::world::{CollisionWorld, LandSlot, WLD_EXTRA_WIDTH};
use crate::num::Vec3;
use flyff_common::math::ftoi_x86;

/// Per-query context for mover-flavoured queries: the height a `Pending` land reports ("flat at the
/// querying mover's y: neither falls nor snaps", Plan/31 §10.1) and the lands touched while pending.
#[derive(Clone, Debug, Default)]
pub struct QueryCtx {
    /// Height reported for vertices of `Pending` lands (the querying mover's current y).
    pub pending_y: f32,
    /// Pending lands touched for the first time (world-wide latch), for `SimEvent::ChunkMissing`.
    pub missing: Vec<(u16, u16)>,
}

impl QueryCtx {
    /// A context for a mover at height `y`.
    pub fn new(pending_y: f32) -> QueryCtx {
        QueryCtx {
            pending_y,
            missing: Vec::new(),
        }
    }
}

/// Up to nine cells x two triangles (`GetLandTris`).
#[derive(Clone, Copy, Debug)]
pub struct LandTris {
    /// The triangles, `count` of them valid.
    pub tris: [[Vec3; 3]; 18],
    /// Number of valid triangles (`0..=18`).
    pub count: usize,
}

impl LandTris {
    /// Valid triangles in query order.
    pub fn iter(&self) -> impl Iterator<Item = &[Vec3; 3]> {
        self.tris.iter().take(self.count)
    }
}

impl CollisionWorld {
    /// `VecInWorld(x, z)`: `x >= 0 && x/mpu < WORLD_WIDTH - 1` and the same for `z`; NaN is outside.
    #[inline]
    pub fn vec_in_world(&self, x: f32, z: f32) -> bool {
        let inv_mpu = 1.0_f32 / (self.mpu as f32);
        if x.is_nan() || x < 0.0 {
            return false;
        }
        if x * inv_mpu >= (self.world_width_cells() - 1) as f32 {
            return false;
        }
        if z.is_nan() || z < 0.0 {
            return false;
        }
        z * inv_mpu < (self.world_height_cells() - 1) as f32
    }

    /// `ClipX(x)`: clamps to `[10, WORLD_WIDTH * mpu - 10]` (`WLD_EXTRA_WIDTH`). **FIX**: NaN maps to the
    /// lower bound instead of propagating.
    #[inline]
    pub fn clip_x(&self, x: f32) -> f32 {
        if x.is_nan() || x < WLD_EXTRA_WIDTH {
            WLD_EXTRA_WIDTH
        } else {
            let max = (self.world_width_cells() * self.mpu as i32) as f32 - WLD_EXTRA_WIDTH;
            if x > max { max } else { x }
        }
    }

    /// `ClipZ(z)`: see [`CollisionWorld::clip_x`].
    #[inline]
    pub fn clip_z(&self, z: f32) -> f32 {
        if z.is_nan() || z < WLD_EXTRA_WIDTH {
            WLD_EXTRA_WIDTH
        } else {
            let max = (self.world_height_cells() * self.mpu as i32) as f32 - WLD_EXTRA_WIDTH;
            if z > max { max } else { z }
        }
    }

    /// Locates cell `(px, pz)` (global heightmap cell, `px, pz >= 0`): the land slot index and the local
    /// cell coordinates.
    #[inline]
    fn locate(&self, px: i32, pz: i32) -> Option<(usize, usize, usize)> {
        if px < 0 || pz < 0 {
            return None;
        }
        let map = MAP_SIZE as i32;
        let idx = self.land_index((px / map) as u32, (pz / map) as u32)?;
        Some((idx, (px % map) as usize, (pz % map) as usize))
    }

    /// The four vertex heights `[(0,0), (1,0), (0,1), (1,1)]` of cell `(px, pz)`.
    fn cell_heights(&self, px: i32, pz: i32, q: &mut QueryCtx) -> [f32; 4] {
        let Some((idx, lx, lz)) = self.locate(px, pz) else {
            return [0.0; 4];
        };
        match self.slot_by_index(idx) {
            Some(LandSlot::Loaded(c)) => {
                let h = |a: usize, b: usize| {
                    vertex_index(lx + a, lz + b).map_or(0.0, |i| c.heights.get(i))
                };
                [h(0, 0), h(1, 0), h(0, 1), h(1, 1)]
            }
            Some(LandSlot::Pending) => {
                if self.first_pending_touch(idx) {
                    q.missing.push(self.land_coords(idx));
                }
                [q.pending_y; 4]
            }
            _ => [0.0; 4],
        }
    }

    /// `GetLandHeight(x, z)`: bilinear height (`0.0` outside the world or over a `Missing` land; over a
    /// `Pending` land `q.pending_y`).
    pub fn land_height_q(&self, x: f32, z: f32, q: &mut QueryCtx) -> f32 {
        if !self.vec_in_world(x, z) {
            return 0.0;
        }
        let mpu = self.mpu as f32;
        let fx = x / mpu;
        let fz = z / mpu;
        let px = ftoi_x86(fx);
        let pz = ftoi_x86(fz);
        let dx = fx - px as f32;
        let dz = fz - pz as f32;
        let [y1, y2, y3, y4] = self.cell_heights(px, pz, q);
        let dy1 = y1 * (1.0 - dx) * (1.0 - dz);
        let dy2 = y2 * dx * (1.0 - dz);
        let dy3 = y3 * (1.0 - dx) * dz;
        let dy4 = y4 * dx * dz;
        ((dy1 + dy2) + dy3) + dy4
    }

    /// [`CollisionWorld::land_height_q`] without a mover context (`Pending` reads as `0.0`).
    pub fn land_height(&self, x: f32, z: f32) -> f32 {
        self.land_height_q(x, z, &mut QueryCtx::default())
    }

    /// `GetLandTri(x, z)`: the heightmap triangle containing `(x, z)` (checkerboard diagonal), vertices in
    /// world units; `None` when `(x, z)` is outside the world.
    pub fn land_tri_q(&self, x: f32, z: f32, q: &mut QueryCtx) -> Option<[Vec3; 3]> {
        self.land_tri_pair(x, z, q).map(|(first, _)| first)
    }

    /// [`CollisionWorld::land_tri_q`] without a mover context.
    pub fn land_tri(&self, x: f32, z: f32) -> Option<[Vec3; 3]> {
        self.land_tri_q(x, z, &mut QueryCtx::default())
    }

    /// Both triangles of the cell containing `(x, z)` (crate-visible form of `GetLandTri2`).
    pub(crate) fn land_tri_pair_pub(
        &self,
        x: f32,
        z: f32,
        q: &mut QueryCtx,
    ) -> Option<([Vec3; 3], [Vec3; 3])> {
        self.land_tri_pair(x, z, q)
    }

    /// `GetLandTri2`: the triangle containing the point first, then the other triangle of the cell.
    fn land_tri_pair(&self, x: f32, z: f32, q: &mut QueryCtx) -> Option<([Vec3; 3], [Vec3; 3])> {
        if !self.vec_in_world(x, z) {
            return None;
        }
        let mpu = self.mpu as f32;
        let fx = x / mpu;
        let fz = z / mpu;
        let gx = ftoi_x86(fx);
        let gz = ftoi_x86(fz);
        let glx = fx - gx as f32;
        let glz = fz - gz as f32;
        let [y00, y10, y01, y11] = self.cell_heights(gx, gz, q);
        let m = self.mpu as i32;
        let vx = |a: i32| ((gx + a) * m) as f32;
        let vz = |b: i32| ((gz + b) * m) as f32;
        let v00 = Vec3::new(vx(0), y00, vz(0));
        let v10 = Vec3::new(vx(1), y10, vz(0));
        let v01 = Vec3::new(vx(0), y01, vz(1));
        let v11 = Vec3::new(vx(1), y11, vz(1));
        let even = (gx + gz) % 2 == 0;
        Some(if even {
            let a = [v00, v11, v10];
            let b = [v00, v01, v11];
            if glx > glz { (a, b) } else { (b, a) }
        } else {
            let a = [v00, v01, v10];
            let b = [v10, v01, v11];
            if glx + glz < 1.0 { (a, b) } else { (b, a) }
        })
    }

    /// `GetLandTris(x, z)`: the triangles of the 3x3 cell neighbourhood in the original query order
    /// `(0,0), (-1,-1), (0,-1), (+1,-1), (-1,0), (+1,0), (-1,+1), (0,+1), (+1,+1)` (offsets of one `mpu` in
    /// world units); points outside the world are skipped.
    pub fn land_tris_3x3_q(&self, x: f32, z: f32, q: &mut QueryCtx) -> LandTris {
        let m = self.mpu as f32;
        const OFFSETS: [(i32, i32); 9] = [
            (0, 0),
            (-1, -1),
            (0, -1),
            (1, -1),
            (-1, 0),
            (1, 0),
            (-1, 1),
            (0, 1),
            (1, 1),
        ];
        let mut out = LandTris {
            tris: [[Vec3::ZERO; 3]; 18],
            count: 0,
        };
        for (ox, oz) in OFFSETS {
            if let Some((a, b)) = self.land_tri_pair(x + ox as f32 * m, z + oz as f32 * m, q) {
                if let Some(slot) = out.tris.get_mut(out.count) {
                    *slot = a;
                }
                if let Some(slot) = out.tris.get_mut(out.count + 1) {
                    *slot = b;
                }
                out.count += 2;
            }
        }
        out
    }

    /// [`CollisionWorld::land_tris_3x3_q`] without a mover context.
    pub fn land_tris_3x3(&self, x: f32, z: f32) -> LandTris {
        self.land_tris_3x3_q(x, z, &mut QueryCtx::default())
    }

    /// `GetHeightAttribute(x, z)`: the attribute class (`HATTR_*`) of the lower-left vertex of the cell
    /// containing `(x, z)` (no interpolation); `-1` outside the world; `NONE` over `Missing`/`Pending`
    /// lands.
    pub fn height_attr(&self, x: f32, z: f32) -> i32 {
        if !self.vec_in_world(x, z) {
            return -1;
        }
        let inv_mpu = 1.0_f32 / (self.mpu as f32);
        let px = ftoi_x86(x * inv_mpu);
        let pz = ftoi_x86(z * inv_mpu);
        let Some((idx, lx, lz)) = self.locate(px, pz) else {
            return -1;
        };
        match self.slot_by_index(idx) {
            Some(LandSlot::Loaded(c)) => {
                vertex_index(lx, lz).map_or(0, |i| i32::from(c.attrs.get(i)))
            }
            _ => 0,
        }
    }

    /// `GetWaterHeight(int x, int z)` of `pos` (`x`, `z` truncated first): the `(height, texture)` pair of
    /// the 8x8-cell patch; `Some((0, 0))` over a `Missing` land, `None` over a `Pending` land or outside
    /// the world. The water type is `texture & 3` and `WTYPE_CLOUD` compares the whole byte (`== 1`).
    pub fn water_at(&self, pos: Vec3) -> Option<(u8, u8)> {
        let ix = ftoi_x86(pos.x);
        let iz = ftoi_x86(pos.z);
        if !self.vec_in_world(ix as f32, iz as f32) {
            return None;
        }
        let mpu = self.mpu as i32;
        let tx = (ix / mpu) / 8;
        let tz = (iz / mpu) / 8;
        let per_land = (VERTS_PER_SIDE as i32 - 1) / 8; // 16 patches per land side
        let idx = self.land_index((tx / per_land) as u32, (tz / per_land) as u32)?;
        match self.slot_by_index(idx) {
            Some(LandSlot::Loaded(c)) => {
                c.water_patch((tx % per_land) as usize, (tz % per_land) as usize)
            }
            Some(LandSlot::Missing) => Some((0, 0)),
            _ => None,
        }
    }

    /// `CWorld::IntersectRayTerrain2` (the server branch, `WorldIntersect.cpp:1025-1066`, used by the ClockWorks
    /// laser): the parameter `t` (in units of `dir`) of the nearest terrain hit of the RAY `origin + t * dir`
    /// (not a segment) among the triangles of a 20 x 20 window of cells, `j` and `i` from `-10` up to and
    /// including `+9` cells (an asymmetric window) around the xz of `origin`. Both triangles of every cell are
    /// tested in the order of `GetLandTri2` with `IsTouchRayTri` ([`geom::ray_tri`](super::geom::ray_tri): back
    /// faces and near-parallel triangles miss); only `0 <= t < 65535` counts (`fMinDist` starts at 65535 and a
    /// negative `t` never lowers it). A hit farther out than the window is a miss, and `None` means "no hit" (the
    /// original then yields a point 65535 m away, which hits nobody; its `if (fDist)` test of the LAST
    /// triangle distance is not reproduced: it differs only for a last hit at exactly `t == 0`).
    pub fn intersect_ray_terrain2(&self, origin: Vec3, dir: Vec3) -> Option<f32> {
        const FAR: f32 = 65_535.0;
        let mpu = self.mpu as f32;
        let mut q = QueryCtx::default();
        let mut best = FAR;
        for i in -10..10 {
            for j in -10..10 {
                let Some((a, b)) = self.land_tri_pair(
                    origin.x + mpu * j as f32,
                    origin.z + mpu * i as f32,
                    &mut q,
                ) else {
                    continue;
                };
                for tri in [a, b] {
                    if let Some(t) = super::geom::ray_tri(tri[0], tri[1], tri[2], origin, dir)
                        && t >= 0.0
                        && t < best
                    {
                        best = t;
                    }
                }
            }
        }
        (best < FAR).then_some(best)
    }
}
