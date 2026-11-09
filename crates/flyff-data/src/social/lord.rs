//! The lord tables (Plan/14 §6.15, ref/17 §8.11, ref/15 §9):
//!
//! * `lordskill.txt` (`CLordSkill::Initialize`, src:_Common/lordskill.cpp:111-146): rows of
//!   `ID TYPE NAME DESC TARGETTYPE ITEM COOLTIME ICON PASSIVE SRCSFX DSTSFX RANGE` (8 rows),
//! * `lordevent.inc` (`ILordEvent::Initialize`, src:_Common/lord.cpp:448-480): `exp { (RATE COST)* }` and
//!   `item { (RATE COST)* }`, the exp/drop bonus of a lord event and what it costs (11 rows each, the first is
//!   0 / 0),
//! * `election.inc` (`CTElection::ReadIni`, src:databaseserver/tlord.cpp:200-250): `property { nDayOfWeek ...
//!   nDays }`, `rate { ... }`, `fRequirementFactor F` and `item SEX { ITEM* }`.

use serde::{Deserialize, Serialize};

/// One lord skill (`CLordSkillComponent`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct LordSkill {
    /// `LI_*`.
    pub id: u32,
    /// `LT_*`.
    pub kind: u32,
    /// The name.
    pub name: String,
    /// The description.
    pub desc: String,
    /// `LTT_*` target type.
    pub target: u32,
    /// The buff item (`NULL_ID` for `=`).
    pub item: u32,
    /// The cool time in MINUTES (one lord-minute tick of P4 each; `CTLordSkill::OnTimer` decrements per run).
    pub cooltime: i32,
    /// The icon file (a bare file name under `icon/`).
    pub icon: String,
    /// Passive skill (1) or not (0).
    pub passive: i32,
    /// The source effect (`NULL_ID` for `=`).
    pub src_sfx: u32,
    /// The destination effect.
    pub dst_sfx: u32,
    /// The range in metres.
    pub range: f32,
}

/// `election.inc` (`CTElection::sProperty`; every field is 0 / empty when its key is absent).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ElectionTable {
    /// `nDayOfWeek` when the candidacy starts (1 = Sunday .. 7 = Saturday; 6 = Friday here).
    pub day_of_week: i32,
    /// `nHour` when the candidacy starts.
    pub hour: i32,
    /// `tCandidacy`: the candidacy period in seconds (86,400).
    pub candidacy_s: i64,
    /// `tVote`: the voting period in seconds (172,800).
    pub vote_s: i64,
    /// `nDays`: days between two elections (14).
    pub days: i32,
    /// `rate { ... }`: the deposit refund rate by vote rank (10 entries) and, last, for a candidate who drops
    /// out (11 entries here).
    pub rates: Vec<f32>,
    /// `fRequirementFactor`: the share of the votes a lord needs (0.1).
    pub requirement_factor: f32,
    /// `item SEX { ... }`: the lord's reward items by sex (`[male, female]`, 7 each here).
    pub items: [Vec<u32>; 2],
}

/// The lord tables.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LordTables {
    /// `lordskill.txt` rows in file order.
    pub skills: Vec<LordSkill>,
    /// `lordevent.inc` `exp`: `(factor, cost)` rows (`m_vEFactor`).
    pub event_exp: Vec<(f32, i64)>,
    /// `lordevent.inc` `item`: `(factor, cost)` rows (`m_vIFactor`).
    pub event_item: Vec<(f32, i64)>,
    /// `election.inc`.
    pub election: ElectionTable,
}

impl LordTables {
    /// `ILordEvent::GetCost( iEEvent, iIEvent )`: the sum of both costs; 0 when both indexes are 0 or one is
    /// beyond its table.
    pub fn event_cost(&self, exp_index: usize, item_index: usize) -> i64 {
        if exp_index == 0 && item_index == 0 {
            return 0;
        }
        match (
            self.event_exp.get(exp_index),
            self.event_item.get(item_index),
        ) {
            (Some(e), Some(i)) => e.1.wrapping_add(i.1),
            _ => 0,
        }
    }

    /// `GetEFactor( index )`: the exp factor of a lord event step (1.0 beyond the table).
    pub fn event_exp_factor(&self, index: usize) -> f32 {
        self.event_exp.get(index).map_or(1.0, |e| e.0)
    }

