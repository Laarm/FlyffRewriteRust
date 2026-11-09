//! The world registry: `World.inc` plus the per-folder text data (Plan/14 §6.9; ref/19 §4-§12).
//!
//! [`WorldRegistry`] holds the world definitions by id, one [`WorldFolderData`] per distinct folder (ids
//! that share a folder share the data: `WI_DUNGEON_SECRET_1..16` use `WdHeaven06_1`) and the revival tables
//! of `CWorldMng::LoadRevivalPos`.
//!
//! # Types
//!
//! The settings, region, respawn, continent and patrol types mirror the output of the text parsers of
//! `flyff-formats` (`world_text`, Plan/30 §9) field for field. They are separate types because `GameData`
//! is `serde` and always compiled (wasm-safe), while `flyff-formats` is a dependency of the loaders only
//! (feature `load`) and has no `serde` derives. `From` impls (feature `load`) convert the parser output.
//!
//! # Semantics left to the consumers (documented where the plan puts them)
//!
//! * Region placement: `RI_BEGIN`/`RI_REVIVAL` are not stored in the world, `RI_STRUCTURE` is separate
//!   (doc 15). `regions` keeps everything in file order.
//! * Respawn values are raw: the `(short)` cast of the time, the x0.5/x0.7 factors, the `__INCREASED_SPAWN`
//!   multiplier and the cap of 80 belong to doc 15.
//! * `.wld.cnt`: all blocks are kept; the server uses only `use_real_data` blocks, the client only
//!   `use_envir` blocks. `.pat`: the closing segment from the last to the first point is the consumer's.

use crate::types::{MusicId, WorldId};
use crate::world_list::WorldDef;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// `RI_REVIVAL` and `RI_PLACE` are the region indices `LoadRevivalPos` collects.
pub use flyff_common::defs::obj::{
    RI_ATTRIBUTE, RI_BEGIN, RI_PLACE, RI_REVIVAL, RI_STRUCTURE, RI_TRIGGER,
};

/// All worlds of `World.inc` and their text data.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WorldRegistry {
    /// `CWorldMng::m_aWorld`: every `World.inc` entry by id.
    pub worlds: BTreeMap<WorldId, WorldDef>,
    /// One entry per distinct folder that exists and has a `.wld`, keyed by the lower-cased folder name.
    pub folders: BTreeMap<String, WorldFolderData>,
    /// `CWorldMng::LoadRevivalPos`: the revival regions of every world.
    pub revival: RevivalTables,
}

impl WorldRegistry {
    /// The lower-case key of a folder name (the key of [`Self::folders`]).
    pub fn folder_key(folder: &str) -> String {
        folder.to_ascii_lowercase()
    }

    /// The definition of world `id`.
    pub fn world(&self, id: WorldId) -> Option<&WorldDef> {
        self.worlds.get(&id)
    }

    /// The folder data of world `id`; `None` when the id is unknown or its folder is not loadable.
    pub fn folder_data(&self, id: WorldId) -> Option<&WorldFolderData> {
        let def = self.worlds.get(&id)?;
        self.folders.get(&Self::folder_key(&def.folder))
    }

    /// The ids of the loadable worlds in ascending order (the set the world server loads).
    pub fn loadable_ids(&self) -> impl Iterator<Item = WorldId> + '_ {
        self.worlds.values().filter(|w| w.loadable).map(|w| w.id)
    }

    /// The display name of world `id` (`SetTitle`).
    pub fn title(&self, id: WorldId) -> Option<&str> {
        self.worlds.get(&id).map(|w| w.title.as_str())
    }
}

/// The text data of one world folder (shared by every world id that uses the folder).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WorldFolderData {
    /// The folder name as first written in `World.inc`.
    pub name: String,
    /// The world ids that use this folder, ascending.
    pub world_ids: Vec<WorldId>,
    /// `<W>.wld`.
    pub settings: WldSettings,
    /// `<W>.rgn` `region`/`region2`/`region3`, file order.
    pub regions: Vec<RegionDef>,
    /// `<W>.rgn` `respawn`..`respawn7`, file order (raw values).
    pub respawns: Vec<RespawnDef>,
    /// `<W>.wld.cnt`: `WdMadrigal` has the real polygons, the others are stubs (no blocks).
    pub continents: Vec<ContinentDef>,
    /// `<W>.pat` (all 15 files of this data set are empty).
    pub patrols: Vec<PatrolPath>,
}

