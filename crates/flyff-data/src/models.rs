//! The model registry: `mdlObj.inc` and `mdlDyna.inc` (`CModelMng::LoadScript`, Plan/14 §6.10;
//! src:_Common/ModelMng.cpp:5-155, 314-531; ref/18 §10).
//!
//! # File grammar
//!
//! ```text
//! <typeName> <iType> {                  // "obj" 0, "ctrl" 2, "sfx" 3, "item" 4, "mvr" 5, "region" 6, "ship" 7
//!    <folderName> { ... }               // nested folders (a name followed by '{'), purely organisational
//!    <name> <index> <modelType> <part> <bFly> <distant> <bPick> <fScale> <bTrans> <bShadow> <textureEx> <renderFlag>
//!        [ { <motionFile> <motionId> ... } ]   // optional motion list
//! }
//! ```
//!
//! The original's `MODELELEM` keeps `m_bFly`, `m_dwDistant` (2 bits), `m_bPick`, `m_bTrans`, `m_bShadow` and
//! `m_bRenderFlag` in bit fields, so the values are truncated to their width (`fly`, `pick`, `trans`,
//! `shadow`, `render_flag` keep the low bit, `distant` the low two bits). The world server skipped the `sfx`
//! entries; this registry keeps them (the client bundle needs them).
//!
//! # File-name builders
//!
//! [`ModelRegistry::model_file`], [`bone_file`](ModelRegistry::bone_file),
//! [`motion_file`](ModelRegistry::motion_file) and [`parts_file`](ModelRegistry::parts_file) are the exact
//! ports of `MakeModelName`, `MakeBoneName`, `MakeMotionName` and `MakePartsName`. They return the name as
//! built (mixed case); the VFS is case-insensitive. An item without a model uses the model of
//! `II_ARM_M_VAG_HELMET02` (`GetModelElem`).

use crate::types::{IdTable, MotionId};
use flyff_common::defs::define::{
    MAX_OBJTYPE, MODELTYPE_BILLBOARD, MODELTYPE_SFX, OT_ITEM, OT_SFX, SEX_MALE, SEX_SEXLESS,
};
use flyff_common::defs::item::II_ARM_M_VAG_HELMET02;
use flyff_common::defs::neuz::MTI_STAND;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// `g_szRoot[type]`: the file-name prefix of each object type.
const ROOTS: [&str; MAX_OBJTYPE as usize] = [
    "obj", "ani", "ctrl", "sfx", "item", "mvr", "region", "obj", "path",
];

/// The file-name prefix of an object type (`g_szRoot`), `None` for an unknown type.
pub fn type_root(obj_type: u32) -> Option<&'static str> {
    ROOTS.get(usize::try_from(obj_type).ok()?).copied()
}

/// The `mdlObj.inc` / `mdlDyna.inc` entries, one table per object type (`m_aaModelElem`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelRegistry {
    /// `OT_OBJ` 0, `OT_CTRL` 2, `OT_SFX` 3, `OT_ITEM` 4, `OT_MOVER` 5, `OT_REGION` 6, `OT_SHIP` 7.
    pub by_type: BTreeMap<u32, IdTable<ModelElem>>,
}

