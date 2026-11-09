//! Glue of the client-only loaders (P7, L95) and of the name rules (L46): reads the files through
//! `LoadCtx`, runs the pure parsers of [`crate::client`] and [`crate::names`], fills
//! `LoadCtx::client` / `GameData::name_rules` and counts (Plan/14 §5.3, §9.1 C21, C26, C27, C32, C36).

use crate::client::atlas::{TexInfo, canonical_or_lower, parse_atlas};
use crate::client::filter::parse_filter;
use crate::client::help::{
    parse_faq, parse_guide, parse_help_topics, parse_instant_help, parse_tips, parse_tree_help,
    parse_tutorial,
};
use crate::client::map_info::{
    MONSTER_FILES, MapMonsterInfo, RAINBOW_FILE, parse_monster_records, parse_rainbow_records,
};
use crate::client::panels::panel_key;
use crate::client::quest_dest::parse_destinations;
use crate::client::resdata::parse_resdata;
use crate::client::sound::parse_id_file;
use crate::client::terrain::parse_terrain;
use crate::client::text_emotion::parse_text_emotion;
use crate::load::ctx::{LoadCtx, Need};
use crate::names::NameRules;
use crate::names::{parse_allowed_letters, parse_invalid_names};
use flyff_script::{DiagCode, Severity};
use flyff_vfs::Vfs;
use std::collections::BTreeSet;
use std::io::Read;

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// L46: `InvalidName_ENG.inc`, `Letter_ENG.inc` and `Letter1_ENG.inc` (a missing file means no rule).
pub fn load_names(cx: &mut LoadCtx<'_>) {
    let mut rules = NameRules::default();
    if let Some(text) = cx.read_text("InvalidName_ENG.inc", Need::Info) {
        let file = cx.file_arc("InvalidName_ENG.inc");
        rules.invalid_substrings = parse_invalid_names(&text, &file, &mut cx.diags);
    }
    if let Some(text) = cx.read_text("Letter_ENG.inc", Need::Info) {
        let file = cx.file_arc("Letter_ENG.inc");
        rules.allowed_letters = parse_allowed_letters(&text, &file, &mut cx.diags);
    }
    if let Some(text) = cx.read_text("Letter1_ENG.inc", Need::Info) {
        let file = cx.file_arc("Letter1_ENG.inc");
        rules.vendor_allowed_letters = parse_allowed_letters(&text, &file, &mut cx.diags);
    }
    let c = &mut cx.gd.meta.counts;
    c.invalid_names = count(rules.invalid_substrings.len());
    c.allowed_letters = count(rules.allowed_letters.as_ref().map_or(0, BTreeSet::len));
    cx.gd.name_rules = rules;
}

/// The size of an image from the first bytes of its file: BMP and TGA through the `flyff-formats` header
/// parsers, DDS from its fixed header (`dwHeight` at 12, `dwWidth` at 16). `None` for other formats.
fn image_size(path: &str, head: &[u8]) -> Option<(u32, u32)> {
    let ext = path.rsplit('.').next().unwrap_or("");
    match ext {
        "bmp" => {
            let h = flyff_formats::bmp::header(head).ok()?;
            Some((u32::try_from(h.width).ok()?, h.height.unsigned_abs()))
        }
        "tga" => {
            let h = flyff_formats::tga::header(head).ok()?;
            Some((u32::from(h.width), u32::from(h.height)))
        }
        "dds" => {
            if head.get(..4) != Some(b"DDS ".as_slice()) {
                return None;
            }
            let rd = |o: usize| -> Option<u32> {
                let b = head.get(o..o.checked_add(4)?)?;
                Some(u32::from_le_bytes(b.try_into().ok()?))
            };
            Some((rd(16)?, rd(12)?))
        }
        _ => None,
    }
}

/// What the VFS knows about a texture: existence and the header size.
fn texture_info(vfs: &Vfs, path: &str) -> TexInfo {
    if !vfs.exists(path) {
        return TexInfo::default();
    }
    let mut head = Vec::with_capacity(256);
    let size = vfs.open_file(path).ok().and_then(|f| {
        f.take(256).read_to_end(&mut head).ok()?;
        image_size(&canonical_or_lower(path), &head)
    });
    TexInfo { exists: true, size }
}

/// L95: the client-only files, in the order of the plan.
pub fn load_client(cx: &mut LoadCtx<'_>) {
    load_terrain(cx);
    load_help(cx);
    load_filter(cx);
    load_sounds(cx);
    load_music(cx);
    load_layouts(cx);
    load_emotions(cx);
    load_atlases(cx);
    load_map_records(cx);
    load_panels(cx);
    load_destinations(cx);
}

