//! The `.o3d` parser (Plan/30 §6; see the module docs of [`crate::o3d`] for the layout).

use super::{
    AttrBlock, BodyCounts, EmbeddedBones, GeometryMode, GmBody, GmObject, LodGroup, Material,
    MaterialBlock, MaterialList, NodeAnim, NormalVertex, O3dFile, O3dOptions, SkinVertex, Vertices,
    consts, header_matches,
};
use crate::FormatKind;
use crate::ani::KeyMode;
use crate::error::{ErrorKind, FormatError, FormatWarning, Parsed, WarningCode};
use crate::limits::{
    MAX_ANI_FRAMES, MAX_BONES, MAX_MATERIAL_BLOCKS, MAX_MATERIALS_PER_BODY, MAX_MDL_EVENTS,
    MAX_RENDER_VERTICES, MAX_TEXTURE_NAME_LEN, MAX_VS_BONE, O3D_NAME_MAX_LEN,
    TEXTURE_NAME_WARN_LEN,
};
use crate::motion::{checked_bytes, f32s, read_attrs, read_core, read_keys, read_vec3s};
use crate::raw::{D3dMatrix, RawStr};
use crate::reader::ByteReader;

/// Largest object count accepted per LOD group (the corpus maximum is far below; the remaining-bytes
/// check bounds the real value).
const MAX_GROUP_OBJECTS: u32 = 65_535;
/// Smallest possible GMOBJECT: type, nUseBone, id, parentIdx (4 each) + localTM (64) + an empty body
/// (bounds 24, three flags 12, reserved 28, four counts 16, hasPhysique, bIsMaterial, nMtrlBlk 12).
const MIN_OBJECT_BYTES: usize = 4 * 4 + 64 + 24 + 12 + 28 + 16 + 12;
/// `SKINVERTEX` size.
const SKIN_VERTEX_SIZE: usize = 44;
/// `NORMALVERTEX` size.
const NORMAL_VERTEX_SIZE: usize = 32;
/// `MATERIAL_BLOCK` size.
const BLOCK_SIZE: usize = 136;
/// `D3DMATERIAL9` size.
const D3D_MATERIAL_SIZE: usize = 68;

