//! `cargo xtask config-docs [--check]` (Plan/03 WP-CORE-90, 10 §8.7): the generated configuration artifacts.
//!
//! * `crates/flyff-server/assets/flyff.toml.example`: the commented default template,
//! * `web/src/gen/config/clientFeatures.ts`: the typed `ClientFeatureConfig` (one field per registry key with
//!   the C flag) and its defaults,
//! * `docs/config-keys.md`: the key reference.
//!
//! The generators live in `flyff_server::config::docs` (they read the three key tables: `OPS_KEYS`,
//! `flyff_db::config::KEY_DOCS`, `flyff_common::config::KEY_REGISTRY`); this command runs the
//! `config-docs` example of `flyff-server`. With `--check` nothing is written: stale files are named and the
//! command fails. `cargo xtask gen [--check]` runs it too.

use crate::proc::{Ctx, Run};
use anyhow::{Context, Result};

/// Arguments of `cargo xtask config-docs`.
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
        "flyff-server",
        "--example",
        "config-docs",
        "--",
    ]);
    if check {
        cmd = cmd.arg("--check");
    }
    cmd.run().with_context(|| {
        if check {
            "the generated configuration files are stale; run `cargo xtask config-docs`"
        } else {
            "config-docs failed"
        }
    })
}
