//! `cargo xtask gen-data-types [--check]` (Plan/14 §10.7, WP-DATA-18): the JSON Schemas of the client data bundles
//! (`schemas/data/*.schema.json`) and their TypeScript types (`web/src/gen/bundles/`), generated from the Rust
//! DTOs of `flyff_data::bundle::types`. The generators live in `flyff_data::bundle::{schema, tsgen}`; this command
//! runs the `gen-bundle-types` example of `flyff-data`. With `--check` nothing is written: stale files are named
//! and the command fails. `cargo xtask gen [--check]` runs it too. It also covers the TypeScript of the client asset
//! manifest (`web/src/gen/data/`, from `flyff-assets`, WP-WEB-06).

use crate::proc::{Ctx, Run};
use anyhow::{Context, Result};

/// Arguments of `cargo xtask gen-data-types`.
#[derive(clap::Args, Debug, Clone, Default)]
pub struct Args {
    /// Verify instead of writing; exit 1 naming every stale file.
    #[arg(long)]
    pub check: bool,
}

/// Runs the generator (or the check).
pub fn run(ctx: &Ctx, check: bool) -> Result<()> {
    let mut cmd = Run::new(ctx, "cargo").args([
        "run",
        "--quiet",
        "-p",
        "flyff-data",
        "--features",
        "bundle,schema",
        "--example",
        "gen-bundle-types",
        "--",
    ]);
    if check {
        cmd = cmd.arg("--check");
    }
    cmd.run().with_context(|| {
        if check {
            "the generated bundle schemas/types are stale; run `cargo xtask gen-data-types`"
        } else {
            "gen-data-types failed"
        }
    })?;
    asset_manifest_types(ctx, check)
}

/// `web/src/gen/data/*.ts`: the TypeScript of the client asset manifest (Plan/24 §6.4, WP-WEB-06), exported by the
/// ts-rs test of `flyff-assets` (`UPDATE_GOLDEN=1` rewrites, otherwise it fails on drift).
fn asset_manifest_types(ctx: &Ctx, check: bool) -> Result<()> {
    let mut cmd = Run::new(ctx, "cargo").args([
        "test",
        "--quiet",
        "-p",
        "flyff-assets",
        "--features",
        "ts",
        "--test",
        "ts_export",
    ]);
    if !check {
        cmd = cmd.env("UPDATE_GOLDEN", "1");
    }
    cmd.run().with_context(|| {
        if check {
            "web/src/gen/data (the asset manifest types) is stale; run `cargo xtask gen-data-types`"
        } else {
            "generating the asset manifest types failed"
        }
    })
}
