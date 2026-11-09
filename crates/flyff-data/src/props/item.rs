//! `ItemProp` and the `LoadPropItem` row loader (Plan/14 §6.3, ref/17 §7.2;
//! src:_Common/ProjectCmn.cpp:311-556), shared by `propItem.txt`, `propSkill.txt` and
//! `propTroupeSkill.txt`.
//!
//! Rows are **token streams**: a triple-quoted cell is 3 tokens and one field, `-n` is 2 tokens and one
//! field (ref/17 §4.8). The loader reads `ver = get_number()` before the loop and again at the end of
//! each row; `id == 0` stops the file with an Error (the rows read so far are kept).

use crate::types::{ItemId, NULL_ID};
use flyff_common::defs::{itemkind, neuz};
use serde::{Deserialize, Serialize};

/// `IP_FLAG_BINDS`: the item is soulbound (`ItemProp::flag` bit, set for `IK3_EVENTMAIN`/`IK3_BINDS`).
pub const IP_FLAG_BINDS: u32 = 0x01;

/// The data version of the original build (`__VER`): rows with a larger `ver` are skipped.
pub const DATA_VER: i32 = 15;

/// A skill row (propSkill.txt; the columns reinterpret as in ref/17 §7.2: `item_kind1` = `JTYPE_*`,
/// `item_kind2` = owning `JOB_*`, `item_kind3` = `DIS_*`, `req_dis_lv` = required level, `expert_max` =
/// max skill level, `sub_define` = first `SA_` row, `combo_style` = `CT_*`).
pub type SkillProp = ItemProp;
/// A party skill row (propTroupeSkill.txt, ids `ST_*`).
pub type PartySkillProp = ItemProp;