fn load_terrain(cx: &mut LoadCtx<'_>) {
    let path = "Terrain.inc";
    if let Some(text) = cx.read_text(path, Need::Warn) {
        let file = cx.file_arc(path);
        cx.client.terrain = parse_terrain(&text, &file, &mut cx.diags);
        cx.gd.meta.counts.terrain_rows = count(cx.client.terrain.len());
    }
}

fn load_help(cx: &mut LoadCtx<'_>) {
    let sctx = cx.script_ctx();
    macro_rules! read {
        ($path:expr) => {{
            let path: &str = $path;
            cx.read_text(path, Need::Warn)
                .map(|t| (t, cx.file_arc(path)))
        }};
    }
    if let Some((t, f)) = read!("Client/help.inc") {
        cx.client.help.topics = parse_help_topics(&t, &f, &sctx, &mut cx.diags);
    }
    if let Some((t, f)) = read!("Client/treeHelp.inc") {
        cx.client.help.tree = parse_tree_help(&t, &f, &sctx, &mut cx.diags);
    }
    if let Some((t, f)) = read!("Client/faq.inc") {
        cx.client.help.faq = parse_faq(&t, &f, &sctx, &mut cx.diags);
    }
    if let Some((t, f)) = read!("Client/tip.inc") {
        cx.client.help.tips = parse_tips(&t, &f, &sctx, &mut cx.diags);
    }
    if let Some((t, f)) = read!("Client/instantHelp.inc") {
        cx.client.help.instant = parse_instant_help(&t, &f, &sctx, &mut cx.diags);
    }
    if let Some((t, f)) = read!("Client/tutorial.inc") {
        cx.client.help.tutorial = parse_tutorial(&t, &f, &sctx, &mut cx.diags);
    }
    if let Some((t, f)) = read!("Client/Guide.inc") {
        cx.client.help.guide = parse_guide(&t, &f, &sctx, &mut cx.diags);
    }
    cx.gd.meta.counts.help_topics = count(cx.client.help.topics.len());
}

fn load_filter(cx: &mut LoadCtx<'_>) {
    let path = "filter_ENG.inc";
    if let Some(text) = cx.read_text(path, Need::Warn) {
        let file = cx.file_arc(path);
        cx.client.chat_filter = parse_filter(&text, &file, &mut cx.diags);
        cx.gd.meta.counts.filter_pairs = count(cx.client.chat_filter.len());
    }
}

/// `Client/sound.inc` (required): ids to `Sound/<file>`.
fn load_sounds(cx: &mut LoadCtx<'_>) {
    let path = "Client/sound.inc";
    let Some(text) = cx.read_text(path, Need::Required) else {
        return;
    };
    let sctx = cx.script_ctx();
    let file = cx.file_arc(path);
    let table = parse_id_file(&text, &file, &sctx, &mut cx.diags);
    let mut missing = BTreeSet::new();
    let mut missing_ids = 0u32;
    for f in table.files.values() {
        if !f.is_empty() && !cx.vfs.exists(&format!("Sound/{f}")) {
            missing_ids += 1;
            missing.insert(f.to_ascii_lowercase());
        }
    }
    for f in &missing {
        cx.report(
            Severity::Info,
            DiagCode::AssetMissing,
            Some(path),
            0,
            format!("sound file Sound/{f} does not exist"),
        );
    }
    let c = &mut cx.gd.meta.counts;
    c.sound_entries = table.entries;
    c.sound_ids = count(table.files.len());
    c.sound_max_id = table.files.keys().next_back().copied().unwrap_or(0);
    c.sound_empty = table.empty_entries;
    c.sound_missing = missing_ids;
    cx.client.sounds = table.files;
}

/// `Music/default.bgm`: ids to the paths as written (`music\x.ogg`, relative to the client root).
fn load_music(cx: &mut LoadCtx<'_>) {
    let path = "Music/default.bgm";
    let Some(text) = cx.read_text(path, Need::Warn) else {
        return;
    };
    let sctx = cx.script_ctx();
    let file = cx.file_arc(path);
    let table = parse_id_file(&text, &file, &sctx, &mut cx.diags);
    let mut missing = 0u32;
    for f in table.files.values() {
        if !f.is_empty() && !cx.vfs.exists(f) {
            missing += 1;
            cx.report(
                Severity::Info,
                DiagCode::AssetMissing,
                Some(path),
                0,
                format!("music file {f} does not exist"),
            );
        }
    }
    let c = &mut cx.gd.meta.counts;
    c.music_entries = table.entries;
    c.music_missing = missing;
    cx.client.music = table.files;
}

