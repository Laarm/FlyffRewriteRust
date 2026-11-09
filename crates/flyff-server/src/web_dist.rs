//! Where the browser client comes from (Plan/02 §7.1): the build embedded in the executable
//! (feature `embed-web`), a directory on disk (`--web-dir`, dev), or a built-in placeholder page.
//!
//! `EmbeddedWeb` embeds the folder named by `$FLYFF_WEB_DIST_DIR` (default `web/dist`, set in
//! `.cargo/config.toml`); `cargo xtask dist` points it at a generated placeholder folder when the web
//! client has not been built. `.gz` siblings produced by `build-web` are embedded too and returned in
//! [`WebFile::gz`] for the HTTP layer to negotiate.

use std::borrow::Cow;
use std::path::{Component, Path, PathBuf};

#[cfg(feature = "embed-web")]
#[derive(rust_embed::RustEmbed)]
#[folder = "$FLYFF_WEB_DIST_DIR"]
#[exclude = "*.map"]
struct EmbeddedWeb;

/// The NPC dialog scripts, embedded only so first start can recreate a missing `scripts/npc/`.
#[cfg(feature = "embed-web")]
#[derive(rust_embed::RustEmbed)]
#[folder = "../../content/npc/"]
#[exclude = "*.md"]
pub struct EmbeddedNpcScripts;

/// Fingerprint of the embedded web build, injected by xtask so cargo rebuilds when it changes.
pub const WEB_FINGERPRINT: Option<&str> = option_env!("FLYFF_WEB_FINGERPRINT");

const PLACEHOLDER_HTML: &str = concat!(
    "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">",
    "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">",
    "<title>Flyff V15</title>",
    "<style>body{font-family:system-ui,sans-serif;max-width:40rem;margin:4rem auto;padding:0 1rem;",
    "color:#222;background:#fafafa}code{background:#eee;padding:.1rem .3rem;border-radius:3px}</style>",
    "</head><body><h1>Flyff V15</h1>",
    "<p>The server is running, but the web client has not been built into this executable.</p>",
    "<p>Run <code>cargo xtask build-web</code> and rebuild with <code>--features embed-web</code>, ",
    "or point the server at a build with <code>--web-dir</code>.</p>",
    "<p>Server health: <a href=\"/healthz\">/healthz</a></p></body></html>\n"
);

/// Source of the web client files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WebDistSource {
    /// The build embedded with feature `embed-web`.
    Embedded,
    /// A directory (`--web-dir`).
    Disk(PathBuf),
    /// Neither: serves one built-in page.
    Placeholder,
}

/// One servable file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebFile {
    /// Raw bytes.
    pub bytes: Cow<'static, [u8]>,
    /// Pre-compressed gzip sibling, when present.
    pub gz: Option<Cow<'static, [u8]>>,
    /// `Content-Type`.
    pub mime: &'static str,
    /// Content-hashed asset (`assets/…`): safe to cache forever.
    pub immutable: bool,
    /// `(len, mtime_ns)` of a file read from disk (`--web-dir`); the validator of Plan/24 §4.4 mode B.
    pub disk_stamp: Option<(u64, i64)>,
}

impl WebDistSource {
    /// `--web-dir` wins, then the embedded build (if compiled in), then the placeholder.
    pub fn select(web_dir: Option<PathBuf>) -> WebDistSource {
        match web_dir {
            Some(dir) => WebDistSource::Disk(dir),
            None if cfg!(feature = "embed-web") => WebDistSource::Embedded,
            None => WebDistSource::Placeholder,
        }
    }

    /// Looks a request path up (without the leading `/`). Rejects `..`, absolute paths and
    /// backslashes. The empty path is `index.html`.
    pub fn get(&self, path: &str) -> Option<WebFile> {
        let path = sanitize(path)?;
        let path = if path.is_empty() {
            "index.html".to_owned()
        } else {
            path
        };
        match self {
            WebDistSource::Placeholder => (path == "index.html").then(|| WebFile {
                bytes: Cow::Borrowed(PLACEHOLDER_HTML.as_bytes()),
                gz: None,
                mime: "text/html; charset=utf-8",
                immutable: false,
                disk_stamp: None,
            }),
            WebDistSource::Disk(dir) => {
                let bytes = std::fs::read(dir.join(&path)).ok()?;
                let gz = std::fs::read(dir.join(format!("{path}.gz"))).ok();
                let stamp = std::fs::metadata(dir.join(&path)).ok().map(|m| {
                    let mtime_ns = m
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map_or(0, |d| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX));
                    (m.len(), mtime_ns)
                });
                let mut file = make_file(&path, Cow::Owned(bytes), gz.map(Cow::Owned));
                file.disk_stamp = stamp;
                Some(file)
            }
            WebDistSource::Embedded => embedded_get(&path),
        }
    }

    /// `build-id.txt` of the client build; `"dev"` for the placeholder or when missing.
    pub fn build_id(&self) -> String {
        match self {
            WebDistSource::Placeholder => "dev".to_owned(),
            _ => self
                .get("build-id.txt")
                .and_then(|f| String::from_utf8(f.bytes.into_owned()).ok())
                .map_or_else(|| "dev".to_owned(), |s| s.trim().to_owned()),
        }
    }
}

#[cfg(feature = "embed-web")]
fn embedded_get(path: &str) -> Option<WebFile> {
    let file = EmbeddedWeb::get(path)?;
    let gz = EmbeddedWeb::get(&format!("{path}.gz")).map(|g| g.data);
    Some(make_file(path, file.data, gz))
}

#[cfg(not(feature = "embed-web"))]
fn embedded_get(_path: &str) -> Option<WebFile> {
    None
}

fn make_file(path: &str, bytes: Cow<'static, [u8]>, gz: Option<Cow<'static, [u8]>>) -> WebFile {
    let mime = mime_guess::from_path(path)
        .first_raw()
        .unwrap_or("application/octet-stream");
    WebFile {
        bytes,
        gz,
        mime,
        immutable: path.starts_with("assets/"),
        disk_stamp: None,
    }
}

/// Normalises a URL path to a relative `/`-separated path, or `None` when it tries to escape.
fn sanitize(path: &str) -> Option<String> {
    if path.contains('\\') || path.contains('\0') {
        return None;
    }
    let trimmed = path.trim_start_matches('/');
    let mut parts = Vec::new();
    for comp in Path::new(trimmed).components() {
        match comp {
            Component::Normal(s) => parts.push(s.to_str()?.to_owned()),
            Component::CurDir => {}
            // `..`, prefixes and root directories are never valid inside the dist folder.
            _ => return None,
        }
    }
    Some(parts.join("/"))
}
