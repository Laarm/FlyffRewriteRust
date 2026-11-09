//! `transformitem.inc`: what a pet egg transforms into (`CTransformItemProperty::LoadScript`, Plan/14 §6.14,
//! ref/13 §19.7; src:_Common/pet.cpp:891-985).
//!
//! ```text
//! N '{' { stuff NUM | item '{' (PROB TYPE ...)* '}' }* '}'
//!       TYPE = TI_GENERIC (0): ITEM NUM                         a plain item
//!       TYPE = TI_PET (1):     KIND LEVELSTRING LIFE EXP%       a system pet (PK_*; LEVELSTRING is the avail level
//!                                                               of each pet level, one digit 1..9 per level, so
//!                                                               "13579" is an S pet and "1" a D pet)
//! ```
//!
//! Probabilities are out of 1,000,000 (a sum below that leaves a "nothing happens" gap, which the original
//! reports at use time). The shipped file is not empty, as the plan assumed: transformation 0 needs a stuff size
//! of 10 and turns into one of seven `II_SYS_SYS_FEED_01` stacks (26 %, 22 %, 19 %, 13 %, 8 %, 3 %, 2 %) or a
//! level-D pet of each kind (1 % each); the commented-out example above it is ignored.
//! The original fails an element whose item does not exist or whose level string has a digit outside 1..9
//! (`CreateItem*` returns `NULL`, which then crashes on the `assert`); here such an element is an Error and is
//! skipped.

use serde::{Deserialize, Serialize};

/// What a transformation yields.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TransformResult {
    /// `TI_GENERIC`: `num` of `item`.
    Item {
        /// The item.
        item: u32,
        /// The stack size.
        num: i32,
    },
    /// `TI_PET`: a system pet.
    Pet {
        /// `PK_*` (0..=6).
        kind: u32,
        /// The avail level of each pet level D.. (`avail[0]` is D; its length is the pet level 1..=5).
        avail: Vec<u8>,
        /// `SetLife`.
        life: i32,
        /// The experience in percent of the level's maximum (`GetFloat()`).
        exp_percent: f32,
    },
}

/// One weighted element.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TransformElement {
    /// The probability out of 1,000,000.
    pub prob: u32,
    /// The result.
    pub result: TransformResult,
}

/// One transformation (`CTransformItemComponent`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TransformComponent {
    /// The transformation number (the `nTransform` of the egg).
    pub transform: i32,
    /// How many stuff items the transformation needs (`stuff`).
    pub stuff: i32,
    /// The weighted results in file order.
    pub elements: Vec<TransformElement>,
}

/// All transformations.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TransformItemTable {
    /// The components in file order (the first block of a number wins, `map::insert`).
    pub components: Vec<TransformComponent>,
}

impl TransformItemTable {
    /// `GetComponent( nTransform )`.
    pub fn component(&self, transform: i32) -> Option<&TransformComponent> {
        self.components.iter().find(|c| c.transform == transform)
    }

    /// `GetStuffSize( nTransform )` (0 without a component).
    pub fn stuff_size(&self, transform: i32) -> i32 {
        self.component(transform).map_or(0, |c| c.stuff)
    }
}

impl TransformComponent {
    /// `GetItem()` for a roll in `0..1_000_000`: the first element whose running probability exceeds the roll.
    pub fn pick(&self, roll: u32) -> Option<&TransformElement> {
        let mut total = 0_u32;
        self.elements.iter().find(|e| {
            total = total.saturating_add(e.prob);
            roll < total
        })
    }
}

#[cfg(feature = "load")]
pub(crate) use parse::parse_transform_items;

#[cfg(feature = "load")]
mod parse {
    use super::{TransformComponent, TransformElement, TransformItemTable, TransformResult};
    use flyff_script::{DiagCode, Diagnostics, ScannerOptions, Script, ScriptCtx, Severity};
    use std::sync::Arc;

    const TI_GENERIC: u32 = 0;
    const TI_PET: u32 = 1;

    fn open(sc: &Script<'_>) -> bool {
        !sc.token().starts_with('}') && !sc.finished()
    }

    /// `CTransformItemProperty::LoadScript` over decoded `text`.
    pub(crate) fn parse_transform_items(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> TransformItemTable {
        let mut out = TransformItemTable::default();
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        let mut transform = sc.get_number_i32();
        while !sc.finished() {
            let mut comp = TransformComponent {
                transform,
                ..TransformComponent::default()
            };
            sc.get_token(); // {
            sc.get_token(); // subject or '}'
            while open(&sc) {
                match sc.token().to_owned().as_str() {
                    "stuff" => comp.stuff = sc.get_number_i32(),
                    "item" => {
                        sc.get_token(); // {
                        let mut prob = sc.get_number();
                        while open(&sc) {
                            let kind = sc.get_number();
                            let result = if kind == TI_GENERIC {
                                let item = sc.get_number();
                                let num = sc.get_number_i32();
                                if item == 0 {
                                    sc.report(
                                        Severity::Error,
                                        DiagCode::RefUnresolved,
                                        "transformitem.inc: a TI_GENERIC element names an undefined item; skipped",
                                    );
                                    None
                                } else {
                                    Some(TransformResult::Item { item, num })
                                }
                            } else if kind == TI_PET {
                                let pet_kind = sc.get_number();
                                sc.get_token();
                                let levels = sc.token().to_owned();
                                let life = sc.get_number_i32();
                                let exp_percent = sc.get_float();
                                let avail: Vec<u8> =
                                    levels.bytes().map(|b| b.wrapping_sub(b'0')).collect();
                                if avail.is_empty()
                                    || avail.len() > 5
                                    || avail.iter().any(|a| !(1..=9).contains(a))
                                {
                                    sc.report(
                                        Severity::Error,
                                        DiagCode::ValueOutOfRange,
                                        format!(
                                            "transformitem.inc: pet level string {levels:?} is not 1..5 digits of 1..9; skipped"
                                        ),
                                    );
                                    None
                                } else {
                                    Some(TransformResult::Pet {
                                        kind: pet_kind,
                                        avail,
                                        life,
                                        exp_percent,
                                    })
                                }
                            } else {
                                sc.report(
                                    Severity::Error,
                                    DiagCode::ValueOutOfRange,
                                    format!("transformitem.inc: unknown element type {kind}; the rest of the block is skipped"),
                                );
                                // the original would read the columns as the wrong type; skip to the block end
                                while open(&sc) {
                                    sc.get_token();
                                }
                                break;
                            };
                            if let Some(result) = result {
                                comp.elements.push(TransformElement { prob, result });
                            }
                            prob = sc.get_number();
                        }
                    }
                    _ => {}
                }
                sc.get_token();
            }
            if !out.components.iter().any(|c| c.transform == transform) {
                out.components.push(comp);
            }
            transform = sc.get_number_i32();
        }
        out
    }
}
