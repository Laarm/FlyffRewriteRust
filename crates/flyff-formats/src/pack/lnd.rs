//! The `.lnd` land pack (`pack::lnd::build`, Plan/30 §13.5): left-handed grid data for the browser, no mesh.
//!
//! Sections of the blob (every section starts at a multiple of 4 bytes):
//!
//! | `LndMeta` field | type | content |
//! |---|---|---|
//! | `heights` | `f32` x 16,641 | `strip_height_client` of the raw heights, `h[z * 129 + x]` |
//! | `attrs` | `u8` x 16,641 | [`HeightAttr`] number (0..=4) of the raw height |
//! | `water` | `u8` x 2 x 256 | `{height, tex}` per patch, `px + pz * 16` (raw) |
//! | `layers[i].patch_mask` | in the meta | bit `i % 32` of word `i / 32` = patch `i` (`px + pz * 16`) enabled |
//! | `alpha` | `u8` x `layers * 16,384` | the A channel of each layer's light map, layer-major, `z * 128 + x` |
//! | `light_rgb` | `u8` x 3 x 16,384 | R, G, B of layer 0's light map (identical in all layers of the corpus) |
//! | `normals` | `f32` x 3 x 16,641 | optional: central differences of the real heights (one-sided at the edges) |
//! | `objects` / `sfx` | `ids` `u32` x 4, `xf` `f32` x 10 | per record: `(type, index, motion, ai_interface)` and `(abs pos x y z, angle, angX, angZ, scale x y z, 0)` |
//!
//! Positions of placements are [`ObjBase::absolute_position`] with the land placement of the options. Nothing
//! here is mapped to three.js space (there is no mesh): the renderer applies `render/space.ts`.

use super::blob::{BlobBuilder, BlobView, ElemType, Packed};
use crate::FormatKind;
use crate::error::{ErrorKind, FormatError};
use crate::lnd::{
    HEIGHT_COUNT, HEIGHT_DIM, HeightAttr, LIGHTMAP_TEXELS, LandView, strip_height_client,
};
use crate::objects::{ObjBase, Placement, RecordIter};

/// Options of [`build`] (Plan/30 §13.5): where the land sits (the indices of the FILE NAME, which the
/// original uses; the header coordinates are only compared) and what to compute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", serde(default))]
pub struct LndPackOptions {
    /// Land column (the first number of the file name).
    pub land_x: i32,
    /// Land row (the second number of the file name).
    pub land_z: i32,
    /// World units per heightmap cell (`.wld` key `MPU`, default 4).
    pub mpu: u32,
    /// Also emit per-vertex normals (default `false`).
    pub normals: bool,
}

impl Default for LndPackOptions {
    fn default() -> LndPackOptions {
        LndPackOptions {
            land_x: 0,
            land_z: 0,
            mpu: 4,
            normals: false,
        }
    }
}

/// One terrain layer: the texture and the patches it covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LayerMeta {
    /// Terrain texture id (`Terrain.inc`).
    pub texture_id: u16,
    /// Bit `i % 32` of word `i / 32`: patch `i` (`px + pz * 16`) is enabled for this layer.
    pub patch_mask: [u32; 8],
}

/// Object or static-sfx placements of a land (Plan/30 §13.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Placements {
    /// Number of records.
    pub count: u32,
    /// `u32` x 4 per record: object type, index, motion, ai interface.
    pub ids: BlobView,
    /// `f32` x 10 per record: absolute position x y z, angle, angX, angZ, scale x y z, 0.
    pub xf: BlobView,
}

