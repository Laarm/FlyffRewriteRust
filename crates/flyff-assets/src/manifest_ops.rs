//! Content of the client asset manifest (Plan/24 §6.4, §6.5) and of the `/data` bundle lookup table
//! (WP-WEB-04, WP-WEB-05).
//!
//! * [`ClientManifest::assemble`]: the manifest of a started [`AssetPrep`]: `res.groups` (one group fingerprint
//!   per whitelisted directory), `rules` (the §5.2 conversion rows, so the client has no hard-coded copy),
//!   `gen` (the partition fingerprints of every registered served kind), and what the builders published in
//!   [`crate::ManifestParts`] (`dds_fix`, `cursors`, `worlds`). The server calls it once after the start-up kinds
//!   exist (§11.1 step 5f) and hands the result to [`AssetPrep::set_manifest`].
//! * [`asset_url`]: the Rust reference of the client's `assetUrl` (§6.5). `tests/golden/asset-urls.json` holds
//!   its vectors; the TypeScript resolver (`web/src/assets/paths.ts`) must reproduce every one.
//! * [`BundleIndex::from_manifest`]: the `/data/<bundle>.<h16>.json` lookup table of a bundle manifest.

use crate::fp::{self, Fp};
use crate::kinds::{AssetKind, NO_PARTITION};
use crate::manifest::{BundleEntry, BundleIndex, ClientManifest, GenEntry, ManifestRule};
use crate::prep::AssetPrep;
use crate::res::{self, DirPattern, Form};
use flyff_data::bundle::types::{BundleManifest, LoadClass};
use std::collections::BTreeMap;

/// Bundles reachable without login when `web.public_assets = false` (Plan/24 §13): everything the login
/// screen needs.
pub const PUBLIC_BUNDLES: [&str; 6] = [
    "texts",
    "resdata-layouts",
    "ui-atlases",
    "sounds",
    "music",
    "names",
];

/// `true` for kinds whose `/gen` URL carries one fingerprint per partition (`png`: source directory,
/// `collision`: world folder, ...); the other kinds have the single partition [`NO_PARTITION`].
pub const fn is_partitioned(kind: AssetKind) -> bool {
    matches!(
        kind,
        AssetKind::UiPng | AssetKind::DdsFix | AssetKind::CollisionChunk | AssetKind::LandPack
    )
}

/// The conversion rows of the §5.2 table as manifest rules (`Raw` rows and cursors have no rule: they are
/// served from `/res`, or through the dedicated cursor URL).
pub fn conversion_rules() -> Vec<ManifestRule> {
    let mut out = Vec::new();
    for r in res::CLIENT_DIRS {
        let DirPattern::Exact(dir) = r.dir else {
            continue;
        };
        let (form, key) = match r.form {
            Form::PngExact => ("png", Some("exact")),
            Form::PngNone => ("png", Some("none")),
            Form::DdsRaw => ("dds", None),
            Form::Raw | Form::Cursor => continue,
        };
        out.push(ManifestRule {
            dir: dir.to_owned(),
            exts: r.exts.iter().map(|e| (*e).to_owned()).collect(),
            form: form.to_owned(),
            key: key.map(str::to_owned),
        });
    }
    out
}

impl ClientManifest {
    /// Assembles the manifest from the VFS index, the registered builders and the parts they published.
    /// Stat-only: no file is read.
    pub fn assemble(prep: &AssetPrep) -> ClientManifest {
        let vfs = prep.vfs();
        let cfg = prep.config();
        let mut m = ClientManifest::empty(&cfg.server_build);
        for dir in res::client_dirs(vfs) {
            if let Some(fp) = fp::group_fp(vfs, &dir) {
                m.res.groups.insert(dir, fp);
            }
        }
        m.rules = conversion_rules();
        let ctx = prep.fp_ctx();
        for kind in prep.registered_kinds() {
            if !kind.served() {
                continue;
            }
            let Some(builder) = prep.builder(kind) else {
                continue;
            };
            let entry = if is_partitioned(kind) {
                let mut map: BTreeMap<String, Fp> = BTreeMap::new();
                for job in builder.jobs(&ctx) {
                    if !map.contains_key(&job.partition)
                        && let Some(fp) = builder.partition_fp(&ctx, &job.partition)
                    {
                        map.insert(job.partition, fp);
                    }
                }
                GenEntry::Partitioned(map)
            } else {
                match builder.partition_fp(&ctx, NO_PARTITION) {
                    Some(fp) => GenEntry::Single(fp),
                    None => continue,
                }
            };
            m.gen_urls.kinds.insert(kind.slug().to_owned(), entry);
        }
        let parts = prep.manifest_parts();
        m.dds_fix = parts.dds_fix();
        m.cursors = parts.cursors();
        m.worlds = parts.worlds();
        m.gzip.raw = cfg.gzip_raw.as_str().to_owned();
        m
    }
}

