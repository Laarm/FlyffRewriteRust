//! Consistency checks of the loaded data (Plan/14 §9, §10.9; WP-DATA-19, WP-DATA-17).
//!
//! * [`report`]: the reference counts C01-C36 ([`report::count_rows`]), the spot checks of §9.2
//!   ([`report::spot_checks`]) and the report type [`report::DataReport`] (wasm-safe),
//! * `xref` (feature `load`): the cross-reference checks of §9.3, the `extra_checks` hook and
//!   [`xref::run`] = load + checks + report, which `flyff check` calls,
//! * `export` (feature `load`): the `flyff export gamedata-json` sections.
//!
//! The define-drift check of WP-DATA-19 is in this file:
//!
//! * [`define_drift`] compares every generated constant of `flyff_common::defs` with the runtime define
//!   table (called in P8). Code that uses compile-time ids is only correct while the headers still say
//!   the same, so a modded header is reported: a value mismatch is an **Error** `DefineDrift`, names the
//!   headers do not define at all are one aggregated **Warning** `DefineDrift`.
//! * [`render_define_headers`] writes the define headers from the constants (the synthetic pack of
//!   `tools/synthetic-pack` uses it, so CI data and code agree).

use flyff_common::defs;
use flyff_script::{Defines, DiagCode, Diagnostic, Severity};
use std::sync::Arc;

#[cfg(feature = "load")]
pub mod export;
pub mod report;
#[cfg(feature = "load")]
pub mod xref;

/// How many missing names the aggregated warning lists.
const MISSING_SAMPLE: usize = 8;

/// Compares `flyff_common::defs::ALL` with `defines` (first definition wins on both sides; the generated
/// values are the 32-bit patterns of the runtime `i32` values).
///
/// * a value mismatch: one `Error` per name (with the header and line that define it),
/// * names missing at runtime: a single `Warning` with the count and the first few names.
///
/// Returns an empty vector when the table equals the reference data.
pub fn define_drift(defines: &Defines) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let mut missing: Vec<&str> = Vec::new();
    let mut missing_count = 0usize;
    for (name, value) in defs::ALL {
        match defines.get(name) {
            None => {
                missing_count += 1;
                if missing.len() < MISSING_SAMPLE {
                    missing.push(name);
                }
            }
            Some(v) if i64::from(v as u32) != *value => {
                let (file, line) = defines
                    .origin(name)
                    .map_or((None, 0), |(f, l)| (Some(Arc::<str>::from(f)), l));
                out.push(Diagnostic {
                    severity: Severity::Error,
                    code: DiagCode::DefineDrift,
                    file,
                    line,
                    message: format!(
                        "define {name} is {} but the code was generated with {value}",
                        i64::from(v as u32)
                    ),
                });
            }
            Some(_) => {}
        }
    }
    if missing_count > 0 {
        out.push(Diagnostic {
            severity: Severity::Warning,
            code: DiagCode::DefineDrift,
            file: None,
            line: 0,
            message: format!(
                "{missing_count} generated define(s) are not defined by the headers (first: {})",
                missing.join(", ")
            ),
        });
    }
    out
}

/// Renders the define headers from the constants: `(header file name, content)` for the 17 headers, each
/// line `#define NAME value` with CRLF. Values of 2^31 and above are written as `0x........` (the scanner
/// reads a hex literal as its 32-bit pattern). `only` restricts the output to the names it accepts.
pub fn render_define_headers(only: impl Fn(&str) -> bool) -> Vec<(String, String)> {
    defs::BY_HEADER
        .iter()
        .map(|h| {
            let mut s = String::new();
            for (name, value) in h.entries {
                if !only(name) {
                    continue;
                }
                if *value >= 1 << 31 {
                    s.push_str(&format!("#define {name} 0x{value:08X}\r\n"));
                } else {
                    s.push_str(&format!("#define {name} {value}\r\n"));
                }
            }
            (h.file.to_owned(), s)
        })
        .collect()
}
