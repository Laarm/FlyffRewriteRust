//! The validation report (Plan/20 §5.7): counts against the golden values of §5.2, the shape histogram, the
//! unreachable functions, missing strings, refused keys, NPC/script mismatches and the error classes, as
//! JSON and Markdown.

use crate::ast::Expr;
use crate::dialog::KeyMap;
use crate::error::{ErrorClass, Finding};
use crate::inputs::{Inputs, sha256_hex};
use crate::model::Model;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

/// The measured facts of Plan/20 §5.2.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    /// Distinct script names (files).
    pub scripts: u32,
    /// `CNpcScript::<name>_<n>()` functions.
    pub functions: u32,
    /// `g_tables` entries.
    pub registrations: u32,
    /// Functions no `g_tables` entry points at.
    pub unregistered_functions: u32,
    /// Registrations whose name differs from the member name.
    pub registration_name_differs: u32,
    /// Registrations whose name has capital letters.
    pub capitalised_registrations: u32,
    /// Functions the original dispatches.
    pub reachable: u32,
    /// Functions it never calls.
    pub unreachable: u32,
    /// Bodies without a token.
    pub empty_bodies: u32,
    /// Distinct normalised body shapes.
    pub shapes: u32,
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
    /// Deepest brace nesting including the function's own braces.
    pub max_brace_depth: u32,
    /// Float literals.
    pub float_literals: u32,
    /// Distinct `UPPER_CASE` constants including `TRUE` and `FALSE`.
    pub constants_distinct: u32,
    /// Of those, resolved through the define headers (`TRUE`/`FALSE` are literals).
    pub constants_defined: u32,
    /// References to dialog lines past the end of `WorldDialog.txt`.
    pub missing_string_refs: u32,
    /// `void CNpcScript::name()` methods without a numeric suffix.
    pub other_methods: u32,
    /// Distinct helpers called.
    pub helpers_distinct: u32,
    /// Operator counts.
    pub operators: BTreeMap<String, u32>,
    /// Call counts by helper.
    pub helper_calls: BTreeMap<String, u32>,
    /// Functions per key suffix.
    pub key_suffix_functions: BTreeMap<u32, u32>,
}

/// The golden values of Plan/20 §5.2 that hold for the shipped input.
pub fn golden() -> Counts {
    let calls: [(&str, u32); 43] = [
        ("Say", 889),
        ("AddKey", 624),
        ("Speak", 540),
        ("NpcId", 540),
        ("Exit", 292),
        ("SetScriptTimer", 280),
        ("LaunchQuest", 270),
        ("GetQuestState", 69),
        ("GetPlayerLvl", 37),
        ("PlaySound", 35),
        ("GetPlayerJob", 30),
        ("ChangeJob", 30),
        ("Random", 19),
        ("GetPlayerExpPercent", 16),
        ("SetLevel", 16),
        ("AddGPPoint", 16),
        ("InitStat", 14),
        ("SayQuest", 12),
        ("IsSetQuest", 11),
        ("PrintSystemMessage", 7),
        ("MonHuntStartParty", 7),
        ("GetGuildQuestState", 6),
        ("GetItemNum", 5),
        ("IsWormonServer", 5),
        ("GetEmptyInventoryNum", 4),
        ("CreateItem", 4),
        ("RemoveKey", 4),
        ("AddAnswer", 4),
        ("IsParty", 4),
        ("AddCondKey", 3),
        ("Replace", 3),
        ("IsGuildQuest", 3),
        ("GetPlayerGold", 3),
        ("IsPartyMaster", 3),
        ("GetPartyNum", 3),
        ("IsPartyGuild", 3),
        ("RemoveItem", 3),
        ("IsGuildMaster", 2),
        ("MonHuntStart", 2),
        ("RemoveGold", 2),
        ("CreateGuild", 2),
        ("RemoveAllItem", 1),
        ("IsGuild", 1),
    ];
    let suffixes: [(u32, u32); 35] = [
        (0, 271),
        (1, 271),
        (2, 267),
        (3, 267),
        (4, 267),
        (5, 267),
        (6, 267),
        (7, 111),
        (8, 267),
        (9, 261),
        (10, 271),
        (11, 1),
        (12, 5),
        (13, 1),
        (14, 1),
        (15, 1),
        (16, 1),
        (17, 1),
        (18, 1),
        (19, 1),
        (20, 1),
        (21, 1),
        (22, 1),
        (23, 1),
        (26, 9),
        (36, 4),
        (37, 2),
        (38, 1),
        (39, 1),
        (40, 2),
        (41, 2),
        (42, 9),
        (43, 9),
        (865, 1),
        (866, 1),
    ];
    Counts {
        scripts: 271,
        functions: 2845,
        registrations: 2785,
        unregistered_functions: 60,
        registration_name_differs: 1,
        capitalised_registrations: 21,
        reachable: 2764,
        unreachable: 81,
        empty_bodies: 995,
        shapes: 51,
        ifs: 99,
        elses: 92,
        switches: 19,
        cases: 50,
        breaks: 50,
        max_brace_depth: 6,
        float_literals: 9,
        // §5.2 says 60: that list leaves out FALSE; TRUE and FALSE are literals, not defines
        constants_distinct: 61,
        constants_defined: 59,
        missing_string_refs: 17,
        other_methods: 0,
        helpers_distinct: 43,
        operators: [("==", 168), ("&&", 102), (">=", 42), ("||", 13), ("<", 4)]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
        helper_calls: calls.into_iter().map(|(k, v)| (k.to_owned(), v)).collect(),
        key_suffix_functions: suffixes.into_iter().collect(),
    }
}

