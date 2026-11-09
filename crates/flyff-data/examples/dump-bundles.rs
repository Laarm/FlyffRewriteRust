//! `cargo run --release -p flyff-data --features load,bundle --example dump-bundles -- [OUT_DIR]`: loads `$FLYFF_RESOURCE`
//! and writes the 20 client data bundles of the real data set (`<bundle>.<h16>.json(.gz)`, `manifest.json`) into
//! `OUT_DIR` (default `data-bundles-out`) through the same `BundleStore` the server cache uses (14 §10.3). The
//! files are for web tests against the real data (`FLYFF_BUNDLES_DIR=<OUT_DIR> pnpm exec vitest run`) and for
//! inspecting the sizes.

#![allow(clippy::print_stdout, clippy::print_stderr)]

use flyff_data::bundle::store::{BundleStore, StoreOutcome, fingerprint, vfs_tree_digest};
use flyff_data::bundle::{ExportOptions, export_all};
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
        .map_or_else(|| PathBuf::from("data-bundles-out"), PathBuf::from);
    let vfs =
        Vfs::open(&VfsConfig::new(&root)).map_err(|e| format!("open {}: {e}", root.display()))?;
    let data = load_all(&vfs, &DataOptions::default()).map_err(|e| e.to_string())?;
    let client = data
        .client
        .as_ref()
        .ok_or("the loader produced no client data")?;
    let fp = fingerprint(data.game.meta.fingerprint, vfs_tree_digest(&vfs));
    let store = BundleStore::new(&out);
    let outcome = store
        .ensure(&fp, || {
            export_all(&data.game, client, &vfs, &ExportOptions::default())
        })
        .map_err(|e| e.to_string())?;
    let (what, manifest) = match &outcome {
        StoreOutcome::Hit(m) => ("up to date", m),
        StoreOutcome::Built(m) => ("written", m),
    };
    println!(
        "{} ({what}), data_version {}",
        out.display(),
        manifest.data_version
    );
    let (mut raw, mut gz) = (0u64, 0u64);
    for (name, e) in &manifest.bundles {
        println!(
            "  {name:16} {:>9} B  gzip {:>8} B  {:?}  {}",
            e.bytes, e.gz_bytes, e.load, e.url
        );
        raw += e.bytes;
        gz += e.gz_bytes;
    }
    println!("  {:16} {raw:>9} B  gzip {gz:>8} B", "total");
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("dump-bundles: {e}");
            ExitCode::FAILURE
        }
    }
}
