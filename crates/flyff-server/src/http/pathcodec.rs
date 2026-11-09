//! Strict URL path decoding for `/res` and `/gen` (Plan/24 §5.1, §9.1).
//!
//! The server accepts exactly one spelling per file: every `/`-separated segment is percent-decoded on its own,
//! and the result must be canonical (`flyff_common::normalize_resource_path` of itself): ASCII only, lower case,
//! `/` separators, no empty, `.` or `..` segments, no leading or trailing `/`. An encoded slash (`%2F`) or
//! backslash inside a segment, a NUL or control character, and an invalid escape are all refused. Spaces exist in
//! real file names (`Obj_ AzriaThicket01.o3d`) and arrive as `%20`.

/// Decodes `%XX` escapes strictly: a `%` not followed by two hex digits is an error, as is a result that is not
/// UTF-8.
pub fn percent_decode(segment: &str) -> Option<String> {
    let bytes = segment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes.get(i) {
            Some(b'%') => {
                let hi = hex(*bytes.get(i + 1)?)?;
                let lo = hex(*bytes.get(i + 2)?)?;
                out.push(hi << 4 | lo);
                i += 3;
            }
            Some(b) => {
                out.push(*b);
                i += 1;
            }
            None => break,
        }
    }
    String::from_utf8(out).ok()
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Decodes the part of a URL path after a fixed prefix into a canonical VFS-style path; `None` = `400 bad path`.
pub fn decode_canonical(rest: &str) -> Option<String> {
    if rest.is_empty() || rest.len() > 1024 {
        return None;
    }
    let mut segments = Vec::new();
    for raw in rest.split('/') {
        let seg = percent_decode(raw)?;
        if !segment_ok(&seg) {
            return None;
        }
        segments.push(seg);
    }
    let joined = segments.join("/");
    // canonical: normalising changes nothing (this also rejects leading/trailing whitespace)
    match flyff_common::normalize_resource_path(&joined) {
        Ok(n) if n == joined => Some(joined),
        _ => None,
    }
}

fn segment_ok(seg: &str) -> bool {
    !seg.is_empty()
        && seg != "."
        && seg != ".."
        && seg.bytes().all(|b| {
            b.is_ascii()
                && b >= 0x20
                && b != 0x7f
                && b != b'/'
                && b != b'\\'
                && !b.is_ascii_uppercase()
        })
}

/// A 16-digit lower-hex fingerprint segment.
pub fn parse_fp(text: &str) -> Option<flyff_assets::Fp> {
    (text.len() == 16 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
        .then(|| u64::from_str_radix(text, 16).ok().map(flyff_assets::Fp))
        .flatten()
}
