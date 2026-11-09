//! `randomoption.inc`: the awakening, blessing and pet random-option tables (`CRandomOptionProperty::LoadScript`,
//! Plan/14 §6.13, ref/17 §8.7; src:_Common/randomoption.cpp:23-100).
//!
//! ```text
//! (Awakening | Blessing | SystemPet | EatPet) A B C '{' (DST PROB '{' (VALUE PROB)* '}')* '}'
//! ```
//!
//! `A B C` are the probabilities of one, two and three options. The original turns every probability into a
//! cumulative number at load time; the tables here keep the **raw** numbers of the file in file order (the
//! cumulative sums are the server's derivation, 19). A `(DST, value)` list whose probabilities do not sum to
//! 1,000,000,000 is reported by the original with `Error()`; here it is a Warning.

use serde::{Deserialize, Serialize};

/// One destination parameter of a block with its possible values.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RandomOptionDst {
    /// `DST_*`.
    pub dst: u32,
    /// The probability of this destination within the block (raw).
    pub n: i32,
    /// `(value, probability)` pairs (raw probabilities, 1,000,000,000 in total).
    pub values: Vec<[i32; 2]>,
}

/// One block of the file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RandomOptionGroup {
    /// The block keyword (`Awakening`, `Blessing`, `SystemPet`, `EatPet`).
    pub kind: String,
    /// The probability of one option.
    pub a: i32,
    /// The probability of two options.
    pub b: i32,
    /// The probability of three options.
    pub c: i32,
    /// The destinations in file order.
    pub dsts: Vec<RandomOptionDst>,
}

/// The plan's name for a block of the file (`RandomOptKind`, Plan/14 §6.13).
pub type RandomOptKind = RandomOptionGroup;

/// The random-option tables.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RandomOptionTables {
    /// The blocks in file order.
    pub groups: Vec<RandomOptionGroup>,
}

impl RandomOptionTables {
    /// The first block of a kind (`Awakening`, `Blessing`, `SystemPet`, `EatPet`).
    pub fn group(&self, kind: &str) -> Option<&RandomOptionGroup> {
        self.groups.iter().find(|g| g.kind == kind)
    }
}

#[cfg(feature = "load")]
pub(crate) use parse::parse_random_option;

#[cfg(feature = "load")]
mod parse {
    use super::{RandomOptionDst, RandomOptionGroup, RandomOptionTables};
    use flyff_script::{DiagCode, Diagnostics, ScannerOptions, Script, ScriptCtx, Severity};
    use std::sync::Arc;

    const KINDS: [&str; 4] = ["Awakening", "Blessing", "SystemPet", "EatPet"];
    /// The total of the probabilities of one `(DST, value)` list.
    const PROB_TOTAL: i64 = 1_000_000_000;

    fn open(sc: &Script<'_>) -> bool {
        !sc.token().starts_with('}') && !sc.finished()
    }

    /// `CRandomOptionProperty::LoadScript` over decoded `text`.
    pub(crate) fn parse_random_option(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> RandomOptionTables {
        let mut out = RandomOptionTables::default();
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        sc.get_token();
        while !sc.finished() {
            let key = sc.token().to_owned();
            if KINDS.contains(&key.as_str()) {
                let mut g = RandomOptionGroup {
                    kind: key,
                    a: sc.get_number_i32(),
                    b: sc.get_number_i32(),
                    c: sc.get_number_i32(),
                    dsts: Vec::new(),
                };
                sc.get_token(); // {
                let mut dst = sc.get_number();
                while open(&sc) {
                    let n = sc.get_number_i32();
                    sc.get_token(); // {
                    let mut values = Vec::new();
                    let mut total = 0i64;
                    let mut adj = sc.get_number_i32();
                    while open(&sc) {
                        let prob = sc.get_number_i32();
                        total += i64::from(prob);
                        values.push([adj, prob]);
                        adj = sc.get_number_i32();
                    }
                    if total != PROB_TOTAL {
                        sc.report(
                            Severity::Warning,
                            DiagCode::ValueOutOfRange,
                            format!(
                                "{}: dst {dst} probabilities sum to {total}, not {PROB_TOTAL}",
                                g.kind
                            ),
                        );
                    }
                    g.dsts.push(RandomOptionDst { dst, n, values });
                    dst = sc.get_number();
                }
                out.groups.push(g);
            }
            sc.get_token();
        }
        out
    }
}
