//! `.o3d` mesh files, versions 20-22 (Plan/30 §6; ref/18 §4; `CObject3D::LoadObject`,
//! src:_Common/Object3D.cpp:913-1186, `LoadGMObject` :1266-1489, `LoadTMAni` :1494-1504).
//!
//! The model layer is a faithful, lossless, left-handed image of the file ([`O3dFile`]); the browser
//! oriented render pack lives in [`crate::pack::o3d`]. Parsing is lenient about semantics (it reports
//! [`WarningCode`](crate::WarningCode)s such as a header-name mismatch) and strict about structure.
//!
//! Top-level layout (Plan/30 §6.1): `u8 nameLen (< 64)`, name XOR `0xCD`, `i32 ver (>= 20)`, `i32 id`,
//! `VEC3 force1/force2` (+ `force3/force4` for ver >= 22), `f32 scrollU/V`, `u8[16]` reserved,
//! `VEC3 bbMin/bbMax`, `f32 perSlerp`, `i32 maxFrame`, `i32 nEvent` + events (<= 8), `i32 hasColl` + a
//! collision `GMBODY`, `i32 bLOD`, `i32 nBone` (+ `baseBone`, `baseBoneInv`, MOTIONCORE when
//! `maxFrame > 0`, `bSendVS`), `i32 poolSize`, 1 or 3 groups of `i32 nObject` + `GMOBJECT`s, and for
//! ver >= 21 an optional `i32 nAttr` + `MOTION_ATTR[maxFrame]` when bytes remain.

use crate::motion::{MotionAttr, MotionCore, TmKey};
use crate::raw::{D3dMatrix, RawStr, Vec3};

mod parse;

pub use parse::parse;

/// Constants of the `.o3d` format and its material blocks (ref/18 §8.2).
pub mod consts {
    /// `XE_REFLECT`: sphere-mapped reflection stage.
    pub const XE_REFLECT: u32 = 0x1;
    /// `XE_OPACITY`: alpha test with reference [`OPACITY_ALPHA_REF`] (and blend if `amount < 255`).
    pub const XE_OPACITY: u32 = 0x2;
    /// `XE_2SIDE`: no back-face culling.
    pub const XE_2SIDE: u32 = 0x4;
    /// `XE_SELF_ILLUMINATE`: lighting off.
    pub const XE_SELF_ILLUMINATE: u32 = 0x8;
    /// `XE_SPECULAR`: no-op in the original.
    pub const XE_SPECULAR: u32 = 0x10;
    /// `XE_BUMP`: only for the inactive `__YENV` path.
    pub const XE_BUMP: u32 = 0x20;
    /// `XE_HIGHLIGHT_OBJ`: selection highlight blend.
    pub const XE_HIGHLIGHT_OBJ: u32 = 0x2000;
    /// Alpha-test reference value for `XE_OPACITY` blocks (`>=`).
    pub const OPACITY_ALPHA_REF: u8 = 0xB0;
    /// Bit 31 of a GMOBJECT's type word: light object (`m_bLight`), rendered additively at night only.
    pub const LIGHT_OBJECT_BIT: u32 = 0x8000_0000;
    /// Lowest accepted `ver` (`VER_MESH`, Model.h:39).
    pub const VER_MESH: i32 = 20;
    /// First version with the second sword-trail line (`force3`, `force4`).
    pub const VER_FORCE34: i32 = 22;
    /// First version with the optional trailing attribute block.
    pub const VER_ATTRS: i32 = 21;
}

/// Whether the bulk geometry arrays of GMOBJECT bodies are read or skipped (Plan/30 §6.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GeometryMode {
    /// Read everything.
    #[default]
    Full,
    /// Skip the vertex list, vertex buffer, index buffer, `iib` and physique arrays of GMOBJECT
    /// bodies (counts are kept; the collision body, materials, blocks, node animation, embedded
    /// bones/motion and attributes are always loaded). Used by the server-side builders.
    Skip,
}

