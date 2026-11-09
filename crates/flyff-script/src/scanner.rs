//! `CScanner` port (Plan/14 §3, ref/17 §4; src:_Common/scanner.cpp:15-32, 439-1003).
//!
//! The input is decoded UTF-8 text viewed as bytes. The cursor is a byte offset; `byte_at(i)` returns
//! `0` beyond the end, which emulates the two NUL bytes the original appends, and a literal NUL inside
//! the text ends the scan exactly as in C. All behaviour of the original that matters for the data is
//! reproduced, including its quirks (see the `KEEP-QUIRK` notes); the documented deviations of
//! Plan/14 §3.6 are the LF terminators, UTF-8 multibyte sequences acting like DBCS characters,
//! `Mark`/`reset` instead of `PutBack`, and typed diagnostics instead of message boxes.
//!
//! Nothing in here panics: all indexing goes through bounds-checked accessors.

use crate::cnum::{c_atof, c_atoi_checked, c_atoi64_checked};
use crate::diag::{DiagCode, Diagnostic, Diagnostics, Severity};
use std::cell::Cell;
use std::sync::Arc;

/// `NULL_ID` of the original (`0xFFFFFFFF`).
pub const NULL_ID: u32 = 0xFFFF_FFFF;

/// `MAX_TOKENSTR`: the longest `"` string the scanner copies (Plan/14 §3.3 step 7).
pub const MAX_TOKENSTR: usize = 2048;

/// Token type; the order is that of `enum TokenType` in `scanner.h:28-31`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum TokenType {
    /// Identifier-like or unknown token (the scanner itself only produces this, not `Identifier`).
    Temp,
    /// Punctuation / operator.
    Delimiter,
    /// A `Temp` that is not a keyword (produced by [`crate::Script`]).
    Identifier,
    /// Decimal number text (`1.0f`, `12abc` and `1e5` are each one token).
    Number,
    /// `0x...`; the text excludes the prefix in whitespace mode.
    Hex,
    /// A keyword (produced by [`crate::Script`]).
    Keyword,
    /// A `"` string; the text excludes the quotes.
    Str,
    /// `{` or `}` (produced by [`crate::Script`]).
    Block,
}

/// A saved cursor (`SetMark`/`GoMark`). Like the original only the cursor is restored; the current
/// token, its type and `finished` stay those of the last token read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Mark {
    pos: usize,
}

/// Scanner switches (Plan/14 §3.6).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ScannerOptions {
    /// FIX-LF: every scan that stops at `\r` also stops at `\n` (comments, strings, comma fields,
    /// `get_last_full`). Default `true`. Test T-SCAN-EQ asserts identical token streams for every
    /// Resource file with the option on and off.
    pub lf_terminators: bool,
}

impl Default for ScannerOptions {
    fn default() -> Self {
        Self {
            lf_terminators: true,
        }
    }
}

/// `iswhite`: `0 < c <= 0x20` (NUL is not white).
#[inline]
pub(crate) fn iswhite(c: u8) -> bool {
    c > 0 && c <= 0x20
}

/// `isdelim`: `" !:;,+-<>'/*%^=()&|\"{}"`, TAB, CR, LF and NUL.
#[inline]
pub(crate) fn isdelim(c: u8) -> bool {
    matches!(
        c,
        b' ' | b'!'
            | b':'
            | b';'
            | b','
            | b'+'
            | b'-'
            | b'<'
            | b'>'
            | b'\''
            | b'/'
            | b'*'
            | b'%'
            | b'^'
            | b'='
            | b'('
            | b')'
            | b'&'
            | b'|'
            | b'"'
            | b'{'
            | b'}'
            | b'\t'
            | b'\r'
            | b'\n'
            | 0
    )
}

/// The single-character delimiter set `+-*^/%=;(),':{}.`.
#[inline]
fn is_single_delim(c: u8) -> bool {
    matches!(
        c,
        b'+' | b'-'
            | b'*'
            | b'^'
            | b'/'
            | b'%'
            | b'='
            | b';'
            | b'('
            | b')'
            | b','
            | b'\''
            | b':'
            | b'{'
            | b'}'
            | b'.'
    )
}

