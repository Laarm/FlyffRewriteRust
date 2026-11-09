//! `http::mime::by_ext` (Plan/24 §3.8): lower-case extension to `Content-Type`.

/// The `Content-Type` of a lower-case extension without the dot; `application/octet-stream` when unknown.
///
/// Deviation: DDS is `image/vnd.ms-dds` (the IANA registration, the string `flyff-assets` uses for cached
/// artifacts); the plan table spells it `image/vnd-ms.dds`, which no client looks at (DDS is fetched as bytes).
pub fn by_ext(ext: &str) -> &'static str {
    match ext {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "wasm" => "application/wasm",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "svg" => "image/svg+xml",
        "o3d" | "chr" | "ani" | "lnd" | "sfx" | "bin" | "fpak" => "application/octet-stream",
        "dds" => "image/vnd.ms-dds",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "bmp" => "image/bmp",
        "tga" => "image/x-tga",
        "cur" | "ico" => "image/x-icon",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "mp3" => "audio/mpeg",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// The `Content-Type` for a (normalised) path by its extension.
pub fn by_path(path: &str) -> &'static str {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((_, ext)) => by_ext(&ext.to_ascii_lowercase()),
        None => "application/octet-stream",
    }
}