/// Options of [`parse`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct O3dOptions {
    /// Geometry loading mode.
    pub geometry: GeometryMode,
    /// The file name the model was requested under (directories are ignored). When set, a header name
    /// that differs (ASCII case-insensitively) produces one `O3dNameMismatch` warning (Plan/30 §6.6:
    /// the original refuses to load such files). Without it the parser cannot know and never warns.
    pub file_name: Option<String>,
}

impl O3dOptions {
    /// Full geometry, no requested name.
    pub fn full() -> O3dOptions {
        O3dOptions::default()
    }

    /// Skipped geometry, no requested name.
    pub fn skip() -> O3dOptions {
        O3dOptions {
            geometry: GeometryMode::Skip,
            file_name: None,
        }
    }

    /// The same options with the requested file name set.
    pub fn with_file_name(mut self, name: &str) -> O3dOptions {
        self.file_name = Some(name.to_owned());
        self
    }
}

/// A parsed `.o3d` (Plan/30 §6.5).
#[derive(Debug, Clone, PartialEq)]
pub struct O3dFile {
    /// The decoded (XOR `0xCD`) header name, e.g. `Part_fAss01Cap.o3d`.
    pub header_name: RawStr,
    /// `ver`: 20, 21 or 22 in the corpus (anything `>= 20` is accepted).
    pub version: i32,
    /// Serial id, unused.
    pub id: i32,
    /// `force1..force4` sword-trail points; `force3`/`force4` are zero (and not stored) below v22.
    pub force: [Vec3; 4],
    /// `scrollU`, `scrollV` (unused at runtime, kept for round trips).
    pub scroll_uv: [f32; 2],
    /// The 16 reserved bytes (junk: leftover XOR'd name bytes).
    pub reserved: [u8; 16],
    /// Model bounding box minimum (server OBB, picking, model bounds).
    pub bb_min: Vec3,
    /// Model bounding box maximum.
    pub bb_max: Vec3,
    /// Animation advance per tick of the built-in animation.
    pub per_slerp: f32,
    /// Frames of the built-in animation (node TM animation or embedded motion); 0 = static.
    pub max_frame: i32,
    /// Model-space event points (0..=8).
    pub events: Vec<Vec3>,
    /// Collision body (implied `GMT_NORMAL`, 32-byte vertices, **model space**).
    pub collision: Option<GmBody>,
    /// The raw `bLOD` word: non-zero means three LOD groups.
    pub lod: i32,
    /// Embedded skeleton (35 corpus files).
    pub bones: Option<EmbeddedBones>,
    /// The stored `poolSize` (total GMOBJECT count over all groups).
    pub pool_size: i32,
    /// One group, or three (high, medium, low detail) when `lod != 0`.
    pub groups: Vec<LodGroup>,
    /// The optional trailing attribute block.
    pub attrs: AttrBlock,
}

impl O3dFile {
    /// The LOD-0 (highest detail) group, if any.
    pub fn lod0(&self) -> Option<&LodGroup> {
        self.groups.first()
    }

    /// Total GMOBJECT count over all groups.
    pub fn object_count(&self) -> usize {
        self.groups.iter().map(|g| g.objects.len()).sum()
    }

    /// `true` when `max_frame > 0` and at least one LOD-0 object has stored node keys.
    pub fn has_node_animation(&self) -> bool {
        self.max_frame > 0
            && self.lod0().is_some_and(|g| {
                g.objects
                    .iter()
                    .any(|o| o.node_anim.as_ref().is_some_and(|n| n.has_frames != 0))
            })
    }
}

/// Embedded skeleton: the object renders without a `.chr` (Plan/30 §6.1 rows 17-21).
#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddedBones {
    /// `baseBone`: bind-pose world matrix per bone.
    pub base: Vec<D3dMatrix>,
    /// `baseBoneInv`: inverse bind matrices.
    pub base_inv: Vec<D3dMatrix>,
    /// The embedded motion; present only when `max_frame > 0` (31 of the 35 corpus files).
    pub motion: Option<MotionCore>,
    /// `bSendVS`.
    pub send_vs: i32,
}

