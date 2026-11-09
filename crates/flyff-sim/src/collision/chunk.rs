//! The `FLYC` v1 per-land collision chunk: types and codec (Plan/24 §10.7, Plan/31 §10.2; WP-SIM-02).
//!
//! `encode_chunk` is the only writer (called by the builder of Plan/24 and by `testkit::scenes`),
//! `decode_chunk` the only reader (server loader, browser `SimWorld.load_chunk`). The decoder validates
//! every length, offset and index and returns [`SimError::BadChunk`] instead of panicking
//! (Plan/31 §2.2 D-6); decoding is O(n).
//!
//! # Layout (little endian)
//!
//! A 32-byte header, `section_count` 16-byte section entries, then the sections `TERR`, `WATR`, `SMSH`,
//! `SOBJ`, `LINK` (absent ones omitted), each starting at a multiple of 8 (zero padding before it). The
//! file ends with the last section (no trailing padding). See Plan/24 §10.7 for every field.
//!
//! # Equality
//!
//! [`Heights`] and [`Attrs`] compare by value (the stored encoding is an implementation detail), so
//! `decode_chunk(&encode_chunk(&c)) == c` holds for every valid chunk `c`; `encode_chunk` always picks
//! the canonical encoding (Plan/24 §10.7.2).

use crate::types::SimError;
use flyff_common::math::{D3dMatrix, Vec3};

/// `FLYC` format version (`CHUNK_FORMAT_VERSION`, Plan/24 §10.7.1).
pub const CHUNK_FORMAT_VERSION: u16 = 1;
/// Header flag: the `.lnd` file existed.
pub const FLAG_LAND_PRESENT: u16 = 0x1;
/// Header flag: the chunk has an `SOBJ` section.
pub const FLAG_HAS_OBJECTS: u16 = 0x2;
/// Land edge length in heightmap cells (`MAP_SIZE`).
pub const MAP_SIZE: usize = 128;
/// Vertices per land side (`MAP_SIZE + 1`).
pub const VERTS_PER_SIDE: usize = MAP_SIZE + 1;
/// Vertices per land (`129 * 129`).
pub const VERT_COUNT: usize = VERTS_PER_SIDE * VERTS_PER_SIDE;
/// Water/cloud patches per land (`16 * 16`).
pub const PATCH_COUNT: usize = 256;
/// `Q16` height quantisation step (Plan/24 §10.7.2).
pub const Q16_STEP: f32 = 0.031_25;
/// `SOBJ.flags` bit 0: use the always-hit OBB test (`obj_guildcombatground.o3d`).
pub const INSTANCE_FLAG_ALWAYS_OBB: u8 = 0x1;
/// `SOBJ.flags` bit 1: the camera test skips this instance (`MODELELEM.m_bTrans`).
pub const INSTANCE_FLAG_TRANS: u8 = 0x2;
/// Number of static link levels (`MAX_LINKLEVEL`).
pub const MAX_LINK_LEVEL: usize = 7;

const HEADER_LEN: usize = 32;
const SECTION_ENTRY_LEN: usize = 16;
const INSTANCE_LEN: usize = 248;
const MESH_DESC_LEN: usize = 48;
const LINK_CELL_LEN: usize = 12;

const TAG_TERR: [u8; 4] = *b"TERR";
const TAG_WATR: [u8; 4] = *b"WATR";
const TAG_SMSH: [u8; 4] = *b"SMSH";
const TAG_SOBJ: [u8; 4] = *b"SOBJ";
const TAG_LINK: [u8; 4] = *b"LINK";

/// Index of the heightmap vertex `(x, z)` inside a land (`z * 129 + x`), or `None` when out of range.
#[inline]
pub fn vertex_index(x: usize, z: usize) -> Option<usize> {
    if x < VERTS_PER_SIDE && z < VERTS_PER_SIDE {
        Some(z * VERTS_PER_SIDE + x)
    } else {
        None
    }
}

/// The 129x129 stripped heights of a land (`index = z * 129 + x`), in one of the three `TERR` encodings.
#[derive(Clone, Debug)]
pub enum Heights {
    /// Every vertex has this height (no 66 KB array).
    Uniform(f32),
    /// `base + (k as f32) * step` per vertex with `k: u16`.
    Q16 {
        /// Base height (the minimum).
        base: f32,
        /// Quantisation step (`Q16_STEP`).
        step: f32,
        /// `VERT_COUNT` quantised values.
        data: Box<[u16]>,
    },
    /// Plain `f32` heights (`VERT_COUNT` values).
    F32(Box<[f32]>),
}

impl Heights {
    /// Builds heights from exactly [`VERT_COUNT`] values, choosing the canonical encoding
    /// (`UNIFORM` > `Q16` > `F32`, Plan/24 §10.7.2).
    pub fn from_values(values: &[f32]) -> Result<Heights, SimError> {
        if values.len() != VERT_COUNT {
            return Err(SimError::BadChunk("heights need 129*129 values"));
        }
        let first = values.first().copied().unwrap_or(0.0);
        if values.iter().all(|v| v.to_bits() == first.to_bits()) {
            return Ok(Heights::Uniform(first));
        }
        if let Some((base, data)) = try_q16(values) {
            return Ok(Heights::Q16 {
                base,
                step: Q16_STEP,
                data,
            });
        }
        Ok(Heights::F32(values.to_vec().into_boxed_slice()))
    }

