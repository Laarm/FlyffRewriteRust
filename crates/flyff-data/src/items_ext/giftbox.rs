//! `propGiftbox.inc` and `propPackItem.inc` (`CProject::LoadGiftbox`, `LoadPackItem`; Plan/14 §6.12, L40/L41;
//! ref/13 §10.5-§10.6; src:_Common/Project.cpp:3815-4256). Server-only content (WP-DATA-11: gift boxes and
//! packs; the use rules are `flyff_game::item::usage::{giftbox, pack}`).
//!
//! ```text
//! GiftBox  <box item> '{' (ITEM PROB NUM)* '}'                              PROB * 100
//! GiftBox2 <box item> '{' (ITEM PROB NUM)* '}'                              PROB * 1
//! GiftBox3 <box item> '{' (ITEM PROB NUM FLAG)* '}'                         PROB * 100
//! GiftBox4 <box item> '{' (ITEM PROB NUM FLAG SPAN)* '}'                    PROB * 100
//! GiftBox5 <box item> '{' (ITEM PROB NUM FLAG SPAN)* '}'                    PROB * 10
//! GiftBox6 <box item> '{' (ITEM PROB NUM FLAG SPAN ABILITY_OPTION)* '}'     PROB * 10
//! PackItem <pack item> <span minutes> '{' (ITEM ABILITY_OPTION NUM)* '}'
//! ```
//!
//! A box may appear in several blocks: its entries are appended in file order (the C++ `map<DWORD, int>`
//! index). Probabilities are stored **cumulatively**; `Verify()` adds `1_000_000 - sum` to the last entry, so
//! the last cumulative value of every box is exactly 1 000 000 and a draw `r < 1_000_000` always finds an entry.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The probability space of a gift box (`xRandom( 1000000 )`).
pub const GIFTBOX_SPACE: u32 = 1_000_000;
/// `MAX_ITEM_PER_PACK`.
pub const MAX_ITEM_PER_PACK: usize = 16;

/// One outcome of a gift box.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GiftBoxEntry {
    /// The item that is created.
    pub item: u32,
    /// The cumulative probability out of [`GIFTBOX_SPACE`] after `Verify()`: the entry is picked when
    /// `r < cum_prob` and no earlier entry matched.
    pub cum_prob: u32,
    /// The stack count.
    pub num: i32,
    /// `nFlag`: the item flag byte; 4 means "ignore the property" (no flag, no `charged`).
    pub flag: u8,
    /// `nSpan`: keep time in minutes, 0 = permanent.
    pub span: i32,
    /// `nAbilityOption`: the upgrade level of the created item.
    pub ability_option: i32,
}

/// One gift box.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GiftBox {
    /// The block kind of the first block that defined the box: 1 `GiftBox` ... 6 `GiftBox6`.
    pub kind: u8,
    /// The outcomes in file order (at most 128 per box in the original; not enforced here).
    pub entries: Vec<GiftBoxEntry>,
}

impl GiftBox {
    /// `CGiftboxMan::Open`: the first entry whose cumulative probability exceeds `roll` (`roll` in
    /// `0..GIFTBOX_SPACE`); `None` when the box is empty or `roll` is out of range.
    pub fn pick(&self, roll: u32) -> Option<&GiftBoxEntry> {
        self.entries.iter().find(|e| roll < e.cum_prob)
    }
}

/// The gift boxes by box item id.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GiftBoxTable {
    /// The boxes, ascending item id.
    pub boxes: BTreeMap<u32, GiftBox>,
    /// How many blocks the file had per keyword (`GiftBox`, `GiftBox2` .. `GiftBox6`); a box can appear in
    /// several blocks, so the sum can exceed `boxes.len()` (C13: 142 / 0 / 21 / 7 / 1 / 26 = 197).
    pub blocks: [u32; 6],
}

/// One pack (`PACKITEMELEM`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackItem {
    /// The pack item.
    pub id: u32,
    /// Keep time of the contents in minutes, 0 = permanent (`nSpan`).
    pub span: i32,
    /// `(item, ability option, count)` per entry, at most [`MAX_ITEM_PER_PACK`].
    pub items: Vec<(u32, i32, i32)>,
}

/// The packs by pack item id.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackTable {
    /// The packs, ascending item id.
    pub packs: BTreeMap<u32, PackItem>,
    /// How many `PackItem` blocks the file had (C14: 460); a pack can appear in several blocks.
    pub blocks: u32,
}