/// One row of propItem / propSkill / propTroupeSkill (124 fields; the column number is the field
/// position in the row). Values are raw: an `=` cell keeps [`NULL_ID`] (or `-1.0`) unless a load-time
/// fix-up (Plan/14 §5.4) says otherwise. Members of the C struct that are never loaded (`dwMotion`,
/// `dwType`, `dwAI`, `dwHP`, `dwCtrlKind1..3`, `dwSfxCtrl`, `dwSndDamage`, `nItemResistDark`) are
/// always 0 and are omitted: code ported from C++ reads `0`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct ItemProp {
    // c0..c2 identity
    /// c0: keep the row iff `ver <= 15`.
    pub ver: i32,
    /// c1: `II_`/`SI_`/`ST_` id.
    pub id: ItemId,
    /// c2: display name (IDS key resolved), `TrimRight`ed.
    pub name: String,
    /// The IDS key of the name (debug/export).
    pub name_key: Option<String>,
    // c3..c27 kinds, ownership, economy, parts
    /// c3: default stack count when created.
    pub num: u32,
    /// c4: maximum stack; `> 1` is forced to `gameplay.max_pack_item` (F2).
    pub pack_max: u32,
    /// c5: `IK1_*` (skills: `JTYPE_*`).
    pub item_kind1: u32,
    /// c6: `IK2_*` (skills: the owning `JOB_*`).
    pub item_kind2: u32,
    /// c7: `IK3_*` (skills: `DIS_*`).
    pub item_kind3: u32,
    /// c8: `JOB_*`.
    pub item_job: u32,
    /// c9: `bPermanence` (cannot be consumed).
    pub permanence: i32,
    /// c10: `dwUseable`.
    pub useable: u32,
    /// c11: `SEX_MALE` 0 / `SEX_FEMALE` 1 / -1.
    pub item_sex: u32,
    /// c12: base price (F3 overrides CS/pet items, F6 `AddShopItem`).
    pub cost: u32,
    /// c13: durability.
    pub endurance: u32,
    /// c14: `nAbrasion`.
    pub abrasion: i32,
    /// c15: `nMaxRepair` (propSkill header: nHardness).
    pub max_repair: i32,
    /// c16: `HD_*`.
    pub handed: u32,
    /// c17: `IP_FLAG_*` bits (F3: `=` becomes 0, then the BINDS flag).
    pub flag: u32,
    /// c18: `PARTS_*`.
    pub parts: u32,
    /// c19: `dwPartsub`.
    pub partsub: u32,
    /// c20: `bPartsFile`.
    pub parts_file: u32,
    /// c21: `dwExclusive`.
    pub exclusive: u32,
    /// c22: `dwBasePartsIgnore`.
    pub base_parts_ignore: u32,
    /// c23: item level.
    pub item_lv: u32,
    /// c24: rarity (sort key of the kind index, unique ranges).
    pub item_rare: u32,
    /// c25: `dwShopAble`.
    pub shop_able: u32,
    /// c26: `nLog`.
    pub log: i32,
    /// c27: `bCharged`, normalised to `1` or `0` (F2).
    pub charged: i32,
    // c28..c52 combat
    /// c28.
    pub link_kind_bullet: u32,
    /// c29.
    pub link_kind: u32,
    /// c30.
    pub ability_min: u32,
    /// c31.
    pub ability_max: u32,
    /// c32: element (`_NONE` 0, `_FIRE` 1, ... defineAttribute.h).
    pub item_type: u32,
    /// c33: `wItemEatk` (short).
    pub item_eatk: i16,
    /// c34.
    pub parry: u32,
    /// c35.
    pub block_rating: u32,
    /// c36.
    pub add_skill_min: i32,
    /// c37.
    pub add_skill_max: i32,
    /// c38: `AS_*`.
    pub atk_style: u32,
    /// c39: `WT_*`.
    pub weapon_type: u32,
    /// c40..c43.
    pub item_atk_order: [u32; 4],
    /// c44: `tmContinuousPain`.
    pub continuous_pain: u32,
    /// c45.
    pub shell_quantity: i32,
    /// c46.
    pub recoil: u32,
    /// c47.
    pub loading_time: u32,
    /// c48.
    pub adj_hit_rate: i32,
    /// c49 (FLOAT, e.g. 0.085).
    pub attack_speed: f32,
    /// c50.
    pub dmg_shift: u32,
    /// c51: `AR_*`.
    pub attack_range: u32,
    /// c52.
    pub probability: i32,
    // c53..c67 parameters
    /// c53..c55: `DST_*`.
    pub dest_param: [u32; 3],
    /// c56..c58.
    pub adj_param_val: [i32; 3],
    /// c59..c61.
    pub chg_param_val: [u32; 3],
    /// c62..c64.
    pub dest_data1: [i32; 3],
    /// c65.
    pub active_skill: u32,
    /// c66.
    pub active_skill_lv: u32,
    /// c67.
    pub active_skill_rate: u32,
    // c68..c97 skill / use
    /// c68.
    pub req_mp: u32,
    /// c69.
    pub req_fp: u32,
    /// c70: skills: required character level (sort key of the job skill list).
    pub req_dis_lv: u32,
    /// c71: prerequisite skill.
    pub re_skill1: u32,
    /// c72.
    pub re_skill_level1: u32,
    /// c73.
    pub re_skill2: u32,
    /// c74.
    pub re_skill_level2: u32,
    /// c75.
    pub skill_ready_type: u32,
    /// c76: also `GetCoolTime()`.
    pub skill_ready: u32,
    /// c77.
    pub skill_range: u32,
    /// c78.
    pub sfx_elemental: u32,
    /// c79..c83: `XI_*`.
    pub sfx_obj: [u32; 5],
    /// c84: `MTI_*`.
    pub use_motion: u32,
    /// c85.
    pub circle_time: u32,
    /// c86.
    pub skill_time: u32,
    /// c87: `EXT_*`.
    pub exe_target: u32,
    /// c88: `WUI_*`.
    pub use_chance: u32,
    /// c89: `SRO_*`.
    pub spell_region: u32,
    /// c90.
    pub spell_type: u32,
    /// c91: e.g. `WEAPON_ULTIMATE`, `PET_VIS`.
    pub refer_stat1: u32,
    /// c92.
    pub refer_stat2: u32,
    /// c93.
    pub refer_target1: u32,
    /// c94.
    pub refer_target2: u32,
    /// c95: also overwritten by `AddShopItem` for non-penya vendors (F6).
    pub refer_value1: u32,
    /// c96.
    pub refer_value2: u32,
    /// c97: `KT_*`.
    pub skill_type: u32,
    // c98..c102 resist (FLOAT x100 -> i32); dark is never loaded and stays 0 (omitted)
    /// c98: `(int)(GetFloat() * 100.0f)`.
    pub item_resist_electricity: i32,
    /// c99.
    pub item_resist_fire: i32,
    /// c100.
    pub item_resist_wind: i32,
    /// c101.
    pub item_resist_water: i32,
    /// c102.
    pub item_resist_earth: i32,
    // c103..c119 misc
    /// c103.
    pub evildoing: i32,
    /// c104.
    pub expert_lv: u32,
    /// c105: skills: max skill level.
    pub expert_max: u32,
    /// c106: skills: first `SA_` id of the add-skill rows.
    pub sub_define: u32,
    /// c107.
    pub exp: u32,
    /// c108: `CT_*`.
    pub combo_style: u32,
    /// c109 (FLOAT; may be patched by except.txt).
    pub flight_speed: f32,
    /// c110 (FLOAT).
    pub flight_lr_angle: f32,
    /// c111 (FLOAT).
    pub flight_tb_angle: f32,
    /// c112.
    pub flight_limit: u32,
    /// c113.
    pub f_fuel_re_max: u32,
    /// c114.
    pub a_fuel_re_max: u32,
    /// c115.
    pub fuel_re: u32,
    /// c116.
    pub limit_level1: u32,
    /// c117.
    pub reflect: i32,
    /// c118: `SND_*`.
    pub snd_attack1: u32,
    /// c119.
    pub snd_attack2: u32,
    // c120..c123 texts
    /// c120: icon file (3 tokens `"""x.dds"""`, the middle one; client-only in C, always loaded here).
    pub icon: String,
    /// c121.
    pub quest_id: u32,
    /// c122: 3 tokens; GM items hold commands here (`/ud /noud`).
    pub text_file: String,
    /// c123: `szCommand` token (IDS resolved), the description (client-only in C).
    pub description: String,
    /// The IDS key of the description.
    pub description_key: Option<String>,
}