/// `resdata.inc` (required): the window layouts.
fn load_layouts(cx: &mut LoadCtx<'_>) {
    let path = "resdata.inc";
    let Some(text) = cx.read_text(path, Need::Required) else {
        return;
    };
    let sctx = cx.script_ctx();
    let file = cx.file_arc(path);
    let (layouts, stats) = parse_resdata(&text, &file, &sctx, &mut cx.diags);
    // the textures the layouts name (applets and controls), once each: bare file names under Theme/
    let mut textures: BTreeSet<String> = BTreeSet::new();
    for a in &layouts {
        textures.insert(a.texture.clone());
        for c in &a.controls {
            textures.insert(c.texture.clone());
        }
    }
    for t in textures.iter().filter(|t| !t.is_empty()) {
        if !cx.vfs.exists(&format!("Theme/{t}")) {
            cx.report(
                Severity::Info,
                DiagCode::AssetMissing,
                Some(path),
                0,
                format!("window texture Theme/{t} does not exist"),
            );
        }
    }
    let c = &mut cx.gd.meta.counts;
    c.resdata_applets = count(layouts.len());
    c.resdata_controls = count(layouts.iter().map(|a| a.controls.len()).sum());
    c.resdata_empty_blocks = stats.empty_blocks;
    c.resdata_unknown_names = stats.unknown_names;
    cx.client.layouts = layouts;
}

fn load_emotions(cx: &mut LoadCtx<'_>) {
    let path = "textEmotion.txt";
    if let Some(text) = cx.read_text(path, Need::Warn) {
        let sctx = cx.script_ctx();
        let file = cx.file_arc(path);
        cx.client.text_emotions = parse_text_emotion(&text, &file, &sctx, &mut cx.diags);
        cx.gd.meta.counts.text_emotions = count(cx.client.text_emotions.len());
    }
}

/// Every `.inc` of `Theme/`, `Icon/` and `SFX/` is an atlas (27 + 9 + 3 in the reference data).
fn load_atlases(cx: &mut LoadCtx<'_>) {
    let vfs = cx.vfs;
    let mut counts = [0u32; 3];
    for (slot, dir) in ["Theme", "Icon", "SFX"].into_iter().enumerate() {
        for e in vfs.list_dir(dir) {
            let key = e.path.as_str().to_owned();
            if !key.ends_with(".inc") {
                continue;
            }
            let Some(text) = cx.read_text(&key, Need::Warn) else {
                continue;
            };
            let file = cx.file_arc(&key);
            let mut info = |p: &str| texture_info(vfs, p);
            let atlas = parse_atlas(&text, &file, &mut info, &mut cx.diags);
            cx.client.atlases.insert(key, atlas);
            if let Some(c) = counts.get_mut(slot) {
                *c += 1;
            }
        }
    }
    cx.gd.meta.counts.atlases = counts;
}

/// The monster records of the seven continent maps and the rainbow race markers.
fn load_map_records(cx: &mut LoadCtx<'_>) {
    let sctx = cx.script_ctx();
    let mut records = 0usize;
    for (i, path) in MONSTER_FILES.iter().enumerate() {
        let Some(text) = cx.read_text(path, Need::Warn) else {
            continue;
        };
        let file = cx.file_arc(path);
        let entries = parse_monster_records(&text, &file, &sctx, &mut cx.diags);
        records += entries.len();
        cx.client.map_monsters.push(MapMonsterInfo {
            map: u8::try_from(i + 1).unwrap_or(u8::MAX),
            atlas: file.to_string(),
            entries,
        });
    }
    if let Some(text) = cx.read_text(RAINBOW_FILE, Need::Warn) {
        let file = cx.file_arc(RAINBOW_FILE);
        cx.client.rainbow_markers = parse_rainbow_records(&text, &file, &sctx, &mut cx.diags);
    }
    let c = &mut cx.gd.meta.counts;
    c.map_monster_records = count(records);
    c.rainbow_markers = count(cx.client.rainbow_markers.len());
}

/// The text panels: every `Client/*.inc` that no other loader reads.
fn load_panels(cx: &mut LoadCtx<'_>) {
    let vfs = cx.vfs;
    for e in vfs.list_dir("Client") {
        let Some(key) = panel_key(e.path.file_name()) else {
            continue;
        };
        let path = e.path.as_str().to_owned();
        if let Some(text) = cx.read_text(&path, Need::Warn) {
            cx.client.panels.insert(key, text);
        }
    }
    cx.gd.meta.counts.panels = count(cx.client.panels.len());
}

fn load_destinations(cx: &mut LoadCtx<'_>) {
    for (path, quest) in [
        ("QuestDestination.txt.txt", true),
        ("PatrolDestination.txt.txt", false),
    ] {
        if let Some(text) = cx.read_text(path, Need::Info) {
            let file = cx.file_arc(path);
            let map = parse_destinations(&text, &file, &mut cx.diags);
            if quest {
                cx.client.quest_destinations = map;
            } else {
                cx.client.patrol_destinations = map;
            }
        }
    }
}
