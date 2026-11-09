//! `worlds`, `models`, `sfx-table`.

use super::{Sources, canon, resolve};
use crate::bundle::BundleError;
use crate::bundle::rowtable::RowTable;
use crate::bundle::types::{
    ClientRegion, MapMonsterDto, MapMonsterRow, ModelRow, ModelsBundle, RainbowMarkerDto, SfxRow,
    SfxTableBundle, WorldFolderDto, WorldMapRow, WorldRow, WorldSettingsDto, WorldsBundle,
};
use crate::models::ModelElem;
use crate::worlds::{RI_BEGIN, RI_REVIVAL, RI_STRUCTURE, RegionDef, WldSettings};
use flyff_common::defs::define::{MODELTYPE_SFX, OT_SFX};
use std::collections::BTreeMap;

/// `CWorldMap` (src:Neuz/WorldMap.cpp:197-208 textures, :1399-1460 rectangles) and `CContinent::GetMapNo`
/// (src:_Common/Continent.cpp:414-430) with the V15 continent ids of `Resource/ContinentDef.h`:
/// `(map, texture, continent, [x1, z1, x2, z2])` (14 §10.5; the screen z axis is inverted inside `rect`).
pub const WORLD_MAPS: [(u32, &str, Option<u32>, [i32; 4]); 8] = [
    (0, "theme/world_flyff.dds", None, [0, 1298, 11800, 9757]),
    (
        1,
        "theme/world_darkon12.dds",
        Some(241),
        [2355, 2019, 7187, 5814],
    ),
    (
        2,
        "theme/world_darkon3.dds",
        Some(242),
        [985, 1621, 5754, 5149],
    ),
    (
        3,
        "theme/world_ricis.dds",
        Some(3),
        [8080, 3200, 10280, 5000],
    ),
    (
        4,
        "theme/world_saint.dds",
        Some(2),
        [6854, 1580, 10600, 4430],
    ),
    (
        5,
        "theme/world_flaris.dds",
        Some(1),
        [5500, 2540, 9200, 5130],
    ),
    (
        6,
        "theme/world_harmonin.dds",
        Some(243),
        [4795, 4587, 8391, 7236],
    ),
    (
        7,
        "theme/world_estia.dds",
        Some(4),
        [5150, 5573, 9890, 9158],
    ),
];

/// A `.wld` texture name (a bare file name under `Weather/`) as a canonical path; `None` for the built-in.
fn weather(name: &Option<String>) -> Option<String> {
    name.as_ref()
        .filter(|n| !n.is_empty() && !n.eq_ignore_ascii_case("default"))
        .map(|n| canon(&format!("Weather/{n}")))
}

fn settings(s: &WldSettings) -> WorldSettingsDto {
    WorldSettingsDto {
        size: [s.size.0, s.size.1],
        indoor: s.indoor,
        fly: s.fly,
        bgm: s.bgm,
        pk_mode: s.pk_mode,
        ambient: s.ambient,
        diffuse: s.diffuse,
        light_dir: s.light_dir,
        fog: [s.fog.0, s.fog.1, s.fog.2],
        mpu: s.mpu,
        sky: s.sky.iter().map(weather).collect(),
        cloud: s.cloud.iter().map(weather).collect(),
        sun: weather(&s.sun),
        moon: weather(&s.moon),
        light24: s
            .light24
            .as_ref()
            .map(|rows| rows.iter().map(|r| r.to_vec()).collect()),
    }
}

/// The client's `CWorld::m_aRegion` entry of a region: every region of the folder except the kinds the world does
/// not keep (`RI_BEGIN`, `RI_REVIVAL`; `RI_STRUCTURE` goes to `m_aStructure`), in `.rgn` file order.
fn client_region(r: &RegionDef) -> Option<ClientRegion> {
    if r.index == RI_BEGIN || r.index == RI_REVIVAL || r.index == RI_STRUCTURE {
        return None;
    }
    Some(ClientRegion {
        index: r.index,
        rect: r.rect,
        attribute: r.attribute,
        music: r.music,
        direct_music: r.direct_music,
        tele_world: r.tele_world,
        key: r.key.clone(),
        title: r.title.clone(),
        desc: r.desc.clone(),
    })
}