    /// `GetIFactor( index )`: the drop factor of a lord event step (1.0 beyond the table).
    pub fn event_item_factor(&self, index: usize) -> f32 {
        self.event_item.get(index).map_or(1.0, |e| e.0)
    }
}

#[cfg(feature = "load")]
pub(crate) use parse::{parse_election, parse_lord_events, parse_lord_skills};

#[cfg(feature = "load")]
mod parse {
    use super::{ElectionTable, LordSkill};
    use flyff_script::{DiagCode, Diagnostics, ScannerOptions, Script, ScriptCtx, Severity};
    use std::sync::Arc;

    fn open(sc: &Script<'_>) -> bool {
        !sc.token().starts_with('}') && !sc.finished()
    }

    /// `ILordEvent::Initialize` over decoded `text`: `(exp rows, item rows)`.
    #[allow(clippy::type_complexity)]
    pub(crate) fn parse_lord_events(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> (Vec<(f32, i64)>, Vec<(f32, i64)>) {
        let (mut exp, mut item) = (Vec::new(), Vec::new());
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        sc.get_token();
        while !sc.finished() {
            let which = sc.token().to_owned();
            if which == "exp" || which == "item" {
                sc.get_token(); // {
                let mut rate = sc.get_float();
                while open(&sc) {
                    let cost = sc.get_int64();
                    if which == "exp" {
                        exp.push((rate, cost));
                    } else {
                        item.push((rate, cost));
                    }
                    rate = sc.get_float();
                }
            }
            sc.get_token();
        }
        (exp, item)
    }

    /// `CTElection::ReadIni` over decoded `text`.
    pub(crate) fn parse_election(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> ElectionTable {
        let mut t = ElectionTable::default();
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        sc.get_token();
        while !sc.finished() {
            match sc.token().to_owned().as_str() {
                "property" => {
                    sc.get_token(); // {
                    sc.get_token();
                    while open(&sc) {
                        match sc.token().to_owned().as_str() {
                            "nDayOfWeek" => t.day_of_week = sc.get_number_i32(),
                            "nHour" => t.hour = sc.get_number_i32(),
                            "tCandidacy" => t.candidacy_s = i64::from(sc.get_number()),
                            "tVote" => t.vote_s = i64::from(sc.get_number()),
                            "nDays" => t.days = sc.get_number_i32(),
                            _ => {}
                        }
                        sc.get_token();
                    }
                }
                "rate" => {
                    sc.get_token(); // {
                    let mut rate = sc.get_float();
                    while open(&sc) {
                        t.rates.push(rate);
                        rate = sc.get_float();
                    }
                }
                "fRequirementFactor" => t.requirement_factor = sc.get_float(),
                "item" => {
                    let index = sc.get_number();
                    sc.get_token(); // {
                    let mut item = sc.get_number();
                    while open(&sc) {
                        match t.items.get_mut(index as usize) {
                            Some(list) => list.push(item),
                            None => sc.report(
                                Severity::Error,
                                DiagCode::IndexOutOfRange,
                                format!("election.inc: item list {index} is not 0 or 1; ignored"),
                            ),
                        }
                        item = sc.get_number();
                    }
                }
                _ => {}
            }
            sc.get_token();
        }
        t
    }

    /// `CLordSkill::Initialize` over decoded `text`.
    pub(crate) fn parse_lord_skills(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> Vec<LordSkill> {
        let mut out = Vec::new();
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        let mut id = sc.get_number();
        while !sc.finished() {
            let kind = sc.get_number();
            sc.get_token();
            let name = sc.token().to_owned();
            sc.get_token();
            let desc = sc.token().to_owned();
            let target = sc.get_number();
            let item = sc.get_number();
            let cooltime = sc.get_number_i32();
            sc.get_token();
            let icon = sc.token().to_owned();
            let passive = sc.get_number_i32();
            let src_sfx = sc.get_number();
            let dst_sfx = sc.get_number();
            let range = sc.get_float();
            if sc.finished() {
                break; // the row ran into the end of the file
            }
            out.push(LordSkill {
                id,
                kind,
                name,
                desc,
                target,
                item,
                cooltime,
                icon,
                passive,
                src_sfx,
                dst_sfx,
                range,
            });
            id = sc.get_number();
        }
        out
    }
}