/// `CWorldMng::m_aRevivalPos` / `m_aRevivalRgn` (`LoadRevivalPos`, src:_Common/worldmng.cpp:381-544).
///
/// A region with index `RI_REVIVAL` or `RI_PLACE` goes to `points` when its `target_key` is false (a revival
/// *point*) and to `areas` otherwise (a revival *area* whose `key` names the point). The scan runs once per
/// world **id** in ascending order, so a folder shared by several ids appears once per id.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RevivalTables {
    /// Revival points with the id of the world they belong to.
    pub points: Vec<(WorldId, RegionDef)>,
    /// Revival areas with the id of the world they belong to.
    pub areas: Vec<(WorldId, RegionDef)>,
}

/// The settings of a `.wld` (ref/19 §5). Absent keys keep the `CWorld` constructor defaults.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WldSettings {
    /// `size w , h`: land counts in X and Z.
    pub size: (u32, u32),
    /// `indoor`.
    pub indoor: bool,
    /// `ambient` (hex colour as written; default `0xFF808080`).
    pub ambient: u32,
    /// `fly` (default **true**).
    pub fly: bool,
    /// `bgm`.
    pub bgm: MusicId,
    /// `pkmode`: the world's default `RA_*` region attribute flags.
    pub pk_mode: u32,
    /// `diffuse`.
    pub diffuse: u32,
    /// `lightDir`.
    pub light_dir: [f32; 3],
    /// `fogSetting start end density` (default 70, 400, 0).
    pub fog: (f32, f32, f32),
    /// `MPU` (default 4).
    pub mpu: u32,
    /// `revival <world> "<key>"`: the default revival world (0 = none).
    pub revival_world: WorldId,
    /// The revival key.
    pub revival_key: String,
    /// `sky`: day, evening, night texture; `None` = `default`.
    pub sky: [Option<String>; 3],
    /// `cloud`: day, evening, night texture; `None` = `default`.
    pub cloud: [Option<String>; 3],
    /// `sun`.
    pub sun: Option<String>,
    /// `moon`.
    pub moon: Option<String>,
    /// `true` when the file has a `moon` key (the original stored its value into the sun slot).
    pub moon_into_sun_bug: bool,
    /// `use24light`.
    pub use24light: bool,
    /// The `<W>.wld.24l` table (24 rows `r1 g1 b1 r2 g2 b2`), read by the loader when `use24light` is set.
    pub light24: Option<Vec<[f32; 6]>>,
}

impl Default for WldSettings {
    fn default() -> Self {
        Self {
            size: (0, 0),
            indoor: false,
            ambient: 0xFF80_8080,
            fly: true,
            bgm: 0,
            pk_mode: 0,
            diffuse: 0,
            light_dir: [0.0; 3],
            fog: (70.0, 400.0, 0.0),
            mpu: 4,
            revival_world: 0,
            revival_key: String::new(),
            sky: [None, None, None],
            cloud: [None, None, None],
            sun: None,
            moon: None,
            moon_into_sun_bug: false,
            use24light: false,
            light24: None,
        }
    }
}

/// The entry requirements of a `region3` record (fields 21-31; ref/19 §9.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegionReq {
    /// `m_uItemId` (`u32::MAX` = none).
    pub item: u32,
    /// `m_uiItemCount`.
    pub item_count: u32,
    /// `m_uiMinLevel`.
    pub min_level: u32,
    /// `m_uiMaxLevel`.
    pub max_level: u32,
    /// `m_iQuest`.
    pub quest: i32,
    /// `m_iQuestFlag`.
    pub quest_flag: i32,
    /// `m_iJob`.
    pub job: i32,
    /// `m_iGender`.
    pub gender: i32,
    /// `m_bCheckParty`.
    pub check_party: bool,
    /// `m_bCheckGuild`.
    pub check_guild: bool,
    /// `m_bChaoKey`: the revival point used by chaotic players.
    pub chao_key: bool,
}

