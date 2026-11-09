//! The reloadable lists of the HTTP layer: `net.ip_block` (layer 0 of Plan/24 §3.3, F-007) and
//! `net.allowed_origins` (§3.5). Both are reloadable keys: [`LiveLists::set_ip_block`] and
//! [`LiveLists::set_allowed_origins`] swap the list for the next request; readers take a snapshot `Arc`.
//!
//! **The admin IP blocks** (Plan/25 §4.8, WP-ADMIN-17) are a second list merged into the same check: the rows of
//! the `admin_ip_blocks` table, each with an optional end time. [`LiveLists::set_admin_blocks`] swaps the whole
//! list (at start and after every add or remove of the console), a block that ran out stops matching by itself
//! (`until` is compared with the clock at every check), and a `net.ip_block` reload leaves it untouched.

use super::cidr::{Cidr, CidrList};
use parking_lot::RwLock;
use std::net::IpAddr;
use std::sync::Arc;

/// The `admin_ip_blocks` rows: the network and the unix second it ends (`None` = for ever).
type AdminBlocks = Arc<Vec<(Cidr, Option<i64>)>>;

/// The swappable lists.
#[derive(Debug, Default)]
pub struct LiveLists {
    block: RwLock<Arc<CidrList>>,
    admin: RwLock<AdminBlocks>,
    origins: RwLock<Arc<Vec<String>>>,
}

impl LiveLists {
    /// Lists with these initial values; CIDR entries that do not parse are returned (the config validator
    /// rejects them at load, so this only reports hand-built configs).
    pub fn new(ip_block: &[String], allowed_origins: &[String]) -> (LiveLists, Vec<String>) {
        let lists = LiveLists::default();
        let bad = lists.set_ip_block(ip_block);
        lists.set_allowed_origins(allowed_origins);
        (lists, bad)
    }

    /// Replaces `net.ip_block`; returns the entries that are not CIDR networks (skipped).
    pub fn set_ip_block(&self, entries: &[String]) -> Vec<String> {
        let (list, bad) = CidrList::parse_all(entries);
        *self.block.write() = Arc::new(list);
        bad
    }

    /// Replaces `net.allowed_origins`.
    pub fn set_allowed_origins(&self, origins: &[String]) {
        *self.origins.write() = Arc::new(origins.to_vec());
    }

    /// Replaces the admin IP blocks (`(cidr, until)` per row); returns the entries that are not CIDR networks
    /// (skipped).
    pub fn set_admin_blocks(&self, rows: &[(String, Option<i64>)]) -> Vec<String> {
        let mut bad = Vec::new();
        let mut list = Vec::with_capacity(rows.len());
        for (text, until) in rows {
            match Cidr::parse(text) {
                Some(c) => list.push((c, *until)),
                None => bad.push(text.clone()),
            }
        }
        *self.admin.write() = Arc::new(list);
        bad
    }

    /// How many admin IP blocks are loaded (running out or not).
    pub fn admin_block_count(&self) -> usize {
        self.admin.read().len()
    }

    /// `true` when `ip` is in a blocked network (`net.ip_block` or an admin block in force now). A loopback client is
    /// never blocked.
    pub fn is_blocked(&self, ip: IpAddr) -> bool {
        self.is_blocked_at(ip, unix_now())
    }

    /// [`LiveLists::is_blocked`] at the unix second `now`.
    pub fn is_blocked_at(&self, ip: IpAddr, now: i64) -> bool {
        let ip = ip.to_canonical();
        if ip.is_loopback() {
            return false;
        }
        self.block.read().contains(ip)
            || self
                .admin
                .read()
                .iter()
                .any(|(c, until)| until.is_none_or(|u| u > now) && c.contains(ip))
    }

    /// The current `net.allowed_origins`.
    pub fn allowed_origins(&self) -> Arc<Vec<String>> {
        Arc::clone(&self.origins.read())
    }
}

/// The wall clock in unix seconds (the admin blocks end by it).
fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}
