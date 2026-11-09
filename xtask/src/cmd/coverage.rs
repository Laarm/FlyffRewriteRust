//! `cargo xtask coverage [proto] [--strict] [--stub-max-milestone M<n>] [--sync-sites]` (Plan/02 section 4 and 9.6).
//!
//! `proto` runs `tools/proto-coverage` (the legacy-id gate: checks C1-C8 of Plan/11 section 11 and rule C9 of Plan/02
//! section 9.6, the `// legacy-send:` markers under `web/src`) and fails on any error; `--strict` also fails on warnings.
//! The threshold `PROTO_STUB_MAX_MILESTONE` (C6, and the milestone of the due `todo` rows of C9) defaults to
//! `current_milestone` of `docs/wp-status.toml`; `--stub-max-milestone M8` proves the error mode of C9. `--sync-sites`
//! sets `done` on every `todo` row of `proto/legacy/ui_send_sites.csv` that has a marker (never the reverse) before the
//! gate runs. The milestone-close dry run (G-Mn.8) is `cargo xtask coverage proto --stub-max-milestone M<n+1>`. The
//! line-coverage report (`cargo-llvm-cov`) of the plan is not part of the gate and is not built.

use crate::proc::{Ctx, Run, banner};
use anyhow::Result;
use clap::{Args as ClapArgs, ValueEnum};

/// Which coverage report.
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which {
    /// The legacy-id gate of the wire protocol.
    Proto,
}

/// Arguments of `cargo xtask coverage`.
#[derive(ClapArgs, Debug)]
pub struct Args {
    /// Report to run (default and only one: `proto`).
    pub which: Option<Which>,
    /// Fail on warnings as well.
    #[arg(long)]
    pub strict: bool,
    /// `PROTO_STUB_MAX_MILESTONE` for this run, for example `M3`.
    #[arg(long)]
    pub stub_max_milestone: Option<String>,
    /// Set `done` on every `todo` row of ui_send_sites.csv that has a `// legacy-send:` marker, then run the gate.
    #[arg(long)]
    pub sync_sites: bool,
}

/// Entry point of the command.
pub fn run(ctx: &Ctx, args: &Args) -> Result<()> {
    proto(
        ctx,
        args.strict,
        args.stub_max_milestone.as_deref(),
        args.sync_sites,
    )
}

/// The gate: used by `cargo xtask check` and `cargo xtask coverage proto`.
pub fn proto(ctx: &Ctx, strict: bool, stub_max: Option<&str>, sync_sites: bool) -> Result<()> {
    banner("coverage", 1, 1, "proto-coverage (legacy-id gate, C1-C9)");
    let mut run = Run::new(ctx, "cargo")
        .args(["run", "-q", "-p", "proto-coverage", "--"])
        .arg("--root")
        .arg(&ctx.root);
    if strict {
        run = run.arg("--deny-warnings");
    }
    if let Some(m) = stub_max {
        run = run.arg("--stub-max-milestone").arg(m);
    }
    if sync_sites {
        run = run.arg("--sync-sites");
    }
    run.run()
}
