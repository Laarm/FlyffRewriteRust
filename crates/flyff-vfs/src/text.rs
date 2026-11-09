//! Text decoding (Plan/14 §2.6, ref/17 §3.3).
//!
//! The original read a file as raw bytes; a leading `FF FE` made it convert the rest from UTF-16LE to
//! the process code page (stopping at the first NUL). The rewrite decodes every file once to UTF-8.
//! The first matching rule wins:
//!
//! 1. `FF FE`: UTF-16LE, cut at the first U+0000 (**KEEP-QUIRK**); optionally lossy to windows-1252,
//! 2. `EF BB BF`: UTF-8 without the BOM; `FE FF`: UTF-16BE (extensions for admin-authored files),
//! 3. an explicit hint (`Cp1252`, `Cp949`, `Utf16Le` without BOM),
//! 4. valid UTF-8 (all ASCII files qualify),
//! 5. otherwise CP949 (WHATWG EUC-KR = Windows-949; it never swallows an ASCII byte into an invalid
//!    multibyte sequence, so delimiters, quotes and line ends survive).

use encoding_rs::{EUC_KR, UTF_16BE, UTF_16LE, WINDOWS_1252};

/// A caller-supplied encoding hint (Plan/14 §2.6).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default)]
pub enum EncodingHint {
    /// Decide from BOM and content.
    #[default]
    Auto,
    /// Force windows-1252 for BOM-less files (`WorldDialog.txt`, accented letter lists).
    Cp1252,
    /// Force CP949 for BOM-less files.
    Cp949,
    /// Treat BOM-less bytes as UTF-16LE.
    Utf16Le,
}

/// The encoding a file was found to have.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum SourceEncoding {
    /// UTF-16LE with BOM.
    Utf16Le,
    /// UTF-16BE with BOM (extension).
    Utf16Be,
    /// Valid UTF-8 without BOM (includes plain ASCII).
    Utf8,
    /// UTF-8 with BOM (extension).
    Utf8Bom,
    /// CP949 (Korean ANSI).
    Cp949,
    /// windows-1252.
    Cp1252,
}

/// The result of [`decode_text`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DecodedText {
    /// The decoded UTF-8 text (line endings untouched).
    pub text: String,
    /// The encoding that was used.
    pub encoding: SourceEncoding,
    /// Number of U+FFFD that malformed input produced (Info for CP949 comments, Warning elsewhere).
    pub replaced_chars: u32,
    /// UTF-16 only: the text was cut at its first U+0000.
    pub truncated_at_nul: bool,
    /// Number of non-NUL characters that followed the cut and were dropped (Warning when `> 0`).
    pub dropped_after_nul: u32,
}

/// Decodes `bytes` to UTF-8 (rules in the module documentation). With `cp1252_lossy`, text that came
/// from UTF-16 additionally has every character that windows-1252 cannot encode replaced by `?`
/// (emulates the original's conversion to the ANSI code page; config `data.cp1252_lossy`).
pub fn decode_text(bytes: &[u8], hint: EncodingHint, cp1252_lossy: bool) -> DecodedText {
    // Rule 1: UTF-16LE with BOM.
    if let Some(body) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return decode_utf16le(body, cp1252_lossy);
    }
    // Rule 2: BOM extensions.
    if let Some(body) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        let (cow, _) = encoding_rs::UTF_8.decode_without_bom_handling(body);
        let text = cow.into_owned();
        return DecodedText {
            replaced_chars: count_replacements(&text),
            text,
            encoding: SourceEncoding::Utf8Bom,
            truncated_at_nul: false,
            dropped_after_nul: 0,
        };
    }
    if let Some(body) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let (cow, _) = UTF_16BE.decode_without_bom_handling(body);
        let text = cow.into_owned();
        return DecodedText {
            replaced_chars: count_replacements(&text),
            text,
            encoding: SourceEncoding::Utf16Be,
            truncated_at_nul: false,
            dropped_after_nul: 0,
        };
    }
    // Rule 3: explicit hint.
    match hint {
        EncodingHint::Cp1252 => return decode_single(bytes, SourceEncoding::Cp1252),
        EncodingHint::Cp949 => return decode_single(bytes, SourceEncoding::Cp949),
        EncodingHint::Utf16Le => return decode_utf16le(bytes, cp1252_lossy),
        EncodingHint::Auto => {}
    }
    // Rule 4: valid UTF-8.
    if let Ok(s) = std::str::from_utf8(bytes) {
        return DecodedText {
            text: s.to_owned(),
            encoding: SourceEncoding::Utf8,
            replaced_chars: 0,
            truncated_at_nul: false,
            dropped_after_nul: 0,
        };
    }
    // Rule 5: CP949.
    decode_single(bytes, SourceEncoding::Cp949)
}

