//! Cross-check of the helper table against the original headers (Plan/20 §5.1: `NpcScript.h`,
//! `FunctionsInfo.h`): the 91 `CNpcScript` methods with their argument counts, and the 9 table-only names of
//! the `Functions` struct.

use crate::error::{ErrorClass, Finding};
use crate::helpers::HELPERS;
use crate::source::{split_suffix, strip_comments};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};

/// Members of `CNpcScript` that are not helpers.
const INFRASTRUCTURE: [&str; 3] = ["SetLang", "DoExec", "Find"];

/// A method declared in the class.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Decl {
    returns_value: bool,
    arities: BTreeSet<usize>,
    variadic: bool,
}

/// Compares the helper table with `NpcScript.h` and `FunctionsInfo.h`; returns the errors and informational
/// notes (return-type differences, which the plan decided on purpose).
pub fn check_helpers(
    npc_script_h: Option<&str>,
    functions_info_h: Option<&str>,
) -> (Vec<Finding>, Vec<String>) {
    let mut errors = Vec::new();
    let mut notes = Vec::new();
    if let Some(text) = npc_script_h {
        check_class(text, &mut errors, &mut notes);
    } else {
        notes.push(
            "NpcScript.h was not available: the helper table was not cross-checked".to_owned(),
        );
    }
    if let Some(text) = functions_info_h {
        for h in HELPERS.iter().filter(|h| !h.dll) {
            let needle = format!("PF{} {};", h.name, h.name);
            if !text.contains(&needle) {
                errors.push(Finding::new(
                    ErrorClass::HeaderMismatch,
                    h.name,
                    0,
                    "a table-only name is not a member of the Functions struct of FunctionsInfo.h",
                ));
            }
        }
    } else {
        notes.push(
            "FunctionsInfo.h was not available: the table-only names were not cross-checked"
                .to_owned(),
        );
    }
    (errors, notes)
}

/// The text between the braces of `class CNpcScript { ... };` (the whole text when there is no such class).
fn class_body(text: &str) -> &str {
    let Some(start) = text.find("class CNpcScript") else {
        return text;
    };
    let rest = text.get(start..).unwrap_or("");
    let Some(open) = rest.find('{') else {
        return rest;
    };
    let mut depth = 0usize;
    for (i, b) in rest.bytes().enumerate().skip(open) {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return rest.get(open + 1..i).unwrap_or(rest);
                }
            }
            _ => {}
        }
    }
    rest.get(open + 1..).unwrap_or(rest)
}

fn check_class(text: &str, errors: &mut Vec<Finding>, notes: &mut Vec<String>) {
    let stripped = strip_comments(text);
    let class = class_body(&stripped);
    let Ok(method) = Regex::new(r"(?m)^\s*(void|int)\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(([^)]*)\)\s*;")
    else {
        return;
    };
    let mut decls: BTreeMap<String, Decl> = BTreeMap::new();
    for caps in method.captures_iter(class) {
        let (Some(ret), Some(name), Some(params)) = (caps.get(1), caps.get(2), caps.get(3)) else {
            continue;
        };
        let name = name.as_str();
        if INFRASTRUCTURE.contains(&name) || split_suffix(name).is_some() {
            continue; // not a helper: infrastructure or a script function `npc_key()`
        }
        let params = params.as_str().trim();
        let variadic = params.contains("...");
        let fixed: Vec<&str> = params
            .split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty() && *p != "..." && *p != "void")
            .collect();
        let d = decls.entry(name.to_owned()).or_insert_with(|| Decl {
            returns_value: ret.as_str() == "int",
            arities: BTreeSet::new(),
            variadic: false,
        });
        d.arities.insert(fixed.len());
        d.variadic |= variadic;
    }
    for h in HELPERS.iter().filter(|h| h.dll) {
        let Some(d) = decls.get(h.name) else {
            errors.push(Finding::new(
                ErrorClass::HeaderMismatch,
                h.name,
                0,
                "the helper table has a method that NpcScript.h does not declare",
            ));
            continue;
        };
        let table: BTreeSet<usize> = h.arities.iter().map(|a| usize::from(*a)).collect();
        if table != d.arities || h.variadic != d.variadic {
            errors.push(Finding::new(
                ErrorClass::HeaderMismatch,
                h.name,
                0,
                format!(
                    "argument counts differ: table {:?}{}, NpcScript.h {:?}{}",
                    table,
                    if h.variadic { " or more" } else { "" },
                    d.arities,
                    if d.variadic { " or more" } else { "" }
                ),
            ));
        }
        if h.returns_value != d.returns_value {
            notes.push(format!(
                "{}: NpcScript.h declares `{}`, the helper table says it {} a value (Plan/20 §6)",
                h.name,
                if d.returns_value { "int" } else { "void" },
                if h.returns_value {
                    "returns"
                } else {
                    "returns no"
                }
            ));
        }
    }
    for name in decls.keys() {
        if !HELPERS.iter().any(|h| h.dll && h.name == name) {
            errors.push(Finding::new(
                ErrorClass::HeaderMismatch,
                name,
                0,
                "NpcScript.h declares a method that is not in the helper table",
            ));
        }
    }
}