/// One LOD group.
#[derive(Debug, Clone, PartialEq)]
pub struct LodGroup {
    /// The group's GMOBJECTs in file order (parents precede children).
    pub objects: Vec<GmObject>,
}

/// The kind of a GMOBJECT (the low 16 bits of its type word).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GmType {
    /// `GMT_NORMAL` (0): rigid, 32-byte vertices.
    Normal,
    /// `GMT_SKIN` (1): skinned, 44-byte vertices.
    Skin,
    /// Any other value (`GMT_BONE` 2 never occurs as an object type); treated like `Normal`.
    Other(u16),
}

/// One GMOBJECT (Plan/30 §6.2).
#[derive(Debug, Clone, PartialEq)]
pub struct GmObject {
    /// The full type word: kind in the low 16 bits, bit 31 = light object.
    pub type_raw: u32,
    /// Object-level bone palette (`nUseBone` entries, palette slot to skeleton bone), 0..=28.
    pub use_bone: Vec<i32>,
    /// 3ds Max node id, unused.
    pub id: i32,
    /// Parent object/bone index, `-1` for a root.
    pub parent_idx: i32,
    /// `Some` iff `parent_idx != -1`: `0` = another GMOBJECT of the same group, `2` = a skeleton bone.
    pub parent_type: Option<i32>,
    /// Local transform relative to the parent (rigid vertices are stored in this local space).
    pub local_tm: D3dMatrix,
    /// The geometry.
    pub body: GmBody,
    /// Node animation: `Some` iff the kind is [`GmType::Normal`] and the file's `max_frame > 0`.
    pub node_anim: Option<NodeAnim>,
}

impl GmObject {
    /// The kind (low 16 bits of the type word).
    pub fn kind(&self) -> GmType {
        match (self.type_raw & 0xFFFF) as u16 {
            0 => GmType::Normal,
            1 => GmType::Skin,
            other => GmType::Other(other),
        }
    }

    /// `true` for a light object (bit 31).
    pub fn is_light(&self) -> bool {
        self.type_raw & consts::LIGHT_OBJECT_BIT != 0
    }
}

/// Per-node TM animation (28 bytes per key).
#[derive(Debug, Clone, PartialEq)]
pub struct NodeAnim {
    /// The stored `hasFrames` word.
    pub has_frames: i32,
    /// `max_frame` keys when `has_frames != 0`, else empty.
    pub keys: Vec<TmKey>,
}

/// Counts of a body as stored (`nVertexList`, `nVB`, `nFace`, `nIB`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BodyCounts {
    /// Unique positions.
    pub n_vertex_list: i32,
    /// Render vertices.
    pub n_vb: i32,
    /// Triangles.
    pub n_face: i32,
    /// Indices (`3 * n_face` in every corpus file).
    pub n_ib: i32,
}

/// A geometry body (`GMBODY`, Plan/30 §6.3).
#[derive(Debug, Clone, PartialEq)]
pub struct GmBody {
    /// Object-local bounding box minimum.
    pub bb_min: Vec3,
    /// Object-local bounding box maximum.
    pub bb_max: Vec3,
    /// `bOpacity` (unused at runtime).
    pub opacity: i32,
    /// `bBump` (unused).
    pub bump: i32,
    /// `bRigid` (unused).
    pub rigid: i32,
    /// The 28 reserved bytes.
    pub reserved: [u8; 28],
    /// Counts as stored.
    pub counts: BodyCounts,
    /// Unique vertex positions (empty when skipped).
    pub vertex_list: Vec<Vec3>,
    /// Render vertices.
    pub vertices: Vertices,
    /// Triangle list indices (empty when skipped).
    pub indices: Vec<u16>,
    /// Per render vertex: index into `vertex_list` (empty when skipped).
    pub iib: Vec<u16>,
    /// The raw `hasPhysique` word.
    pub has_physique: i32,
    /// Primary bone per unique vertex (global skeleton index); empty when absent or skipped.
    pub physique: Vec<i32>,
    /// Materials; `None` when `bIsMaterial == 0`.
    pub materials: Option<MaterialList>,
    /// Material blocks (draw calls), at most 31.
    pub blocks: Vec<MaterialBlock>,
}

