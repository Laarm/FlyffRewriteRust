//! `Client/Guide.inc`: the tutorial guide, of which the server only needs the highest `LEVEL` sequence number
//! (`CProject::LoadGuideMaxSeq`, `__MOD_TUTORIAL`; Plan/14 §6.16, L62; ref/17 §8.11; src:_Common/Project.cpp:
//! 1034-1100). The client reads the whole file (`ClientData::help.guide`, WP-DATA-16).
//!
//! ```text
//! NAME '{' BEGINNER n  SHOWLEVEL n  FLAG n  KEY n  IDS_TEXT  [SEQUENCE n] [LEVEL n] [CONDITION n] [INPUTCHAR n]
//!          [INPUTSTR n | '{' ... '}']  '}'
//! ```
//!
//! The original is a rigid positional reader: it expects the keywords in exactly this order and ignores any block
//! that is not introduced by `{`. It is ported token for token, with an end-of-file guard on the one loop that would
//! spin there. The shipped file starts with a block comment that holds a sample entry.

#[cfg(feature = "load")]
pub(crate) use parse::parse_guide_max_seq;

#[cfg(feature = "load")]
mod parse {
    use flyff_script::{Diagnostics, ScannerOptions, Script, ScriptCtx};
    use std::sync::Arc;

    /// `LoadGuideMaxSeq` over decoded `text`: the largest number after a `LEVEL` keyword (0 for none).
    pub(crate) fn parse_guide_max_seq(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> u32 {
        let mut max_seq = 0_u32;
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        // `script.tok = 0` primes the loop; the first iteration always runs
        let mut first = true;
        while first || !sc.finished() {
            first = false;
            sc.get_number(); // the block name (a define such as GUIDE_EVENT_INTRO)
            sc.get_token(); // {
            if sc.token().starts_with('{') {
                sc.get_token(); // BEGINNER
                sc.get_number();
                sc.get_token(); // SHOWLEVEL
                sc.get_number();
                sc.get_token(); // FLAG
                sc.get_number();
                sc.get_token(); // KEY
                sc.get_number();
                sc.get_token(); // the text key
                sc.get_token();
                if sc.token().eq_ignore_ascii_case("SEQUENCE") {
                    sc.get_number();
                    sc.get_token();
                }
                if sc.token().eq_ignore_ascii_case("LEVEL") {
                    let cur = sc.get_number();
                    max_seq = max_seq.max(cur);
                    sc.get_token();
                }
                if sc.token().eq_ignore_ascii_case("CONDITION") {
                    sc.get_number();
                    sc.get_token();
                }
                if sc.token().eq_ignore_ascii_case("INPUTCHAR") {
                    sc.get_number();
                    sc.get_token();
                }
                if sc.token().eq_ignore_ascii_case("INPUTSTR") {
                    sc.get_token();
                    sc.get_token(); // }
                    while !sc.token().starts_with('}') && !sc.finished() {
                        sc.get_token();
                    }
                }
            } else {
                sc.get_token();
            }
        }
        max_seq
    }
}
