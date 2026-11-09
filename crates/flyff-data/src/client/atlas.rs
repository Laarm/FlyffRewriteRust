//! Texture atlases: `Theme/*.inc` (27), `Icon/*.inc` (9), `SFX/*.inc` (3) (`CTexturePack::LoadScript`,
//! Plan/14 §6.19, ref/22 §6.9; src:_Common/2DRender.cpp:1656-1861).
//!
//! The original loop is `do { GetToken(); if keyword ... } while (tok != FINISHED)` over a raw `CScanner`
//! with the keywords `number`, `MULTI_LANGUAGE`, `texture`, `data`, `datauv`, `pos` and `serialize`; **every
//! other token is skipped**. That rule is why the record lines of the two world-map files
//! (`texMapMonster_*.inc`, `texMapRainbow_NPC.inc`) do not disturb their atlas parse, and why the two
//! non-atlas files of `Theme/` (`Openning.inc`, `TexBannerList.inc`) parse to an atlas without texture or
//! sprites.
//!
//! ```text
//! number N                                    sub-image count (informative)
//! MULTI_LANGUAGE                              the texture path is a file name under Theme/
//! texture "path" 0xAARRGGBB                   texture file and colour key (0 = none)
//! data w h uLT vLT uRT vRT uLB vLB uRB vRB    pixel coordinates (divided by the texture size)
//! datauv w h f f f f f f f f                  normalised uv
//! pos w h x y [n|h|v|hv]                      one sub-image, optional flip
//! serialize count w h x y cx cy               `count` sub-images, row-major from (x, y), stepping w and h
//! ```
//!
//! `serialize` walks the texture in `w` x `h` cells (`for y from start.y step h while y < H; for x from
//! start.x step w while x < W`), so the sprite positions depend on the **texture size**: the loader asks a
//! callback for it (the real loader reads the image header through the VFS).

use serde::{Deserialize, Serialize};

/// One sub-image of an atlas.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AtlasSprite {
    /// Width in pixels.
    pub w: i32,
    /// Height in pixels.
    pub h: i32,
    /// Left edge in pixels (the `LT` corner of the `data` forms).
    pub x: i32,
    /// Top edge in pixels.
    pub y: i32,
    /// 0 none, 1 horizontal (`h`), 2 vertical (`v`), 3 both (`hv`).
    pub flip: u8,
    /// `[uLT, vLT, uRT, vRT, uLB, vLB, uRB, vRB]` for the `data` and `datauv` forms (normalised, f32 as the
    /// original computed them); `None` for `pos` and `serialize`, whose corners follow from `x`, `y`, `w`,
    /// `h` and `flip`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uv: Option<[f32; 8]>,
    /// The draw centre of a `serialize` sprite.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub center: Option<(i32, i32)>,
}

/// A parsed `.inc` atlas.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Atlas {
    /// The texture as a canonical VFS path (`theme/imgnavobj.bmp`); as written, lower-cased and `/`-separated,
    /// when the file does not exist. Empty when the file has no `texture` line.
    pub texture: String,
    /// The colour key (`0xffff00ff` is magenta; 0 = none).
    pub color_key: u32,
    /// `MULTI_LANGUAGE`: the path was a file name under `Theme/`.
    pub multi_language: bool,
    /// The `number N` the file declares.
    pub number: u32,
    /// The texture size in pixels (0, 0 when unknown).
    pub size: (u32, u32),
    /// The sub-images in file order.
    pub sprites: Vec<AtlasSprite>,
}

/// What the loader knows about a candidate texture path.
#[cfg(feature = "load")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TexInfo {
    /// The path resolves in the VFS.
    pub exists: bool,
    /// The image size from its header, when it could be read.
    pub size: Option<(u32, u32)>,
}

#[cfg(feature = "load")]
pub(crate) use parse::{canonical_or_lower, parse_atlas};

#[cfg(feature = "load")]
mod parse {
    use super::{Atlas, AtlasSprite, TexInfo};
    use flyff_script::{DiagCode, Diagnostics, Scanner, ScannerOptions, Severity};
    use std::sync::Arc;

    /// Upper bound of the sprites one `serialize` line may create (the data has at most 86).
    const MAX_SERIALIZE: i32 = 65_536;

    /// `normalize_resource_path`, or the plain lower-cased `/` form when the path cannot be normalised.
    pub(crate) fn canonical_or_lower(path: &str) -> String {
        flyff_common::normalize_resource_path(path)
            .unwrap_or_else(|_| path.replace('\\', "/").to_ascii_lowercase())
    }

    /// The candidate texture paths of a `texture` line, in lookup order.
    fn candidates(written: &str, multi_language: bool) -> Vec<String> {
        let name = written.rsplit(['\\', '/']).next().unwrap_or(written);
        let theme = canonical_or_lower(&format!("Theme/{name}"));
        if multi_language {
            vec![theme]
        } else {
            let as_written = canonical_or_lower(written);
            if as_written == theme {
                vec![as_written]
            } else {
                vec![as_written, theme]
            }
        }
    }

    fn norm(v: i32, size: u32) -> f32 {
        v as f32 / size as f32
    }

