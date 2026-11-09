//! `CmdArgs`: the command-line tokenizer (Plan/23 §3.5, ref/24 §5.3) over the bit-exact scanner port of
//! `flyff-script`, so tokens behave as the legacy `CScanner`.
//!
//! * whitespace is skipped; delimiters `" !:;,+-<>'/*%^=()&|\"{}"`, TAB, CR, LF; `"quoted strings"` are one token
//!   with the quotes removed (names with spaces or delimiters MUST be quoted: `/te "Mr Bob"`);
//! * a leading digit gives a NUMBER token up to the next delimiter (`1.5` is one token; `0x..` is HEX);
//! * [`CmdArgs::number`] is `GetNumber` (`atoi`; a separate `-` token negates; `=` gives `NULL_ID`;
//!   empty gives 0);
//! * [`CmdArgs::last_full`] is `GetLastFull`: the rest of the line, trimmed (message bodies).

use flyff_script::{Scanner, ScannerOptions, TokenType};
use std::sync::Arc;

/// The token reader of one command line.
pub struct CmdArgs<'a> {
    sc: Scanner<'a>,
}

impl<'a> CmdArgs<'a> {
    /// A reader over `line` (the whole line including the leading `/`).
    pub fn new(line: &'a str) -> Self {
        Self {
            sc: Scanner::new(line, Arc::from("command"), ScannerOptions::default()),
        }
    }

    /// The next token (empty at the end of the input).
    pub fn token(&mut self) -> &str {
        self.sc.get_token(false);
        self.sc.token()
    }

    /// `true` when the last token was a NUMBER.
    pub fn token_is_number(&self) -> bool {
        self.sc.token_type() == TokenType::Number
    }

    /// `GetNumber`: `atoi` of the next token.
    pub fn number(&mut self) -> i32 {
        self.sc.get_number_i32(false)
    }

    /// `GetInt64` (`/rtg`).
    pub fn number_i64(&mut self) -> i64 {
        self.sc.get_int64(false)
    }

    /// `GetFloat`.
    pub fn float(&mut self) -> f32 {
        self.sc.get_float(false)
    }

    /// `GetLastFull`: the rest of the line, trimmed.
    pub fn last_full(&mut self) -> String {
        self.sc.get_last_full().to_owned()
    }

    /// `tok == FINISHED` after a read at the end of the input.
    pub fn finished(&self) -> bool {
        self.sc.finished()
    }

    /// The remainder of the line before any trimming (`m_pProg`; the shout same-text check, 23 §6.4).
    pub fn rest_raw(&self) -> &'a str {
        self.sc.rest()
    }
}
