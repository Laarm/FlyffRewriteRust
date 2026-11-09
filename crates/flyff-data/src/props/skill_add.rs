//! `AddSkillProp` and `LoadPropAddSkill` (Plan/14 §6.4, §5.4 F5, ref/17 §7.8;
//! src:_Common/Project.cpp:2555-2644, 912-930).
//!
//! `propSkillAdd.csv` is ASCII **comma mode** (`GetNumber(TRUE)`): 2,301 rows of 36 fields, row index =
//! `SA_*` define value (max 4504). The loop stops at the first `id == 0` or at EOF. `=` is `-1`, an
//! empty field is 0 (one occurrence in the data set).

use crate::types::{AddSkillId, IdTable, SkillId};
use serde::{Deserialize, Serialize};

/// One row of `propSkillAdd.csv`: the per-level numbers of a skill (field `fN` = CSV column).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AddSkillProp {
    /// f0: `SA_*` (the row index).
    pub id: AddSkillId,
    /// f1: `dwName`, the `SI_*` skill id.
    pub skill_id: SkillId,
    /// f2.
    pub skill_lvl: u32,
    /// f3.
    pub ability_min: u32,
    /// f4.
    pub ability_max: u32,
    /// f5 (F5: `=` copies `ability_min`).
    pub ability_min_pvp: u32,
    /// f6 (F5: `=` copies `ability_max`).
    pub ability_max_pvp: u32,
    /// f7.
    pub attack_speed: u32,
    /// f8.
    pub dmg_shift: u32,
    /// f9.
    pub probability: u32,
    /// f10 (F5: `=` copies `probability`).
    pub probability_pvp: u32,
    /// f11.
    pub taunt: u32,
    /// f12..f13: `DST_*` (DWORD in C).
    pub dest_param: [u32; 2],
    /// f14..f15 (DWORD in C).
    pub adj_param_val: [u32; 2],
    /// f16..f17 (DWORD in C).
    pub chg_param_val: [u32; 2],
    /// f18..f20.
    pub dest_data1: [i32; 3],
    /// f21.
    pub active_skill: u32,
    /// f22.
    pub active_skill_rate: u32,
    /// f23 (F5: `=` copies `active_skill_rate`).
    pub active_skill_rate_pvp: u32,
    /// f24.
    pub req_mp: i32,
    /// f25.
    pub req_fp: i32,
    /// f26 (F5: `=` takes the skill's `skill_ready`).
    pub cooldown: u32,
    /// f27 (F5: divided by 4 when the cooldown was defaulted and the skill row has `ver < 9`).
    pub casting_time: u32,
    /// f28.
    pub skill_range: u32,
    /// f29.
    pub circle_time: u32,
    /// f30.
    pub pain_time: u32,
    /// f31.
    pub skill_time: u32,
    /// f32.
    pub skill_count: i32,
    /// f33.
    pub skill_exp: u32,
    /// f34.
    pub exp: u32,
    /// f35.
    pub combo_skill_time: u32,
}

/// `GetAddSkillProp(sub_define, level)`: `idx = sub_define + max(level, 1) - 1`; **KEEP-QUIRK:** when
/// `idx < 5` the index is bumped by 5. `None` for a hole or an index outside the table (the original logs
/// an Error and returns null).
pub fn lookup_add_skill(
    table: &IdTable<AddSkillProp>,
    sub_define: u32,
    level: u32,
) -> Option<&AddSkillProp> {
    let level = level.max(1);
    // The original computes `int nskillIndex = dwSubDefine + dwLevel - 1` (32-bit wrap) and tests it signed.
    let mut idx = sub_define.wrapping_add(level).wrapping_sub(1) as i32;
    if idx < 5 {
        idx += 5;
    }
    if idx < 0 || idx as u32 >= table.size() {
        return None;
    }
    table.get(idx as u32)
}

#[cfg(feature = "load")]
pub(crate) use parse::parse_add_skill_file;

#[cfg(feature = "load")]
mod parse {
    use super::AddSkillProp;
    use crate::props::item::SkillProp;
    use crate::types::{IdTable, NULL_ID};
    use flyff_script::{Diagnostics, ScannerOptions, Script, ScriptCtx};
    use std::sync::Arc;

    /// The rows of `propSkillAdd.csv` in file order.
    #[derive(Debug, Default)]
    pub(crate) struct AddSkillFile {
        /// Rows with F5 applied; a repeated id replaces the earlier one when stored.
        pub rows: Vec<AddSkillProp>,
        /// Empty CSV fields read as 0 (C08: 1 in the reference data).
        pub empty_fields: u32,
    }

    /// `CProject::LoadPropAddSkill` over decoded `text`; `skills` is `m_aPropSkill` (needed by F5).
    pub(crate) fn parse_add_skill_file(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        skills: &IdTable<SkillProp>,
        diags: &mut Diagnostics,
    ) -> AddSkillFile {
        let mut out = AddSkillFile::default();
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        let mut empty = 0u32;
        // One field: GetNumber(TRUE); an empty cell inside a row counts.
        macro_rules! n {
            () => {{
                let v = sc.get_number_comma();
                if sc.token().is_empty() && !sc.finished() {
                    empty += 1;
                }
                v
            }};
        }
        let mut i = sc.get_number_comma();
        while !sc.finished() {
            if i == 0 {
                break; // "the rest of the file is ignored"
            }
            let mut p = AddSkillProp {
                id: i,
                skill_id: n!(),
                skill_lvl: n!(),
                ability_min: n!(),
                ability_max: n!(),
                ability_min_pvp: n!(),
                ability_max_pvp: n!(),
                attack_speed: n!(),
                dmg_shift: n!(),
                probability: n!(),
                probability_pvp: n!(),
                taunt: n!(),
                dest_param: [n!(), n!()],
                adj_param_val: [n!(), n!()],
                chg_param_val: [n!(), n!()],
                dest_data1: [n!() as i32, n!() as i32, n!() as i32],
                active_skill: n!(),
                active_skill_rate: n!(),
                active_skill_rate_pvp: n!(),
                req_mp: n!() as i32,
                req_fp: n!() as i32,
                cooldown: n!(),
                casting_time: n!(),
                skill_range: n!(),
                circle_time: n!(),
                pain_time: n!(),
                skill_time: n!(),
                skill_count: n!() as i32,
                skill_exp: n!(),
                exp: n!(),
                combo_skill_time: n!(),
            };
            // F5 (`dwDefault` = 0xFFFFFFFF)
            if p.ability_min_pvp == NULL_ID {
                p.ability_min_pvp = p.ability_min;
            }
            if p.ability_max_pvp == NULL_ID {
                p.ability_max_pvp = p.ability_max;
            }
            if let Some(skill) = skills.get(p.skill_id)
                && p.cooldown == NULL_ID
            {
                p.cooldown = skill.skill_ready;
                if skill.ver < 9 && p.casting_time != NULL_ID {
                    p.casting_time /= 4;
                }
            }
            if p.probability_pvp == NULL_ID {
                p.probability_pvp = p.probability;
            }
            if p.active_skill_rate_pvp == NULL_ID {
                p.active_skill_rate_pvp = p.active_skill_rate;
            }
            out.rows.push(p);
            i = sc.get_number_comma();
        }
        out.empty_fields = empty;
        out
    }
}