    /// `CTexturePack::LoadScript` over decoded `text`. `info(path)` reports the texture header.
    pub(crate) fn parse_atlas(
        text: &str,
        file: &Arc<str>,
        info: &mut dyn FnMut(&str) -> TexInfo,
        diags: &mut Diagnostics,
    ) -> Atlas {
        let mut atlas = Atlas::default();
        let mut sc = Scanner::new(text, Arc::clone(file), ScannerOptions::default());
        let mut warned_size = false;
        loop {
            sc.get_token(false);
            if sc.finished() {
                break;
            }
            let key = sc.token().to_owned();
            match key.as_str() {
                "number" => atlas.number = sc.get_number(false),
                "MULTI_LANGUAGE" => atlas.multi_language = true,
                "texture" => {
                    sc.get_token(false);
                    let written = sc.token().to_owned();
                    atlas.color_key = sc.get_hex(false);
                    let cands = candidates(&written, atlas.multi_language);
                    let mut chosen: Option<(String, TexInfo)> = None;
                    for c in &cands {
                        let i = info(c);
                        if i.exists {
                            chosen = Some((c.clone(), i));
                            break;
                        }
                    }
                    match chosen {
                        Some((path, i)) => {
                            atlas.texture = path;
                            atlas.size = i.size.unwrap_or((0, 0));
                        }
                        None => {
                            atlas.texture = cands.into_iter().next().unwrap_or_default();
                            atlas.size = (0, 0);
                            diags.report(
                                Severity::Info,
                                DiagCode::AssetMissing,
                                Some(file),
                                sc.line(),
                                format!("atlas texture {written} does not exist"),
                            );
                        }
                    }
                }
                "data" => {
                    let w = sc.get_number_i32(false);
                    let h = sc.get_number_i32(false);
                    let mut px = [0i32; 8];
                    for p in &mut px {
                        *p = sc.get_number_i32(false);
                    }
                    let (tw, th) = atlas.size;
                    let uv = (tw > 0 && th > 0).then(|| {
                        [
                            norm(px[0], tw),
                            norm(px[1], th),
                            norm(px[2], tw),
                            norm(px[3], th),
                            norm(px[4], tw),
                            norm(px[5], th),
                            norm(px[6], tw),
                            norm(px[7], th),
                        ]
                    });
                    atlas.sprites.push(AtlasSprite {
                        w,
                        h,
                        x: px[0],
                        y: px[1],
                        flip: 0,
                        uv,
                        center: None,
                    });
                }
                "datauv" => {
                    let w = sc.get_number_i32(false);
                    let h = sc.get_number_i32(false);
                    let mut uv = [0f32; 8];
                    for u in &mut uv {
                        *u = sc.get_float(false);
                    }
                    let (tw, th) = atlas.size;
                    let x = (f64::from(uv[0]) * f64::from(tw)).round() as i32;
                    let y = (f64::from(uv[1]) * f64::from(th)).round() as i32;
                    atlas.sprites.push(AtlasSprite {
                        w,
                        h,
                        x,
                        y,
                        flip: 0,
                        uv: Some(uv),
                        center: None,
                    });
                }
                "pos" => {
                    let w = sc.get_number_i32(false);
                    let h = sc.get_number_i32(false);
                    let x = sc.get_number_i32(false);
                    let y = sc.get_number_i32(false);
                    // the original reads the flip token unconditionally (a missing flag eats the next keyword)
                    sc.get_token(false);
                    let flip = match sc.token() {
                        "h" => 1,
                        "v" => 2,
                        "hv" => 3,
                        _ => 0,
                    };
                    atlas.sprites.push(AtlasSprite {
                        w,
                        h,
                        x,
                        y,
                        flip,
                        uv: None,
                        center: None,
                    });
                }
                "serialize" => {
                    let frames = sc.get_number_i32(false);
                    let w = sc.get_number_i32(false);
                    let h = sc.get_number_i32(false);
                    let sx = sc.get_number_i32(false);
                    let sy = sc.get_number_i32(false);
                    let cx = sc.get_number_i32(false);
                    let cy = sc.get_number_i32(false);
                    let (tw, th) = atlas.size;
                    let known = tw > 0 && th > 0;
                    if !known && !warned_size {
                        warned_size = true;
                        diags.report(
                            Severity::Warning,
                            DiagCode::AssetMissing,
                            Some(file),
                            sc.line(),
                            "atlas: the texture size is unknown, serialize frames are laid out in one row".to_owned(),
                        );
                    }
                    let (tw, th) = if known {
                        (i64::from(tw), i64::from(th))
                    } else {
                        (i64::MAX / 4, i64::from(sy) + 1)
                    };
                    let total = frames.clamp(0, MAX_SERIALIZE);
                    let mut made = 0i32;
                    if w > 0 && h > 0 {
                        let mut y = i64::from(sy);
                        'rows: while y < th {
                            let mut x = i64::from(sx);
                            while x < tw {
                                if made >= total {
                                    break 'rows;
                                }
                                atlas.sprites.push(AtlasSprite {
                                    w,
                                    h,
                                    x: x as i32,
                                    y: y as i32,
                                    flip: 0,
                                    uv: None,
                                    center: Some((cx, cy)),
                                });
                                made += 1;
                                x += i64::from(w);
                            }
                            y += i64::from(h);
                        }
                    }
                }
                _ => {}
            }
        }
        if atlas.number as usize != atlas.sprites.len() && !atlas.texture.is_empty() {
            diags.report(
                Severity::Warning,
                DiagCode::ValueOutOfRange,
                Some(file),
                0,
                format!(
                    "atlas declares {} sub-images but defines {}",
                    atlas.number,
                    atlas.sprites.len()
                ),
            );
        }
        diags.extend(sc.take_diagnostics());
        atlas
    }
}
