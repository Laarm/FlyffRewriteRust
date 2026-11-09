//! `ClientData`: the client-only sources (`resdata.inc`, `sound.inc`, `default.bgm`, `Terrain.inc`, the
//! texture atlases, the help family, panel texts, `textEmotion.txt`, `filter_ENG.inc`, ...), consumed by
//! the bundle exporter and by the server's chat path (Plan/14 §6.19).
//!
//! Two fields are also used server-side: `chat_filter` and `text_emotions` (the game builds its
//! `ChatFilter` and `EmoteTable` from them, Plan/23 §6.9-§6.10). No other field is read by game logic.
//! The loaders are in [`crate::client`]; the glue of phase P7 is `load::steps::client`.

use crate::client::{Applet, Atlas, HelpTexts, MapMonsterInfo, RainbowMarker, TerrainTex};
use crate::types::{MusicId, SoundId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The client-only data (empty when `DataOptions::load_client_data` is false).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ClientData {
    /// `resdata.inc`: 365 applets / 2,809 controls in the reference data (ref/22 §7).
    pub layouts: Vec<Applet>,
    /// `Client/sound.inc`: `SND_`/`VOC_` id to the file name as written (`""` = silent); 1,665 ids.
    pub sounds: BTreeMap<SoundId, String>,
    /// `Music/default.bgm`: `BGM_` id to `"music\\file.ogg"` as written; 42 entries.
    pub music: BTreeMap<MusicId, String>,
    /// `Terrain.inc`: the texture rows in file order with their group paths (ref/19 §14).
    pub terrain: Vec<TerrainTex>,
    /// The texture atlases of `Theme/`, `Icon/` and `SFX/` (27 / 9 / 3), by canonical VFS path of the `.inc`.
    pub atlases: BTreeMap<String, Atlas>,
    /// The help family of `Client/`.
    pub help: HelpTexts,
    /// The text panels of `Client/*.inc` by lower-case file stem (incl. `notice_eng`, the login notice).
    pub panels: BTreeMap<String, String>,
    /// `textEmotion.txt`: emoticon index to command (29).
    pub text_emotions: BTreeMap<u32, String>,
    /// `filter_ENG.inc`: word and replacement as written, file order (330 pairs).
    pub chat_filter: Vec<(String, String)>,
    /// `QuestDestination.txt.txt` (empty in the reference data).
    pub quest_destinations: BTreeMap<u32, String>,
    /// `PatrolDestination.txt.txt`: id to name.
    pub patrol_destinations: BTreeMap<u32, String>,
    /// The monster records of `texMapMonster_*.inc`, maps 1..=7 in map order.
    pub map_monsters: Vec<MapMonsterInfo>,
    /// The rainbow race NPC markers of `texMapRainbow_NPC.inc`.
    pub rainbow_markers: Vec<RainbowMarker>,
}

impl ClientData {
    /// The login notice (`Client/notice_ENG.inc`), `""` when the file is missing.
    pub fn notice(&self) -> &str {
        self.panels
            .get(crate::client::panels::NOTICE_KEY)
            .map_or("", String::as_str)
    }
}
