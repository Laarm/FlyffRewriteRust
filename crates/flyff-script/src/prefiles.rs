//! `CProject::LoadPreFiles` without the file access (Plan/14 §4.3-4.4): the 17 define headers, then the
//! 64 string-table entries, in the original order.
//!
//! The crate does no I/O, so the caller supplies a `read` closure that maps a file name as written in
//! the original code (`"defineItem.h"`, `"Client\\help.txt.txt"`; resolution is the caller's business and
//! must be case-insensitive) to its decoded UTF-8 text, or `None` when the file does not exist. A missing
//! file is skipped with an Info `FileMissingOptional`, as in the original. `textClient.inc`
//! (`LoadText`) is not part of this step (WP-DATA-04).

use crate::defines::{DEFINE_HEADERS, Defines};
use crate::diag::{DiagCode, Diagnostics, Severity};
use crate::strings::{STRING_TABLE_FILES, StringTable};

/// The result of [`load_pre_files`].
#[derive(Debug, Default)]
pub struct PreFiles {
    /// The merged define table (first definition wins).
    pub defines: Defines,
    /// The merged string table (first key wins).
    pub strings: StringTable,
    /// Everything reported while loading.
    pub diags: Diagnostics,
    /// Per header that was read: names added by that file.
    pub header_counts: Vec<(&'static str, usize)>,
    /// Per string-table entry that was read (a file listed twice appears twice): keys added.
    pub string_counts: Vec<(&'static str, usize)>,
    /// Headers that `read` could not supply.
    pub missing_headers: Vec<&'static str>,
    /// String-table entries that `read` could not supply.
    pub missing_tables: Vec<&'static str>,
}

/// Loads [`DEFINE_HEADERS`] and then [`STRING_TABLE_FILES`] (see the module documentation).
pub fn load_pre_files(mut read: impl FnMut(&str) -> Option<String>) -> PreFiles {
    let mut out = PreFiles::default();
    for name in DEFINE_HEADERS {
        let key = file_key(name);
        match read(name) {
            Some(text) => {
                let added = out.defines.scan_header(&text, &key, &mut out.diags);
                out.header_counts.push((name, added));
            }
            None => {
                missing(&mut out.diags, &key);
                out.missing_headers.push(name);
            }
        }
    }
    for name in STRING_TABLE_FILES {
        let key = file_key(name);
        match read(name) {
            Some(text) => {
                let added = out
                    .strings
                    .load_text(&text, &key, &out.defines, &mut out.diags);
                out.string_counts.push((name, added));
            }
            None => {
                missing(&mut out.diags, &key);
                out.missing_tables.push(name);
            }
        }
    }
    out
}

fn file_key(name: &str) -> String {
    flyff_common::normalize_resource_path(name).unwrap_or_else(|_| name.to_owned())
}

fn missing(diags: &mut Diagnostics, key: &str) {
    diags.report(
        Severity::Info,
        DiagCode::FileMissingOptional,
        None,
        0,
        format!("{key} does not exist and is skipped"),
    );
}
