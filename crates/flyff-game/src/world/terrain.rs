//! Typed wrappers over the terrain and collision queries of `flyff-sim` (Plan/15 §3.4, WP-WORLD-01).
//!
//! The server never parses `.lnd`: heights, height attributes and the static collision triangles come from the
//! per-folder `CollisionWorld` (31 §10). The wrappers fix the names the world code uses and give the height
//! attribute a type.

use super::catalog::WorldStatic;
use flyff_common::math::Vec3;

/// The class of the lower-left vertex of a cell (`GetHeightAttribute`, ref/19 §15.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(i8)]
pub enum HeightAttr {
    /// Outside the world (the legacy `-1`).
    Outside = -1,
    /// `HATTR_NONE`.
    None = 0,
    /// `HATTR_NOWALK`: raw height >= 1000.
    NoWalk = 1,
    /// `HATTR_NOFLY`: >= 2000.
    NoFly = 2,
    /// `HATTR_NOMOVE`: >= 3000.
    NoMove = 3,
    /// `HATTR_DIE`: >= 4000.
    Die = 4,
}

impl HeightAttr {
    /// The attribute of a raw `height_attr` result (anything above 4 is `Die`, below `-1` is `Outside`).
    pub fn from_raw(v: i32) -> HeightAttr {
        match v {
            i32::MIN..=-1 => HeightAttr::Outside,
            0 => HeightAttr::None,
            1 => HeightAttr::NoWalk,
            2 => HeightAttr::NoFly,
            3 => HeightAttr::NoMove,
            _ => HeightAttr::Die,
        }
    }

    /// The raw legacy value (`-1`, `0..=4`).
    pub fn raw(self) -> i32 {
        self as i32
    }
}

impl WorldStatic {
    /// `GetLandHeight(x, z)`: bilinear height with the attribute offsets stripped, 0 outside the world
    /// (31 §10.3).
    pub fn land_height(&self, x: f32, z: f32) -> f32 {
        self.collision.land_height(x, z)
    }

    /// `GetHeightAttribute(x, z)` (31 §10.3).
    pub fn height_attr(&self, x: f32, z: f32) -> HeightAttr {
        HeightAttr::from_raw(self.collision.height_attr(x, z))
    }

    /// `GetFullHeight(p)`: the closest static hit below `p` (a ray of 2000), else the land height (31 §10.7).
    pub fn full_height(&self, p: Vec3) -> f32 {
        self.collision.full_height(p)
    }

    /// `GetUnderHeight(p)`: the first static whose OBB the 1000 ray hits, else the land height (31 §10.7).
    pub fn under_height(&self, p: Vec3) -> f32 {
        self.collision.under_height(p)
    }
}