impl ItemProp {
    /// `IsUltimate()`: an ultimate weapon (`IK2_WEAPON_DIRECT`/`IK2_WEAPON_MAGIC` with
    /// `refer_stat1 == WEAPON_ULTIMATE`).
    pub fn is_ultimate(&self) -> bool {
        (self.item_kind2 == itemkind::IK2_WEAPON_DIRECT
            || self.item_kind2 == itemkind::IK2_WEAPON_MAGIC)
            && self.refer_stat1 == itemkind::WEAPON_ULTIMATE
    }

    /// `IsVisPet()`: a vis pet (`IK3_PET` with `refer_stat1 == PET_VIS`).
    pub fn is_vis_pet(&self) -> bool {
        self.item_kind3 == itemkind::IK3_PET && self.refer_stat1 == itemkind::PET_VIS
    }

    /// `IsVis()`: a vis item (`IK3_VIS`).
    pub fn is_vis(&self) -> bool {
        self.item_kind3 == itemkind::IK3_VIS
    }

    /// `GetCoolTime()`: the cool-down (`skill_ready`).
    pub fn cool_time(&self) -> u32 {
        self.skill_ready
    }

    /// `true` when `item_kind3` is set (not `NULL_ID`).
    pub fn has_kind3(&self) -> bool {
        self.item_kind3 != NULL_ID
    }
}

/// `true` for the movers whose speed is multiplied (`AII_PET`/`AII_EGG`, `__IMPROVED_PETS`); used by the
/// mover loader, kept here next to the other `neuz.h` constants the props need.
pub(crate) fn is_pet_ai(ai: u32) -> bool {
    ai == neuz::AII_PET || ai == neuz::AII_EGG
}