/// The vertex buffer of a body.
#[derive(Debug, Clone, PartialEq)]
pub enum Vertices {
    /// 44-byte skinned vertices.
    Skin(Vec<SkinVertex>),
    /// 32-byte rigid vertices (also the collision body).
    Normal(Vec<NormalVertex>),
    /// [`GeometryMode::Skip`].
    Skipped,
}

impl Vertices {
    /// Number of vertices held (0 when skipped).
    pub fn len(&self) -> usize {
        match self {
            Vertices::Skin(v) => v.len(),
            Vertices::Normal(v) => v.len(),
            Vertices::Skipped => 0,
        }
    }

    /// `true` when no vertex is held.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// `SKINVERTEX` (44 bytes, Plan/30 §6.4).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkinVertex {
    /// Bind-pose model-space position.
    pub pos: Vec3,
    /// Weight of bone A.
    pub w1: f32,
    /// Weight of bone B (`w1 + w2 = 1` for all checked vertices).
    pub w2: f32,
    /// Palette slot A times 3 (shader constant register index).
    pub idx_a: u16,
    /// Palette slot B times 3 (ignored when `w2 == 0`).
    pub idx_b: u16,
    /// Normal.
    pub normal: Vec3,
    /// `u, v` (D3D convention, `v` down).
    pub uv: [f32; 2],
}

/// `NORMALVERTEX` (32 bytes).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NormalVertex {
    /// Object-local position.
    pub pos: Vec3,
    /// Normal.
    pub normal: Vec3,
    /// `u, v`.
    pub uv: [f32; 2],
}

/// The material list of a body.
#[derive(Debug, Clone, PartialEq)]
pub struct MaterialList {
    /// The stored `nMaterial` (a stored 0 is read as 1).
    pub stored_count: i32,
    /// `max(stored_count, 1)` materials.
    pub materials: Vec<Material>,
}

/// One material: ignored colours plus the texture file name.
#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    /// `D3DMATERIAL9` as 17 floats (Diffuse, Ambient, Specular, Emissive RGBA, Power); the renderer
    /// ignores them (ref/18 §8.1).
    pub d3d: [f32; 17],
    /// Texture file name as stored (the consumer lower-cases it; may be empty).
    pub texture: RawStr,
}

/// `MATERIAL_BLOCK` (136 bytes): one draw call (Plan/30 §6.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaterialBlock {
    /// First index in the body's index buffer (named `m_nStartVertex` in the original).
    pub start_index: i32,
    /// Triangles.
    pub prim_count: i32,
    /// Index into the body's material list.
    pub texture_id: i32,
    /// `XE_*` bits ([`consts`]).
    pub effect: u32,
    /// Opacity 0-255, meaningful only with `XE_OPACITY`.
    pub amount: i32,
    /// Block palette size, 0..=28.
    pub n_use_bone: i32,
    /// The full 28-slot palette array as stored.
    pub use_bone: [i32; 28],
}

/// The optional trailing attribute block (Plan/30 §6.5).
#[derive(Debug, Clone, PartialEq)]
pub enum AttrBlock {
    /// v20, or v >= 21 with fewer than 4 bytes left (the original read past EOF and got 0).
    Absent,
    /// `nAttr` was present but the array was not read (`nAttr != maxFrame`, or `maxFrame == 0`).
    CountOnly(i32),
    /// `maxFrame` dense attribute entries (index = `(int)frame`).
    Attrs(Vec<MotionAttr>),
}

/// Case-insensitive comparison of the header name with the requested file name, directories ignored
/// (the original compares with `strcmpi` against the base name, `Object3D.cpp:939-955`).
pub fn name_matches(file: &O3dFile, requested_file_name: &str) -> bool {
    header_matches(&file.header_name, requested_file_name)
}

pub(crate) fn header_matches(header: &RawStr, requested_file_name: &str) -> bool {
    let base = requested_file_name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(requested_file_name);
    header.eq_ignore_ascii_case(base)
}
