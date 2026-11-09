//! `http::files`: the one helper that serves every byte source (embedded file, disk file, cache file,
//! in-memory bytes) — Plan/24 §3.9.
//!
//! * conditional GET: `If-None-Match` (weak comparison) and `If-Modified-Since` give `304`;
//! * `Range` (only when [`Servable::ranges`] and the stored form is identity): one `bytes=a-b`, `bytes=a-` or
//!   `bytes=-n` gives `206`; unsatisfiable gives `416`; several ranges, a malformed header or an `If-Range` that
//!   does not match give the full `200` (RFC 9110 allows all three);
//! * headers: `Content-Type`, `Content-Length`, `ETag`, `Cache-Control`, `Vary`, `Content-Encoding`,
//!   `Accept-Ranges`, `Last-Modified`;
//! * bodies of at most 256 KiB are read whole (one blocking read on tokio's bounded blocking pool), larger ones
//!   stream in 256 KiB chunks;
//! * a gzip-stored artifact is sent as is when the client accepts gzip, otherwise it is inflated.
//!
//! The ETag of a [`Servable`] is the tag of the *identity* form; the response carries `"<tag>"`, or
//! `"<tag>-gz"` when the body is sent gzip-encoded (§5.3 step 7, §9.1 step 6).

use super::error;
use axum::body::Body;
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, header};
use axum::response::Response;
use bytes::Bytes;
use flyff_assets::StoredEncoding;
use flyff_assets::build::gz::accepts_gzip;
use std::io::SeekFrom;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

/// Bodies up to this size are read whole; larger ones are streamed (§3.9 step 4).
pub const SMALL_BODY: u64 = 256 * 1024;
/// Chunk size of streamed bodies.
const CHUNK: usize = 256 * 1024;

/// `Cache-Control` of a response.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CachePolicy {
    /// `public, max-age=31536000, immutable` (a current fingerprint).
    Immutable,
    /// `no-cache` (revalidate every time).
    NoCache,
    /// `no-store` (a stale fingerprint, API-ish responses).
    NoStore,
}

impl CachePolicy {
    /// The header value.
    pub const fn value(self) -> &'static str {
        match self {
            CachePolicy::Immutable => "public, max-age=31536000, immutable",
            CachePolicy::NoCache => "no-cache",
            CachePolicy::NoStore => "no-store",
        }
    }
}

/// Where the stored bytes come from.
#[derive(Clone, Debug)]
pub enum ByteSource {
    /// Compiled into the executable.
    Static(&'static [u8]),
    /// In memory.
    Owned(Bytes),
    /// A file (cache artifact, raw resource, web dist file on disk).
    File(PathBuf),
}

/// What to serve and how (the plan's `Servable`).
#[derive(Clone, Debug)]
pub struct Servable {
    /// The stored bytes.
    pub source: ByteSource,
    /// Stored length in bytes.
    pub len: u64,
    /// How the stored bytes are encoded.
    pub encoding: StoredEncoding,
    /// `Content-Type` of the identity form.
    pub content_type: &'static str,
    /// Tag of the identity form (without quotes); `None` = no validator.
    pub etag: Option<String>,
    /// `Cache-Control`.
    pub cache: CachePolicy,
    /// Emit `Vary: Accept-Encoding`.
    pub vary_encoding: bool,
    /// Answer `Range` requests (identity-stored sources only).
    pub ranges: bool,
    /// `Last-Modified` (disk sources).
    pub last_modified: Option<SystemTime>,
}

impl Servable {
    /// A servable with the usual defaults: identity, no validator, `no-cache`, no ranges.
    pub fn new(source: ByteSource, len: u64, content_type: &'static str) -> Self {
        Self {
            source,
            len,
            encoding: StoredEncoding::Identity,
            content_type,
            etag: None,
            cache: CachePolicy::NoCache,
            vary_encoding: false,
            ranges: false,
            last_modified: None,
        }
    }

    /// In-memory identity bytes.
    pub fn bytes(bytes: impl Into<Bytes>, content_type: &'static str) -> Self {
        let bytes: Bytes = bytes.into();
        let len = bytes.len() as u64;
        Self::new(ByteSource::Owned(bytes), len, content_type)
    }

    /// Sets the identity ETag tag.
    #[must_use]
    pub fn with_etag(mut self, tag: impl Into<String>) -> Self {
        self.etag = Some(tag.into());
        self
    }

    /// Sets the cache policy.
    #[must_use]
    pub fn with_cache(mut self, cache: CachePolicy) -> Self {
        self.cache = cache;
        self
    }

