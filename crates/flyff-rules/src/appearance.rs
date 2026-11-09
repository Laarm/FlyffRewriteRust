//! Character appearance at creation (Plan/12 §9.1 step 4, §9.2, WP-AUTH-01): the accepted ranges of sex,
//! hair, head and skin set, the mover index per sex and the default hair colour table.
//!
//! The legacy client enforced the ranges (`Resource/defineNeuz.h:1124-1137`) and sent the hair colour itself;
//! the server trusted both. Here the lobby validates the ranges and the server computes the colour
//! (**FIX** PL-033). Everything is pure and wasm-safe: the create-character screen of the browser previews
//! with the same table.

use flyff_common::defs::neuz::{MAX_BASE_HAIR, MAX_DEFAULT_HEAD, SKINSET_01};
use flyff_common::defs::obj::{MI_FEMALE, MI_MALE};

/// Which appearance field is out of range (12 §9.1 step 4: `LobbyError{104, TID_DIAG_0039}`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AppearanceError {
    /// `sex` is not 0 (male) or 1 (female).
    Sex,
    /// `hair_mesh` is not in `0..MAX_BASE_HAIR` (0..=9).
    HairMesh,
    /// `head_mesh` is not in `0..MAX_DEFAULT_HEAD` (0..=10).
    HeadMesh,
    /// `skin_set` is not `SKINSET_01` (0).
    SkinSet,
}

impl std::fmt::Display for AppearanceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            AppearanceError::Sex => "sex must be 0 (male) or 1 (female)",
            AppearanceError::HairMesh => "hair mesh is out of range",
            AppearanceError::HeadMesh => "head mesh is out of range",
            AppearanceError::SkinSet => "skin set is out of range",
        })
    }
}

impl std::error::Error for AppearanceError {}

/// Checks the appearance fields of a `CreatePlayer`: `sex` 0 or 1, `hair_mesh <= 9` (`MAX_BASE_HAIR` 10),
/// `head_mesh <= 10` (`MAX_DEFAULT_HEAD` 11) and `skin_set == 0` (`SKINSET_01`). The first violated field is
/// reported in the order sex, hair, head, skin. Takes any unsigned integer type so that the proto `u32`
/// values are checked before they are narrowed.
pub fn validate_appearance(
    sex: impl Into<u32>,
    hair_mesh: impl Into<u32>,
    head_mesh: impl Into<u32>,
    skin_set: impl Into<u32>,
) -> Result<(), AppearanceError> {
    if sex.into() > 1 {
        return Err(AppearanceError::Sex);
    }
    if hair_mesh.into() >= MAX_BASE_HAIR {
        return Err(AppearanceError::HairMesh);
    }
    if head_mesh.into() >= MAX_DEFAULT_HEAD {
        return Err(AppearanceError::HeadMesh);
    }
    if skin_set.into() != SKINSET_01 {
        return Err(AppearanceError::SkinSet);
    }
    Ok(())
}

/// `MI_MALE` (11) for sex 0, `MI_FEMALE` (12) for sex 1 (`Resource/defineObj.h:962-963`); `None` otherwise.
pub fn mover_index(sex: impl Into<u32>) -> Option<u32> {
    match sex.into() {
        0 => Some(MI_MALE),
        1 => Some(MI_FEMALE),
        _ => None,
    }
}

/// `(r, g, b)` per hair mesh, male and female (`src:_Common/MoverEquip.cpp:43-69`); meshes 5-9 are grey.
const HAIR_RGB: [[[u8; 3]; 10]; 2] = [
    [
        [144, 99, 101],
        [100, 107, 125],
        [158, 119, 109],
        [199, 156, 100],
        [100, 123, 117],
        [100, 100, 100],
        [100, 100, 100],
        [100, 100, 100],
        [100, 100, 100],
        [100, 100, 100],
    ],
    [
        [250, 250, 90],
        [250, 250, 90],
        [147, 118, 103],
        [150, 100, 85],
        [213, 130, 100],
        [100, 100, 100],
        [100, 100, 100],
        [100, 100, 100],
        [100, 100, 100],
        [100, 100, 100],
    ],
];

/// The default hair colour of a new character, `0xFF000000 | r << 16 | g << 8 | b` (the client's
/// `D3DCOLOR_ARGB(255, r, g, b)`, `src:_Interface/WndTitle.cpp:2991-3003`). `None` for a sex above 1 or a
/// hair mesh above 9.
pub fn default_hair_color(sex: impl Into<u32>, hair_mesh: impl Into<u32>) -> Option<u32> {
    let sex = usize::try_from(sex.into()).ok()?;
    let mesh = usize::try_from(hair_mesh.into()).ok()?;
    let [r, g, b] = *HAIR_RGB.get(sex)?.get(mesh)?;
    Some(0xFF00_0000 | u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b))
}

/// `D3DCOLOR_COLORVALUE(r, g, b, 1.0f)`: every channel is `(DWORD)(c * 255.f)`, truncated.
fn color_value(r: f32, g: f32, b: f32) -> u32 {
    let ch = |c: f32| (c * 255.0_f32) as u32 & 0xFF;
    0xFF00_0000 | ch(r) << 16 | ch(g) << 8 | ch(b)
}

/// The colour stored by `CMover::SetHairColor(FLOAT r, g, b)` of the server build (`__Y_HAIR_BUG_FIX` is not
/// defined, src:_Common/MoverEquip.cpp:156-190), called by the beauty shop with `byte / 255.0f` channels
/// (src:WORLDSERVER/DPSrvr.cpp:4952-5008, Plan/19 §7.8): a colour within 0.001 of white is replaced by the
/// default colour of the hair mesh (`nMaleHairColor` / `nFeMaleHairColor`, the table of
/// [`default_hair_color`]), every other colour is raised to at least 0.3 per channel (the client's picker
/// maps its sliders to 0.3..1.0). The result is `D3DCOLOR_COLORVALUE`: the channels truncated from single
/// precision, so it can be one below the chosen byte, which is why the cost comparison of the next change
/// goes through [`crate::item::price::hair_color_roundtrip`]. A `sex` or `hair_mesh` outside the table keeps
/// the requested channels unreplaced.
pub fn set_hair_color(sex: u8, hair_mesh: u32, rgb: [u8; 3]) -> u32 {
    let to_f = |b: u8| f32::from(b) / 255.0_f32;
    let [r, g, b] = rgb.map(to_f);
    let near_white = (r == 1.0 && g == 1.0 && b == 1.0)
        || ((r - 1.0).abs() < 0.001 && (g - 1.0).abs() < 0.001 && (b - 1.0).abs() < 0.001);
    let table = usize::try_from(hair_mesh)
        .ok()
        .and_then(|m| HAIR_RGB.get(usize::from(sex))?.get(m));
    if near_white && let Some(&[tr, tg, tb]) = table {
        return color_value(to_f(tr), to_f(tg), to_f(tb));
    }
    let raise = |c: f32| if c < 0.3_f32 { 0.3_f32 } else { c };
    color_value(raise(r), raise(g), raise(b))
}
