//! The exporter core: envelope encoding, content hashes, gzip, the integer-range assertion and the
//! [`BundleSet`] that [`export_all`] returns (Plan/14 §10.2-10.3).

use super::catalog::catalog;
use super::manifest::data_version_of;
use super::project::{PathResolver, Sources};
use super::types::{BUNDLE_FORMAT, BundleData, BundleManifest, Envelope, LoadClass, ManifestEntry};
use crate::client_data::ClientData;
use crate::game_data::GameData;
use flate2::{Compression, GzBuilder};
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::Write;
use xxhash_rust::xxh3::xxh3_64;

/// Why an export failed.
#[derive(Debug, thiserror::Error)]
pub enum BundleError {
    /// A value could not be serialised.
    #[error("bundle {bundle}: {message}")]
    Serialize {
        /// The bundle.
        bundle: String,
        /// The serde error.
        message: String,
    },
    /// An integer is outside the range that survives JavaScript numbers (±2^53, 14 §10.1).
    #[error("bundle {bundle}: integer {value} is outside ±2^53")]
    UnsafeInteger {
        /// The bundle.
        bundle: String,
        /// The offending digits.
        value: String,
    },
    /// A file operation of the store failed.
    #[error("bundle store {path}: {message}")]
    Io {
        /// The path.
        path: String,
        /// The I/O error.
        message: String,
    },
    /// A requested bundle does not exist (or is external).
    #[error("unknown or external bundle {0:?}")]
    Unknown(String),
}

/// The largest integer a JSON consumer reads exactly (`Number.MAX_SAFE_INTEGER` + 1).
const SAFE_LIMIT: u128 = 1 << 53;

/// Asserts that every integer literal of `json` is within ±2^53 (14 §10.1). Strings are skipped; a number with a
/// fraction or an exponent is not an integer literal.
pub fn check_safe_integers(bundle: &str, json: &[u8]) -> Result<(), BundleError> {
    let mut i = 0;
    let n = json.len();
    let mut in_string = false;
    while i < n {
        let c = json.get(i).copied().unwrap_or(0);
        if in_string {
            match c {
                b'\\' => i += 1,
                b'"' => in_string = false,
                _ => {}
            }
            i += 1;
            continue;
        }
        match c {
            b'"' => {
                in_string = true;
                i += 1;
            }
            b'-' | b'0'..=b'9' => {
                let start = i;
                i += 1;
                let mut integer = true;
                while let Some(&d) = json.get(i) {
                    match d {
                        b'0'..=b'9' => i += 1,
                        b'.' | b'e' | b'E' | b'+' | b'-' => {
                            integer = false;
                            i += 1;
                        }
                        _ => break,
                    }
                }
                let text = json.get(start..i).unwrap_or(&[]);
                let digits = text.iter().filter(|b| b.is_ascii_digit()).count();
                if integer && digits >= 16 {
                    let s = std::str::from_utf8(text).unwrap_or("0");
                    let v: u128 = s.trim_start_matches('-').parse().unwrap_or(u128::MAX);
                    if v > SAFE_LIMIT {
                        return Err(BundleError::UnsafeInteger {
                            bundle: bundle.to_owned(),
                            value: s.to_owned(),
                        });
                    }
                }
            }
            _ => i += 1,
        }
    }
    Ok(())
}

/// `xxh3_64` of `bytes` as 16 lower-hex digits (the `<h16>` of the file names).
pub fn h16(bytes: &[u8]) -> String {
    format!("{:016x}", xxh3_64(bytes))
}

/// gzip level 9 with a fixed header (no mtime, no file name, unknown OS), so equal input gives equal bytes.
pub fn gzip9(bytes: &[u8]) -> Vec<u8> {
    let mut enc = GzBuilder::new()
        .mtime(0)
        .operating_system(255)
        .write(Vec::with_capacity(bytes.len() / 4), Compression::best());
    // writing into a `Vec` cannot fail
    let _ = enc.write_all(bytes);
    enc.finish().unwrap_or_default()
}

/// The compact JSON of the envelope `{bundle, schema, data}` of `data`.
pub fn encode_envelope<T: BundleData + Serialize>(data: &T) -> Result<Vec<u8>, BundleError> {
    let env = Envelope {
        bundle: T::NAME.to_owned(),
        schema: T::SCHEMA,
        data,
    };
    let json = serde_json::to_vec(&env).map_err(|e| BundleError::Serialize {
        bundle: T::NAME.to_owned(),
        message: e.to_string(),
    })?;
    check_safe_integers(T::NAME, &json)?;
    Ok(json)
}

