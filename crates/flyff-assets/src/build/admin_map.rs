//! The `admin-map` kind (Plan/25 §3.3, §3.4, §11.7; WP-ADMIN-08): the overview pyramid of a world's minimap and its
//! coverage document, for the admin console's live map.
//!
//! **Level 0** is not built: it is the original minimap, `World/<Wd>/<Wd>XX-ZZ.dds` with `XX` the landscape column and
//! `ZZ` the landscape row (z), 256 x 256 pixels per landscape, image row 0 on the **north** edge (the original
//! navigator, `src:_Interface/WndField.cpp:9599-9642`), served by the `/res` route like any other texture.
//! **Levels 1..k** are built here: a level-`k` tile covers `2^k x 2^k` landscapes in one 256 px PNG (a box filter of the
//! level below, premultiplied by alpha, so the edge of a land without a tile does not darken its neighbours). `k` goes
//! up to the level at which the whole world fits one tile (`ceil(log2(max(lands)))`: Madrigal 5, with
//! 225 + 64 + 16 + 4 + 1 = 310 tiles when every land has a tile; the real world has 249 of 900 lands covered, so
//! about 150 tiles are built: a tile exists iff one of its landscapes has a minimap). A land without a tile stays
//! transparent: the client draws its void colour underneath.
//!
//! **The 257-pixel anomaly.** One tile of `WdHeaven06` and one of `WdHeaven06_1` are 257 x 256 pixels. Every tile is
//! cropped to its first 256 columns before it is filtered, and the coverage document lists the odd tiles so a client
//! that draws level 0 directly knows to use `256 / 257` of the texture's width.
//!
//! Keys (`/gen/admin-map/<fp>/<key>`, partition = the world folder, job = the world id; the whole pyramid of a world is
//! one job, built on the first request and served from the cache afterwards):
//!
//! | key | content |
//! |---|---|
//! | `world-<id>.tiles.json` | the coverage document ([`TilesDoc`]) |
//! | `<id>/<k>/<tx>_<tz>.png` | the level-`k` tile at column `tx`, row `tz` of that level (north = high `tz`, like the lands) |
//!
//! `jobs()` is empty: the kind is built lazily on request and neither `flyff prepare` nor the prewarm touches it.

use crate::build::png::{decode_source, encode_png};
use crate::error::AssetError;
use crate::fp::{Fp, FpCtx};
use crate::kinds::{AssetBuilder, AssetKind, BuildCtx, BuiltFile, JobKey, KeyRoute};
use flyff_data::GameData;
use flyff_data::types::WorldId;
use flyff_vfs::Vfs;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// Bump on any change that alters the produced bytes.
pub const BUILDER_VERSION: u32 = 1;
/// The layout of the coverage document.
pub const FORMAT_VERSION: u32 = 1;
/// Pixels per tile side, at every level.
pub const TILE_PX: u32 = 256;

/// The highest level that can exist (a world of 2^16 lands per side is far beyond any real one).
const MAX_LEVEL_CAP: u32 = 16;

/// The minimap of one world: which landscapes have a tile and where the files are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Coverage {
    /// The world id.
    pub world: WorldId,
    /// The folder, lower case (`wdmadrigal`), the key of `WorldRegistry.folders` and the partition of this kind.
    pub folder: String,
    /// The file-name prefix of the tiles, lower case (`wdmadrigal` of `wdmadrigal13-06.dds`).
    pub prefix: String,
    /// Landscapes in x and z (`.wld` `size`).
    pub lands: (u32, u32),
    /// Metres per heightmap unit.
    pub mpu: u32,
    /// The landscapes that have a tile `(x, z)`, with the canonical VFS path of the file.
    pub tiles: BTreeMap<(u32, u32), String>,
}

impl Coverage {
    /// The side of a landscape in metres (`128 * mpu`).
    pub fn land_m(&self) -> u32 {
        128 * self.mpu
    }

    /// The highest overview level: the level at which the whole world fits one tile (0 for a one-landscape world).
    pub fn max_level(&self) -> u32 {
        let longest = self.lands.0.max(self.lands.1).max(1);
        (longest.next_power_of_two().trailing_zeros()).min(MAX_LEVEL_CAP)
    }

    /// `true` when landscape `(x, z)` has a tile.
    pub fn covered(&self, x: u32, z: u32) -> bool {
        self.tiles.contains_key(&(x, z))
    }

    /// One string per landscape row, row 0 (z = 0, the south) first, one `1` or `0` per column.
    pub fn rows(&self) -> Vec<String> {
        (0..self.lands.1)
            .map(|z| {
                (0..self.lands.0)
                    .map(|x| if self.covered(x, z) { '1' } else { '0' })
                    .collect()
            })
            .collect()
    }