/// One row of the shape histogram.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ShapeRow {
    /// The normalised body (`(empty)` for none).
    pub shape: String,
    /// Functions with this shape.
    pub count: u32,
    /// The first three of them.
    pub examples: Vec<String>,
}

/// An unreachable function.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UnreachableRow {
    /// The C++ function.
    pub function: String,
    /// The script (file stem).
    pub script: String,
    /// The key line.
    pub key: u32,
    /// `unregistered`, `capitalised` or `non_canonical`.
    pub class: String,
    /// The reason text.
    pub reason: String,
}

/// A reference to a dialog line that does not exist.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MissingString {
    /// The script.
    pub script: String,
    /// The C++ function.
    pub function: String,
    /// Whether the function is reachable.
    pub reachable: bool,
    /// The helper that takes the id.
    pub helper: String,
    /// The line id.
    pub id: i64,
}

/// Why the run time refuses a key call.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Refusal {
    /// The text is not the text of a key line.
    NotAKey,
    /// The key exists but the script has no function for it.
    NoFunction,
}

/// A key call the original refused at run time (`Error("key error AddKey ...")`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RefusedKey {
    /// The script.
    pub script: String,
    /// The C++ function.
    pub function: String,
    /// The helper.
    pub helper: String,
    /// The key text the run time computes.
    pub key_text: String,
    /// The reason.
    pub reason: Refusal,
    /// Line in `NpcScript.cpp`.
    pub line: u32,
}

/// NPCs bound to a script that does not exist.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NpcWithoutScript {
    /// The script name the NPCs ask for.
    pub dialog_key: String,
    /// The NPC keys.
    pub npcs: Vec<String>,
}

/// The whole report.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// Tool and format versions.
    pub tool: String,
    /// The measured counts.
    pub counts: Counts,
    /// Differences from the golden values (empty for the shipped input).
    pub golden_differences: Vec<String>,
    /// The shape histogram, most frequent first.
    pub shapes: Vec<ShapeRow>,
    /// SHA-256 of the histogram text.
    pub shapes_sha256: String,
    /// The unreachable functions.
    pub unreachable: Vec<UnreachableRow>,
    /// References past the end of `WorldDialog.txt`.
    pub missing_strings: Vec<MissingString>,
    /// Key calls the run time refuses.
    pub refused_keys: Vec<RefusedKey>,
    /// Dialog keys of `character*.inc` without a script.
    pub npc_without_script: Vec<NpcWithoutScript>,
    /// Scripts no NPC refers to.
    pub script_without_npc: Vec<String>,
    /// Notes (header cross-check, skipped sections).
    pub notes: Vec<String>,
    /// Errors by class; every class is present, empty for the shipped input.
    pub errors: BTreeMap<String, Vec<Finding>>,
}

impl Report {
    /// `true` when no error was found.
    pub fn is_clean(&self) -> bool {
        self.errors.values().all(Vec::is_empty)
    }