/// The byte-level tokenizer. See the module documentation.
pub struct Scanner<'a> {
    src: &'a str,
    buf: &'a [u8],
    pos: usize,
    file: Arc<str>,
    opts: ScannerOptions,
    token: String,
    ty: TokenType,
    finished: bool,
    error_check: bool,
    diags: Diagnostics,
    /// `(position, line)` of the last `line_of` query (queries are mostly monotonic).
    line_cache: Cell<(usize, u32)>,
}

impl<'a> Scanner<'a> {
    /// Creates a scanner over `text`; `file` is only used in diagnostics.
    pub fn new(text: &'a str, file: Arc<str>, opts: ScannerOptions) -> Self {
        Self {
            src: text,
            buf: text.as_bytes(),
            pos: 0,
            file,
            opts,
            token: String::new(),
            ty: TokenType::Temp,
            finished: false,
            error_check: true,
            diags: Diagnostics::new(),
            line_cache: Cell::new((0, 1)),
        }
    }

    // ---- accessors -------------------------------------------------------------------------

    /// The current token text (after substitution when read through [`crate::Script`]).
    pub fn token(&self) -> &str {
        &self.token
    }

    /// The current token type.
    pub fn token_type(&self) -> TokenType {
        self.ty
    }

    /// `true` once the last `get_token` hit the end of input (`tok == FINISHED`).
    pub fn finished(&self) -> bool {
        self.finished
    }

    /// The cursor (byte offset; may exceed the text length by one or two after a skip at the end).
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// The file name given at construction.
    pub fn file(&self) -> &Arc<str> {
        &self.file
    }

    /// The options.
    pub fn options(&self) -> ScannerOptions {
        self.opts
    }