#[cfg(feature = "load")]
pub(crate) use parse::parse_item_file;

#[cfg(feature = "load")]
mod parse {
    use super::{DATA_VER, ItemProp};
    use crate::options::DataOptions;
    use flyff_script::{DiagCode, Diagnostics, ScannerOptions, Script, ScriptCtx, Severity};
    use std::sync::Arc;

    /// The rows of one prop file in file order.
    #[derive(Debug, Default)]
    pub(crate) struct ItemFile {
        /// Rows with `ver <= 15`, fix-up F2 applied.
        pub rows: Vec<ItemProp>,
        /// A row with `id == 0` ended the file early (`return FALSE`).
        pub stopped_at_zero_id: bool,
        /// Rows skipped because `ver > 15`.
        pub skipped_ver: u32,
    }

    /// `CString::TrimRight()`: removes trailing C whitespace.
    fn trim_right(s: &str) -> &str {
        s.trim_end_matches([' ', '\t', '\n', '\u{b}', '\u{c}', '\r'])
    }

    /// `(int)(GetFloat() * 100.0f)`: f32 multiplication, then truncation toward zero.
    pub(super) fn resist(sc: &mut Script<'_>) -> i32 {
        (sc.get_float() * 100.0f32) as i32
    }

    /// `CProject::LoadPropItem` over decoded `text`.
    pub(crate) fn parse_item_file(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        opts: &DataOptions,
        diags: &mut Diagnostics,
    ) -> ItemFile {
        let mut out = ItemFile::default();
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        let mut ver = sc.get_number_i32();
        let mut last = (0u32, String::new());
        while !sc.finished() {
            let id = sc.get_number();
            if id == 0 {
                let msg = format!("{file} ID:{id} last_read(ID:{} name:{})", last.0, last.1);
                sc.report(Severity::Error, DiagCode::PropIdInvalid, msg);
                out.stopped_at_zero_id = true;
                break;
            }
            let (_, name_key) = sc.get_token_with_key();
            let name = trim_right(sc.token()).to_owned();
            last = (id, name.clone());
            let num = sc.get_number();
            let mut pack_max = sc.get_number();
            if pack_max > 1 {
                pack_max = opts.max_pack_item; // __MAX_PACK_ITEM
            }
            let mut p = ItemProp {
                ver,
                id,
                name,
                name_key,
                num,
                pack_max,
                item_kind1: sc.get_number(),
                item_kind2: sc.get_number(),
                item_kind3: sc.get_number(),
                item_job: sc.get_number(),
                permanence: sc.get_number_i32(),
                useable: sc.get_number(),
                item_sex: sc.get_number(),
                cost: sc.get_number(),
                endurance: sc.get_number(),
                abrasion: sc.get_number_i32(),
                max_repair: sc.get_number_i32(),
                handed: sc.get_number(),
                flag: sc.get_number(),
                parts: sc.get_number(),
                partsub: sc.get_number(),
                parts_file: sc.get_number(),
                exclusive: sc.get_number(),
                base_parts_ignore: sc.get_number(),
                item_lv: sc.get_number(),
                item_rare: sc.get_number(),
                shop_able: sc.get_number(),
                log: sc.get_number_i32(),
                charged: i32::from(sc.get_number_i32() == 1),
                link_kind_bullet: sc.get_number(),
                link_kind: sc.get_number(),
                ability_min: sc.get_number(),
                ability_max: sc.get_number(),
                item_type: sc.get_number(),
                item_eatk: sc.get_number() as i16, // static_cast<short>
                parry: sc.get_number(),
                block_rating: sc.get_number(),
                add_skill_min: sc.get_number_i32(),
                add_skill_max: sc.get_number_i32(),
                atk_style: sc.get_number(),
                weapon_type: sc.get_number(),
                item_atk_order: [
                    sc.get_number(),
                    sc.get_number(),
                    sc.get_number(),
                    sc.get_number(),
                ],
                continuous_pain: sc.get_number(),
                shell_quantity: sc.get_number_i32(),
                recoil: sc.get_number(),
                loading_time: sc.get_number(),
                adj_hit_rate: sc.get_number_i32(),
                attack_speed: sc.get_float(),
                dmg_shift: sc.get_number(),
                attack_range: sc.get_number(),
                probability: sc.get_number_i32(),
                dest_param: [sc.get_number(), sc.get_number(), sc.get_number()],
                adj_param_val: [
                    sc.get_number_i32(),
                    sc.get_number_i32(),
                    sc.get_number_i32(),
                ],
                chg_param_val: [sc.get_number(), sc.get_number(), sc.get_number()],
                dest_data1: [
                    sc.get_number_i32(),
                    sc.get_number_i32(),
                    sc.get_number_i32(),
                ],
                active_skill: sc.get_number(),
                active_skill_lv: sc.get_number(),
                active_skill_rate: sc.get_number(),
                req_mp: sc.get_number(),
                req_fp: sc.get_number(),
                req_dis_lv: sc.get_number(),
                re_skill1: sc.get_number(),
                re_skill_level1: sc.get_number(),
                re_skill2: sc.get_number(),
                re_skill_level2: sc.get_number(),
                skill_ready_type: sc.get_number(),
                skill_ready: sc.get_number(),
                skill_range: sc.get_number(),
                sfx_elemental: sc.get_number(),
                sfx_obj: [
                    sc.get_number(),
                    sc.get_number(),
                    sc.get_number(),
                    sc.get_number(),
                    sc.get_number(),
                ],
                use_motion: sc.get_number(),
                circle_time: sc.get_number(),
                skill_time: sc.get_number(),
                exe_target: sc.get_number(),
                use_chance: sc.get_number(),
                spell_region: sc.get_number(),
                spell_type: sc.get_number(),
                refer_stat1: sc.get_number(),
                refer_stat2: sc.get_number(),
                refer_target1: sc.get_number(),
                refer_target2: sc.get_number(),
                refer_value1: sc.get_number(),
                refer_value2: sc.get_number(),
                skill_type: sc.get_number(),
                item_resist_electricity: resist(&mut sc),
                item_resist_fire: resist(&mut sc),
                item_resist_wind: resist(&mut sc),
                item_resist_water: resist(&mut sc),
                item_resist_earth: resist(&mut sc),
                evildoing: sc.get_number_i32(),
                expert_lv: sc.get_number(),
                expert_max: sc.get_number(),
                sub_define: sc.get_number(),
                exp: sc.get_number(),
                combo_style: sc.get_number(),
                flight_speed: sc.get_float(),
                flight_lr_angle: sc.get_float(),
                flight_tb_angle: sc.get_float(),
                flight_limit: sc.get_number(),
                f_fuel_re_max: sc.get_number(),
                a_fuel_re_max: sc.get_number(),
                fuel_re: sc.get_number(),
                limit_level1: sc.get_number(),
                reflect: sc.get_number_i32(),
                snd_attack1: sc.get_number(),
                snd_attack2: sc.get_number(),
                ..ItemProp::default()
            };
            // c120 szIcon: three tokens, the middle one is the file name.
            sc.get_token();
            sc.get_token();
            p.icon = sc.token().to_owned();
            sc.get_token();
            // c121 dwQuestId
            p.quest_id = sc.get_number();
            // c122 szTextFileName: three tokens.
            sc.get_token();
            sc.get_token();
            p.text_file = sc.token().to_owned();
            sc.get_token();
            // c123 szCommand
            let (_, key) = sc.get_token_with_key();
            p.description = sc.token().to_owned();
            p.description_key = key;

            if ver <= DATA_VER {
                out.rows.push(p);
            } else {
                out.skipped_ver += 1;
            }
            ver = sc.get_number_i32(); // version of the next row
        }
        out
    }
}