/// Parses an `.o3d`.
///
/// Errors: a header name longer than 63 bytes, a version below 20 (`LegacyLayout`: the two pre-v20 heads
/// `Part_maleHead.o3d` / `Part_femaleHead.o3d` read as such), any count above its cap (events 8, palettes
/// 28, material blocks 31, materials 16, render vertices 65,536, frames 65,535), a texture name above 256
/// bytes, and truncated data. Warnings: `O3dNameMismatch` (only when [`O3dOptions::file_name`] is set),
/// `O3dPoolSizeMismatch`, `O3dIndexCountMismatch`, `O3dAttrCountMismatch`, `MotionPoolMismatch`,
/// `TextureNameLong`, `TrailingBytes`.
pub fn parse(b: &[u8], opts: &O3dOptions) -> Result<Parsed<O3dFile>, FormatError> {
    let mut r = ByteReader::new(b, FormatKind::O3d);
    let mut w: Vec<FormatWarning> = Vec::new();

    // Header name, XOR 0xCD (src:_Common/Object3D.cpp:933-955).
    let name_len = usize::from(r.u8()?);
    if name_len > O3D_NAME_MAX_LEN {
        return Err(r.err_at(
            0,
            ErrorKind::StringTooLong {
                field: "name",
                len: name_len as i64,
                max: O3D_NAME_MAX_LEN as i64,
            },
        ));
    }
    let mut name = r.bytes(name_len)?.to_vec();
    for c in &mut name {
        *c ^= 0xCD;
    }
    let header_name = RawStr(name.into_boxed_slice());
    if let Some(requested) = opts
        .file_name
        .as_deref()
        .filter(|req| !header_matches(&header_name, req))
    {
        w.push(FormatWarning::new(
            1,
            WarningCode::O3dNameMismatch,
            format!(
                "header name {:?} does not match the file name {requested:?}; the original refuses to load this model",
                header_name.to_string_lossy_ascii()
            ),
        ));
    }

    let version = r.i32()?;
    if version < consts::VER_MESH {
        return Err(r.err_at(0, ErrorKind::LegacyLayout));
    }
    let id = r.i32()?;
    let force1 = r.vec3()?;
    let force2 = r.vec3()?;
    let (force3, force4) = if version >= consts::VER_FORCE34 {
        (r.vec3()?, r.vec3()?)
    } else {
        ([0.0; 3], [0.0; 3])
    };
    let scroll_uv = [r.f32()?, r.f32()?];
    let mut reserved = [0u8; 16];
    reserved.copy_from_slice(r.bytes(16)?);
    let bb_min = r.vec3()?;
    let bb_max = r.vec3()?;
    let per_slerp = r.f32()?;
    let frame_at = r.pos();
    let max_frame = r.i32()?;
    if !(0..=MAX_ANI_FRAMES as i32).contains(&max_frame) {
        return Err(r.err_at(
            frame_at,
            ErrorKind::CountOutOfRange {
                field: "maxFrame",
                value: i64::from(max_frame),
                max: i64::from(MAX_ANI_FRAMES),
            },
        ));
    }
    let frames = usize::try_from(max_frame).unwrap_or(0);
    let n_events = r.count("nEvent", 12, MAX_MDL_EVENTS)?;
    let events = read_vec3s(&mut r, n_events)?;

    let has_coll = r.i32()?;
    let collision = if has_coll != 0 {
        Some(read_body(&mut r, false, GeometryMode::Full, &mut w)?)
    } else {
        None
    };
    let lod = r.i32()?;

    let n_bone = r.count("nBone", 2 * 64, MAX_BONES)?;
    let bones = if n_bone > 0 {
        let base = read_mats(&mut r, n_bone)?;
        let base_inv = read_mats(&mut r, n_bone)?;
        let motion = if frames > 0 {
            Some(read_core(&mut r, n_bone, frames, KeyMode::Load, &mut w)?)
        } else {
            None
        };
        let send_vs = r.i32()?;
        Some(EmbeddedBones {
            base,
            base_inv,
            motion,
            send_vs,
        })
    } else {
        None
    };

    let pool_at = r.pos();
    let pool_size = r.i32()?;
    let n_groups = if lod != 0 { 3 } else { 1 };
    let mut groups = Vec::with_capacity(n_groups);
    let mut total_objects = 0usize;
    for _ in 0..n_groups {
        let n_obj = r.count("nObject", MIN_OBJECT_BYTES, MAX_GROUP_OBJECTS)?;
        let mut objects = Vec::with_capacity(n_obj);
        for _ in 0..n_obj {
            objects.push(read_object(&mut r, frames, opts.geometry, &mut w)?);
        }
        total_objects += n_obj;
        groups.push(LodGroup { objects });
    }
    if i64::from(pool_size) != total_objects as i64 {
        w.push(FormatWarning::new(
            pool_at,
            WarningCode::O3dPoolSizeMismatch,
            format!("poolSize {pool_size} != {total_objects} objects in the groups"),
        ));
    }

    // Optional frame attributes (version >= 21; the original read past EOF and got 0).
    let attrs = if version >= consts::VER_ATTRS && r.remaining() >= 4 {
        let at = r.pos();
        let n_attr = r.i32()?;
        if n_attr == max_frame {
            if frames > 0 {
                AttrBlock::Attrs(read_attrs(&mut r, frames)?)
            } else {
                AttrBlock::CountOnly(n_attr)
            }
        } else {
            w.push(FormatWarning::new(
                at,
                WarningCode::O3dAttrCountMismatch,
                format!("nAttr {n_attr} != maxFrame {max_frame}; attributes not read"),
            ));
            AttrBlock::CountOnly(n_attr)
        }
    } else {
        AttrBlock::Absent
    };

    if !r.at_end() {
        w.push(FormatWarning::new(
            r.pos(),
            WarningCode::TrailingBytes,
            format!("{} byte(s) after the last field", r.remaining()),
        ));
    }
    Ok(Parsed {
        value: O3dFile {
            header_name,
            version,
            id,
            force: [force1, force2, force3, force4],
            scroll_uv,
            reserved,
            bb_min,
            bb_max,
            per_slerp,
            max_frame,
            events,
            collision,
            lod,
            bones,
            pool_size,
            groups,
            attrs,
        },
        warnings: w,
    })
}

/// `n` matrices; the size is checked before the allocation.
fn read_mats(r: &mut ByteReader<'_>, n: usize) -> Result<Vec<D3dMatrix>, FormatError> {
    let total = checked_bytes(r, n, 64)?;
    let bytes = r.bytes(total)?;
    let (chunks, _) = bytes.as_chunks::<64>();
    Ok(chunks.iter().map(|c| D3dMatrix(f32s::<16>(c))).collect())
}

/// `n` little-endian `u16`; the size is checked before the allocation.
fn read_u16s(r: &mut ByteReader<'_>, n: usize) -> Result<Vec<u16>, FormatError> {
    let total = checked_bytes(r, n, 2)?;
    let bytes = r.bytes(total)?;
    let (chunks, _) = bytes.as_chunks::<2>();
    Ok(chunks.iter().map(|c| u16::from_le_bytes(*c)).collect())
}

