//! The world catalog and the shared per-folder static data (Plan/15 §3, WP-WORLD-01).
//!
//! * **Which worlds load** (§3.1): every `World.inc` entry whose folder and `.wld` exist
//!   (`WorldDef.loadable`), in ascending `WorldId`. With the reference data that is exactly the 51 ids of
//!   [`REFERENCE_WORLD_IDS`] (check `C-W01`, [`check_c_w01`]).
//! * **`WorldStatic`** (§3.2): built once per folder and shared through `Arc` by every world id (and every layer)
//!   of the folder: settings, the `CollisionWorld`, the regions, the raw respawn lines and the parsed `.dyo`.
//! * **Per-world constants** (§3.3): [`GridGeom`], [`n_view`] and the bounds helpers
//!   ([`WorldStatic::in_world`], [`WorldStatic::in_world3`], [`WorldStatic::clip_x`]).
//!
//! The collision worlds are built by `flyff-assets` (`ServerDerived.collision`); `flyff-game` has no
//! dependency on that crate (Plan/01 §5.3), so the builder takes them through the [`CollisionSource`] trait,
//! which the server implements over `CollisionSet::folder` (a closure works too).

use flyff_common::WorldId;
use flyff_common::defs::world::WI_WORLD_GUILDWAR;
use flyff_common::math::Vec3;
use flyff_data::GameData;
use flyff_data::worlds::{
    RI_ATTRIBUTE, RI_PLACE, RI_STRUCTURE, RI_TRIGGER, RegionDef, RespawnDef, WldSettings,
};
use flyff_formats::dyo::DyoFile;
use flyff_sim::CollisionWorld;
use flyff_vfs::Vfs;
use std::collections::BTreeMap;
use std::sync::Arc;

/// `WLD_EXTRA_WIDTH`: the border `clip_x` / `clip_z` keep movers away from (`World.h:34`).
pub const WLD_EXTRA_WIDTH: f32 = 10.0;
/// Cells (heightmap units) of one land side (`MAP_SIZE`).
pub const MAP_SIZE: u32 = 128;
/// The link-map cell of level 0 is `CELL_UNITS` heightmap units wide (`nUnit = 8`, 15 §6.1).
pub const CELL_UNITS: u32 = 8;

/// The 51 world ids the reference data loads (F-080, ref/06 §5.3): 1, 2, 3, 21-25, 121-125, 151-167, 200-214,
/// 220, 230, 241-244.
pub const REFERENCE_WORLD_IDS: [u32; 51] = [
    1, 2, 3, 21, 22, 23, 24, 25, 121, 122, 123, 124, 125, 151, 152, 153, 154, 155, 156, 157, 158,
    159, 160, 161, 162, 163, 164, 165, 166, 167, 200, 201, 202, 203, 204, 205, 206, 207, 208, 209,
    210, 211, 212, 213, 214, 220, 230, 241, 242, 243, 244,
];

/// The link-map geometry of a world (15 §3.3, §6.1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridGeom {
    /// Metres per heightmap unit (1, 2 or 4).
    pub mpu: u32,
    /// Side of a cell in metres: `8 * mpu` (32 m at MPU 4).
    pub cell_m: f32,
    /// Cells in x and z: `16 * lands` each.
    pub cells: (i32, i32),
    /// The window radius `R = 2 * n_view` in cells.
    pub radius: i32,
}

/// `nView` of a world (`src:_Common/WorldFile.cpp:237-245`, ref/06 §8.2): 1, +1 for the guild-war world 202,
/// plus `4 - mpu` for the smaller-MPU worlds (`__GUILD_HOUSE`: MPU 2 gives +2, MPU 1 +3).
pub fn n_view(world: WorldId, mpu: u32) -> u32 {
    let mut n = 1;
    if world.0 == WI_WORLD_GUILDWAR {
        n = 2;
    }
    n + 4_u32.saturating_sub(mpu)
}

