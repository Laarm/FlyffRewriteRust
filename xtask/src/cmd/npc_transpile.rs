//! `cargo xtask npc-transpile [--check]`: `NpcScript.cpp` to `content/npc/*.lua` (Plan/20 §5.1, WP-NPC-04).
//!
//! A wrapper of `tools/npcscript-transpile`: it resolves `FLYFF_SOURCE` and `FLYFF_RESOURCE` (or the flags),
//! runs the tool against `content/npc` and passes its exit code on. `--check` regenerates in memory and fails
//! when the committed files differ byte for byte (Plan/02 §9.3 step 5, the Resource+Source-gated CI job).

use crate::proc::{Ctx, Run};
use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

/// Arguments of `cargo xtask npc-transpile`.
#[derive(clap::Args, Debug, Clone, Default)]
pub struct Args {
    /// The original `Source/` directory (default: `$FLYFF_SOURCE`, else `Source/` next to the Resource folder).
    #[arg(long)]
    pub source: Option<PathBuf>,
    /// The original `Resource/` directory (default: `$FLYFF_RESOURCE`).
    #[arg(long)]
    pub resource: Option<PathBuf>,
    /// Regenerate in memory and fail on any difference with `content/npc`.
    #[arg(long)]
    pub check: bool,
}

fn has_worlddialog(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|rd| {
        rd.filter_map(Result::ok).any(|e| {
            e.file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case("worlddialog")
                && e.path().is_dir()
        })
    })
}

/// Resolves the Source folder: the flag, `$FLYFF_SOURCE`, then `Source/` beside the Resource folder.
pub fn resolve_source(flag: Option<&Path>, resource: &Path) -> Result<PathBuf> {
    let env = std::env::var_os("FLYFF_SOURCE").map(PathBuf::from);
    let beside = resource.parent().map(|p| p.join("Source"));
    for cand in [flag.map(Path::to_path_buf), env, beside]
        .into_iter()
        .flatten()
    {
        if has_worlddialog(&cand) {
            return Ok(cand);
        }
    }
    bail!(
        "the original Source folder (with WORLDDIALOG/NpcScript.cpp) was not found: pass --source or set FLYFF_SOURCE"
    )
}

/// Runs the tool.
pub fn run(ctx: &Ctx, args: &Args) -> Result<()> {
    let resource = match args
        .resource
        .clone()
        .or_else(|| std::env::var_os("FLYFF_RESOURCE").map(PathBuf::from))
    {
        Some(r) if r.is_dir() => r,
        _ => bail!(
            "the original Resource folder was not found: pass --resource or set FLYFF_RESOURCE"
        ),
    };
    let source = resolve_source(args.source.as_deref(), &resource)?;
    let mut run = Run::new(ctx, "cargo")
        .args(["run", "--quiet", "-p", "npcscript-transpile", "--"])
        .arg("--source")
        .arg(&source)
        .arg("--resource")
        .arg(&resource)
        .arg("--out")
        .arg(ctx.path("content/npc"))
        .arg("--report")
        .arg(ctx.path("target/npc-transpile"));
    if args.check {
        run = run.arg("--check");
    }
    run.run()
}
