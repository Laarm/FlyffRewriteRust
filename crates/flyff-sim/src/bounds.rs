//! Radius, ranges and model bounds (Plan/31 §11.5; `src:_Common/Obj.cpp:764-842`; WP-SIM-10 part).
//!
//! `IsRangeObj` uses 0.8x the average-of-three-axes radius of the model AABBs; model bounds of players
//! are the union of the loaded part files starting from the *empty box* (`+65535`/`-65535`,
//! `CModelObject::Init`). [`ModelBounds`] is the `FLMB` v1 file the builder of Plan/24 writes and both
//! sides load.

use crate::collision::geom::Aabb;
use crate::num::{Vec3, length_sq};
use crate::step::MoverSnapshot;
use crate::types::{AR_FAR, AR_HRANGE, AR_HWAND, AR_LONG, AR_RANGE, AR_SHORT, AR_WAND, SimError};

/// `INV_6 = 1/6`.
pub const INV_6: f32 = 1.0 / 6.0;
/// `FLMB` format version (`MODEL_BOUNDS_VERSION`).
pub const MODEL_BOUNDS_VERSION: u16 = 1;

/// The empty box `CModelObject::Init` starts from: `min = +65535`, `max = -65535`.
pub const EMPTY_AABB: Aabb = Aabb {
    min: Vec3::new(65535.0, 65535.0, 65535.0),
    max: Vec3::new(-65535.0, -65535.0, -65535.0),
};

/// `GetRadius()`: `((|dx| + |dy|) + |dz|) * INV_6 * scale` of the model AABB.
pub fn radius(b: &Aabb, scale: f32) -> f32 {
    ((((b.max.x - b.min.x).abs() + (b.max.y - b.min.y).abs()) + (b.max.z - b.min.z).abs()) * INV_6)
        * scale
}

/// `GetRadiusXZ()`: `(((|dx| + |dz|) / 2) / 2) * scale`.
pub fn radius_xz(b: &Aabb, scale: f32) -> f32 {
    ((((b.max.x - b.min.x).abs() + (b.max.z - b.min.z).abs()) / 2.0) / 2.0) * scale
}

/// `IsRangeObj(A, B, r)` (A is `this`): `r == 0` counts as 1; true iff the squared 3-D distance is strictly
/// below `R^2` with `R = 0.8 * Ra + (0.8 * Rb + r)`.
pub fn is_range_obj(a_pos: Vec3, a_radius: f32, b_pos: Vec3, b_radius: f32, range: f32) -> bool {
    let r = if range == 0.0 { 1.0 } else { range };
    let ra = a_radius * 0.8;
    let rb = b_radius * 0.8;
    let big_r = ra + (rb + r);
    length_sq(a_pos - b_pos) < big_r * big_r
}

/// `IsRangeObj(A, point, r)` (A is `this`): `R = 0.8 + (0.8 * Ra + r)`; no zero-range rule.
pub fn is_range_point(a_pos: Vec3, a_radius: f32, point: Vec3, range: f32) -> bool {
    let r_src = a_radius * 0.8 + range;
    let big_r = 1.0_f32 * 0.8 + r_src;
    length_sq(point - a_pos) < big_r * big_r
}

/// `IsRangeObj(dest, mover, r)` over snapshots: `dest` is `this` (the destination object, which the
/// original calls as `pObj->IsRangeObj(this, range)`).
pub fn is_range_obj_snap(
    dest: &MoverSnapshot,
    mover_pos: Vec3,
    mover_radius: f32,
    range: f32,
) -> bool {
    is_range_obj(dest.pos, dest.radius, mover_pos, mover_radius, range)
}

/// `GetAttackRange(ar)` (`MoverMsg.cpp:140-166`): metres per `AR_*` code, multiplied by
/// `(100 + hawkeye) / 100` when `hawkeye > 0`; unknown codes give 0.
pub fn get_attack_range(ar: u32, hawkeye_rate: i32) -> f32 {
    let mut r = match ar {
        AR_SHORT => 2.0_f32,
        AR_LONG => 3.0,
        AR_FAR => 4.0,
        AR_RANGE => 10.0,
        AR_WAND => 15.0,
        AR_HRANGE => 6.0,
        AR_HWAND => 18.0,
        _ => 0.0,
    };
    if hawkeye_rate > 0 {
        let n = hawkeye_rate + 100;
        r = r * n as f32 / 100.0;
    }
    r
}