/// A `region`/`region2`/`region3` record (`REGIONELEM`, ref/19 §9.1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RegionDef {
    /// 1..=3 (`region`, `region2`, `region3`).
    pub version: u8,
    /// `dwType` (6 = `OT_REGION`).
    pub obj_type: u32,
    /// `dwIndex`: `RI_TRIGGER` 10, `RI_ATTRIBUTE` 11, `RI_BEGIN` 12, `RI_REVIVAL` 13, `RI_STRUCTURE` 14,
    /// `RI_PLACE` 15.
    pub index: u32,
    /// `vPos` in world units (a revival point's spawn position).
    pub pos: [f32; 3],
    /// `m_dwAttribute`: `RA_*` flags.
    pub attribute: u32,
    /// `m_dwIdMusic`.
    pub music: MusicId,
    /// `m_bDirectMusic`.
    pub direct_music: bool,
    /// `m_szScript`.
    pub script: String,
    /// `m_szSound`.
    pub sound: String,
    /// `m_dwIdTeleWorld`: the portal destination world (0 = not a portal).
    pub tele_world: WorldId,
    /// `m_vTeleWorld`: the portal destination position.
    pub tele_pos: [f32; 3],
    /// `m_rect` as `[left, top, right, bottom]` = `[xmin, zmin, xmax, zmax]`.
    pub rect: [i32; 4],
    /// `m_szKey`: the revival or place key.
    pub key: String,
    /// `m_bTargetKey`.
    pub target_key: bool,
    /// The `region3` entry requirements (`None` for `region`/`region2`: use [`RegionReq::LEGACY_DEFAULT`]).
    pub req: Option<RegionReq>,
    /// `title` text (resolved through the string table).
    pub title: Option<String>,
    /// `desc` text (resolved through the string table).
    pub desc: Option<String>,
    /// The `IDS_*` key the title was written as, when it was one.
    pub title_key: Option<String>,
    /// The `IDS_*` key the description was written as, when it was one.
    pub desc_key: Option<String>,
}

impl RegionReq {
    /// The requirements of a region without fields 21-31: everything "none" (`LoadRevivalPos` and
    /// `ReadRegion` initialise the unread fields to 0xFFFFFFFF / FALSE).
    pub const LEGACY_DEFAULT: RegionReq = RegionReq {
        item: u32::MAX,
        item_count: u32::MAX,
        min_level: u32::MAX,
        max_level: u32::MAX,
        quest: -1,
        quest_flag: -1,
        job: -1,
        gender: -1,
        check_party: false,
        check_guild: false,
        chao_key: false,
    };
}

impl RegionDef {
    /// The entry requirements, with the legacy default for `region`/`region2`.
    pub fn req_or_legacy(&self) -> RegionReq {
        self.req.unwrap_or(RegionReq::LEGACY_DEFAULT)
    }

    /// `true` when `(x, z)` lies inside the rect: `CRect::PtInRect` with `(LONG)` truncated coordinates,
    /// `left <= x < right && top <= z < bottom` (ref/19 §9.2).
    pub fn contains(&self, x: f32, z: f32) -> bool {
        let (xi, zi) = (x as i32, z as i32);
        let [left, top, right, bottom] = self.rect;
        xi >= left && xi < right && zi >= top && zi < bottom
    }
}