/// `worlds`: the world list, the client regions and continents of each folder, the world-map code table and the
/// map records.
pub fn project_worlds(src: &Sources<'_>) -> Result<WorldsBundle, BundleError> {
    let reg = &src.game.worlds;
    let worlds = reg
        .worlds
        .values()
        .map(|w| WorldRow {
            id: w.id,
            define: w.define_name.clone(),
            folder: w.folder.clone(),
            title: w.title.clone(),
            loadable: w.loadable,
        })
        .collect();
    let folders: BTreeMap<String, WorldFolderDto> = reg
        .folders
        .iter()
        .map(|(k, f)| {
            (
                k.clone(),
                WorldFolderDto {
                    settings: settings(&f.settings),
                    regions: f.regions.iter().filter_map(client_region).collect(),
                    continents: f.continents.clone(),
                },
            )
        })
        .collect();
    let map_monsters = src
        .client
        .map_monsters
        .iter()
        .map(|m| MapMonsterDto {
            map: u32::from(m.map),
            atlas: canon(&m.atlas),
            entries: m
                .entries
                .iter()
                .map(|e| MapMonsterRow {
                    frame: e.frame,
                    movers: e.movers.clone(),
                    pos: [e.pos.0, e.pos.1],
                    size: [e.size.0, e.size.1],
                    drop_item: e.drop_item,
                })
                .collect(),
        })
        .collect();
    let rainbow_markers = src
        .client
        .rainbow_markers
        .iter()
        .map(|r| RainbowMarkerDto {
            frame: r.frame,
            map: u32::from(r.map),
            total_pos: [r.total_pos.0, r.total_pos.1],
            map_pos: [r.map_pos.0, r.map_pos.1],
            size: [r.size.0, r.size.1],
        })
        .collect();
    Ok(WorldsBundle {
        worlds,
        folders,
        world_maps: WORLD_MAPS
            .iter()
            .map(|(map, texture, continent, rect)| WorldMapRow {
                map: *map,
                texture: (*texture).to_owned(),
                continent: *continent,
                rect: *rect,
            })
            .collect(),
        map_monsters,
        rainbow_markers,
    })
}

/// The directory a model file lives in: `.sfx` effects in `sfx/`, everything else in `model/`.
fn model_dir(e: &ModelElem) -> &'static str {
    if e.model_type == MODELTYPE_SFX {
        "sfx"
    } else {
        "model"
    }
}

/// The registry file name as a path stem: effects get their `.sfx` extension (`MakeModelName` gives none).
fn model_file_name(src: &Sources<'_>, e: &ModelElem) -> Option<String> {
    let f = src.game.models.model_file(e.obj_type, e.index)?;
    Some(if e.model_type == MODELTYPE_SFX {
        format!("{f}.sfx")
    } else {
        f
    })
}

/// `models`: the model registry of every object type with resolved files (`null` when missing).
pub fn project_models(src: &Sources<'_>) -> Result<ModelsBundle, BundleError> {
    let reg = &src.game.models;
    let mut types = BTreeMap::new();
    for (ot, table) in &reg.by_type {
        let rows: Vec<ModelRow> = table
            .iter()
            .map(|(_, e)| {
                let dir = model_dir(e);
                ModelRow {
                    index: e.index,
                    name: e.name.clone(),
                    model_type: e.model_type,
                    part: e.part.clone(),
                    fly: e.fly,
                    distant: e.distant,
                    pick: e.pick,
                    scale: e.scale,
                    trans: e.trans,
                    shadow: e.shadow,
                    texture_ex: e.texture_ex,
                    render_flag: e.render_flag,
                    file: model_file_name(src, e).and_then(|f| resolve(src.files, dir, &f)),
                    bone: reg
                        .bone_file(e.obj_type, e.index)
                        .and_then(|f| resolve(src.files, "model", &f)),
                    motions: e
                        .motions
                        .iter()
                        .map(|(mti, _)| {
                            (
                                *mti,
                                reg.motion_file(e.obj_type, e.index, *mti)
                                    .and_then(|f| resolve(src.files, "model", &f)),
                            )
                        })
                        .collect(),
                }
            })
            .collect();
        types.insert(ot.to_string(), RowTable::new(rows));
    }
    Ok(ModelsBundle { types })
}

/// `sfx-table`: the `OT_SFX` entries by `XI_` id with their `.sfx` file and attached sound.
pub fn project_sfx_table(src: &Sources<'_>) -> Result<SfxTableBundle, BundleError> {
    let rows: Vec<SfxRow> = src
        .game
        .models
        .by_type
        .get(&OT_SFX)
        .map(|t| {
            t.iter()
                .map(|(_, e)| SfxRow {
                    id: e.index,
                    name: e.name.clone(),
                    file: model_file_name(src, e).and_then(|f| resolve(src.files, "sfx", &f)),
                    sound: (!e.part.is_empty()).then(|| e.part.clone()),
                    scale: e.scale,
                    distant: e.distant,
                    render_flag: e.render_flag,
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(SfxTableBundle {
        sfx: RowTable::new(rows),
    })
}
