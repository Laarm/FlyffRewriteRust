//! Glue of the world loaders: `World.inc` (L12) and the per-world text data (L80; Plan/14 §6.9, §5.1 P5;
//! ref/19 §4-§12).
//!
//! L80 runs for every `World.inc` entry in ascending id order:
//!
//! * the folder (`World/<Folder>/`) is looked at once per distinct folder (lower-cased): `.wld`, `.rgn`,
//!   `.wld.cnt`, `.pat` (and the `.24l` light table when the `.wld` says `use24light`), parsed by the
//!   `flyff-formats` text parsers and stored in `WorldRegistry::folders`;
//! * a folder that does not exist is Warning `WorldFolderMissing`, one without a `.wld` is Info
//!   `WorldFolderMissing`; neither is loadable;
//! * the revival regions (`RI_REVIVAL`/`RI_PLACE`) of the folder's `.rgn` are collected once per world
//!   **id** (`LoadRevivalPos` ran per id), so a folder shared by 16 ids contributes 16 copies.
//!
//! The folder files are optional (`Need::Silent`): a world without `.rgn` simply has no regions.

use crate::load::ctx::{LoadCtx, Need};
use crate::world_list::parse_world_inc;
use crate::worlds::{
    ContinentDef, PatrolPath, RegionDef, RespawnDef, WldSettings, WorldFolderData, WorldRegistry,
    collect_revival,
};
use flyff_formats as ff;
use flyff_script::{DiagCode, Severity};
use std::collections::BTreeMap;

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// `g_WorldMng.LoadScript(path)` (L12): the world list with the titles. Entries of a second `world` key
/// replace earlier ones with the same id.
pub fn load_world_list(cx: &mut LoadCtx<'_>, path: &str) {
    let Some(text) = cx.read_text(path, Need::Required) else {
        return;
    };
    let sctx = cx.script_ctx();
    let file = cx.file_arc(path);
    let defs = parse_world_inc(&text, &file, &sctx, &mut cx.diags);
    cx.gd.worlds.worlds.extend(defs);
    cx.gd.meta.counts.world_defs = count(cx.gd.worlds.worlds.len());
}

/// The parsed text files of one folder.
struct FolderText {
    settings: WldSettings,
    regions: Vec<RegionDef>,
    respawns: Vec<RespawnDef>,
    continents: Vec<ContinentDef>,
    patrols: Vec<PatrolPath>,
}

/// What L80 knows about a folder after looking at it.
enum Folder {
    /// The directory does not exist.
    Missing,
    /// The directory exists but has no `.wld`: not loadable; its `.rgn` regions still feed the revival scan.
    NoWld(Vec<RegionDef>),
    /// Loaded into `WorldRegistry::folders`.
    Loaded,
}

