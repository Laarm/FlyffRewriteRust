//! `cargo run --release -p flyff-assets --example dump-motions -- [OUT_DIR]`: loads `$FLYFF_RESOURCE` and writes the
//! two derived tables the browser needs next to the 20 bundles of `flyff-data`'s `dump-bundles` example
//! (`motion-table.bin` = the `FLMT` body of `/gen/motion-table/<fp>/all.bin`, `motions.json` = the `motions` bundle,
//! `model-bounds.bin` = `FLMB`, `model-bounds.json`) into `OUT_DIR` (default `dev-motions-out`). The files are for the
//! renderer's dev pages and Playwright specs when no server derives them (`/dev/models`, `/dev/previews`:
//! `FLYFF_GEN_DIR=<OUT_DIR>`, `web/e2e/render/helpers.ts`), never committed.

#![allow(clippy::print_stdout, clippy::print_stderr)]

use flyff_assets::build::bounds::{build_model_bounds, bundle_of};
use flyff_assets::build::motion::{build_motion_table, motions_bundle};
use flyff_data::bundle::envelope::encode_envelope;
use flyff_data::{DataOptions, load_all};
use flyff_vfs::{Vfs, VfsConfig};
use std::path::PathBuf;
use std::process::ExitCode;

fn run() -> Result<(), String> {
    let root = std::env::var_os("FLYFF_RESOURCE")
        .map(PathBuf::from)
        .ok_or("FLYFF_RESOURCE is not set")?;
    let out = std::env::args_os()
        .nth(1)
        .map_or_else(|| PathBuf::from("dev-motions-out"), PathBuf::from);
    std::fs::create_dir_all(&out).map_err(|e| format!("create {}: {e}", out.display()))?;
    let vfs =
        Vfs::open(&VfsConfig::new(&root)).map_err(|e| format!("open {}: {e}", root.display()))?;
    let opts = DataOptions {
        load_client_data: false,
        ..DataOptions::default()
    };
    let data = load_all(&vfs, &opts).map_err(|e| e.to_string())?;

    let motions =
        build_motion_table(&vfs, &data.game.models, &|| false).map_err(|e| e.to_string())?;
    let write = |name: &str, bytes: &[u8]| -> Result<(), String> {
        let p = out.join(name);
        std::fs::write(&p, bytes).map_err(|e| format!("write {}: {e}", p.display()))?;
        println!("  {name:20} {:>10} B", bytes.len());
        Ok(())
    };
    println!("{}", out.display());
    write("motion-table.bin", &motions.table.encode())?;
    let json = encode_envelope(&motions_bundle(&motions.table)).map_err(|e| e.to_string())?;
    write("motions.json", &json)?;

    let bounds = build_model_bounds(&vfs, &|| false).map_err(|e| e.to_string())?;
    write("model-bounds.bin", &bounds.bounds.encode())?;
    let json = encode_envelope(&bundle_of(&bounds.entries)).map_err(|e| e.to_string())?;
    write("model-bounds.json", &json)?;
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("dump-motions: {e}");
            ExitCode::FAILURE
        }
    }
}
