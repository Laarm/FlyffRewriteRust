//! `proto-coverage`: the gate of Plan/11 section 11 (C1-C8) with rule C9 of Plan/02 section 9.6 and the one-off importers
//! of WP-PROTO-04 / WP-PROTO-05.

// A command line tool: printing is its output channel.
#![allow(clippy::print_stdout, clippy::print_stderr)]

use anyhow::{Context, Result, bail};
use clap::Parser;
use proto_coverage::checks::Severity;
use proto_coverage::{Config, csv, import};
use std::path::PathBuf;
use std::process::ExitCode;

/// Legacy-id coverage gate of the wire protocol (checks C1-C8 of Plan/11 section 11, rule C9 of Plan/02 section 9.6).
#[derive(Parser, Debug)]
#[command(name = "proto-coverage", version)]
struct Cli {
    /// Repository root (default: the workspace this tool was built in).
    #[arg(long)]
    root: Option<PathBuf>,
    /// Descriptor set (default: the one compiled into flyff-proto).
    #[arg(long)]
    descriptor: Option<PathBuf>,
    /// legacy_index.csv (default: proto/legacy/legacy_index.csv).
    #[arg(long)]
    legacy_index: Option<PathBuf>,
    /// ui_send_sites.csv (default: proto/legacy/ui_send_sites.csv).
    #[arg(long)]
    ui_sites: Option<PathBuf>,
    /// Client sources scanned for `// legacy-send:` markers, rule C9 (default: web/src; a missing directory skips C9).
    #[arg(long)]
    web_src: Option<PathBuf>,
    /// Set `done` on every `todo` row of ui_send_sites.csv that has a marker under web/src (never the reverse), then run the gate.
    #[arg(long)]
    sync_sites: bool,
    /// Report directory (default: target/reports).
    #[arg(long)]
    out_dir: Option<PathBuf>,
    /// PROTO_STUB_MAX_MILESTONE, for example M3 (default: the environment variable, then current_milestone).
    #[arg(long)]
    stub_max_milestone: Option<String>,
    /// Fail on warnings as well.
    #[arg(long)]
    deny_warnings: bool,
    /// Import mode: parse annex 11a sections 2-4 and 7 and write the legacy index CSV.
    #[arg(long, value_name = "11A_MD")]
    import_11a: Option<PathBuf>,
    /// Import mode: parse ref/99 appendix B and write the UI send-site CSV (needs the legacy index).
    #[arg(long, value_name = "REF99_MD")]
    import_ui_sites: Option<PathBuf>,
}

fn default_root() -> PathBuf {
    // tools/proto-coverage -> repository root
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn main() -> ExitCode {
    match real_main() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("proto-coverage: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn real_main() -> Result<ExitCode> {
    let cli = Cli::parse();
    let root = cli.root.clone().unwrap_or_else(default_root);
    let mut cfg = Config::for_root(&root);
    cfg.descriptor = cli.descriptor.clone();
    if let Some(p) = &cli.legacy_index {
        cfg.index = p.clone();
    }
    if let Some(p) = &cli.ui_sites {
        cfg.sites = p.clone();
    }
    if let Some(p) = &cli.web_src {
        cfg.web_src = p.clone();
    }
    if let Some(p) = &cli.out_dir {
        cfg.out_dir = p.clone();
    }
    cfg.stub_max = cli.stub_max_milestone.clone();

    if cli.import_11a.is_some() || cli.import_ui_sites.is_some() {
        if let Some(md) = &cli.import_11a {
            let text =
                std::fs::read_to_string(md).with_context(|| format!("read {}", md.display()))?;
            let imported = import::import_11a(&text)?;
            std::fs::write(&cfg.index, csv::write_index(&imported.rows))
                .with_context(|| format!("write {}", cfg.index.display()))?;
            println!(
                "wrote {} rows to {}",
                imported.rows.len(),
                cfg.index.display()
            );
            for (name, c) in ["§2 C->S", "§3 S->C", "§4 snapshots", "§7 internal"]
                .iter()
                .zip(imported.counts)
            {
                println!(
                    "  {name}: {} rows (port {}, merge {}, split {}, drop {}, internal {})",
                    c[0], c[1], c[2], c[3], c[4], c[5]
                );
            }
        }
        if let Some(md) = &cli.import_ui_sites {
            let text =
                std::fs::read_to_string(md).with_context(|| format!("read {}", md.display()))?;
            let index_text = std::fs::read_to_string(&cfg.index).with_context(|| {
                format!("read {} (run --import-11a first)", cfg.index.display())
            })?;
            let index = csv::read_index(&index_text)?;
            let sites = import::import_ui_sites(&text, &index)?;
            std::fs::write(&cfg.sites, csv::write_sites(&sites))
                .with_context(|| format!("write {}", cfg.sites.display()))?;
            println!(
                "wrote {} send functions to {}",
                sites.len(),
                cfg.sites.display()
            );
        }
        return Ok(ExitCode::SUCCESS);
    }

    if cli.sync_sites {
        let flipped = proto_coverage::sync_sites(&cfg)?;
        if flipped.is_empty() {
            println!("sync-sites: every row that has a marker is already done");
        } else {
            println!(
                "sync-sites: {} row(s) set to done: {}",
                flipped.len(),
                flipped.join(", ")
            );
        }
    }
    let outcome = proto_coverage::run(&cfg)?;
    let r = &outcome.report;
    println!(
        "proto-coverage: {} C->S + {} S->C envelope messages, {} legacy rows, {} send functions, {} stubs, {} errors, {} warnings",
        r.envelope_c2s,
        r.envelope_s2c,
        r.legacy_rows,
        r.send_functions,
        r.stubs.len(),
        r.errors,
        r.warnings
    );
    if r.c9.ran {
        println!(
            "  C9: {} marker lines for {} send functions in {} files; {} todo rows due by {}; {} unknown, {} malformed",
            r.c9.marker_lines,
            r.c9.distinct_names,
            r.c9.marker_files,
            r.c9.due.values().map(Vec::len).sum::<usize>(),
            r.c9.current_milestone,
            r.c9.unknown_markers.len(),
            r.c9.malformed_markers.len()
        );
    } else {
        println!("  C9: not evaluated ({})", r.c9.note);
    }
    println!(
        "  report: {} and {}",
        outcome.md_path.display(),
        outcome.json_path.display()
    );
    for f in outcome
        .findings
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .take(60)
    {
        eprintln!(
            "  error {} {}: {}{}",
            f.check,
            f.subject,
            f.detail,
            f.loc.as_ref().map_or(String::new(), |l| format!(" ({l})"))
        );
    }
    if r.errors > 60 {
        eprintln!("  ... and {} more (see the report)", r.errors - 60);
    }
    // the warnings (C9: the todo rows that are due) are listed, but capped: the report has all of them
    for f in outcome
        .findings
        .iter()
        .filter(|f| f.severity == Severity::Warning)
        .take(40)
    {
        eprintln!(
            "  warning {} {}: {}{}",
            f.check,
            f.subject,
            f.detail,
            f.loc.as_ref().map_or(String::new(), |l| format!(" ({l})"))
        );
    }
    if r.warnings > 40 {
        eprintln!(
            "  ... and {} more warnings (see the report)",
            r.warnings - 40
        );
    }
    if !r.ok {
        bail!("{} error(s); see {}", r.errors, outcome.md_path.display());
    }
    if cli.deny_warnings && r.warnings > 0 {
        bail!(
            "{} warning(s) with --deny-warnings; see {}",
            r.warnings,
            outcome.md_path.display()
        );
    }
    Ok(ExitCode::SUCCESS)
}
