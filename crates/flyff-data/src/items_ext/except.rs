//! `except.txt` (`LoadExcept`, Plan/14 §6.3, ref/17 §8.6; src:_Common/Project.cpp:4787-4876): per-language
//! patches of item properties and the `worldDrop = 0` exclusion set.
//!
//! ```text
//! { LANG SUBLANG '{' { 'ItemProp' '{' { ITEMID '{' { ATTR '=' VALUE ';' }* '}' }* '}' }* '}' }*
//! ATTR in fFlightSpeed | dwShopAble | dwCircleTime | dwFlag | dwLimitLevel1 | dwSkillReadyType | worldDrop
//! ```
//!
//! Attributes apply only when the block's language equals the server language (`LANG_ENG` 1, sub 0).
//! This data set has **no** such block, so the file has no effect (`patched == 0`, empty set); the
//! loader is complete for data compatibility.

use crate::types::ItemId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// `LANG_ENG`.
pub const SERVER_LANG: i32 = 1;
/// The server sub-language (`SUBLANG_DEFAULT`).
pub const SERVER_SUBLANG: i32 = 0;

/// The result of loading `except.txt` (`m_setExcept` and a patch counter).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemExcept {
    /// `m_setExcept`: items with `worldDrop = 0` (excluded from the world drop events).
    pub world_drop_excluded: BTreeSet<ItemId>,
    /// Number of attribute assignments that were applied to an item.
    pub patched: u32,
}

#[cfg(feature = "load")]
pub(crate) use parse::apply_except;

#[cfg(feature = "load")]
mod parse {
    use super::{ItemExcept, SERVER_LANG, SERVER_SUBLANG};
    use crate::props::item::ItemProp;
    use crate::types::IdTable;
    use flyff_script::{
        DiagCode, Diagnostics, ScannerOptions, Script, ScriptCtx, Severity, c_atoi,
    };
    use std::sync::Arc;

    /// `CProject::LoadExcept`: patches `items` in place and returns the exclusion set. A block for another
    /// language is skipped token by token. A truncated file ends the loop (the original would spin).
    pub(crate) fn apply_except(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        items: &mut IdTable<ItemProp>,
        diags: &mut Diagnostics,
    ) -> ItemExcept {
        let mut out = ItemExcept::default();
        let mut s = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        s.get_token(); // subject or FINISHED
        while !s.finished() {
            let lang = c_atoi(s.token().as_bytes());
            let sublang = s.get_number_i32();
            let matches = lang == SERVER_LANG && sublang == SERVER_SUBLANG;
            s.get_token(); // {
            s.get_token(); // ItemProp & moverProp
            while !s.finished() && !s.token().starts_with('}') {
                if s.is("ItemProp") {
                    s.get_token(); // {
                    s.get_token(); // dwID
                    while !s.finished() && !s.token().starts_with('}') {
                        let id = c_atoi(s.token().as_bytes()) as u32;
                        s.get_token(); // {
                        s.get_token(); // attribute
                        while !s.finished() && !s.token().starts_with('}') {
                            if !matches {
                                s.get_token();
                                continue;
                            }
                            let attr = s.token().to_owned();
                            let known = matches!(
                                attr.as_str(),
                                "fFlightSpeed"
                                    | "dwShopAble"
                                    | "dwCircleTime"
                                    | "dwFlag"
                                    | "dwLimitLevel1"
                                    | "dwSkillReadyType"
                                    | "worldDrop"
                            );
                            if known {
                                s.get_token(); // =
                                apply_attr(&mut s, &attr, id, items, &mut out);
                                s.get_token(); // ;
                            }
                            s.get_token(); // attribute
                        }
                        s.get_token(); // dwID
                    }
                }
                s.get_token(); // ItemProp OR moverProp
            }
            s.get_token(); // lang
        }
        out
    }

    fn apply_attr(
        s: &mut Script<'_>,
        attr: &str,
        id: u32,
        items: &mut IdTable<ItemProp>,
        out: &mut ItemExcept,
    ) {
        if attr == "worldDrop" {
            if s.get_number() == 0 {
                out.world_drop_excluded.insert(id);
            }
            return;
        }
        // The value is always consumed (a float for fFlightSpeed, a number otherwise).
        let (float, number) = if attr == "fFlightSpeed" {
            (s.get_float(), 0)
        } else {
            (0.0, s.get_number())
        };
        let Some(p) = items.get_mut(id) else {
            // The original would dereference a null pointer here.
            s.report(
                Severity::Warning,
                DiagCode::RefUnresolved,
                format!("except.txt patches unknown item {id} ({attr})"),
            );
            return;
        };
        match attr {
            "fFlightSpeed" => p.flight_speed = float,
            "dwShopAble" => p.shop_able = number,
            "dwCircleTime" => p.circle_time = number,
            "dwFlag" => p.flag = number,
            "dwLimitLevel1" => p.limit_level1 = number,
            "dwSkillReadyType" => p.skill_ready_type = number,
            _ => return,
        }
        out.patched = out.patched.saturating_add(1);
    }
}
