//! The front end result (Plan/20 §5.3): every function parsed, registrations resolved, reachability decided,
//! functions grouped by script, body statistics and shapes collected.
//!
//! Reachability (D20-05, KEEP): the server looks a function up as `sprintf("%s_%d", lower(npcName), key)` in
//! a map built from the `g_tables` names, so a registration is dispatched only when its name is entirely
//! lower-case and its key suffix is the canonical decimal form of the number. The function behind it is
//! the registered *member*, whatever its own name is (`mafl_Christmas_Romeo_10` -> `mafl_christmas_romeo_10`).

use crate::ast::{self, Stmt};
use crate::error::{ErrorClass, Finding};
use crate::inputs::Inputs;
use crate::lex::{Tok, Token, lex};
use crate::parse::{Parsed, is_upper_const, parse_body};
use crate::source::{
    CppFunction, DIALOG_KEY_LINES, Registration, extract_functions, extract_key_table,
    extract_registrations, split_suffix, strip_comments,
};
use std::collections::{BTreeMap, BTreeSet};

/// Why a function is, or is not, called by the original server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reach {
    /// Dispatched under `reg_name`.
    Reachable {
        /// The `g_tables` name.
        reg_name: String,
    },
    /// No `g_tables` entry points at the function.
    Unregistered,
    /// Registered, but only under names with capital letters.
    OnlyCapitalised(Vec<String>),
    /// Registered under lower-case names whose key suffix is not the canonical decimal form.
    NonCanonical(Vec<String>),
}

impl Reach {
    /// `true` when the original dispatched the function.
    pub fn is_reachable(&self) -> bool {
        matches!(self, Reach::Reachable { .. })
    }

    /// The reason text of the report and of the emitted `UNREACHABLE` block.
    pub fn reason(&self) -> String {
        match self {
            Reach::Reachable { reg_name } => format!("registered as `{reg_name}`"),
            Reach::Unregistered => "not registered in g_tables".to_owned(),
            Reach::OnlyCapitalised(names) => format!(
                "registered only as {} (capital letters: the dispatch name is lower-case)",
                quote_list(names)
            ),
            Reach::NonCanonical(names) => format!(
                "registered only as {} (the key suffix is not the canonical decimal form)",
                quote_list(names)
            ),
        }
    }

    /// The short class name of the report.
    pub fn class(&self) -> &'static str {
        match self {
            Reach::Reachable { .. } => "reachable",
            Reach::Unregistered => "unregistered",
            Reach::OnlyCapitalised(_) => "capitalised",
            Reach::NonCanonical(_) => "non_canonical",
        }
    }
}

