//! `.rgn` regions and respawns (Plan/30 §9.3-§9.4; `CWorld::LoadRegion`, `ReadRegion`, `ReadRespawn`,
//! src:_Common/WorldFile.cpp:413-819; ref/19 §9; ref/06 respawn semantics).
//!
//! The file is UTF-16LE text (decoded by the caller). The top level is `get_token` until the end: `region`,
//! `region2`, `region3` start a region record, `respawn` and `respawn2` ... `respawn7` a respawn record, every
//! other token is skipped. The corpus has only `region3` (190) and `respawn7` (1,579).
//!
//! Semantics of the values (placement of `RI_BEGIN`/`RI_REVIVAL`, respawn multipliers, spawn filters) are
//! consumer rules (14 §6.9, 15); the parser returns the raw record values.

use flyff_script::{DiagCode, Diagnostics, Script, ScriptCtx, Severity};

use super::{check_string_len, open_script, report_truncated};
use crate::objects::CtrlElemDef;

/// `RI_TRIGGER`: portal/trigger rectangle (`defineObj.h:1998`).
pub const RI_TRIGGER: u32 = 10;
/// `RI_ATTRIBUTE`: attribute (music, danger) area.
pub const RI_ATTRIBUTE: u32 = 11;
/// `RI_BEGIN`: new-character start point (not stored in the world by the original).
pub const RI_BEGIN: u32 = 12;
/// `RI_REVIVAL`: revival point or area (scanned by the server for its revival tables).
pub const RI_REVIVAL: u32 = 13;
/// `RI_STRUCTURE`: structure region (kept separately by the original).
pub const RI_STRUCTURE: u32 = 14;
/// `RI_PLACE`: named place.
pub const RI_PLACE: u32 = 15;

/// A parsed `.rgn`: both lists keep the file order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RgnFile {
    /// Every `region`/`region2`/`region3` record, including `RI_BEGIN`/`RI_REVIVAL` ones.
    pub regions: Vec<RegionDef>,
    /// Every `respawn` ... `respawn7` record.
    pub respawns: Vec<RespawnDef>,
}

/// The entry requirements of a `region3` record (fields 21-31, Plan/30 §9.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegionReq {
    /// `m_uItemId` (`u32::MAX` = none).
    pub item: u32,
    /// `m_uiItemCount`.
    pub item_count: u32,
    /// `m_uiMinLevel`.
    pub min_level: u32,
    /// `m_uiMaxLevel`.
    pub max_level: u32,
    /// `m_iQuest` (-1 = none).
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
    /// `m_bChaoKey`: a revival point used by chaotic (PK) players.
    pub chao_key: bool,
}