/// One exported bundle file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bundle {
    /// The bundle name.
    pub name: String,
    /// The schema version.
    pub schema: u32,
    /// Boot or lazy.
    pub load: LoadClass,
    /// The uncompressed JSON (the envelope).
    pub json: Vec<u8>,
    /// The gzip (level 9) of `json`.
    pub gz: Vec<u8>,
    /// `xxh3_64` of `json` as 16 hex digits.
    pub h16: String,
}

impl Bundle {
    /// Builds a bundle from its JSON.
    pub fn from_json(name: &str, schema: u32, load: LoadClass, json: Vec<u8>) -> Self {
        let gz = gzip9(&json);
        let h = h16(&json);
        Self {
            name: name.to_owned(),
            schema,
            load,
            json,
            gz,
            h16: h,
        }
    }

    /// `<bundle>.<h16>.json`, the name of the uncompressed file (the `.gz` file appends `.gz`).
    pub fn file_name(&self) -> String {
        format!("{}.{}.json", self.name, self.h16)
    }

    /// `/data/<bundle>.<h16>.json`, the URL of the manifest entry.
    pub fn url(&self) -> String {
        format!("/data/{}", self.file_name())
    }

    /// The manifest entry of this bundle.
    pub fn entry(&self) -> ManifestEntry {
        ManifestEntry {
            url: self.url(),
            schema: self.schema,
            bytes: self.json.len() as u64,
            gz_bytes: self.gz.len() as u64,
            load: self.load,
        }
    }
}

/// The result of [`export_all`]: the bundles and the manifest of them.
#[derive(Clone, Debug)]
pub struct BundleSet {
    /// The bundles in catalogue order.
    pub bundles: Vec<Bundle>,
    /// `manifest.json` over the bundles (without the two external projections, 14 §10.3).
    pub manifest: BundleManifest,
}

impl BundleSet {
    /// The bundle called `name`.
    pub fn get(&self, name: &str) -> Option<&Bundle> {
        self.bundles.iter().find(|b| b.name == name)
    }

    /// The manifest of the bundles as compact JSON.
    pub fn manifest_json(&self) -> Result<Vec<u8>, BundleError> {
        serde_json::to_vec(&self.manifest).map_err(|e| BundleError::Serialize {
            bundle: "manifest".to_owned(),
            message: e.to_string(),
        })
    }

    /// The summed size of the boot-class bundles: `(raw, gzip)`.
    pub fn boot_bytes(&self) -> (u64, u64) {
        self.bundles
            .iter()
            .filter(|b| b.load == LoadClass::Boot)
            .fold((0, 0), |(r, g), b| {
                (r + b.json.len() as u64, g + b.gz.len() as u64)
            })
    }
}

/// Options of [`export_all`].
#[derive(Clone, Debug)]
pub struct ExportOptions {
    /// `flyff <version> (<commit>)`, written into the manifest.
    pub server_build: String,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            server_build: format!("flyff {}", env!("CARGO_PKG_VERSION")),
        }
    }
}

/// Exports the 20 bundles of this crate (14 §10.4) and the manifest over them. Deterministic: the same
/// `GameData`, `ClientData` and file set give byte-identical output.
///
/// `files` answers which canonical VFS paths exist (the `null` rules of the `sounds`, `music`, `models` and
/// `sfx-table` bundles, the resolved atlas textures); use the `Vfs` (feature `load`) or a set of paths.
pub fn export_all(
    game: &GameData,
    client: &ClientData,
    files: &dyn PathResolver,
    opts: &ExportOptions,
) -> Result<BundleSet, BundleError> {
    let src = Sources {
        game,
        client,
        files,
    };
    let mut bundles = Vec::new();
    for spec in catalog() {
        let Some(project) = spec.project else {
            continue;
        };
        let json = project(&src)?;
        bundles.push(Bundle::from_json(spec.name, spec.schema, spec.load, json));
    }
    let entries: BTreeMap<String, ManifestEntry> = bundles
        .iter()
        .map(|b| (b.name.clone(), b.entry()))
        .collect();
    let data_version = data_version_of(bundles.iter().map(|b| (b.name.as_str(), b.h16.as_str())));
    let manifest = BundleManifest {
        format: BUNDLE_FORMAT,
        data_version,
        server_build: opts.server_build.clone(),
        bundles: entries,
    };
    Ok(BundleSet { bundles, manifest })
}