/// The structured description of a land pack (Plan/30 §13.5).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LndMeta {
    /// File version.
    pub version: u32,
    /// Land column of the placement (from the options).
    pub land_x: i32,
    /// Land row of the placement (from the options).
    pub land_z: i32,
    /// `true` when the header coordinates equal the placement (or the file has no header): 36 files of the
    /// unloaded worlds `WdEvent01` and `WdLux` differ.
    pub header_matches: bool,
    /// World units per heightmap cell (from the options).
    pub mpu: u32,
    /// `f32` x 16,641 real heights (`strip_height_client`).
    pub heights: BlobView,
    /// `u8` x 16,641 [`HeightAttr`] numbers.
    pub attrs: BlobView,
    /// `u8` x 2 x 256 water/cloud patches `{height, tex}`.
    pub water: BlobView,
    /// The layers in file order.
    pub layers: Vec<LayerMeta>,
    /// `u8` x `layers.len() * 16,384`: the splat alpha of every layer, layer-major.
    pub alpha: BlobView,
    /// `u8` x 3 x 16,384: RGB of layer 0's light map (empty without layers).
    pub light_rgb: BlobView,
    /// `true` when every layer's light-map RGB equals layer 0's (always, in the corpus).
    pub light_rgb_identical: bool,
    /// `f32` x 3 x 16,641 normals when requested.
    pub normals: Option<BlobView>,
    /// Static objects (`OT_OBJ`).
    pub objects: Placements,
    /// Static effects (`OT_SFX`); `count == 0` when the file has no sfx section.
    pub sfx: Placements,
    /// Pack-time notes, one line each (`"TrailingBytes: ..."`).
    pub warnings: Vec<String>,
}

fn err(what: &'static str) -> FormatError {
    FormatError::new(FormatKind::Lnd, 0, ErrorKind::Inconsistent { what })
}

/// Builds the land pack of `view` (Plan/30 §13.5); see the module docs for the sections.
///
/// Errors: `Inconsistent` when a section does not fit the blob (cannot happen for a validated view) and the
/// record errors of the object lists.
pub fn build(view: &LandView<'_>, opts: &LndPackOptions) -> Result<Packed<LndMeta>, FormatError> {
    let mut bb = BlobBuilder::for_format(FormatKind::Lnd);

    // heights, attributes
    let raw: Vec<f32> = view.heights().collect();
    if raw.len() != HEIGHT_COUNT {
        return Err(err("height section length"));
    }
    let real: Vec<f32> = raw.iter().map(|h| strip_height_client(*h)).collect();
    let attr_bytes: Vec<u8> = raw.iter().map(|h| HeightAttr::classify(*h) as u8).collect();
    let heights = bb.push_f32(1, &real)?;
    let attrs = bb.push_typed(ElemType::U8, 1, &attr_bytes)?;
    let water = bb.push_typed(ElemType::U8, 2, view.water_bytes())?;

    // layers: texture ids and patch masks (meta), alpha and light (blob)
    let layer_count = view.layer_count();
    let mut layers = Vec::with_capacity(layer_count);
    let mut alpha = vec![0u8; layer_count * LIGHTMAP_TEXELS];
    for (layer, out) in alpha
        .as_chunks_mut::<LIGHTMAP_TEXELS>()
        .0
        .iter_mut()
        .enumerate()
    {
        let texture_id = view
            .layer_texture_id(layer)
            .ok_or_else(|| err("layer texture id"))?;
        let enable = view
            .layer_patch_enable_bytes(layer)
            .ok_or_else(|| err("layer patch flags"))?;
        let mut patch_mask = [0u32; 8];
        for (patch, flag) in enable.as_chunks::<4>().0.iter().enumerate() {
            if i32::from_le_bytes(*flag) != 0
                && let Some(word) = patch_mask.get_mut(patch / 32)
            {
                *word |= 1u32 << (patch % 32);
            }
        }
        layers.push(LayerMeta {
            texture_id,
            patch_mask,
        });
        view.layer_alpha(layer, out.as_mut_slice())?;
    }
    let alpha_view = bb.push_typed(ElemType::U8, 1, &alpha)?;

    let mut light = vec![
        0u8;
        if layer_count > 0 {
            LIGHTMAP_TEXELS * 3
        } else {
            0
        }
    ];
    if layer_count > 0 {
        view.light_rgb(0, &mut light)?;
    }
    let light_rgb = bb.push_typed(ElemType::U8, 3, &light)?;
    let light_rgb_identical = (1..layer_count).all(|layer| same_light_rgb(view, 0, layer));

    let normals = if opts.normals {
        Some(bb.push_f32(3, &normals_of(&real, opts.mpu))?)
    } else {
        None
    };

    // placements
    let placement = Placement {
        land_x: opts.land_x,
        land_z: opts.land_z,
        mpu: opts.mpu,
    };
    let objects = placements(&mut bb, view.objects(), &placement)?;
    let sfx = placements(&mut bb, view.sfx(), &placement)?;

    let mut warnings = Vec::new();
    if view.trailing_bytes() > 0 {
        warnings.push(format!(
            "TrailingBytes: {} bytes after the last record",
            view.trailing_bytes()
        ));
    }
    let meta = LndMeta {
        version: view.version(),
        land_x: opts.land_x,
        land_z: opts.land_z,
        header_matches: view
            .land_xz()
            .is_none_or(|xz| xz == (opts.land_x, opts.land_z)),
        mpu: opts.mpu,
        heights,
        attrs,
        water,
        layers,
        alpha: alpha_view,
        light_rgb,
        light_rgb_identical,
        normals,
        objects,
        sfx,
        warnings,
    };
    Ok(Packed {
        meta,
        blob: bb.finish(),
    })
}