impl GridGeom {
    /// The geometry of `world` with `lands` lands per side and `mpu` metres per unit.
    pub fn new(world: WorldId, mpu: u32, lands: (u32, u32)) -> GridGeom {
        let per_land = (MAP_SIZE / CELL_UNITS) as i32;
        let clamp = |l: u32| i32::try_from(l).unwrap_or(i32::MAX / per_land).max(1);
        GridGeom {
            mpu,
            cell_m: (CELL_UNITS * mpu) as f32,
            cells: (clamp(lands.0) * per_land, clamp(lands.1) * per_land),
            radius: 2 * n_view(world, mpu) as i32,
        }
    }

    /// Number of cells.
    pub fn cell_count(&self) -> usize {
        (self.cells.0.max(0) as usize) * (self.cells.1.max(0) as usize)
    }
}

/// The immutable data of one world folder, shared by every world id and layer that uses it (15 §3.2).
pub struct WorldStatic {
    /// The lower-case folder key (`wdmadrigal`).
    pub folder: String,
    /// The `.wld` settings (size, indoor, fly, bgm, pk mode, MPU, revival world and key ...).
    pub settings: WldSettings,
    /// Metres per heightmap unit: 1, 2 or 4 (default 4).
    pub mpu: u32,
    /// World size in lands (`settings.size`).
    pub lands: (u32, u32),
    /// World size in heightmap units, `(128 * lands.0, 128 * lands.1)` (`WORLD_WIDTH` / `WORLD_HEIGHT`).
    pub size_units: (u32, u32),
    /// Heights, attributes and static collision (one per folder, 31 §10.1).
    pub collision: Arc<CollisionWorld>,
    /// `RI_TRIGGER`, `RI_ATTRIBUTE` and `RI_PLACE` regions in file order (15 §11.1).
    pub regions: Vec<RegionDef>,
    /// `RI_STRUCTURE` regions (stored, no server use).
    pub structures: Vec<RegionDef>,
    /// The respawn lines of the `.rgn` as read (raw values; WP-WORLD-14 builds the `RespawnProto` table from
    /// them with the load-time adjustments of 15 §15.1-15.2).
    pub respawn_defs: Vec<RespawnDef>,
    /// The parsed `.dyo`: the dynamic objects (NPCs, ctrls) in file order (15 §13.1). Empty when the file is
    /// missing or unreadable.
    pub dyo: DyoFile,
}

impl std::fmt::Debug for WorldStatic {
    /// A summary: the collision data holds megabytes that must not end up in a log line.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorldStatic")
            .field("folder", &self.folder)
            .field("mpu", &self.mpu)
            .field("lands", &self.lands)
            .field("regions", &self.regions.len())
            .field("structures", &self.structures.len())
            .field("respawn_defs", &self.respawn_defs.len())
            .field("dyo_records", &self.dyo.records.len())
            .finish_non_exhaustive()
    }
}

impl WorldStatic {
    /// `VecInWorld(x, z)`: `x >= 0 && x / mpu < size_units.0 - 1` (and the same for `z`), NaN is outside
    /// (15 §3.3; `f32` arithmetic, `x / mpu` is exact for the power-of-two MPUs).
    pub fn in_world(&self, x: f32, z: f32) -> bool {
        let mpu = self.mpu as f32;
        !x.is_nan()
            && !z.is_nan()
            && x >= 0.0
            && z >= 0.0
            && x / mpu < self.size_units.0.saturating_sub(1) as f32
            && z / mpu < self.size_units.1.saturating_sub(1) as f32
    }

    /// `in_world(p.x, p.z)` and a finite-or-infinite but not NaN `y`.
    pub fn in_world3(&self, p: Vec3) -> bool {
        self.in_world(p.x, p.z) && !p.y.is_nan()
    }

    /// `ClipX(x)`: clamps to `[10, size_units.0 * mpu - 10]` (`WLD_EXTRA_WIDTH`); NaN maps to the lower bound.
    pub fn clip_x(&self, x: f32) -> f32 {
        clip(x, self.size_units.0, self.mpu)
    }

