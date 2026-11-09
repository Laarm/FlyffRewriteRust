//! Lexer for the function bodies (Plan/20 §5.3 step 5).
//!
//! Identifiers, integer and float literals, `( ) { } ; , :`, the operators `== != >= <= < > && || ! -` and
//! the keywords `if else switch case break` (identifiers here, the parser tells them apart). Anything else
//! is an `unsupported_token` error: the bodies of the shipped input contain no string literals, no hex
//! literals, no assignments and no arithmetic.

use crate::error::{ErrorClass, Finding};

/// A token.
#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    /// An identifier or keyword.
    Ident(String),
    /// An integer literal (fits a C `int`).
    Int(i64),
    /// A float literal: canonical text (`7161.0`) and value.
    Float(String, f64),
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `{`
    LBrace,
    /// `}`
    RBrace,
    /// `;`
    Semi,
    /// `,`
    Comma,
    /// `:`
    Colon,
    /// `==`
    EqEq,
    /// `!=`
    NotEq,
    /// `>=`
    Ge,
    /// `<=`
    Le,
    /// `<`
    Lt,
    /// `>`
    Gt,
    /// `&&`
    AndAnd,
    /// `||`
    OrOr,
    /// `!`
    Bang,
    /// `-`
    Minus,
}

impl Tok {
    /// The token as it appears in the normalised body shape (numbers are abstracted by the caller).
    pub fn text(&self) -> String {
        match self {
            Tok::Ident(s) => s.clone(),
            Tok::Int(v) => v.to_string(),
            Tok::Float(t, _) => t.clone(),
            Tok::LParen => "(".into(),
            Tok::RParen => ")".into(),
            Tok::LBrace => "{".into(),
            Tok::RBrace => "}".into(),
            Tok::Semi => ";".into(),
            Tok::Comma => ",".into(),
            Tok::Colon => ":".into(),
            Tok::EqEq => "==".into(),
            Tok::NotEq => "!=".into(),
            Tok::Ge => ">=".into(),
            Tok::Le => "<=".into(),
            Tok::Lt => "<".into(),
            Tok::Gt => ">".into(),
            Tok::AndAnd => "&&".into(),
            Tok::OrOr => "||".into(),
            Tok::Bang => "!".into(),
            Tok::Minus => "-".into(),
        }
    }
}

/// A token with its 1-based line in the original file.
#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    /// The token.
    pub tok: Tok,
    /// Line in `NpcScript.cpp`.
    pub line: u32,
}

/// C++ words that are not helper names and not part of the grammar (reported as unsupported constructs).
const RESERVED: &[&str] = &[
    "default",
    "while",
    "for",
    "do",
    "return",
    "goto",
    "continue",
    "int",
    "float",
    "double",
    "char",
    "long",
    "short",
    "unsigned",
    "signed",
    "bool",
    "void",
    "new",
    "delete",
    "sizeof",
    "static",
    "const",
    "struct",
    "class",
    "typedef",
    "true",
    "false",
    "this",
    "try",
    "catch",
    "throw",
    "auto",
    "enum",
    "union",
    "namespace",
    "using",
];

/// `true` when `word` is a C++ keyword the grammar does not accept.
pub fn is_reserved(word: &str) -> bool {
    RESERVED.contains(&word)
}