    /// Sets the stored encoding.
    #[must_use]
    pub fn with_encoding(mut self, encoding: StoredEncoding) -> Self {
        self.encoding = encoding;
        self
    }

    /// Emits `Vary: Accept-Encoding`.
    #[must_use]
    pub fn varying(mut self, vary: bool) -> Self {
        self.vary_encoding = vary;
        self
    }

    /// Enables `Range` handling.
    #[must_use]
    pub fn with_ranges(mut self, ranges: bool) -> Self {
        self.ranges = ranges;
        self
    }

    /// Sets `Last-Modified`.
    #[must_use]
    pub fn modified(mut self, t: Option<SystemTime>) -> Self {
        self.last_modified = t;
        self
    }
}

/// The `Accept-Encoding` header as text.
pub fn accept_encoding(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::ACCEPT_ENCODING)
        .and_then(|v| v.to_str().ok())
}

/// Serves `s` for the request `parts` (GET or HEAD; anything else is the caller's `405`).
pub async fn respond(parts: &Parts, s: Servable) -> Response {
    let head = parts.method == Method::HEAD;
    let headers = &parts.headers;
    let stored_gzip = s.encoding == StoredEncoding::Gzip;
    let send_gzip = stored_gzip && accepts_gzip(accept_encoding(headers));
    let inflate = stored_gzip && !send_gzip;
    let etag = s.etag.as_ref().map(|t| {
        if send_gzip {
            format!("\"{t}-gz\"")
        } else {
            format!("\"{t}\"")
        }
    });
    let last_modified = s.last_modified.and_then(unix_secs);
    let last_modified_text = last_modified.map(http_date);

    // 1. conditional requests
    if not_modified(headers, etag.as_deref(), last_modified) {
        let mut r = Response::new(Body::empty());
        *r.status_mut() = StatusCode::NOT_MODIFIED;
        decorate(
            r.headers_mut(),
            &s,
            etag.as_deref(),
            last_modified_text.as_deref(),
            send_gzip,
        );
        return r;
    }

    // An inflated body has no stored length: read and decode it before the headers are written.
    let (source, stored_len) = if inflate {
        match inflated(&s).await {
            Ok(b) => {
                let len = b.len() as u64;
                (ByteSource::Owned(b), len)
            }
            Err(r) => return *r,
        }
    } else {
        (s.source.clone(), s.len)
    };
    let ranges_ok = s.ranges && !stored_gzip;

    // 2. Range
    let mut status = StatusCode::OK;
    let mut window = (0u64, stored_len);
    let mut content_range: Option<String> = None;
    if ranges_ok
        && let Some(raw) = headers.get(header::RANGE).and_then(|v| v.to_str().ok())
        && if_range_matches(headers, etag.as_deref(), last_modified_text.as_deref())
    {
        match parse_range(raw, stored_len) {
            RangeOutcome::Ignore => {}
            RangeOutcome::Single(a, b) => {
                status = StatusCode::PARTIAL_CONTENT;
                window = (a, b - a + 1);
                content_range = Some(format!("bytes {a}-{b}/{stored_len}"));
            }
            RangeOutcome::Unsatisfiable => {
                let mut r = Response::new(Body::empty());
                *r.status_mut() = StatusCode::RANGE_NOT_SATISFIABLE;
                decorate(
                    r.headers_mut(),
                    &s,
                    etag.as_deref(),
                    last_modified_text.as_deref(),
                    send_gzip,
                );
                set(
                    r.headers_mut(),
                    header::CONTENT_RANGE,
                    &format!("bytes */{stored_len}"),
                );
                return r;
            }
        }
    }

    // 3. body
    let body = if head {
        Body::empty()
    } else {
        match open_body(&source, window).await {
            Ok(b) => b,
            Err(r) => return *r,
        }
    };
    let mut resp = Response::new(body);
    *resp.status_mut() = status;
    decorate(
        resp.headers_mut(),
        &s,
        etag.as_deref(),
        last_modified_text.as_deref(),
        send_gzip,
    );
    let h = resp.headers_mut();
    set(h, header::CONTENT_TYPE, s.content_type);
    set(h, header::CONTENT_LENGTH, &window.1.to_string());
    if send_gzip {
        h.insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
    }
    if ranges_ok {
        h.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    }
    if let Some(cr) = content_range {
        set(h, header::CONTENT_RANGE, &cr);
    }
    resp
}

/// Validators and cache headers shared by 200, 206, 304 and 416 responses.
fn decorate(
    h: &mut HeaderMap,
    s: &Servable,
    etag: Option<&str>,
    last_modified: Option<&str>,
    _gzip: bool,
) {
    if let Some(e) = etag {
        set(h, header::ETAG, e);
    }
    h.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(s.cache.value()),
    );
    if s.vary_encoding {
        h.insert(header::VARY, HeaderValue::from_static("Accept-Encoding"));
    }
    if let Some(lm) = last_modified {
        set(h, header::LAST_MODIFIED, lm);
    }
}