/// The union of model part boxes starting from [`EMPTY_AABB`] (zero parts give `EMPTY_AABB`).
pub fn union_parts(parts: &[Aabb]) -> Aabb {
    let mut out = EMPTY_AABB;
    for p in parts {
        out.min = Vec3::new(
            out.min.x.min(p.min.x),
            out.min.y.min(p.min.y),
            out.min.z.min(p.min.z),
        );
        out.max = Vec3::new(
            out.max.x.max(p.max.x),
            out.max.y.max(p.max.y),
            out.max.z.max(p.max.z),
        );
    }
    out
}

/// The `FLMB` v1 model-bounds table: lower-case VFS paths sorted by byte order with the o3d header AABB.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct ModelBounds {
    entries: Vec<(String, Aabb)>,
}

fn bad(msg: &'static str) -> SimError {
    SimError::BadModelBounds(msg)
}

impl ModelBounds {
    /// Builds a table from entries; they must be sorted by path bytes with unique paths and finite floats.
    pub fn new(entries: Vec<(String, Aabb)>) -> Result<ModelBounds, SimError> {
        for w in entries.windows(2) {
            if let [a, b] = w
                && a.0.as_bytes() >= b.0.as_bytes()
            {
                return Err(bad("entries are not sorted by unique path"));
            }
        }
        for (_, b) in &entries {
            if ![b.min.x, b.min.y, b.min.z, b.max.x, b.max.y, b.max.z]
                .iter()
                .all(|v| v.is_finite())
            {
                return Err(bad("non-finite float"));
            }
        }
        Ok(ModelBounds { entries })
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` when there are no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The box of `path` (lower-case VFS path).
    pub fn get(&self, path: &str) -> Option<Aabb> {
        let i = self.index_of(path)?;
        self.entry(i)
    }

    /// The box of entry `i` (the row of the JSON `model-bounds` bundle).
    pub fn entry(&self, i: u32) -> Option<Aabb> {
        self.entries.get(i as usize).map(|(_, b)| *b)
    }

    /// The index of `path`.
    pub fn index_of(&self, path: &str) -> Option<u32> {
        self.entries
            .binary_search_by(|(p, _)| p.as_bytes().cmp(path.as_bytes()))
            .ok()
            .and_then(|i| u32::try_from(i).ok())
    }

    /// The union of the entries `indices` (`union_parts` of their boxes); an unknown index is an error.
    pub fn union_entries(&self, indices: &[u32]) -> Result<Aabb, SimError> {
        let mut parts = Vec::with_capacity(indices.len());
        for &i in indices {
            parts.push(self.entry(i).ok_or_else(|| bad("unknown entry index"))?);
        }
        Ok(union_parts(&parts))
    }

    /// Serialises the table as `FLMB` v1 (`32 + 32 * N + pad4(L)` bytes).
    pub fn encode(&self) -> Vec<u8> {
        let mut strings = Vec::new();
        let mut descs = Vec::with_capacity(self.entries.len() * 32);
        for (path, b) in &self.entries {
            descs.extend_from_slice(&(strings.len() as u32).to_le_bytes()); // BOUNDS: far below 4 GiB
            descs.extend_from_slice(&(path.len() as u32).to_le_bytes()); // BOUNDS: far below 4 GiB
            for v in [b.min.x, b.min.y, b.min.z, b.max.x, b.max.y, b.max.z] {
                descs.extend_from_slice(&v.to_le_bytes());
            }
            strings.extend_from_slice(path.as_bytes());
        }
        let strings_len = strings.len() as u32; // BOUNDS: far below 4 GiB
        let pad = (4 - strings.len() % 4) % 4;
        strings.extend(std::iter::repeat_n(0u8, pad));
        let file_len = 32 + descs.len() + strings.len();
        let mut out = Vec::with_capacity(file_len);
        out.extend_from_slice(b"FLMB");
        out.extend_from_slice(&MODEL_BOUNDS_VERSION.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(file_len as u32).to_le_bytes()); // BOUNDS: far below 4 GiB
        out.extend_from_slice(&(self.entries.len() as u32).to_le_bytes()); // BOUNDS: far below 2^32
        out.extend_from_slice(&strings_len.to_le_bytes());
        out.extend_from_slice(&[0u8; 12]);
        out.extend_from_slice(&descs);
        out.extend_from_slice(&strings);
        out
    }

    /// Decodes an `FLMB` v1 file; rejects a wrong magic/version, a `file_len` mismatch, unsorted or
    /// duplicate paths, out-of-range or non-consecutive string slices, non-UTF-8 paths and non-finite
    /// floats ([`SimError::BadModelBounds`]); never panics.
    pub fn decode(b: &[u8]) -> Result<ModelBounds, SimError> {
        if b.get(..4) != Some(b"FLMB") {
            return Err(bad("wrong magic"));
        }
        let u16_at = |o: usize| -> Option<u16> {
            Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?))
        };
        let u32_at = |o: usize| -> Option<u32> {
            Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
        };
        let f32_at = |o: usize| -> Option<f32> {
            Some(f32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
        };
        let version = u16_at(4).ok_or_else(|| bad("truncated"))?;
        let flags = u16_at(6).ok_or_else(|| bad("truncated"))?;
        let file_len = u32_at(8).ok_or_else(|| bad("truncated"))? as usize;
        let n = u32_at(12).ok_or_else(|| bad("truncated"))? as usize;
        let l = u32_at(16).ok_or_else(|| bad("truncated"))? as usize;
        if version != MODEL_BOUNDS_VERSION || flags != 0 {
            return Err(bad("unsupported version or flags"));
        }
        if b.get(20..32).is_none_or(|r| r.iter().any(|&x| x != 0)) {
            return Err(bad("reserved bytes are not zero"));
        }
        let expect = n
            .checked_mul(32)
            .and_then(|v| v.checked_add(32))
            .and_then(|v| v.checked_add(l.checked_add(3)? & !3))
            .ok_or_else(|| bad("overflow"))?;
        if file_len != b.len() || expect != b.len() {
            return Err(bad("length mismatch"));
        }
        let str_off = 32 + n * 32;
        let strings = b
            .get(str_off..str_off + l)
            .ok_or_else(|| bad("string area"))?;
        if b.get(str_off + l..)
            .is_some_and(|pad| pad.iter().any(|&x| x != 0))
        {
            return Err(bad("string padding is not zero"));
        }
        let mut entries: Vec<(String, Aabb)> = Vec::with_capacity(n);
        let mut next = 0usize;
        for i in 0..n {
            let o = 32 + i * 32;
            let path_off = u32_at(o).ok_or_else(|| bad("truncated"))? as usize;
            let path_len = u32_at(o + 4).ok_or_else(|| bad("truncated"))? as usize;
            if path_off != next {
                return Err(bad("paths are not consecutive"));
            }
            let end = path_off
                .checked_add(path_len)
                .ok_or_else(|| bad("overflow"))?;
            let raw = strings
                .get(path_off..end)
                .ok_or_else(|| bad("path out of range"))?;
            next = end;
            let path = core::str::from_utf8(raw).map_err(|_| bad("path is not UTF-8"))?;
            let mut f = [0.0_f32; 6];
            for (k, slot) in f.iter_mut().enumerate() {
                let v = f32_at(o + 8 + k * 4).ok_or_else(|| bad("truncated"))?;
                if !v.is_finite() {
                    return Err(bad("non-finite float"));
                }
                *slot = v;
            }
            if entries.last().is_some_and(|(p, _)| p.as_bytes() >= raw) {
                return Err(bad("entries are not sorted by unique path"));
            }
            entries.push((
                path.to_owned(),
                Aabb {
                    min: Vec3::new(f[0], f[1], f[2]),
                    max: Vec3::new(f[3], f[4], f[5]),
                },
            ));
        }
        if next != l {
            return Err(bad("unused string bytes"));
        }
        Ok(ModelBounds { entries })
    }
}
