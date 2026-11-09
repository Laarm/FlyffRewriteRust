//! `RulesData`: the small, owned table set the formulas read (Plan/16 §18.2), built once from the loaded
//! `GameData` on the server (feature `game`) and from the client data bundles in the browser (WP-MOVER-13).
//!
//! Only the tables that the functions of this crate use so far are present: the job rows, the EXP rows, the
//! `expUpitem` and `Setitem` tables and the skill-point costs. Later work packages append fields (die
//! penalty, set items, accessories, PK levels) without renaming these.

use crate::consts::MAX_JOB;
use flyff_data::bundle::types::{Envelope, ItemExtBundle, TablesBundle};
use flyff_data::exp_table::{EXP_UPITEM_COLS, EXP_UPITEM_ROWS, ExpCharacterRow, SetItemAvail};
use flyff_data::props::item::ItemProp;
use flyff_data::props::job::JobProp;
use flyff_data::rules::die_penalty::DiePenaltyTable;

/// Rows of `Setitem` (`+1 ..= +10`).
pub const SET_ITEM_ROWS: usize = 10;

/// One job of the `propJob.inc` / `etc.inc` tables.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct JobRow {
    /// The 18 job factors (`propJob.inc`).
    pub prop: JobProp,
    /// `dwJobType`: `JTYPE_BASE`, `JTYPE_EXPERT`, `JTYPE_PRO`, `JTYPE_MASTER`, `JTYPE_HERO`, ...
    pub job_type: u32,
    /// `dwJobBase`: the job this one advances from.
    pub job_base: u32,
}

/// The data tables of the rules (16 §18.2).
#[derive(Clone, Debug, PartialEq)]
pub struct RulesData {
    /// The jobs by id; `None` for jobs the data does not list.
    pub jobs: [Option<JobRow>; MAX_JOB],
    /// `expCharacter` rows (row 0 is a dummy).
    pub exp_character: Vec<ExpCharacterRow>,
    /// `expFlight` rows `(fxp, fxp2)`.
    pub exp_flight: Vec<(u32, u32)>,
    /// `expUpitem[column][option]` (weapon, helmet, suit, gauntlet, boots, shield x `+0..=+10`).
    pub upitem: [[i32; EXP_UPITEM_ROWS]; EXP_UPITEM_COLS],
    /// `Setitem` rows for `+1 ..= +10` (`GetSetItemAvail(n)` is `[n - 1]`).
    pub set_item_avail: [SetItemAvail; SET_ITEM_ROWS],
    /// Skill points per level of a vagrant, expert and pro skill (`Constant.inc`; defaults 1/2/3).
    pub sp_cost: [u32; 3],
    /// The bare-hand item `II_WEA_HAN_HAND` that a player without a weapon attacks with
    /// (`GetActiveHandItemProp`): its ability range, attack speed and weapon type feed the attack formulas.
    /// `None` when the data lacks it (the attack range is then 0, as the original's NULL prop).
    pub hand: Option<ItemProp>,
    /// `DiePenalty.inc`: the revival HP rate, the death EXP loss and the level-down switch by level (server only;
    /// empty in the browser, 16 §10.1). WP-MOVER-10 reads it through `progression::die_penalty_rates`.
    pub die_penalty: DiePenaltyTable,
}

impl Default for RulesData {
    fn default() -> Self {
        RulesData {
            jobs: std::array::from_fn(|_| None),
            exp_character: Vec::new(),
            exp_flight: Vec::new(),
            upitem: [[0; EXP_UPITEM_ROWS]; EXP_UPITEM_COLS],
            set_item_avail: std::array::from_fn(|_| SetItemAvail::default()),
            sp_cost: [1, 2, 3],
            hand: None,
            die_penalty: DiePenaltyTable::default(),
        }
    }
}

impl RulesData {
    /// The job row of `job`; `None` for an unknown job id (the callers use the documented defaults).
    pub fn job(&self, job: i32) -> Option<&JobRow> {
        self.jobs.get(usize::try_from(job).ok()?)?.as_ref()
    }

    /// `GetSetItemAvail(n)` for `n` in `1..=10`.
    pub fn set_item_avail(&self, n: i32) -> Option<&SetItemAvail> {
        self.set_item_avail
            .get(usize::try_from(n.checked_sub(1)?).ok()?)
    }

    /// `expCharacter[level]`.
    pub fn exp_row(&self, level: i32) -> Option<&ExpCharacterRow> {
        self.exp_character.get(usize::try_from(level).ok()?)
    }
}

