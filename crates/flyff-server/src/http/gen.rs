//! `GET /gen/<kind>/<fp>/<key>`: derived artifacts built lazily by `flyff-assets` (Plan/24 §9, WP-WEB-06).
//!
//! Handler algorithm of §9.1:
//!
//! 1. the kind must be a served slug and registered at this milestone (a kind without a builder answers `404
//!    asset kind not available`);
//! 2. the key is percent-decoded (canonical, §5.1) and `AssetBuilder::route` validates the grammar (`400`) and the
//!    source (`404`);
//! 3. the asset gate (§13);
//! 4. `stale = fp != partition fingerprint`: the current artifact is served with `Cache-Control: no-store`;
//! 5. a cache hit is served at once; a miss takes the per-IP `gen_wait` permit (`429 Retry-After: 2` beyond
//!    `web.gen_waits_per_ip`), asks `AssetPrep::get(.., Interactive)` and waits at most
//!    `assets.interactive_timeout_s` (`503 Retry-After: 2`; the job keeps running and the next request after it
//!    finished is a hit); a build error is `500` with `X-Flyff-Asset-Error`;
//! 6. `ETag: "g-<partition fp>"` (`-gz` for the stored gzip form), `Vary: Accept-Encoding` for gzip artifacts,
//!    the content type of the builder, no Range.

use super::asset_auth::{self, GateTarget};
use super::client_ip::ClientInfo;
use super::error;
use super::files::{self, ByteSource, CachePolicy, Servable};
use super::pathcodec::{decode_canonical, parse_fp};
use super::state::HttpState;
use super::telemetry::RateClass;
use crate::assets::served_kind;
use axum::extract::{Request, State};
use axum::response::Response;
use flyff_assets::{AssetError, CachedFile, Fp, Priority, StoredEncoding};
use std::time::Instant;

/// The URL prefix.
pub const PREFIX: &str = "/gen/";

/// A parsed `/gen` request path (before the builder looks at the key).
#[derive(Debug, PartialEq, Eq)]
pub struct GenRequest<'a> {
    /// The kind slug.
    pub slug: &'a str,
    /// The fingerprint segment.
    pub fp: Option<Fp>,
    /// The decoded key (`None` = not decodable).
    pub key: Option<String>,
}

/// Splits `/gen/<kind>/<fp>/<key...>`.
pub fn parse(uri_path: &str) -> Option<GenRequest<'_>> {
    let rest = uri_path.strip_prefix(PREFIX)?;
    let (slug, rest) = rest.split_once('/')?;
    let (fp, key) = rest.split_once('/')?;
    Some(GenRequest {
        slug,
        fp: parse_fp(fp),
        key: decode_canonical(key),
    })
}

/// R14: the handler.
pub async fn get(State(st): State<HttpState>, req: Request) -> Response {
    let (parts, _) = req.into_parts();
    let Some(gr) = parse(parts.uri.path()) else {
        return error::not_found();
    };
    let Some(kind) = served_kind(gr.slug) else {
        return error::plain(axum::http::StatusCode::NOT_FOUND, "no such asset kind");
    };
    let prep = &st.assets.prep;
    let Some(builder) = prep.builder(kind) else {
        return error::asset_error(&AssetError::disabled(format!("{kind} is not available")));
    };
    let (Some(fp), Some(key)) = (gr.fp, gr.key) else {
        return error::bad_path();
    };
    // 2. grammar and source
    let route = match builder.route(&prep.fp_ctx(), &key) {
        Ok(r) => r,
        Err(e) => return error::asset_error(&e),
    };
    // 3. the gate
    if !st.cfg.public_assets
        && !st.asset_gate_open(&parts)
        && !asset_auth::is_public(&st.assets.public_files, GateTarget::Gen { kind, key: &key })
    {
        return error::login_required();
    }
    // 4. stale fingerprint
    let Some(current) = prep.partition_fp(kind, &route.partition) else {
        return error::not_found();
    };
    let stale = current != fp;
    // 5. hit or miss
    match prep.lookup(kind, &key) {
        Ok(Some(cf)) => return serve(&parts, &cf, stale).await,
        Ok(None) => {}
        Err(e) => return error::asset_error(&e),
    }
    let ip = parts
        .extensions
        .get::<ClientInfo>()
        .map_or(std::net::IpAddr::from([0, 0, 0, 0]), |c| c.ip);
    let Some(_permit) = st.limits.acquire_gen_wait(ip) else {
        st.telemetry.rate_limited(RateClass::GenWait);
        return error::rate_limited(2);
    };
    let started = Instant::now();
    let rx = prep.get(kind, &key, Priority::Interactive);
    let waited = tokio::time::timeout(st.cfg.interactive_timeout(), rx.recv_async()).await;
    st.telemetry
        .gen_wait
        .observe_ns(u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX));
    match waited {
        Err(_elapsed) => error::unavailable("preparing, retry", 2),
        Ok(Err(_)) => error::asset_error(&AssetError::shutting_down()),
        Ok(Ok(Err(e))) => {
            tracing::warn!(%kind, key = %key, error = %e, "asset build failed");
            error::asset_error(&e)
        }
        Ok(Ok(Ok(cf))) => serve(&parts, &cf, stale).await,
    }
}

async fn serve(parts: &axum::http::request::Parts, cf: &CachedFile, stale: bool) -> Response {
    let source = match &cf.mem {
        Some(b) => ByteSource::Owned(b.clone()),
        None => ByteSource::File(cf.path.clone()),
    };
    let s = Servable::new(source, cf.len, cf.content_type)
        .with_encoding(cf.encoding)
        .with_etag(format!("g-{}", cf.fp))
        .with_cache(if stale {
            CachePolicy::NoStore
        } else {
            CachePolicy::Immutable
        })
        .varying(cf.encoding == StoredEncoding::Gzip);
    files::respond(parts, s).await
}
