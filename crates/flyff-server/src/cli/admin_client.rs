//! The loopback client of the admin API (Plan/12 §5.5 online mode, 10 §9.2 `grant`): a tiny blocking HTTP/1.1
//! client for `http://127.0.0.1:<port of net.bind>`. The server is on the same machine, the connection is plain
//! HTTP (the built-in TLS is WP-WEB-16; behind a TLS-terminating proxy the proxy is not involved because the CLI
//! talks to the bind address itself), the request has `Connection: close`, and every answer is small JSON.
//!
//! `Authorization: Bearer <data/admin.token>` is sent when the file is readable: loopback requests are accepted
//! without it (23 §8.3), but the header makes the CLI work when `admin.allow_remote` rules require it.

use serde_json::Value;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::path::Path;
use std::time::Duration;

/// An answer of the admin API.
#[derive(Clone, Debug, PartialEq)]
pub struct Reply {
    /// HTTP status.
    pub status: u16,
    /// The JSON body (`Null` when it is empty or not JSON).
    pub body: Value,
    /// The body text as received (for messages).
    pub text: String,
}

impl Reply {
    /// The `error` token of a refusal, or the empty string.
    pub fn error_token(&self) -> &str {
        self.body.get("error").and_then(Value::as_str).unwrap_or("")
    }

    /// The `detail` text of a refusal, or the whole body.
    pub fn detail(&self) -> &str {
        self.body
            .get("detail")
            .and_then(Value::as_str)
            .unwrap_or(self.text.as_str())
    }
}

/// Why a request produced no [`Reply`].
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// The connection failed or timed out (exit 3).
    #[error("cannot reach the server at {addr}: {source}")]
    Unreachable {
        /// The address tried.
        addr: SocketAddr,
        /// Cause.
        source: std::io::Error,
    },
    /// The answer is not HTTP.
    #[error("the server answered something that is not HTTP")]
    BadResponse,
}

/// The loopback client.
#[derive(Clone, Debug)]
pub struct AdminClient {
    addr: SocketAddr,
    token: Option<String>,
    timeout: Duration,
}

/// The address the CLI connects to for a `net.bind` value: a wildcard bind is reached through loopback.
pub fn loopback_addr(bind: &str) -> Option<SocketAddr> {
    let a: SocketAddr = bind.parse().ok()?;
    let ip = match a.ip() {
        IpAddr::V4(v4) if v4.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(v6) if v6.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        other => other,
    };
    Some(SocketAddr::new(ip, a.port()))
}

impl AdminClient {
    /// A client for the server bound at `addr`; the token is read from `<data_dir>/admin.token` when present.
    pub fn new(addr: SocketAddr, data_dir: &Path) -> AdminClient {
        let token = std::fs::read_to_string(data_dir.join(crate::admin::ADMIN_TOKEN_FILE))
            .ok()
            .map(|t| t.trim().to_owned())
            .filter(|t| !t.is_empty());
        AdminClient {
            addr,
            token,
            timeout: Duration::from_secs(30),
        }
    }

    /// `request` with a JSON body (`None` = an empty body; mutating methods still carry the JSON content type,
    /// which the access rules require).
    pub fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Reply, ClientError> {
        let unreachable = |source| ClientError::Unreachable {
            addr: self.addr,
            source,
        };
        let mut stream =
            TcpStream::connect_timeout(&self.addr, Duration::from_secs(3)).map_err(unreachable)?;
        stream
            .set_read_timeout(Some(self.timeout))
            .map_err(unreachable)?;
        stream
            .set_write_timeout(Some(self.timeout))
            .map_err(unreachable)?;
        let payload = body.map(Value::to_string).unwrap_or_default();
        let host = match self.addr {
            SocketAddr::V4(a) => format!("{}:{}", a.ip(), a.port()),
            SocketAddr::V6(a) => format!("[{}]:{}", a.ip(), a.port()),
        };
        let mut head = format!(
            "{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nAccept: application/json\r\n"
        );
        if method != "GET" {
            head.push_str("Content-Type: application/json\r\n");
            head.push_str(&format!("Content-Length: {}\r\n", payload.len()));
        }
        if let Some(t) = &self.token {
            head.push_str(&format!("Authorization: Bearer {t}\r\n"));
        }
        head.push_str("\r\n");
        stream
            .write_all(head.as_bytes())
            .and_then(|()| stream.write_all(payload.as_bytes()))
            .map_err(unreachable)?;
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).map_err(unreachable)?;
        parse_response(&raw)
    }
}

/// Parses `HTTP/1.1 <status> ...` + headers + a body (`Content-Length` or until the close; `chunked` decoded).
pub fn parse_response(raw: &[u8]) -> Result<Reply, ClientError> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or(ClientError::BadResponse)?;
    let head = String::from_utf8_lossy(raw.get(..split).unwrap_or_default()).into_owned();
    let mut lines = head.lines();
    let status = lines
        .next()
        .filter(|l| l.starts_with("HTTP/1."))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse::<u16>().ok())
        .ok_or(ClientError::BadResponse)?;
    let chunked = lines.any(|l| {
        l.split_once(':').is_some_and(|(k, v)| {
            k.trim().eq_ignore_ascii_case("transfer-encoding")
                && v.to_ascii_lowercase().contains("chunked")
        })
    });
    let rest = raw.get(split + 4..).unwrap_or_default();
    let bytes = if chunked {
        dechunk(rest)
    } else {
        rest.to_vec()
    };
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let body = serde_json::from_str(&text).unwrap_or(Value::Null);
    Ok(Reply { status, body, text })
}

fn dechunk(mut rest: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(eol) = rest.windows(2).position(|w| w == b"\r\n") {
        let size_text = String::from_utf8_lossy(rest.get(..eol).unwrap_or_default()).into_owned();
        let size_text = size_text.split(';').next().unwrap_or("").trim().to_owned();
        let Ok(size) = usize::from_str_radix(&size_text, 16) else {
            break;
        };
        rest = rest.get(eol + 2..).unwrap_or_default();
        if size == 0 {
            break;
        }
        let Some(chunk) = rest.get(..size) else {
            break;
        };
        out.extend_from_slice(chunk);
        rest = rest.get(size + 2..).unwrap_or_default();
    }
    out
}

/// Percent-encodes a query value.
pub fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