    /// `ClipZ(z)`.
    pub fn clip_z(&self, z: f32) -> f32 {
        clip(z, self.size_units.1, self.mpu)
    }

    /// The link-map geometry of world `id` over this folder.
    pub fn grid_geom(&self, id: WorldId) -> GridGeom {
        GridGeom::new(id, self.mpu, self.lands)
    }
}

fn clip(v: f32, size_units: u32, mpu: u32) -> f32 {
    if v.is_nan() || v < WLD_EXTRA_WIDTH {
        return WLD_EXTRA_WIDTH;
    }
    let max = (size_units as f32) * (mpu as f32) - WLD_EXTRA_WIDTH;
    if v > max { max } else { v }
}

/// One loaded world id: its definition and the folder data it uses.
#[derive(Clone, Debug)]
pub struct WorldEntry {
    /// The id.
    pub id: WorldId,
    /// `SetTitle` text.
    pub title: String,
    /// The `WI_*` define name.
    pub define_name: String,
    /// The shared per-folder data.
    pub statics: Arc<WorldStatic>,
}

/// Where the catalog gets the collision world of a folder from (the server: `ServerDerived.collision`).
pub trait CollisionSource {
    /// The collision world of the lower-case folder key; `None` = the chunk set is missing (the world is then
    /// not loaded).
    fn collision(&self, folder: &str) -> Option<Arc<CollisionWorld>>;
}

impl<F: Fn(&str) -> Option<Arc<CollisionWorld>>> CollisionSource for F {
    fn collision(&self, folder: &str) -> Option<Arc<CollisionWorld>> {
        self(folder)
    }
}

impl CollisionSource for BTreeMap<String, Arc<CollisionWorld>> {
    fn collision(&self, folder: &str) -> Option<Arc<CollisionWorld>> {
        self.get(folder).cloned()
    }
}

/// Why a world was not loaded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SkipReason {
    /// The folder or its `.wld` is missing (`WorldDef.loadable == false`).
    NotLoadable,
    /// The collision chunk set of the folder is missing (fatal for that world, 15 §3.5 step 1).
    NoCollision,
    /// `.wld` size or MPU disagree with the collision world.
    Inconsistent(String),
}

/// What [`WorldCatalog::build`] did (start-up log, `flyff check`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CatalogReport {
    /// The loaded world ids, ascending.
    pub loaded: Vec<WorldId>,
    /// The `World.inc` ids that were not loaded.
    pub skipped: Vec<(WorldId, SkipReason)>,
    /// The folders that were built (shared statics), ascending.
    pub folders: Vec<String>,
    /// Non-fatal findings (a missing or damaged `.dyo`, a size mismatch that was tolerated).
    pub warnings: Vec<String>,
    /// Approximate heap bytes of the statics (regions, respawn lines, `.dyo` records; the collision worlds
    /// are shared with `ServerDerived` and not counted).
    pub static_bytes: usize,
}

/// All loadable worlds of the data set with their shared statics (15 §3.5 step 1).
#[derive(Clone, Debug, Default)]
pub struct WorldCatalog {
    worlds: BTreeMap<WorldId, WorldEntry>,
    folders: BTreeMap<String, Arc<WorldStatic>>,
}

impl WorldCatalog {
    /// An empty catalog (no world at all: the M1 behaviour).
    pub fn empty() -> WorldCatalog {
        WorldCatalog::default()
    }

