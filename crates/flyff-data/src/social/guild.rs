//! `GuildTables` (Plan/14 §6.15): the guild level table from `expTable.inc` (`expCompanyTest`, F11), the
//! guild buffs of `guildbuff.inc` (`CGuildMng::LoadBuffs`, src:_Common/guild.cpp:1139-1157) and the guild
//! siege settings and state script of `GuildCombat.txt` (`CGuildCombat::LoadScript`,
//! src:_Common/eveschool.cpp:2966-3213; ref/14 §7.1).

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// `MAX_GUILD_LEVEL`: the capacity of the original `CGuildTable::m_table`.
pub const MAX_GUILD_LEVEL: usize = 50;

/// One guild level (`GUILD_TABLE_ENTRY`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuildLevelRow {
    /// `dwPxpCount`: PXP needed for the level, already divided by `gameplay.guild_rate` (F11, integer
    /// division).
    pub pxp_count: u32,
    /// `dwPenya`: the fee.
    pub penya: u32,
    /// `nMaxMember`: the member limit.
    pub max_member: u32,
}

/// One guild buff (`GUILD_BUFF_INFO`): the buff item the guild gets when the constraints hold.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GuildBuffDef {
    /// `dwBuffID`: the buff item.
    pub item: u32,
    /// `dwLvConstraint`: the minimum guild level.
    pub min_level: u32,
    /// `nMemCountConstraint`: the member count needed.
    pub member_count: i32,
    /// `nMemOnlineConstraint`: the members that must be online.
    pub member_online: i32,
}

/// `GuildCombatProcess[i].nState` (`eveschool.h:76`): what a row of the `OPEN` / `MAINTENANCE` / `WAR` script
/// does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum GcRowKind {
    /// A row whose first word is none of the seven (the C++ array stays zeroed).
    #[default]
    None,
    /// `ALLMSG` = 1000: a message to everybody.
    AllMsg,
    /// `GUILDMSG`: to the guilds in the siege.
    GuildMsg,
    /// `JOINMSG`: to the bidders.
    JoinMsg,
    /// `WORLDMSG`: to the siege world.
    WorldMsg,
    /// `STATE`: a siege state change (`OPEN_STATE`..`GM_COLSE_STATE`).
    State,
    /// `WARSTATE`: a war sub-state (`NOTENTER_STATE` 100 ...).
    WarState,
    /// `WAIT`: just wait.
    Wait,
}

/// One row of an `OPEN` / `MAINTENANCE` / `WAR` block: `KIND TIME COMMAND PARAM` (`__GuildCombatProcess`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GcProcessRow {
    /// What the row does.
    pub kind: GcRowKind,
    /// `dwTime`: ms to wait after the row.
    pub time_ms: u32,
    /// `dwCommand`: the text id, state or war state.
    pub command: u32,
    /// `dwParam`: the next-state hint broadcast to clients (0 = none).
    pub param: u32,
}

/// One `AUTO_OPEN_IDC` line: the siege opens on `day` (1 = Sunday .. 7 = Saturday) at `hour:minute`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GcAutoOpen {
    /// `1..=7`.
    pub day: i32,
    /// `0..=24`.
    pub hour: i32,
    /// `0..=59`.
    pub minute: i32,
}

/// One `GCSENDITEM` line: the reward for a win streak.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GcSendItem {
    /// The win count.
    pub win_count: i32,
    /// The item.
    pub item: u32,
    /// The count.
    pub count: i32,
}