/// `encodeURIComponent` of one path segment (unreserved: `A-Z a-z 0-9 - _ . ! ~ * ' ( )`).
pub fn encode_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for b in segment.bytes() {
        if b.is_ascii_alphanumeric()
            || matches!(
                b,
                b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')'
            )
        {
            out.push(char::from(b));
        } else {
            out.push('%');
            out.push(char::from(b"0123456789ABCDEF"[usize::from(b >> 4)]));
            out.push(char::from(b"0123456789ABCDEF"[usize::from(b & 15)]));
        }
    }
    out
}

/// [`encode_segment`] applied to every `/`-separated segment of a normalised path.
pub fn encode_path(path: &str) -> String {
    path.split('/')
        .map(encode_segment)
        .collect::<Vec<_>>()
        .join("/")
}

/// The URL the client fetches `path` (a normalised VFS path) from, or `None` when the path is not
/// client-visible (Plan/24 §6.5, transcribed).
///
/// A `png` rule whose partition is missing from `gen.png` (the kind is not registered) falls back to the raw
/// `/res` URL, which the server allows for every whitelisted file.
pub fn asset_url(m: &ClientManifest, path: &str) -> Option<String> {
    let (dir, file) = path.rsplit_once('/').unwrap_or(("", path));
    let ext = file.rsplit_once('.').map_or("", |(_, e)| e);
    let group = m.res.groups.get(dir)?;
    let rule = m
        .rules
        .iter()
        .find(|r| r.dir == dir && r.exts.iter().any(|e| e == ext));
    let gen_prefix = &m.gen_urls.prefix;
    if let Some(rule) = rule {
        if rule.form == "png"
            && let Some(GenEntry::Partitioned(png)) = m.gen_urls.kinds.get("png")
            && let Some(fp) = png.get(dir)
        {
            return Some(format!("{gen_prefix}/png/{fp}/{}.png", encode_path(path)));
        }
        if rule.form == "dds"
            && m.dds_fix.get(dir).is_some_and(|set| set.contains(file))
            && let Some(GenEntry::Partitioned(dds)) = m.gen_urls.kinds.get("dds")
            && let Some(fp) = dds.get(dir)
        {
            return Some(format!("{gen_prefix}/dds/{fp}/{}", encode_path(path)));
        }
    }
    Some(format!("{}/{group}/{}", m.res.prefix, encode_path(path)))
}

impl BundleIndex {
    /// The lookup table of a bundle manifest. Entries whose URL is not `/data/<bundle>.<h16>.json` are skipped.
    pub fn from_manifest(m: &BundleManifest) -> BundleIndex {
        let mut bundles = BTreeMap::new();
        for (name, e) in &m.bundles {
            let Some(h16) = h16_of_url(&e.url) else {
                continue;
            };
            bundles.insert(
                name.clone(),
                BundleEntry {
                    h16: h16.to_owned(),
                    file: format!("{name}.{h16}.json"),
                    lazy: e.load == LoadClass::Lazy,
                    public: PUBLIC_BUNDLES.contains(&name.as_str()),
                    bytes: e.bytes,
                    gz_bytes: e.gz_bytes,
                },
            );
        }
        BundleIndex {
            data_version: m.data_version.clone(),
            bundles,
        }
    }

    /// The entry of bundle `name`.
    pub fn get(&self, name: &str) -> Option<&BundleEntry> {
        self.bundles.get(name)
    }
}

/// The `<h16>` of `/data/<bundle>.<h16>.json`.
fn h16_of_url(url: &str) -> Option<&str> {
    let file = url.rsplit('/').next()?;
    let stem = file.strip_suffix(".json")?;
    let (_, h) = stem.rsplit_once('.')?;
    (h.len() == 16 && h.bytes().all(|b| b.is_ascii_hexdigit())).then_some(h)
}