    /// Height of vertex `idx` (`z * 129 + x`); `0.0` for an out-of-range index of a dense encoding.
    #[inline]
    pub fn get(&self, idx: usize) -> f32 {
        match self {
            Heights::Uniform(v) => *v,
            Heights::Q16 { base, step, data } => match data.get(idx) {
                Some(k) => *base + f32::from(*k) * *step,
                None => 0.0,
            },
            Heights::F32(d) => d.get(idx).copied().unwrap_or(0.0),
        }
    }

    /// All `VERT_COUNT` values.
    pub fn to_values(&self) -> Vec<f32> {
        (0..VERT_COUNT).map(|i| self.get(i)).collect()
    }

    /// `true` for the `UNIFORM` form.
    pub fn is_uniform(&self) -> bool {
        matches!(self, Heights::Uniform(_))
    }
}

impl PartialEq for Heights {
    fn eq(&self, other: &Self) -> bool {
        if let (Heights::Uniform(a), Heights::Uniform(b)) = (self, other) {
            return a.to_bits() == b.to_bits();
        }
        (0..VERT_COUNT).all(|i| self.get(i).to_bits() == other.get(i).to_bits())
    }
}

fn try_q16(values: &[f32]) -> Option<(f32, Box<[u16]>)> {
    let mut base = *values.first()?;
    for &v in values {
        if !v.is_finite() {
            return None;
        }
        if v < base {
            base = v;
        }
    }
    let mut data = Vec::with_capacity(values.len());
    for &v in values {
        // (v - base) / 0.03125 == (v - base) * 32 (exact, power of two)
        let kf = (v - base) * 32.0;
        if !(0.0..=65535.0).contains(&kf) {
            return None;
        }
        let k = (kf + 0.5) as u32; // BOUNDS: 0 <= kf + 0.5 < 65536.5
        let k = u16::try_from(k).ok()?;
        let back = base + f32::from(k) * Q16_STEP;
        if back.to_bits() != v.to_bits() {
            return None;
        }
        data.push(k);
    }
    Some((base, data.into_boxed_slice()))
}

/// The 129x129 attribute classes (`HATTR_*`, `0..=4`) of a land (`index = z * 129 + x`).
#[derive(Clone, Debug)]
pub enum Attrs {
    /// Every vertex has this class.
    Uniform(u8),
    /// One class per vertex (`VERT_COUNT` values).
    Dense(Box<[u8]>),
}

impl Attrs {
    /// Builds attributes from exactly [`VERT_COUNT`] values, choosing `UNIFORM` when possible.
    pub fn from_values(values: &[u8]) -> Result<Attrs, SimError> {
        if values.len() != VERT_COUNT {
            return Err(SimError::BadChunk("attributes need 129*129 values"));
        }
        let first = values.first().copied().unwrap_or(0);
        if values.iter().all(|&v| v == first) {
            Ok(Attrs::Uniform(first))
        } else {
            Ok(Attrs::Dense(values.to_vec().into_boxed_slice()))
        }
    }

    /// Class of vertex `idx` (`z * 129 + x`); `0` for an out-of-range index of the dense form.
    #[inline]
    pub fn get(&self, idx: usize) -> u8 {
        match self {
            Attrs::Uniform(v) => *v,
            Attrs::Dense(d) => d.get(idx).copied().unwrap_or(0),
        }
    }

    /// `true` for the `UNIFORM` form.
    pub fn is_uniform(&self) -> bool {
        matches!(self, Attrs::Uniform(_))
    }
}

impl PartialEq for Attrs {
    fn eq(&self, other: &Self) -> bool {
        if let (Attrs::Uniform(a), Attrs::Uniform(b)) = (self, other) {
            return a == b;
        }
        (0..VERT_COUNT).all(|i| self.get(i) == other.get(i))
    }
}

/// An object-space collision mesh (`SMSH`, Plan/24 §10.7.4): positions of the o3d collision body and
/// its triangle list in file order (the original returns on the first triangle hit).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct CollisionMesh {
    /// `mdlObj` model index (`m_dwIndex`).
    pub model_index: u32,
    /// Vertex positions (model space).
    pub positions: Vec<[f32; 3]>,
    /// Triangles as indices into `positions`.
    pub tris: Vec<[u16; 3]>,
    /// Model AABB minimum (the o3d header box).
    pub aabb_min: [f32; 3],
    /// Model AABB maximum.
    pub aabb_max: [f32; 3],
}

impl CollisionMesh {
    /// Object-space position of vertex `i`.
    #[inline]
    pub fn position(&self, i: u16) -> Option<Vec3> {
        self.positions
            .get(usize::from(i))
            .map(|p| Vec3::new(p[0], p[1], p[2]))
    }

    /// The three vertices of triangle `t` (object space).
    #[inline]
    pub fn triangle(&self, t: usize) -> Option<[Vec3; 3]> {
        let tri = self.tris.get(t)?;
        Some([
            self.position(tri[0])?,
            self.position(tri[1])?,
            self.position(tri[2])?,
        ])
    }
}

/// An oriented bounding box (`BBOX`): centre, signed half extents (the V15 test uses `|extent|`) and the
/// three unscaled rotation rows.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Obb {
    /// Box centre in world space.
    pub center: Vec3,
    /// Half extents per axis (signed as stored).
    pub extent: [f32; 3],
    /// Axis `i` = row `i` of the rotation matrix.
    pub axis: [Vec3; 3],
}

