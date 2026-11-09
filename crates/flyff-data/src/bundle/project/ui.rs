//! `resdata-layouts`, `ui-atlases`, `sounds`, `music`, `terrain`.

use super::{Sources, canon, resolve};
use crate::bundle::BundleError;
use crate::bundle::types::{
    AppletRow, AtlasDto, ControlRow, MusicBundle, ResdataBundle, SoundsBundle, TerrainBundle,
    TerrainTexture, UiAtlasesBundle,
};
use std::collections::BTreeMap;

/// `resdata-layouts`: the 365 applets with their controls (the three ignored flags are dropped).
pub fn project_resdata(src: &Sources<'_>) -> Result<ResdataBundle, BundleError> {
    let applets = src
        .client
        .layouts
        .iter()
        .map(|a| AppletRow {
            name: a.name.clone(),
            id: a.id,
            texture: a.texture.clone(),
            tile: a.tile,
            w: a.w,
            h: a.h,
            style: a.style,
            title: a.title.clone(),
            help: a.help_key.clone(),
            controls: a
                .controls
                .iter()
                .map(|c| ControlRow {
                    kind: c.wtype,
                    name: c.name.clone(),
                    id: c.id,
                    texture: c.texture.clone(),
                    tile: c.tile,
                    rect: c.rect,
                    style: c.style,
                    group: c.group,
                    caption: c.caption.clone(),
                    tooltip: c.tooltip.clone(),
                })
                .collect(),
        })
        .collect();
    Ok(ResdataBundle { applets })
}

/// `ui-atlases`: every atlas descriptor by the canonical path of its `.inc`.
pub fn project_atlases(src: &Sources<'_>) -> Result<UiAtlasesBundle, BundleError> {
    let atlases: BTreeMap<String, AtlasDto> = src
        .client
        .atlases
        .iter()
        .map(|(k, a)| {
            (
                canon(k),
                AtlasDto {
                    texture: a.texture.clone(),
                    color_key: a.color_key,
                    multi_language: a.multi_language,
                    size: [a.size.0, a.size.1],
                    sprites: a.sprites.clone(),
                },
            )
        })
        .collect();
    Ok(UiAtlasesBundle { atlases })
}

/// `sounds`: the canonical path `sound/<file>` when the file resolves, `null` when it is missing, `""` for a
/// silent entry; an id the file does not define is absent.
pub fn project_sounds(src: &Sources<'_>) -> Result<SoundsBundle, BundleError> {
    let sounds = src
        .client
        .sounds
        .iter()
        .map(|(id, f)| {
            let v = if f.is_empty() {
                Some(String::new())
            } else {
                resolve(src.files, "sound", f)
            };
            (*id, v)
        })
        .collect();
    Ok(SoundsBundle { sounds })
}

/// `music`: the canonical path (`music/<file>`) when the file resolves, `null` otherwise.
pub fn project_music(src: &Sources<'_>) -> Result<MusicBundle, BundleError> {
    let music = src
        .client
        .music
        .iter()
        .map(|(id, f)| (*id, resolve(src.files, "", f)))
        .collect();
    Ok(MusicBundle { music })
}

/// `terrain`: the rows of `Terrain.inc` in file order.
pub fn project_terrain(src: &Sources<'_>) -> Result<TerrainBundle, BundleError> {
    Ok(TerrainBundle {
        textures: src
            .client
            .terrain
            .iter()
            .map(|t| TerrainTexture {
                folder: t.folder.clone(),
                id: t.id,
                frames: t.frames,
                file: t.file.clone(),
                sound: t.sound.clone(),
                group_path: t.group_path.clone(),
            })
            .collect(),
    })
}