    /// The error findings in report order.
    pub fn all_errors(&self) -> Vec<&Finding> {
        self.errors.values().flatten().collect()
    }

    /// Pretty JSON.
    pub fn to_json(&self) -> String {
        let mut s =
            serde_json::to_string_pretty(self).unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}"));
        s.push('\n');
        s
    }

    /// Markdown.
    pub fn to_markdown(&self) -> String {
        let mut o = String::new();
        let _ = writeln!(o, "# NPC script transpile report\n\n{}\n", self.tool);
        let c = &self.counts;
        let _ = writeln!(o, "## counts (info)\n\n| fact | value |\n|---|---|");
        for (k, v) in [
            ("scripts (files)", c.scripts),
            ("functions", c.functions),
            ("g_tables entries", c.registrations),
            ("functions without a registration", c.unregistered_functions),
            (
                "registrations whose name differs from the member",
                c.registration_name_differs,
            ),
            (
                "registrations with capital letters",
                c.capitalised_registrations,
            ),
            ("reachable", c.reachable),
            ("unreachable", c.unreachable),
            ("empty bodies", c.empty_bodies),
            ("distinct body shapes", c.shapes),
            ("if", c.ifs),
            ("else", c.elses),
            ("switch", c.switches),
            ("case", c.cases),
            ("break", c.breaks),
            ("max brace depth", c.max_brace_depth),
            ("float literals", c.float_literals),
            ("distinct constants (with TRUE/FALSE)", c.constants_distinct),
            (
                "constants resolved through the headers",
                c.constants_defined,
            ),
            ("references to missing dialog lines", c.missing_string_refs),
            ("distinct helpers called", c.helpers_distinct),
        ] {
            let _ = writeln!(o, "| {k} | {v} |");
        }
        let ops: Vec<String> = c
            .operators
            .iter()
            .map(|(k, v)| format!("`{k}` {v}"))
            .collect();
        let _ = writeln!(o, "| operators | {} |", ops.join(", "));
        let calls: Vec<String> = {
            let mut v: Vec<(&String, &u32)> = c.helper_calls.iter().collect();
            v.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
            v.into_iter().map(|(k, n)| format!("`{k}` {n}")).collect()
        };
        let _ = writeln!(o, "| helper calls | {} |", calls.join(", "));
        if self.golden_differences.is_empty() {
            let _ = writeln!(o, "\nAll counts equal the golden values of Plan/20 §5.2.");
        } else {
            let _ = writeln!(o, "\nDifferences from the golden values of Plan/20 §5.2:\n");
            for d in self.golden_differences.iter().take(12) {
                let _ = writeln!(o, "* {d}");
            }
            if self.golden_differences.len() > 12 {
                let _ = writeln!(
                    o,
                    "* ... and {} more (report.json has all)",
                    self.golden_differences.len() - 12
                );
            }
        }
        let _ = writeln!(
            o,
            "\n## shapes (info): {} distinct, sha256 {}\n",
            self.shapes.len(),
            self.shapes_sha256
        );
        let _ = writeln!(o, "| count | shape | examples |\n|---|---|---|");
        for r in &self.shapes {
            let _ = writeln!(
                o,
                "| {} | `{}` | {} |",
                r.count,
                r.shape.replace('|', "\\|"),
                r.examples.join(", ")
            );
        }
        let _ = writeln!(o, "\n## unreachable (info): {}\n", self.unreachable.len());
        let _ = writeln!(o, "| function | key | reason |\n|---|---|---|");
        for r in &self.unreachable {
            let _ = writeln!(o, "| `{}` | {} | {} |", r.function, r.key, r.reason);
        }
        let _ = writeln!(
            o,
            "\n## missing_strings (warning): {}\n",
            self.missing_strings.len()
        );
        for r in &self.missing_strings {
            let _ = writeln!(
                o,
                "* `{}` ({}): {}({}) {}",
                r.function,
                r.script,
                r.helper,
                r.id,
                if r.reachable {
                    ""
                } else {
                    "(unreachable function)"
                }
            );
        }
        let _ = writeln!(
            o,
            "\n## refused_keys (warning): {}\n",
            self.refused_keys.len()
        );
        for r in &self.refused_keys {
            let _ = writeln!(
                o,
                "* `{}` line {}: {} with key `{}`: {}",
                r.function,
                r.line,
                r.helper,
                r.key_text,
                match r.reason {
                    Refusal::NotAKey => "the text is not a dialog key",
                    Refusal::NoFunction => "the script has no function for that key",
                }
            );
        }
        let _ = writeln!(
            o,
            "\n## npc_without_script (info): {}\n",
            self.npc_without_script.len()
        );
        for r in &self.npc_without_script {
            let _ = writeln!(o, "* `{}`: {}", r.dialog_key, r.npcs.join(", "));
        }
        let _ = writeln!(
            o,
            "\n## script_without_npc (info): {}\n",
            self.script_without_npc.len()
        );
        for r in &self.script_without_npc {
            let _ = writeln!(o, "* `{r}`");
        }
        if !self.notes.is_empty() {
            let _ = writeln!(o, "\n## notes\n");
            for n in &self.notes {
                let _ = writeln!(o, "* {n}");
            }
        }
        let _ = writeln!(o, "\n## errors\n");
        for (class, list) in &self.errors {
            let _ = writeln!(o, "* {class}: {}", list.len());
            for f in list {
                let _ = writeln!(o, "    * {f}");
            }
        }
        o
    }
}