    /// Columns and rows of the level-`k` grid (`ceil(lands / 2^k)`).
    pub fn grid(&self, k: u32) -> (u32, u32) {
        let s = 1_u32 << k.min(MAX_LEVEL_CAP);
        (self.lands.0.div_ceil(s), self.lands.1.div_ceil(s))
    }

    /// The tiles of level `k >= 1` that exist: those with at least one covered landscape, ascending by row then
    /// column.
    pub fn level_tiles(&self, k: u32) -> Vec<(u32, u32)> {
        if k == 0 {
            return self.tiles.keys().map(|&(x, z)| (x, z)).collect();
        }
        let set: BTreeSet<(u32, u32)> = self.tiles.keys().map(|&(x, z)| (z >> k, x >> k)).collect();
        set.into_iter().map(|(tz, tx)| (tx, tz)).collect()
    }

    /// The number of overview tiles over all levels `1..=max_level`.
    pub fn overview_tile_count(&self) -> usize {
        (1..=self.max_level())
            .map(|k| self.level_tiles(k).len())
            .sum()
    }

    /// Does the overview tile `(tx, tz)` of level `k` exist?
    pub fn has_tile(&self, k: u32, tx: u32, tz: u32) -> bool {
        k >= 1
            && k <= self.max_level()
            && {
                let (cols, rows) = self.grid(k);
                tx < cols && tz < rows
            }
            && (0..1_u32 << k)
                .any(|dz| (0..1_u32 << k).any(|dx| self.covered((tx << k) + dx, (tz << k) + dz)))
    }

    /// Where the world position `(x, z)` (metres) falls on the level-0 tiles (25 §3.3, the placement of the original
    /// navigator): the landscape `(lx, lz)` and the pixel `(u, v)` in its 256 x 256 tile, `v` counted from the north
    /// edge: `u = (x - lx * L) * 256 / L`, `v = ((lz + 1) * L - z) * 256 / L` with `L = 128 * mpu`. `None` outside
    /// the world.
    pub fn locate(&self, x: f32, z: f32) -> Option<Located> {
        let l = self.land_m() as f32;
        if !(x.is_finite() && z.is_finite()) || x < 0.0 || z < 0.0 {
            return None;
        }
        let (lx, lz) = ((x / l) as u32, (z / l) as u32);
        if lx >= self.lands.0 || lz >= self.lands.1 {
            return None;
        }
        let px = TILE_PX as f32 / l;
        Some(Located {
            lx,
            lz,
            u: (x - lx as f32 * l) * px,
            v: ((lz + 1) as f32 * l - z) * px,
            covered: self.covered(lx, lz),
        })
    }

    /// The file name of the tile of landscape `(x, z)` (`wdmadrigal13-06.dds`).
    pub fn file_name(&self, x: u32, z: u32) -> String {
        format!("{}{x:02}-{z:02}.dds", self.prefix)
    }
}

/// A world position on the level-0 minimap tiles ([`Coverage::locate`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Located {
    /// Landscape column.
    pub lx: u32,
    /// Landscape row.
    pub lz: u32,
    /// Pixel column in the tile (0 = west edge).
    pub u: f32,
    /// Pixel row in the tile (0 = **north** edge).
    pub v: f32,
    /// The landscape has a tile.
    pub covered: bool,
}

/// The minimap coverage of world `world`: the tile files of its folder, each landscape inside the world's size once.
/// `None` when the world or its folder is unknown or has no tile at all.
pub fn coverage(vfs: &Vfs, data: &GameData, world: WorldId) -> Option<Coverage> {
    let def = data.worlds.world(world)?;
    let folder = data.worlds.folder_data(world)?;
    let key = flyff_data::worlds::WorldRegistry::folder_key(&def.folder);
    let dir = format!("world/{key}");
    let lands = folder.settings.size;
    let mpu = if folder.settings.mpu == 0 {
        4
    } else {
        folder.settings.mpu
    };
    let mut tiles = BTreeMap::new();
    for e in vfs.list_dir(&dir) {
        let path = e.path.as_str();
        let Some(name) = path.rsplit('/').next() else {
            continue;
        };
        let Some((x, z)) = parse_tile_name(name, &key) else {
            continue;
        };
        if x < lands.0 && z < lands.1 {
            tiles.insert((x, z), path.to_owned());
        }
    }
    (!tiles.is_empty()).then(|| Coverage {
        world,
        folder: key.clone(),
        prefix: key,
        lands,
        mpu,
        tiles,
    })
}

