//! Parser and type pass (Plan/20 §5.3 steps 6-8).
//!
//! ```text
//! body      := stmt*
//! stmt      := call ';' | if_stmt | switch_stmt | '{' stmt* '}' | ';'
//! if_stmt   := 'if' '(' cond ')' stmt [ 'else' stmt ]
//! switch    := 'switch' '(' 'Random' '(' INT ')' ')' '{' case* '}'
//! case      := 'case' INT ':' stmt* 'break' ';'            // 'case a: case b:' = grouped labels
//! cond      := or ;  or := and ('||' and)* ;  and := unary ('&&' unary)* ;  unary := '!' unary | rel
//! rel       := term [ ('=='|'!='|'>='|'<='|'<'|'>') term ]
//! term      := call | number | CONST | '(' cond ')' | '-' number
//! call      := HELPER '(' [ arg (',' arg)* ] ')'
//! ```
//!
//! The first error of a body ends its parse. Helper names, arities, void-in-expression and the constants are
//! resolved while parsing; the constants are looked up through the `is_defined` callback.

use crate::ast::{Call, Case, Expr, RelOp, Stmt};
use crate::error::{ErrorClass, Finding};
use crate::helpers::{self, ArgKind};
use crate::lex::{Tok, Token, is_reserved};
use std::collections::BTreeSet;

const MAX_DEPTH: usize = 128;
/// Operands of one `&&`/`||` chain (the AST of a chain is as deep as it is long).
const MAX_CHAIN: usize = 512;

/// The result of parsing one body.
#[derive(Clone, Debug, Default)]
pub struct Parsed {
    /// The statements.
    pub body: Vec<Stmt>,
    /// The define constants used (without `TRUE`/`FALSE`).
    pub consts: BTreeSet<String>,
}

/// Parses `tokens` (the lexed body of the C++ function `subject`).
pub fn parse_body(
    subject: &str,
    tokens: &[Token],
    is_defined: &dyn Fn(&str) -> bool,
) -> Result<Parsed, Finding> {
    let mut p = Parser {
        toks: tokens,
        pos: 0,
        subject,
        is_defined,
        consts: BTreeSet::new(),
        depth: 0,
    };
    let mut body = Vec::new();
    while p.peek().is_some() {
        body.extend(p.stmt()?);
    }
    Ok(Parsed {
        body,
        consts: p.consts,
    })
}

/// `true` for `UPPER_CASE` identifiers (the constants).
pub fn is_upper_const(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_uppercase())
        && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

struct Parser<'a> {
    toks: &'a [Token],
    pos: usize,
    subject: &'a str,
    is_defined: &'a dyn Fn(&str) -> bool,
    consts: BTreeSet<String>,
    depth: usize,
}