/// `GuildCombat.txt`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuildCombatSettings {
    /// The scalar keys (`JOINPENYA`, `GUILDLEVEL`, `MINJOINGUILDSIZE`, `MAXJOINGUILDSIZE`, `MAXPLAYERLIFE`,
    /// `MAXWARPLAYER`, `MAXMAPTIME`, `MAXJOINMEMBERSIZE`, `MAXGUILDPERCENT`, `MAXPLAYERPERCENT`,
    /// `REQUESTCANCLEPERCENT`, `NOTREQUESTPERCENT`, `ITEMPENYA`, `MAX_GCSIENDITEM`) in file order; a repeated
    /// key keeps its position and takes the last value.
    pub values: IndexMap<String, i64>,
    /// The `AUTO_OPEN_IDC` (or `AUTO_OPEN_TEST`) lines in file order (at most 7).
    pub auto_open: Vec<GcAutoOpen>,
    /// `m_nDay`: the day of the last auto-open line (0 without one).
    pub last_open_day: i32,
    /// The `GCSENDITEM` lines.
    pub send_items: Vec<GcSendItem>,
    /// The count after `OPEN`, `MAINTENANCE`, `WAR` (`m_nProcessCount[0..3]`; 14 for `OPEN` here, with 13 rows).
    pub process_count: [i32; 3],
    /// The rows of each block (`OPEN`, `MAINTENANCE`, `WAR`), in file order.
    pub process: [Vec<GcProcessRow>; 3],
}

impl GuildCombatSettings {
    /// The value of a scalar key (`None` when the file has none).
    pub fn get(&self, key: &str) -> Option<i64> {
        self.values.get(key).copied()
    }
}

/// The guild tables.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GuildTables {
    /// `expCompanyTest` rows in file order (row `i` is guild level `i + 1`), at most
    /// [`MAX_GUILD_LEVEL`].
    pub levels: Vec<GuildLevelRow>,
    /// `guildbuff.inc` rows in file order.
    pub buffs: Vec<GuildBuffDef>,
    /// `GuildCombat.txt`.
    pub combat: GuildCombatSettings,
}

#[cfg(feature = "load")]
pub(crate) use parse::{parse_guild_buffs, parse_guild_combat};

#[cfg(feature = "load")]
mod parse {
    use super::{
        GcAutoOpen, GcProcessRow, GcRowKind, GcSendItem, GuildBuffDef, GuildCombatSettings,
    };
    use flyff_script::{
        DiagCode, Diagnostics, Scanner, ScannerOptions, Script, ScriptCtx, Severity, c_atoi,
    };
    use std::sync::Arc;

    /// The scalar keys of `GuildCombat.txt` (each is `KEY NUMBER`).
    const SCALAR_KEYS: [&str; 13] = [
        "JOINPENYA",
        "GUILDLEVEL",
        "MINJOINGUILDSIZE",
        "MAXJOINGUILDSIZE",
        "MAXPLAYERLIFE",
        "MAXWARPLAYER",
        "MAXMAPTIME",
        "MAXJOINMEMBERSIZE",
        "MAXGUILDPERCENT",
        "MAXPLAYERPERCENT",
        "REQUESTCANCLEPERCENT",
        "NOTREQUESTPERCENT",
        "ITEMPENYA",
    ];

