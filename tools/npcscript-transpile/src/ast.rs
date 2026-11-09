//! The typed AST of a function body (Plan/20 §5.3 steps 6-7), its text dump (the snapshot format of the
//! fixture tests) and the reference interpreter [`eval`] (step 9) that the dual-execution test compares with
//! the emitted Lua.
//!
//! The AST is already type-checked in C `int` semantics: conditions are boolean nodes, every operand of a
//! relation is a number node, and the coercions the plan asks for (`(x ~= 0)` for a bare int, `(x) == 0` for
//! `!int`, `(b and 1 or 0)` for a boolean used as a number) are explicit nodes. `TRUE`/`FALSE` are the
//! integers 1/0, a unary minus on a literal is folded into the literal, braces are flattened.

use crate::helpers::HelperSig;
use std::fmt::{self, Write as _};

/// A relational operator.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RelOp {
    /// `==`
    Eq,
    /// `!=`
    Ne,
    /// `>=`
    Ge,
    /// `<=`
    Le,
    /// `<`
    Lt,
    /// `>`
    Gt,
}

impl RelOp {
    /// The C operator.
    pub fn c(self) -> &'static str {
        match self {
            RelOp::Eq => "==",
            RelOp::Ne => "!=",
            RelOp::Ge => ">=",
            RelOp::Le => "<=",
            RelOp::Lt => "<",
            RelOp::Gt => ">",
        }
    }

    /// The Lua operator.
    pub fn lua(self) -> &'static str {
        match self {
            RelOp::Ne => "~=",
            other => other.c(),
        }
    }
}

/// A helper call.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    /// The helper.
    pub helper: &'static HelperSig,
    /// The arguments (all number-typed).
    pub args: Vec<Expr>,
    /// Line in `NpcScript.cpp`.
    pub line: u32,
}

/// An expression. [`Expr::is_bool`] tells the two types apart.
#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    /// An integer literal (`TRUE`/`FALSE` are 1/0).
    Int(i64),
    /// A float literal: canonical text and value.
    Float(String, f64),
    /// A define constant, `C.NAME` in the Lua.
    Const(String),
    /// A helper call used as a value.
    Call(Call),
    /// A boolean used as a number: `(b and 1 or 0)`.
    Num(Box<Expr>),
    /// `!b` of a boolean.
    Not(Box<Expr>),
    /// `a && b`, short-circuit.
    And(Box<Expr>, Box<Expr>),
    /// `a || b`, short-circuit.
    Or(Box<Expr>, Box<Expr>),
    /// A relation of two numbers.
    Rel(RelOp, Box<Expr>, Box<Expr>),
}

impl Expr {
    /// `true` for the boolean-typed nodes.
    pub fn is_bool(&self) -> bool {
        matches!(
            self,
            Expr::Not(_) | Expr::And(..) | Expr::Or(..) | Expr::Rel(..)
        )
    }
}

/// A `case` of a `switch(Random(n))`: one or more grouped labels and the statements up to `break`.
#[derive(Clone, Debug, PartialEq)]
pub struct Case {
    /// The labels (`case 0: case 1:` is one case with two labels).
    pub labels: Vec<i64>,
    /// The statements.
    pub body: Vec<Stmt>,
}

/// A statement.
#[derive(Clone, Debug, PartialEq)]
pub enum Stmt {
    /// `Helper(args);`
    Call(Call),
    /// `if (cond) then else els` (`else if` is an `els` holding one `If`).
    If {
        /// The boolean condition.
        cond: Expr,
        /// The `then` statements.
        then: Vec<Stmt>,
        /// The `else` statements (empty without `else`).
        els: Vec<Stmt>,
        /// Line of the `if`.
        line: u32,
    },
    /// `switch (Random(n)) { cases }`.
    Switch {
        /// The `n` of `Random(n)`.
        random_arg: i64,
        /// The cases in source order.
        cases: Vec<Case>,
        /// Line of the `switch`.
        line: u32,
    },
}

// ------------------------------------------------------------------------------------------------
// traversal
// ------------------------------------------------------------------------------------------------