    /// The whole text.
    pub fn source(&self) -> &'a str {
        self.src
    }

    /// The not yet consumed text (empty at the end).
    pub fn rest(&self) -> &'a str {
        self.src.get(self.pos..).unwrap_or("")
    }

    /// 1-based line of the cursor.
    pub fn line(&self) -> u32 {
        self.line_of(self.pos)
    }

    /// 1-based line of a byte offset: 1 + the number of `\n` before it.
    pub fn line_of(&self, pos: usize) -> u32 {
        let pos = pos.min(self.buf.len());
        let (cached_pos, cached_line) = self.line_cache.get();
        let (from, line) = if pos >= cached_pos {
            (cached_pos, cached_line)
        } else {
            (0, 1)
        };
        let n = self
            .buf
            .get(from..pos)
            .unwrap_or_default()
            .iter()
            .filter(|&&b| b == b'\n')
            .count();
        let line = line.saturating_add(u32::try_from(n).unwrap_or(u32::MAX));
        self.line_cache.set((pos, line));
        line
    }

    /// `SetErrorCheck`: when `false`, string errors are not reported.
    pub fn set_error_check(&mut self, on: bool) {
        self.error_check = on;
    }

    /// Diagnostics reported by the scanner itself (string errors, number overflow).
    pub fn diagnostics(&self) -> &Diagnostics {
        &self.diags
    }

    /// Takes the scanner's diagnostics, leaving it empty.
    pub fn take_diagnostics(&mut self) -> Diagnostics {
        std::mem::take(&mut self.diags)
    }

    /// `SetMark`: remembers the cursor.
    pub fn mark(&self) -> Mark {
        Mark { pos: self.pos }
    }

    /// `GoMark`: restores the cursor (only the cursor, see [`Mark`]).
    pub fn reset(&mut self, m: Mark) {
        self.pos = m.pos;
    }

    /// Replaces the current token (used by [`crate::Script`] for substitution).
    pub(crate) fn set_token(&mut self, ty: TokenType, text: &str) {
        self.token.clear();
        self.token.push_str(text);
        self.ty = ty;
    }

    /// Changes only the type of the current token.
    pub(crate) fn set_type(&mut self, ty: TokenType) {
        self.ty = ty;
    }

    pub(crate) fn report(&mut self, severity: Severity, code: DiagCode, line: u32, msg: String) {
        let file = self.file.clone();
        self.diags.push(Diagnostic {
            severity,
            code,
            file: Some(file),
            line,
            message: msg,
        });
    }

    /// Raw `strchr`-style skip used by `PreScan` for "functions": skips to the next `)` and one byte
    /// more (`while(*p!=')') p++; p++;`), stopping at the end of input instead of running off the
    /// buffer as the C code would.
    pub(crate) fn skip_past_close_paren(&mut self) {
        while self.cur() != b')' && self.cur() != 0 {
            self.pos += 1;
        }
        if self.cur() == b')' {
            self.pos += 1;
        }
    }

    // ---- byte access -----------------------------------------------------------------------

    #[inline]
    fn at(&self, i: usize) -> u8 {
        self.buf.get(i).copied().unwrap_or(0)
    }

    #[inline]
    fn cur(&self) -> u8 {
        self.at(self.pos)
    }

    /// Byte length of the (UTF-8) character starting at `i`: the DBCS rule of Plan/14 §3.1. Bytes
    /// below `0xC2`, and anything that does not start a valid sequence, count as one byte.
    fn mb_len(&self, i: usize) -> usize {
        let n = match self.at(i) {
            0xC2..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF4 => 4,
            _ => return 1,
        };
        if self.src.get(i..i + n).is_some() {
            n
        } else {
            1
        }
    }

    /// `IsMultiByte`: the byte at `i` starts a multibyte sequence.
    #[inline]
    fn is_multibyte(&self, i: usize) -> bool {
        self.at(i) >= 0xC2 && self.mb_len(i) > 1
    }

    /// `true` when a scan loop that stops at `\r` must stop at `c` (FIX-LF).
    #[inline]
    fn is_eol(&self, c: u8) -> bool {
        c == b'\r' || (self.opts.lf_terminators && c == b'\n')
    }

    /// Appends `buf[a..b]` to the token. Boundaries are char boundaries except after a mid-sequence
    /// skip; such bytes become U+FFFD instead of invalid UTF-8.
    fn push_range(&mut self, a: usize, b: usize) {
        let b = b.min(self.buf.len());
        if a >= b {
            return;
        }
        match self.src.get(a..b) {
            Some(s) => self.token.push_str(s),
            None => {
                let bytes = self.buf.get(a..b).unwrap_or_default();
                self.token.push_str(&String::from_utf8_lossy(bytes));
            }
        }
    }

    // ---- tokenizer -------------------------------------------------------------------------

    /// Skips white space and comments (Plan/14 §3.3 step 1). Returns `true` when an unterminated
    /// `/* ... ` ran into the end of input.
    fn skip_ws_and_comments(&mut self) -> bool {
        loop {
            let mut again = false;
            while iswhite(self.cur()) {
                self.pos += 1;
            }
            while self.cur() == b'/' {
                self.pos += 1;
                match self.cur() {
                    b'/' => {
                        self.pos += 1;
                        while !self.is_eol(self.cur()) && self.cur() != 0 {
                            self.pos += 1;
                        }
                        match self.cur() {
                            // KEEP-QUIRK: assumes "\r\n"; for "\r\r\n" the second CR is skipped and
                            // the LF stays as white space.
                            b'\r' => self.pos += 2,
                            // Only reachable with `lf_terminators`.
                            b'\n' => self.pos += 1,
                            _ => {}
                        }
                    }
                    b'*' => {
                        self.pos += 1;
                        loop {
                            while self.cur() != b'*' && self.cur() != 0 {
                                self.pos += 1;
                            }
                            if self.cur() == 0 {
                                return true;
                            }
                            self.pos += 1;
                            if self.cur() == b'/' {
                                break;
                            }
                        }
                        self.pos += 1;
                        while iswhite(self.cur()) {
                            self.pos += 1;
                        }
                    }
                    _ => {
                        // A lone '/': not a comment; it is tokenized as a delimiter.
                        self.pos = self.pos.saturating_sub(1);
                        break;
                    }
                }
                again = true;
            }
            if !again {
                return false;
            }
        }
    }

    /// The raw `CScanner::GetToken(bComma)` (Plan/14 §3.3 and §3.4).
    pub fn get_token(&mut self, comma: bool) -> TokenType {
        self.token.clear();
        self.ty = TokenType::Temp;
        self.finished = false;

        if self.skip_ws_and_comments() || self.cur() == 0 {
            self.finished = true;
            self.ty = TokenType::Delimiter;
            return self.ty;
        }
        if comma {
            return self.comma_token();
        }

        let c = self.cur();
        let n1 = self.at(self.pos + 1);

        // Relational operators.
        match c {
            b'=' if n1 == b'=' => return self.delim_run(2),
            b'!' | b'<' | b'>' => {
                let len = if n1 == b'=' { 2 } else { 1 };
                return self.delim_run(len);
            }
            _ => {}
        }
        // "&&" and "||"; a lone '&' or '|' falls through to the one-byte TEMP case.
        if (c == b'&' || c == b'|') && n1 == c {
            return self.delim_run(2);
        }
        if is_single_delim(c) {
            return self.delim_run(1);
        }
        if c == b'"' {
            return self.string_token();
        }
        // Hex: "0x" with a lowercase x only.
        if c == b'0' && n1 == b'x' {
            self.pos += 2;
            self.scan_until_delim();
            self.ty = TokenType::Hex;
            return self.ty;
        }
        // Number: never a multibyte lead.
        if c.is_ascii_digit() {
            self.scan_until_delim();
            self.ty = TokenType::Number;
            return self.ty;
        }
        // Identifier.
        if c.is_ascii_alphabetic()
            || self.is_multibyte(self.pos)
            || matches!(c, b'#' | b'_' | b'@' | b'$' | b'?')
        {
            self.scan_until_delim();
        } else if c < 0x80 {
            self.token.push(char::from(c));
            self.pos += 1;
        } else {
            // A stray byte that does not start a UTF-8 sequence (only after a mid-sequence skip).
            self.token.push('\u{FFFD}');
            self.pos += 1;
        }
        self.ty = TokenType::Temp;
        self.ty
    }

    /// Emits a delimiter token of `len` bytes at the cursor.
    fn delim_run(&mut self, len: usize) -> TokenType {
        let start = self.pos;
        self.pos += len;
        self.push_range(start, self.pos);
        self.ty = TokenType::Delimiter;
        self.ty
    }

    /// Copies from the cursor up to the next delimiter (`while(!isdelim(*p)) copy`).
    fn scan_until_delim(&mut self) {
        let start = self.pos;
        while !isdelim(self.cur()) {
            self.pos += 1;
        }
        self.push_range(start, self.pos);
    }

    /// `"..."` in whitespace mode (Plan/14 §3.3 step 7).
    fn string_token(&mut self) -> TokenType {
        let start_pos = self.pos;
        self.pos += 1; // opening quote
        loop {
            let c = self.cur();
            if c == b'"' || self.is_eol(c) || c == 0 || self.token.len() >= MAX_TOKENSTR {
                break;
            }
            let start = self.pos;
            // Copy a run of plain bytes in one go while the 2048-byte cap cannot be reached.
            if c < 0xC2 {
                let mut end = self.pos + 1;
                let budget = MAX_TOKENSTR - self.token.len();
                while end - start < budget {
                    let d = self.at(end);
                    if d == b'"' || self.is_eol(d) || d == 0 || d >= 0xC2 {
                        break;
                    }
                    end += 1;
                }
                self.pos = end;
                self.push_range(start, end);
            } else {
                let n = self.mb_len(self.pos);
                self.pos += n;
                self.push_range(start, self.pos);
            }
        }
        self.pos += 1; // closing quote (or the byte that ended the string)
        self.ty = TokenType::Str;
        if self.at(self.pos - 1) != b'"' {
            if self.at(self.pos - 1) == 0 {
                self.pos -= 1;
            }
            if self.error_check {
                let line = self.line_of(start_pos);
                if self.token.len() >= MAX_TOKENSTR {
                    let msg = format!(
                        "string \"{}\" exceeds {MAX_TOKENSTR} bytes",
                        truncate_for_message(&self.token)
                    );
                    self.report(Severity::Error, DiagCode::StringTooLong, line, msg);
                } else {
                    let msg = format!(
                        "string \"{}\" does not end with a quote",
                        truncate_for_message(&self.token)
                    );
                    self.report(Severity::Error, DiagCode::UnterminatedString, line, msg);
                }
            }
        }
        self.ty
    }

    /// Comma mode (Plan/14 §3.4): only `propSkillAdd.csv` uses it.
    fn comma_token(&mut self) -> TokenType {
        let start;
        if self.cur() == b'"' {
            self.pos += 1;
            start = self.pos;
            while self.cur() != b'"' && !self.is_eol(self.cur()) && self.cur() != 0 {
                self.pos += 1;
            }
            self.push_range(start, self.pos);
            self.pos += 1;
            self.ty = TokenType::Str;
        } else {
            start = self.pos;
            while self.cur() != b',' && !self.is_eol(self.cur()) && self.cur() != 0 {
                self.pos += 1;
            }
            self.push_range(start, self.pos);
        }
        // Skip ONE separator; the LF of a CRLF is skipped by the next call's white-space skip.
        if self.cur() == b',' || self.is_eol(self.cur()) {
            self.pos += 1;
        }
        // Trim trailing white space.
        while self.token.as_bytes().last().copied().is_some_and(iswhite) {
            self.token.pop();
        }
        // Type: a leading digit makes it a Number, "0x" then overrides that with Hex (the text keeps
        // the prefix in comma mode).
        if let Some(&first) = self.token.as_bytes().first() {
            if first.is_ascii_digit() {
                self.ty = TokenType::Number;
            }
            if self.token.starts_with("0x") {
                self.ty = TokenType::Hex;
            }
        }
        self.ty
    }

    /// `GetLastFull` (Plan/14 §3.5): the rest of the line, trimmed. The terminator is not consumed.
    /// Only the token text changes; its type and `finished` stay as they were.
    pub fn get_last_full(&mut self) -> &str {
        self.token.clear();
        while iswhite(self.cur()) && !self.is_eol(self.cur()) {
            self.pos += 1;
        }
        let start = self.pos;
        while self.cur() != 0 && !self.is_eol(self.cur()) {
            self.pos += 1;
        }
        self.push_range(start, self.pos);
        while self.token.as_bytes().last().copied().is_some_and(iswhite) {
            self.token.pop();
        }
        &self.token
    }

    // ---- raw value getters -----------------------------------------------------------------

    /// `GetNumber(bComma)` without substitution: see [`crate::Script::get_number`] for the rules.
    pub fn get_number(&mut self, comma: bool) -> u32 {
        getters::number(self, comma)
    }

    /// `GetNumber` read as a C `int`.
    pub fn get_number_i32(&mut self, comma: bool) -> i32 {
        self.get_number(comma) as i32
    }

    /// `GetInt64`/`GetExpInteger` without substitution.
    pub fn get_int64(&mut self, comma: bool) -> i64 {
        getters::int64(self, comma)
    }

    /// `GetFloat` without substitution.
    pub fn get_float(&mut self, comma: bool) -> f32 {
        getters::float(self, comma)
    }

    /// `GetHex` without substitution.
    pub fn get_hex(&mut self, comma: bool) -> u32 {
        getters::hex(self, comma)
    }
}

