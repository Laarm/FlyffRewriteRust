//! The embedded web client routes (Plan/24 §2 R1-R4, §4; WP-WEB-03).
//!
//! | route | content | cache |
//! |---|---|---|
//! | R1 `/`, `/index.html` | `index.html` with `%FLYFF_TITLE%` substituted | `no-cache`, `ETag: "w-<build16>"` |
//! | R2 `/assets/*` | Vite output; the `.gz` sibling when accepted | `immutable` |
//! | R3 `/<file>` | a regular file in the root of the web dist (`favicon.ico`, `build-id.txt`, `robots.txt`) | `no-cache` |
//! | R4 `/dev/{ui,models,world,sim}[/...]` | `dev.html`, only with `--dev` or `debug.dev_surfaces` | `no-cache` |
//! | R17 `/admin`, `/admin/` | `admin.html` (the admin console, Plan/25 §11.2), served by `admin::routes` through [`WebSite::admin_servable`] | `no-store` |
//!
//! There is no SPA fallback (RT1): the client is one page, every other path is `404 not found`. The title
//! substitution (HTML-escaped `web.page_title`) is done once at construction for the embedded and placeholder
//! sources, both for the identity and the gzip form; a `--web-dir` source is read per request (mode B of §4.4:
//! the dist may be rebuilt while the server runs) with `ETag: "d-<xxh3(len ‖ mtime_ns)>"`.

use super::error;
use super::files::{self, ByteSource, CachePolicy, Servable};
use super::mime;
use super::state::HttpState;
use crate::build_info::BUILD_ID;
use crate::web_dist::{WebDistSource, WebFile};
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::response::Response;
use bytes::Bytes;
use flyff_assets::StoredEncoding;
use flyff_assets::build::gz::accepts_gzip;
use flyff_assets::cache::{gzip_beneficial, gzip_bytes};
use std::collections::BTreeMap;
use xxhash_rust::xxh3::{Xxh3, xxh3_64};

/// The literal placeholder `web/index.html` keeps for the configured page title.
pub const TITLE_PLACEHOLDER: &str = "%FLYFF_TITLE%";

/// Top-level names that are never root files (RT4).
const RESERVED: [&str; 10] = [
    "api", "assets", "data", "res", "gen", "dev", "ws", "healthz", "metrics", "admin",
];

/// Pages of the dist that have their own route and are never served as root files (Plan/24 R17, Plan/25 §11.2: `GET
/// /admin.html` and `GET /dev.html` are `404`; the console is `/admin`, the dev surfaces `/dev/*` behind their gate).
const OWN_ROUTE_PAGES: [&str; 2] = ["admin.html", "dev.html"];

/// `true` for [`OWN_ROUTE_PAGES`] and for their precompressed siblings (`admin.html.gz`: the dist ships a `.gz` next to every page of 1 KiB or
/// more, and the root-file route would serve it as `application/octet-stream`; found by the security suite of WP-ADMIN-33).
fn is_own_route_page(name: &str) -> bool {
    OWN_ROUTE_PAGES.contains(&name.strip_suffix(".gz").unwrap_or(name))
}

/// The `/dev/*` surfaces (R4).
const DEV_SURFACES: [&str; 4] = ["ui", "models", "world", "sim"];

/// `robots.txt` when the dist has none (RT4).
const DEFAULT_ROBOTS: &str = "User-agent: *\nDisallow: /\n";

/// A page precomputed at start-up: the substituted HTML in both encodings.
#[derive(Clone, Debug)]
struct Page {
    identity: Bytes,
    gz: Option<Bytes>,
    etag: String,
}

/// The web client as the HTTP layer sees it.
#[derive(Debug)]
pub struct WebSite {
    source: WebDistSource,
    /// In-memory dist (tests, `WebSite::in_memory`): consulted instead of `source`.
    files: Option<BTreeMap<String, WebFile>>,
    page_title: String,
    build16: String,
    dev_surfaces: bool,
    pages: BTreeMap<&'static str, Page>,
}

impl WebSite {
    /// Builds the site: precomputes `index.html` and `dev.html` for the embedded and placeholder sources.
    pub fn new(source: WebDistSource, page_title: &str, dev_surfaces: bool) -> WebSite {
        Self::build(source, None, page_title, dev_surfaces)
    }

    /// A site over an in-memory dist (path to file), precomputed like an embedded one. For tests and for
    /// callers that assemble a dist without a directory.
    pub fn in_memory(
        files: BTreeMap<String, WebFile>,
        page_title: &str,
        dev_surfaces: bool,
    ) -> WebSite {
        Self::build(
            WebDistSource::Placeholder,
            Some(files),
            page_title,
            dev_surfaces,
        )
    }

