//! The world-map records: the monster information of `Theme/texMapMonster_<Continent>.inc` and the rainbow
//! race NPC markers of `Theme/texMapRainbow_NPC.inc` (`CMonsterInfoPack::LoadScript`,
//! `CRainbowNPCPack::LoadScript`; Plan/14 §6.19, ref/99 §4.5; src:Neuz/WorldMap.cpp:1513-1800).
//!
//! The same files are read twice: once as an atlas (the record tokens are skipped there, see
//! [`crate::client::atlas`]) and once here with `CScript` (so that `MI_`/`II_` names become ids):
//!
//! ```text
//! file   := { 'number' N | 'MULTI_LANGUAGE' | 'texture' STR HEX | 'serialize' F W H X0 Y0 CX CY record{F} | other }*
//! record := (monster file)  CNT MI{CNT} X Y II          (rainbow file)  MAP XT YT XM YM
//! ```
//!
//! `frame` is the running record index of the file (the original pairs record `i` with sprite `i` of the same
//! file's atlas), `size` the `W H` of the enclosing `serialize` line, coordinates are raw (1280x960 reference
//! space; the original scaled them at load time, that scaling moves to the client, Plan/45 §7.7).

use crate::types::{ItemId, MoverId};
use serde::{Deserialize, Serialize};

/// The most monsters one record lists (`m_dwMonsterId[5]`).
pub const MAX_MONSTERS_PER_RECORD: usize = 5;

/// The file of each continent map 1..=7 (`m_strMonScript`, src:Neuz/WorldMap.cpp:214-224).
pub const MONSTER_FILES: [&str; 7] = [
    "Theme/texMapMonster_Darkon12.inc",
    "Theme/texMapMonster_Darkon3.inc",
    "Theme/texMapMonster_Ricis.inc",
    "Theme/texMapMonster_Saint.inc",
    "Theme/texMapMonster_Flaris.inc",
    "Theme/texMapMonster_Harmonin.inc",
    "Theme/texMapMonster_Estia.inc",
];

/// The rainbow race marker file.
pub const RAINBOW_FILE: &str = "Theme/texMapRainbow_NPC.inc";

/// The monster information of one continent map.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapMonsterInfo {
    /// The map number 1..=7 (`CContinent::GetMapNo`).
    pub map: u8,
    /// The canonical VFS path of the `.inc`, which is also the key of its atlas in `ClientData::atlases`.
    pub atlas: String,
    /// The records in file order.
    pub entries: Vec<MapMonsterEntry>,
}

/// One hover rectangle of the monster overlay.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapMonsterEntry {
    /// The running record index in the file: the sprite index in the same file's atlas.
    pub frame: u32,
    /// 1..=5 mover ids; the last one is the giant (drawn red in the tooltip).
    pub movers: Vec<MoverId>,
    /// Left/top in the 1280x960 reference space.
    pub pos: (i32, i32),
    /// Width and height of the enclosing `serialize` line (the hover rectangle is `pos .. pos + size`).
    pub size: (i32, i32),
    /// The drop item shown as "Drop item: `<name>`"; an unknown id shows `TID_GAME_DROP_NONE`.
    pub drop_item: ItemId,
}

/// One rainbow race NPC marker (`__RAINBOW_NPC`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RainbowMarker {
    /// The running record index = the sprite index.
    pub frame: u32,
    /// The continent map number 1..=7 on which `map_pos` applies.
    pub map: u8,
    /// The position on the whole-world map (map 0), 1280x960 reference space.
    pub total_pos: (i32, i32),
    /// The position on continent map `map`, same space.
    pub map_pos: (i32, i32),
    /// Width and height of the enclosing `serialize` line.
    pub size: (i32, i32),
}

#[cfg(feature = "load")]
pub(crate) use parse::{parse_monster_records, parse_rainbow_records};

#[cfg(feature = "load")]
mod parse {
    use super::{MAX_MONSTERS_PER_RECORD, MapMonsterEntry, RainbowMarker};
    use flyff_script::{DiagCode, Diagnostics, ScannerOptions, Script, ScriptCtx, Severity};
    use std::sync::Arc;

    /// The loop shared by both files: `visit(sc, frame, w, h)` reads one record's tokens.
    fn walk<T>(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
        mut record: impl FnMut(&mut Script<'_>, u32, (i32, i32)) -> Option<T>,
    ) -> (Vec<T>, u32) {
        let mut out = Vec::new();
        let mut declared = 0u32;
        let mut frame = 0u32;
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        loop {
            sc.get_token();
            if sc.finished() {
                break;
            }
            match sc.token() {
                "number" => declared = sc.get_number(),
                "serialize" => {
                    let count = sc.get_number();
                    let w = sc.get_number_i32();
                    let h = sc.get_number_i32();
                    for _ in 0..4 {
                        sc.get_number(); // x0 y0 cx cy
                    }
                    for _ in 0..count {
                        if sc.finished() {
                            break;
                        }
                        let r = record(&mut sc, frame, (w, h));
                        if sc.finished() {
                            break; // the record ran into the end of the file: drop it
                        }
                        if let Some(r) = r {
                            out.push(r);
                        }
                        frame += 1;
                    }
                }
                _ => {}
            }
        }
        if declared != 0 && declared != frame {
            sc.report(
                Severity::Warning,
                DiagCode::ValueOutOfRange,
                format!("map info: `number` is {declared} but the file has {frame} records"),
            );
        }
        (out, frame)
    }

    /// `CMonsterInfoPack::LoadScript`: `CNT MI{CNT} X Y II` records.
    ///
    /// `CNT > 5` is Error `CapacityExceeded` (**FIX**: the original wrote past `m_dwMonsterId[5]`): its
    /// tokens are consumed, the record is skipped and the next record keeps its running frame number. A
    /// record count different from `number` is a Warning.
    pub(crate) fn parse_monster_records(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> Vec<MapMonsterEntry> {
        walk(text, file, sctx, diags, |sc, frame, size| {
            let cnt = sc.get_number();
            let mut movers = Vec::new();
            for _ in 0..cnt.min(1024) {
                movers.push(sc.get_number());
            }
            let x = sc.get_number_i32();
            let y = sc.get_number_i32();
            let drop_item = sc.get_number();
            if sc.finished() {
                return None; // truncated: `walk` drops it
            }
            if cnt as usize > MAX_MONSTERS_PER_RECORD {
                let msg = format!(
                    "map monster record {frame}: {cnt} monsters (at most {MAX_MONSTERS_PER_RECORD}); skipped"
                );
                sc.report(Severity::Error, DiagCode::CapacityExceeded, msg);
                return None;
            }
            if cnt == 0 {
                let msg = format!("map monster record {frame} lists no monster; skipped");
                sc.report(Severity::Warning, DiagCode::ValueOutOfRange, msg);
                return None;
            }
            Some(MapMonsterEntry {
                frame,
                movers,
                pos: (x, y),
                size,
                drop_item,
            })
        })
        .0
    }

    /// `CRainbowNPCPack::LoadScript`: `MAP XT YT XM YM` records.
    pub(crate) fn parse_rainbow_records(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> Vec<RainbowMarker> {
        walk(text, file, sctx, diags, |sc, frame, size| {
            let map = sc.get_number();
            let total_pos = (sc.get_number_i32(), sc.get_number_i32());
            let map_pos = (sc.get_number_i32(), sc.get_number_i32());
            Some(RainbowMarker {
                frame,
                map: u8::try_from(map).unwrap_or(u8::MAX),
                total_pos,
                map_pos,
                size,
            })
        })
        .0
    }
}