/// Builds the report from the model.
pub fn build_report(
    inputs: &Inputs,
    model: &Model,
    header_errors: Vec<Finding>,
    mut notes: Vec<String>,
) -> Report {
    let keys = KeyMap::new(&inputs.dialog_lines);
    let mut errors: BTreeMap<String, Vec<Finding>> = ErrorClass::ALL
        .iter()
        .map(|c| (c.name().to_owned(), Vec::new()))
        .collect();
    for f in model.findings.iter().cloned().chain(header_errors) {
        errors.entry(f.class.name().to_owned()).or_default().push(f);
    }
    for line in &keys.empty_lines {
        errors
            .entry(ErrorClass::DialogKey.name().to_owned())
            .or_default()
            .push(Finding::new(
                ErrorClass::DialogKey,
                "WorldDialog.txt",
                *line + 1,
                format!("key line {line} is empty or missing: the original refused to start"),
            ));
    }
    if let Some(n) = &inputs.npcs_note {
        notes.push(n.clone());
    }

    // counts
    let mut counts = Counts {
        scripts: u32::try_from(model.scripts.len()).unwrap_or(u32::MAX),
        functions: u32::try_from(model.funcs.len()).unwrap_or(u32::MAX),
        registrations: u32::try_from(model.registrations.len()).unwrap_or(u32::MAX),
        other_methods: u32::try_from(model.other_methods.len()).unwrap_or(u32::MAX),
        ..Counts::default()
    };
    let targeted: BTreeSet<&str> = model
        .registrations
        .iter()
        .map(|r| r.member.as_str())
        .collect();
    counts.unregistered_functions = count(
        model
            .funcs
            .iter()
            .filter(|f| !targeted.contains(f.cpp.name.as_str()))
            .count(),
    );
    counts.registration_name_differs = count(
        model
            .registrations
            .iter()
            .filter(|r| r.reg_name != r.member)
            .count(),
    );
    counts.capitalised_registrations = count(
        model
            .registrations
            .iter()
            .filter(|r| r.reg_name.bytes().any(|b| b.is_ascii_uppercase()))
            .count(),
    );
    counts.reachable = count(
        model
            .funcs
            .iter()
            .filter(|f| f.reach.is_reachable())
            .count(),
    );
    counts.unreachable = counts.functions - counts.reachable;
    let mut constants: BTreeSet<&str> = BTreeSet::new();
    let mut shapes: BTreeMap<String, (u32, Vec<String>)> = BTreeMap::new();
    for f in &model.funcs {
        let s = &f.stats;
        counts.empty_bodies += u32::from(s.empty);
        counts.ifs += s.ifs;
        counts.elses += s.elses;
        counts.switches += s.switches;
        counts.cases += s.cases;
        counts.breaks += s.breaks;
        counts.float_literals += s.floats;
        counts.max_brace_depth = counts.max_brace_depth.max(s.max_depth);
        for (op, n) in &s.ops {
            *counts.operators.entry((*op).to_owned()).or_default() += n;
        }
        for (name, n) in &s.calls {
            *counts.helper_calls.entry(name.clone()).or_default() += n;
        }
        constants.extend(s.constants.iter().map(String::as_str));
        *counts.key_suffix_functions.entry(f.cpp.key).or_default() += 1;
        if let Some(shape) = &f.shape {
            let shape = if shape.is_empty() {
                "(empty)".to_owned()
            } else {
                shape.clone()
            };
            let e = shapes.entry(shape).or_default();
            e.0 += 1;
            if e.1.len() < 3 {
                e.1.push(f.cpp.name.clone());
            }
        }
    }
    counts.constants_distinct = count(constants.len());
    counts.constants_defined = count(
        constants
            .iter()
            .filter(|c| **c != "TRUE" && **c != "FALSE" && inputs.defines.contains_key(**c))
            .count(),
    );
    counts.helpers_distinct = count(counts.helper_calls.len());
    counts.shapes = count(shapes.len());

    let mut shape_rows: Vec<ShapeRow> = shapes
        .into_iter()
        .map(|(shape, (count, examples))| ShapeRow {
            shape,
            count,
            examples,
        })
        .collect();
    shape_rows.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.shape.cmp(&b.shape)));
    let histogram: String = shape_rows
        .iter()
        .map(|r| format!("{}\t{}\n", r.count, r.shape))
        .collect();
    let shapes_sha256 = sha256_hex(histogram.as_bytes());

    // unreachable / missing strings / refused keys
    let mut unreachable = Vec::new();
    let mut missing = Vec::new();
    let mut refused = Vec::new();
    for f in &model.funcs {
        if !f.reach.is_reachable() {
            unreachable.push(UnreachableRow {
                function: f.cpp.name.clone(),
                script: f.script.clone(),
                key: f.key,
                class: f.reach.class().to_owned(),
                reason: f.reach.reason(),
            });
        }
        for (helper, id) in crate::model::text_refs(f.body()) {
            let present = usize::try_from(id).is_ok_and(|i| i < inputs.dialog_lines.len());
            if !present {
                missing.push(MissingString {
                    script: f.script.clone(),
                    function: f.cpp.name.clone(),
                    reachable: f.reach.is_reachable(),
                    helper: helper.to_owned(),
                    id,
                });
            }
        }
        if f.reach.is_reachable() {
            refused_keys_of(inputs, model, &keys, f, &mut refused);
        }
    }
    counts.missing_string_refs = count(missing.len());

    // NPC bindings
    let (mut npc_without_script, mut script_without_npc) = (Vec::new(), Vec::new());
    if let Some(npcs) = &inputs.npcs {
        let mut by_key: BTreeMap<&str, Vec<String>> = BTreeMap::new();
        for n in npcs {
            by_key
                .entry(n.dialog_key.as_str())
                .or_default()
                .push(n.key.clone());
        }
        for (key, list) in &by_key {
            if !model.scripts.contains_key(*key) {
                npc_without_script.push(NpcWithoutScript {
                    dialog_key: (*key).to_owned(),
                    npcs: list.clone(),
                });
            }
        }
        for name in model.scripts.keys() {
            if !by_key.contains_key(name.as_str()) {
                script_without_npc.push(name.clone());
            }
        }
    }

    let golden_differences = diff_golden(&counts);
    Report {
        tool: format!(
            "npcscript-transpile {} (format {})",
            crate::emit::TOOL_VERSION,
            crate::emit::FORMAT
        ),
        counts,
        golden_differences,
        shapes: shape_rows,
        shapes_sha256,
        unreachable,
        missing_strings: missing,
        refused_keys: refused,
        npc_without_script,
        script_without_npc,
        notes,
        errors,
    }
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// The key text the run time computes for a key call, if it can be known statically.
fn refused_keys_of(
    inputs: &Inputs,
    model: &Model,
    keys: &KeyMap,
    f: &crate::model::Func,
    out: &mut Vec<RefusedKey>,
) {
    crate::ast::walk_calls(f.body(), &mut |c| {
        let lit = |i: usize| match c.args.get(i) {
            Some(Expr::Int(v)) => Some(*v),
            _ => None,
        };
        let key_text: Option<String> = match c.helper.name {
            "AddKey" | "AddCondKey" | "AddAnswer" => {
                let word = lit(0).map(|v| inputs.text(v).to_owned());
                let key = if c.args.len() >= 2 {
                    lit(1).map(|v| inputs.text(v).to_owned())
                } else {
                    None
                };
                match (word, key) {
                    (Some(w), None) if c.args.len() < 2 => Some(w),
                    (Some(w), Some(k)) => Some(if k.is_empty() { w } else { k }),
                    _ => None,
                }
            }
            "RemoveKey" | "Run" => lit(0).map(|v| inputs.text(v).to_owned()),
            "AddQuestKey" => lit(1)
                .map(|v| inputs.text(v).to_owned())
                .filter(|t| !t.is_empty()),
            _ => None,
        };
        let Some(key_text) = key_text else { return };
        let reason = match keys.line_of(&key_text) {
            None => Some(Refusal::NotAKey),
            Some(line) => model
                .scripts
                .get(&f.script)
                .is_none_or(|s| !s.reachable.contains_key(&line))
                .then_some(Refusal::NoFunction),
        };
        if let Some(reason) = reason {
            out.push(RefusedKey {
                script: f.script.clone(),
                function: f.cpp.name.clone(),
                helper: c.helper.name.to_owned(),
                key_text,
                reason,
                line: c.line,
            });
        }
    });
}