/// One model entry (`MODELELEM`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelElem {
    /// The object type (`m_dwType`).
    pub obj_type: u32,
    /// The index within the type (`m_dwIndex`): `MI_*`, `II_*`, `CI_*`, `XI_*`, or the obj number.
    pub index: u32,
    /// `m_szName` (at most 47 bytes).
    pub name: String,
    /// `m_dwModelType`: `MODELTYPE_MESH` 1, `ANIMATED_MESH` 2, `BILLBOARD` 3, `SFX` 4, `ASE` 5.
    pub model_type: u32,
    /// `m_szPart`: the part base name, `"male/female"` split by `/`.
    pub part: String,
    /// `m_bFly` (1 bit).
    pub fly: bool,
    /// `m_dwDistant` (2 bits): `MD_FAR` 0, `MD_MID` 1, `MD_NEAR` 2, `MD_FIX` 3.
    pub distant: u32,
    /// `m_bPick` (1 bit).
    pub pick: bool,
    /// `m_fScale`: the default object scale.
    pub scale: f32,
    /// `m_bTrans` (1 bit): the object fades when it hides the player.
    pub trans: bool,
    /// `m_bShadow` (1 bit).
    pub shadow: bool,
    /// `m_nTextureEx`: `ATEX_NONE` 0, `ATEX_USE` 1, `ATEX_01..07`.
    pub texture_ex: u32,
    /// `m_bRenderFlag` (1 bit).
    pub render_flag: u32,
    /// `m_nMax`: the highest listed motion id + 1 (0 when the entry has no motion list). Empty entries
    /// count.
    pub motion_max: u32,
    /// The used motion slots: `MTI_*` id to file stem (at most 31 bytes), ascending id, no empty names.
    pub motions: Vec<(MotionId, String)>,
    /// The organisational folders this entry was nested in (informational).
    pub folder_path: Vec<String>,
}

impl ModelElem {
    /// The file stem of motion slot `mti` (`GetMotion`); `None` when the slot is unused.
    pub fn motion(&self, mti: MotionId) -> Option<&str> {
        let i = self.motions.binary_search_by_key(&mti, |(m, _)| *m).ok()?;
        self.motions.get(i).map(|(_, s)| s.as_str())
    }
}

impl ModelRegistry {
    /// The raw entry (no fallback).
    pub fn get(&self, obj_type: u32, index: u32) -> Option<&ModelElem> {
        self.by_type.get(&obj_type)?.get(index)
    }

    /// `GetModelElem`: the entry, or for a missing **item** model the model of
    /// `II_ARM_M_VAG_HELMET02` (a plain armour shape instead of the money model).
    pub fn elem(&self, obj_type: u32, index: u32) -> Option<&ModelElem> {
        match self.get(obj_type, index) {
            Some(e) => Some(e),
            None if obj_type == OT_ITEM => self.get(OT_ITEM, II_ARM_M_VAG_HELMET02),
            None => None,
        }
    }

    /// Number of entries of one type.
    pub fn len_of(&self, obj_type: u32) -> usize {
        self.by_type.get(&obj_type).map_or(0, IdTable::len)
    }

    /// Number of entries of all types.
    pub fn len(&self) -> usize {
        self.by_type.values().map(IdTable::len).sum()
    }

    /// `true` when nothing is registered.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// `MakeModelName`: `name` for billboards; for `OT_SFX` the name itself when it contains `_`, else
    /// `sfx_<name>`; otherwise `<root>_<name>`. `.o3d` is appended unless the model type is
    /// `MODELTYPE_SFX` (those name an `.sfx` effect).
    pub fn model_file(&self, obj_type: u32, index: u32) -> Option<String> {
        let e = self.elem(obj_type, index)?;
        if e.model_type == MODELTYPE_BILLBOARD {
            return Some(e.name.clone());
        }
        let mut s = if obj_type == OT_SFX && e.name.contains('_') {
            e.name.clone()
        } else {
            format!("{}_{}", type_root(obj_type)?, e.name)
        };
        if e.model_type != MODELTYPE_SFX {
            s.push_str(".o3d");
        }
        Some(s)
    }

    /// `MakeBoneName`: `<root>_<name>.chr`.
    pub fn bone_file(&self, obj_type: u32, index: u32) -> Option<String> {
        let e = self.elem(obj_type, index)?;
        Some(format!("{}_{}.chr", type_root(obj_type)?, e.name))
    }

