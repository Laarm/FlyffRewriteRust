//! Semantic validation (Plan/10 §8.4 "ranges" and "cross-key"): the range of every key from the key table,
//! then the cross-key rules. All problems are collected.

use super::error::{ConfigError, ValueSource};
use super::keys::all_keys;
use super::{Config, lookup};
use flyff_game::WallZone;
use std::collections::BTreeMap;
use std::net::{IpAddr, SocketAddr};

/// Parses `a.b.c.d/len` or `v6::/len`; a bare address is a host route (`/32`, `/128`).
pub fn parse_cidr(s: &str) -> Option<(IpAddr, u8)> {
    let (addr, prefix) = match s.trim().split_once('/') {
        Some((a, p)) => (a, Some(p)),
        None => (s.trim(), None),
    };
    let ip: IpAddr = addr.parse().ok()?;
    let max = if ip.is_ipv4() { 32 } else { 128 };
    let len = match prefix {
        Some(p) => p.parse::<u8>().ok().filter(|l| *l <= max)?,
        None => max,
    };
    Some((ip, len))
}

/// `scheme://host[:port]` with an `http` or `https` scheme and nothing after the authority.
pub fn is_origin(s: &str) -> bool {
    let Some(rest) = s
        .strip_prefix("http://")
        .or_else(|| s.strip_prefix("https://"))
    else {
        return false;
    };
    !rest.is_empty() && !rest.contains(['/', '?', '#', ' ', '@'])
}

struct Collector<'a> {
    sources: &'a BTreeMap<String, ValueSource>,
    errs: Vec<ConfigError>,
}

impl Collector<'_> {
    fn err(&mut self, key: &str, message: impl Into<String>) {
        let source = self
            .sources
            .get(key)
            .cloned()
            .unwrap_or(ValueSource::Default);
        self.errs.push(ConfigError::new(key, source, message));
    }

    fn ensure(&mut self, ok: bool, key: &str, message: impl FnOnce() -> String) {
        if !ok {
            self.err(key, message());
        }
    }
}