    /// Builds the catalog: one [`WorldStatic`] per folder of a loadable world (in ascending first world id),
    /// then one entry per loadable world id. A folder without a collision world, or whose `.wld` disagrees
    /// with it in size or MPU, is skipped with a [`SkipReason`]; a missing or unreadable `.dyo` is a warning
    /// (the world loads without NPCs).
    pub fn build(
        gd: &GameData,
        vfs: &Vfs,
        collision: &dyn CollisionSource,
    ) -> (WorldCatalog, CatalogReport) {
        let mut cat = WorldCatalog::empty();
        let mut report = CatalogReport::default();
        let mut failed: BTreeMap<String, SkipReason> = BTreeMap::new();
        for def in gd.worlds.worlds.values() {
            if !def.loadable {
                report
                    .skipped
                    .push((WorldId(def.id), SkipReason::NotLoadable));
                continue;
            }
            let key = flyff_data::worlds::WorldRegistry::folder_key(&def.folder);
            if let Some(reason) = failed.get(&key) {
                report.skipped.push((WorldId(def.id), reason.clone()));
                continue;
            }
            if !cat.folders.contains_key(&key) {
                match build_static(gd, vfs, collision, &key, &mut report) {
                    Ok(s) => {
                        report.folders.push(key.clone());
                        cat.folders.insert(key.clone(), Arc::new(s));
                    }
                    Err(reason) => {
                        report.skipped.push((WorldId(def.id), reason.clone()));
                        failed.insert(key, reason);
                        continue;
                    }
                }
            }
            if let Some(statics) = cat.folders.get(&key) {
                cat.worlds.insert(
                    WorldId(def.id),
                    WorldEntry {
                        id: WorldId(def.id),
                        title: def.title.clone(),
                        define_name: def.define_name.clone(),
                        statics: Arc::clone(statics),
                    },
                );
                report.loaded.push(WorldId(def.id));
            }
        }
        report.static_bytes = cat.folders.values().map(|s| static_bytes(s)).sum();
        (cat, report)
    }

    /// A catalog from hand-made statics (tests, tools): `ids` all use `statics`.
    pub fn from_statics(
        entries: impl IntoIterator<Item = (WorldId, String, Arc<WorldStatic>)>,
    ) -> Self {
        let mut cat = WorldCatalog::empty();
        for (id, title, statics) in entries {
            cat.folders
                .entry(statics.folder.clone())
                .or_insert_with(|| Arc::clone(&statics));
            cat.worlds.insert(
                id,
                WorldEntry {
                    id,
                    title,
                    define_name: id.0.to_string(),
                    statics,
                },
            );
        }
        cat
    }

    /// The loaded world ids, ascending.
    pub fn world_ids(&self) -> impl Iterator<Item = WorldId> + '_ {
        self.worlds.keys().copied()
    }

    /// The entries in ascending id order.
    pub fn entries(&self) -> impl Iterator<Item = &WorldEntry> {
        self.worlds.values()
    }

    /// The entry of a loaded world.
    pub fn get(&self, id: WorldId) -> Option<&WorldEntry> {
        self.worlds.get(&id)
    }

    /// The statics of a folder key.
    pub fn folder(&self, key: &str) -> Option<&Arc<WorldStatic>> {
        self.folders.get(key)
    }

    /// The number of loaded world ids.
    pub fn len(&self) -> usize {
        self.worlds.len()
    }

    /// `true` when no world is loaded.
    pub fn is_empty(&self) -> bool {
        self.worlds.is_empty()
    }

    /// The number of distinct folders (shared statics).
    pub fn folder_count(&self) -> usize {
        self.folders.len()
    }
}

/// Approximate heap size of a [`WorldStatic`] without its collision world.
fn static_bytes(s: &WorldStatic) -> usize {
    let regions = (s.regions.len() + s.structures.len()) * size_of::<RegionDef>();
    let respawn = s.respawn_defs.len() * size_of::<RespawnDef>();
    let dyo = s.dyo.records.len() * size_of::<flyff_formats::objects::ObjRecord>();
    regions + respawn + dyo
}