    fn build(
        source: WebDistSource,
        files: Option<BTreeMap<String, WebFile>>,
        page_title: &str,
        dev_surfaces: bool,
    ) -> WebSite {
        let mut site = WebSite {
            source,
            files,
            page_title: page_title.to_owned(),
            build16: String::new(),
            dev_surfaces,
            pages: BTreeMap::new(),
        };
        let mut h = Xxh3::new();
        h.update(BUILD_ID.as_bytes());
        h.update(&[0]);
        h.update(site.web_build_id().as_bytes());
        h.update(&[0]);
        h.update(page_title.as_bytes());
        site.build16 = format!("{:016x}", h.digest());
        if !matches!(site.source, WebDistSource::Disk(_)) {
            for name in ["index.html", "dev.html", "admin.html"] {
                if let Some(file) = site.load(name) {
                    let page = site.make_page(name, &file);
                    site.pages.insert(name, page);
                }
            }
        }
        site
    }

    /// One file of the dist.
    fn load(&self, path: &str) -> Option<WebFile> {
        match &self.files {
            Some(map) => map.get(path).cloned(),
            None => self.source.get(path),
        }
    }

    /// The `build-id.txt` of the client build (`"dev"` when missing).
    pub fn web_build_id(&self) -> String {
        match &self.files {
            Some(map) => map
                .get("build-id.txt")
                .and_then(|f| std::str::from_utf8(&f.bytes).ok())
                .map_or_else(|| "dev".to_owned(), |s| s.trim().to_owned()),
            None => self.source.build_id(),
        }
    }

    /// The substituted HTML of a page and its validators.
    fn make_page(&self, name: &str, file: &WebFile) -> Page {
        let identity = substitute_title(&file.bytes, &self.page_title)
            .map_or_else(|| Bytes::copy_from_slice(&file.bytes), Bytes::from);
        let gz = gzip_bytes(&identity, 9);
        let gz = gzip_beneficial(identity.len() as u64, gz.len() as u64).then(|| Bytes::from(gz));
        let etag = match name {
            "index.html" => format!("w-{}", self.build16),
            "admin.html" => format!("w-{}-admin", self.build16),
            _ => format!("w-{}-dev", self.build16),
        };
        Page { identity, gz, etag }
    }

    /// The page `name` (`index.html`, `dev.html`) as a servable, honouring `Accept-Encoding`.
    fn page_servable(&self, name: &'static str, accept_gzip: bool) -> Option<Servable> {
        if let Some(page) = self.pages.get(name) {
            let ct = mime::by_ext("html");
            let (bytes, enc) = match (&page.gz, accept_gzip) {
                (Some(gz), true) => (gz.clone(), StoredEncoding::Gzip),
                _ => (page.identity.clone(), StoredEncoding::Identity),
            };
            let len = bytes.len() as u64;
            return Some(
                Servable::new(ByteSource::Owned(bytes), len, ct)
                    .with_encoding(enc)
                    .with_etag(page.etag.clone())
                    .with_cache(CachePolicy::NoCache)
                    .varying(page.gz.is_some()),
            );
        }
        // Disk source: read and substitute per request (no gzip sibling for HTML: the title changes the bytes).
        let file = self.load(name)?;
        let bytes = substitute_title(&file.bytes, &self.page_title)
            .map_or_else(|| Bytes::copy_from_slice(&file.bytes), Bytes::from);
        let len = bytes.len() as u64;
        let mut s = Servable::new(ByteSource::Owned(bytes), len, mime::by_ext("html"))
            .with_cache(CachePolicy::NoCache);
        if let Some(stamp) = file.disk_stamp {
            s = s.with_etag(disk_etag(stamp, &self.page_title));
        }
        Some(s)
    }

    /// `admin.html`, the admin console's page, honouring `Accept-Encoding` (Plan/25 §11.2). `None` when the dist has no
    /// such page (a placeholder build or a client built before the console existed): the caller serves the rescue page.
    pub fn admin_servable(&self, accept_gzip: bool) -> Option<Servable> {
        self.page_servable("admin.html", accept_gzip)
    }