/// Cuts long text for diagnostic messages (on a char boundary).
fn truncate_for_message(s: &str) -> &str {
    const MAX: usize = 80;
    if s.len() <= MAX {
        return s;
    }
    let mut end = MAX;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s.get(..end).unwrap_or("")
}

/// What the value getters need from a token source: the raw [`Scanner`] or the substituting
/// [`crate::Script`].
pub(crate) trait TokenSource {
    /// Reads the next token (with substitution for `Script`).
    fn next_token(&mut self, comma: bool) -> TokenType;
    /// The current token text.
    fn text(&self) -> &str;
    /// `m_dwDef = 1`.
    fn enter_getter(&mut self) {}
    /// `m_dwDef = 0`.
    fn leave_getter(&mut self) {}
    /// A conversion saturated.
    fn overflow(&mut self) {}
}

impl TokenSource for Scanner<'_> {
    fn next_token(&mut self, comma: bool) -> TokenType {
        self.get_token(comma)
    }
    fn text(&self) -> &str {
        &self.token
    }
    fn overflow(&mut self) {
        let line = self.line();
        let msg = format!(
            "number \"{}\" is out of range and saturates",
            truncate_for_message(&self.token)
        );
        self.report(Severity::Warning, DiagCode::NumberOverflow, line, msg);
    }
}