/// One static object instance of a chunk (`SOBJ`, Plan/24 §10.7.5).
#[derive(Clone, PartialEq, Debug)]
pub struct StaticInstance {
    /// Index into [`LandChunk::meshes`].
    pub mesh: u32,
    /// `mdlObj` model index (must equal the mesh's).
    pub model_index: u32,
    /// Land file the record came from (x).
    pub src_land_x: u16,
    /// Land file the record came from (z).
    pub src_land_z: u16,
    /// Position in that file's object list.
    pub src_ordinal: u32,
    /// World position (file position `x*4`, `z*4` plus the land offset).
    pub pos: Vec3,
    /// `angle_y`, `ang_x`, `ang_z` in degrees as read.
    pub angles: [f32; 3],
    /// Scale.
    pub scale: Vec3,
    /// Link level `0..=6` (Plan/24 §10.7.6).
    pub link_level: u8,
    /// `INSTANCE_FLAG_*` bits.
    pub flags: u8,
    /// `S * R * T` (D3DX row-vector convention).
    pub world: D3dMatrix,
    /// Inverse of `world`.
    pub inv_world: D3dMatrix,
    /// The oriented bounding box.
    pub obb: Obb,
}

/// One static link-map cell (`LINK`, Plan/24 §10.7.6): global cell coordinates at `level` and the
/// instance indices in iteration order.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LinkCell {
    /// Link level `0..=6`.
    pub level: u8,
    /// Global cell x at that level.
    pub cell_x: u16,
    /// Global cell z at that level.
    pub cell_z: u16,
    /// Instance indices into [`LandChunk::instances`], in list order.
    pub instances: Vec<u32>,
}

/// The decoded content of one `FLYC` chunk (Plan/31 §10.2).
#[derive(Clone, PartialEq, Debug)]
pub struct LandChunk {
    /// Land x coordinate.
    pub land_x: u16,
    /// Land z coordinate.
    pub land_z: u16,
    /// World width in lands.
    pub lands_w: u16,
    /// World height in lands.
    pub lands_h: u16,
    /// World units per heightmap cell (1, 2 or 4).
    pub mpu: u8,
    /// `.lnd` file existed (`LAND_PRESENT`).
    pub land_present: bool,
    /// Stripped heights (server heightmap view, edge rows/columns from the neighbours).
    pub heights: Heights,
    /// Attribute classes.
    pub attrs: Attrs,
    /// `WATERHEIGHT[256]` as `{ height, texture }` pairs, patch index `px + pz * 16`.
    pub water: [u8; 512],
    /// Collision meshes used by this chunk's instances, sorted by `model_index`.
    pub meshes: Vec<CollisionMesh>,
    /// Static instances.
    pub instances: Vec<StaticInstance>,
    /// Link cells sorted by `(level, cell_z, cell_x)`.
    pub links: Vec<LinkCell>,
}

impl LandChunk {
    /// A land without a file: uniform zero terrain, no water, no objects (the server's memset rule).
    pub fn empty(
        land_x: u16,
        land_z: u16,
        lands_w: u16,
        lands_h: u16,
        mpu: u8,
        land_present: bool,
    ) -> LandChunk {
        LandChunk {
            land_x,
            land_z,
            lands_w,
            lands_h,
            mpu,
            land_present,
            heights: Heights::Uniform(0.0),
            attrs: Attrs::Uniform(0),
            water: [0; 512],
            meshes: Vec::new(),
            instances: Vec::new(),
            links: Vec::new(),
        }
    }

    /// Water/cloud patch `(height, texture)` of patch `(px, pz)` (`0..16` each).
    pub fn water_patch(&self, px: usize, pz: usize) -> Option<(u8, u8)> {
        if px >= 16 || pz >= 16 {
            return None;
        }
        let i = (px + pz * 16) * 2;
        Some((*self.water.get(i)?, *self.water.get(i + 1)?))
    }