/// A respawn record (`CRespawnInfo`, ref/19 §9.4). Values are raw.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RespawnDef {
    /// 1..=7 (`respawn`..`respawn7`).
    pub version: u8,
    /// `dwType`: 5 mover, 2 control, 4 item.
    pub obj_type: u32,
    /// `dwIndex`.
    pub index: u32,
    /// `vPos`.
    pub pos: [f32; 3],
    /// `m_nMaxcb`.
    pub max_count: i32,
    /// `m_uTime` raw.
    pub time_s: u32,
    /// `m_nMaxAttackNum`.
    pub max_attack_num: i32,
    /// Spawn area `[x1, z1, x2, z2]`.
    pub rect: [i32; 4],
    /// `m_nDayMin`, `m_nDayMax`.
    pub day: (u32, u32),
    /// `m_nHourMin`, `m_nHourMax`.
    pub hour: (u32, u32),
    /// `m_nItemMin`, `m_nItemMax`.
    pub item: (u32, u32),
    /// `m_dwAiState`.
    pub ai_state: u32,
    /// `m_fAngle`.
    pub angle: f32,
    /// `m_dwPatrolIndex` (`u32::MAX` = none).
    pub patrol_index: u32,
    /// `m_bPatrolCycle`.
    pub patrol_cycle: bool,
    /// The control marker: 0 none, 1 or 2 = a [`CtrlElemDef`] follows (2 = the extended variant).
    pub ctrl_kind: u8,
    /// The control payload when `ctrl_kind != 0`.
    pub ctrl: Option<Box<CtrlElemDef>>,
}

/// `CCtrlElem`: the payload of a control respawn (ref/19 §9.4; the same record as in a `.dyo`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CtrlElemDef {
    /// `dwSet`.
    pub set: u32,
    /// `dwSetItem`.
    pub set_item: u32,
    /// `dwSetLevel`.
    pub set_level: u32,
    /// `dwSetQuestNum`.
    pub set_quest_num: u32,
    /// `dwSetFlagNum`.
    pub set_flag_num: u32,
    /// `dwSetGender`.
    pub set_gender: u32,
    /// `bSetJob[32]`.
    pub set_job: [u32; 32],
    /// `dwSetEndu`.
    pub set_endu: u32,
    /// `dwMinItemNum`.
    pub min_item_num: u32,
    /// `dwMaxiItemNum`.
    pub max_item_num: u32,
    /// `dwInsideItemKind[4]`.
    pub inside_item_kind: [u32; 4],
    /// `dwInsideItemPer[4]`.
    pub inside_item_per: [u32; 4],
    /// `dwMonResKind[3]`.
    pub mon_res_kind: [u32; 3],
    /// `dwMonResNum[3]`.
    pub mon_res_num: [u32; 3],
    /// `dwMonActAttack[3]`.
    pub mon_act_attack: [u32; 3],
    /// `dwTrapOperType`.
    pub trap_oper_type: u32,
    /// `dwTrapRandomPer`.
    pub trap_random_per: u32,
    /// `dwTrapDelay`.
    pub trap_delay: u32,
    /// `dwTrapKind[3]`.
    pub trap_kind: [u32; 3],
    /// `dwTrapLevel[3]`.
    pub trap_level: [u32; 3],
    /// `strLinkCtrlKey` (empty in a `.rgn`).
    pub link_ctrl_key: String,
    /// `strCtrlKey` (empty in a `.rgn`).
    pub ctrl_key: String,
    /// `dwSetQuestNum1`.
    pub set_quest_num1: u32,
    /// `dwSetFlagNum1`.
    pub set_flag_num1: u32,
    /// `dwSetQuestNum2`.
    pub set_quest_num2: u32,
    /// `dwSetFlagNum2`.
    pub set_flag_num2: u32,
    /// `dwSetItemCount`.
    pub set_item_count: u32,
    /// `dwTeleWorldId`.
    pub tele_world_id: u32,
    /// `dwTeleX` (raw bits as read).
    pub tele_x: u32,
    /// `dwTeleY`.
    pub tele_y: u32,
    /// `dwTeleZ`.
    pub tele_z: u32,
}

