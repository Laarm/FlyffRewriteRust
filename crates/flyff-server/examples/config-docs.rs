//! `cargo xtask config-docs [--check]` worker: writes (or verifies) the generated configuration artifacts
//! (Plan/03 WP-CORE-90, 10 §8.7): `flyff.toml.example`, `web/src/gen/config/clientFeatures.ts` and
//! `docs/config-keys.md`. With `--check` nothing is written; stale files are listed and the exit code is 1.

#![allow(clippy::print_stdout, clippy::print_stderr)]

use flyff_server::config::docs::artifacts;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let check = std::env::args().any(|a| a == "--check");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut stale = Vec::new();
    for (rel, content) in artifacts() {
        let path = root.join(rel);
        let current = std::fs::read_to_string(&path).ok();
        if current.as_deref() == Some(content.as_str()) {
            continue;
        }
        if check {
            stale.push(rel);
            continue;
        }
        if let Some(parent) = path.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            eprintln!("error: cannot create {}: {e}", parent.display());
            return ExitCode::FAILURE;
        }
        if let Err(e) = std::fs::write(&path, content) {
            eprintln!("error: cannot write {rel}: {e}");
            return ExitCode::FAILURE;
        }
        println!("    wrote {rel}");
    }
    if stale.is_empty() {
        if check {
            println!("    config-docs: up to date");
        }
        ExitCode::SUCCESS
    } else {
        for rel in &stale {
            eprintln!("stale generated file: {rel} (run `cargo xtask config-docs`)");
        }
        ExitCode::FAILURE
    }
}