    /// The highest link level used by this chunk's cells, plus one (`0` when there are no cells).
    pub fn link_levels_in_use(&self) -> usize {
        self.links
            .iter()
            .map(|c| usize::from(c.level) + 1)
            .max()
            .unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------------------------------
// Link geometry shared by the codec (validation) and the runtime walk (Plan/31 §10.4)
// ---------------------------------------------------------------------------------------------------

/// `fStaticGrid` (Plan/31 §10.4).
pub const STATIC_GRID: [f32; MAX_LINK_LEVEL] = [4.0, 8.0, 16.0, 32.0, 64.0, 128.0, 256.0];

/// `(int)(fStaticGrid[level] * (0.25 * mpu))` — the cell size in world units (`CLinkMap::Init`).
pub fn link_cell_size(level: usize, mpu: u32) -> i32 {
    let grid = STATIC_GRID.get(level).copied().unwrap_or(256.0);
    let mul = 0.25_f32 * (mpu as f32);
    flyff_common::math::ftoi_x86(grid * mul)
}

/// Link cells per land side at `level`: `(128 * mpu) / cell_size`.
pub fn link_width(level: usize, mpu: u32) -> i32 {
    let size = link_cell_size(level, mpu);
    if size <= 0 {
        return 1;
    }
    (MAP_SIZE as i32 * mpu as i32) / size
}

// ---------------------------------------------------------------------------------------------------
// Reader
// ---------------------------------------------------------------------------------------------------

fn bad(msg: &'static str) -> SimError {
    SimError::BadChunk(msg)
}

struct Rd<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Rd<'a> {
    fn new(b: &'a [u8]) -> Rd<'a> {
        Rd { b, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], SimError> {
        let end = self.pos.checked_add(n).ok_or_else(|| bad("overflow"))?;
        let s = self.b.get(self.pos..end).ok_or_else(|| bad("truncated"))?;
        self.pos = end;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8, SimError> {
        Ok(*self.take(1)?.first().ok_or_else(|| bad("truncated"))?)
    }

    fn u16(&mut self) -> Result<u16, SimError> {
        let s = self.take(2)?;
        Ok(u16::from_le_bytes(
            s.try_into().map_err(|_| bad("truncated"))?,
        ))
    }

    fn u32(&mut self) -> Result<u32, SimError> {
        let s = self.take(4)?;
        Ok(u32::from_le_bytes(
            s.try_into().map_err(|_| bad("truncated"))?,
        ))
    }

    fn f32(&mut self) -> Result<f32, SimError> {
        let s = self.take(4)?;
        Ok(f32::from_le_bytes(
            s.try_into().map_err(|_| bad("truncated"))?,
        ))
    }

    fn finite_f32(&mut self) -> Result<f32, SimError> {
        let v = self.f32()?;
        if v.is_finite() {
            Ok(v)
        } else {
            Err(bad("non-finite float"))
        }
    }

    fn f32x3(&mut self) -> Result<[f32; 3]> {
        Ok([self.finite_f32()?, self.finite_f32()?, self.finite_f32()?])
    }

    fn zero_bytes(&mut self, n: usize) -> Result<(), SimError> {
        if self.take(n)?.iter().all(|&b| b == 0) {
            Ok(())
        } else {
            Err(bad("reserved bytes are not zero"))
        }
    }

    fn zero_u32(&mut self) -> Result<(), SimError> {
        if self.u32()? == 0 {
            Ok(())
        } else {
            Err(bad("reserved field is not zero"))
        }
    }

    fn matrix(&mut self) -> Result<D3dMatrix, SimError> {
        let mut m = [0.0_f32; 16];
        for slot in &mut m {
            *slot = self.finite_f32()?;
        }
        Ok(D3dMatrix(m))
    }
}

type Result<T, E = SimError> = core::result::Result<T, E>;

fn vec3(a: [f32; 3]) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}

fn align8(n: usize) -> Option<usize> {
    n.checked_add(7).map(|v| v & !7)
}

#[derive(Clone, Copy)]
struct Section<'a> {
    data: &'a [u8],
}

/// Decodes and validates a `FLYC` v1 chunk (Plan/24 §10.7, rejection rules of §10.7.7).
///
/// Any malformed input (wrong magic or version, `file_len` mismatch, sections outside the file or
/// overlapping, unknown or duplicate tags, count x record size overflows, indices out of range, non-zero
/// reserved bytes, non-finite floats) returns [`SimError::BadChunk`]; the function never panics.
pub fn decode_chunk(bytes: &[u8]) -> Result<LandChunk> {
    let mut h = Rd::new(bytes);
    if h.take(4)? != b"FLYC" {
        return Err(bad("wrong magic"));
    }
    if h.u16()? != CHUNK_FORMAT_VERSION {
        return Err(bad("unsupported version"));
    }
    let flags = h.u16()?;
    if flags & !(FLAG_LAND_PRESENT | FLAG_HAS_OBJECTS) != 0 {
        return Err(bad("unknown header flags"));
    }
    let file_len = h.u32()?;
    if usize::try_from(file_len).ok() != Some(bytes.len()) {
        return Err(bad("file_len does not match the data length"));
    }
    let land_x = h.u16()?;
    let land_z = h.u16()?;
    let lands_w = h.u16()?;
    let lands_h = h.u16()?;
    let mpu = h.u8()?;
    let section_count = usize::from(h.u8()?);
    h.zero_bytes(10)?;
    if !matches!(mpu, 1 | 2 | 4) {
        return Err(bad("mpu must be 1, 2 or 4"));
    }
    if lands_w == 0 || lands_h == 0 || land_x >= lands_w || land_z >= lands_h {
        return Err(bad("land coordinates outside the world"));
    }

    // section table
    let table_end = HEADER_LEN
        .checked_add(
            section_count
                .checked_mul(SECTION_ENTRY_LEN)
                .ok_or_else(|| bad("overflow"))?,
        )
        .ok_or_else(|| bad("overflow"))?;
    if table_end > bytes.len() {
        return Err(bad("section table outside the file"));
    }
    let mut terr = None;
    let mut watr = None;
    let mut smsh = None;
    let mut sobj = None;
    let mut link = None;
    let mut prev_rank = 0u8;
    let mut prev_end = table_end;
    for _ in 0..section_count {
        let tag: [u8; 4] = h.take(4)?.try_into().map_err(|_| bad("truncated"))?;
        let offset = usize::try_from(h.u32()?).map_err(|_| bad("offset"))?;
        let len = usize::try_from(h.u32()?).map_err(|_| bad("length"))?;
        h.zero_u32()?;
        let rank = match tag {
            TAG_TERR => 1,
            TAG_WATR => 2,
            TAG_SMSH => 3,
            TAG_SOBJ => 4,
            TAG_LINK => 5,
            _ => return Err(bad("unknown section tag")),
        };
        if rank <= prev_rank {
            return Err(bad("duplicate or out-of-order section"));
        }
        prev_rank = rank;
        if offset % 8 != 0 {
            return Err(bad("section offset is not 8-aligned"));
        }
        let end = offset.checked_add(len).ok_or_else(|| bad("overflow"))?;
        if offset < prev_end || end > bytes.len() {
            return Err(bad("section outside the file or overlapping"));
        }
        prev_end = end;
        let data = bytes.get(offset..end).ok_or_else(|| bad("section range"))?;
        let sec = Some(Section { data });
        match rank {
            1 => terr = sec,
            2 => watr = sec,
            3 => smsh = sec,
            4 => sobj = sec,
            _ => link = sec,
        }
    }
    let terr = terr.ok_or_else(|| bad("missing TERR section"))?;
    let watr = watr.ok_or_else(|| bad("missing WATR section"))?;
    if (flags & FLAG_HAS_OBJECTS != 0) != sobj.is_some() {
        return Err(bad("HAS_OBJECTS flag disagrees with the SOBJ section"));
    }

    let (heights, attrs) = decode_terr(terr.data)?;
    let water = decode_watr(watr.data)?;
    let meshes = match smsh {
        Some(s) => decode_smsh(s.data)?,
        None => Vec::new(),
    };
    let instances = match sobj {
        Some(s) => decode_sobj(s.data, &meshes)?,
        None => Vec::new(),
    };
    let links = match link {
        Some(s) => decode_link(s.data, &instances, lands_w, lands_h, mpu, land_x, land_z)?,
        None => Vec::new(),
    };
    Ok(LandChunk {
        land_x,
        land_z,
        lands_w,
        lands_h,
        mpu,
        land_present: flags & FLAG_LAND_PRESENT != 0,
        heights,
        attrs,
        water,
        meshes,
        instances,
        links,
    })
}

fn decode_terr(data: &[u8]) -> Result<(Heights, Attrs)> {
    let mut r = Rd::new(data);
    let height_enc = r.u8()?;
    let attr_enc = r.u8()?;
    let uniform_attr = r.u8()?;
    if r.u8()? != 0 {
        return Err(bad("TERR reserved byte"));
    }
    let base = r.finite_f32()?;
    let step = r.finite_f32()?;
    r.zero_u32()?;
    if attr_enc > 1 {
        return Err(bad("unknown attribute encoding"));
    }
    if uniform_attr > 4 || (attr_enc == 1 && uniform_attr != 0) {
        return Err(bad("uniform attribute class"));
    }
    let mut end = 16usize;
    let heights = match height_enc {
        0 => {
            if step.to_bits() != 0 {
                return Err(bad("UNIFORM heights carry a step"));
            }
            Heights::Uniform(base)
        }
        1 => {
            if step.to_bits() != Q16_STEP.to_bits() {
                return Err(bad("Q16 step must be 0.03125"));
            }
            let raw = r.take(VERT_COUNT * 2)?;
            end += VERT_COUNT * 2;
            let data: Vec<u16> = raw
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| {
                    u16::from_le_bytes([
                        c.first().copied().unwrap_or(0),
                        c.get(1).copied().unwrap_or(0),
                    ])
                })
                .collect();
            Heights::Q16 {
                base,
                step,
                data: data.into_boxed_slice(),
            }
        }
        2 => {
            if base.to_bits() != 0 || step.to_bits() != 0 {
                return Err(bad("F32 heights carry a base or step"));
            }
            let raw = r.take(VERT_COUNT * 4)?;
            end += VERT_COUNT * 4;
            let mut vals = Vec::with_capacity(VERT_COUNT);
            for c in raw.as_chunks::<4>().0 {
                let v = f32::from_le_bytes([
                    c.first().copied().unwrap_or(0),
                    c.get(1).copied().unwrap_or(0),
                    c.get(2).copied().unwrap_or(0),
                    c.get(3).copied().unwrap_or(0),
                ]);
                if !v.is_finite() {
                    return Err(bad("non-finite height"));
                }
                vals.push(v);
            }
            Heights::F32(vals.into_boxed_slice())
        }
        _ => return Err(bad("unknown height encoding")),
    };
    let attrs = if attr_enc == 0 {
        Attrs::Uniform(uniform_attr)
    } else {
        let start = align8(end).ok_or_else(|| bad("overflow"))?;
        let pad = start - end;
        r.zero_bytes(pad)?;
        let raw = r.take(VERT_COUNT)?;
        end = start + VERT_COUNT;
        if raw.iter().any(|&a| a > 4) {
            return Err(bad("attribute class out of range"));
        }
        Attrs::Dense(raw.to_vec().into_boxed_slice())
    };
    if end != data.len() {
        return Err(bad("TERR length mismatch"));
    }
    Ok((heights, attrs))
}

fn decode_watr(data: &[u8]) -> Result<[u8; 512]> {
    let arr: [u8; 512] = data.try_into().map_err(|_| bad("WATR must be 512 bytes"))?;
    Ok(arr)
}

fn decode_smsh(data: &[u8]) -> Result<Vec<CollisionMesh>> {
    let mut r = Rd::new(data);
    let count = r.u32()? as usize;
    r.zero_u32()?;
    let descs_end = count
        .checked_mul(MESH_DESC_LEN)
        .and_then(|n| n.checked_add(8))
        .ok_or_else(|| bad("overflow"))?;
    if descs_end > data.len() {
        return Err(bad("SMSH descriptors outside the section"));
    }
    let mut meshes = Vec::with_capacity(count);
    let mut prev_model: Option<u32> = None;
    for _ in 0..count {
        let model_index = r.u32()?;
        let vertex_count = r.u32()? as usize;
        let tri_count = r.u32()? as usize;
        let vertex_offset = r.u32()? as usize;
        let index_offset = r.u32()? as usize;
        r.zero_u32()?;
        let aabb_min = r.f32x3()?;
        let aabb_max = r.f32x3()?;
        if prev_model.is_some_and(|p| model_index <= p) {
            return Err(bad("meshes are not sorted by unique model_index"));
        }
        prev_model = Some(model_index);
        if !vertex_offset.is_multiple_of(4) || !index_offset.is_multiple_of(4) {
            return Err(bad("mesh data offsets are not 4-aligned"));
        }
        if vertex_offset < descs_end || index_offset < descs_end {
            return Err(bad("mesh data overlaps the descriptors"));
        }
        let vend = vertex_count
            .checked_mul(12)
            .and_then(|n| n.checked_add(vertex_offset))
            .ok_or_else(|| bad("overflow"))?;
        let iend = tri_count
            .checked_mul(6)
            .and_then(|n| n.checked_add(index_offset))
            .ok_or_else(|| bad("overflow"))?;
        let vraw = data
            .get(vertex_offset..vend)
            .ok_or_else(|| bad("mesh vertices outside the section"))?;
        let iraw = data
            .get(index_offset..iend)
            .ok_or_else(|| bad("mesh indices outside the section"))?;
        let mut positions = Vec::with_capacity(vertex_count);
        let mut vr = Rd::new(vraw);
        for _ in 0..vertex_count {
            positions.push(vr.f32x3()?);
        }
        let mut tris = Vec::with_capacity(tri_count);
        let mut ir = Rd::new(iraw);
        for _ in 0..tri_count {
            let t = [ir.u16()?, ir.u16()?, ir.u16()?];
            if t.iter().any(|&i| usize::from(i) >= vertex_count) {
                return Err(bad("mesh index out of range"));
            }
            tris.push(t);
        }
        meshes.push(CollisionMesh {
            model_index,
            positions,
            tris,
            aabb_min,
            aabb_max,
        });
    }
    Ok(meshes)
}

fn decode_sobj(data: &[u8], meshes: &[CollisionMesh]) -> Result<Vec<StaticInstance>> {
    let mut r = Rd::new(data);
    let count = r.u32()? as usize;
    r.zero_u32()?;
    let need = count
        .checked_mul(INSTANCE_LEN)
        .and_then(|n| n.checked_add(8))
        .ok_or_else(|| bad("overflow"))?;
    if need != data.len() {
        return Err(bad("SOBJ length mismatch"));
    }
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let mesh = r.u32()?;
        let model_index = r.u32()?;
        let src_land_x = r.u16()?;
        let src_land_z = r.u16()?;
        let src_ordinal = r.u32()?;
        let pos = r.f32x3()?;
        let angles = r.f32x3()?;
        let scale = r.f32x3()?;
        let link_level = r.u8()?;
        let flags = r.u8()?;
        r.zero_bytes(2)?;
        let world = r.matrix()?;
        let inv_world = r.matrix()?;
        let center = r.f32x3()?;
        let extent = r.f32x3()?;
        let a0 = r.f32x3()?;
        let a1 = r.f32x3()?;
        let a2 = r.f32x3()?;
        r.zero_bytes(4)?;
        let m = meshes
            .get(mesh as usize)
            .ok_or_else(|| bad("instance mesh index out of range"))?;
        if m.model_index != model_index {
            return Err(bad("instance model_index differs from its mesh"));
        }
        if usize::from(link_level) >= MAX_LINK_LEVEL {
            return Err(bad("link level out of range"));
        }
        if flags & !(INSTANCE_FLAG_ALWAYS_OBB | INSTANCE_FLAG_TRANS) != 0 {
            return Err(bad("unknown instance flags"));
        }
        out.push(StaticInstance {
            mesh,
            model_index,
            src_land_x,
            src_land_z,
            src_ordinal,
            pos: vec3(pos),
            angles,
            scale: vec3(scale),
            link_level,
            flags,
            world,
            inv_world,
            obb: Obb {
                center: vec3(center),
                extent,
                axis: [vec3(a0), vec3(a1), vec3(a2)],
            },
        });
    }
    Ok(out)
}

