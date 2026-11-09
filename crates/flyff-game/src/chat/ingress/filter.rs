//! The profanity filter (Plan/23 §6.9, PL-111, F-309): `CWndMgr::WordChange` (`WndManager.cpp:3810`) over the
//! word table of `filter_ENG.inc` (`CProject::LoadFilter`, `Project.cpp:1140`), ported line for line on the
//! bit-exact scanner of `flyff-script`.
//!
//! Quirks reproduced on purpose (KEEP-QUIRK, part of PL-111): runs of spaces collapse to one, leading spaces
//! vanish and `"..."` quotes disappear in filtered channels.

use flyff_script::{Scanner, ScannerOptions};
use std::collections::BTreeMap;
use std::sync::Arc;

use flyff_common::det::DetMap;

/// The two word tables: pure-alphabetic keys (lower-cased, whole-token match) and every other key (substring
/// replace in byte order).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChatFilter {
    /// Keys whose every byte is an ASCII letter, lower-cased; the first insertion of a key wins.
    alpha: DetMap<String, String>,
    /// The other keys verbatim, iterated in byte order (`std::map` order); the first insertion wins.
    non_alpha: BTreeMap<String, String>,
}

impl ChatFilter {
    /// The filter of `ClientData.chat_filter` (word and replacement as written, file order).
    pub fn from_pairs(pairs: &[(String, String)]) -> ChatFilter {
        let mut f = ChatFilter::default();
        for (src, dst) in pairs {
            if src.is_empty() {
                continue;
            }
            if src.bytes().all(|b| b.is_ascii_alphabetic()) {
                f.alpha
                    .entry(src.to_ascii_lowercase())
                    .or_insert_with(|| dst.clone());
            } else {
                f.non_alpha
                    .entry(src.clone())
                    .or_insert_with(|| dst.clone());
            }
        }
        f
    }

    /// `true` when the filter has no word.
    pub fn is_empty(&self) -> bool {
        self.alpha.is_empty() && self.non_alpha.is_empty()
    }

    /// Number of alphabetic and non-alphabetic keys.
    pub fn len(&self) -> (usize, usize) {
        (self.alpha.len(), self.non_alpha.len())
    }

    /// `WordChange`: the filtered text (`*` x length for an alphabetic word, the file's replacement for the
    /// others).
    pub fn word_change(&self, s: &str) -> String {
        let file: Arc<str> = Arc::from("chat");
        let opts = ScannerOptions::default();
        let mut result = String::with_capacity(s.len());
        let mut outer = Scanner::new(s, Arc::clone(&file), opts);
        outer.get_token(false);
        let mut first = outer.rest().starts_with(' ');
        while !outer.finished() {
            let token = outer.token().to_owned();
            let mut inner = Scanner::new(&token, Arc::clone(&file), opts);
            inner.get_token(false);
            let mut second = inner.rest().starts_with(' ');
            let mut terminate = inner.rest().is_empty();
            while !inner.finished() {
                let t1 = inner.token().to_owned();
                let lower = t1.to_ascii_lowercase();
                if self.alpha.contains_key(&lower) {
                    result.extend(std::iter::repeat_n('*', lower.chars().count()));
                } else {
                    result.push_str(&t1);
                }
                if (terminate && first) || (!terminate && second) {
                    result.push(' ');
                }
                inner.get_token(false);
                second = inner.rest().starts_with(' ');
                terminate = inner.rest().is_empty();
            }
            outer.get_token(false);
            first = outer.rest().starts_with(' ');
        }
        for (k, v) in &self.non_alpha {
            if !k.is_empty() {
                result = result.replace(k.as_str(), v);
            }
        }
        result
    }
}

/// The vendor-title filter of `flyff_rules::names` (23 §7): the title goes through `word_change`.
impl flyff_rules::names::TitleFilter for ChatFilter {
    fn filter(&self, s: &str) -> String {
        self.word_change(s)
    }
}
