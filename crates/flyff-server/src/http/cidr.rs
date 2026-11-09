//! CIDR networks for `web.trusted_proxies` and `net.ip_block` (Plan/24 §3.2, §3.3 layer 0).
//!
//! [`Cidr::parse`] accepts what the config validator accepts (`a.b.c.d/len`, `v6::/len`, a bare address is a
//! host route). Matching canonicalises IPv4-mapped IPv6 addresses first, so `::ffff:10.0.0.1` is inside
//! `10.0.0.0/8`.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// One CIDR network (the address is stored masked).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cidr {
    addr: IpAddr,
    len: u8,
}

impl Cidr {
    /// Parses `10.0.0.0/8`, `fe80::/10` or a bare host address; `None` for anything else (including a prefix
    /// longer than the address).
    pub fn parse(text: &str) -> Option<Cidr> {
        let text = text.trim();
        let (addr, prefix) = match text.split_once('/') {
            Some((a, p)) => (a, Some(p)),
            None => (text, None),
        };
        let ip: IpAddr = addr.trim().parse().ok()?;
        let ip = ip.to_canonical();
        let max = if ip.is_ipv4() { 32 } else { 128 };
        let len = match prefix {
            // A prefix on a mapped address counts in IPv6 bits in the text form (`::ffff:1.2.3.0/120`):
            // convert to the IPv4 prefix.
            Some(p) => {
                let raw: u8 = p.trim().parse().ok()?;
                let was_mapped = matches!(addr.trim().parse::<IpAddr>(), Ok(IpAddr::V6(v6)) if v6.to_ipv4_mapped().is_some());
                let len = if was_mapped && ip.is_ipv4() {
                    raw.checked_sub(96)?
                } else {
                    raw
                };
                if len > max {
                    return None;
                }
                len
            }
            None => max,
        };
        Some(Cidr {
            addr: mask(ip, len),
            len,
        })
    }

    /// `true` when `ip` is inside the network.
    pub fn contains(&self, ip: IpAddr) -> bool {
        let ip = ip.to_canonical();
        match (self.addr, ip) {
            (IpAddr::V4(_), IpAddr::V4(_)) | (IpAddr::V6(_), IpAddr::V6(_)) => {
                mask(ip, self.len) == self.addr
            }
            _ => false,
        }
    }

    /// The prefix length.
    pub fn prefix_len(&self) -> u8 {
        self.len
    }
}

fn mask(ip: IpAddr, len: u8) -> IpAddr {
    match ip {
        IpAddr::V4(v4) => {
            let bits = u32::from(v4);
            let m = if len == 0 {
                0
            } else {
                u32::MAX << (32 - u32::from(len.min(32)))
            };
            IpAddr::V4(Ipv4Addr::from(bits & m))
        }
        IpAddr::V6(v6) => {
            let bits = u128::from(v6);
            let m = if len == 0 {
                0
            } else {
                u128::MAX << (128 - u32::from(len.min(128)))
            };
            IpAddr::V6(Ipv6Addr::from(bits & m))
        }
    }
}

/// A list of networks.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CidrList(Vec<Cidr>);

impl CidrList {
    /// Parses every entry; entries that are not CIDRs are skipped and returned (the config validator
    /// already rejects them at load, so this only happens for hand-built configs).
    pub fn parse_all<S: AsRef<str>>(entries: &[S]) -> (CidrList, Vec<String>) {
        let mut ok = Vec::with_capacity(entries.len());
        let mut bad = Vec::new();
        for e in entries {
            match Cidr::parse(e.as_ref()) {
                Some(c) => ok.push(c),
                None => bad.push(e.as_ref().to_owned()),
            }
        }
        (CidrList(ok), bad)
    }

    /// `true` when `ip` is inside any network.
    pub fn contains(&self, ip: IpAddr) -> bool {
        self.0.iter().any(|c| c.contains(ip))
    }

    /// `true` for an empty list.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Number of networks.
    pub fn len(&self) -> usize {
        self.0.len()
    }
}
