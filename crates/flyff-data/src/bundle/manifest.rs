//! `manifest.json` and `data_version` (Plan/14 §10.3).
//!
//! `data_version` is `xxh3_64` (16 lower-hex digits) over the **sorted** `name:h16` lines of the bundles,
//! joined by `\n` without a trailing newline. [`data_version_of`] is the single implementation: the exporter
//! uses it for the 20 bundles of this crate and the manifest finalisation of `flyff-assets` (WP-WEB-05) for all
//! 22 once the `motions` and `model-bounds` projections exist ([`BundleManifest::with_external`]).

use super::types::{BundleManifest, ManifestEntry};
use xxhash_rust::xxh3::xxh3_64;

/// `data_version` over `(name, h16)` pairs (any order; they are sorted by name).
pub fn data_version_of<'a>(pairs: impl IntoIterator<Item = (&'a str, &'a str)>) -> String {
    let mut lines: Vec<String> = pairs.into_iter().map(|(n, h)| format!("{n}:{h}")).collect();
    lines.sort();
    format!("{:016x}", xxh3_64(lines.join("\n").as_bytes()))
}

/// The `<h16>` of a manifest entry URL (`/data/<bundle>.<h16>.json`), `None` for another shape.
pub fn h16_of_url(url: &str) -> Option<&str> {
    let file = url.rsplit('/').next()?;
    let stem = file.strip_suffix(".json")?;
    let (_, h) = stem.rsplit_once('.')?;
    (h.len() == 16 && h.bytes().all(|b| b.is_ascii_hexdigit())).then_some(h)
}

impl BundleManifest {
    /// `data_version` recomputed from the entries' URLs (the `h16` of each file name).
    pub fn recompute_data_version(&self) -> String {
        data_version_of(
            self.bundles
                .iter()
                .filter_map(|(n, e)| h16_of_url(&e.url).map(|h| (n.as_str(), h))),
        )
    }

    /// A copy with an extra entry inserted (a bundle produced elsewhere: `motions`, `model-bounds`) and
    /// `data_version` recomputed over all entries (the finalisation of 14 §10.3).
    #[must_use]
    pub fn with_external(&self, name: &str, entry: ManifestEntry) -> Self {
        let mut m = self.clone();
        m.bundles.insert(name.to_owned(), entry);
        m.data_version = m.recompute_data_version();
        m
    }
}