fn decode_link(
    data: &[u8],
    instances: &[StaticInstance],
    lands_w: u16,
    lands_h: u16,
    mpu: u8,
    land_x: u16,
    land_z: u16,
) -> Result<Vec<LinkCell>> {
    let mut r = Rd::new(data);
    let cell_count = r.u32()? as usize;
    let index_count = r.u32()? as usize;
    let need = cell_count
        .checked_mul(LINK_CELL_LEN)
        .and_then(|n| n.checked_add(index_count.checked_mul(4)?))
        .and_then(|n| n.checked_add(8))
        .ok_or_else(|| bad("overflow"))?;
    if need != data.len() {
        return Err(bad("LINK length mismatch"));
    }
    let idx_base = 8 + cell_count * LINK_CELL_LEN;
    let idx_raw = data
        .get(idx_base..)
        .ok_or_else(|| bad("LINK indices outside the section"))?;
    let mut cells = Vec::with_capacity(cell_count);
    let mut prev_key: Option<(u8, u16, u16)> = None;
    for _ in 0..cell_count {
        let level = r.u8()?;
        if r.u8()? != 0 {
            return Err(bad("LINK reserved byte"));
        }
        let cell_x = r.u16()?;
        let cell_z = r.u16()?;
        let count = usize::from(r.u16()?);
        let first = r.u32()? as usize;
        if usize::from(level) >= MAX_LINK_LEVEL {
            return Err(bad("link cell level out of range"));
        }
        if count == 0 {
            return Err(bad("empty link cell"));
        }
        let key = (level, cell_z, cell_x);
        if prev_key.is_some_and(|p| key <= p) {
            return Err(bad("link cells are not strictly sorted"));
        }
        prev_key = Some(key);
        let width = link_width(usize::from(level), u32::from(mpu));
        let width = u32::try_from(width).map_err(|_| bad("link width"))?;
        if u32::from(cell_x) / width != u32::from(land_x)
            || u32::from(cell_z) / width != u32::from(land_z)
            || u32::from(cell_x) >= width * u32::from(lands_w)
            || u32::from(cell_z) >= width * u32::from(lands_h)
        {
            return Err(bad("link cell does not belong to this land"));
        }
        let end = first.checked_add(count).ok_or_else(|| bad("overflow"))?;
        if end > index_count {
            return Err(bad("link cell range outside the index array"));
        }
        let raw = idx_raw
            .get(first * 4..end * 4)
            .ok_or_else(|| bad("link cell range"))?;
        let mut list = Vec::with_capacity(count);
        for c in raw.as_chunks::<4>().0 {
            let i = u32::from_le_bytes([
                c.first().copied().unwrap_or(0),
                c.get(1).copied().unwrap_or(0),
                c.get(2).copied().unwrap_or(0),
                c.get(3).copied().unwrap_or(0),
            ]);
            let inst = instances
                .get(i as usize)
                .ok_or_else(|| bad("link instance index out of range"))?;
            if inst.link_level != level {
                return Err(bad("instance link level differs from its cell"));
            }
            list.push(i);
        }
        cells.push(LinkCell {
            level,
            cell_x,
            cell_z,
            instances: list,
        });
    }
    Ok(cells)
}