fn set(h: &mut HeaderMap, name: HeaderName, value: &str) {
    if let Ok(v) = HeaderValue::from_str(value) {
        h.insert(name, v);
    }
}

fn unix_secs(t: SystemTime) -> Option<u64> {
    t.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs())
}

// ---- conditional requests ------------------------------------------------------------------------------------

/// `If-None-Match` (weak comparison; `*` matches any current representation) or, without it,
/// `If-Modified-Since`.
fn not_modified(headers: &HeaderMap, etag: Option<&str>, last_modified: Option<u64>) -> bool {
    if let Some(inm) = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
    {
        let Some(etag) = etag else {
            return inm.trim() == "*";
        };
        return inm.split(',').any(|t| {
            let t = t.trim();
            t == "*" || strip_weak(t) == etag
        });
    }
    if let (Some(lm), Some(ims)) = (
        last_modified,
        headers
            .get(header::IF_MODIFIED_SINCE)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_http_date),
    ) {
        return lm <= ims;
    }
    false
}

fn strip_weak(tag: &str) -> &str {
    tag.strip_prefix("W/").unwrap_or(tag)
}

/// `If-Range`: absent, or an entity tag equal to ours (strong comparison), or our `Last-Modified` date.
fn if_range_matches(headers: &HeaderMap, etag: Option<&str>, last_modified: Option<&str>) -> bool {
    let Some(v) = headers.get(header::IF_RANGE).and_then(|v| v.to_str().ok()) else {
        return true;
    };
    let v = v.trim();
    if v.starts_with('"') {
        etag == Some(v)
    } else if v.starts_with("W/") {
        false
    } else {
        last_modified == Some(v)
    }
}

// ---- ranges --------------------------------------------------------------------------------------------------

/// The result of interpreting a `Range` header against a representation of `len` bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RangeOutcome {
    /// Serve the whole representation (no, several or malformed ranges).
    Ignore,
    /// Serve bytes `a..=b` as `206`.
    Single(u64, u64),
    /// `416`.
    Unsatisfiable,
}

/// Parses a `Range` header value (`bytes=a-b`, `bytes=a-`, `bytes=-n`).
pub fn parse_range(value: &str, len: u64) -> RangeOutcome {
    let Some(spec) = value.trim().strip_prefix("bytes=") else {
        return RangeOutcome::Ignore;
    };
    if spec.contains(',') {
        return RangeOutcome::Ignore;
    }
    let Some((first, last)) = spec.trim().split_once('-') else {
        return RangeOutcome::Ignore;
    };
    let (first, last) = (first.trim(), last.trim());
    if first.is_empty() {
        // suffix range: the last n bytes
        let Ok(n) = last.parse::<u64>() else {
            return RangeOutcome::Ignore;
        };
        if n == 0 || len == 0 {
            return RangeOutcome::Unsatisfiable;
        }
        let n = n.min(len);
        return RangeOutcome::Single(len - n, len - 1);
    }
    let Ok(a) = first.parse::<u64>() else {
        return RangeOutcome::Ignore;
    };
    let b = if last.is_empty() {
        None
    } else {
        match last.parse::<u64>() {
            Ok(b) => Some(b),
            Err(_) => return RangeOutcome::Ignore,
        }
    };
    if let Some(b) = b
        && b < a
    {
        return RangeOutcome::Ignore;
    }
    if a >= len {
        return RangeOutcome::Unsatisfiable;
    }
    let b = b.map_or(len - 1, |b| b.min(len - 1));
    RangeOutcome::Single(a, b)
}

// ---- bodies --------------------------------------------------------------------------------------------------

type BodyResult = Result<Body, Box<Response>>;

/// Opens `window = (offset, length)` of the stored bytes as a body.
async fn open_body(source: &ByteSource, window: (u64, u64)) -> BodyResult {
    let (offset, length) = window;
    match source {
        ByteSource::Static(b) => Ok(Body::from(slice_of(Bytes::from_static(b), offset, length))),
        ByteSource::Owned(b) => Ok(Body::from(slice_of(b.clone(), offset, length))),
        ByteSource::File(path) => {
            if length <= SMALL_BODY {
                let path = path.clone();
                let read = tokio::task::spawn_blocking(move || read_window(&path, offset, length))
                    .await
                    .map_err(|_| {
                        Box::new(error::plain(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "read failed",
                        ))
                    })?;
                match read {
                    Ok(b) => Ok(Body::from(b)),
                    Err(e) => Err(Box::new(io_response(&e))),
                }
            } else {
                let mut f = tokio::fs::File::open(path)
                    .await
                    .map_err(|e| io_response(&e))?;
                if offset > 0 {
                    f.seek(SeekFrom::Start(offset))
                        .await
                        .map_err(|e| Box::new(io_response(&e)))?;
                }
                let reader = f.take(length);
                Ok(Body::from_stream(
                    tokio_util::io::ReaderStream::with_capacity(reader, CHUNK),
                ))
            }
        }
    }
}