/// Skips `n` elements of `elem_size` bytes.
fn skip_array(r: &mut ByteReader<'_>, n: usize, elem_size: usize) -> Result<(), FormatError> {
    let total = checked_bytes(r, n, elem_size)?;
    r.skip(total)
}

/// One GMOBJECT (Plan/30 §6.2).
fn read_object(
    r: &mut ByteReader<'_>,
    frames: usize,
    mode: GeometryMode,
    w: &mut Vec<FormatWarning>,
) -> Result<GmObject, FormatError> {
    let type_raw = r.u32()?;
    let n_use = r.count("nUseBone", 4, MAX_VS_BONE)?;
    let use_bone = r.i32_vec(n_use)?;
    let id = r.i32()?;
    let parent_idx = r.i32()?;
    let parent_type = if parent_idx != -1 {
        Some(r.i32()?)
    } else {
        None
    };
    let local_tm = r.mat()?;
    let kind = type_raw & 0xFFFF;
    let body = read_body(r, kind == 1, mode, w)?;
    // Node animation only exists for rigid objects of animated files (`LoadTMAni`).
    let node_anim = if kind == 0 && frames > 0 {
        let has_frames = r.i32()?;
        let keys = if has_frames != 0 {
            read_keys(r, frames)?
        } else {
            Vec::new()
        };
        Some(NodeAnim { has_frames, keys })
    } else {
        None
    };
    Ok(GmObject {
        type_raw,
        use_bone,
        id,
        parent_idx,
        parent_type,
        local_tm,
        body,
        node_anim,
    })
}

/// One GMBODY (`LoadGMObject`, Plan/30 §6.3). `skin` selects the 44-byte vertex layout.
fn read_body(
    r: &mut ByteReader<'_>,
    skin: bool,
    mode: GeometryMode,
    w: &mut Vec<FormatWarning>,
) -> Result<GmBody, FormatError> {
    let bb_min = r.vec3()?;
    let bb_max = r.vec3()?;
    let opacity = r.i32()?;
    let bump = r.i32()?;
    let rigid = r.i32()?;
    let mut reserved = [0u8; 28];
    reserved.copy_from_slice(r.bytes(28)?);

    let vsize = if skin {
        SKIN_VERTEX_SIZE
    } else {
        NORMAL_VERTEX_SIZE
    };
    let counts_at = r.pos();
    let n_vl = r.count("nVertexList", 12, MAX_RENDER_VERTICES)?;
    let n_vb = r.count("nVB", vsize, MAX_RENDER_VERTICES)?;
    let n_face = r.count("nFace", 0, i32::MAX as u32)?;
    let n_ib = r.count("nIB", 2, i32::MAX as u32)?;
    let counts = BodyCounts {
        n_vertex_list: n_vl as i32,
        n_vb: n_vb as i32,
        n_face: n_face as i32,
        n_ib: n_ib as i32,
    };
    if n_ib != n_face.saturating_mul(3) {
        w.push(FormatWarning::new(
            counts_at,
            WarningCode::O3dIndexCountMismatch,
            format!("nIB {n_ib} != 3 * nFace {n_face}"),
        ));
    }

    let (vertex_list, vertices, indices, iib);
    if mode == GeometryMode::Full {
        vertex_list = read_vec3s(r, n_vl)?;
        vertices = if skin {
            Vertices::Skin(read_skin_vertices(r, n_vb)?)
        } else {
            Vertices::Normal(read_normal_vertices(r, n_vb)?)
        };
        indices = read_u16s(r, n_ib)?;
        iib = read_u16s(r, n_vb)?;
    } else {
        skip_array(r, n_vl, 12)?;
        skip_array(r, n_vb, vsize)?;
        skip_array(r, n_ib, 2)?;
        skip_array(r, n_vb, 2)?;
        vertex_list = Vec::new();
        vertices = Vertices::Skipped;
        indices = Vec::new();
        iib = Vec::new();
    }

    let has_physique = r.i32()?;
    let physique = if has_physique != 0 {
        if mode == GeometryMode::Full {
            r.i32_vec(n_vl)?
        } else {
            skip_array(r, n_vl, 4)?;
            Vec::new()
        }
    } else {
        Vec::new()
    };

    let is_material = r.i32()?;
    let materials = if is_material != 0 {
        let stored_count = r.i32()?;
        // A stored 0 is treated as 1 and one material is still read (src:_Common/Object3D.cpp:1349).
        let at = r.pos().saturating_sub(4);
        let n = match usize::try_from(stored_count) {
            Ok(n) if n <= MAX_MATERIALS_PER_BODY as usize => n.max(1),
            _ => {
                return Err(r.err_at(
                    at,
                    ErrorKind::CountOutOfRange {
                        field: "nMaterial",
                        value: i64::from(stored_count),
                        max: i64::from(MAX_MATERIALS_PER_BODY),
                    },
                ));
            }
        };
        let mut list = Vec::with_capacity(n);
        for _ in 0..n {
            let d3d = f32s::<17>(r.bytes(D3D_MATERIAL_SIZE)?);
            let name_at = r.pos();
            let texture = r.nstr("texture name", MAX_TEXTURE_NAME_LEN)?;
            if texture.len() + 1 > TEXTURE_NAME_WARN_LEN {
                w.push(FormatWarning::new(
                    name_at,
                    WarningCode::TextureNameLong,
                    format!(
                        "texture name of {} bytes exceeds the original's {TEXTURE_NAME_WARN_LEN}-byte buffer",
                        texture.len()
                    ),
                ));
            }
            list.push(Material { d3d, texture });
        }
        Some(MaterialList {
            stored_count,
            materials: list,
        })
    } else {
        None
    };

    let n_blk = r.count("nMtrlBlk", BLOCK_SIZE, MAX_MATERIAL_BLOCKS)?;
    let mut blocks = Vec::with_capacity(n_blk);
    for _ in 0..n_blk {
        blocks.push(read_block(r)?);
    }

    Ok(GmBody {
        bb_min,
        bb_max,
        opacity,
        bump,
        rigid,
        reserved,
        counts,
        vertex_list,
        vertices,
        indices,
        iib,
        has_physique,
        physique,
        materials,
        blocks,
    })
}

