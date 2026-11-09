//! The text world formats (Plan/30 §9; `CWorld::OpenWorld`, `LoadRegion`, `ReadRegion`, `ReadRespawn`,
//! src:_Common/WorldFile.cpp:56-819; Continent.cpp:30-104; WorldEnvironment.cpp:183-204, 633-720;
//! PathObj.cpp:147-227; ref/19 §5, §9-11).
//!
//! | file | parser | result |
//! |---|---|---|
//! | `<World>.wld` | [`parse_wld`] | [`WldSettings`] |
//! | `<World>.wld.24l` style light table (`<path>.24l`) | [`parse_24l`] | 24 rows of 6 floats |
//! | `<World>.rgn` (UTF-16LE) | [`parse_rgn`] | [`RgnFile`] (regions and respawns) |
//! | `<World>.wld.cnt` | [`parse_cnt`] | [`CntFile`] (continent polygons) |
//! | `<World>.pat` | [`parse_pat`] | patrol paths |
//!
//! # Common rules (Plan/30 §9.1)
//!
//! * The input is already decoded text (`flyff_vfs::text::decode_text`, 14 §2.6). Tokenising uses
//!   [`flyff_script::Script`] with the caller's [`ScriptCtx`], so identifiers are substituted exactly as
//!   `CScript::GetToken` did (string table first, defines second).
//! * Keyword comparisons are exact and case-sensitive. Unknown tokens are skipped one at a time.
//! * The parsers never fail: they return a value plus [`Diagnostics`]. A record cut off by the end of the
//!   file is dropped with `WorldTextTruncated` (the original read zeros). Strings longer than the original's
//!   31-character buffers are kept and flagged `WorldTextStringLong`.

use std::sync::Arc;

use flyff_script::{DiagCode, Diagnostics, ScannerOptions, Script, ScriptCtx, Severity};

pub mod cnt;
pub mod l24;
pub mod pat;
pub mod rgn;
pub mod wld;

pub use cnt::{CntFile, ContinentDef, parse_cnt};
pub use l24::parse_24l;
pub use pat::{PatrolPath, PatrolPoint, parse_pat};
pub use rgn::{RegionDef, RegionReq, RespawnDef, RgnFile, parse_rgn};
pub use wld::{WldSettings, parse_wld};

/// The longest key/script/sound/revival-key string the original's buffers hold (31 characters + NUL).
pub const ORIGINAL_STRING_MAX: usize = 31;

/// A [`Script`] over `text` (a leading BOM is dropped), diagnostics go to `diags`.
pub(crate) fn open_script<'a>(
    text: &'a str,
    file: &str,
    ctx: &'a ScriptCtx,
    diags: &'a mut Diagnostics,
) -> Script<'a> {
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    Script::new(text, Arc::from(file), ScannerOptions::default(), ctx, diags)
}

/// `true` when `s` would not fit the original's 31-character buffer. The original counted bytes of the
/// multi-byte (CP949) text, so a non-ASCII character counts as two.
pub(crate) fn exceeds_original_buffer(s: &str) -> bool {
    s.chars()
        .map(|c| if c.is_ascii() { 1 } else { 2 })
        .sum::<usize>()
        > ORIGINAL_STRING_MAX
}

/// Reports `WorldTextStringLong` when `value` (the `what` of a record) is longer than the original buffer.
pub(crate) fn check_string_len(script: &mut Script<'_>, what: &str, value: &str) {
    if exceeds_original_buffer(value) {
        script.report(
            Severity::Warning,
            DiagCode::WorldTextStringLong,
            format!(
                "{what} `{value}` is longer than the original's {ORIGINAL_STRING_MAX}-character buffer"
            ),
        );
    }
}

/// Reports a record that the end of the file cut off (`WorldTextTruncated`).
pub(crate) fn report_truncated(script: &mut Script<'_>, what: &str) {
    script.report(
        Severity::Warning,
        DiagCode::WorldTextTruncated,
        format!("{what} is cut off by the end of the file and dropped"),
    );
}