fn slice_of(b: Bytes, offset: u64, length: u64) -> Bytes {
    let start = usize::try_from(offset).unwrap_or(usize::MAX).min(b.len());
    let end = usize::try_from(offset.saturating_add(length))
        .unwrap_or(usize::MAX)
        .min(b.len());
    b.slice(start..end)
}

fn read_window(path: &std::path::Path, offset: u64, length: u64) -> std::io::Result<Vec<u8>> {
    use std::io::{Read, Seek};
    let mut f = std::fs::File::open(path)?;
    if offset > 0 {
        f.seek(SeekFrom::Start(offset))?;
    }
    let mut buf = Vec::with_capacity(usize::try_from(length).unwrap_or(0));
    f.take(length).read_to_end(&mut buf)?;
    Ok(buf)
}

fn io_response(e: &std::io::Error) -> Response {
    if e.kind() == std::io::ErrorKind::NotFound {
        error::not_found()
    } else {
        tracing::warn!(error = %e, "reading a servable file failed");
        error::plain(StatusCode::INTERNAL_SERVER_ERROR, "read failed")
    }
}

/// The identity bytes of a gzip-stored servable (for clients that do not accept gzip).
async fn inflated(s: &Servable) -> Result<Bytes, Box<Response>> {
    let source = s.source.clone();
    let hint = usize::try_from(s.len.saturating_mul(4))
        .unwrap_or(0)
        .min(64 << 20);
    let result = tokio::task::spawn_blocking(move || -> std::io::Result<Vec<u8>> {
        use std::io::Read;
        let stored: Vec<u8> = match &source {
            ByteSource::Static(b) => b.to_vec(),
            ByteSource::Owned(b) => b.to_vec(),
            ByteSource::File(p) => std::fs::read(p)?,
        };
        let mut out = Vec::with_capacity(hint);
        flate2::read::GzDecoder::new(&stored[..]).read_to_end(&mut out)?;
        Ok(out)
    })
    .await
    .map_err(|_| {
        Box::new(error::plain(
            StatusCode::INTERNAL_SERVER_ERROR,
            "inflate failed",
        ))
    })?;
    match result {
        Ok(v) => Ok(Bytes::from(v)),
        Err(e) => Err(Box::new(io_response(&e))),
    }
}

// ---- HTTP dates ----------------------------------------------------------------------------------------------

const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// `Sun, 06 Nov 1994 08:49:37 GMT` (IMF-fixdate) for Unix seconds.
pub fn http_date(secs: u64) -> String {
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    let weekday = usize::try_from((days + 4).rem_euclid(7)).unwrap_or(0);
    format!(
        "{}, {:02} {} {:04} {:02}:{:02}:{:02} GMT",
        DAYS.get(weekday).copied().unwrap_or("Thu"),
        d,
        MONTHS
            .get(usize::try_from(m - 1).unwrap_or(0))
            .copied()
            .unwrap_or("Jan"),
        y,
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Parses an IMF-fixdate back to Unix seconds (the only form this server emits).
pub fn parse_http_date(text: &str) -> Option<u64> {
    let t = text.trim();
    let rest = t.split_once(", ")?.1;
    let mut it = rest.split_whitespace();
    let day: i64 = it.next()?.parse().ok()?;
    let month_name = it.next()?;
    let month = MONTHS.iter().position(|m| *m == month_name)? as i64 + 1;
    let year: i64 = it.next()?.parse().ok()?;
    let mut hms = it.next()?.split(':');
    let h: u64 = hms.next()?.parse().ok()?;
    let mi: u64 = hms.next()?.parse().ok()?;
    let s: u64 = hms.next()?.parse().ok()?;
    if it.next()? != "GMT" || !(1..=31).contains(&day) || h > 23 || mi > 59 || s > 60 {
        return None;
    }
    let days = days_from_civil(year, month, day);
    u64::try_from(days)
        .ok()
        .map(|d| d * 86_400 + h * 3600 + mi * 60 + s)
}

/// Days since 1970-01-01 to (year, month, day) (proleptic Gregorian; H. Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}
