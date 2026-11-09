//! `npcscript-transpile`: `NpcScript.cpp` to `content/npc/*.lua` (Plan/20 §5, WP-NPC-03 front end and
//! WP-NPC-04 emitter).
//!
//! ```text
//! Inputs (sources + WorldDialog.txt + defines + NPC list)
//!   -> source:   comment stripping, function bodies, g_tables, g_nKeys
//!   -> lex/parse: typed AST (ast), helper table (helpers)
//!   -> model:    reachability, scripts, shapes, statistics
//!   -> report:   counts, golden differences, unreachable, missing strings, refused keys, errors
//!   -> emit/output: content/npc/*.lua + MANIFEST.txt + _transpile.toml
//! ```
//!
//! [`run`] is the whole pipeline over in-memory [`inputs::Inputs`]; the binary adds the file system.
//! A library so the integration tests can drive each stage; not shipped.

pub mod ast;
pub mod crosscheck;
pub mod dialog;
pub mod emit;
pub mod error;
pub mod helpers;
pub mod inputs;
pub mod lex;
pub mod model;
pub mod output;
pub mod parse;
pub mod report;
pub mod source;

use inputs::Inputs;
use model::Model;
use output::Output;
use report::Report;

/// Everything one run produces.
#[derive(Debug)]
pub struct Outcome {
    /// The front end result.
    pub model: Model,
    /// The validation report.
    pub report: Report,
    /// The generated directory content; `None` when the report has errors.
    pub output: Option<Output>,
}

/// Runs the pipeline over `inputs`.
pub fn run(inputs: &Inputs) -> Outcome {
    let model = model::build(inputs);
    let (header_errors, notes) = crosscheck::check_helpers(
        inputs.npc_script_h.as_ref().map(|f| f.text.as_str()),
        inputs.functions_info_h.as_ref().map(|f| f.text.as_str()),
    );
    let report = report::build_report(inputs, &model, header_errors, notes);
    let output = report
        .is_clean()
        .then(|| output::generate(inputs, &model, &report));
    Outcome {
        model,
        report,
        output,
    }
}
