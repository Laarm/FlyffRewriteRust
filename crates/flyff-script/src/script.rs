//! `CScript` layer: token classification and define/string substitution (Plan/14 §4.1-4.2,
//! ref/17 §5.1-5.2; src:_Common/Script.cpp:164-254).
//!
//! A [`Script`] wraps a [`Scanner`]. Every token read through it is classified (`{`/`}` become
//! [`TokenType::Block`], `Temp` becomes `Keyword` or `Identifier`) and every `Identifier` is replaced
//! by its string-table text (type `Str`) or its define value (type `Number`). Inside a value getter an
//! identifier that is in neither table is reported as `UnknownIdentifier` ("X Not Found.") and stays
//! as it is, so `atoi` yields 0.

use crate::defines::Defines;
use crate::diag::{DiagCode, Diagnostics, Severity};
use crate::scanner::{Mark, Scanner, ScannerOptions, TokenSource, TokenType, getters};
use crate::strings::StringTable;
use std::fmt::Write as _;
use std::sync::Arc;

/// Which executable's keyword rule is used (Plan/14 §4.1).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum ScriptVariant {
    /// The world server (`__REMOVE_SCIRPT_060712`): the only keyword is `#define` (prefix match).
    Server,
    /// The client and the DB server: 18 keywords that are not substituted.
    Client,
}

/// The 18 keywords of the Client variant (src:_Common/Script.cpp:17-41).
pub const CLIENT_KEYWORDS: [&str; 18] = [
    "int", "if", "else", "for", "do", "while", "break", "switch", "answer", "Select", "YesNo",
    "case", "default", "goto", "return", "#define", "#include", "enum",
];

impl ScriptVariant {
    /// `LookUp`: is `token` a keyword in this variant?
    pub fn is_keyword(self, token: &str) -> bool {
        match self {
            // memcmp(s, "#define", 7) == 0: a prefix match.
            Self::Server => token.starts_with("#define"),
            Self::Client => CLIENT_KEYWORDS.contains(&token),
        }
    }
}

/// The tables a [`Script`] substitutes from (the original kept them in process-global statics).
#[derive(Clone, Debug)]
pub struct ScriptCtx {
    /// The numeric `#define` table.
    pub defines: Arc<Defines>,
    /// The `IDS_` string table.
    pub strings: Arc<StringTable>,
    /// Keyword rule.
    pub variant: ScriptVariant,
}

impl ScriptCtx {
    /// A context over the given tables.
    pub fn new(defines: Arc<Defines>, strings: Arc<StringTable>, variant: ScriptVariant) -> Self {
        Self {
            defines,
            strings,
            variant,
        }
    }

    /// A context with empty tables (nothing is substituted).
    pub fn empty(variant: ScriptVariant) -> Self {
        Self::new(
            Arc::new(Defines::default()),
            Arc::new(StringTable::default()),
            variant,
        )
    }
}

/// A tokenizer with classification and substitution. See the module documentation.
pub struct Script<'a> {
    sc: Scanner<'a>,
    ctx: &'a ScriptCtx,
    diags: &'a mut Diagnostics,
    in_getter: bool,
    divergence: u32,
}

impl<'a> Script<'a> {
    /// Creates a script over `text`; `file` is used in diagnostics, which go to `diags`.
    pub fn new(
        text: &'a str,
        file: Arc<str>,
        opts: ScannerOptions,
        ctx: &'a ScriptCtx,
        diags: &'a mut Diagnostics,
    ) -> Self {
        Self {
            sc: Scanner::new(text, file, opts),
            ctx,
            diags,
            in_getter: false,
            divergence: 0,
        }
    }

    // ---- accessors -------------------------------------------------------------------------

    /// The current token text (substituted).
    pub fn token(&self) -> &str {
        self.sc.token()
    }

    /// The current token type (after classification and substitution).
    pub fn token_type(&self) -> TokenType {
        self.sc.token_type()
    }

    /// `true` when the last token read hit the end of input.
    pub fn finished(&self) -> bool {
        self.sc.finished()
    }

    /// `true` when the current token text equals `s`.
    pub fn is(&self, s: &str) -> bool {
        self.sc.token() == s
    }

    /// 1-based line of the cursor.
    pub fn line(&self) -> u32 {
        self.sc.line()
    }

