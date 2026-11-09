//! Build and code-generation commands for FlyffRewriteRust.

// xtask is a CLI: printing is its output channel.
#![allow(clippy::print_stdout, clippy::print_stderr)]

mod build_id;
mod cmd;
mod meta;
mod proc;
mod tools;

use anyhow::Result;
use clap::{Parser, Subcommand};
use proc::Ctx;

/// Build orchestration for the Flyff V15 rewrite.
#[derive(Parser, Debug)]
#[command(
    name = "xtask",
    about = "Build orchestration for the Flyff V15 rewrite (Plan/02 §4)"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Tool versions, Rust targets, FLYFF_RESOURCE/FLYFF_SOURCE, free disk space.
    Doctor,

    /// Pinned tools: install, check, path.
    #[command(subcommand)]
    Tools(ToolsCmd),
    /// Run every registered generator (the clippy.toml generator, gen-proto when buf is installed).
    Gen {
        /// Verify instead of writing.
        #[arg(long)]
        check: bool,
    },
    /// Define constants for Rust and TypeScript from the real Resource headers (Plan/14 §10.9).
    GenDefs(cmd::gen_defs::Args),
    /// JSON Schemas and TypeScript types of the client data bundles from the Rust DTOs (Plan/14 §10.7).
    GenDataTypes(cmd::gen_data_types::Args),
    /// Generated configuration artifacts: flyff.toml.example, web/src/gen/config/clientFeatures.ts, docs/config-keys.md.
    ConfigDocs(cmd::config_docs::Args),
    /// The capability enum of the admin console from crates/flyff-server/src/admin/caps.toml (Plan/25 §1.6, WP-ADMIN-02).
    AdminCaps(cmd::admin_gen::Args),
    /// The TypeScript DTOs of the admin console from the Rust DTOs of flyff-server (Plan/25 §11.10, WP-ADMIN-02).
    AdminDto(cmd::admin_gen::Args),
    /// Protobuf code generation (implemented by boot-web).
    GenProto(cmd::gen_proto::Args),
    /// Coverage reports: `proto` = the legacy-id gate of the wire protocol (tools/proto-coverage, C1-C9; `--sync-sites`).
    Coverage(cmd::coverage::Args),
    /// Build the wasm module (implemented by boot-web).
    BuildWasm(cmd::build_wasm::Args),
    /// Build the web client (implemented by boot-web).
    BuildWeb(cmd::build_web::Args),
    /// Dev environment: dev home, Rust server, Vite, wasm watcher.
    Dev(cmd::dev::Args),

    /// NpcScript.cpp to content/npc/*.lua with tools/npcscript-transpile (needs FLYFF_SOURCE and FLYFF_RESOURCE); --check compares only.
    NpcTranspile(cmd::npc_transpile::Args),
    /// Build the release package (static musl executable with the embedded web client).
    Dist(cmd::dist::Args),

    /// Dependency gate (X1, X5-X7, tiers, wasm closure, shipped features).
    DepRules,
    /// Source scanner for determinism and panic-free rules.
    LintDeterminism,
}

#[derive(Subcommand, Debug)]
enum ToolsCmd {
    /// Verify the required build tools.
    Check,
    /// Install the pinned tools that can be installed automatically into .tools/.
    Install {
        /// Also install system packages with `sudo apt-get`.
        #[arg(long)]
        apt: bool,
    },

    /// Print the .tools/bin directory.
    Path,
}

fn main() {
    if let Err(e) = real_main() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn real_main() -> Result<()> {
    let cli = Cli::parse();
    let ctx = Ctx::discover()?;
    match cli.command {
        Command::Doctor => cmd::doctor::run(&ctx),

        Command::Tools(ToolsCmd::Install { apt }) => tools::cmd_install(&ctx, apt),
        Command::Tools(ToolsCmd::Check) => tools::cmd_check(&ctx),
        Command::Tools(ToolsCmd::Path) => {
            tools::cmd_path(&ctx);
            Ok(())
        }
        Command::Gen { check } => {
            cmd::gen_lint::run(&ctx, check)?;
            cmd::gen_defs::gen_if_available(&ctx, check)?;
            cmd::config_docs::run(&ctx, check)?;
            cmd::gen_data_types::run(&ctx, check)?;
            cmd::admin_gen::caps(&ctx, check)?;
            cmd::admin_gen::dto(&ctx, check)?;
            cmd::gen_proto::gen_if_available(&ctx, check)
        }
        Command::GenDefs(a) => cmd::gen_defs::run(&ctx, &a),
        Command::GenDataTypes(a) => cmd::gen_data_types::run(&ctx, a.check),
        Command::ConfigDocs(a) => cmd::config_docs::run(&ctx, a.check),
        Command::AdminCaps(a) => cmd::admin_gen::caps(&ctx, a.check),
        Command::AdminDto(a) => cmd::admin_gen::dto(&ctx, a.check),
        Command::GenProto(a) => cmd::gen_proto::run(&ctx, &a),
        Command::Coverage(a) => cmd::coverage::run(&ctx, &a),
        Command::BuildWasm(a) => cmd::build_wasm::run(&ctx, &a),
        Command::BuildWeb(a) => cmd::build_web::run(&ctx, &a),
        Command::Dev(a) => cmd::dev::run(&ctx, &a),

        Command::NpcTranspile(a) => cmd::npc_transpile::run(&ctx, &a),
        Command::Dist(a) => cmd::dist::run(&ctx, &a),

        Command::DepRules => cmd::dep_rules::run(&ctx),
        Command::LintDeterminism => cmd::lint_det::run(&ctx),
    }
}