/// One `.wld.cnt` continent block (ref/19 §11).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ContinentDef {
    /// `TOWN`.
    pub town: bool,
    /// `C_useEnvir`: the client environment override.
    pub use_envir: bool,
    /// `C_useRealData`: a gameplay continent for the server.
    pub use_real_data: bool,
    /// `C_name`.
    pub name: String,
    /// `C_id` (`ContinentDef.h`).
    pub id: u32,
    /// `C_ambient`.
    pub ambient: Option<[f32; 3]>,
    /// `C_diffuse`.
    pub diffuse: Option<[f32; 3]>,
    /// `C_fogstartend`.
    pub fog: Option<(f32, f32)>,
    /// `C_weather`.
    pub weather: Option<u32>,
    /// `C_sky <use> <file>`.
    pub sky: Option<(bool, String)>,
    /// `C_cloud <use> <file>`.
    pub cloud: Option<(bool, String)>,
    /// `VERTEX x y z` as written (not closed).
    pub vertices: Vec<[f32; 3]>,
}

impl ContinentDef {
    /// The vertices with the first one appended (both original readers closed the polygon).
    pub fn closed_vertices(&self) -> Vec<[f32; 3]> {
        let mut v = self.vertices.clone();
        if let Some(first) = self.vertices.first() {
            v.push(*first);
        }
        v
    }

    /// The closed polygon as the server stored it: `((int)x, (int)z)` per vertex (x87 `(int)` cast).
    pub fn server_points(&self) -> Vec<(i32, i32)> {
        self.closed_vertices()
            .iter()
            .map(|v| {
                (
                    flyff_common::math::ftoi_x86(v[0]),
                    flyff_common::math::ftoi_x86(v[2]),
                )
            })
            .collect()
    }
}

/// One `.pat` patrol path (ref/19 §10).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PatrolPath {
    /// `INDEX n`.
    pub index: u32,
    /// The points in file order.
    pub points: Vec<PatrolPoint>,
}

/// One point of a patrol path.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PatrolPoint {
    /// Position.
    pub pos: [f32; 3],
    /// Direction (only when the point's count was non-zero).
    pub dir: Option<[f32; 3]>,
    /// Motion state.
    pub state: u32,
    /// Delay.
    pub delay: u32,
    /// Length.
    pub len: f32,
    /// The point's count field.
    pub count: u32,
}

#[cfg(feature = "load")]
mod convert {
    use super::{
        ContinentDef, CtrlElemDef, PatrolPath, PatrolPoint, RegionDef, RegionReq, RespawnDef,
        WldSettings,
    };
    use flyff_formats as ff;

    impl From<ff::WldSettings> for WldSettings {
        fn from(w: ff::WldSettings) -> Self {
            Self {
                size: w.size,
                indoor: w.indoor,
                ambient: w.ambient,
                fly: w.fly,
                bgm: w.bgm,
                pk_mode: w.pk_mode,
                diffuse: w.diffuse,
                light_dir: w.light_dir,
                fog: w.fog,
                mpu: w.mpu,
                revival_world: w.revival_world,
                revival_key: w.revival_key,
                sky: w.sky,
                cloud: w.cloud,
                sun: w.sun,
                moon: w.moon,
                moon_into_sun_bug: w.moon_into_sun_bug,
                use24light: w.use24light,
                light24: w.light24,
            }
        }
    }

    impl From<ff::RegionReq> for RegionReq {
        fn from(r: ff::RegionReq) -> Self {
            Self {
                item: r.item,
                item_count: r.item_count,
                min_level: r.min_level,
                max_level: r.max_level,
                quest: r.quest,
                quest_flag: r.quest_flag,
                job: r.job,
                gender: r.gender,
                check_party: r.check_party,
                check_guild: r.check_guild,
                chao_key: r.chao_key,
            }
        }
    }

    impl From<ff::RegionDef> for RegionDef {
        fn from(r: ff::RegionDef) -> Self {
            Self {
                version: r.version,
                obj_type: r.obj_type,
                index: r.index,
                pos: r.pos,
                attribute: r.attribute,
                music: r.music,
                direct_music: r.direct_music,
                script: r.script,
                sound: r.sound,
                tele_world: r.tele_world,
                tele_pos: r.tele_pos,
                rect: r.rect,
                key: r.key,
                target_key: r.target_key,
                req: r.req.map(RegionReq::from),
                title: r.title,
                desc: r.desc,
                title_key: r.title_key,
                desc_key: r.desc_key,
            }
        }
    }