    /// `MakeMotionName`: the `.ani` file of motion `mti`.
    ///
    /// * `mti >= m_nMax` or `NULL_ID` (or a value the original would read out of range) is `MTI_STAND`.
    /// * The result is `<root>_` + (`<name>_` unless the slot text contains `_`: a *borrowed clip* such as
    ///   `"BangBig_walk"`) + the slot text + `.ani`.
    /// * An **unused slot** falls back to the `MTI_STAND` text, but (as in the original, which decides the
    ///   prefix before the fallback) the `<name>_` prefix is still added.
    /// * An entry without a motion list has no motions: `None` (the original dereferences NULL).
    pub fn motion_file(&self, obj_type: u32, index: u32, mti: MotionId) -> Option<String> {
        let e = self.elem(obj_type, index)?;
        if e.motion_max == 0 {
            return None;
        }
        let mti = match i32::try_from(mti) {
            Ok(m) if i64::from(m) < i64::from(e.motion_max) => mti,
            _ => MTI_STAND,
        };
        let slot = e.motion(mti).unwrap_or("");
        let mut s = format!("{}_", type_root(obj_type)?);
        if !slot.contains('_') {
            s.push_str(&e.name);
            s.push('_');
        }
        let used = if slot.is_empty() {
            e.motion(MTI_STAND).unwrap_or("")
        } else {
            slot
        };
        s.push_str(used);
        s.push_str(".ani");
        Some(s)
    }

    /// `MakePartsName(root, item_idx, sex)`: `<root>_<part>.o3d` of an item's `m_szPart`. For
    /// `SEX_SEXLESS` (and `u32::MAX` = -1) the whole string is used; otherwise a `male/female` part is cut
    /// at the first `/` (male before it, female after it) and a part without `/` is used whole.
    pub fn parts_file(&self, root: &str, item_idx: u32, sex: u32) -> Option<String> {
        let e = self.elem(OT_ITEM, item_idx)?;
        let part = if sex == SEX_SEXLESS || sex == u32::MAX {
            e.part.as_str()
        } else {
            match e.part.split_once('/') {
                None => e.part.as_str(),
                Some((male, female)) => {
                    if sex == SEX_MALE {
                        male
                    } else {
                        female
                    }
                }
            }
        };
        Some(format!("{root}_{part}.o3d"))
    }

    /// Adds `elem` (last wins). Returns the replaced entry, or the element back when its index is above
    /// [`IdTable::MAX_ID`].
    pub fn insert(&mut self, elem: ModelElem) -> Result<Option<ModelElem>, Box<ModelElem>> {
        let table = self.by_type.entry(elem.obj_type).or_default();
        let index = elem.index;
        table.insert(index, elem).map_err(Box::new)
    }
}

/// The name buffer sizes of `MODELELEM` (`m_szName[48]`, `m_szPart[48]`, 32-byte motion slots).
pub const NAME_MAX: usize = 47;
/// Longest motion file stem a 32-byte slot holds.
pub const MOTION_NAME_MAX: usize = 31;

#[cfg(feature = "load")]
pub(crate) use parse::parse_model_file;

#[cfg(feature = "load")]
mod parse {
    use super::{MOTION_NAME_MAX, ModelElem, NAME_MAX};
    use flyff_common::defs::define::MAX_OBJTYPE;
    use flyff_script::{DiagCode, Diagnostics, ScannerOptions, Script, ScriptCtx, Severity};
    use std::collections::BTreeMap;
    use std::sync::Arc;

    fn too_long(sc: &mut Script<'_>, what: &str, s: &str, max: usize) {
        if s.len() > max {
            let msg = format!(
                "{what} `{s}` is longer than {max} bytes (the original overflows its buffer)"
            );
            sc.report(Severity::Warning, DiagCode::ValueOutOfRange, msg);
        }
    }

    /// Reads the motion list after its `{`: `"file" MTI`... up to the closing `}`. Returns the highest id
    /// + 1 and the slots (an empty name clears a slot; a repeated id replaces it).
    fn read_motions(sc: &mut Script<'_>, owner: &str) -> (u32, BTreeMap<u32, String>) {
        let mut slots: BTreeMap<u32, String> = BTreeMap::new();
        let mut n_max: i32 = 0;
        sc.get_token(); // motion name or }
        while !sc.token().starts_with('}') && !sc.finished() {
            let name = sc.token().to_owned();
            let raw = sc.get_number();
            // `(int)iMotion > nMax`; a negative id reaches `GetMotion`, which clamps it to slot 0
            let id = if (raw as i32) < 0 { 0 } else { raw };
            if (raw as i32) < 0 {
                let msg = format!(
                    "{owner}: motion id {} is negative; stored in slot 0",
                    raw as i32
                );
                sc.report(Severity::Warning, DiagCode::ValueOutOfRange, msg);
            }
            if (id as i32) > n_max {
                n_max = id as i32;
            }
            too_long(sc, "motion file", &name, MOTION_NAME_MAX);
            if name.is_empty() {
                slots.remove(&id);
            } else if slots.insert(id, name.clone()).is_some() {
                let msg = format!("{owner}: motion id {id} listed twice ({name}); the last wins");
                sc.report(Severity::Warning, DiagCode::ModelDuplicate, msg);
            }
            sc.get_token(); // motion name or }
        }
        (n_max.max(0) as u32 + 1, slots)
    }

