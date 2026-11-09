//! `expTable.inc` (`LoadExpTable`, Plan/14 §6.7, ref/17 §8.5, ref/10 §12.1; src:_Common/Project.cpp:3387-3618,
//! guild.cpp:187-211).
//!
//! Every block follows the original pattern "read the first value; `while token != "}"` { ...; read the
//! next value }". Block names are matched case-sensitively. Unknown block names (`jobAbillity`, inactive
//! under `__S_ADD_RESTATE`, and `expCompany`) are skipped token by token. Rows beyond the array capacity
//! are an Error and ignored (**FIX**: the original overflowed). Exp values exceed 32 bits (up to 3.7e11)
//! and use `GetExpInteger` (= `GetInt64`).

use serde::{Deserialize, Serialize};

/// `MAX_EXPCHARACTER`.
pub const MAX_EXPCHARACTER: usize = 200;
/// `MAX_FXPCHARACTER`.
pub const MAX_FXPCHARACTER: usize = 200;
/// `MAX_EXPLPPOINT`.
pub const MAX_EXPLPPOINT: usize = 41;
/// `MAX_EXPSKILL`.
pub const MAX_EXPSKILL: usize = 21;
/// `MAX_PARTYLEVEL`.
pub const MAX_PARTYLEVEL: usize = 10;
/// `MAX_ADDEXPPARTY`.
pub const MAX_ADDEXPPARTY: usize = 16;
/// Rows of `expDropLuck` (`m_adwExpDropLuck[122][11]`).
pub const EXP_DROP_LUCK_ROWS: usize = 122;
/// Columns of `expDropLuck`.
pub const EXP_DROP_LUCK_COLS: usize = 11;
/// `m_aSetItemAvail[11]`.
pub const MAX_SETITEM_AVAIL: usize = 11;
/// Rows of `expUpitem` (`+0 ..= +10`).
pub const EXP_UPITEM_ROWS: usize = 11;
/// Columns of `expUpitem` (weapon, helmet, suit, gauntlet, boots, shield).
pub const EXP_UPITEM_COLS: usize = 6;

/// One row of `expCharacter`: `EXP PXP LP LIMEXP`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpCharacterRow {
    /// `nExp1`: the EXP needed to reach this level from the previous one (row 0 is a dummy).
    pub exp1: i64,
    /// `nExp2`: the second column (PXP).
    pub exp2: i64,
    /// `dwLPPoint`: skill (LP) points gained at this level.
    pub lp_point: u32,
    /// `nLimitExp`: the EXP limit column.
    pub limit_exp: i64,
}

/// One row of `expParty`: `EXP POINT`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpPartyRow {
    /// `Exp`.
    pub exp: u32,
    /// `Point`.
    pub point: u32,
}

/// One row of `expPartySkill`: `EXP LEVEL` (extra EXP percent, level that grants it).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpPartySkillRow {
    /// `Exp` (percent).
    pub exp: u32,
    /// `Level`.
    pub level: u32,
}

/// One row of `Setitem`: `Acc Block HPRate AtkRate Stats` for `+1 ..= +10`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SetItemAvail {
    /// `nHitRate`.
    pub hit_rate: i32,
    /// `nBlock`.
    pub block: i32,
    /// `nMaxHitPointRate`.
    pub max_hit_point_rate: i32,
    /// `nAttackPowerRate` (custom 4th column, `__BETTER_ARMOR_BONUS`).
    pub attack_power_rate: i32,
    /// `nAdded`.
    pub added: i32,
}

/// One row of `RestExpFactorTable`: `MINLV MAXLV FACTOR` (guild house).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RestExpFactor {
    /// Minimum level.
    pub min_level: i32,
    /// Maximum level.
    pub max_level: i32,
    /// The factor.
    pub factor: f32,
}