/// One 136-byte `MATERIAL_BLOCK`; the block palette size is validated against `MAX_VS_BONE`.
fn read_block(r: &mut ByteReader<'_>) -> Result<MaterialBlock, FormatError> {
    let at = r.pos();
    let bytes = r.bytes(BLOCK_SIZE)?;
    let (chunks, _) = bytes.as_chunks::<4>();
    let mut words = chunks.iter().map(|c| u32::from_le_bytes(*c));
    let mut next = || words.next().unwrap_or(0);
    let start_index = next() as i32;
    let prim_count = next() as i32;
    let texture_id = next() as i32;
    let effect = next();
    let amount = next() as i32;
    let n_use_bone = next() as i32;
    let mut use_bone = [0i32; 28];
    for slot in &mut use_bone {
        *slot = next() as i32;
    }
    if !(0..=MAX_VS_BONE as i32).contains(&n_use_bone) {
        return Err(r.err_at(
            at + 20,
            ErrorKind::CountOutOfRange {
                field: "block nUseBone",
                value: i64::from(n_use_bone),
                max: i64::from(MAX_VS_BONE),
            },
        ));
    }
    Ok(MaterialBlock {
        start_index,
        prim_count,
        texture_id,
        effect,
        amount,
        n_use_bone,
        use_bone,
    })
}

fn read_skin_vertices(r: &mut ByteReader<'_>, n: usize) -> Result<Vec<SkinVertex>, FormatError> {
    let total = checked_bytes(r, n, SKIN_VERTEX_SIZE)?;
    let bytes = r.bytes(total)?;
    let (chunks, _) = bytes.as_chunks::<SKIN_VERTEX_SIZE>();
    Ok(chunks
        .iter()
        .map(|c| {
            let f = f32s::<11>(c);
            let (words, _) = c.as_chunks::<4>();
            let packed = words.get(5).map_or(0, |w| u32::from_le_bytes(*w));
            SkinVertex {
                pos: [f[0], f[1], f[2]],
                w1: f[3],
                w2: f[4],
                idx_a: (packed & 0xFFFF) as u16,
                idx_b: (packed >> 16) as u16,
                normal: [f[6], f[7], f[8]],
                uv: [f[9], f[10]],
            }
        })
        .collect())
}

fn read_normal_vertices(
    r: &mut ByteReader<'_>,
    n: usize,
) -> Result<Vec<NormalVertex>, FormatError> {
    let total = checked_bytes(r, n, NORMAL_VERTEX_SIZE)?;
    let bytes = r.bytes(total)?;
    let (chunks, _) = bytes.as_chunks::<NORMAL_VERTEX_SIZE>();
    Ok(chunks
        .iter()
        .map(|c| {
            let f = f32s::<8>(c);
            NormalVertex {
                pos: [f[0], f[1], f[2]],
                normal: [f[3], f[4], f[5]],
                uv: [f[6], f[7]],
            }
        })
        .collect())
}