type R<T> = Result<T, Finding>;

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&'a Token> {
        self.toks.get(self.pos)
    }

    fn peek_tok(&self) -> Option<&'a Tok> {
        self.peek().map(|t| &t.tok)
    }

    fn peek_at(&self, k: usize) -> Option<&'a Tok> {
        self.toks.get(self.pos + k).map(|t| &t.tok)
    }

    fn line(&self) -> u32 {
        self.peek()
            .or_else(|| self.toks.last())
            .map_or(0, |t| t.line)
    }

    fn bump(&mut self) -> Option<&'a Token> {
        let t = self.toks.get(self.pos);
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn err(&self, class: ErrorClass, msg: impl Into<String>) -> Finding {
        Finding::new(class, self.subject, self.line(), msg)
    }

    fn syntax<T>(&self, msg: impl Into<String>) -> R<T> {
        Err(self.err(ErrorClass::Syntax, msg))
    }

    fn describe(&self) -> String {
        self.peek_tok().map_or_else(
            || "the end of the function".to_owned(),
            |t| format!("`{}`", t.text()),
        )
    }

    fn expect(&mut self, want: &Tok, what: &str) -> R<()> {
        if self.peek_tok() == Some(want) {
            self.pos += 1;
            Ok(())
        } else {
            self.syntax(format!("expected {what}, found {}", self.describe()))
        }
    }

    fn is_ident(&self, word: &str) -> bool {
        matches!(self.peek_tok(), Some(Tok::Ident(s)) if s == word)
    }

    fn enter(&mut self) -> R<()> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return self.syntax("nesting is too deep");
        }
        Ok(())
    }

    // ---- statements ------------------------------------------------------------------------

    fn stmt(&mut self) -> R<Vec<Stmt>> {
        self.enter()?;
        let r = self.stmt_inner();
        self.depth -= 1;
        r
    }

    fn stmt_inner(&mut self) -> R<Vec<Stmt>> {
        match self.peek_tok() {
            None => self.syntax("a statement is missing at the end of the function"),
            Some(Tok::Semi) => {
                self.pos += 1;
                Ok(Vec::new())
            }
            Some(Tok::LBrace) => {
                self.pos += 1;
                let mut out = Vec::new();
                loop {
                    match self.peek_tok() {
                        Some(Tok::RBrace) => {
                            self.pos += 1;
                            return Ok(out);
                        }
                        None => return self.syntax("unbalanced braces: `}` is missing"),
                        _ => out.extend(self.stmt()?),
                    }
                }
            }
            Some(Tok::Ident(w)) if w == "if" => self.if_stmt(),
            Some(Tok::Ident(w)) if w == "switch" => self.switch_stmt(),
            Some(Tok::Ident(w)) if w == "else" => self.syntax("`else` without a matching `if`"),
            Some(Tok::Ident(w)) if w == "case" || w == "break" => {
                self.syntax(format!("`{w}` outside the top level of a switch case"))
            }
            Some(Tok::Ident(_)) => {
                let call = self.call()?;
                self.expect(&Tok::Semi, "`;` after the call")?;
                Ok(vec![Stmt::Call(call)])
            }
            Some(_) => self.syntax(format!("expected a statement, found {}", self.describe())),
        }
    }

    fn if_stmt(&mut self) -> R<Vec<Stmt>> {
        let line = self.line();
        self.pos += 1; // if
        self.expect(&Tok::LParen, "`(` after `if`")?;
        let cond = self.cond()?;
        let cond = to_bool(cond);
        self.expect(&Tok::RParen, "`)` after the condition")?;
        let then = self.stmt()?;
        let els = if self.is_ident("else") {
            self.pos += 1;
            self.stmt()?
        } else {
            Vec::new()
        };
        Ok(vec![Stmt::If {
            cond,
            then,
            els,
            line,
        }])
    }

    fn switch_stmt(&mut self) -> R<Vec<Stmt>> {
        let line = self.line();
        self.pos += 1; // switch
        self.expect(&Tok::LParen, "`(` after `switch`")?;
        if !self.is_ident("Random") {
            return Err(self.err(
                ErrorClass::UnsupportedToken,
                "only `switch (Random(n))` is part of the grammar",
            ));
        }
        self.pos += 1;
        self.expect(&Tok::LParen, "`(` after `Random`")?;
        let random_arg = match self.bump().map(|t| &t.tok) {
            Some(Tok::Int(n)) => *n,
            _ => {
                return Err(self.err(
                    ErrorClass::UnsupportedToken,
                    "`Random` in a switch takes an integer literal",
                ));
            }
        };
        self.expect(&Tok::RParen, "`)` after the argument of `Random`")?;
        self.expect(&Tok::RParen, "`)` after the switch expression")?;
        self.expect(&Tok::LBrace, "`{` after the switch expression")?;
        let mut cases: Vec<Case> = Vec::new();
        let mut seen: BTreeSet<i64> = BTreeSet::new();
        loop {
            match self.peek_tok() {
                Some(Tok::RBrace) => {
                    self.pos += 1;
                    break;
                }
                None => return self.syntax("unbalanced braces: `}` of the switch is missing"),
                Some(Tok::Ident(w)) if w == "case" => {}
                Some(Tok::Ident(w)) if w == "default" => {
                    return Err(self.err(
                        ErrorClass::UnsupportedToken,
                        "`default` is not part of the grammar",
                    ));
                }
                _ => {
                    return self.syntax(format!(
                        "expected `case` or `}}` in the switch, found {}",
                        self.describe()
                    ));
                }
            }
            // one case: grouped labels, then the statements
            let mut labels = Vec::new();
            while self.is_ident("case") {
                self.pos += 1;
                let label = match self.bump().map(|t| &t.tok) {
                    Some(Tok::Int(n)) => *n,
                    _ => {
                        return Err(self.err(
                            ErrorClass::UnsupportedToken,
                            "a case label is an integer literal",
                        ));
                    }
                };
                self.expect(&Tok::Colon, "`:` after the case label")?;
                if !seen.insert(label) {
                    return self.syntax(format!("duplicate case label {label}"));
                }
                labels.push(label);
            }
            let mut body = Vec::new();
            loop {
                match self.peek_tok() {
                    Some(Tok::Ident(w)) if w == "break" => {
                        self.pos += 1;
                        self.expect(&Tok::Semi, "`;` after `break`")?;
                        cases.push(Case { labels, body });
                        break;
                    }
                    Some(Tok::Ident(w)) if w == "case" => {
                        if body.is_empty() {
                            // grouped labels after statements are not possible; an empty body followed by
                            // `case` was consumed by the label loop above
                            return self.syntax("internal: unconsumed case label");
                        }
                        return Err(self.err(
                            ErrorClass::SwitchFallThrough,
                            format!(
                                "case {} falls through into the next case (no `break`)",
                                labels
                                    .iter()
                                    .map(i64::to_string)
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                        ));
                    }
                    Some(Tok::RBrace) => {
                        // the last case may end at the closing brace: nothing to fall into
                        cases.push(Case { labels, body });
                        break;
                    }
                    Some(Tok::Ident(w)) if w == "default" => {
                        return Err(self.err(
                            ErrorClass::UnsupportedToken,
                            "`default` is not part of the grammar",
                        ));
                    }
                    None => return self.syntax("unbalanced braces: `}` of the switch is missing"),
                    _ => body.extend(self.stmt()?),
                }
            }
        }
        Ok(vec![Stmt::Switch {
            random_arg,
            cases,
            line,
        }])
    }

    // ---- expressions -----------------------------------------------------------------------

    fn cond(&mut self) -> R<Expr> {
        self.enter()?;
        let r = self.or_expr();
        self.depth -= 1;
        r
    }

    fn or_expr(&mut self) -> R<Expr> {
        let mut lhs = self.and_expr()?;
        let mut operands = 1usize;
        while self.peek_tok() == Some(&Tok::OrOr) {
            self.pos += 1;
            operands += 1;
            if operands > MAX_CHAIN {
                return self.syntax("a `||` chain is too long");
            }
            let rhs = self.and_expr()?;
            lhs = Expr::Or(Box::new(to_bool(lhs)), Box::new(to_bool(rhs)));
        }
        Ok(lhs)
    }

    fn and_expr(&mut self) -> R<Expr> {
        let mut lhs = self.unary()?;
        let mut operands = 1usize;
        while self.peek_tok() == Some(&Tok::AndAnd) {
            self.pos += 1;
            operands += 1;
            if operands > MAX_CHAIN {
                return self.syntax("a `&&` chain is too long");
            }
            let rhs = self.unary()?;
            lhs = Expr::And(Box::new(to_bool(lhs)), Box::new(to_bool(rhs)));
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> R<Expr> {
        if self.peek_tok() == Some(&Tok::Bang) {
            self.pos += 1;
            self.enter()?;
            let operand = self.unary();
            self.depth -= 1;
            let operand = operand?;
            return Ok(if operand.is_bool() {
                Expr::Not(Box::new(operand))
            } else {
                Expr::Rel(RelOp::Eq, Box::new(operand), Box::new(Expr::Int(0)))
            });
        }
        self.rel()
    }

    fn rel(&mut self) -> R<Expr> {
        let lhs = self.term()?;
        let op = match self.peek_tok() {
            Some(Tok::EqEq) => RelOp::Eq,
            Some(Tok::NotEq) => RelOp::Ne,
            Some(Tok::Ge) => RelOp::Ge,
            Some(Tok::Le) => RelOp::Le,
            Some(Tok::Lt) => RelOp::Lt,
            Some(Tok::Gt) => RelOp::Gt,
            _ => return Ok(lhs),
        };
        self.pos += 1;
        let rhs = self.term()?;
        Ok(Expr::Rel(op, Box::new(to_num(lhs)), Box::new(to_num(rhs))))
    }

    fn term(&mut self) -> R<Expr> {
        let Some(tok) = self.peek_tok() else {
            return self.syntax("an operand is missing at the end of the function");
        };
        match tok {
            Tok::Int(v) => {
                self.pos += 1;
                Ok(Expr::Int(*v))
            }
            Tok::Float(t, v) => {
                self.pos += 1;
                Ok(Expr::Float(t.clone(), *v))
            }
            Tok::Minus => {
                self.pos += 1;
                match self.peek_tok() {
                    Some(Tok::Int(v)) => {
                        let v = *v;
                        self.pos += 1;
                        Ok(Expr::Int(-v))
                    }
                    Some(Tok::Float(t, v)) => {
                        let (t, v) = (t.clone(), *v);
                        self.pos += 1;
                        Ok(Expr::Float(format!("-{t}"), -v))
                    }
                    _ => Err(self.err(
                        ErrorClass::UnsupportedToken,
                        "arithmetic is not part of the grammar: `-` is only the sign of a number",
                    )),
                }
            }
            Tok::LParen => {
                self.pos += 1;
                let inner = self.cond()?;
                self.expect(&Tok::RParen, "`)`")?;
                Ok(inner)
            }
            Tok::Ident(name) => {
                if self.peek_at(1) == Some(&Tok::LParen) {
                    let call = self.call()?;
                    if !call.helper.returns_value {
                        return Err(Finding::new(
                            ErrorClass::Type,
                            self.subject,
                            call.line,
                            format!(
                                "`{}` returns no value but is used in an expression",
                                call.helper.name
                            ),
                        ));
                    }
                    return Ok(Expr::Call(call));
                }
                self.constant(name)
            }
            other => self.syntax(format!("expected an operand, found `{}`", other.text())),
        }
    }

    fn constant(&mut self, name: &str) -> R<Expr> {
        if is_reserved(name)
            || name == "if"
            || name == "else"
            || name == "switch"
            || name == "case"
            || name == "break"
        {
            return Err(self.err(
                ErrorClass::UnsupportedToken,
                format!("the keyword `{name}` is not part of the grammar here"),
            ));
        }
        if !is_upper_const(name) {
            return Err(self.err(
                ErrorClass::UnsupportedToken,
                format!("`{name}` is neither a helper call nor a constant (variables are not part of the grammar)"),
            ));
        }
        self.pos += 1;
        match name {
            "TRUE" => Ok(Expr::Int(1)),
            "FALSE" => Ok(Expr::Int(0)),
            _ => {
                if !(self.is_defined)(name) {
                    return Err(Finding::new(
                        ErrorClass::UndefinedConstant,
                        self.subject,
                        self.toks.get(self.pos - 1).map_or(0, |t| t.line),
                        format!("`{name}` is not defined in any define header"),
                    ));
                }
                self.consts.insert(name.to_owned());
                Ok(Expr::Const(name.to_owned()))
            }
        }
    }

    fn call(&mut self) -> R<Call> {
        let line = self.line();
        let Some(Tok::Ident(name)) = self.bump().map(|t| &t.tok) else {
            return self.syntax("a helper name is missing");
        };
        if self.peek_tok() != Some(&Tok::LParen) {
            return Err(self.err(
                ErrorClass::UnsupportedToken,
                format!("`{name}` is not followed by `(`: only helper calls are statements"),
            ));
        }
        if is_reserved(name) {
            return Err(Finding::new(
                ErrorClass::UnsupportedToken,
                self.subject,
                line,
                format!("the C++ keyword `{name}` is not part of the grammar"),
            ));
        }
        let Some(helper) = helpers::lookup(name).filter(|h| h.dll) else {
            let why = if helpers::lookup(name).is_some() {
                "exists only in the Functions table (no CNpcScript method)"
            } else {
                "is not in the helper table"
            };
            return Err(Finding::new(
                ErrorClass::UnknownHelper,
                self.subject,
                line,
                format!("`{name}` {why}"),
            ));
        };
        self.pos += 1; // (
        let mut args = Vec::new();
        if self.peek_tok() == Some(&Tok::RParen) {
            self.pos += 1;
        } else {
            loop {
                self.enter()?;
                let arg = self.term();
                self.depth -= 1;
                args.push(to_num(arg?));
                match self.peek_tok() {
                    Some(Tok::Comma) => self.pos += 1,
                    Some(Tok::RParen) => {
                        self.pos += 1;
                        break;
                    }
                    _ => {
                        return self.syntax(format!(
                            "expected `,` or `)` in the arguments of {name}, found {}",
                            self.describe()
                        ));
                    }
                }
            }
        }
        if !helper.accepts(args.len()) {
            return Err(Finding::new(
                ErrorClass::BadArity,
                self.subject,
                line,
                format!(
                    "`{name}` takes {} argument(s), called with {}",
                    arity_text(helper),
                    args.len()
                ),
            ));
        }
        for (i, a) in args.iter().enumerate() {
            if helper.kind_of(i) == ArgKind::TextId
                && let Expr::Int(v) = a
                && *v < 0
            {
                return Err(Finding::new(
                    ErrorClass::Type,
                    self.subject,
                    line,
                    format!(
                        "argument {} of `{name}` is a text id and cannot be negative",
                        i + 1
                    ),
                ));
            }
        }
        Ok(Call { helper, args, line })
    }
}

fn arity_text(h: &helpers::HelperSig) -> String {
    let list: Vec<String> = h.arities.iter().map(u8::to_string).collect();
    if h.variadic {
        format!("{} or more", list.join("/"))
    } else {
        list.join(" or ")
    }
}

/// Int to boolean: `(x ~= 0)`.
fn to_bool(e: Expr) -> Expr {
    if e.is_bool() {
        e
    } else {
        Expr::Rel(RelOp::Ne, Box::new(e), Box::new(Expr::Int(0)))
    }
}

/// Boolean to int: `(b and 1 or 0)`.
fn to_num(e: Expr) -> Expr {
    if e.is_bool() {
        Expr::Num(Box::new(e))
    } else {
        e
    }
}