fn load_folder(cx: &mut LoadCtx<'_>, folder: &str) -> Folder {
    let dir = format!("World/{folder}");
    if !cx.vfs.is_dir(&dir) {
        cx.report(
            Severity::Warning,
            DiagCode::WorldFolderMissing,
            Some(&dir),
            0,
            format!("world folder {dir} does not exist; the world is not loadable"),
        );
        return Folder::Missing;
    }
    let base = format!("{dir}/{folder}");
    let sctx = cx.script_ctx();

    // .rgn: regions and respawns (also needed for the revival scan of a folder without .wld)
    let (regions, respawns) = match cx.read_text(&format!("{base}.rgn"), Need::Silent) {
        Some(text) => {
            let key = cx.file_arc(&format!("{base}.rgn"));
            let f = ff::parse_rgn(&text, &key, &sctx, &mut cx.diags);
            (
                f.regions
                    .into_iter()
                    .map(RegionDef::from)
                    .collect::<Vec<_>>(),
                f.respawns
                    .into_iter()
                    .map(RespawnDef::from)
                    .collect::<Vec<_>>(),
            )
        }
        None => (Vec::new(), Vec::new()),
    };

    let wld_path = format!("{base}.wld");
    let Some(wld_text) = cx.read_text(&wld_path, Need::Silent) else {
        cx.report(
            Severity::Info,
            DiagCode::WorldFolderMissing,
            Some(&dir),
            0,
            format!("world folder {dir} has no .wld file; the world is not loadable"),
        );
        return Folder::NoWld(regions);
    };
    let key = cx.file_arc(&wld_path);
    let mut settings = WldSettings::from(ff::parse_wld(&wld_text, &key, &sctx, &mut cx.diags));
    if settings.use24light {
        // `InitWorldEnvir(<path>.wld)` reads `<path>.wld.24l`
        let p = format!("{base}.wld.24l");
        if let Some(t) = cx.read_text(&p, Need::Warn) {
            let k = cx.file_arc(&p);
            settings.light24 = Some(ff::parse_24l(&t, &k, &sctx, &mut cx.diags));
        }
    }

    let continents = match cx.read_text(&format!("{base}.wld.cnt"), Need::Silent) {
        Some(t) => {
            let k = cx.file_arc(&format!("{base}.wld.cnt"));
            ff::parse_cnt(&t, &k, &sctx, &mut cx.diags)
                .continents
                .into_iter()
                .map(ContinentDef::from)
                .collect()
        }
        None => Vec::new(),
    };
    let patrols = match cx.read_text(&format!("{base}.pat"), Need::Silent) {
        Some(t) => {
            let k = cx.file_arc(&format!("{base}.pat"));
            ff::parse_pat(&t, &k, &sctx, &mut cx.diags)
                .into_iter()
                .map(PatrolPath::from)
                .collect()
        }
        None => Vec::new(),
    };
    let text = FolderText {
        settings,
        regions,
        respawns,
        continents,
        patrols,
    };
    let c = &mut cx.gd.meta.counts;
    c.regions = c.regions.saturating_add(count(text.regions.len()));
    c.respawns = c.respawns.saturating_add(count(text.respawns.len()));
    cx.gd.worlds.folders.insert(
        WorldRegistry::folder_key(folder),
        WorldFolderData {
            name: folder.to_owned(),
            world_ids: Vec::new(),
            settings: text.settings,
            regions: text.regions,
            respawns: text.respawns,
            continents: text.continents,
            patrols: text.patrols,
        },
    );
    Folder::Loaded
}

/// L80: the text data of every world folder, the `loadable` flags and the revival tables.
pub fn load_world_data(cx: &mut LoadCtx<'_>) {
    let defs: Vec<(u32, String)> = cx
        .gd
        .worlds
        .worlds
        .values()
        .map(|w| (w.id, w.folder.clone()))
        .collect();
    let mut seen: BTreeMap<String, Folder> = BTreeMap::new();
    for (id, folder) in defs {
        let key = WorldRegistry::folder_key(&folder);
        if !seen.contains_key(&key) {
            let outcome = load_folder(cx, &folder);
            seen.insert(key.clone(), outcome);
        }
        let Some(outcome) = seen.get(&key) else {
            continue;
        };
        match outcome {
            Folder::Missing => {}
            Folder::NoWld(regions) => collect_revival(id, regions, &mut cx.gd.worlds.revival),
            Folder::Loaded => {
                let worlds = &mut cx.gd.worlds;
                if let Some(f) = worlds.folders.get_mut(&key) {
                    f.world_ids.push(id);
                    collect_revival(id, &f.regions, &mut worlds.revival);
                }
                if let Some(w) = worlds.worlds.get_mut(&id) {
                    w.loadable = true;
                }
            }
        }
    }
    let c = &mut cx.gd.meta.counts;
    c.worlds_loadable = count(cx.gd.worlds.worlds.values().filter(|w| w.loadable).count());
    c.world_folders = count(cx.gd.worlds.folders.len());
}