fn decode_single(bytes: &[u8], enc: SourceEncoding) -> DecodedText {
    let codec = match enc {
        SourceEncoding::Cp1252 => WINDOWS_1252,
        _ => EUC_KR,
    };
    let (cow, _) = codec.decode_without_bom_handling(bytes);
    let text = cow.into_owned();
    DecodedText {
        replaced_chars: count_replacements(&text),
        text,
        encoding: enc,
        truncated_at_nul: false,
        dropped_after_nul: 0,
    }
}

fn decode_utf16le(body: &[u8], cp1252_lossy: bool) -> DecodedText {
    let (cow, _) = UTF_16LE.decode_without_bom_handling(body);
    let mut text = cow.into_owned();
    // KEEP-QUIRK: WideCharToMultiByte(..., -1, ...) stops at the first NUL.
    let mut truncated = false;
    let mut dropped = 0u32;
    if let Some(nul) = text.find('\0') {
        let tail = text.get(nul..).unwrap_or_default();
        dropped = u32::try_from(tail.chars().filter(|&c| c != '\0').count()).unwrap_or(u32::MAX);
        text.truncate(nul);
        truncated = true;
    }
    let replaced_chars = count_replacements(&text);
    if cp1252_lossy {
        text = to_cp1252_lossy(&text);
    }
    DecodedText {
        text,
        encoding: SourceEncoding::Utf16Le,
        replaced_chars,
        truncated_at_nul: truncated,
        dropped_after_nul: dropped,
    }
}

fn count_replacements(text: &str) -> u32 {
    u32::try_from(text.chars().filter(|&c| c == '\u{FFFD}').count()).unwrap_or(u32::MAX)
}

/// Replaces every character that windows-1252 cannot encode by `?`.
fn to_cp1252_lossy(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut buf = [0u8; 4];
    for c in text.chars() {
        if c.is_ascii() {
            out.push(c);
            continue;
        }
        let (_, _, unmappable) = WINDOWS_1252.encode(c.encode_utf8(&mut buf));
        out.push(if unmappable { '?' } else { c });
    }
    out
}

/// The encoding hint for a file, from its normalised path (Plan/14 §2.6 hint table): `WorldDialog.txt`
/// and the accented-letter lists of the other languages are windows-1252; everything else is `Auto`.
pub fn hint_for(normalised_path: &str) -> EncodingHint {
    let name = normalised_path
        .rsplit('/')
        .next()
        .unwrap_or(normalised_path);
    let is_inc = name.ends_with(".inc");
    let cp1252 = name == "worlddialog.txt"
        || (is_inc
            && (name.starts_with("letter_")
                || name.starts_with("letter1_")
                || name.starts_with("letter2_")
                || ["invalidname_", "filter_"].iter().any(|p| {
                    ["fre", "ger", "spa"]
                        .iter()
                        .any(|l| name == format!("{p}{l}.inc"))
                })));
    if cp1252 {
        EncodingHint::Cp1252
    } else {
        EncodingHint::Auto
    }
}