// ---------------------------------------------------------------------------------------------------
// Writer
// ---------------------------------------------------------------------------------------------------

fn put_u16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn put_f32(out: &mut Vec<u8>, v: f32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn put_f32x3(out: &mut Vec<u8>, v: [f32; 3]) {
    for c in v {
        put_f32(out, c);
    }
}
fn put_vec3(out: &mut Vec<u8>, v: Vec3) {
    put_f32(out, v.x);
    put_f32(out, v.y);
    put_f32(out, v.z);
}
fn put_matrix(out: &mut Vec<u8>, m: &D3dMatrix) {
    for v in m.0 {
        put_f32(out, v);
    }
}
fn pad_to(out: &mut Vec<u8>, align: usize) {
    while !out.len().is_multiple_of(align) {
        out.push(0);
    }
}

fn encode_terr(c: &LandChunk) -> Vec<u8> {
    let mut out = Vec::new();
    let values = c.heights.to_values();
    let canonical = match Heights::from_values(&values) {
        Ok(h) => h,
        Err(_) => Heights::Uniform(0.0),
    };
    let attr_values: Vec<u8> = (0..VERT_COUNT).map(|i| c.attrs.get(i)).collect();
    let attrs = match Attrs::from_values(&attr_values) {
        Ok(a) => a,
        Err(_) => Attrs::Uniform(0),
    };
    let (height_enc, base, step) = match &canonical {
        Heights::Uniform(v) => (0u8, *v, 0.0_f32),
        Heights::Q16 { base, step, .. } => (1u8, *base, *step),
        Heights::F32(_) => (2u8, 0.0, 0.0),
    };
    let (attr_enc, uniform_attr) = match &attrs {
        Attrs::Uniform(a) => (0u8, *a),
        Attrs::Dense(_) => (1u8, 0u8),
    };
    out.extend_from_slice(&[height_enc, attr_enc, uniform_attr, 0]);
    put_f32(&mut out, base);
    put_f32(&mut out, step);
    put_u32(&mut out, 0);
    match &canonical {
        Heights::Uniform(_) => {}
        Heights::Q16 { data, .. } => {
            for k in data.iter() {
                put_u16(&mut out, *k);
            }
        }
        Heights::F32(d) => {
            for v in d.iter() {
                put_f32(&mut out, *v);
            }
        }
    }
    if let Attrs::Dense(d) = &attrs {
        pad_to(&mut out, 8);
        out.extend_from_slice(d);
    }
    out
}

fn encode_smsh(meshes: &[CollisionMesh]) -> Vec<u8> {
    let mut out = Vec::new();
    put_u32(&mut out, meshes.len() as u32); // BOUNDS: mesh counts are far below 2^32
    put_u32(&mut out, 0);
    let mut data_off = 8 + meshes.len() * MESH_DESC_LEN;
    let mut descs = Vec::new();
    let mut blob = Vec::new();
    for m in meshes {
        let vertex_offset = data_off;
        let index_offset = vertex_offset + m.positions.len() * 12;
        let mut vb = Vec::new();
        for p in &m.positions {
            put_f32x3(&mut vb, *p);
        }
        let mut ib = Vec::new();
        for t in &m.tris {
            for i in t {
                put_u16(&mut ib, *i);
            }
        }
        pad_to(&mut ib, 4);
        data_off = index_offset + ib.len();
        put_u32(&mut descs, m.model_index);
        put_u32(&mut descs, m.positions.len() as u32); // BOUNDS: see above
        put_u32(&mut descs, m.tris.len() as u32); // BOUNDS: see above
        put_u32(&mut descs, vertex_offset as u32); // BOUNDS: sections are far below 4 GiB
        put_u32(&mut descs, index_offset as u32); // BOUNDS: see above
        put_u32(&mut descs, 0);
        put_f32x3(&mut descs, m.aabb_min);
        put_f32x3(&mut descs, m.aabb_max);
        blob.extend_from_slice(&vb);
        blob.extend_from_slice(&ib);
    }
    out.extend_from_slice(&descs);
    out.extend_from_slice(&blob);
    out
}

fn encode_sobj(instances: &[StaticInstance]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + instances.len() * INSTANCE_LEN);
    put_u32(&mut out, instances.len() as u32); // BOUNDS: instance counts are far below 2^32
    put_u32(&mut out, 0);
    for i in instances {
        put_u32(&mut out, i.mesh);
        put_u32(&mut out, i.model_index);
        put_u16(&mut out, i.src_land_x);
        put_u16(&mut out, i.src_land_z);
        put_u32(&mut out, i.src_ordinal);
        put_vec3(&mut out, i.pos);
        put_f32x3(&mut out, i.angles);
        put_vec3(&mut out, i.scale);
        out.push(i.link_level);
        out.push(i.flags);
        out.extend_from_slice(&[0, 0]);
        put_matrix(&mut out, &i.world);
        put_matrix(&mut out, &i.inv_world);
        put_vec3(&mut out, i.obb.center);
        put_f32x3(&mut out, i.obb.extent);
        for a in &i.obb.axis {
            put_vec3(&mut out, *a);
        }
        out.extend_from_slice(&[0, 0, 0, 0]);
    }
    out
}