impl RegionReq {
    /// What `region`/`region2` records (no fields 21-31) have: all "none" values (`0xFFFFFFFF` for the four
    /// unsigned and the four signed fields, flags false).
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

/// A region record (`REGIONELEM`; = 14 §6.9 `RegionDef` plus `obj_type`, `title_key`, `desc_key`).
#[derive(Debug, Clone, PartialEq)]
pub struct RegionDef {
    /// 1 (`region`), 2 (`region2`) or 3 (`region3`).
    pub version: u8,
    /// `dwType`: always 6 (`OT_REGION`) in the data.
    pub obj_type: u32,
    /// `dwIndex`: one of the `RI_*` constants.
    pub index: u32,
    /// `vPos` in world units (not divided by 4); the spawn position of revival points.
    pub pos: [f32; 3],
    /// `m_dwAttribute`: `RA_*` flags.
    pub attribute: u32,
    /// `m_dwIdMusic`.
    pub music: u32,
    /// `m_bDirectMusic` (`== 1`).
    pub direct_music: bool,
    /// `m_szScript`.
    pub script: String,
    /// `m_szSound`.
    pub sound: String,
    /// `m_dwIdTeleWorld`: portal destination world (0 = not a portal).
    pub tele_world: u32,
    /// `m_vTeleWorld`: portal destination position.
    pub tele_pos: [f32; 3],
    /// `m_rect` as `[left, top, right, bottom]` = `[xmin, zmin, xmax, zmax]` in world units.
    pub rect: [i32; 4],
    /// `m_szKey`: revival key / place key.
    pub key: String,
    /// `m_bTargetKey` (`== 1`): the region names a revival point by `key` instead of being one.
    pub target_key: bool,
    /// Entry requirements (`region3` only; use [`RegionDef::req_or_legacy`]).
    pub req: Option<RegionReq>,
    /// Title text, `\n` replaced by CRLF (`region2`, `region3`).
    pub title: Option<String>,
    /// Description text, `\n` replaced by CRLF (all versions; `region` stores only this).
    pub desc: Option<String>,
    /// The `IDS_*` key the title token was resolved from, when it was a string-table key.
    pub title_key: Option<String>,
    /// The `IDS_*` key of the description.
    pub desc_key: Option<String>,
}

impl RegionDef {
    /// The requirements, or [`RegionReq::LEGACY_DEFAULT`] for `region`/`region2` records.
    pub fn req_or_legacy(&self) -> RegionReq {
        self.req.unwrap_or(RegionReq::LEGACY_DEFAULT)
    }
}

/// A respawn record (`CRespawnInfo`; = 14 §6.9 `RespawnDef` plus `ctrl_kind`). Fields that older versions do
/// not store keep the constructor defaults of src:_Common/Respawn.cpp:11-50.
#[derive(Debug, Clone, PartialEq)]
pub struct RespawnDef {
    /// 1..=7 (`respawn` ... `respawn7`).
    pub version: u8,
    /// `dwType`: 5 mover, 2 control, 4 item.
    pub obj_type: u32,
    /// `dwIndex`: mover/control/item id.
    pub index: u32,
    /// `vPos` in world units.
    pub pos: [f32; 3],
    /// `m_nMaxcb` raw (the server multiplies movers by `__INCREASED_SPAWN` and caps at 80: doc 15).
    pub max_count: i32,
    /// `m_uTime` raw `get_number` value (the original's `(short)` cast and the x0.5/x0.7 factors are doc 15).
    pub time_s: u32,
    /// `m_nMaxAttackNum`.
    pub max_attack_num: i32,
    /// Spawn area `[x1, z1, x2, z2]` in world units.
    pub rect: [i32; 4],
    /// `m_nDayMin`, `m_nDayMax` (default (1, 30); `respawn2` and later).
    pub day: (u32, u32),
    /// `m_nHourMin`, `m_nHourMax` (default (1, 24)).
    pub hour: (u32, u32),
    /// `m_nItemMin`, `m_nItemMax` (default (1, 1)).
    pub item: (u32, u32),
    /// `m_dwAiState` (default 2; `respawn4` and later).
    pub ai_state: u32,
    /// `m_fAngle` in degrees (default 0).
    pub angle: f32,
    /// `m_dwPatrolIndex` (default `0xFFFFFFFF` = none; `respawn5` and later).
    pub patrol_index: u32,
    /// `m_bPatrolCycle` (default false; `respawn6` and later).
    pub patrol_cycle: bool,
    /// The control marker: 0 = none, 1 or 2 = a `CCtrlElem` follows (2 = the extended variant). Any other
    /// non-zero marker is read like 1 (as the original does) and stored as 1 with a `RespawnBadControl`
    /// warning.
    pub ctrl_kind: u8,
    /// The control payload when `ctrl_kind != 0`.
    pub ctrl: Option<Box<CtrlElemDef>>,
}

impl RespawnDef {
    fn new(version: u8) -> RespawnDef {
        RespawnDef {
            version,
            obj_type: 0,
            index: 0,
            pos: [0.0; 3],
            max_count: 0,
            time_s: 0,
            max_attack_num: 0,
            rect: [0; 4],
            day: (1, 30),
            hour: (1, 24),
            item: (1, 1),
            ai_state: 2,
            angle: 0.0,
            patrol_index: u32::MAX,
            patrol_cycle: false,
            ctrl_kind: 0,
            ctrl: None,
        }
    }
}

/// What a top-level token starts.
enum Record {
    Region(u8),
    Respawn(u8),
}

fn record_of(token: &str) -> Option<Record> {
    Some(match token {
        "region" => Record::Region(1),
        "region2" => Record::Region(2),
        "region3" => Record::Region(3),
        "respawn" => Record::Respawn(1),
        "respawn2" => Record::Respawn(2),
        "respawn3" => Record::Respawn(3),
        "respawn4" => Record::Respawn(4),
        "respawn5" => Record::Respawn(5),
        "respawn6" => Record::Respawn(6),
        "respawn7" => Record::Respawn(7),
        _ => return None,
    })
}

/// Parses a `.rgn` (Plan/30 §9.3-§9.4). A record cut off by the end of the file is dropped with
/// `WorldTextTruncated`.
pub fn parse_rgn(text: &str, file: &str, ctx: &ScriptCtx, diags: &mut Diagnostics) -> RgnFile {
    let mut out = RgnFile::default();
    let mut s = open_script(text, file, ctx, diags);
    s.get_token();
    while !s.finished() {
        match record_of(s.token()) {
            Some(Record::Region(v)) => {
                let region = read_region(&mut s, v);
                if s.finished() {
                    report_truncated(&mut s, "a region record");
                    break;
                }
                out.regions.push(region);
            }
            Some(Record::Respawn(v)) => {
                let respawn = read_respawn(&mut s, v);
                if s.finished() {
                    report_truncated(&mut s, "a respawn record");
                    break;
                }
                out.respawns.push(respawn);
            }
            None => {}
        }
        s.get_token();
    }
    out
}

fn vec3(s: &mut Script<'_>) -> [f32; 3] {
    [s.get_float(), s.get_float(), s.get_float()]
}

/// One string token (substituted), checked against the original's 31-character buffer.
fn short_string(s: &mut Script<'_>, what: &str) -> String {
    s.get_token();
    let v = s.token().to_owned();
    check_string_len(s, what, &v);
    v
}

/// `{ text }`: the braces are not verified; the text keeps `\n` (backslash, n) -> CRLF and the `IDS_` key it
/// was resolved from.
fn text_block(s: &mut Script<'_>) -> (String, Option<String>) {
    s.get_token(); // {
    let (_, key) = s.get_token_with_key();
    let text = s.token().replace("\\n", "\r\n");
    s.get_token(); // }
    (text, key)
}

/// The `title`/`desc` marker token: the original does not verify it.
fn marker(s: &mut Script<'_>, expected: &str) {
    s.get_token();
    if !s.is(expected) && !s.finished() {
        let found = s.token().to_owned();
        s.report(
            Severity::Info,
            DiagCode::RgnUnexpectedToken,
            format!("expected `{expected}`, found `{found}`; the original continues identically"),
        );
    }
}

/// `read_region(v)`, the exact read order of Plan/30 §9.3.
fn read_region(s: &mut Script<'_>, version: u8) -> RegionDef {
    let obj_type = s.get_number();
    let index = s.get_number();
    let pos = vec3(s);
    let attribute = s.get_number();
    let music = s.get_number();
    let direct_music = s.get_number() == 1;
    let script = short_string(s, "script");
    let sound = short_string(s, "sound");
    let tele_world = s.get_number();
    let tele_pos = vec3(s);
    let rect = [
        s.get_number_i32(),
        s.get_number_i32(),
        s.get_number_i32(),
        s.get_number_i32(),
    ];
    let key = short_string(s, "key");
    let target_key = s.get_number() == 1;
    let req = (version >= 3).then(|| RegionReq {
        item: s.get_number(),
        item_count: s.get_number(),
        min_level: s.get_number(),
        max_level: s.get_number(),
        quest: s.get_number_i32(),
        quest_flag: s.get_number_i32(),
        job: s.get_number_i32(),
        gender: s.get_number_i32(),
        check_party: s.get_number() != 0,
        check_guild: s.get_number() != 0,
        chao_key: s.get_number() != 0,
    });

    let mut region = RegionDef {
        version,
        obj_type,
        index,
        pos,
        attribute,
        music,
        direct_music,
        script,
        sound,
        tele_world,
        tele_pos,
        rect,
        key,
        target_key,
        req,
        title: None,
        desc: None,
        title_key: None,
        desc_key: None,
    };
    if version == 1 {
        // KEEP-QUIRK Q13: `(char)(LONG)GetNumber()`: only the low byte decides, so 256 means "no description".
        let flag = s.get_number();
        if flag & 0xFF != 0 {
            let (text, key) = text_block(s);
            region.desc = Some(text);
            region.desc_key = key;
        }
    } else {
        marker(s, "title");
        if s.get_number() != 0 {
            let (text, key) = text_block(s);
            region.title = Some(text);
            region.title_key = key;
        }
        marker(s, "desc");
        if s.get_number() != 0 {
            let (text, key) = text_block(s);
            region.desc = Some(text);
            region.desc_key = key;
        }
    }
    region
}

/// `read_respawn(v)`, the exact read order of Plan/30 §9.4.
fn read_respawn(s: &mut Script<'_>, version: u8) -> RespawnDef {
    let mut r = RespawnDef::new(version);
    r.obj_type = s.get_number();
    r.index = s.get_number();
    r.pos = vec3(s);
    r.max_count = s.get_number_i32();
    r.time_s = s.get_number();
    r.max_attack_num = s.get_number_i32();
    r.rect = [
        s.get_number_i32(),
        s.get_number_i32(),
        s.get_number_i32(),
        s.get_number_i32(),
    ];
    if version >= 2 {
        r.day = (s.get_number(), s.get_number());
        r.hour = (s.get_number(), s.get_number());
        r.item = (s.get_number(), s.get_number());
    }
    if version >= 4 {
        r.ai_state = s.get_number();
        r.angle = s.get_float();
    }
    if version >= 5 {
        r.patrol_index = s.get_number();
    }
    if version >= 6 {
        r.patrol_cycle = s.get_number() != 0;
    }
    if version >= 3 {
        let marker = s.get_number_i32();
        if marker != 0 {
            if !(1..=2).contains(&marker) {
                s.report(
                    Severity::Warning,
                    DiagCode::RespawnBadControl,
                    format!("control marker {marker} is neither 0, 1 nor 2; it is read like 1"),
                );
            }
            let extended = marker == 2;
            r.ctrl_kind = if extended { 2 } else { 1 };
            r.ctrl = Some(Box::new(read_ctrl(s, version, extended)));
        }
    }
    r
}

/// The `CCtrlElem` payload of a respawn record (Plan/30 §9.4): `extended` is the `nContrl == 2` variant.
fn read_ctrl(s: &mut Script<'_>, version: u8, extended: bool) -> CtrlElemDef {
    let mut c = CtrlElemDef {
        // ZeroMemory + the constructor's defaults; every one of these is overwritten below.
        min_item_num: 1,
        max_item_num: 1,
        ..CtrlElemDef::default()
    };
    c.set = s.get_number();
    c.set_item = s.get_number();
    c.set_item_count = if extended { s.get_number() } else { 1 };
    c.set_level = s.get_number();
    c.set_quest_num = s.get_number();
    c.set_flag_num = s.get_number();
    if extended {
        c.set_quest_num1 = s.get_number();
        c.set_flag_num1 = s.get_number();
        c.set_quest_num2 = s.get_number();
        c.set_flag_num2 = s.get_number();
    }
    c.set_gender = s.get_number();
    let jobs = if version <= 6 { 16 } else { 32 };
    for job in c.set_job.iter_mut().take(jobs) {
        *job = s.get_number();
    }
    c.set_endu = s.get_number();
    c.min_item_num = s.get_number();
    c.max_item_num = s.get_number();
    for v in &mut c.inside_item_kind {
        *v = s.get_number();
    }
    for v in &mut c.inside_item_per {
        *v = s.get_number();
    }
    for v in &mut c.mon_res_kind {
        *v = s.get_number();
    }
    for v in &mut c.mon_res_num {
        *v = s.get_number();
    }
    for v in &mut c.mon_act_attack {
        *v = s.get_number();
    }
    c.trap_oper_type = s.get_number();
    c.trap_random_per = s.get_number();
    c.trap_delay = s.get_number();
    for v in &mut c.trap_kind {
        *v = s.get_number();
    }
    for v in &mut c.trap_level {
        *v = s.get_number();
    }
    if extended {
        c.tele_world_id = s.get_number();
        c.tele_x = s.get_number();
        c.tele_y = s.get_number();
        c.tele_z = s.get_number();
    }
    c
}
