//! `MoverProp` and `LoadPropMover` (Plan/14 §6.5.1, ref/17 §7.3; src:_Common/ProjectCmn.cpp:112-309).
//!
//! `propMover.txt` is ASCII, 783 rows, **no version column**; each row is exactly 83 tokens (a negative
//! value adds a `-` token). Ids are `MI_*` defines (10..1165); `MAX_PROPMOVER` = 2000.

use crate::mover_ex::MoverEx;
use crate::types::MoverId;
use serde::{Deserialize, Serialize};

/// `MAX_PROPMOVER`: the id bound of `propMover.txt` (an id outside `[0, 2000)` fails the load).
pub const MAX_PROPMOVER: u32 = 2000;

/// One row of `propMover.txt` (token index `tN` = position in the row).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct MoverProp {
    /// t0: `MI_*`.
    pub id: MoverId,
    /// t1: display name (IDS key resolved).
    pub name: String,
    /// The IDS key of the name (debug/export).
    pub name_key: Option<String>,
    /// t2: `AII_*`.
    pub ai: u32,
    /// t3.
    pub str: u32,
    /// t4.
    pub sta: u32,
    /// t5.
    pub dex: u32,
    /// t6.
    pub int: u32,
    /// t7.
    pub hr: u32,
    /// t8.
    pub er: u32,
    /// t9.
    pub race: u32,
    /// t10: `BELLI_*`.
    pub belligerence: u32,
    /// t11.
    pub gender: u32,
    /// t12.
    pub level: u32,
    /// t13.
    pub flight_level: u32,
    /// t14.
    pub size: u32,
    /// t15: `RANK_*`.
    pub class: u32,
    /// t16: `bIfParts` (`-1` is an Error only).
    pub if_parts: i32,
    /// t17: `nChaotic` (header "dwKarma").
    pub chaotic: i32,
    /// t18: `dwUseable`.
    pub useable: u32,
    /// t19.
    pub action_radius: u32,
    /// t20.
    pub atk_min: u32,
    /// t21.
    pub atk_max: u32,
    /// t22: `II_WEA_MOB_*` attack item.
    pub atk1: u32,
    /// t23.
    pub atk2: u32,
    /// t24.
    pub atk3: u32,
    /// t25 (header "dwHorizontalRate").
    pub atk4: u32,
    /// t26: FLOAT (header "dwVerticalRate"); `=` (-1) becomes 1.0 (F1).
    pub frame: f32,
    /// t27 (header "dwDiagonalRate").
    pub orthograde: u32,
    /// t28.
    pub thrust_rate: u32,
    /// t29.
    pub chest_rate: u32,
    /// t30.
    pub head_rate: u32,
    /// t31.
    pub arm_rate: u32,
    /// t32.
    pub leg_rate: u32,
    /// t33.
    pub attack_speed: u32,
    /// t34.
    pub re_attack_delay: u32,
    /// t35.
    pub add_hp: u32,
    /// t36.
    pub add_mp: u32,
    /// t37.
    pub natural_armor: u32,
    /// t38.
    pub abrasion: i32,
    /// t39.
    pub hardness: i32,
    /// t40.
    pub adj_atk_delay: u32,
    /// t41: element.
    pub element_type: u32,
    /// t42: `wElementAtk` (short).
    pub element_atk: i16,
    /// t43.
    pub hide_level: u32,
    /// t44: FLOAT; x2.80 for `AII_PET`/`AII_EGG` (F1, f32 multiply).
    pub speed: f32,
    /// t45.
    pub shelter: u32,
    /// t46.
    pub flying: u32,
    /// t47.
    pub jump_ing: u32,
    /// t48.
    pub air_jump: u32,
    /// t49.
    pub taming: u32,
    /// t50.
    pub resist_magic: u32,
    /// t51: `(int)(GetFloat() * 100.0f)`.
    pub resist_electricity: i32,
    /// t52.
    pub resist_fire: i32,
    /// t53.
    pub resist_wind: i32,
    /// t54.
    pub resist_water: i32,
    /// t55 (dark is never loaded).
    pub resist_earth: i32,
    /// t56.
    pub cash: u32,
    /// t57.
    pub source_material: u32,
    /// t58.
    pub material_amount: u32,
    /// t59.
    pub cohesion: u32,
    /// t60: corpse time.
    pub holding_time: u32,
    /// t61.
    pub correction_value: u32,
    /// t62: `GetExpInteger` (int64).
    pub exp_value: i64,
    /// t63.
    pub fxp_value: i32,
    /// t64.
    pub body_state: u32,
    /// t65.
    pub add_ability: u32,
    /// t66.
    pub killable: u32,
    /// t67..t72 read as 3 items (`virt_item`) then 3 types (`virt_type`); unused by the game.
    pub virt_item: [u32; 3],
    /// See `virt_item`.
    pub virt_type: [u32; 3],
    /// t73..t74.
    pub snd_atk: [u32; 2],
    /// t75..t76.
    pub snd_die: [u32; 2],
    /// t77..t79.
    pub snd_dmg: [u32; 3],
    /// t80..t81.
    pub snd_idle: [u32; 2],
    // t82 szComment: read and discarded
    /// `propMoverEx.inc` block and drops (L32/L39; `Default` when there is no block). Never part of a client
    /// bundle (14 §10.6) and not in the generated schema.
    #[cfg_attr(feature = "schema", schemars(skip))]
    pub ex: MoverEx,
}