fn encode_link(links: &[LinkCell]) -> Vec<u8> {
    let index_count: usize = links.iter().map(|c| c.instances.len()).sum();
    let mut out = Vec::with_capacity(8 + links.len() * LINK_CELL_LEN + index_count * 4);
    put_u32(&mut out, links.len() as u32); // BOUNDS: cell counts are far below 2^32
    put_u32(&mut out, index_count as u32); // BOUNDS: see above
    let mut first = 0u32;
    for c in links {
        out.push(c.level);
        out.push(0);
        put_u16(&mut out, c.cell_x);
        put_u16(&mut out, c.cell_z);
        put_u16(
            &mut out,
            c.instances.len().min(usize::from(u16::MAX)) as u16,
        ); // BOUNDS: clamped
        put_u32(&mut out, first);
        first = first.wrapping_add(c.instances.len() as u32); // BOUNDS: see above
    }
    for c in links {
        for i in &c.instances {
            put_u32(&mut out, *i);
        }
    }
    out
}

/// Serialises a chunk as `FLYC` v1 (Plan/24 §10.7). Heights and attributes are written in their
/// canonical encoding; sections are 8-aligned; sections without content are omitted. The only
/// writer of the format (24's builder and `testkit::scenes` call it).
pub fn encode_chunk(c: &LandChunk) -> Vec<u8> {
    let mut sections: Vec<([u8; 4], Vec<u8>)> = Vec::with_capacity(5);
    sections.push((TAG_TERR, encode_terr(c)));
    sections.push((TAG_WATR, c.water.to_vec()));
    if !c.meshes.is_empty() {
        sections.push((TAG_SMSH, encode_smsh(&c.meshes)));
    }
    let has_objects = !c.instances.is_empty();
    if has_objects {
        sections.push((TAG_SOBJ, encode_sobj(&c.instances)));
    }
    if !c.links.is_empty() {
        sections.push((TAG_LINK, encode_link(&c.links)));
    }

    let table_end = HEADER_LEN + sections.len() * SECTION_ENTRY_LEN;
    let mut entries = Vec::with_capacity(sections.len());
    let mut cursor = table_end;
    for (tag, data) in &sections {
        let offset = (cursor + 7) & !7;
        entries.push((*tag, offset, data.len()));
        cursor = offset + data.len();
    }
    let file_len = cursor;

    let mut out = Vec::with_capacity(file_len);
    out.extend_from_slice(b"FLYC");
    put_u16(&mut out, CHUNK_FORMAT_VERSION);
    let mut flags = 0u16;
    if c.land_present {
        flags |= FLAG_LAND_PRESENT;
    }
    if has_objects {
        flags |= FLAG_HAS_OBJECTS;
    }
    put_u16(&mut out, flags);
    put_u32(&mut out, file_len as u32); // BOUNDS: chunks are far below 4 GiB
    put_u16(&mut out, c.land_x);
    put_u16(&mut out, c.land_z);
    put_u16(&mut out, c.lands_w);
    put_u16(&mut out, c.lands_h);
    out.push(c.mpu);
    out.push(sections.len() as u8); // BOUNDS: at most 5 sections
    out.extend_from_slice(&[0; 10]);
    for (tag, offset, len) in &entries {
        out.extend_from_slice(tag);
        put_u32(&mut out, *offset as u32); // BOUNDS: see above
        put_u32(&mut out, *len as u32); // BOUNDS: see above
        put_u32(&mut out, 0);
    }
    for ((_, data), (_, offset, _)) in sections.iter().zip(&entries) {
        pad_to(&mut out, 8);
        debug_assert_eq!(out.len(), *offset);
        out.extend_from_slice(data);
    }
    out
}
