//! Publishing the JSON projections of the two binary derived assets (`motions`, `model-bounds`; Plan/24
//! §10.5, §10.6) into `cache/data/`, where `/data` serves the bundles of doc 14.
//!
//! The kind's job stores the envelope JSON as one more artifact (`motions.json`, `model-bounds.json`) next
//! to `all.bin`, so both outputs share one fingerprint and one build. The `load_server` hook of the kind
//! calls [`publish`]: it writes `data/<name>.<h16>.json` and `.json.gz` (the layout of
//! `flyff_data::bundle::store`), removes the files of older versions of the same bundle, and records the
//! bundle's `manifest.json` line in [`crate::ManifestParts::set_external_bundle`] for the manifest
//! finalisation (`BundleManifest::with_external`, WP-WEB-05).

use crate::error::AssetError;
use crate::prep::AssetPrep;
use flyff_data::bundle::envelope::Bundle;
use flyff_data::bundle::{LoadClass, ManifestEntry};
use std::fs;

/// Writes the bundle files of `json` (the envelope bytes) and registers the manifest entry; returns it.
/// Idempotent: a file that already has the right size is not rewritten.
pub fn publish(
    prep: &AssetPrep,
    name: &str,
    schema: u32,
    load: LoadClass,
    json: Vec<u8>,
) -> Result<ManifestEntry, AssetError> {
    let bundle = Bundle::from_json(name, schema, load, json);
    let cache = prep.cache();
    for (rel, bytes) in [
        (format!("data/{}", bundle.file_name()), &bundle.json),
        (format!("data/{}.gz", bundle.file_name()), &bundle.gz),
    ] {
        let current = fs::metadata(cache.path_of(&rel)).ok().map(|m| m.len());
        if current != Some(bytes.len() as u64) {
            cache.write_atomic(&rel, bytes, false)?;
        }
    }
    remove_stale(prep, name, &bundle.h16);
    let entry = bundle.entry();
    prep.manifest_parts()
        .set_external_bundle(name, entry.clone());
    Ok(entry)
}

/// Deletes `data/<name>.<h16>.json(.gz)` files whose hash is not `keep` (the bundle changed).
fn remove_stale(prep: &AssetPrep, name: &str, keep: &str) {
    let Ok(rd) = fs::read_dir(prep.cache().path_of("data")) else {
        return;
    };
    let prefix = format!("{name}.");
    for ent in rd.flatten() {
        let file = ent.file_name().to_string_lossy().into_owned();
        let Some(rest) = file.strip_prefix(&prefix) else {
            continue;
        };
        let h16 = rest.get(..16).unwrap_or("");
        let tail = rest.get(16..).unwrap_or("");
        let is_bundle_file = h16.bytes().all(|b| b.is_ascii_hexdigit())
            && h16.len() == 16
            && (tail == ".json" || tail == ".json.gz");
        if is_bundle_file && h16 != keep {
            let _ = fs::remove_file(ent.path());
        }
    }
}