    /// `CModelMng::LoadScript` over decoded `text`: the entries in file order. Entries of an object type
    /// `>= MAX_OBJTYPE` are reported (Error) and dropped, a truncated file ends the parse.
    pub(crate) fn parse_model_file(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> Vec<ModelElem> {
        let mut out = Vec::new();
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        sc.get_token(); // subject or FINISHED
        while !sc.finished() {
            let obj_type = sc.get_number();
            let type_ok = obj_type < MAX_OBJTYPE;
            if !type_ok {
                sc.report(
                    Severity::Error,
                    DiagCode::IndexOutOfRange,
                    format!("object type {obj_type} is not below MAX_OBJTYPE ({MAX_OBJTYPE}); its entries are ignored"),
                );
            }
            sc.get_token(); // {
            sc.get_token(); // object name or }
            let mut brace = 1u32;
            let mut folders: Vec<String> = Vec::new();
            while brace > 0 {
                if sc.finished() {
                    sc.report(
                        Severity::Warning,
                        DiagCode::PropRowSkipped,
                        "the model script ends inside a block".to_owned(),
                    );
                    return out;
                }
                if sc.token().starts_with('}') {
                    brace -= 1;
                    if brace > 0 {
                        folders.pop();
                        sc.get_token(); // object name or }
                    }
                    continue;
                }
                let name = sc.token().to_owned();
                let mark = sc.mark();
                sc.get_token(); // '{' of a folder
                if sc.token().starts_with('{') {
                    brace += 1;
                    folders.push(name);
                    sc.get_token(); // object name or }
                    continue;
                }
                sc.reset(mark);
                let index = sc.get_number();
                if index == 0 {
                    sc.report(
                        Severity::Info,
                        DiagCode::ValueOutOfRange,
                        format!("model `{name}` has index 0"),
                    );
                }
                too_long(&mut sc, "model name", &name, NAME_MAX);
                let model_type = sc.get_number();
                sc.get_token();
                let part = sc.token().to_owned();
                too_long(&mut sc, "model part", &part, NAME_MAX);
                // bit fields: m_bFly:1, m_dwDistant:2, m_bPick:1, m_bTrans:1, m_bShadow:1, m_bRenderFlag:1
                let fly = sc.get_number() & 1 != 0;
                let distant = sc.get_number() & 3;
                let pick = sc.get_number() & 1 != 0;
                let scale = sc.get_float();
                let trans = sc.get_number() & 1 != 0;
                let shadow = sc.get_number() & 1 != 0;
                let texture_ex = sc.get_number();
                let render_flag = sc.get_number() & 1;
                sc.get_token(); // object name or { or }
                let (motion_max, motions) = if sc.token().starts_with('{') {
                    let owner = format!("{name} ({index})");
                    let (max, slots) = read_motions(&mut sc, &owner);
                    sc.get_token(); // object name or }
                    (max, slots.into_iter().collect())
                } else {
                    (0, Vec::new())
                };
                if type_ok {
                    out.push(ModelElem {
                        obj_type,
                        index,
                        name,
                        model_type,
                        part,
                        fly,
                        distant,
                        pick,
                        scale,
                        trans,
                        shadow,
                        texture_ex,
                        render_flag,
                        motion_max,
                        motions,
                        folder_path: folders.clone(),
                    });
                }
            }
            sc.get_token(); // type name or }
        }
        out
    }
}
