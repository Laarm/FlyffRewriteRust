//! The one client-IP and scheme rule of the process (Plan/24 §3.2).
//!
//! * A peer inside `web.trusted_proxies` is a reverse proxy: the client is the right-most address of
//!   `X-Forwarded-For` that is not itself trusted (every hop trusted: the left-most hop; header absent or
//!   unparsable: the peer). The scheme comes from `X-Forwarded-Proto`.
//! * Any other peer: the peer address; forwarding headers are ignored; the scheme is `https` iff the connection
//!   is TLS.
//!
//! IPv4-mapped IPv6 addresses are normalised to IPv4. The block list, the rate limits, the accept caps, the
//! gateway and the lobby all call this module; [`ClientInfo`] is computed once per request by the outermost
//! middleware and stored in the request extensions.

use super::cidr::CidrList;
use axum::http::{HeaderMap, Request};
use std::net::{IpAddr, SocketAddr};

/// What the outermost layer learned about the requester (inserted into the request extensions).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientInfo {
    /// The effective client address (§3.2).
    pub ip: IpAddr,
    /// The TCP peer (the proxy when one is trusted).
    pub peer: IpAddr,
    /// `true` for TLS connections, or `X-Forwarded-Proto: https` from a trusted proxy.
    pub https: bool,
}

impl ClientInfo {
    /// `"https"` or `"http"`.
    pub fn scheme(&self) -> &'static str {
        if self.https { "https" } else { "http" }
    }
}

/// The client address of a request (the plan's `http::client_ip`).
pub fn client_ip(headers: &HeaderMap, peer: SocketAddr, trusted: &CidrList) -> IpAddr {
    resolve(headers, peer, trusted, false).ip
}

/// Resolves address and scheme; `tls` is whether this connection itself is TLS.
pub fn resolve(headers: &HeaderMap, peer: SocketAddr, trusted: &CidrList, tls: bool) -> ClientInfo {
    let peer_ip = peer.ip().to_canonical();
    if trusted.is_empty() || !trusted.contains(peer_ip) {
        return ClientInfo {
            ip: peer_ip,
            peer: peer_ip,
            https: tls,
        };
    }
    let ip = forwarded_client(headers, trusted).unwrap_or(peer_ip);
    let https = match headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("https") => true,
        Some("http") => false,
        _ => tls,
    };
    ClientInfo {
        ip,
        peer: peer_ip,
        https,
    }
}

/// The convenience form for a whole request (`peer` from `ConnectInfo`).
pub fn of_request<B>(
    req: &Request<B>,
    peer: SocketAddr,
    trusted: &CidrList,
    tls: bool,
) -> ClientInfo {
    resolve(req.headers(), peer, trusted, tls)
}

fn forwarded_client(headers: &HeaderMap, trusted: &CidrList) -> Option<IpAddr> {
    let mut hops: Vec<IpAddr> = Vec::new();
    for value in headers.get_all("x-forwarded-for") {
        for part in value.to_str().ok()?.split(',') {
            hops.push(parse_hop(part)?);
        }
    }
    if hops.is_empty() {
        return None;
    }
    hops.iter()
        .rev()
        .find(|ip| !trusted.contains(**ip))
        .or_else(|| hops.first())
        .copied()
}

/// `1.2.3.4`, `1.2.3.4:5678`, `::1`, `[::1]`, `[::1]:80`.
fn parse_hop(text: &str) -> Option<IpAddr> {
    let t = text.trim();
    if let Ok(ip) = t.parse::<IpAddr>() {
        return Some(ip.to_canonical());
    }
    if let Ok(sa) = t.parse::<SocketAddr>() {
        return Some(sa.ip().to_canonical());
    }
    let inner = t.strip_prefix('[')?.strip_suffix(']')?;
    inner.parse::<IpAddr>().ok().map(|ip| ip.to_canonical())
}
