//! `HttpState` and `HttpConfig` (Plan/24 §16): everything the HTTP handlers and middleware share.
//!
//! `HttpState` is `Clone` (every field is an `Arc`) and is the axum state of the routes this module owns. The
//! sub-states of other owners (lobby, gateway, admin) are not fields: their routers carry their own state and
//! are mounted with [`super::Mounts`]; `FromRef` impls give handlers of this module the pieces they need.

use super::asset_auth::AssetTokens;
use super::cidr::CidrList;
use super::headers::HeaderPolicy;
use super::live::LiveLists;
use super::ratelimit::{RateConfig, RateLimits};
use super::telemetry::HttpTelemetry;
use super::web::WebSite;
use crate::assets::AssetsState;
use crate::config::Config;
use axum::extract::FromRef;
use flyff_assets::Clock;
use flyff_common::rt::Telemetry;
use flyff_db::PersistHandle;
use std::sync::Arc;
use std::time::Duration;

/// The slice of the configuration the HTTP layer reads (`[web]`, `[net]`, `[assets]`, `debug.dev_surfaces`).
#[derive(Clone, Debug)]
pub struct HttpConfig {
    /// `--dev`: the Vite dev origins pass the Origin check.
    pub dev: bool,
    /// `--dev` or `debug.dev_surfaces`: `/dev/*` is served.
    pub dev_surfaces: bool,
    /// `web.page_title`.
    pub page_title: String,
    /// `web.register_url`.
    pub register_url: String,
    /// `web.public_assets`.
    pub public_assets: bool,
    /// `admin.owner_loopback`: the owner on the host itself passes the asset gate without a cookie (25 §11.7).
    pub owner_loopback: bool,
    /// `web.asset_cookie_ttl_s`.
    pub asset_cookie_ttl_s: u32,
    /// `web.trusted_proxies`.
    pub trusted_proxies: CidrList,
    /// `web.max_tcp_per_ip` (0 = unlimited).
    pub max_tcp_per_ip: u32,
    /// `web.max_tcp_total` (0 = unlimited).
    pub max_tcp_total: u32,
    /// The rate classes.
    pub rate: RateConfig,
    /// `web.hsts_max_age_s`.
    pub hsts_max_age_s: u32,
    /// `web.csp`.
    pub csp: bool,
    /// `assets.interactive_timeout_s`.
    pub interactive_timeout_s: u32,
    /// The listener terminates TLS (built-in TLS, WP-WEB-16).
    pub tls: bool,
    /// `net.ip_block` (the initial list; [`LiveLists`] holds the live one).
    pub ip_block: Vec<String>,
    /// `net.allowed_origins` (the initial list).
    pub allowed_origins: Vec<String>,
}

impl HttpConfig {
    /// Projects the loaded configuration (`dev` is `--dev`).
    pub fn from_config(cfg: &Config, dev: bool) -> HttpConfig {
        let (trusted, _bad) = CidrList::parse_all(&cfg.web.trusted_proxies);
        HttpConfig {
            dev,
            dev_surfaces: dev || cfg.debug.dev_surfaces,
            page_title: cfg.web.page_title.clone(),
            register_url: cfg.web.register_url.clone(),
            public_assets: cfg.web.public_assets,
            owner_loopback: cfg.admin.owner_loopback,
            asset_cookie_ttl_s: cfg.web.asset_cookie_ttl_s,
            trusted_proxies: trusted,
            max_tcp_per_ip: cfg.web.max_tcp_per_ip,
            max_tcp_total: cfg.web.max_tcp_total,
            rate: RateConfig {
                static_per_sec: cfg.web.req_per_ip_per_sec,
                static_burst: cfg.web.req_per_ip_burst,
                api_per_sec: cfg.web.api_per_ip_per_sec,
                api_burst: cfg.web.api_per_ip_burst,
                gen_waits_per_ip: cfg.web.gen_waits_per_ip,
            },
            hsts_max_age_s: cfg.web.hsts_max_age_s,
            csp: cfg.web.csp,
            interactive_timeout_s: cfg.assets.interactive_timeout_s,
            // The built-in TLS acceptor is WP-WEB-16: until `serve` can terminate TLS the listener is plain HTTP and
            // this stays `false` (HSTS and the `Secure` cookie flag follow the real scheme).
            tls: false,
            ip_block: cfg.net.ip_block.clone(),
            allowed_origins: cfg.net.allowed_origins.clone(),
        }
    }