/// Validates a typed configuration; `sources` attributes each error to where the value came from.
pub fn validate(c: &Config, sources: &BTreeMap<String, ValueSource>) -> Vec<ConfigError> {
    let mut v = Collector {
        sources,
        errs: Vec::new(),
    };

    // ---- per-key ranges from the key table ----
    let table = c.to_table();
    for k in all_keys() {
        if let Some(value) = lookup(&table, k.key)
            && let Some(reason) = k.range.violation(value)
        {
            v.err(k.key, reason);
        }
    }

    // ---- [net] ----
    let n = &c.net;
    v.ensure(n.bind.parse::<SocketAddr>().is_ok(), "net.bind", || {
        format!("'{}' is not a socket address like 0.0.0.0:8080", n.bind)
    });
    v.ensure(
        n.ws_idle_timeout_s > n.ws_ping_interval_s,
        "net.ws_idle_timeout_s",
        || {
            format!(
                "must be greater than net.ws_ping_interval_s ({})",
                n.ws_ping_interval_s
            )
        },
    );
    v.ensure(
        n.client_msgs_burst >= n.client_msgs_per_sec,
        "net.client_msgs_burst",
        || {
            format!(
                "must be >= net.client_msgs_per_sec ({})",
                n.client_msgs_per_sec
            )
        },
    );
    v.ensure(
        n.upgrade_per_ip_burst >= n.upgrade_per_ip_per_sec,
        "net.upgrade_per_ip_burst",
        || {
            format!(
                "must be >= net.upgrade_per_ip_per_sec ({})",
                n.upgrade_per_ip_per_sec
            )
        },
    );
    v.ensure(
        n.outbound_queue_bytes >= u64::from(n.max_batch_bytes),
        "net.outbound_queue_bytes",
        || format!("must be >= net.max_batch_bytes ({})", n.max_batch_bytes),
    );
    v.ensure(
        n.tls.cert.is_empty() == n.tls.key.is_empty(),
        "net.tls.cert",
        || "net.tls.cert and net.tls.key must both be set or both be empty".to_owned(),
    );
    for (i, entry) in n.ip_block.iter().enumerate() {
        v.ensure(parse_cidr(entry).is_some(), "net.ip_block", || {
            format!(
                "entry {} '{entry}' is not a CIDR network like 10.0.0.0/8",
                i + 1
            )
        });
    }
    for (i, entry) in n.allowed_origins.iter().enumerate() {
        v.ensure(is_origin(entry), "net.allowed_origins", || {
            format!(
                "entry {} '{entry}' is not an origin like http://host:8080 (scheme, host, optional port)",
                i + 1
            )
        });
    }

    // ---- [web] / [auth] ----
    for (i, entry) in c.web.trusted_proxies.iter().enumerate() {
        v.ensure(parse_cidr(entry).is_some(), "web.trusted_proxies", || {
            format!("entry {} '{entry}' is not a CIDR network", i + 1)
        });
    }
    for (i, entry) in c.auth.maintenance_allow_ips.iter().enumerate() {
        v.ensure(
            parse_cidr(entry).is_some(),
            "auth.maintenance_allow_ips",
            || {
                format!(
                    "entry {} '{entry}' is not an IP address or CIDR network",
                    i + 1
                )
            },
        );
    }
    v.ensure(
        c.web.req_per_ip_burst >= c.web.req_per_ip_per_sec,
        "web.req_per_ip_burst",
        || {
            format!(
                "must be >= web.req_per_ip_per_sec ({})",
                c.web.req_per_ip_per_sec
            )
        },
    );
    v.ensure(
        c.web.api_per_ip_burst >= c.web.api_per_ip_per_sec,
        "web.api_per_ip_burst",
        || {
            format!(
                "must be >= web.api_per_ip_per_sec ({})",
                c.web.api_per_ip_per_sec
            )
        },
    );
    let url = c.web.register_url.trim();
    v.ensure(
        url.is_empty() || url.starts_with("http://") || url.starts_with("https://"),
        "web.register_url",
        || "must be empty or an http(s) URL".to_owned(),
    );
    let close = c.gameplay.timewait_close_s;
    v.ensure(
        u64::from(c.auth.token_grace_s) >= u64::from(close) + 10,
        "auth.token_grace_s",
        || {
            format!(
                "must be >= gameplay.timewait_close_s + 10 ({})",
                u64::from(close) + 10
            )
        },
    );
    let flush_wait_s = u64::from(c.database.flush_wait_timeout_ms).div_ceil(1000);
    v.ensure(
        u64::from(c.auth.takeover_wait_s) >= u64::from(close) + flush_wait_s,
        "auth.takeover_wait_s",
        || {
            format!(
                "must be >= gameplay.timewait_close_s + ceil(database.flush_wait_timeout_ms / 1000) ({})",
                u64::from(close) + flush_wait_s
            )
        },
    );

    // ---- [gm] (23 §1.3 step 5: a bad grant list refuses the start) ----
    if let Err(problems) = c.gm.typed() {
        for problem in problems {
            v.err("gm.grants", problem);
        }
    }

    // ---- [admin] (25 §1.6, §2.3): the capability overrides and the reason presets ----
    if let Err(problems) = crate::admin::identity::CapTable::with_overrides(&c.admin.capabilities) {
        for problem in problems {
            v.err("admin.capabilities", problem);
        }
    }
    let presets = &c.admin.reason_presets;
    v.ensure(
        !presets.is_empty()
            && presets.len() <= 32
            && presets
                .iter()
                .all(|p| (1..=32).contains(&p.chars().count()) && !p.chars().any(char::is_control)),
        "admin.reason_presets",
        || "must hold 1..=32 entries of 1..=32 printable characters".to_owned(),
    );

    v.ensure(
        (100..=100_000).contains(&c.admin.map_tier_a_m)
            && c.admin.map_tier_a_m <= c.admin.map_tier_b_m,
        "admin.map_tier_a_m",
        || {
            format!(
                "must be in 100..=100000 and <= admin.map_tier_b_m ({})",
                c.admin.map_tier_b_m
            )
        },
    );

    // ---- [server] / [log] ----
    if let Err(e) = WallZone::resolve_with(&c.server.timezone, None, || None) {
        v.err("server.timezone", e.to_string());
    }
    v.ensure(
        tracing_subscriber::EnvFilter::try_new(&c.log.level).is_ok(),
        "log.level",
        || format!("'{}' is not a valid tracing filter", c.log.level),
    );

    // ---- paths that the persistence section names ----
    v.ensure(!c.backup.dir.as_os_str().is_empty(), "backup.dir", || {
        "must not be empty".to_owned()
    });
    v.ensure(!c.audit.dir.as_os_str().is_empty(), "audit.dir", || {
        "must not be empty".to_owned()
    });

    // ---- 13's own rules ([database], [save], [backup], [audit], [retention]) ----
    if let Err(e) = c.persistence().validate() {
        let key = e
            .context
            .split_whitespace()
            .next()
            .unwrap_or("database")
            .to_owned();
        // The key table already reported the same key when it is a plain range; avoid a duplicate.
        if !v.errs.iter().any(|x| x.key == key) {
            let message = e
                .context
                .strip_prefix(&key)
                .map_or_else(|| e.context.clone(), |m| m.trim().to_owned());
            v.err(&key, message);
        }
    }
    v.errs
}

/// Checks that the TLS files named by `net.tls.*` exist and are readable (relative paths resolve against
/// `home`). Called by the loader, which knows the home directory.
pub fn check_tls_files(
    c: &Config,
    home: &std::path::Path,
    sources: &BTreeMap<String, ValueSource>,
) -> Vec<ConfigError> {
    let mut out = Vec::new();
    for (key, value) in [
        ("net.tls.cert", &c.net.tls.cert),
        ("net.tls.key", &c.net.tls.key),
    ] {
        if value.is_empty() {
            continue;
        }
        let path = home.join(value);
        if let Err(e) = std::fs::File::open(&path) {
            out.push(ConfigError::new(
                key,
                sources.get(key).cloned().unwrap_or(ValueSource::Default),
                format!("cannot read {}: {e}", path.display()),
            ));
        }
    }
    out
}