/// Splits `body` into tokens. `first_line` is the 1-based line of the first character of `body`.
pub fn lex(subject: &str, body: &str, first_line: u32) -> Result<Vec<Token>, Finding> {
    let mut out = Vec::new();
    let chars: Vec<char> = body.chars().collect();
    let mut i = 0usize;
    let mut line = first_line;
    let err =
        |line: u32, msg: String| Finding::new(ErrorClass::UnsupportedToken, subject, line, msg);
    while let Some(&c) = chars.get(i) {
        if c == '\n' {
            line += 1;
            i += 1;
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        let peek = |k: usize| chars.get(i + k).copied();
        let simple = |t: Tok, n: usize| (t, n);
        let (tok, advance) = match c {
            '(' => simple(Tok::LParen, 1),
            ')' => simple(Tok::RParen, 1),
            '{' => simple(Tok::LBrace, 1),
            '}' => simple(Tok::RBrace, 1),
            ';' => simple(Tok::Semi, 1),
            ',' => simple(Tok::Comma, 1),
            ':' => simple(Tok::Colon, 1),
            '-' => simple(Tok::Minus, 1),
            '=' => {
                if peek(1) == Some('=') {
                    simple(Tok::EqEq, 2)
                } else {
                    return Err(err(
                        line,
                        "assignment `=` is not part of the grammar".into(),
                    ));
                }
            }
            '!' => {
                if peek(1) == Some('=') {
                    simple(Tok::NotEq, 2)
                } else {
                    simple(Tok::Bang, 1)
                }
            }
            '>' => {
                if peek(1) == Some('=') {
                    simple(Tok::Ge, 2)
                } else {
                    simple(Tok::Gt, 1)
                }
            }
            '<' => {
                if peek(1) == Some('=') {
                    simple(Tok::Le, 2)
                } else {
                    simple(Tok::Lt, 1)
                }
            }
            '&' => {
                if peek(1) == Some('&') {
                    simple(Tok::AndAnd, 2)
                } else {
                    return Err(err(line, "bitwise `&` is not part of the grammar".into()));
                }
            }
            '|' => {
                if peek(1) == Some('|') {
                    simple(Tok::OrOr, 2)
                } else {
                    return Err(err(line, "bitwise `|` is not part of the grammar".into()));
                }
            }
            '"' | '\'' => {
                return Err(err(
                    line,
                    "string and character literals are not part of the grammar".into(),
                ));
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let mut j = i;
                let mut s = String::new();
                while let Some(&d) = chars.get(j) {
                    if d.is_ascii_alphanumeric() || d == '_' {
                        s.push(d);
                        j += 1;
                    } else {
                        break;
                    }
                }
                (Tok::Ident(s), j - i)
            }
            c if c.is_ascii_digit() => {
                let (tok, n) = lex_number(&chars, i).map_err(|m| err(line, m))?;
                (tok, n)
            }
            other => {
                return Err(err(line, format!("unexpected character {other:?}")));
            }
        };
        out.push(Token { tok, line });
        i += advance;
    }
    Ok(out)
}

/// Lexes a number at `start`; returns the token and the number of characters consumed.
fn lex_number(chars: &[char], start: usize) -> Result<(Tok, usize), String> {
    let mut j = start;
    let mut int_part = String::new();
    while let Some(&d) = chars.get(j) {
        if d.is_ascii_digit() {
            int_part.push(d);
            j += 1;
        } else {
            break;
        }
    }
    let mut is_float = false;
    let mut frac = String::new();
    if chars.get(j) == Some(&'.') {
        is_float = true;
        j += 1;
        while let Some(&d) = chars.get(j) {
            if d.is_ascii_digit() {
                frac.push(d);
                j += 1;
            } else {
                break;
            }
        }
        if matches!(chars.get(j), Some('f' | 'F')) {
            j += 1;
        }
    }
    if let Some(&d) = chars.get(j)
        && (d.is_ascii_alphanumeric() || d == '_' || d == '.')
    {
        return Err(format!(
            "malformed number near {:?} (hex, suffixed and exponent literals are not part of the grammar)",
            chars
                .get(start..=j)
                .map(|s| s.iter().collect::<String>())
                .unwrap_or_default()
        ));
    }
    if is_float {
        let trimmed = int_part.trim_start_matches('0');
        let int_text = if trimmed.is_empty() { "0" } else { trimmed };
        let frac_text = if frac.is_empty() { "0" } else { frac.as_str() };
        let text = format!("{int_text}.{frac_text}");
        let value: f64 = text
            .parse()
            .map_err(|_| format!("float literal {text} does not parse"))?;
        if !value.is_finite() {
            return Err(format!("float literal {text} is not finite"));
        }
        Ok((Tok::Float(text, value), j - start))
    } else {
        let value: u64 = int_part
            .parse()
            .map_err(|_| format!("integer literal {int_part} is out of range"))?;
        if value > i32::MAX as u64 {
            return Err(format!("integer literal {int_part} does not fit a C int"));
        }
        Ok((Tok::Int(value as i64), j - start))
    }
}
