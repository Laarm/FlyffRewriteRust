//! `propItemEtc.inc`: piercing materials, armor sets and random-option items (`CProject::LoadPiercingAvail`,
//! Plan/14 §6.12, ref/17 §8.7; src:_Common/Project.cpp:4265-4455).
//!
//! ```text
//! Piercing ITEM '{' (DST ADJ)* '}'                                  40 blocks
//! SetItem ID NAME '{' Elem '{' (ITEM PARTS)* '}'  Avail '{' (DST ADJ EQUIPED)* '}' '}'    134 blocks
//! RandomOptItem ID NAME LEVEL PROB '{' (DST ADJ)* '}'               48 blocks
//! ```
//!
//! Quirks kept: piercing rows of one item are merged in first-appearance order (`CPiercingAvail`, at most
//! 32 parameters per material and 128 materials); a set has at most 8 elements and 32 avail rows, whose
//! rows are exchange-sorted by the number of equipped pieces (`SortItemAvail`); the random-option items
//! (at most 256) are exchange-sorted by level (`Arrange`). Rows beyond a capacity are an Error and ignored
//! (the original refused them). The original reads a set body with a loop that never advances on an unknown
//! keyword; here an unknown keyword is skipped.

use serde::{Deserialize, Serialize};

/// `MAX_PIERCING_DSTPARAM`.
pub const MAX_PIERCING_DSTPARAM: usize = 32;
/// `MAX_PIERCING_MATERIAL`.
pub const MAX_PIERCING_MATERIAL: usize = 128;
/// `MAX_SETITEM_ELEM`.
pub const MAX_SETITEM_ELEM: usize = 8;
/// `MAX_ITEMAVAIL`.
pub const MAX_ITEMAVAIL: usize = 32;
/// `MAX_RANDOMOPTITEM`.
pub const MAX_RANDOMOPTITEM: usize = 256;

/// One piercing material and the parameters it grants.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PiercingDef {
    /// The material item.
    pub item: u32,
    /// `(DST, adjust)` pairs in file order.
    pub params: Vec<[i32; 2]>,
}

/// One piece of a set: the item and the `PARTS_*` slot it occupies.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SetElem {
    /// The item.
    pub item: u32,
    /// `PARTS_*`.
    pub parts: i32,
}

/// One bonus of a set.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SetAvail {
    /// `DST_*`.
    pub dst: i32,
    /// The adjustment.
    pub adj: i32,
    /// The number of equipped pieces that activates it.
    pub equiped: i32,
}

/// One armor set (`CSetItem`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SetItemDef {
    /// The set id.
    pub id: i32,
    /// The set name (IDS key resolved).
    pub name: String,
    /// The pieces (at most 8).
    pub elems: Vec<SetElem>,
    /// The bonuses, sorted by `equiped` (exchange sort).
    pub avail: Vec<SetAvail>,
}

/// One random-option item (`RANDOMOPTITEM`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RandomOptItemDef {
    /// The id.
    pub id: i32,
    /// The name (IDS key resolved).
    pub name: String,
    /// The level.
    pub level: i32,
    /// The probability.
    pub prob: u32,
    /// `(DST, adjust)` pairs.
    pub params: Vec<[i32; 2]>,
}

/// The tables of `propItemEtc.inc`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemEtcTables {
    /// Piercing materials in first-appearance order.
    pub piercing: Vec<PiercingDef>,
    /// Armor sets in file order (a repeated id keeps the first, like `map::insert`).
    pub set_items: Vec<SetItemDef>,
    /// Random-option items, exchange-sorted by level.
    pub random_opt_items: Vec<RandomOptItemDef>,
}

#[cfg(feature = "load")]
pub(crate) use parse::parse_item_etc;

#[cfg(feature = "load")]
mod parse {
    use super::{
        ItemEtcTables, MAX_ITEMAVAIL, MAX_PIERCING_DSTPARAM, MAX_PIERCING_MATERIAL,
        MAX_RANDOMOPTITEM, MAX_SETITEM_ELEM, PiercingDef, RandomOptItemDef, SetAvail, SetElem,
        SetItemDef,
    };
    use flyff_script::{DiagCode, Diagnostics, ScannerOptions, Script, ScriptCtx, Severity};
    use std::sync::Arc;

    fn open(sc: &Script<'_>) -> bool {
        !sc.token().starts_with('}') && !sc.finished()
    }

    /// `CSetItem::SortItemAvail`: the exchange sort by `equiped` (not stable, as the original).
    fn sort_avail(a: &mut [SetAvail]) {
        for i in 0..a.len().saturating_sub(1) {
            for j in i + 1..a.len() {
                if a.get(i).map(|x| x.equiped) > a.get(j).map(|x| x.equiped) {
                    a.swap(i, j);
                }
            }
        }
    }

    /// `CRandomOptItemGen::Arrange`: the exchange sort by level.
    fn sort_random(a: &mut [RandomOptItemDef]) {
        for i in 0..a.len().saturating_sub(1) {
            for j in i + 1..a.len() {
                if a.get(i).map(|x| x.level) > a.get(j).map(|x| x.level) {
                    a.swap(i, j);
                }
            }
        }
    }

