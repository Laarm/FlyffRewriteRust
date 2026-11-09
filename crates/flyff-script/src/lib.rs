//! `flyff-script` — tier S (Plan/14 §3-4): the bit-exact CScanner tokenizer, CScript define/string
//! substitution, `Defines`, `StringTable` and diagnostics. Operates on already-decoded UTF-8 text;
//! pure, wasm-safe, panic-free, nothing to do with Lua.
//!
//! * [`scanner`] — `CScanner` (`Scanner`, `TokenType`, `Mark`, `ScannerOptions`): whitespace and comma
//!   mode, the raw value getters (`get_number`, `get_int64`, `get_float`, `get_hex`, `get_last_full`),
//!   marks instead of `PutBack` (14 §3),
//! * [`cnum`] — the C numeric conversions (`c_atoi`, `c_atoi64`, `c_atof`) (14 §3.5),
//! * [`script`] — `CScript`: classification, define/string substitution, the "Not Found" diagnostic and
//!   the client-variant divergence counter (14 §4.1-4.2),
//! * [`defines`] — `Defines`, the 17 header names and `PreScan`/`ExecDefine` (14 §4.3),
//! * [`strings`] — `StringTable`, the 64 string-table names and `LoadString` (14 §4.4),
//! * [`prefiles`] — `load_pre_files`: both lists in the original order over a caller-supplied reader
//!   (`CProject::LoadPreFiles` without the file access),
//! * [`diag`] — `Diagnostic`, `Diagnostics`, `Severity`, `DiagCode` (14 §8.1),
//! * [`trace`] — the operation trace used by the oracle harness and the property tests.
//!
//! File access is not this crate's business: callers decode a file with `flyff_vfs` and pass the text.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented
)]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]
#![deny(clippy::iter_over_hash_type)]
#![warn(missing_docs)]

pub mod cnum;
pub mod defines;
pub mod diag;
pub mod prefiles;
pub mod scanner;
pub mod script;
pub mod strings;
pub mod trace;

pub use cnum::{c_atof, c_atoi, c_atoi64, c_isspace, c_sscanf_x};
pub use defines::{DEFINE_HEADERS, Defines};
pub use diag::{DiagCode, DiagSummary, Diagnostic, Diagnostics, Severity};
pub use prefiles::{PreFiles, load_pre_files};
pub use scanner::{MAX_TOKENSTR, Mark, NULL_ID, Scanner, ScannerOptions, TokenType};
pub use script::{CLIENT_KEYWORDS, Script, ScriptCtx, ScriptVariant};
pub use strings::{STRING_TABLE_FILES, StringTable};

/// Crate marker used by the workspace smoke test and by `cargo xtask doctor`.
pub const CRATE_NAME: &str = "flyff-script";