    /// A plain file of the dist (R2, R3).
    fn file_servable(&self, path: &str, accept_gzip: bool, cache: CachePolicy) -> Option<Servable> {
        let file = self.load(path)?;
        let ct = mime::by_path(path);
        let etag = file.disk_stamp.map(|s| disk_etag(s, ""));
        let has_gz = file.gz.is_some();
        let (source, len, enc) = match (&file.gz, accept_gzip) {
            (Some(gz), true) => (
                cow_source(gz.clone()),
                gz.len() as u64,
                StoredEncoding::Gzip,
            ),
            _ => (
                cow_source(file.bytes.clone()),
                file.bytes.len() as u64,
                StoredEncoding::Identity,
            ),
        };
        let mut s = Servable::new(source, len, ct)
            .with_encoding(enc)
            .with_cache(cache)
            .varying(has_gz);
        s.etag = etag.or_else(|| {
            // Embedded files: a content hash (they are small root files; assets are immutable)
            (cache == CachePolicy::NoCache).then(|| format!("w-{:016x}", xxh3_64(&file.bytes)))
        });
        Some(s)
    }
}

fn cow_source(c: std::borrow::Cow<'static, [u8]>) -> ByteSource {
    match c {
        std::borrow::Cow::Borrowed(b) => ByteSource::Static(b),
        std::borrow::Cow::Owned(v) => ByteSource::Owned(Bytes::from(v)),
    }
}

fn disk_etag((len, mtime_ns): (u64, i64), salt: &str) -> String {
    let mut h = Xxh3::new();
    h.update(&len.to_le_bytes());
    h.update(&mtime_ns.to_le_bytes());
    h.update(salt.as_bytes());
    format!("d-{:016x}", h.digest())
}

/// Replaces `%FLYFF_TITLE%` with the HTML-escaped title; `None` when the page has no placeholder (or is not
/// UTF-8).
pub fn substitute_title(html: &[u8], title: &str) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(html).ok()?;
    if !text.contains(TITLE_PLACEHOLDER) {
        return None;
    }
    let mut escaped = String::with_capacity(title.len());
    for c in title.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            c => escaped.push(c),
        }
    }
    Some(text.replace(TITLE_PLACEHOLDER, &escaped).into_bytes())
}

fn accepts(parts: &axum::http::request::Parts) -> bool {
    accepts_gzip(files::accept_encoding(&parts.headers))
}

/// R1: `GET /` and `GET /index.html`.
pub async fn index(State(st): State<HttpState>, req: Request) -> Response {
    let (parts, _) = req.into_parts();
    match st.site.page_servable("index.html", accepts(&parts)) {
        Some(s) => files::respond(&parts, s).await,
        None => error::not_found(),
    }
}

/// R2: `GET /assets/*`.
pub async fn asset(State(st): State<HttpState>, req: Request) -> Response {
    let (parts, _) = req.into_parts();
    let path = parts.uri.path().trim_start_matches('/');
    match st
        .site
        .file_servable(path, accepts(&parts), CachePolicy::Immutable)
    {
        Some(s) => files::respond(&parts, s).await,
        None => error::not_found(),
    }
}

/// R4: `GET /dev/{ui,models,world,sim}[/...]`.
pub async fn dev(State(st): State<HttpState>, req: Request) -> Response {
    let (parts, _) = req.into_parts();
    if !st.site.dev_surfaces {
        return error::not_found();
    }
    let first = parts
        .uri
        .path()
        .trim_start_matches('/')
        .split('/')
        .nth(1)
        .unwrap_or("");
    if !DEV_SURFACES.contains(&first) {
        return error::not_found();
    }
    match st.site.page_servable("dev.html", accepts(&parts)) {
        Some(s) => files::respond(&parts, s).await,
        None => error::not_found(),
    }
}

/// R3: `GET /<file>` for a single-segment path outside the reserved names; the default `robots.txt`.
pub async fn root_file(State(st): State<HttpState>, req: Request) -> Response {
    let (parts, _) = req.into_parts();
    let name = parts.uri.path().trim_start_matches('/');
    if name.is_empty() || name.contains('/') || RESERVED.contains(&name) || is_own_route_page(name)
    {
        return error::not_found();
    }
    if name == "index.html" {
        return match st.site.page_servable("index.html", accepts(&parts)) {
            Some(s) => files::respond(&parts, s).await,
            None => error::not_found(),
        };
    }
    if let Some(s) = st
        .site
        .file_servable(name, accepts(&parts), CachePolicy::NoCache)
    {
        return files::respond(&parts, s).await;
    }
    if name == "robots.txt" {
        let s = Servable::bytes(
            Bytes::from_static(DEFAULT_ROBOTS.as_bytes()),
            mime::by_ext("txt"),
        )
        .with_cache(CachePolicy::NoCache);
        return files::respond(&parts, s).await;
    }
    error::plain(StatusCode::NOT_FOUND, "not found")
}