/// `wdmadrigal13-06.dds` with prefix `wdmadrigal` -> `(13, 6)`.
fn parse_tile_name(name: &str, prefix: &str) -> Option<(u32, u32)> {
    let rest = name.strip_prefix(prefix)?.strip_suffix(".dds")?;
    let (x, z) = rest.split_once('-')?;
    let digits = |s: &str| {
        (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse::<u32>().ok())?
    };
    Some((digits(x)?, digits(z)?))
}

// ---- the coverage document -----------------------------------------------------------------------------------------

/// One odd-sized level-0 tile (the 257 x 256 anomaly).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OddTile {
    /// Landscape column.
    pub x: u32,
    /// Landscape row.
    pub z: u32,
    /// Width in pixels of the file.
    pub w: u32,
    /// Height in pixels of the file.
    pub h: u32,
}

/// One overview level of the coverage document.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LevelDoc {
    /// The level (1..).
    pub k: u32,
    /// Tile columns.
    pub cols: u32,
    /// Tile rows.
    pub rows: u32,
    /// The tiles that exist `[tx, tz]`, ascending by row then column.
    pub tiles: Vec<[u32; 2]>,
}

/// `world-<id>.tiles.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TilesDoc {
    /// Layout version ([`FORMAT_VERSION`]).
    pub version: u32,
    /// The world id.
    pub world: u32,
    /// The folder (lower case).
    pub folder: String,
    /// The file-name prefix of the level-0 tiles (`<prefix>XX-ZZ.dds`, two digits at least).
    pub prefix: String,
    /// Landscapes in x and z.
    pub lands: [u32; 2],
    /// Metres per heightmap unit.
    pub mpu: u32,
    /// The side of a landscape in metres.
    pub land_m: u32,
    /// Pixels per tile side.
    pub tile_px: u32,
    /// The highest overview level.
    pub max_level: u32,
    /// Landscapes with a tile: one string per row (row 0 = z 0 first), `1` per covered column.
    pub covered: Vec<String>,
    /// How many landscapes have a tile.
    pub covered_count: usize,
    /// Level-0 tiles whose file is not 256 x 256 (cropped to 256 columns in the overview).
    pub odd: Vec<OddTile>,
    /// The overview levels.
    pub levels: Vec<LevelDoc>,
}

// ---- pixels --------------------------------------------------------------------------------------------------------

const PX: usize = TILE_PX as usize;
const HALF: usize = PX / 2;

/// A decoded tile cropped to 256 x 256 RGBA8.
fn decode_land(vfs: &Vfs, path: &str) -> Result<(Vec<u8>, (u32, u32)), AssetError> {
    let bytes = vfs
        .read(path)
        .map_err(|e| AssetError::build(format!("{path}: {e}")))?;
    let (img, _alpha) =
        decode_source("dds", &bytes).map_err(|e| AssetError::build(format!("{path}: {e}")))?;
    let (w, h) = (img.width, img.height);
    // 256 x 256, or the 257 x 256 anomaly cropped to its first 256 columns
    if h != TILE_PX || !(w == TILE_PX || w == TILE_PX + 1) {
        return Err(AssetError::build(format!(
            "{path}: a minimap tile is 256 x 256 pixels, this one is {w} x {h}"
        )));
    }
    let px = if w == TILE_PX {
        img.pixels
    } else {
        let src_stride = w as usize * 4;
        let mut out = Vec::with_capacity(PX * PX * 4);
        for row in img.pixels.chunks_exact(src_stride).take(PX) {
            out.extend_from_slice(row.get(..PX * 4).unwrap_or(row));
        }
        out
    };
    if px.len() != PX * PX * 4 {
        return Err(AssetError::build(format!("{path}: unexpected pixel count")));
    }
    Ok((px, (w, h)))
}

/// A 2 x 2 box filter of a 256 x 256 RGBA8 image to 128 x 128, premultiplied by alpha.
fn halve(src: &[u8]) -> Vec<u8> {
    let mut out = vec![0_u8; HALF * HALF * 4];
    for y in 0..HALF {
        for x in 0..HALF {
            let mut a_sum = 0_u32;
            let mut c_sum = [0_u32; 3];
            for (dy, dx) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
                let i = ((2 * y + dy) * PX + 2 * x + dx) * 4;
                let Some(p) = src.get(i..i + 4) else { continue };
                let a = u32::from(p[3]);
                a_sum += a;
                for (c, s) in c_sum.iter_mut().zip(p) {
                    *c += u32::from(*s) * a;
                }
            }
            let o = (y * HALF + x) * 4;
            if let (a_sum @ 1.., Some(dst)) = (a_sum, out.get_mut(o..o + 4)) {
                for (d, c) in dst.iter_mut().zip(c_sum) {
                    *d = ((c + a_sum / 2) / a_sum).min(255) as u8;
                }
                dst[3] = ((a_sum + 2) / 4).min(255) as u8;
            }
        }
    }
    out
}

