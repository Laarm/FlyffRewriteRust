//! Preparation progress for the client (Plan/24 §12.2-§12.3).
//!
//! * R8 `GET /api/prepare`: the [`PrepareProgress`] object (`no-store`; limited by the `api` rate class only).
//! * [`bootstrap_assets`]: the values this document contributes to doc 12's flat `BootstrapResponse`
//!   (`asset_manifest_url`, `bundle_manifest_url`, `gated`, `prepare`, `page_title`, `register_url`).
//!
//! The WebSocket push (`AssetsPreparing`, §12.4, WP-WEB-07) is the gateway's and is not part of this module.

use super::state::HttpState;
use axum::Json;
use axum::extract::State;
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use flyff_assets::PrepareProgress;
use serde::Serialize;

/// The asset-related fields of `/api/bootstrap` (`web.*` and `AssetPrep`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BootstrapAssetFields {
    /// `"/data/asset-manifest.<fp>.json"` (R12).
    pub asset_manifest_url: String,
    /// `"/data/manifest.json"` (R10).
    pub bundle_manifest_url: String,
    /// `!web.public_assets`.
    pub gated: bool,
    /// The current preparation wave.
    pub prepare: PrepareProgress,
    /// `web.page_title`.
    pub page_title: String,
    /// `web.register_url` (`""` = hidden).
    pub register_url: String,
}

/// The values doc 12 copies into its `BootstrapResponse`.
pub fn bootstrap_assets(st: &HttpState) -> BootstrapAssetFields {
    BootstrapAssetFields {
        asset_manifest_url: format!("/data/asset-manifest.{}.json", st.assets.manifest_fp),
        bundle_manifest_url: "/data/manifest.json".to_owned(),
        gated: !st.cfg.public_assets,
        prepare: st.assets.prep.progress(),
        page_title: st.cfg.page_title.clone(),
        register_url: st.cfg.register_url.clone(),
    }
}

/// R8.
pub async fn get(State(st): State<HttpState>) -> Response {
    let mut resp = Json(st.assets.prep.progress()).into_response();
    resp.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp
}
