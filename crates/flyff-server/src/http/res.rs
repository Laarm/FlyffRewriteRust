//! `GET /res/<fp>/<path>`: raw `Resource/` (+ `overrides/`) files under the client-visible whitelist
//! (Plan/24 §5, WP-WEB-04).
//!
//! The whitelist ([`CLIENT_DIRS`]) is data owned by `flyff-assets` (`group_fp` and the gzip eligibility need it
//! too); it is re-exported here. Handler algorithm of §5.3:
//!
//! 1. percent-decode the path and require it canonical (§5.1) → `400 bad path`;
//! 2. the directory must match a whitelist row for the file's extension → `404`;
//! 3. the file must exist in the VFS (overrides first) → `404`;
//! 4. the asset gate (§13) → `401 login required`;
//! 5. `stale = fp != current group fingerprint of the directory`: the current content is served with
//!    `Cache-Control: no-store`;
//! 6. representation: a `Range` request is identity; otherwise the cached gzip variant when it exists and the
//!    client accepts gzip; otherwise identity, and a miss queues the variant (`request_gz`, never awaited);
//! 7. `ETag: "r-<file fp>"` (`-gz` for the variant), `Vary: Accept-Encoding` for gzip-eligible files;
//! 8. `respond` with `immutable` (or `no-store` when stale) and Range support.

use super::asset_auth::{self, GateTarget};
use super::error;
use super::files::{self, ByteSource, CachePolicy, Servable};
use super::mime;
use super::pathcodec::{decode_canonical, parse_fp};
use super::state::HttpState;
use axum::extract::{Request, State};
use axum::http::header;
use axum::response::Response;
use flyff_assets::build::gz::{self, Representation, accepts_gzip};
use flyff_assets::fp::file_fp;
use flyff_assets::{GzState, StoredEncoding};

pub use flyff_assets::res::{
    CLIENT_DIRS, DirRule, Form, dir_of, ext_of, is_client_visible, rule_for,
};

/// The URL prefix.
pub const PREFIX: &str = "/res/";

/// The result of looking at a `/res` request path (before any I/O).
#[derive(Debug, PartialEq, Eq)]
pub struct ResRequest {
    /// The fingerprint segment of the URL.
    pub fp: flyff_assets::Fp,
    /// The canonical VFS path.
    pub path: String,
}

/// Parses `/res/<fp>/<path>`; `None` = `400 bad path`.
pub fn parse(uri_path: &str) -> Option<ResRequest> {
    let rest = uri_path.strip_prefix(PREFIX)?;
    let (fp, path) = rest.split_once('/')?;
    Some(ResRequest {
        fp: parse_fp(fp)?,
        path: decode_canonical(path)?,
    })
}

/// `true` when the URL has the shape `/res/<fp>/<something>`; anything shorter is no route at all (`404`),
/// while a malformed fingerprint or path of that shape is `400 bad path`.
fn has_route_shape(uri_path: &str) -> bool {
    uri_path
        .strip_prefix(PREFIX)
        .and_then(|rest| rest.split_once('/'))
        .is_some_and(|(_, path)| !path.is_empty())
}

/// R13: the handler.
pub async fn get(State(st): State<HttpState>, req: Request) -> Response {
    let (parts, _) = req.into_parts();
    let Some(rr) = parse(parts.uri.path()) else {
        return if has_route_shape(parts.uri.path()) {
            error::bad_path()
        } else {
            error::not_found()
        };
    };
    // 2. whitelist
    let dir = dir_of(&rr.path);
    let Some(ext) = ext_of(&rr.path) else {
        return error::not_found();
    };
    if rule_for(dir, ext).is_none() {
        return error::not_found();
    }
    // 3. the file
    let assets = &st.assets;
    let Some(entry) = assets.vfs.resolve_key(&rr.path) else {
        return error::not_found();
    };
    // 4. the gate
    if !st.cfg.public_assets
        && !st.asset_gate_open(&parts)
        && !asset_auth::is_public(&assets.public_files, GateTarget::Res(&rr.path))
    {
        return error::login_required();
    }
    // 5. stale fingerprint
    let stale = !assets.group_is_current(dir, rr.fp);
    // 6. representation
    let ae = files::accept_encoding(&parts.headers);
    let has_range = parts.headers.contains_key(header::RANGE);
    let state = assets.prep.res_gz(entry);
    let eligible = gz::is_eligible(&rr.path, entry.size);
    let tag = format!("r-{}", file_fp(entry));
    let cache = if stale {
        CachePolicy::NoStore
    } else {
        CachePolicy::Immutable
    };
    let ct = mime::by_path(&rr.path);
    let servable = match gz::choose_representation(&state, ae, has_range) {
        Representation::Gzip => match &state {
            GzState::Present(cf) => {
                let source = match &cf.mem {
                    Some(b) => ByteSource::Owned(b.clone()),
                    None => ByteSource::File(cf.path.clone()),
                };
                Servable::new(source, cf.len, ct).with_encoding(StoredEncoding::Gzip)
            }
            _ => raw(entry, ct),
        },
        Representation::Identity => {
            if eligible && !has_range && accepts_gzip(ae) && matches!(state, GzState::Unknown) {
                assets.prep.request_gz(entry);
            }
            raw(entry, ct)
        }
    }
    .with_etag(tag)
    .with_cache(cache)
    .varying(eligible)
    .with_ranges(true);
    files::respond(&parts, servable).await
}

fn raw(entry: &flyff_vfs::VfsEntry, content_type: &'static str) -> Servable {
    Servable::new(
        ByteSource::File(entry.real.clone()),
        entry.size,
        content_type,
    )
}
