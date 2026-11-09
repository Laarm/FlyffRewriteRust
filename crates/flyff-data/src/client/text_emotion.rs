//! `textEmotion.txt`: chat emoticon commands (`CDialogMsg::LoadEmotion`, Plan/14 §6.19;
//! src:_Common/DialogMsg.cpp:1000-1030). `{ INDEX COMMAND }*` read with `CScript`, `COMMAND` is an
//! `IDS_TEXTEMOTION_TXT_n` key resolved to the English word (29 entries: `laugh`, `sad`, `kiss`, ...).
//! The server builds its `EmoteTable` from the same map (Plan/23 §6.10).

/// `MAX_EMOTICON_NUM`: the size of the original's array; a longer file is an Error and stops the load.
pub const MAX_EMOTICON_NUM: usize = 100;

#[cfg(feature = "load")]
pub(crate) use parse::parse_text_emotion;

#[cfg(feature = "load")]
mod parse {
    use super::MAX_EMOTICON_NUM;
    use flyff_script::{
        DiagCode, Diagnostics, ScannerOptions, Script, ScriptCtx, Severity, c_atoi,
    };
    use std::collections::BTreeMap;
    use std::sync::Arc;

    /// Index (`atoi` of the first token of a pair) to command; a repeated index is replaced.
    pub(crate) fn parse_text_emotion(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> BTreeMap<u32, String> {
        let mut out = BTreeMap::new();
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        sc.get_token();
        let mut n = 0usize;
        while !sc.finished() {
            if n >= MAX_EMOTICON_NUM {
                sc.report(
                    Severity::Error,
                    DiagCode::CapacityExceeded,
                    format!(
                        "textEmotion: more than {MAX_EMOTICON_NUM} entries; the rest is ignored"
                    ),
                );
                break;
            }
            let idx = u32::try_from(c_atoi(sc.token().as_bytes())).unwrap_or(0);
            sc.get_token();
            if sc.finished() {
                break;
            }
            out.insert(idx, sc.token().to_owned());
            sc.get_token();
            n += 1;
        }
        out
    }
}
