//! Continents (Plan/15 §12.4, WP-WORLD-11; ref/06 §17; F-093): `CContinent` of the server.
//!
//! Built from `WdMadrigal.wld.cnt`: only the blocks with `C_useRealData` are server continents, the `TOWN`
//! blocks go to the town list; every vertex is converted to the integer point `((int)x, (int)z)`. The polygon
//! test is `CContinent::Point_In_Poly` (`src:_Common/Continent.cpp:219-254`): the crossing number with
//! **integer** arithmetic (`xinters = (t.z - p1.z) * (p2.x - p1.x) / (p2.z - p1.z) + p1.x` with truncating
//! division), the counter reset per polygon, the first polygon in ascending id order that contains the point
//! wins (the original iterated a `std::map<BYTE, ..>`).

use flyff_common::WorldId;
use flyff_common::defs::world::{WI_WORLD_ARENA, WI_WORLD_GUILDWAR, WI_WORLD_MADRIGAL};
use flyff_common::math::Vec3;
use flyff_data::worlds::{ContinentDef, WorldRegistry};
use std::collections::BTreeMap;

/// `CONT_NODATA`.
pub const CONT_NODATA: u8 = 0;
/// `CONT_FLARIS`.
pub const CONT_FLARIS: u8 = 1;
/// `CONT_SAINTMORNING`.
pub const CONT_SAINTMORNING: u8 = 2;
/// `CONT_RICIS`.
pub const CONT_RICIS: u8 = 3;
/// `CONT_ESTIA`.
pub const CONT_ESTIA: u8 = 4;
/// `CONT_EAST`.
pub const CONT_EAST: u8 = 15;
/// `CONT_WEST`.
pub const CONT_WEST: u8 = 240;
/// `CONT_DARKON12`.
pub const CONT_DARKON12: u8 = 241;
/// `CONT_DARKON3`.
pub const CONT_DARKON3: u8 = 242;
/// `CONT_HARMONIN`.
pub const CONT_HARMONIN: u8 = 243;
/// `CONT_ALL`.
pub const CONT_ALL: u8 = 255;

/// `CContinent::Point_In_Poly`: `true` when `(x, z)` is inside the closed polygon `poly` by the integer
/// crossing rule. An empty polygon contains nothing.
pub fn point_in_poly(poly: &[(i32, i32)], x: i32, z: i32) -> bool {
    let n = poly.len();
    if n == 0 {
        return false;
    }
    let mut counter = 0;
    let mut p1 = poly[0];
    for i in 1..=n {
        let p2 = poly[i % n];
        if z > p1.1.min(p2.1) && z <= p1.1.max(p2.1) && x <= p1.0.max(p2.0) && p1.1 != p2.1 {
            // the legacy long arithmetic, in 64 bits so that nothing wraps
            let xinters = i64::from(z - p1.1) * i64::from(p2.0 - p1.0) / i64::from(p2.1 - p1.1)
                + i64::from(p1.0);
            if p1.0 == p2.0 || i64::from(x) <= xinters {
                counter += 1;
            }
        }
        p1 = p2;
    }
    counter % 2 != 0
}

/// The server's continents and towns (15 §12.4).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Continents {
    conts: BTreeMap<u8, Vec<(i32, i32)>>,
    towns: BTreeMap<u8, Vec<(i32, i32)>>,
}

impl Continents {
    /// The server continents of the `.wld.cnt` blocks: `use_real_data` blocks only; `town` blocks go to the town
    /// list. A block with an id above 255 or without vertices is skipped. A later block with the same id
    /// replaces the earlier one (the original `map::insert` kept the **first**: so does this).
    pub fn from_defs(defs: &[ContinentDef]) -> Continents {
        let mut c = Continents::default();
        for d in defs {
            if !d.use_real_data || d.vertices.is_empty() {
                continue;
            }
            let Ok(id) = u8::try_from(d.id) else {
                continue;
            };
            let target = if d.town { &mut c.towns } else { &mut c.conts };
            target.entry(id).or_insert_with(|| d.server_points());
        }
        c
    }

    /// The continents of `WdMadrigal` in the world registry (empty when the folder is not loaded).
    pub fn from_registry(reg: &WorldRegistry) -> Continents {
        reg.folders
            .get("wdmadrigal")
            .map_or_else(Continents::default, |f| {
                Continents::from_defs(&f.continents)
            })
    }

    /// Number of continent polygons.
    pub fn len(&self) -> usize {
        self.conts.len()
    }

    /// `true` without any continent polygon.
    pub fn is_empty(&self) -> bool {
        self.conts.is_empty()
    }

    /// `CContinent::GetContinent(mover)`: Madrigal is the polygon test, the arena (208) is DARKON12, the
    /// guild-war world (202) FLARIS, everything else `NODATA`.
    pub fn continent_of(&self, world: WorldId, pos: Vec3) -> u8 {
        match world.0 {
            WI_WORLD_MADRIGAL => self.continent_at(pos),
            WI_WORLD_ARENA => CONT_DARKON12,
            WI_WORLD_GUILDWAR => CONT_FLARIS,
            _ => CONT_NODATA,
        }
    }

    /// `GetContinent(pos)`: the polygon test of Madrigal coordinates, `(long)x`, `(long)z`.
    pub fn continent_at(&self, pos: Vec3) -> u8 {
        let (x, z) = (pos.x as i32, pos.z as i32);
        self.conts
            .iter()
            .find(|(_, poly)| point_in_poly(poly, x, z))
            .map_or(CONT_NODATA, |(id, _)| *id)
    }

    /// `GetTown(pos)`.
    pub fn town_of(&self, pos: Vec3) -> u8 {
        let (x, z) = (pos.x as i32, pos.z as i32);
        self.towns
            .iter()
            .find(|(_, poly)| point_in_poly(poly, x, z))
            .map_or(CONT_NODATA, |(id, _)| *id)
    }

    /// `GetArea(cont)` (`__USING_CONTINENT_DATA`): `NODATA` stays, `<= EAST` is `EAST`, `>= WEST` is `WEST`.
    pub fn area_of(cont: u8) -> u8 {
        if cont == CONT_NODATA {
            CONT_NODATA
        } else if cont <= CONT_EAST {
            CONT_EAST
        } else if cont >= CONT_WEST {
            CONT_WEST
        } else {
            CONT_NODATA
        }
    }

    /// `GetRevivalPos(cont)`.
    pub fn revival_pos(cont: u8) -> Vec3 {
        match cont {
            CONT_SAINTMORNING | CONT_RICIS => Vec3::new(8470.0, 0.0, 3635.0),
            CONT_DARKON12 | CONT_DARKON3 => Vec3::new(3808.0, 0.0, 4455.0),
            CONT_EAST => Vec3::new(6959.0, 0.0, 3217.0),
            CONT_WEST => Vec3::new(3824.0, 0.0, 4456.0),
            // NODATA, FLARIS and every other id
            _ => Vec3::new(6968.0, 0.0, 3328.0),
        }
    }
}
