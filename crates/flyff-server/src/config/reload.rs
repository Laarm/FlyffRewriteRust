//! The pure part of a configuration reload (Plan/10 §8.6, WP-OPS-16): which changed key is applied on the tokio side
//! and by which holder ([`Applier`]), which one is a runtime-class key for the game thread, and which one waits for a
//! restart; the key-by-key comparison of two configurations ([`diff`]) and the merge that keeps the running value of
//! every key that is not applied ([`merge`]). `admin/reload.rs` does the I/O around it.
//!
//! The applier table is the single source: [`applier_of`] names the holder of every `Reload::Yes` key that is not a
//! runtime key, and the test `every_reload_yes_key_has_an_applier` fails for a new key without one.

use super::keys::{self, KeyInfo};
use super::schema::Reload;
use super::{Config, lookup};

/// The holder that takes the new value of a reloaded key (all of them swap an `Arc` snapshot or an atomic: readers
/// see the old or the new value, never a mix).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Applier {
    /// `net.ip_block`: `http::live::LiveLists::set_ip_block`.
    IpBlock,
    /// `net.allowed_origins`: `LiveLists::set_allowed_origins` and `GatewayShared::set_allowed_origins`.
    Origins,
    /// The six reloadable `[auth]` keys (12 §4): `LobbyCore::reload` with a new `LobbySettings`.
    Lobby,
    /// Every `admin.*` and `admin.alerts.*` reload key except the ones read per request: `AdminShared::apply_config`.
    Admin,
    /// `log.level`: `LogHandles::set_filter` (unless `RUST_LOG` is set).
    LogFilter,
    /// `log.client_warn_per_min`: the atomic of the gateway.
    ClientWarn,
    /// `server.shutdown_countdown_s`: `ShutdownCoordinator::set_default_countdown`.
    Countdown,
    /// A key that is read per request from `AdminOps::current` (`admin.allow_backup_download_remote`): replacing
    /// `current` is all there is to do.
    Current,
}

/// How a changed key takes effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReloadClass {
    /// Applied now on the tokio side by this holder.
    Apply(Applier),
    /// A runtime-class key (`[rates]`, `[events]`): the game thread takes it (`GameInput::ReloadLive`) and keeps an
    /// override in force.
    Runtime,
    /// Takes effect at the next start: reported, never applied.
    Restart,
}

/// The six `[auth]` keys that `LobbySettings` carries live (12 §4).
const LOBBY_KEYS: [&str; 6] = [
    "auth.allow_registration",
    "auth.delete_requires_password",
    "auth.max_in_world",
    "auth.register_per_ip_per_hour",
    "auth.maintenance",
    "auth.maintenance_allow_ips",
];

/// The holder of the reload-`yes` key `key`, `None` for a key without one (a restart key, or a runtime key, which
/// the game thread owns). Only meaningful together with `Reload::Yes`: use [`class_of`].
pub fn applier_of(key: &str) -> Option<Applier> {
    match key {
        "net.ip_block" => Some(Applier::IpBlock),
        "net.allowed_origins" => Some(Applier::Origins),
        "log.level" => Some(Applier::LogFilter),
        "log.client_warn_per_min" => Some(Applier::ClientWarn),
        "server.shutdown_countdown_s" => Some(Applier::Countdown),
        "admin.allow_backup_download_remote" => Some(Applier::Current),
        k if LOBBY_KEYS.contains(&k) => Some(Applier::Lobby),
        k if k.starts_with("admin.") => Some(Applier::Admin),
        _ => None,
    }
}

/// The class of a key of the table: runtime keys go to the game, a `Reload::Yes` key with an applier is applied, every
/// other key needs a restart (`gm.grants`, `features.*`, `gameplay.*`, `fidelity.*`, `log.file_*`, `net.bind`, ...).
pub fn class_of(info: &KeyInfo) -> ReloadClass {
    if info.runtime {
        return ReloadClass::Runtime;
    }
    if info.reload == Reload::Yes
        && let Some(a) = applier_of(info.key)
    {
        return ReloadClass::Apply(a);
    }
    ReloadClass::Restart
}

/// The keys whose value differs between two configurations, by class, in template order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReloadDiff {
    /// Reload-`yes` keys with their holder.
    pub apply: Vec<(&'static str, Applier)>,
    /// Runtime-class keys.
    pub runtime: Vec<&'static str>,
    /// Keys that need a restart.
    pub restart: Vec<&'static str>,
}

impl ReloadDiff {
    /// `true` when no key differs.
    pub fn is_empty(&self) -> bool {
        self.apply.is_empty() && self.runtime.is_empty() && self.restart.is_empty()
    }

    /// The holders that have something to apply, each once, in a fixed order.
    pub fn appliers(&self) -> Vec<Applier> {
        let mut out: Vec<Applier> = self.apply.iter().map(|(_, a)| *a).collect();
        out.sort();
        out.dedup();
        out
    }
}

/// Compares `old` and `new` key by key over [`keys::all_keys`] (the effective values: a key pinned by `FLYFF__*`, a
/// command-line flag or `ServerOptions::config_overrides` holds the same value in both, so it never differs).
pub fn diff(old: &Config, new: &Config) -> ReloadDiff {
    let (old_table, new_table) = (old.to_table(), new.to_table());
    let mut out = ReloadDiff::default();
    for info in keys::all_keys() {
        if lookup(&old_table, info.key) == lookup(&new_table, info.key) {
            continue;
        }
        match class_of(info) {
            ReloadClass::Apply(a) => out.apply.push((info.key, a)),
            ReloadClass::Runtime => out.runtime.push(info.key),
            ReloadClass::Restart => out.restart.push(info.key),
        }
    }
    out
}

/// `old` with the values of `keys` taken from `new` (the keys that are applied; every other key keeps the running
/// value, which is why a second reload lists a pending restart key again). A merged table that does not deserialise
/// is an error and nothing may be applied.
pub fn merge(old: &Config, new: &Config, keys: &[&str]) -> Result<Config, String> {
    let mut table = old.to_table();
    let new_table = new.to_table();
    for key in keys {
        set_path(&mut table, key, lookup(&new_table, key).cloned());
    }
    finish(table)
}

fn finish(table: toml::Table) -> Result<Config, String> {
    toml::Value::Table(table)
        .try_into::<Config>()
        .map_err(|e| format!("the merged configuration does not load: {e}"))
}

fn set_path(table: &mut toml::Table, dotted: &str, value: Option<toml::Value>) {
    let mut parts: Vec<&str> = dotted.split('.').collect();
    let Some(last) = parts.pop() else {
        return;
    };
    let mut cur = table;
    for p in parts {
        let entry = cur
            .entry(p.to_owned())
            .or_insert_with(|| toml::Value::Table(toml::Table::new()));
        match entry {
            toml::Value::Table(t) => cur = t,
            _ => return,
        }
    }
    match value {
        Some(v) => {
            cur.insert(last.to_owned(), v);
        }
        None => {
            cur.remove(last);
        }
    }
}