/// All the tables of `expTable.inc` except the guild levels (those go to
/// [`crate::social::guild::GuildTables::levels`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExpTables {
    /// `expCharacter` rows in file order (index = file row; row 0 is a dummy), at most 200.
    pub character: Vec<ExpCharacterRow>,
    /// `expFlight`: `(fxp, fxp2)` per row, at most 200.
    pub flight: Vec<(u32, u32)>,
    /// `expLPPoint`: at most 41.
    pub lp_point: Vec<u32>,
    /// `expSkill`: at most 21.
    pub skill: Vec<u32>,
    /// `expParty`: at most 10.
    pub party: Vec<ExpPartyRow>,
    /// `expPartySkill`: at most 16.
    pub party_skill: Vec<ExpPartySkillRow>,
    /// `expDropLuck`, filled row-major into `[122][11]` (the unread tail is zero in the original; only
    /// the rows touched by the file are stored).
    pub drop_luck: Vec<[u32; EXP_DROP_LUCK_COLS]>,
    /// `Setitem` rows for `+1 ..= +10` (`GetSetItemAvail(n)` is `[n - 1]`), at most 11.
    pub set_item_avail: Vec<SetItemAvail>,
    /// `expUpitem` stored `[column][row]` (weapon, helmet, suit, gauntlet, boots, shield x `+0..=+10`);
    /// all zero when the block is absent.
    pub upitem: [[i32; EXP_UPITEM_ROWS]; EXP_UPITEM_COLS],
    /// `RestExpFactorTable` rows.
    pub rest_exp_factor: Vec<RestExpFactor>,
}

impl Default for ExpTables {
    fn default() -> Self {
        Self {
            character: Vec::new(),
            flight: Vec::new(),
            lp_point: Vec::new(),
            skill: Vec::new(),
            party: Vec::new(),
            party_skill: Vec::new(),
            drop_luck: Vec::new(),
            set_item_avail: Vec::new(),
            upitem: [[0; EXP_UPITEM_ROWS]; EXP_UPITEM_COLS],
            rest_exp_factor: Vec::new(),
        }
    }
}

impl ExpTables {
    /// `GetSetItemAvail(n)` for `n` in `1..=10`.
    pub fn set_item_avail(&self, n: i32) -> Option<&SetItemAvail> {
        usize::try_from(n.checked_sub(1)?)
            .ok()
            .and_then(|i| self.set_item_avail.get(i))
    }

    /// The `expDropLuck` value at `(row, col)`; 0 beyond the rows the file supplied (as the zeroed array).
    pub fn drop_luck(&self, row: usize, col: usize) -> u32 {
        self.drop_luck
            .get(row)
            .and_then(|r| r.get(col))
            .copied()
            .unwrap_or(0)
    }
}

#[cfg(feature = "load")]
pub(crate) use parse::parse_exp_table;

#[cfg(feature = "load")]
mod parse {
    use super::*;
    use crate::social::guild::{GuildLevelRow, GuildTables, MAX_GUILD_LEVEL};
    use flyff_script::{DiagCode, Diagnostics, ScannerOptions, Script, ScriptCtx, Severity};
    use std::sync::Arc;

    /// What `LoadExpTable` produces.
    #[derive(Debug, Default)]
    pub(crate) struct ExpFile {
        pub exp: ExpTables,
        pub guild: GuildTables,
        /// Names of the blocks that were recognised, in file order.
        pub blocks: Vec<String>,
    }

    /// `*script.token != '}'` with an EOF guard (the original would spin on a truncated file).
    fn open(sc: &Script<'_>) -> bool {
        !sc.finished() && !sc.token().starts_with('}')
    }

    fn overflow(sc: &mut Script<'_>, block: &str, cap: usize) {
        let msg = format!("{block}: more than {cap} entries; the rest is ignored");
        sc.report(Severity::Error, DiagCode::CapacityExceeded, msg);
    }

