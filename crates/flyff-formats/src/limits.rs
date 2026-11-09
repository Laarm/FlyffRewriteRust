//! The caps table applied by the parsers (Plan/30 §2.2): every count or length read from untrusted bytes
//! is validated against one of these before anything is allocated.
//!
//! Each constant is the largest accepted value (inclusive) unless its name says otherwise.

/// `.o3d` name length: the original requires `< 64` (src:_Common/Object3D.cpp `LoadObject`).
pub const O3D_NAME_MAX_LEN: usize = 63;
/// `.o3d` / `.chr` events: `MAX_MDL_EVENT 8` (Bone.h:22).
pub const MAX_MDL_EVENTS: u32 = 8;
/// `.ani` events: `m_vEvent[4]` (Bone.h:147).
pub const MAX_ANI_EVENTS: u32 = 4;
/// Object/block palette `nUseBone`: `MAX_VS_BONE 28` (Bone.h:7).
pub const MAX_VS_BONE: u32 = 28;
/// Render vertex count `nVB` (u16 indices).
pub const MAX_RENDER_VERTICES: u32 = 65_536;
/// Materials per body (stored 0 reads as 1): `m_MaterialAry[16]` (Object3D.h:140).
pub const MAX_MATERIALS_PER_BODY: u32 = 16;
/// Material texture NSTR length: `szBitmap[256]`.
pub const MAX_TEXTURE_NAME_LEN: usize = 256;
/// Texture names longer than this produce `TextureNameLong`: `strBitMapFileName[64]`.
pub const TEXTURE_NAME_WARN_LEN: usize = 64;
/// Material blocks `nMtrlBlk`: the original requires `< 32`.
pub const MAX_MATERIAL_BLOCKS: u32 = 31;
/// Bone name NSTR (`.chr`, MOTIONCORE): `m_szName[32]`.
pub const MAX_BONE_NAME_LEN: usize = 32;
/// Bones per skeleton: at least 1 (corpus max 75).
pub const MAX_BONES: u32 = 255;
/// `.ani` frames: the original requires `> 0`.
pub const MAX_ANI_FRAMES: u32 = 65_535;
/// `.sfx` parts (corpus max 41).
pub const MAX_SFX_PARTS: u32 = 1024;
/// `.sfx` keys per part (corpus max 57).
pub const MAX_SFX_KEYS: u32 = 4096;
/// `.sfx` string length: 255-byte read buffer.
pub const MAX_SFX_STRING_LEN: usize = 254;
/// DDS width/height.
pub const MAX_DDS_DIM: u32 = 8192;
/// Any input buffer handed to a wasm decoder (worker budget 64 MB, 01 §12.2).
pub const MAX_DECODER_INPUT: usize = 32 * 1024 * 1024;
/// Objects (or static sfx) per `.lnd`: a sanity cap; the remaining-bytes check bounds the real value
/// (corpus max is a few thousand per land).
pub const MAX_LAND_OBJECTS: u32 = 1_000_000;

/// Largest legal DDS mip-level count for `width x height`: `floor(log2(max(w, h))) + 1` (0 for a zero side).
pub fn max_mip_levels(width: u32, height: u32) -> u32 {
    let m = width.max(height);
    if m == 0 { 0 } else { m.ilog2() + 1 }
}
