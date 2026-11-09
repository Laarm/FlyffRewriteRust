//! `npcscript-transpile --source <Source dir> --resource <Resource dir> --out content/npc [--report <dir>] [--check]`
//! (Plan/20 §5.1). Exit codes: 0 ok, 1 `--check` found a difference, 2 parse/validation/input error.

// A CLI: printing is its output channel.
#![allow(clippy::print_stdout, clippy::print_stderr)]

use clap::Parser;
use npcscript_transpile::inputs::Inputs;
use npcscript_transpile::output::{check_output, write_output};
use std::path::PathBuf;
use std::process::ExitCode;

/// NpcScript.cpp -> content/npc/*.lua
#[derive(Parser, Debug)]
#[command(name = "npcscript-transpile", version, about)]
struct Args {
    /// The original `Source/` folder (holds `WORLDDIALOG/NpcScript.cpp`); default `$FLYFF_SOURCE`.
    #[arg(long)]
    source: Option<PathBuf>,
    /// The original `Resource/` folder (`WorldDialog.txt`, the define headers, `character*.inc`); default
    /// `$FLYFF_RESOURCE`.
    #[arg(long)]
    resource: Option<PathBuf>,
    /// The output folder (`content/npc`).
    #[arg(long, default_value = "content/npc")]
    out: PathBuf,
    /// Where `report.json` and `report.md` go.
    #[arg(long, default_value = "target/npc-transpile")]
    report: PathBuf,
    /// Do not write the report files.
    #[arg(long)]
    no_report: bool,
    /// Generate in memory and compare with `--out` byte for byte instead of writing (exit 1 on a difference).
    #[arg(long)]
    check: bool,
}

fn main() -> ExitCode {
    let args = Args::parse();
    match real_main(&args) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(2)
        }
    }
}

fn dir_arg(arg: &Option<PathBuf>, var: &str, flag: &str) -> anyhow::Result<PathBuf> {
    match arg {
        Some(p) => Ok(p.clone()),
        None => std::env::var_os(var)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("pass {flag} <dir> or set {var}")),
    }
}

fn real_main(args: &Args) -> anyhow::Result<u8> {
    let source = dir_arg(&args.source, "FLYFF_SOURCE", "--source")?;
    let resource = dir_arg(&args.resource, "FLYFF_RESOURCE", "--resource")?;
    let inputs = match Inputs::from_dirs(&source, &resource) {
        Ok(i) => i,
        Err(findings) => {
            for f in &findings {
                eprintln!("{f}");
            }
            return Ok(2);
        }
    };
    let outcome = npcscript_transpile::run(&inputs);
    let report = &outcome.report;
    if !args.no_report {
        std::fs::create_dir_all(&args.report)?;
        std::fs::write(args.report.join("report.json"), report.to_json())?;
        std::fs::write(args.report.join("report.md"), report.to_markdown())?;
    }
    let c = &report.counts;
    println!(
        "{} scripts, {} functions ({} reachable, {} unreachable), {} shapes, {} registrations",
        c.scripts, c.functions, c.reachable, c.unreachable, c.shapes, c.registrations
    );
    if !report.golden_differences.is_empty() {
        println!(
            "note: {} count(s) differ from the golden values of Plan/20 §5.2 (the shipped input has none); first: {}",
            report.golden_differences.len(),
            report
                .golden_differences
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join("; ")
        );
    }
    for n in &report.notes {
        println!("note: {n}");
    }
    println!(
        "{} missing strings, {} refused keys, {} NPC dialogs without a script, {} scripts without an NPC",
        report.missing_strings.len(),
        report.refused_keys.len(),
        report.npc_without_script.len(),
        report.script_without_npc.len()
    );
    let Some(output) = outcome.output else {
        for f in report.all_errors() {
            eprintln!("{f}");
        }
        eprintln!(
            "{} error(s): nothing was written",
            report.all_errors().len()
        );
        return Ok(2);
    };
    if args.check {
        let diffs = check_output(&args.out, &output);
        if diffs.is_empty() {
            println!(
                "{}: identical ({} files)",
                args.out.display(),
                output.files.len()
            );
            return Ok(0);
        }
        for d in &diffs {
            eprintln!("{}: {d}", args.out.display());
        }
        eprintln!(
            "{} difference(s): run without --check to regenerate",
            diffs.len()
        );
        return Ok(1);
    }
    write_output(&args.out, &output)?;
    println!("{}: wrote {} files", args.out.display(), output.files.len());
    Ok(0)
}