    /// `LoadPiercingAvail` over decoded `text`.
    pub(crate) fn parse_item_etc(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> ItemEtcTables {
        let mut out = ItemEtcTables::default();
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
                "Piercing" => {
                    let item = sc.get_number();
                    sc.get_token(); // {
                    let mut dst = sc.get_number_i32();
                    let idx = match out.piercing.iter().position(|p| p.item == item) {
                        Some(i) => Some(i),
                        None if out.piercing.len() >= MAX_PIERCING_MATERIAL => {
                            sc.report(
                                Severity::Error,
                                DiagCode::CapacityExceeded,
                                format!("propItemEtc: more than {MAX_PIERCING_MATERIAL} piercing materials; {item} ignored"),
                            );
                            None
                        }
                        None => {
                            out.piercing.push(PiercingDef {
                                item,
                                params: Vec::new(),
                            });
                            Some(out.piercing.len() - 1)
                        }
                    };
                    while open(&sc) {
                        let adj = sc.get_number_i32();
                        if let Some(p) = idx.and_then(|i| out.piercing.get_mut(i)) {
                            if p.params.len() < MAX_PIERCING_DSTPARAM {
                                p.params.push([dst, adj]);
                            } else {
                                sc.report(
                                    Severity::Error,
                                    DiagCode::CapacityExceeded,
                                    format!("propItemEtc: piercing {item} has more than {MAX_PIERCING_DSTPARAM} parameters"),
                                );
                            }
                        }
                        dst = sc.get_number_i32();
                    }
                }
                "SetItem" => {
                    let id = sc.get_number_i32();
                    sc.get_token();
                    let mut set = SetItemDef {
                        id,
                        name: sc.token().to_owned(),
                        ..SetItemDef::default()
                    };
                    sc.get_token(); // {
                    sc.get_token(); // category
                    while open(&sc) {
                        match sc.token().to_owned().as_str() {
                            "Elem" => {
                                sc.get_token(); // {
                                let mut item = sc.get_number();
                                while open(&sc) {
                                    let parts = sc.get_number_i32();
                                    if set.elems.len() < MAX_SETITEM_ELEM {
                                        set.elems.push(SetElem { item, parts });
                                    } else {
                                        sc.report(
                                            Severity::Error,
                                            DiagCode::CapacityExceeded,
                                            format!("propItemEtc: set {id} has more than {MAX_SETITEM_ELEM} elements"),
                                        );
                                    }
                                    item = sc.get_number();
                                }
                                sc.get_token();
                            }
                            "Avail" => {
                                sc.get_token(); // {
                                let mut dst = sc.get_number_i32();
                                while open(&sc) {
                                    let adj = sc.get_number_i32();
                                    let equiped = sc.get_number_i32();
                                    if set.avail.len() < MAX_ITEMAVAIL {
                                        set.avail.push(SetAvail { dst, adj, equiped });
                                    } else {
                                        sc.report(
                                            Severity::Error,
                                            DiagCode::CapacityExceeded,
                                            format!("propItemEtc: set {id} has more than {MAX_ITEMAVAIL} bonuses"),
                                        );
                                    }
                                    dst = sc.get_number_i32();
                                }
                                sc.get_token();
                            }
                            _ => {
                                sc.get_token(); // an unknown category: skip it (the original would loop)
                            }
                        }
                    }
                    sort_avail(&mut set.avail);
                    if out.set_items.iter().any(|s| s.id == set.id) {
                        sc.report(
                            Severity::Warning,
                            DiagCode::PropIdDuplicate,
                            format!("propItemEtc: set {id} is defined twice; the first wins"),
                        );
                    } else {
                        out.set_items.push(set);
                    }
                }
                "RandomOptItem" => {
                    let id = sc.get_number_i32();
                    sc.get_token();
                    let name = sc.token().to_owned();
                    let level = sc.get_number_i32();
                    let prob = sc.get_number();
                    sc.get_token(); // {
                    let mut params = Vec::new();
                    let mut dst = sc.get_number_i32();
                    while open(&sc) {
                        let adj = sc.get_number_i32();
                        if params.len() < MAX_ITEMAVAIL {
                            params.push([dst, adj]);
                        }
                        dst = sc.get_number_i32();
                    }
                    if out.random_opt_items.len() < MAX_RANDOMOPTITEM {
                        out.random_opt_items.push(RandomOptItemDef {
                            id,
                            name,
                            level,
                            prob,
                            params,
                        });
                    } else {
                        sc.report(
                            Severity::Error,
                            DiagCode::CapacityExceeded,
                            format!("propItemEtc: more than {MAX_RANDOMOPTITEM} random-option items; {id} ignored"),
                        );
                    }
                }
                _ => {}
            }
            sc.get_token();
        }
        sort_random(&mut out.random_opt_items);
        out
    }
}