fn quote_list(names: &[String]) -> String {
    names
        .iter()
        .map(|n| format!("`{n}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Token-level statistics of one body (the counts of Plan/20 §5.2).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BodyStats {
    /// `if` keywords.
    pub ifs: u32,
    /// `else` keywords.
    pub elses: u32,
    /// `switch` keywords.
    pub switches: u32,
    /// `case` keywords.
    pub cases: u32,
    /// `break` keywords.
    pub breaks: u32,
    /// Float literals.
    pub floats: u32,
    /// Deepest `{` nesting, counting the braces of the function itself.
    pub max_depth: u32,
    /// Operator counts (`==`, `&&`, ...).
    pub ops: BTreeMap<&'static str, u32>,
    /// Helper call counts by name (an identifier followed by `(`, without `if`/`switch`).
    pub calls: BTreeMap<String, u32>,
    /// Distinct `UPPER_CASE` constants used (including `TRUE`/`FALSE`).
    pub constants: BTreeSet<String>,
    /// The body has no token.
    pub empty: bool,
}

/// One C++ function with everything derived from it.
#[derive(Clone, Debug)]
pub struct Func {
    /// The source function.
    pub cpp: CppFunction,
    /// The lower-case script name (file stem): from the registration for a reachable function, else from the
    /// member name.
    pub script: String,
    /// The key line.
    pub key: u32,
    /// Reachability.
    pub reach: Reach,
    /// The parsed body; `None` when it did not parse (a finding was recorded).
    pub parsed: Option<Parsed>,
    /// The normalised body shape (numbers `N`, constants `K`).
    pub shape: Option<String>,
    /// Token statistics.
    pub stats: BodyStats,
}

impl Func {
    /// The statements (empty when the body failed to parse).
    pub fn body(&self) -> &[Stmt] {
        self.parsed.as_ref().map_or(&[], |p| p.body.as_slice())
    }
}

/// The functions of one script.
#[derive(Clone, Debug, Default)]
pub struct ScriptModel {
    /// Lower-case name (file stem).
    pub name: String,
    /// Reachable functions by key line (indices into [`Model::funcs`]).
    pub reachable: BTreeMap<u32, usize>,
    /// Unreachable functions in source order.
    pub unreachable: Vec<usize>,
}

/// The whole front end result.
#[derive(Debug)]
pub struct Model {
    /// All functions in file order.
    pub funcs: Vec<Func>,
    /// All `g_tables` entries in file order.
    pub registrations: Vec<Registration>,
    /// The scripts by lower-case name.
    pub scripts: BTreeMap<String, ScriptModel>,
    /// `void CNpcScript::name()` methods without a numeric suffix.
    pub other_methods: Vec<String>,
    /// `g_nKeys[]` as read from the source.
    pub key_table: Option<Vec<u32>>,
    /// Everything that is an error.
    pub findings: Vec<Finding>,
}

impl Model {
    /// The function behind `key` of `script`, if reachable.
    pub fn reachable_func(&self, script: &str, key: u32) -> Option<&Func> {
        let idx = self.scripts.get(script)?.reachable.get(&key)?;
        self.funcs.get(*idx)
    }

    /// The reachable functions in (script, key) order.
    pub fn reachable_funcs(&self) -> impl Iterator<Item = &Func> + '_ {
        self.scripts
            .values()
            .flat_map(|s| s.reachable.values())
            .filter_map(|i| self.funcs.get(*i))
    }
}

/// `true` when the server could build `reg_name` as `"<lower npc name>_<key>"`.
pub fn dispatch_name(reg_name: &str) -> Option<(&str, u32)> {
    if reg_name.bytes().any(|b| b.is_ascii_uppercase()) {
        return None;
    }
    let (script, digits) = split_suffix(reg_name)?;
    let canonical = digits == "0" || !digits.starts_with('0');
    if !canonical {
        return None;
    }
    Some((script, digits.parse().ok()?))
}

/// Builds the model from the inputs (never fails: problems are `findings`).
pub fn build(inputs: &Inputs) -> Model {
    let mut findings = Vec::new();
    let stripped = strip_comments(&inputs.npc_script_cpp.text);
    let fns = extract_functions(&stripped);
    findings.extend(fns.findings);
    let (registrations, reg_findings) = extract_registrations(&inputs.helper_cpp.text);
    findings.extend(reg_findings);

    // key table of the source against the constant
    let key_table = extract_key_table(&inputs.npc_script_cpp.text);
    match &key_table {
        Some(t) if t.as_slice() != DIALOG_KEY_LINES.as_slice() => findings.push(Finding::new(
            ErrorClass::KeyTable,
            "g_nKeys",
            0,
            "g_nKeys[] of NpcScript.cpp differs from DIALOG_KEY_LINES",
        )),
        _ => {}
    }

    // registrations: unique names, defined members
    let mut seen = BTreeSet::new();
    for r in &registrations {
        if !seen.insert(r.reg_name.as_str()) {
            findings.push(Finding::new(
                ErrorClass::Registration,
                &r.reg_name,
                r.line,
                "the name is registered twice in g_tables",
            ));
        }
        if !fns.list.iter().any(|f| f.name == r.member) {
            findings.push(Finding::new(
                ErrorClass::Registration,
                &r.reg_name,
                r.line,
                format!("g_tables points at `{}`, which is not defined", r.member),
            ));
        }
    }

    let is_defined = |n: &str| inputs.defines.contains_key(n);
    let mut funcs = Vec::with_capacity(fns.list.len());
    for cpp in fns.list {
        let regs: Vec<&Registration> = registrations
            .iter()
            .filter(|r| r.member == cpp.name)
            .collect();
        let dispatched = regs.iter().find_map(|r| {
            dispatch_name(&r.reg_name).map(|(s, k)| (r.reg_name.clone(), s.to_owned(), k))
        });
        let (reach, script, key) = match dispatched {
            Some((reg_name, script, key)) => (Reach::Reachable { reg_name }, script, key),
            None => {
                let names: Vec<String> = regs.iter().map(|r| r.reg_name.clone()).collect();
                let reach = if names.is_empty() {
                    Reach::Unregistered
                } else if names
                    .iter()
                    .all(|n| n.bytes().any(|b| b.is_ascii_uppercase()))
                {
                    Reach::OnlyCapitalised(names)
                } else {
                    Reach::NonCanonical(names)
                };
                (reach, cpp.script.to_ascii_lowercase(), cpp.key)
            }
        };
        if reach.is_reachable() && !DIALOG_KEY_LINES.contains(&key) {
            findings.push(Finding::new(
                ErrorClass::KeyTable,
                &cpp.name,
                cpp.head_line,
                format!("key {key} is not one of DIALOG_KEY_LINES"),
            ));
        }
        let (parsed, shape, stats) = analyse_body(&cpp, &is_defined, &mut findings);
        funcs.push(Func {
            cpp,
            script,
            key,
            reach,
            parsed,
            shape,
            stats,
        });
    }

    let mut scripts: BTreeMap<String, ScriptModel> = BTreeMap::new();
    for (i, f) in funcs.iter().enumerate() {
        let s = scripts
            .entry(f.script.clone())
            .or_insert_with(|| ScriptModel {
                name: f.script.clone(),
                ..ScriptModel::default()
            });
        if f.reach.is_reachable() {
            s.reachable.insert(f.key, i);
        } else {
            s.unreachable.push(i);
        }
    }
    Model {
        funcs,
        registrations,
        scripts,
        other_methods: fns.other_methods,
        key_table,
        findings,
    }
}

fn analyse_body(
    cpp: &CppFunction,
    is_defined: &dyn Fn(&str) -> bool,
    findings: &mut Vec<Finding>,
) -> (Option<Parsed>, Option<String>, BodyStats) {
    let tokens = match lex(&cpp.name, &cpp.body, cpp.body_line) {
        Ok(t) => t,
        Err(f) => {
            findings.push(f);
            return (None, None, BodyStats::default());
        }
    };
    let stats = stats_of(&tokens);
    let shape = shape_of(&tokens);
    match parse_body(&cpp.name, &tokens, is_defined) {
        Ok(p) => (Some(p), Some(shape), stats),
        Err(f) => {
            findings.push(f);
            (None, Some(shape), stats)
        }
    }
}

fn stats_of(tokens: &[Token]) -> BodyStats {
    let mut s = BodyStats {
        empty: tokens.is_empty(),
        ..BodyStats::default()
    };
    let mut depth = 1u32; // the function's own brace
    s.max_depth = 1;
    for (i, t) in tokens.iter().enumerate() {
        let next_is_paren = tokens.get(i + 1).map(|n| &n.tok) == Some(&Tok::LParen);
        match &t.tok {
            Tok::LBrace => {
                depth += 1;
                s.max_depth = s.max_depth.max(depth);
            }
            Tok::RBrace => depth = depth.saturating_sub(1),
            Tok::Float(..) => s.floats += 1,
            Tok::EqEq => *s.ops.entry("==").or_default() += 1,
            Tok::NotEq => *s.ops.entry("!=").or_default() += 1,
            Tok::Ge => *s.ops.entry(">=").or_default() += 1,
            Tok::Le => *s.ops.entry("<=").or_default() += 1,
            Tok::Lt => *s.ops.entry("<").or_default() += 1,
            Tok::Gt => *s.ops.entry(">").or_default() += 1,
            Tok::AndAnd => *s.ops.entry("&&").or_default() += 1,
            Tok::OrOr => *s.ops.entry("||").or_default() += 1,
            Tok::Bang => *s.ops.entry("!").or_default() += 1,
            Tok::Ident(w) => match w.as_str() {
                "if" => s.ifs += 1,
                "else" => s.elses += 1,
                "switch" => s.switches += 1,
                "case" => s.cases += 1,
                "break" => s.breaks += 1,
                _ if next_is_paren => *s.calls.entry(w.clone()).or_default() += 1,
                _ if is_upper_const(w) => {
                    s.constants.insert(w.clone());
                }
                _ => {}
            },
            _ => {}
        }
    }
    s
}

/// The normalised shape of a body: numbers become `N`, constants `K`, a unary minus before a number
/// disappears, words are separated by one space.
pub fn shape_of(tokens: &[Token]) -> String {
    let mut out = String::new();
    let mut prev_word = false;
    for (i, t) in tokens.iter().enumerate() {
        let prev = i.checked_sub(1).and_then(|p| tokens.get(p)).map(|p| &p.tok);
        let next = tokens.get(i + 1).map(|n| &n.tok);
        let piece: String = match &t.tok {
            Tok::Int(_) | Tok::Float(..) => "N".to_owned(),
            Tok::Minus => {
                let sign = matches!(next, Some(Tok::Int(_) | Tok::Float(..)))
                    && !matches!(
                        prev,
                        Some(Tok::Int(_) | Tok::Float(..) | Tok::RParen | Tok::Ident(_))
                    );
                if sign {
                    continue;
                }
                "-".to_owned()
            }
            Tok::Ident(w) => {
                if is_upper_const(w) && next != Some(&Tok::LParen) {
                    "K".to_owned()
                } else {
                    w.clone()
                }
            }
            other => other.text(),
        };
        let word = piece
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric());
        if word && prev_word {
            out.push(' ');
        }
        out.push_str(&piece);
        prev_word = word;
    }
    out
}

/// All literal text ids (`Say(46)` -> 46) with the calls they belong to, over the statements of a body.
pub fn text_refs(body: &[Stmt]) -> Vec<(&'static str, i64)> {
    let mut out = Vec::new();
    ast::walk_calls(body, &mut |c| {
        for i in c.helper.text_args(c.args.len()) {
            if let Some(ast::Expr::Int(v)) = c.args.get(i) {
                out.push((c.helper.name, *v));
            }
        }
    });
    out
}