/// `true` when the R, G, B bytes of two layers' light maps are equal texel for texel.
fn same_light_rgb(view: &LandView<'_>, a: usize, b: usize) -> bool {
    match (view.layer_light_bytes(a), view.layer_light_bytes(b)) {
        (Some(la), Some(lb)) => la
            .as_chunks::<4>()
            .0
            .iter()
            .zip(lb.as_chunks::<4>().0)
            .all(|(x, y)| x.get(..3) == y.get(..3)),
        _ => false,
    }
}

/// Per-vertex normals of the real height field: slopes by central differences inside the land and
/// one-sided differences at the four edges, `n = normalize(-dh/dx, 1, -dh/dz)` (left-handed, +Y up).
fn normals_of(h: &[f32], mpu: u32) -> Vec<f32> {
    let cell = mpu.max(1) as f32;
    let at = |x: usize, z: usize| h.get(z * HEIGHT_DIM + x).copied().unwrap_or(0.0);
    let slope = |lo: f32, hi: f32, span: f32| (hi - lo) / (span * cell);
    let mut out = Vec::with_capacity(HEIGHT_COUNT * 3);
    for z in 0..HEIGHT_DIM {
        for x in 0..HEIGHT_DIM {
            let dx = match x {
                0 => slope(at(0, z), at(1, z), 1.0),
                _ if x + 1 == HEIGHT_DIM => slope(at(x - 1, z), at(x, z), 1.0),
                _ => slope(at(x - 1, z), at(x + 1, z), 2.0),
            };
            let dz = match z {
                0 => slope(at(x, 0), at(x, 1), 1.0),
                _ if z + 1 == HEIGHT_DIM => slope(at(x, z - 1), at(x, z), 1.0),
                _ => slope(at(x, z - 1), at(x, z + 1), 2.0),
            };
            let len = (dx * dx + 1.0 + dz * dz).sqrt();
            out.extend_from_slice(&[-dx / len, 1.0 / len, -dz / len]);
        }
    }
    out
}

/// Collects the records of one list into the `ids` / `xf` sections.
fn placements(
    bb: &mut BlobBuilder,
    records: RecordIter<'_>,
    placement: &Placement,
) -> Result<Placements, FormatError> {
    let mut ids: Vec<u32> = Vec::with_capacity(records.remaining() * 4);
    let mut xf: Vec<f32> = Vec::with_capacity(records.remaining() * 10);
    let mut count = 0u32;
    for rec in records {
        let rec = rec?;
        let base: &ObjBase = &rec.base;
        let [x, y, z] = base.absolute_position(Some(placement));
        ids.extend_from_slice(&[rec.obj_type, base.index, base.motion, base.ai_interface]);
        xf.extend_from_slice(&[
            x,
            y,
            z,
            base.angle,
            base.axis[0],
            base.axis[2],
            base.scale[0],
            base.scale[1],
            base.scale[2],
            0.0,
        ]);
        count += 1;
    }
    Ok(Placements {
        count,
        ids: bb.push_u32(4, &ids)?,
        xf: bb.push_f32(10, &xf)?,
    })
}