    /// `CGuildCombat::LoadScript` over decoded `text` (a raw `CScanner`; `GCSENDITEM` resolves the item with
    /// `GetDefineNum`). The original returns FALSE (and the server does not start) for a bad `AUTO_OPEN_IDC` line
    /// or an unknown `GCSENDITEM` item; here the error is reported and the line is skipped.
    pub(crate) fn parse_guild_combat(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> GuildCombatSettings {
        let mut out = GuildCombatSettings::default();
        let mut sc = Scanner::new(text, Arc::clone(file), ScannerOptions::default());
        let mut report = |sc: &Scanner<'_>, code: DiagCode, msg: String| {
            diags.report(Severity::Error, code, Some(sc.file()), sc.line(), msg);
        };
        sc.get_token(false);
        while !sc.finished() {
            let word = sc.token().to_owned();
            let mut block: Option<usize> = None;
            match word.as_str() {
                "AUTO_OPEN_IDC" | "AUTO_OPEN_TEST" => {
                    sc.get_token(false); // {
                    sc.get_token(false);
                    while !sc.token().starts_with('}') && !sc.finished() {
                        let day = c_atoi(sc.token().as_bytes());
                        let hour = sc.get_number_i32(false);
                        let minute = sc.get_number_i32(false);
                        if out.auto_open.len() >= 7 {
                            report(
                                &sc,
                                DiagCode::CapacityExceeded,
                                "GuildCombat.txt: more than 7 AUTO_OPEN lines; ignored".to_owned(),
                            );
                        } else if !(1..=7).contains(&day) {
                            report(
                                &sc,
                                DiagCode::ValueOutOfRange,
                                format!(
                                    "GuildCombat.txt: AUTO_OPEN day {day} is not 1..=7; ignored"
                                ),
                            );
                        } else if !(0..=24).contains(&hour) || !(0..=59).contains(&minute) {
                            report(
                                &sc,
                                DiagCode::ValueOutOfRange,
                                format!(
                                    "GuildCombat.txt: AUTO_OPEN time {hour}:{minute} is out of range; ignored"
                                ),
                            );
                        } else {
                            out.last_open_day = day;
                            out.auto_open.push(GcAutoOpen { day, hour, minute });
                        }
                        sc.get_token(false);
                    }
                }
                "GCSENDITEM" => {
                    let win_count = sc.get_number_i32(false);
                    sc.get_token(false);
                    let name = sc.token().to_owned();
                    let item = sctx.defines.get(&name).map(|v| v as u32);
                    let count = sc.get_number_i32(false);
                    match item {
                        Some(item) if item != u32::MAX => out.send_items.push(GcSendItem {
                            win_count,
                            item,
                            count,
                        }),
                        _ => report(
                            &sc,
                            DiagCode::UnknownIdentifier,
                            format!("GuildCombat.txt: GCSENDITEM {name} is not a define; ignored"),
                        ),
                    }
                }
                "MAX_GCSIENDITEM" => {
                    let v = sc.get_number_i32(false);
                    out.values.insert(word.clone(), i64::from(v));
                }
                "OPEN" => block = Some(0),
                "MAINTENANCE" => block = Some(1),
                "WAR" => block = Some(2),
                w if SCALAR_KEYS.contains(&w) => {
                    let v = sc.get_number(false);
                    out.values.insert(word.clone(), i64::from(v));
                }
                _ => {}
            }
            if let Some(b) = block {
                let count = sc.get_number_i32(false);
                sc.get_token(false); // {
                sc.get_token(false);
                let mut rows = Vec::new();
                while !sc.token().starts_with('}') && !sc.finished() {
                    let kind = match sc.token() {
                        "ALLMSG" => GcRowKind::AllMsg,
                        "GUILDMSG" => GcRowKind::GuildMsg,
                        "JOINMSG" => GcRowKind::JoinMsg,
                        "WORLDMSG" => GcRowKind::WorldMsg,
                        "STATE" => GcRowKind::State,
                        "WARSTATE" => GcRowKind::WarState,
                        "WAIT" => GcRowKind::Wait,
                        _ => GcRowKind::None,
                    };
                    let row = GcProcessRow {
                        kind,
                        time_ms: sc.get_number(false),
                        command: sc.get_number(false),
                        param: sc.get_number(false),
                    };
                    if rows.len() >= 25 {
                        report(
                            &sc,
                            DiagCode::CapacityExceeded,
                            "GuildCombat.txt: more than 25 rows in a block; ignored".to_owned(),
                        );
                    } else {
                        rows.push(row);
                    }
                    sc.get_token(false);
                }
                if let (Some(c), Some(r)) = (out.process_count.get_mut(b), out.process.get_mut(b)) {
                    *c = count;
                    *r = rows;
                }
            }
            sc.get_token(false);
        }
        diags.extend(sc.take_diagnostics());
        out
    }

    /// `CGuildMng::LoadBuffs`: `{ ITEM LVMIN MEMBERCOUNT MEMBERONLINE }*`; a truncated row is dropped.
    pub(crate) fn parse_guild_buffs(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> Vec<GuildBuffDef> {
        let mut out = Vec::new();
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        let mut item = sc.get_number();
        while !sc.finished() {
            let row = GuildBuffDef {
                item,
                min_level: sc.get_number(),
                member_count: sc.get_number_i32(),
                member_online: sc.get_number_i32(),
            };
            if sc.finished() {
                break;
            }
            out.push(row);
            item = sc.get_number();
        }
        out
    }
}