    /// The header policy of §3.4.
    pub fn header_policy(&self) -> HeaderPolicy {
        HeaderPolicy {
            csp: self.csp,
            hsts_max_age_s: self.hsts_max_age_s,
        }
    }

    /// How long a `/gen` request waits for a build before `503`.
    pub fn interactive_timeout(&self) -> Duration {
        Duration::from_secs(u64::from(self.interactive_timeout_s))
    }
}

impl Default for HttpConfig {
    fn default() -> Self {
        HttpConfig::from_config(&Config::default(), false)
    }
}

/// What `/healthz` reads (the walking skeleton's handles; Plan/10 §12.3).
#[derive(Clone)]
pub struct CoreHandles {
    /// Shared counters (tick state, players).
    pub telemetry: Arc<Telemetry>,
    /// The persistence handle (degraded flag).
    pub persist: PersistHandle,
}

/// The shared state of the HTTP layer.
#[derive(Clone)]
pub struct HttpState {
    /// Configuration.
    pub cfg: Arc<HttpConfig>,
    /// The reloadable lists (`net.ip_block`, `net.allowed_origins`).
    pub live: Arc<LiveLists>,
    /// The embedded web client.
    pub site: Arc<WebSite>,
    /// Vfs, game data, `AssetPrep` and the finalised manifests.
    pub assets: Arc<AssetsState>,
    /// Login-gated asset cookies (§13).
    pub tokens: Arc<AssetTokens>,
    /// Per-IP rate classes.
    pub limits: Arc<RateLimits>,
    /// HTTP counters.
    pub telemetry: Arc<HttpTelemetry>,
    /// `/healthz` inputs.
    pub core: CoreHandles,
}

impl HttpState {
    /// Builds the state: the token table, limiter, lists and counters follow `cfg`; `clock` is the monotonic
    /// time source of the limiter and the tokens.
    pub fn new(
        cfg: HttpConfig,
        site: WebSite,
        assets: Arc<AssetsState>,
        core: CoreHandles,
        clock: Arc<dyn Clock>,
    ) -> HttpState {
        let (live, bad) = LiveLists::new(&cfg.ip_block, &cfg.allowed_origins);
        for b in bad {
            tracing::warn!(entry = %b, "net.ip_block: not a CIDR network, ignored");
        }
        HttpState {
            tokens: AssetTokens::new(cfg.asset_cookie_ttl_s, Arc::clone(&clock)),
            limits: RateLimits::new(cfg.rate, clock),
            live: Arc::new(live),
            site: Arc::new(site),
            telemetry: HttpTelemetry::new(),
            cfg: Arc::new(cfg),
            assets,
            core,
        }
    }
}

impl HttpState {
    /// May this request fetch gated assets (`web.public_assets = false`, 24 §13)? With a valid `flyff_assets` cookie,
    /// or (25 §11.7) as the owner on the host itself: the loopback rule of 10 §13.2 is evaluated here too, unless
    /// `admin.owner_loopback` is off.
    pub fn asset_gate_open(&self, parts: &axum::http::request::Parts) -> bool {
        self.tokens.validate_headers(&parts.headers)
            || (self.cfg.owner_loopback
                && parts
                    .extensions
                    .get::<super::ClientInfo>()
                    .is_some_and(|c| crate::admin::auth::trusted_local(&parts.headers, c.peer)))
    }
}

impl FromRef<HttpState> for Arc<AssetsState> {
    fn from_ref(s: &HttpState) -> Self {
        Arc::clone(&s.assets)
    }
}

impl FromRef<HttpState> for Arc<HttpTelemetry> {
    fn from_ref(s: &HttpState) -> Self {
        Arc::clone(&s.telemetry)
    }
}

impl FromRef<HttpState> for Arc<AssetTokens> {
    fn from_ref(s: &HttpState) -> Self {
        Arc::clone(&s.tokens)
    }
}

impl FromRef<HttpState> for Arc<HttpConfig> {
    fn from_ref(s: &HttpState) -> Self {
        Arc::clone(&s.cfg)
    }
}