/// `__IMPROVED_PETS`: the movers whose speed gets the pet factor.
pub fn speed_factor_applies(ai: u32) -> bool {
    crate::props::item::is_pet_ai(ai)
}

#[cfg(feature = "load")]
pub(crate) use parse::parse_mover_file;

#[cfg(feature = "load")]
mod parse {
    use super::{MAX_PROPMOVER, MoverProp, speed_factor_applies};
    use crate::mover_ex::MoverEx;
    use crate::options::DataOptions;
    use crate::types::NULL_ID;
    use flyff_script::{DiagCode, Diagnostics, ScannerOptions, Script, ScriptCtx, Severity};
    use std::collections::BTreeSet;
    use std::sync::Arc;

    /// The rows of `propMover.txt` in file order.
    #[derive(Debug, Default)]
    pub(crate) struct MoverFile {
        /// All rows with a non-zero id.
        pub rows: Vec<MoverProp>,
    }

    /// `LoadPropMover` returned `FALSE`: the server aborts start-up (Fatal).
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct MoverLoadError {
        /// `PropIdOutOfRange` or `PropIdDuplicate`.
        pub code: DiagCode,
        /// Human-readable message with the file position.
        pub message: String,
        /// 1-based line of the offending id.
        pub line: u32,
    }

    /// `abs(-1.0F - f) < 0.000001F`: the `-1.0` of an `=` cell.
    fn is_minus_one(v: f32) -> bool {
        (-1.0f32 - v).abs() < 0.000_001f32
    }

    /// `(int)(GetFloat() * 100.0f)`.
    fn resist(sc: &mut Script<'_>) -> i32 {
        (sc.get_float() * 100.0f32) as i32
    }

