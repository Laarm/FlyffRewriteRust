//! The character-name rules: `InvalidName_ENG.inc`, `Letter_ENG.inc` and `Letter1_ENG.inc` (L46;
//! `CProject::LoadInvalidName`, `LoadAllowedLetter`; Plan/14 §5.3, ref/17 §8.11, ref/24 §10;
//! src:_Common/ProjectCmn.cpp:782-893).
//!
//! * `InvalidName_ENG.inc`: one banned substring per token (lower-cased with `strlwr`); a name is invalid
//!   when its lower-cased form **contains** one of them (`IsInvalidName`). The original keeps a `set<string>`:
//!   `invalid_substrings` is sorted and unique.
//! * `Letter_ENG.inc`: the allowed name characters, one token each; only the **first byte** of each token
//!   is used (`set<char>`). `None` (no file, or an empty set) means no restriction (`IsAllowedLetter`).
//! * `Letter1_ENG.inc` (vendor titles): missing in this data set, so no restriction.
//!
//! The rules are used by the server (character, guild and party names) and exported to the client in the
//! `names` bundle.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// The name rules (`m_sInvalidNames`, `m_sAllowedLetter`, `m_sAllowedLetter2`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NameRules {
    /// The banned substrings, lower-cased, sorted and unique.
    pub invalid_substrings: Vec<String>,
    /// The allowed bytes of character names; `None` means "everything".
    pub allowed_letters: Option<BTreeSet<u8>>,
    /// The allowed bytes of vendor titles; `None` means "everything".
    pub vendor_allowed_letters: Option<BTreeSet<u8>>,
}

impl NameRules {
    /// `IsInvalidName`: `true` when the lower-cased `name` contains a banned substring.
    pub fn is_invalid_name(&self, name: &str) -> bool {
        let lower = name.to_ascii_lowercase();
        self.invalid_substrings
            .iter()
            .any(|w| lower.contains(w.as_str()))
    }

    /// `IsAllowedLetter(name, vendor)`: every byte of `name` is in the allowed set (an empty set allows
    /// everything). `__VENDOR_1106` is on, so a vendor title is checked against `Letter1_ENG.inc`
    /// (`vendor_allowed`) and no longer gets the free `' '` and `'+'` of the older variant.
    pub fn is_allowed_letter(&self, name: &str, vendor: bool) -> bool {
        let set = if vendor {
            &self.vendor_allowed_letters
        } else {
            &self.allowed_letters
        };
        set.as_ref()
            .is_none_or(|s| name.bytes().all(|b| s.contains(&b)))
    }

    /// The allowed bytes of a set as a string, ascending; `None` for no restriction. Non-ASCII bytes are
    /// shown as the Latin-1 character of the same value.
    pub fn allowed_string(set: &Option<BTreeSet<u8>>) -> Option<String> {
        set.as_ref()
            .map(|s| s.iter().map(|&b| char::from(b)).collect())
    }
}

#[cfg(feature = "load")]
pub(crate) use parse::{parse_allowed_letters, parse_invalid_names};

#[cfg(feature = "load")]
mod parse {
    use flyff_script::{Diagnostics, Scanner, ScannerOptions};
    use std::collections::BTreeSet;
    use std::sync::Arc;

    /// `LoadInvalidName`: every token, lower-cased (a sorted unique list, like the original `set<string>`).
    pub(crate) fn parse_invalid_names(
        text: &str,
        file: &Arc<str>,
        diags: &mut Diagnostics,
    ) -> Vec<String> {
        let mut out = BTreeSet::new();
        let mut sc = Scanner::new(text, Arc::clone(file), ScannerOptions::default());
        sc.get_token(false);
        while !sc.finished() {
            out.insert(sc.token().to_ascii_lowercase());
            sc.get_token(false);
        }
        diags.extend(sc.take_diagnostics());
        out.into_iter().collect()
    }

    /// `LoadAllowedLetter`: the first byte of every non-empty token; `None` when the set is empty.
    pub(crate) fn parse_allowed_letters(
        text: &str,
        file: &Arc<str>,
        diags: &mut Diagnostics,
    ) -> Option<BTreeSet<u8>> {
        let mut out = BTreeSet::new();
        let mut sc = Scanner::new(text, Arc::clone(file), ScannerOptions::default());
        sc.get_token(false);
        while !sc.finished() {
            if let Some(&b) = sc.token().as_bytes().first() {
                out.insert(b);
            }
            sc.get_token(false);
        }
        diags.extend(sc.take_diagnostics());
        (!out.is_empty()).then_some(out)
    }
}
