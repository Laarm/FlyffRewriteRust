//! Ingress normalisation helpers (Plan/23 §6.1; WP-GM-05): the pure steps live in `flyff_rules::chat` (shared
//! with the browser); this module adds the `GetLastFull` rule of the message bodies.

pub use flyff_rules::chat::{
    CHAT_DROP_BYTES, CHAT_MAX_CHARS, cut_chars, formalize_chat_string, normalize_line,
    replace_controls,
};

/// `CScanner::GetLastFull`: the rest of the line up to the first CR or LF, trimmed on both sides.
pub fn last_full(s: &str) -> &str {
    let end = s.find(['\r', '\n']).unwrap_or(s.len());
    s.get(..end)
        .unwrap_or(s)
        .trim_matches(|c: char| c.is_ascii_whitespace() || c == '\0')
}

/// `StringTrimRight`: removes trailing ASCII whitespace.
pub fn trim_right(s: &str) -> &str {
    s.trim_end_matches(|c: char| c.is_ascii_whitespace())
}