#[cfg(feature = "load")]
pub(crate) use parse::{parse_giftbox, parse_pack_items};

#[cfg(feature = "load")]
mod parse {
    use super::{
        GIFTBOX_SPACE, GiftBox, GiftBoxEntry, GiftBoxTable, MAX_ITEM_PER_PACK, PackItem, PackTable,
    };
    use flyff_script::{DiagCode, Diagnostics, ScannerOptions, Script, ScriptCtx, Severity};
    use std::sync::Arc;

    fn open(sc: &Script<'_>) -> bool {
        !sc.token().starts_with('}') && !sc.finished()
    }

    /// `CProject::LoadGiftbox` over decoded `text`, including `Verify()`.
    pub(crate) fn parse_giftbox(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> GiftBoxTable {
        let mut out = GiftBoxTable::default();
        // the plain probability sum of each box (the C++ `nSum`)
        let mut sums: std::collections::BTreeMap<u32, u32> = std::collections::BTreeMap::new();
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        sc.get_token();
        while !sc.finished() {
            // (kind, probability scale, columns after NUM: flag, span, ability option)
            let spec = match sc.token() {
                "GiftBox" => Some((1_u8, 100_u32, 0_u8)),
                "GiftBox2" => Some((2, 1, 0)),
                "GiftBox3" => Some((3, 100, 1)),
                "GiftBox4" => Some((4, 100, 2)),
                "GiftBox5" => Some((5, 10, 2)),
                "GiftBox6" => Some((6, 10, 3)),
                _ => None,
            };
            if let Some((kind, scale, extra)) = spec {
                if let Some(n) = out.blocks.get_mut(usize::from(kind) - 1) {
                    *n += 1;
                }
                let box_id = sc.get_number();
                sc.get_token(); // {
                let mut item = sc.get_number();
                while open(&sc) {
                    let prob = sc.get_number();
                    let num = sc.get_number_i32();
                    let flag = if extra >= 1 { sc.get_number() as u8 } else { 0 };
                    let span = if extra >= 2 { sc.get_number_i32() } else { 0 };
                    let ability_option = if extra >= 3 { sc.get_number_i32() } else { 0 };
                    let b = out.boxes.entry(box_id).or_insert_with(|| GiftBox {
                        kind,
                        entries: Vec::new(),
                    });
                    let sum = sums.entry(box_id).or_insert(0);
                    *sum = sum.wrapping_add(prob.wrapping_mul(scale));
                    b.entries.push(GiftBoxEntry {
                        item,
                        cum_prob: *sum,
                        num,
                        flag,
                        span,
                        ability_option,
                    });
                    item = sc.get_number();
                }
            }
            sc.get_token();
        }
        // Verify(): the remainder goes to the last entry of every box
        for (id, b) in &mut out.boxes {
            let sum = sums.get(id).copied().unwrap_or(0);
            if let Some(last) = b.entries.last_mut() {
                last.cum_prob = last.cum_prob.wrapping_add(GIFTBOX_SPACE.wrapping_sub(sum));
            }
        }
        drop(sc);
        out
    }

    /// `CProject::LoadPackItem` over decoded `text`.
    pub(crate) fn parse_pack_items(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> PackTable {
        let mut out = PackTable::default();
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        sc.get_token();
        while !sc.finished() {
            if sc.token() == "PackItem" {
                out.blocks += 1;
                let id = sc.get_number();
                let span = sc.get_number_i32();
                sc.get_token(); // {
                let p = out.packs.entry(id).or_insert_with(|| PackItem {
                    id,
                    span,
                    items: Vec::new(),
                });
                // `pPackItemElem->nSpan = nSpan` runs after every block of the pack: the last block wins
                p.span = span;
                let mut item = sc.get_number();
                while open(&sc) {
                    let ability_option = sc.get_number_i32();
                    let num = sc.get_number_i32();
                    if p.items.len() >= MAX_ITEM_PER_PACK {
                        sc.report(
                            Severity::Error,
                            DiagCode::CapacityExceeded,
                            format!("propPackItem.inc: pack {id} has more than {MAX_ITEM_PER_PACK} entries; the rest is ignored"),
                        );
                    } else {
                        p.items.push((item, ability_option, num));
                    }
                    item = sc.get_number();
                }
            }
            sc.get_token();
        }
        drop(sc);
        out
    }
}