    impl From<ff::CtrlElemDef> for CtrlElemDef {
        fn from(c: ff::CtrlElemDef) -> Self {
            Self {
                set: c.set,
                set_item: c.set_item,
                set_level: c.set_level,
                set_quest_num: c.set_quest_num,
                set_flag_num: c.set_flag_num,
                set_gender: c.set_gender,
                set_job: c.set_job,
                set_endu: c.set_endu,
                min_item_num: c.min_item_num,
                max_item_num: c.max_item_num,
                inside_item_kind: c.inside_item_kind,
                inside_item_per: c.inside_item_per,
                mon_res_kind: c.mon_res_kind,
                mon_res_num: c.mon_res_num,
                mon_act_attack: c.mon_act_attack,
                trap_oper_type: c.trap_oper_type,
                trap_random_per: c.trap_random_per,
                trap_delay: c.trap_delay,
                trap_kind: c.trap_kind,
                trap_level: c.trap_level,
                link_ctrl_key: c.link_ctrl_key.to_string_lossy_ascii(),
                ctrl_key: c.ctrl_key.to_string_lossy_ascii(),
                set_quest_num1: c.set_quest_num1,
                set_flag_num1: c.set_flag_num1,
                set_quest_num2: c.set_quest_num2,
                set_flag_num2: c.set_flag_num2,
                set_item_count: c.set_item_count,
                tele_world_id: c.tele_world_id,
                tele_x: c.tele_x,
                tele_y: c.tele_y,
                tele_z: c.tele_z,
            }
        }
    }

    impl From<ff::RespawnDef> for RespawnDef {
        fn from(r: ff::RespawnDef) -> Self {
            Self {
                version: r.version,
                obj_type: r.obj_type,
                index: r.index,
                pos: r.pos,
                max_count: r.max_count,
                time_s: r.time_s,
                max_attack_num: r.max_attack_num,
                rect: r.rect,
                day: r.day,
                hour: r.hour,
                item: r.item,
                ai_state: r.ai_state,
                angle: r.angle,
                patrol_index: r.patrol_index,
                patrol_cycle: r.patrol_cycle,
                ctrl_kind: r.ctrl_kind,
                ctrl: r.ctrl.map(|c| Box::new(CtrlElemDef::from(*c))),
            }
        }
    }

    impl From<ff::ContinentDef> for ContinentDef {
        fn from(c: ff::ContinentDef) -> Self {
            Self {
                town: c.town,
                use_envir: c.use_envir,
                use_real_data: c.use_real_data,
                name: c.name,
                id: c.id,
                ambient: c.ambient,
                diffuse: c.diffuse,
                fog: c.fog,
                weather: c.weather,
                sky: c.sky,
                cloud: c.cloud,
                vertices: c.vertices,
            }
        }
    }

    impl From<ff::PatrolPoint> for PatrolPoint {
        fn from(p: ff::PatrolPoint) -> Self {
            Self {
                pos: p.pos,
                dir: p.dir,
                state: p.state,
                delay: p.delay,
                len: p.len,
                count: p.count,
            }
        }
    }

    impl From<ff::PatrolPath> for PatrolPath {
        fn from(p: ff::PatrolPath) -> Self {
            Self {
                index: p.index,
                points: p.points.into_iter().map(PatrolPoint::from).collect(),
            }
        }
    }
}

/// Collects the revival regions of one world id (`LoadRevivalPos`): `RI_REVIVAL` and `RI_PLACE` regions go
/// to `points` (no target key) or `areas` (target key), in file order.
pub fn collect_revival(world: WorldId, regions: &[RegionDef], revival: &mut RevivalTables) {
    for r in regions {
        if r.index == RI_REVIVAL || r.index == RI_PLACE {
            if r.target_key {
                revival.areas.push((world, r.clone()));
            } else {
                revival.points.push((world, r.clone()));
            }
        }
    }
}
