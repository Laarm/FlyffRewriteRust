//! The S12 exposure warning (Plan/10 §3.1 S12; 61 §9 Q2, RK-39, RK-43): operator-visible only, no config key.
//!
//! [`exposure_warning`] is a pure function of the configuration and the host's interface addresses; the
//! start-up logs its result once and `flyff check` repeats it as a warning line (not a finding).
//! [`interface_addrs`] reads the addresses of the host from `/proc/net` (best effort, no `unsafe`).

use crate::config::Config;
use std::net::{IpAddr, Ipv6Addr, SocketAddr};

/// `true` when `ip` is a public address: not loopback, RFC 1918, link-local or ULA (`fc00::/7`).
/// The unspecified address is not "public" by itself (its interface addresses are examined instead).
pub fn is_publicly_routable(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            !(v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified())
        }
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => is_publicly_routable(IpAddr::V4(v4)),
            None => !(v6.is_loopback() || v6.is_unspecified() || is_ula(v6) || is_link_local6(v6)),
        },
    }
}

fn is_ula(ip: Ipv6Addr) -> bool {
    ip.segments()[0] & 0xfe00 == 0xfc00
}

fn is_link_local6(ip: Ipv6Addr) -> bool {
    ip.segments()[0] & 0xffc0 == 0xfe80
}

/// The warning text, or `None` when the exposure is acceptable.
///
/// * the bind address is public (or unspecified with a public interface address in `interfaces`), and
/// * `web.public_assets = true` or no TLS certificate is configured.
///
/// The text names the public address(es) and the reason(s) ("public assets", "plain HTTP").
pub fn exposure_warning(cfg: &Config, interfaces: &[IpAddr]) -> Option<String> {
    let bind: SocketAddr = cfg.net.bind.parse().ok()?;
    let exposed: Vec<IpAddr> = if bind.ip().is_unspecified() {
        let mut v: Vec<IpAddr> = interfaces
            .iter()
            .copied()
            .filter(|ip| is_publicly_routable(*ip))
            .collect();
        v.sort();
        v.dedup();
        v
    } else if is_publicly_routable(bind.ip()) {
        vec![bind.ip()]
    } else {
        Vec::new()
    };
    if exposed.is_empty() {
        return None;
    }
    let tls = !cfg.net.tls.cert.is_empty() && !cfg.net.tls.key.is_empty();
    let mut reasons = Vec::new();
    if cfg.web.public_assets {
        reasons.push("web.public_assets = true serves the proprietary game assets to anyone");
    }
    if !tls {
        reasons.push("the connection is plain HTTP (net.tls.cert is empty)");
    }
    if reasons.is_empty() {
        return None;
    }
    let addrs = exposed
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!(
        "the server is reachable on a public address ({addrs}): {}; keep it on a private network \
         or put it behind TLS / a reverse proxy (README, \"Network\")",
        reasons.join(" and ")
    ))
}

/// The IPv4 and IPv6 addresses of this host's interfaces (Linux: `/proc/net/fib_trie`, `if_inet6`).
/// An unreadable file contributes nothing.
pub fn interface_addrs() -> Vec<IpAddr> {
    let mut out = Vec::new();
    if let Ok(text) = std::fs::read_to_string("/proc/net/fib_trie") {
        out.extend(parse_fib_trie(&text).into_iter().map(IpAddr::V4));
    }
    if let Ok(text) = std::fs::read_to_string("/proc/net/if_inet6") {
        out.extend(parse_if_inet6(&text).into_iter().map(IpAddr::V6));
    }
    out.sort();
    out.dedup();
    out
}

/// Local IPv4 addresses from the text of `/proc/net/fib_trie` (`|-- a.b.c.d` followed by `/32 host LOCAL`).
pub fn parse_fib_trie(text: &str) -> Vec<std::net::Ipv4Addr> {
    let mut out = Vec::new();
    let mut last = None;
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("|--") {
            last = rest.trim().parse::<std::net::Ipv4Addr>().ok();
        } else if t.starts_with("/32 host LOCAL")
            && let Some(ip) = last.take()
        {
            out.push(ip);
        }
    }
    out.sort();
    out.dedup();
    out
}

/// IPv6 addresses from the text of `/proc/net/if_inet6` (first column: 32 hex digits).
pub fn parse_if_inet6(text: &str) -> Vec<Ipv6Addr> {
    text.lines()
        .filter_map(|l| {
            let hex = l.split_whitespace().next()?;
            if hex.len() != 32 {
                return None;
            }
            let mut segs = [0u16; 8];
            for (i, seg) in segs.iter_mut().enumerate() {
                *seg = u16::from_str_radix(hex.get(i * 4..i * 4 + 4)?, 16).ok()?;
            }
            Some(Ipv6Addr::from(segs))
        })
        .collect()
}