/// Copies a 128 x 128 block into a 256 x 256 tile at `(x0, y0)`.
fn blit_half(dst: &mut [u8], block: &[u8], x0: usize, y0: usize) {
    for y in 0..HALF {
        let s = y * HALF * 4;
        let d = ((y0 + y) * PX + x0) * 4;
        if let (Some(src), Some(dst)) = (block.get(s..s + HALF * 4), dst.get_mut(d..d + HALF * 4)) {
            dst.copy_from_slice(src);
        }
    }
}

/// Where the four children of a tile go in it: `(dx, dz)` of the child, `(x0, y0)` of its block. North (the higher
/// `z`) is the top of the image.
const CHILDREN: [(u32, u32, usize, usize); 4] = [
    (0, 0, 0, HALF),
    (1, 0, HALF, HALF),
    (0, 1, 0, 0),
    (1, 1, HALF, 0),
];

// ---- the pyramid -------------------------------------------------------------------------------------------------

/// Builds the pyramid of `cov`: the coverage document and the PNGs of the levels `1..=max_level`.
pub fn build_pyramid(ctx: &BuildCtx<'_>, cov: &Coverage) -> Result<Vec<BuiltFile>, AssetError> {
    let max = cov.max_level();
    let mut odd = Vec::new();
    // level 0 -> 1: decode each land once, keep its 128 x 128 half
    let mut halves: BTreeMap<(u32, u32), Vec<u8>> = BTreeMap::new();
    for (&(x, z), path) in &cov.tiles {
        if ctx.cancelled() {
            return Err(AssetError::shutting_down());
        }
        let (px, (w, h)) = decode_land(ctx.vfs, path)?;
        if (w, h) != (TILE_PX, TILE_PX) {
            odd.push(OddTile { x, z, w, h });
        }
        if max >= 1 {
            halves.insert((x, z), halve(&px));
        }
    }
    let mut files = Vec::new();
    let mut levels = Vec::new();
    // the tiles of the level below, as 256 x 256 RGBA8
    let mut below: BTreeMap<(u32, u32), Vec<u8>> = BTreeMap::new();
    for k in 1..=max {
        let mut level: BTreeMap<(u32, u32), Vec<u8>> = BTreeMap::new();
        for (tx, tz) in cov.level_tiles(k) {
            if ctx.cancelled() {
                return Err(AssetError::shutting_down());
            }
            let mut tile = vec![0_u8; PX * PX * 4];
            for (dx, dz, x0, y0) in CHILDREN {
                let child = (tx * 2 + dx, tz * 2 + dz);
                if k == 1 {
                    if let Some(h) = halves.get(&child) {
                        blit_half(&mut tile, h, x0, y0);
                    }
                } else if let Some(t) = below.get(&child) {
                    blit_half(&mut tile, &halve(t), x0, y0);
                }
            }
            let png = encode_png(TILE_PX, TILE_PX, &tile).map_err(|e| {
                AssetError::build(format!("{}: level {k} tile {tx}_{tz}: {e}", cov.folder))
            })?;
            files.push(
                BuiltFile::new(format!("{}/{k}/{tx}_{tz}.png", cov.world), png, "image/png")
                    .uncompressed(),
            );
            level.insert((tx, tz), tile);
        }
        let (cols, rows) = cov.grid(k);
        levels.push(LevelDoc {
            k,
            cols,
            rows,
            tiles: level.keys().map(|&(tx, tz)| [tx, tz]).collect(),
        });
        below = level;
        if k == 1 {
            halves.clear();
        }
    }
    let doc = TilesDoc {
        version: FORMAT_VERSION,
        world: cov.world,
        folder: cov.folder.clone(),
        prefix: cov.prefix.clone(),
        lands: [cov.lands.0, cov.lands.1],
        mpu: cov.mpu,
        land_m: cov.land_m(),
        tile_px: TILE_PX,
        max_level: max,
        covered: cov.rows(),
        covered_count: cov.tiles.len(),
        odd,
        levels,
    };
    let json = serde_json::to_vec(&doc)
        .map_err(|e| AssetError::build(format!("{}: coverage document: {e}", cov.folder)))?;
    files.push(BuiltFile::new(
        format!("world-{}.tiles.json", cov.world),
        json,
        "application/json",
    ));
    Ok(files)
}

