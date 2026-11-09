//! `filter_ENG.inc`: the chat word filter (`CProject::LoadFilter`, Plan/14 §6.19, ref/17 §8.11;
//! src:_Common/Project.cpp:1140-1184).
//!
//! `{ WORD "REPLACEMENT" }*` read with a raw `CScanner` (UTF-16). The client splits the words into an
//! alphabetic map (lower-cased) and a non-alphabetic map (`__FILTER_0705`); that split belongs to the
//! consumer (`ChatFilter::from_pairs` of the game and the TS helper of doc 45), so the pairs are kept as
//! written, in file order: 330 pairs in the reference data (660 tokens; every replacement is `*****`).

#[cfg(feature = "load")]
pub(crate) use parse::parse_filter;

#[cfg(feature = "load")]
mod parse {
    use flyff_script::{Diagnostics, Scanner, ScannerOptions};
    use std::sync::Arc;

    /// The `(word, replacement)` pairs in file order. A word without a replacement at the end of the file is
    /// dropped.
    pub(crate) fn parse_filter(
        text: &str,
        file: &Arc<str>,
        diags: &mut Diagnostics,
    ) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let mut sc = Scanner::new(text, Arc::clone(file), ScannerOptions::default());
        sc.get_token(false);
        while !sc.finished() {
            let word = sc.token().to_owned();
            sc.get_token(false);
            if sc.finished() {
                break;
            }
            out.push((word, sc.token().to_owned()));
            sc.get_token(false);
        }
        diags.extend(sc.take_diagnostics());
        out
    }
}