    /// `CProject::LoadPropMover` over decoded `text`.
    pub(crate) fn parse_mover_file(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        opts: &DataOptions,
        diags: &mut Diagnostics,
    ) -> Result<MoverFile, MoverLoadError> {
        let mut out = MoverFile::default();
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        let mut seen: BTreeSet<i32> = BTreeSet::new();
        let mut last_name = String::new();
        loop {
            let id = sc.get_number_i32();
            if sc.finished() {
                break;
            }
            if id == 0 {
                continue; // the original `continue`s (it would desynchronise the stream)
            }
            let line = sc.line();
            if id < 0 || id as u32 >= MAX_PROPMOVER {
                return Err(MoverLoadError {
                    code: DiagCode::PropIdOutOfRange,
                    message: format!("too many mover property or invalid id:{id} FILE:{file}"),
                    line,
                });
            }
            if !seen.insert(id) {
                return Err(MoverLoadError {
                    code: DiagCode::PropIdDuplicate,
                    message: format!(
                        "last read:{last_name}, ID:{id} is duplicated (column shifted?) FILE:{file}"
                    ),
                    line,
                });
            }
            let (_, name_key) = sc.get_token_with_key();
            let name = sc.token().to_owned();
            last_name.clone_from(&name);
            let ai = sc.get_number();
            let mut p = MoverProp {
                id: id as u32,
                name,
                name_key,
                ai,
                str: sc.get_number(),
                sta: sc.get_number(),
                dex: sc.get_number(),
                int: sc.get_number(),
                hr: sc.get_number(),
                er: sc.get_number(),
                race: sc.get_number(),
                belligerence: sc.get_number(),
                gender: sc.get_number(),
                level: sc.get_number(),
                flight_level: sc.get_number(),
                size: sc.get_number(),
                class: sc.get_number(),
                if_parts: sc.get_number_i32(),
                chaotic: sc.get_number_i32(),
                useable: sc.get_number(),
                action_radius: sc.get_number(),
                atk_min: sc.get_number(),
                atk_max: sc.get_number(),
                atk1: sc.get_number(),
                atk2: sc.get_number(),
                atk3: sc.get_number(),
                atk4: sc.get_number(),
                frame: {
                    let f = sc.get_float();
                    if is_minus_one(f) { 1.0 } else { f }
                },
                orthograde: sc.get_number(),
                thrust_rate: sc.get_number(),
                chest_rate: sc.get_number(),
                head_rate: sc.get_number(),
                arm_rate: sc.get_number(),
                leg_rate: sc.get_number(),
                attack_speed: sc.get_number(),
                re_attack_delay: sc.get_number(),
                add_hp: sc.get_number(),
                add_mp: sc.get_number(),
                natural_armor: sc.get_number(),
                abrasion: sc.get_number_i32(),
                hardness: sc.get_number_i32(),
                adj_atk_delay: sc.get_number(),
                element_type: sc.get_number(),
                element_atk: sc.get_number() as i16, // static_cast<short>
                hide_level: sc.get_number(),
                speed: {
                    let s = sc.get_float();
                    if speed_factor_applies(ai) {
                        s * opts.pet_speed_factor
                    } else {
                        s
                    }
                },
                shelter: sc.get_number(),
                flying: sc.get_number(),
                jump_ing: sc.get_number(),
                air_jump: sc.get_number(),
                taming: sc.get_number(),
                resist_magic: sc.get_number(),
                resist_electricity: resist(&mut sc),
                resist_fire: resist(&mut sc),
                resist_wind: resist(&mut sc),
                resist_water: resist(&mut sc),
                resist_earth: resist(&mut sc),
                cash: sc.get_number(),
                source_material: sc.get_number(),
                material_amount: sc.get_number(),
                cohesion: sc.get_number(),
                holding_time: sc.get_number(),
                correction_value: sc.get_number(),
                exp_value: sc.get_int64(),
                fxp_value: sc.get_number_i32(),
                body_state: sc.get_number(),
                add_ability: sc.get_number(),
                killable: sc.get_number(),
                virt_item: [sc.get_number(), sc.get_number(), sc.get_number()],
                virt_type: [sc.get_number(), sc.get_number(), sc.get_number()],
                snd_atk: [sc.get_number(), sc.get_number()],
                snd_die: [sc.get_number(), sc.get_number()],
                snd_dmg: [sc.get_number(), sc.get_number(), sc.get_number()],
                snd_idle: [sc.get_number(), sc.get_number()],
                ex: MoverEx::default(),
            };
            sc.get_token(); // szComment, discarded
            if p.if_parts as u32 == NULL_ID {
                let msg = format!("LoadPropMover: bIfParts is -1 ({}, id {})", p.name, p.id);
                sc.report(Severity::Error, DiagCode::ValueOutOfRange, msg);
            }
            // `speed` of a pet may overflow to inf only with absurd data; keep as is.
            p.ex = MoverEx::default();
            out.rows.push(p);
        }
        Ok(out)
    }
}