// ---- the builder ---------------------------------------------------------------------------------------------------

/// The builder of kind [`AssetKind::AdminMap`].
#[derive(Clone, Copy, Debug, Default)]
pub struct AdminMapBuilder;

/// A parsed key.
enum Key {
    Doc(WorldId),
    Tile {
        world: WorldId,
        k: u32,
        tx: u32,
        tz: u32,
    },
}

fn parse_key(key: &str) -> Result<Key, AssetError> {
    let bad = || AssetError::bad_key(format!("{key:?}: not an admin-map key"));
    if let Some(id) = key
        .strip_prefix("world-")
        .and_then(|r| r.strip_suffix(".tiles.json"))
    {
        return Ok(Key::Doc(id.parse().map_err(|_| bad())?));
    }
    let stem = key.strip_suffix(".png").ok_or_else(bad)?;
    let mut parts = stem.split('/');
    let (world, k, tile) = (
        parts.next().ok_or_else(bad)?,
        parts.next().ok_or_else(bad)?,
        parts.next().ok_or_else(bad)?,
    );
    if parts.next().is_some() {
        return Err(bad());
    }
    let (tx, tz) = tile.split_once('_').ok_or_else(bad)?;
    let num = |s: &str| -> Result<u32, AssetError> {
        if s.is_empty()
            || s.len() > 9
            || !s.bytes().all(|b| b.is_ascii_digit())
            || (s.len() > 1 && s.starts_with('0'))
        {
            return Err(bad());
        }
        s.parse().map_err(|_| bad())
    };
    Ok(Key::Tile {
        world: num(world)?,
        k: num(k)?,
        tx: num(tx)?,
        tz: num(tz)?,
    })
}

impl AssetBuilder for AdminMapBuilder {
    fn kind(&self) -> AssetKind {
        AssetKind::AdminMap
    }

    fn builder_version(&self) -> u32 {
        BUILDER_VERSION
    }

    fn format_version(&self) -> u32 {
        FORMAT_VERSION
    }

    /// `world-<id>.tiles.json` or `<id>/<k>/<tx>_<tz>.png`; the world must have a minimap and the tile must exist.
    fn route(&self, ctx: &FpCtx<'_>, key: &str) -> Result<KeyRoute, AssetError> {
        let (world, tile) = match parse_key(key)? {
            Key::Doc(w) => (w, None),
            Key::Tile { world, k, tx, tz } => (world, Some((k, tx, tz))),
        };
        let cov = coverage(ctx.vfs, ctx.data, world)
            .ok_or_else(|| AssetError::not_found(format!("world {world} has no minimap")))?;
        if let Some((k, tx, tz)) = tile
            && !cov.has_tile(k, tx, tz)
        {
            return Err(AssetError::not_found(format!(
                "world {world} has no overview tile {k}/{tx}_{tz}"
            )));
        }
        Ok(KeyRoute {
            job: JobKey::new(AssetKind::AdminMap, cov.folder.clone(), world.to_string()),
            partition: cov.folder,
        })
    }

    /// The tile files and the `.wld` of the world folder (its size decides the levels).
    fn partition_fp(&self, ctx: &FpCtx<'_>, partition: &str) -> Option<Fp> {
        let in_registry = ctx.data.worlds.folders.contains_key(partition);
        in_registry.then(|| {
            ctx.kind_fp(
                AssetKind::AdminMap,
                BUILDER_VERSION,
                FORMAT_VERSION,
                &[ctx.set_fp(&format!("world/{partition}"), &["dds", "wld"])],
            )
        })
    }

    /// Nothing is prepared ahead: the pyramid of a world is built on the first request (25 §11.7).
    fn jobs(&self, _ctx: &FpCtx<'_>) -> Vec<JobKey> {
        Vec::new()
    }

    fn build(&self, ctx: &BuildCtx<'_>, job: &JobKey) -> Result<Vec<BuiltFile>, AssetError> {
        let world: WorldId = job
            .name
            .parse()
            .map_err(|_| AssetError::build(format!("{job}: the job name is a world id")))?;
        let cov = coverage(ctx.vfs, ctx.data, world)
            .ok_or_else(|| AssetError::build(format!("world {world} has no minimap")))?;
        build_pyramid(ctx, &cov)
    }

    fn content_type(&self, key: &str) -> &'static str {
        if key.ends_with(".json") {
            "application/json"
        } else {
            "image/png"
        }
    }
}