/// Why a bundle could not become [`RulesData`] (the browser path, [`RulesData::from_bundles`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RulesError(pub String);

impl std::fmt::Display for RulesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RulesError {}

impl RulesData {
    /// The browser constructor (16 §18.2): the JSON files of the `tables` and `item-ext` bundles (the envelope
    /// `{bundle, schema, data}` as served under `/data/<bundle>.<h16>.json`), decoded with flyff-data's
    /// featureless DTO types. `item_ext` is parsed and checked; its tables (named sets, accessories, random
    /// options) are consumed by the M4 rules (WP-MOVER-15) and are not part of `RulesData` yet. The bare-hand
    /// item is not in these bundles: the caller sets [`RulesData::hand`] from the `items` bundle.
    pub fn from_bundles(tables: &[u8], item_ext: &[u8]) -> Result<RulesData, RulesError> {
        let t: Envelope<TablesBundle> = serde_json::from_slice(tables)
            .map_err(|e| RulesError(format!("tables bundle: {e}")))?;
        check_name(&t.bundle, "tables")?;
        let x: Envelope<ItemExtBundle> = serde_json::from_slice(item_ext)
            .map_err(|e| RulesError(format!("item-ext bundle: {e}")))?;
        check_name(&x.bundle, "item-ext")?;
        Ok(RulesData::from_tables(&t.data))
    }

    /// The tables of a decoded `tables` bundle: jobs (the `propJob.inc` factors and the `etc.inc` tier and base),
    /// `expCharacter` (`[exp1, exp2, lp, limit]`), `expUpitem`, the `Setitem` rows and the skill-point costs. The
    /// flight table and the death-penalty tables are server-only and stay empty.
    pub fn from_tables(t: &TablesBundle) -> RulesData {
        let mut d = RulesData::default();
        for row in &t.jobs {
            let slot = usize::try_from(row.id).ok().and_then(|i| d.jobs.get_mut(i));
            if let Some(slot) = slot {
                *slot = Some(JobRow {
                    prop: row.prop.clone().unwrap_or_default(),
                    job_type: row.kind,
                    job_base: row.base,
                });
            }
        }
        d.exp_character = t
            .exp
            .character
            .iter()
            .map(|r| ExpCharacterRow {
                exp1: r[0],
                exp2: r[1],
                lp_point: u32::try_from(r[2]).unwrap_or(0),
                limit_exp: r[3],
            })
            .collect();
        for (col, src) in d.upitem.iter_mut().zip(&t.exp.upitem) {
            for (cell, v) in col.iter_mut().zip(src) {
                *cell = *v;
            }
        }
        for (slot, row) in d.set_item_avail.iter_mut().zip(&t.exp.set_item_avail) {
            *slot = row.clone();
        }
        d.sp_cost = [
            t.constants.vag_sp,
            t.constants.expert_sp,
            t.constants.pro_sp,
        ];
        d
    }
}

fn check_name(got: &str, want: &str) -> Result<(), RulesError> {
    if got == want {
        Ok(())
    } else {
        Err(RulesError(format!(
            "expected the {want} bundle, got {got:?}"
        )))
    }
}

#[cfg(feature = "game")]
impl RulesData {
    /// The server constructor: copies the tables of the loaded `GameData` (jobs with their `etc.inc` rows,
    /// `expTable.inc`, `Constant.inc`).
    pub fn from_game_data(gd: &flyff_data::GameData) -> RulesData {
        let mut d = RulesData::default();
        for (i, slot) in d.jobs.iter_mut().enumerate() {
            let prop = gd.jobs.props.get(i).and_then(Option::as_ref);
            let info = gd.jobs.info.get(i).and_then(Option::as_ref);
            if prop.is_some() || info.is_some() {
                *slot = Some(JobRow {
                    prop: prop.cloned().unwrap_or_default(),
                    job_type: info.map_or(u32::MAX, |x| x.job_type),
                    job_base: info.map_or(0, |x| x.job_base),
                });
            }
        }
        d.exp_character = gd.exp.character.clone();
        d.exp_flight = gd.exp.flight.clone();
        d.upitem = gd.exp.upitem;
        for (slot, row) in d.set_item_avail.iter_mut().zip(&gd.exp.set_item_avail) {
            *slot = row.clone();
        }
        d.sp_cost = [
            gd.constant.vag_sp,
            gd.constant.expert_sp,
            gd.constant.pro_sp,
        ];
        d.hand = gd.item(flyff_common::defs::item::II_WEA_HAN_HAND).cloned();
        d.die_penalty = gd.die_penalty.clone();
        d
    }
}
