//! `cargo xtask gen-data-types [--check]` worker (Plan/14 §10.7): writes (or verifies) the JSON Schemas of the
//! client data bundles (`schemas/data/*.schema.json`) and their TypeScript types (`web/src/gen/bundles/`), all
//! derived from the Rust DTOs of `flyff_data::bundle::types`. With `--check` nothing is written: stale files are
//! listed and the exit code is 1.

#![allow(clippy::print_stdout, clippy::print_stderr)]

use flyff_data::bundle::artifacts::artifacts;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let check = std::env::args().any(|a| a == "--check");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut stale = Vec::new();
    let mut written = 0usize;
    for (rel, content) in artifacts() {
        let path = root.join(&rel);
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
        written += 1;
        println!("    wrote {rel}");
    }
    if stale.is_empty() {
        println!(
            "    gen-data-types: {}",
            if check {
                "up to date".to_owned()
            } else {
                format!("{written} file(s) written")
            }
        );
        ExitCode::SUCCESS
    } else {
        for rel in &stale {
            eprintln!("stale generated file: {rel} (run `cargo xtask gen-data-types`)");
        }
        ExitCode::FAILURE
    }
}