/// Calls `f` for every helper call of `stmts` in execution order of the source text (statement calls, calls in
/// conditions and calls nested in arguments, outer call before its arguments).
pub fn walk_calls(stmts: &[Stmt], f: &mut dyn FnMut(&Call)) {
    for s in stmts {
        match s {
            Stmt::Call(c) => walk_call(c, f),
            Stmt::If {
                cond, then, els, ..
            } => {
                walk_expr(cond, f);
                walk_calls(then, f);
                walk_calls(els, f);
            }
            Stmt::Switch { cases, .. } => {
                for c in cases {
                    walk_calls(&c.body, f);
                }
            }
        }
    }
}

fn walk_call(c: &Call, f: &mut dyn FnMut(&Call)) {
    f(c);
    for a in &c.args {
        walk_expr(a, f);
    }
}

fn walk_expr(e: &Expr, f: &mut dyn FnMut(&Call)) {
    match e {
        Expr::Call(c) => walk_call(c, f),
        Expr::Num(a) | Expr::Not(a) => walk_expr(a, f),
        Expr::And(a, b) | Expr::Or(a, b) | Expr::Rel(_, a, b) => {
            walk_expr(a, f);
            walk_expr(b, f);
        }
        Expr::Int(_) | Expr::Float(..) | Expr::Const(_) => {}
    }
}

// ------------------------------------------------------------------------------------------------
// text dump (the AST snapshot format)
// ------------------------------------------------------------------------------------------------

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expr::Int(v) => write!(f, "{v}"),
            Expr::Float(t, _) => write!(f, "{t}f"),
            Expr::Const(n) => write!(f, "{n}"),
            Expr::Call(c) => write!(f, "{c}"),
            Expr::Num(e) => write!(f, "(num {e})"),
            Expr::Not(e) => write!(f, "(not {e})"),
            Expr::And(a, b) => write!(f, "(and {a} {b})"),
            Expr::Or(a, b) => write!(f, "(or {a} {b})"),
            Expr::Rel(op, a, b) => write!(f, "({} {a} {b})", op.c()),
        }
    }
}

impl fmt::Display for Call {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}(", self.helper.name)?;
        for (i, a) in self.args.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{a}")?;
        }
        f.write_str(")")
    }
}

/// Appends the indented dump of `stmts` to `out`.
pub fn dump_stmts(stmts: &[Stmt], indent: usize, out: &mut String) {
    let pad = "  ".repeat(indent);
    for s in stmts {
        match s {
            Stmt::Call(c) => {
                let _ = writeln!(out, "{pad}{c}");
            }
            Stmt::If {
                cond, then, els, ..
            } => {
                let _ = writeln!(out, "{pad}if {cond}");
                dump_stmts(then, indent + 1, out);
                if !els.is_empty() {
                    let _ = writeln!(out, "{pad}else");
                    dump_stmts(els, indent + 1, out);
                }
            }
            Stmt::Switch {
                random_arg, cases, ..
            } => {
                let _ = writeln!(out, "{pad}switch Random({random_arg})");
                for c in cases {
                    let labels: Vec<String> = c.labels.iter().map(i64::to_string).collect();
                    let _ = writeln!(out, "{pad}  case {}", labels.join(","));
                    dump_stmts(&c.body, indent + 2, out);
                }
            }
        }
    }
}

/// The dump of a whole body (`(empty)` for no statements).
pub fn dump(stmts: &[Stmt]) -> String {
    if stmts.is_empty() {
        return "(empty)\n".to_owned();
    }
    let mut out = String::new();
    dump_stmts(stmts, 0, &mut out);
    out
}

// ------------------------------------------------------------------------------------------------
// reference interpreter
// ------------------------------------------------------------------------------------------------

/// A value of the interpreter: C `int` (as `i64`) or `float` (as `f64`), like a Lua number.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Value {
    /// An integer.
    Int(i64),
    /// A float.
    Float(f64),
}

impl Value {
    /// The value as `f64`.
    pub fn as_f64(self) -> f64 {
        match self {
            Value::Int(v) => v as f64,
            Value::Float(v) => v,
        }
    }
}

