//! `GET /api/bootstrap` (Plan/12 §6.2; WP-AUTH-04; contract C-45, frozen with this WP): what the browser
//! learns before it logs in. The shape is **flat**; the asset-side values (`asset_manifest_url`,
//! `bundle_manifest_url`, `gated`, `prepare`, `page_title`, `auth.register_url`) come from doc 24's
//! `http::prepare::bootstrap_assets`. Later changes of the shape need an ADR (`docs/contracts.toml`,
//! `bootstrap-api`).

use super::LobbyCore;
use super::accounts::{ACCOUNT_NAME_MAX, ACCOUNT_NAME_MIN, PASSWORD_MAX, PASSWORD_MIN};
use crate::build_info;
use crate::http::HttpState;
use crate::http::prepare::bootstrap_assets;
use flyff_common::config::ClientFeatureConfig;
use serde::Serialize;

/// The server's availability as the login screen shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerStatus {
    /// Logins are accepted.
    Open,
    /// `auth.maintenance`, or the persistence layer is degraded (logins answer 109 either way).
    Maintenance,
    /// A graceful shutdown started.
    ShuttingDown,
    /// More avatars than `auth.max_in_world`: `Hello` answers `SERVER_FULL`.
    Full,
}

/// `GET /api/bootstrap` (12 §6.2).
#[derive(Clone, Debug, Serialize)]
pub struct BootstrapResponse {
    /// `server.name`.
    pub server_name: String,
    /// `BUILD_ID` of the executable.
    pub server_build: String,
    /// The embedded web build id; the client compares it with `__FLYFF_BUILD_ID__`.
    pub build_id: String,
    /// `flyff_proto::PROTO_HASH` (16 hex).
    pub proto_hash: String,
    /// The client data-bundle version (14 §10.3).
    pub data_version: String,
    /// `"/data/manifest.json"`.
    pub bundle_manifest_url: String,
    /// `"/data/asset-manifest.<fp>.json"`.
    pub asset_manifest_url: String,
    /// `web.page_title`.
    pub page_title: String,
    /// The `[features]`, `[gameplay]` and `[fidelity]` keys the client uses (03 §4.1).
    pub features: ClientFeatureConfig,
    /// Login and registration parameters.
    pub auth: BootstrapAuth,
    /// Availability.
    pub status: ServerStatus,
    /// `!web.public_assets` (24 §13): load gated bundles and assets only after login.
    pub gated: bool,
    /// The asset preparation wave (24 §12.2).
    pub prepare: flyff_assets::PrepareProgress,
    /// Server wall clock, milliseconds.
    pub server_time_ms: i64,
    /// The effective IANA zone of `server.timezone` (`"local"` resolved).
    pub server_timezone: String,
}

/// The `auth` object of [`BootstrapResponse`].
#[derive(Clone, Debug, Serialize)]
pub struct BootstrapAuth {
    /// `auth.legacy_md5_salt`.
    pub md5_salt: String,
    /// `auth.allow_registration`.
    pub allow_registration: bool,
    /// `web.register_url`.
    pub register_url: String,
    /// 3.
    pub account_min_len: u8,
    /// 16.
    pub account_max_len: u8,
    /// 3.
    pub password_min_len: u8,
    /// 15.
    pub password_max_len: u8,
    /// `auth.delete_requires_password`.
    pub delete_requires_password: bool,
    /// `auth.token_grace_s`.
    pub token_grace_s: u32,
}

impl LobbyCore {
    /// The status of the login screen: shutting down, then maintenance (also a degraded database), then full,
    /// else open.
    pub fn server_status(&self) -> ServerStatus {
        let s = self.settings();
        match self.availability() {
            super::Availability::ShuttingDown => return ServerStatus::ShuttingDown,
            super::Availability::Degraded => return ServerStatus::Maintenance,
            super::Availability::Open => {}
        }
        if s.auth.maintenance {
            ServerStatus::Maintenance
        } else if self.registry.in_world() > s.auth.max_in_world {
            ServerStatus::Full
        } else {
            ServerStatus::Open
        }
    }
}

fn narrow(v: usize) -> u8 {
    u8::try_from(v).unwrap_or(u8::MAX)
}

/// Assembles the response from the lobby settings and the asset side.
pub fn build(core: &LobbyCore, http: &HttpState) -> BootstrapResponse {
    let s = core.settings();
    let assets = bootstrap_assets(http);
    BootstrapResponse {
        server_name: s.server_name.clone(),
        server_build: build_info::BUILD_ID.to_owned(),
        build_id: http.site.web_build_id(),
        proto_hash: flyff_proto::PROTO_HASH.to_owned(),
        data_version: http.assets.bundles.data_version.clone(),
        bundle_manifest_url: assets.bundle_manifest_url,
        asset_manifest_url: assets.asset_manifest_url,
        page_title: assets.page_title,
        features: s.client_features.clone(),
        auth: BootstrapAuth {
            md5_salt: s.auth.legacy_md5_salt.clone(),
            allow_registration: s.auth.allow_registration,
            register_url: assets.register_url,
            account_min_len: narrow(ACCOUNT_NAME_MIN),
            account_max_len: narrow(ACCOUNT_NAME_MAX),
            password_min_len: narrow(PASSWORD_MIN),
            password_max_len: narrow(PASSWORD_MAX),
            delete_requires_password: s.auth.delete_requires_password,
            token_grace_s: s.auth.token_grace_s,
        },
        status: core.server_status(),
        gated: assets.gated,
        prepare: assets.prepare,
        server_time_ms: core.clock.unix_ms(),
        server_timezone: s.server_timezone.clone(),
    }
}