fn build_static(
    gd: &GameData,
    vfs: &Vfs,
    collision: &dyn CollisionSource,
    key: &str,
    report: &mut CatalogReport,
) -> Result<WorldStatic, SkipReason> {
    let folder = gd.worlds.folders.get(key).ok_or(SkipReason::NotLoadable)?;
    let coll = collision.collision(key).ok_or(SkipReason::NoCollision)?;
    let settings = folder.settings.clone();
    let mpu = if matches!(settings.mpu, 1 | 2 | 4) {
        settings.mpu
    } else {
        4
    };
    let lands = settings.size;
    if lands.0 == 0 || lands.1 == 0 {
        return Err(SkipReason::Inconsistent(format!(
            "{key}: the .wld has no size ({} x {})",
            lands.0, lands.1
        )));
    }
    if (coll.lands_w, coll.lands_h, coll.mpu) != (lands.0, lands.1, mpu) {
        return Err(SkipReason::Inconsistent(format!(
            "{key}: .wld {}x{} lands MPU {} but the collision world is {}x{} MPU {}",
            lands.0, lands.1, mpu, coll.lands_w, coll.lands_h, coll.mpu
        )));
    }
    let mut regions = Vec::new();
    let mut structures = Vec::new();
    for r in &folder.regions {
        match r.index {
            RI_TRIGGER | RI_ATTRIBUTE | RI_PLACE => regions.push(r.clone()),
            RI_STRUCTURE => structures.push(r.clone()),
            // RI_BEGIN (character creation) and RI_REVIVAL (the global RevivalIndex) are not stored here
            _ => {}
        }
    }
    let dyo = load_dyo(vfs, &folder.name, key, report);
    Ok(WorldStatic {
        folder: key.to_owned(),
        mpu,
        lands,
        size_units: (MAP_SIZE * lands.0, MAP_SIZE * lands.1),
        collision: coll,
        regions,
        structures,
        respawn_defs: folder.respawns.clone(),
        dyo,
        settings,
    })
}

/// Reads and parses `World/<folder>/<folder>.dyo` through the VFS; a missing file or a parse error is a
/// warning and an empty list (15 §3.2: no file I/O after start-up).
fn load_dyo(vfs: &Vfs, name: &str, key: &str, report: &mut CatalogReport) -> DyoFile {
    let empty = DyoFile {
        records: Vec::new(),
        terminator: flyff_formats::objects::END_OF_LIST,
    };
    let path = format!("World/{name}/{name}.dyo");
    if !vfs.exists(&path) {
        report
            .warnings
            .push(format!("{key}: no .dyo ({path}); the world has no NPCs"));
        return empty;
    }
    let bytes = match vfs.read(&path) {
        Ok(b) => b,
        Err(e) => {
            report.warnings.push(format!("{key}: {path}: {e}"));
            return empty;
        }
    };
    match flyff_formats::dyo::parse(&bytes) {
        Ok(parsed) => {
            for w in &parsed.warnings {
                report.warnings.push(format!("{key}: {path}: {w:?}"));
            }
            parsed.value
        }
        Err(e) => {
            report.warnings.push(format!("{key}: {path}: {e}"));
            empty
        }
    }
}

/// Check `C-W01` (15 §3.1): the loaded world ids are exactly [`REFERENCE_WORLD_IDS`]. `Ok(())` or the
/// difference as text. Meant for `flyff check --expect-reference` (the 14 §9.3 `extra_checks` hook of the
/// data crate is not wired yet, so the server calls this function directly).
pub fn check_c_w01(loaded: impl IntoIterator<Item = WorldId>) -> Result<(), String> {
    let got: Vec<u32> = loaded.into_iter().map(|w| w.0).collect();
    if got == REFERENCE_WORLD_IDS {
        return Ok(());
    }
    let missing: Vec<u32> = REFERENCE_WORLD_IDS
        .iter()
        .copied()
        .filter(|id| !got.contains(id))
        .collect();
    let extra: Vec<u32> = got
        .iter()
        .copied()
        .filter(|id| !REFERENCE_WORLD_IDS.contains(id))
        .collect();
    Err(format!(
        "C-W01: loaded world ids differ from the reference set: missing {missing:?}, unexpected {extra:?}"
    ))
}