fn diff_golden(c: &Counts) -> Vec<String> {
    let g = golden();
    let mut rows: Vec<(String, u32, u32)> = vec![
        ("scripts".into(), g.scripts, c.scripts),
        ("functions".into(), g.functions, c.functions),
        ("registrations".into(), g.registrations, c.registrations),
        (
            "unregistered functions".into(),
            g.unregistered_functions,
            c.unregistered_functions,
        ),
        (
            "registration name differs".into(),
            g.registration_name_differs,
            c.registration_name_differs,
        ),
        (
            "capitalised registrations".into(),
            g.capitalised_registrations,
            c.capitalised_registrations,
        ),
        ("reachable".into(), g.reachable, c.reachable),
        ("unreachable".into(), g.unreachable, c.unreachable),
        ("empty bodies".into(), g.empty_bodies, c.empty_bodies),
        ("shapes".into(), g.shapes, c.shapes),
        ("if".into(), g.ifs, c.ifs),
        ("else".into(), g.elses, c.elses),
        ("switch".into(), g.switches, c.switches),
        ("case".into(), g.cases, c.cases),
        ("break".into(), g.breaks, c.breaks),
        (
            "max brace depth".into(),
            g.max_brace_depth,
            c.max_brace_depth,
        ),
        ("float literals".into(), g.float_literals, c.float_literals),
        (
            "distinct constants".into(),
            g.constants_distinct,
            c.constants_distinct,
        ),
        (
            "defined constants".into(),
            g.constants_defined,
            c.constants_defined,
        ),
        (
            "missing string refs".into(),
            g.missing_string_refs,
            c.missing_string_refs,
        ),
        ("other methods".into(), g.other_methods, c.other_methods),
        (
            "distinct helpers".into(),
            g.helpers_distinct,
            c.helpers_distinct,
        ),
    ];
    let union = |a: &BTreeMap<String, u32>, b: &BTreeMap<String, u32>| -> BTreeSet<String> {
        a.keys().chain(b.keys()).cloned().collect()
    };
    for k in union(&g.operators, &c.operators) {
        rows.push((
            format!("operator {k}"),
            g.operators.get(&k).copied().unwrap_or(0),
            c.operators.get(&k).copied().unwrap_or(0),
        ));
    }
    for k in union(&g.helper_calls, &c.helper_calls) {
        rows.push((
            format!("calls of {k}"),
            g.helper_calls.get(&k).copied().unwrap_or(0),
            c.helper_calls.get(&k).copied().unwrap_or(0),
        ));
    }
    let key_union: BTreeSet<u32> = g
        .key_suffix_functions
        .keys()
        .chain(c.key_suffix_functions.keys())
        .copied()
        .collect();
    for k in key_union {
        rows.push((
            format!("functions with key {k}"),
            g.key_suffix_functions.get(&k).copied().unwrap_or(0),
            c.key_suffix_functions.get(&k).copied().unwrap_or(0),
        ));
    }
    rows.into_iter()
        .filter(|(_, want, got)| want != got)
        .map(|(name, want, got)| format!("{name}: golden {want}, measured {got}"))
        .collect()
}