    /// `CProject::LoadExpTable` over decoded `text`. `guild_exp_divisor` is `gameplay.guild_rate` (F11).
    pub(crate) fn parse_exp_table(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        guild_exp_divisor: u32,
        diags: &mut Diagnostics,
    ) -> ExpFile {
        let mut out = ExpFile::default();
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        sc.get_token(); // subject or FINISHED
        while !sc.finished() {
            let name = sc.token().to_owned();
            match name.as_str() {
                "expCharacter" => {
                    out.blocks.push(name);
                    out.exp.character.clear();
                    sc.get_token(); // {
                    let mut v = sc.get_int64();
                    let mut reported = false;
                    while open(&sc) {
                        let row = ExpCharacterRow {
                            exp1: v,
                            exp2: sc.get_int64(),
                            lp_point: sc.get_number(),
                            limit_exp: sc.get_int64(),
                        };
                        push_capped(
                            &mut out.exp.character,
                            row,
                            MAX_EXPCHARACTER,
                            "expCharacter",
                            &mut sc,
                            &mut reported,
                        );
                        v = sc.get_int64();
                    }
                }
                "expFlight" => {
                    out.blocks.push(name);
                    out.exp.flight.clear();
                    sc.get_token(); // {
                    let mut v = sc.get_number();
                    let mut reported = false;
                    while open(&sc) {
                        let row = (v, sc.get_number());
                        push_capped(
                            &mut out.exp.flight,
                            row,
                            MAX_FXPCHARACTER,
                            "expFlight",
                            &mut sc,
                            &mut reported,
                        );
                        v = sc.get_number();
                    }
                }
                "expLPPoint" => {
                    out.blocks.push(name);
                    out.exp.lp_point.clear();
                    sc.get_token(); // {
                    let mut v = sc.get_number();
                    let mut reported = false;
                    while open(&sc) {
                        push_capped(
                            &mut out.exp.lp_point,
                            v,
                            MAX_EXPLPPOINT,
                            "expLPPoint",
                            &mut sc,
                            &mut reported,
                        );
                        v = sc.get_number();
                    }
                }
                "expJobLevel" => {
                    // read and discarded
                    out.blocks.push(name);
                    sc.get_token(); // {
                    let _ = sc.get_number();
                    while open(&sc) {
                        sc.get_number();
                        sc.get_number();
                    }
                }
                "expSkill" => {
                    out.blocks.push(name);
                    out.exp.skill.clear();
                    sc.get_token(); // {
                    let mut v = sc.get_number();
                    let mut reported = false;
                    while open(&sc) {
                        push_capped(
                            &mut out.exp.skill,
                            v,
                            MAX_EXPSKILL,
                            "expSkill",
                            &mut sc,
                            &mut reported,
                        );
                        v = sc.get_number();
                    }
                }
                "attackStyle" => {
                    // read and discarded
                    out.blocks.push(name);
                    sc.get_token(); // {
                    let _ = sc.get_number();
                    while open(&sc) {
                        sc.get_number();
                        sc.get_number();
                        sc.get_number();
                    }
                }
                "expParty" => {
                    out.blocks.push(name);
                    out.exp.party.clear();
                    sc.get_token(); // {
                    let mut v = sc.get_number();
                    let mut reported = false;
                    while open(&sc) {
                        let row = ExpPartyRow {
                            exp: v,
                            point: sc.get_number(),
                        };
                        push_capped(
                            &mut out.exp.party,
                            row,
                            MAX_PARTYLEVEL,
                            "expParty",
                            &mut sc,
                            &mut reported,
                        );
                        v = sc.get_number();
                    }
                }
                "expPartySkill" => {
                    out.blocks.push(name);
                    out.exp.party_skill.clear();
                    sc.get_token(); // {
                    let mut v = sc.get_number();
                    let mut reported = false;
                    while open(&sc) {
                        let row = ExpPartySkillRow {
                            exp: v,
                            level: sc.get_number(),
                        };
                        push_capped(
                            &mut out.exp.party_skill,
                            row,
                            MAX_ADDEXPPARTY,
                            "expPartySkill",
                            &mut sc,
                            &mut reported,
                        );
                        v = sc.get_number();
                    }
                }
                "expCompanyTest" => {
                    // CGuildTable::ReadBlock (F11)
                    out.blocks.push(name);
                    out.guild.levels.clear();
                    sc.get_token(); // {
                    let mut v = sc.get_number();
                    let mut reported = false;
                    while open(&sc) {
                        // `dwVal /= __GUILD_RATE` (u32 division; a zero divisor leaves the value)
                        let pxp = v.checked_div(guild_exp_divisor).unwrap_or(v);
                        let row = GuildLevelRow {
                            pxp_count: pxp,
                            penya: sc.get_number(),
                            max_member: sc.get_number(),
                        };
                        push_capped(
                            &mut out.guild.levels,
                            row,
                            MAX_GUILD_LEVEL,
                            "expCompanyTest",
                            &mut sc,
                            &mut reported,
                        );
                        v = sc.get_number();
                    }
                }
                "expDropLuck" => {
                    out.blocks.push(name);
                    out.exp.drop_luck.clear();
                    sc.get_token(); // {
                    let mut idx = 0usize;
                    let mut v = sc.get_number();
                    let cap = EXP_DROP_LUCK_ROWS * EXP_DROP_LUCK_COLS;
                    let mut reported = false;
                    while open(&sc) {
                        if idx < cap {
                            let (r, c) = (idx / EXP_DROP_LUCK_COLS, idx % EXP_DROP_LUCK_COLS);
                            if r >= out.exp.drop_luck.len() {
                                out.exp.drop_luck.push([0; EXP_DROP_LUCK_COLS]);
                            }
                            if let Some(cell) =
                                out.exp.drop_luck.get_mut(r).and_then(|row| row.get_mut(c))
                            {
                                *cell = v;
                            }
                        } else if !reported {
                            reported = true;
                            overflow(&mut sc, "expDropLuck", cap);
                        }
                        idx += 1;
                        v = sc.get_number();
                    }
                }
                "Setitem" => {
                    out.blocks.push(name);
                    out.exp.set_item_avail.clear();
                    sc.get_token(); // {
                    let mut v = sc.get_number_i32();
                    let mut reported = false;
                    while open(&sc) {
                        let row = SetItemAvail {
                            hit_rate: v,
                            block: sc.get_number_i32(),
                            max_hit_point_rate: sc.get_number_i32(),
                            attack_power_rate: sc.get_number_i32(),
                            added: sc.get_number_i32(),
                        };
                        push_capped(
                            &mut out.exp.set_item_avail,
                            row,
                            MAX_SETITEM_AVAIL,
                            "Setitem",
                            &mut sc,
                            &mut reported,
                        );
                        v = sc.get_number_i32();
                    }
                }
                "expUpitem" => {
                    out.blocks.push(name);
                    out.exp.upitem = [[0; EXP_UPITEM_ROWS]; EXP_UPITEM_COLS];
                    sc.get_token(); // {
                    let mut rows = 0usize;
                    let mut cols_ok = false;
                    let mut reported = false;
                    loop {
                        let first = sc.get_number_i32();
                        if sc.finished() || sc.token().starts_with('}') {
                            break;
                        }
                        let mut cells = [first, 0, 0, 0, 0, 0];
                        for c in cells.iter_mut().skip(1) {
                            *c = sc.get_number_i32();
                        }
                        cols_ok = true;
                        if rows < EXP_UPITEM_ROWS {
                            for (col, v) in cells.iter().enumerate() {
                                if let Some(cell) =
                                    out.exp.upitem.get_mut(col).and_then(|c| c.get_mut(rows))
                                {
                                    *cell = *v;
                                }
                            }
                        } else if !reported {
                            reported = true;
                            overflow(&mut sc, "expUpitem", EXP_UPITEM_ROWS);
                        }
                        rows += 1;
                    }
                    if !cols_ok || rows != EXP_UPITEM_ROWS {
                        let msg = format!(
                            "expUpitem: expected {EXP_UPITEM_ROWS} rows x {EXP_UPITEM_COLS} columns, found {rows} rows"
                        );
                        sc.report(Severity::Error, DiagCode::ValueOutOfRange, msg);
                    }
                }
                "RestExpFactorTable" => {
                    out.blocks.push(name);
                    sc.get_token(); // {
                    loop {
                        let min_level = sc.get_number_i32();
                        if sc.finished() || sc.token().starts_with('}') {
                            break;
                        }
                        let max_level = sc.get_number_i32();
                        let factor = sc.get_float();
                        out.exp.rest_exp_factor.push(RestExpFactor {
                            min_level,
                            max_level,
                            factor,
                        });
                    }
                }
                _ => {} // jobAbillity, expCompany, ... : skipped token by token
            }
            sc.get_token(); // type name or }
        }
        out
    }

    /// Pushes a row unless the capacity is exhausted (reported once per block).
    fn push_capped<T>(
        v: &mut Vec<T>,
        row: T,
        cap: usize,
        block: &str,
        sc: &mut Script<'_>,
        reported: &mut bool,
    ) {
        if v.len() < cap {
            v.push(row);
        } else if !*reported {
            *reported = true;
            overflow(sc, block, cap);
        }
    }
}