    /// The file name given at construction.
    pub fn file(&self) -> &Arc<str> {
        self.sc.file()
    }

    /// `SetMark`.
    pub fn mark(&self) -> Mark {
        self.sc.mark()
    }

    /// `GoMark` (cursor only).
    pub fn reset(&mut self, m: Mark) {
        self.sc.reset(m);
    }

    /// The underlying raw scanner (no substitution).
    pub fn scanner(&self) -> &Scanner<'a> {
        &self.sc
    }

    /// The underlying raw scanner, for loaders that mix raw and substituted reads.
    /// Diagnostics reported through it are forwarded on the next `Script` call.
    pub fn scanner_mut(&mut self) -> &mut Scanner<'a> {
        &mut self.sc
    }

    /// The substitution tables and the variant.
    pub fn ctx(&self) -> &ScriptCtx {
        self.ctx
    }

    /// The diagnostics sink.
    pub fn diags_mut(&mut self) -> &mut Diagnostics {
        self.flush_scanner_diags();
        self.diags
    }

    /// Number of identifier tokens that the Server variant substituted but the Client variant would
    /// have treated as keywords (`ClientVariantDivergence`, Plan/14 §10.6; reference value 0).
    pub fn divergence(&self) -> u32 {
        self.divergence
    }

    /// Reports a diagnostic at the cursor.
    pub fn report(&mut self, severity: Severity, code: DiagCode, message: impl Into<String>) {
        self.flush_scanner_diags();
        let line = self.sc.line();
        self.diags
            .report(severity, code, Some(self.sc.file()), line, message);
    }

    fn flush_scanner_diags(&mut self) {
        if !self.sc.diagnostics().is_empty() {
            let d = self.sc.take_diagnostics();
            self.diags.extend(d);
        }
    }

    // ---- tokens ----------------------------------------------------------------------------

    /// `CScript::GetTkn`: raw token, then classification only.
    fn get_tkn(&mut self, comma: bool) -> TokenType {
        let ty = self.sc.get_token(comma);
        self.flush_scanner_diags();
        match ty {
            TokenType::Delimiter => {
                if matches!(self.sc.token().as_bytes().first(), Some(b'{' | b'}')) {
                    self.sc.set_type(TokenType::Block);
                    return TokenType::Block;
                }
                ty
            }
            TokenType::Temp => {
                let t = if self.ctx.variant.is_keyword(self.sc.token()) {
                    TokenType::Keyword
                } else {
                    TokenType::Identifier
                };
                self.sc.set_type(t);
                t
            }
            _ => ty,
        }
    }

    /// `CScript::GetToken_NoDef`: classification without substitution (resdata `APP_`/`WIDC_` names).
    pub fn get_token_nodef(&mut self) -> TokenType {
        self.get_tkn(false)
    }

    /// `CScript::GetToken()` in whitespace mode, with substitution.
    pub fn get_token(&mut self) -> TokenType {
        self.get_token_inner(false).0
    }

    /// `CScript::GetToken(TRUE)`: comma mode, with substitution.
    pub fn get_token_comma(&mut self) -> TokenType {
        self.get_token_inner(true).0
    }

    /// Like [`Self::get_token`], but also returns the original identifier when it was substituted
    /// (an `IDS_` key or a define name), so loaders can keep the key (`name_key`).
    pub fn get_token_with_key(&mut self) -> (TokenType, Option<String>) {
        self.get_token_inner(false)
    }

    fn get_token_inner(&mut self, comma: bool) -> (TokenType, Option<String>) {
        let ty = self.get_tkn(comma);
        if ty != TokenType::Identifier {
            return (ty, None);
        }
        let ctx = self.ctx;
        // 1. string table first
        if let Some(text) = ctx.strings.get(self.sc.token()) {
            let key = self.sc.token().to_owned();
            self.note_divergence(&key);
            self.sc.set_token(TokenType::Str, text);
            return (TokenType::Str, Some(key));
        }
        // 2. define table second
        if let Some(value) = ctx.defines.get(self.sc.token()) {
            let key = self.sc.token().to_owned();
            self.note_divergence(&key);
            self.sc.set_token_i32(TokenType::Number, value);
            return (TokenType::Number, Some(key));
        }
        // 3. unknown identifier inside a getter
        if self.in_getter {
            let first = self.sc.token().as_bytes().first().copied();
            if first.is_some_and(|c| !matches!(c, b'=' | b'-' | b'+')) {
                let msg = format!("{} Not Found.", self.sc.token());
                self.report(Severity::Error, DiagCode::UnknownIdentifier, msg);
            }
        }
        (TokenType::Identifier, None)
    }

    fn note_divergence(&mut self, key: &str) {
        if self.ctx.variant == ScriptVariant::Server
            && ScriptVariant::Client.is_keyword(key)
            && !ScriptVariant::Server.is_keyword(key)
        {
            self.divergence = self.divergence.saturating_add(1);
            self.report(
                Severity::Info,
                DiagCode::ClientVariantDivergence,
                format!("`{key}` is substituted by the server but is a keyword for the client"),
            );
        }
    }

    // ---- getters ---------------------------------------------------------------------------

    /// `GetNumber(FALSE)`.
    ///
    /// Hex: the text is lower-cased and parsed nibble by nibble from the right without validation.
    /// Otherwise an empty token gives 0, `=` gives `NULL_ID`; `-` reads the next token and returns
    /// `-atoi(next)` (a hex next token is not hex-parsed: `-0x10` is -10); `+` reads the next token and
    /// continues with it; anything else is `atoi(text)` (a `Str` token too).
    pub fn get_number(&mut self) -> u32 {
        getters::number(self, false)
    }

    /// `GetNumber(TRUE)`: comma mode (the sign is inside the token; hex keeps its `0x`).
    pub fn get_number_comma(&mut self) -> u32 {
        getters::number(self, true)
    }

    /// `GetNumber` assigned to a C `int`.
    pub fn get_number_i32(&mut self) -> i32 {
        self.get_number() as i32
    }

    /// `GetNumber(TRUE)` assigned to a C `int`.
    pub fn get_number_i32_comma(&mut self) -> i32 {
        self.get_number_comma() as i32
    }

    /// `GetInt64`/`GetExpInteger`: like [`Self::get_number`], but hex gives 0 and `=` gives 4294967295.
    pub fn get_int64(&mut self) -> i64 {
        getters::int64(self, false)
    }

    /// `GetInt64(TRUE)`.
    pub fn get_int64_comma(&mut self) -> i64 {
        getters::int64(self, true)
    }

    /// `GetFloat`: empty is 0, `=` is -1, `-`/`+` read the next token, otherwise `atof(text)`.
    pub fn get_float(&mut self) -> f32 {
        getters::float(self, false)
    }

    /// `GetFloat(TRUE)`.
    pub fn get_float_comma(&mut self) -> f32 {
        getters::float(self, true)
    }

    /// `GetHex`: the hex parse of a hex token, else 0.
    pub fn get_hex(&mut self) -> u32 {
        getters::hex(self, false)
    }

    /// `CScript::GetLastFull`: the rest of the line, trimmed (no substitution).
    pub fn get_last_full(&mut self) -> &str {
        self.sc.get_last_full()
    }

    /// `CProject::GetLangScript`: the next token (substituted), skipping the `)` and `;` tokens after
    /// it; an empty token becomes a single space.
    pub fn lang_script(&mut self) -> String {
        self.get_token();
        let s = self.sc.token().to_owned();
        self.get_token(); // )
        self.get_token(); // ;
        if s.is_empty() { " ".to_owned() } else { s }
    }
}

impl TokenSource for Script<'_> {
    fn next_token(&mut self, comma: bool) -> TokenType {
        self.get_token_inner(comma).0
    }

    fn text(&self) -> &str {
        self.sc.token()
    }

    fn enter_getter(&mut self) {
        self.in_getter = true;
    }

    fn leave_getter(&mut self) {
        self.in_getter = false;
    }

    fn overflow(&mut self) {
        let msg = format!(
            "number \"{}\" is out of range and saturates",
            self.sc.token()
        );
        self.report(Severity::Warning, DiagCode::NumberOverflow, msg);
    }
}

impl Scanner<'_> {
    /// Replaces the current token by the decimal text of `value` (`wsprintf("%d")`).
    pub(crate) fn set_token_i32(&mut self, ty: TokenType, value: i32) {
        let mut buf = String::new();
        // Writing to a String cannot fail.
        let _ = write!(buf, "{value}");
        self.set_token(ty, &buf);
    }
}