/// `CScanner::GetNumber` & friends, written once for both token sources (Plan/14 §3.5).
pub(crate) mod getters {
    use super::{NULL_ID, TokenSource, TokenType, c_atof, c_atoi_checked, c_atoi64_checked};

    /// Hex text -> value, byte for byte as the C loop (no validation; a byte >= 0x80 gives a negative
    /// nibble that sign-extends; the shift count wraps modulo 64 like x86 `shl`).
    pub(crate) fn hex_value(text: &str) -> u32 {
        let mut acc: u64 = 0;
        let mut shift: u32 = 0;
        for &b in text.as_bytes().iter().rev() {
            let c = i8::from_ne_bytes([b.to_ascii_lowercase()]);
            let v: i8 = if c >= 0x61 {
                // At most 0x7F - 0x61 + 10 = 40: fits.
                (i32::from(c) - 0x61 + 10) as i8
            } else {
                c.wrapping_sub(0x30)
            };
            acc |= (i64::from(v) as u64).wrapping_shl(shift);
            shift = shift.wrapping_add(4);
        }
        acc as u32
    }

    fn atoi<T: TokenSource>(t: &mut T) -> i32 {
        let (v, overflow) = c_atoi_checked(t.text().as_bytes());
        if overflow {
            t.overflow();
        }
        v
    }

