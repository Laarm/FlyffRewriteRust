//! `cargo xtask admin-dto [--check]` worker: writes (or verifies) `web/src/admin/gen/dto.ts`, the TypeScript of the
//! DTO catalogue of the admin API (Plan/25 §11.10, WP-ADMIN-02). Needs the cargo feature `ts`. With `--check` nothing
//! is written; a stale file is named and the exit code is 1.

#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::path::PathBuf;
use std::process::ExitCode;

const REL: &str = "web/src/admin/gen/dto.ts";

fn main() -> ExitCode {
    let check = std::env::args().any(|a| a == "--check");
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(REL);
    let content = flyff_server::admin::dto::typescript();
    let current = std::fs::read_to_string(&path).ok();
    if current.as_deref() == Some(content.as_str()) {
        if check {
            println!("    admin-dto: up to date");
        }
        return ExitCode::SUCCESS;
    }
    if check {
        eprintln!("stale generated file: {REL} (run `cargo xtask admin-dto`)");
        return ExitCode::FAILURE;
    }
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        eprintln!("error: cannot create {}: {e}", parent.display());
        return ExitCode::FAILURE;
    }
    if let Err(e) = std::fs::write(&path, content) {
        eprintln!("error: cannot write {REL}: {e}");
        return ExitCode::FAILURE;
    }
    println!("    wrote {REL}");
    ExitCode::SUCCESS
}