/// What the interpreter needs from its environment (the mock of the dual-execution test, or a real host).
pub trait ScriptHost {
    /// Executes one helper; void helpers return `Value::Int(0)`.
    fn call(&mut self, helper: &'static HelperSig, args: &[Value]) -> Result<Value, String>;
    /// The value of the define `name` (`C.NAME`).
    fn constant(&mut self, name: &str) -> Result<i64, String>;
    /// Called at every branch point with the line of the `if`/`switch` and the outcome taken (`if`: 1 for the
    /// `then` part, 0 otherwise; `switch`: the 1-based index of the matching case, 0 for none). Lets a test
    /// measure branch coverage; the default does nothing.
    fn branch(&mut self, _line: u32, _outcome: usize) {}
}

/// Executes `body` against `host` with the semantics of the C++ original (left-to-right arguments, short
/// circuit `&&`/`||`, first matching `case`).
pub fn eval(body: &[Stmt], host: &mut dyn ScriptHost) -> Result<(), String> {
    for s in body {
        match s {
            Stmt::Call(c) => {
                eval_call(c, host)?;
            }
            Stmt::If {
                cond,
                then,
                els,
                line,
            } => {
                if eval_bool(cond, host)? {
                    host.branch(*line, 1);
                    eval(then, host)?;
                } else {
                    host.branch(*line, 0);
                    eval(els, host)?;
                }
            }
            Stmt::Switch {
                random_arg,
                cases,
                line,
            } => {
                let random = crate::helpers::lookup("Random")
                    .ok_or_else(|| "Random is not in the helper table".to_owned())?;
                let v = match host.call(random, &[Value::Int(*random_arg)])? {
                    Value::Int(v) => v,
                    Value::Float(f) => f as i64,
                };
                match cases.iter().position(|c| c.labels.contains(&v)) {
                    Some(i) => {
                        host.branch(*line, i + 1);
                        if let Some(case) = cases.get(i) {
                            eval(&case.body, host)?;
                        }
                    }
                    None => host.branch(*line, 0),
                }
            }
        }
    }
    Ok(())
}

fn eval_call(c: &Call, host: &mut dyn ScriptHost) -> Result<Value, String> {
    let mut args = Vec::with_capacity(c.args.len());
    for a in &c.args {
        args.push(eval_num(a, host)?);
    }
    host.call(c.helper, &args)
}

fn eval_num(e: &Expr, host: &mut dyn ScriptHost) -> Result<Value, String> {
    match e {
        Expr::Int(v) => Ok(Value::Int(*v)),
        Expr::Float(_, v) => Ok(Value::Float(*v)),
        Expr::Const(n) => Ok(Value::Int(host.constant(n)?)),
        Expr::Call(c) => eval_call(c, host),
        Expr::Num(b) => Ok(Value::Int(i64::from(eval_bool(b, host)?))),
        Expr::Not(_) | Expr::And(..) | Expr::Or(..) | Expr::Rel(..) => {
            Err("a boolean node where a number is required".to_owned())
        }
    }
}

fn eval_bool(e: &Expr, host: &mut dyn ScriptHost) -> Result<bool, String> {
    match e {
        Expr::Not(b) => Ok(!eval_bool(b, host)?),
        Expr::And(a, b) => Ok(eval_bool(a, host)? && eval_bool(b, host)?),
        Expr::Or(a, b) => Ok(eval_bool(a, host)? || eval_bool(b, host)?),
        Expr::Rel(op, a, b) => {
            let l = eval_num(a, host)?;
            let r = eval_num(b, host)?;
            Ok(match (l, r) {
                (Value::Int(x), Value::Int(y)) => cmp(*op, x, y),
                (x, y) => cmp(*op, x.as_f64(), y.as_f64()),
            })
        }
        _ => Err("a number node where a condition is required".to_owned()),
    }
}

fn cmp<T: PartialOrd>(op: RelOp, a: T, b: T) -> bool {
    match op {
        RelOp::Eq => a == b,
        RelOp::Ne => a != b,
        RelOp::Ge => a >= b,
        RelOp::Le => a <= b,
        RelOp::Lt => a < b,
        RelOp::Gt => a > b,
    }
}