    fn atoi64<T: TokenSource>(t: &mut T) -> i64 {
        let (v, overflow) = c_atoi64_checked(t.text().as_bytes());
        if overflow {
            t.overflow();
        }
        v
    }

    pub(crate) fn number<T: TokenSource>(t: &mut T, comma: bool) -> u32 {
        t.enter_getter();
        let v = number_inner(t, comma);
        t.leave_getter();
        v
    }

    fn number_inner<T: TokenSource>(t: &mut T, comma: bool) -> u32 {
        if t.next_token(comma) == TokenType::Hex {
            return hex_value(t.text());
        }
        match t.text().as_bytes().first().copied() {
            None => 0,
            Some(b'=') => NULL_ID,
            Some(b'-') => {
                if comma {
                    // The sign is inside the token.
                    atoi(t) as u32
                } else {
                    t.next_token(false);
                    atoi(t).wrapping_neg() as u32
                }
            }
            Some(b'+') => {
                if !comma {
                    t.next_token(false);
                }
                atoi(t) as u32
            }
            Some(_) => atoi(t) as u32,
        }
    }

    pub(crate) fn int64<T: TokenSource>(t: &mut T, comma: bool) -> i64 {
        t.enter_getter();
        let v = int64_inner(t, comma);
        t.leave_getter();
        v
    }

    fn int64_inner<T: TokenSource>(t: &mut T, comma: bool) -> i64 {
        if t.next_token(comma) == TokenType::Hex {
            // KEEP-QUIRK: GetInt64 returns 0 for hex tokens.
            return 0;
        }
        match t.text().as_bytes().first().copied() {
            None => 0,
            Some(b'=') => i64::from(NULL_ID),
            Some(b'-') => {
                if comma {
                    atoi64(t)
                } else {
                    t.next_token(false);
                    atoi64(t).wrapping_neg()
                }
            }
            Some(b'+') => {
                if !comma {
                    t.next_token(false);
                }
                atoi64(t)
            }
            Some(_) => atoi64(t),
        }
    }

    pub(crate) fn float<T: TokenSource>(t: &mut T, comma: bool) -> f32 {
        t.enter_getter();
        t.next_token(comma);
        let v = match t.text().as_bytes().first().copied() {
            None => 0.0,
            Some(b'=') => -1.0,
            Some(b'-') => {
                t.next_token(comma);
                (-c_atof(t.text().as_bytes())) as f32
            }
            Some(b'+') => {
                t.next_token(comma);
                c_atof(t.text().as_bytes()) as f32
            }
            Some(_) => c_atof(t.text().as_bytes()) as f32,
        };
        t.leave_getter();
        v
    }

    pub(crate) fn hex<T: TokenSource>(t: &mut T, comma: bool) -> u32 {
        t.enter_getter();
        let v = if t.next_token(comma) == TokenType::Hex {
            hex_value(t.text())
        } else {
            0
        };
        t.leave_getter();
        v
    }
}
