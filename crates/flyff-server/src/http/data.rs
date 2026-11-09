//! `/data/*`: the client data bundles and the two manifests (Plan/24 §2 R10-R12, WP-WEB-05).
//!
//! | route | content | cache |
//! |---|---|---|
//! | R10 `/data/manifest.json` | the finalised bundle manifest (14 §10.3) | `no-cache`, `ETag: "<data_version>"` |
//! | R11 `/data/<bundle>.<h16>.json` | the bundle file from `cache/data/`, gzip when accepted | `immutable`; a stale hash answers the current bytes with `no-store`; an unknown bundle is `404` |
//! | R12 `/data/asset-manifest.<fp>.json` | the `ClientManifest`, from memory | `immutable` (stale fingerprint: `no-store`) |
//!
//! The manifests and the lookup table are built once by [`crate::assets::AssetsState::new`] (the manifest
//! finalisation of §11.1 step 5e-f). Bundles flagged non-public need the asset cookie while
//! `web.public_assets = false`.

use super::asset_auth::{self, GateTarget};
use super::error;
use super::files::{self, ByteSource, CachePolicy, Servable};
use super::mime;
use super::pathcodec::parse_fp;
use super::state::HttpState;
use crate::assets::StoredJson;
use axum::extract::{Request, State};
use axum::http::request::Parts;
use axum::response::Response;
use flyff_assets::StoredEncoding;
use flyff_assets::build::gz::accepts_gzip;

/// The URL prefix.
pub const PREFIX: &str = "/data/";

/// What a `/data/...` path names.
#[derive(Debug, PartialEq, Eq)]
pub enum DataPath<'a> {
    /// `manifest.json`.
    Manifest,
    /// `asset-manifest.<fp>.json`; `None` when the fingerprint is malformed.
    AssetManifest(Option<flyff_assets::Fp>),
    /// `<bundle>.<h16>.json`.
    Bundle {
        /// The bundle name.
        name: &'a str,
        /// The hash segment (lower hex, 16 digits).
        h16: &'a str,
    },
}

/// Classifies the part of the path after `/data/`; `None` = `404`.
pub fn classify(rest: &str) -> Option<DataPath<'_>> {
    if rest == "manifest.json" {
        return Some(DataPath::Manifest);
    }
    let stem = rest.strip_suffix(".json")?;
    if let Some(fp) = stem.strip_prefix("asset-manifest.") {
        return Some(DataPath::AssetManifest(parse_fp(fp)));
    }
    let (name, h16) = stem.rsplit_once('.')?;
    let name_ok = !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_');
    let h_ok = h16.len() == 16 && h16.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    (name_ok && h_ok).then_some(DataPath::Bundle { name, h16 })
}

/// A JSON document held in memory, in the encoding the client accepts.
fn memory_servable(doc: &StoredJson, parts: &Parts, etag: String, cache: CachePolicy) -> Servable {
    let ct = mime::by_ext("json");
    let (bytes, enc) = match &doc.gz {
        Some(gz) if accepts_gzip(files::accept_encoding(&parts.headers)) => {
            (gz.clone(), StoredEncoding::Gzip)
        }
        _ => (doc.identity.clone(), StoredEncoding::Identity),
    };
    let len = bytes.len() as u64;
    Servable::new(ByteSource::Owned(bytes), len, ct)
        .with_encoding(enc)
        .with_etag(etag)
        .with_cache(cache)
        .varying(doc.gz.is_some())
}

/// R10-R12.
pub async fn get(State(st): State<HttpState>, req: Request) -> Response {
    let (parts, _) = req.into_parts();
    let Some(rest) = parts.uri.path().strip_prefix(PREFIX) else {
        return error::not_found();
    };
    let assets = &st.assets;
    match classify(rest) {
        Some(DataPath::Manifest) => {
            let s = memory_servable(
                &assets.bundle_manifest_json,
                &parts,
                assets.bundles.data_version.clone(),
                CachePolicy::NoCache,
            );
            files::respond(&parts, s).await
        }
        Some(DataPath::AssetManifest(fp)) => {
            let Some(fp) = fp else {
                return error::bad_path();
            };
            let cache = if fp == assets.manifest_fp {
                CachePolicy::Immutable
            } else {
                CachePolicy::NoStore
            };
            let s = memory_servable(
                &assets.manifest_json,
                &parts,
                format!("m-{}", assets.manifest_fp),
                cache,
            );
            files::respond(&parts, s).await
        }
        Some(DataPath::Bundle { name, h16 }) => {
            let Some(entry) = assets.bundles.get(name) else {
                return error::not_found();
            };
            if !st.cfg.public_assets
                && !st.asset_gate_open(&parts)
                && !asset_auth::is_public(
                    &assets.public_files,
                    GateTarget::Data {
                        public: entry.public,
                    },
                )
            {
                return error::login_required();
            }
            let cache = if h16 == entry.h16 {
                CachePolicy::Immutable
            } else {
                CachePolicy::NoStore
            };
            let json = assets.data_dir.join(&entry.file);
            let gz_path = assets.data_dir.join(format!("{}.gz", entry.file));
            let use_gz = accepts_gzip(files::accept_encoding(&parts.headers))
                && tokio::fs::metadata(&gz_path).await.is_ok();
            let has_gz = entry.gz_bytes > 0;
            let ct = mime::by_ext("json");
            let s = if use_gz {
                Servable::new(ByteSource::File(gz_path), entry.gz_bytes, ct)
                    .with_encoding(StoredEncoding::Gzip)
            } else {
                Servable::new(ByteSource::File(json), entry.bytes, ct)
            }
            .with_etag(format!("b-{}", entry.h16))
            .with_cache(cache)
            .varying(has_gz);
            files::respond(&parts, s).await
        }
        None => error::not_found(),
    }
}
