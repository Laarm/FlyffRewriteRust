//! Projections of [`Config`] for the consumers (Plan/10 §8.5–§8.6, WP-OPS-02): `GameConfig`,
//! `PersistenceConfig`, `VfsConfig`, `DataOptions`, `AssetConfig`, `GatewayConfig`, `LiveConfig` and the
//! absolute paths. The sections of 13 and 03 are passed through unchanged; nothing here re-validates.

use super::Config;
use flyff_common::config::init_only_snapshot;
use flyff_data::DataOptions;
use flyff_db::PersistenceConfig;
use flyff_game::{GameConfig, LiveConfig};
use flyff_vfs::VfsConfig;
use std::path::{Path, PathBuf};

/// Absolute locations derived from the configuration and the home directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPaths {
    /// Home directory.
    pub home: PathBuf,
    /// `Resource/` (the `--resource` flag is already folded into the configuration by the loader).
    pub resource: PathBuf,
    /// `overrides/`.
    pub overrides: PathBuf,
    /// `scripts/npc/`.
    pub npc_scripts: PathBuf,
    /// `cache/`.
    pub cache: PathBuf,
    /// `logs/` (`log.dir`).
    pub logs: PathBuf,
    /// The database file (`database.path`).
    pub database: PathBuf,
    /// `data/` (parent of the database file).
    pub data: PathBuf,
    /// `backups/` (`backup.dir`).
    pub backups: PathBuf,
    /// The audit directory (`audit.dir`).
    pub audit: PathBuf,
    /// The replay directory (`debug.replay_dir`).
    pub replay: PathBuf,
}

/// The `[net]` limits the gateway needs (Plan/10 §7; WP-NET-01 consumes it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayConfig {
    /// `net.max_client_msg_bytes`.
    pub max_client_msg_bytes: u32,
    /// `net.strict_decode`.
    pub strict_decode: bool,
    /// `net.client_msgs_per_sec`.
    pub client_msgs_per_sec: u32,
    /// `net.client_msgs_burst`.
    pub client_msgs_burst: u32,
    /// `net.rate_kick_factor`.
    pub rate_kick_factor: u32,
    /// `net.max_batch_bytes`.
    pub max_batch_bytes: u32,
    /// `net.outbound_queue_frames`.
    pub outbound_queue_frames: u32,
    /// `net.outbound_queue_bytes`.
    pub outbound_queue_bytes: u64,
    /// `net.ws_ping_interval_s`.
    pub ws_ping_interval_s: u32,
    /// `net.ws_idle_timeout_s`.
    pub ws_idle_timeout_s: u32,
    /// `net.timeout_hello_s`.
    pub timeout_hello_s: u32,
    /// `net.timeout_entering_s`.
    pub timeout_entering_s: u32,
    /// `net.timeout_charselect_idle_s`.
    pub timeout_charselect_idle_s: u32,
    /// `net.allowed_origins` (reloadable).
    pub allowed_origins: Vec<String>,
    /// `net.upgrade_per_ip_per_sec`.
    pub upgrade_per_ip_per_sec: u32,
    /// `net.upgrade_per_ip_burst`.
    pub upgrade_per_ip_burst: u32,
    /// `net.max_conns_per_ip`.
    pub max_conns_per_ip: u32,
    /// `net.ip_block` (reloadable).
    pub ip_block: Vec<String>,
    /// `web.trusted_proxies`.
    pub trusted_proxies: Vec<String>,
    /// `log.client_warn_per_min` (the per-session limiter of client-caused WARN lines, 10 §10.4).
    pub client_warn_per_min: u32,
}

/// `assets.gzip_raw`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GzipRaw {
    /// Pre-compress every raw file.
    All,
    /// Compress on first request.
    Lazy,
    /// Never.
    Off,
}

/// `assets.fingerprint_mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FingerprintMode {
    /// Path, size and mtime.
    Stat,
    /// File content.
    Content,
}

/// What the asset pool and the derived-asset builders need (Plan/24; fields of `[runtime]`, `[assets]`,
/// `[web]` and `[paths]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetConfig {
    /// `cache/`.
    pub cache_dir: PathBuf,
    /// `runtime.startup_threads`; 0 = all cores.
    pub startup_threads: u32,
    /// `runtime.asset_threads`; 0 = `max(1, cores - 2)`.
    pub asset_threads: u32,
    /// `runtime.asset_nice`.
    pub nice: u32,
    /// `assets.prewarm`.
    pub prewarm: bool,
    /// `web.public_assets`.
    pub public_assets: bool,
    /// `assets.gzip_raw`.
    pub gzip_raw: GzipRaw,
    /// `assets.disk_budget_mb`.
    pub disk_budget_mb: u32,
    /// `assets.interactive_timeout_s`.
    pub interactive_timeout_s: u32,
    /// `assets.fingerprint_mode`.
    pub fingerprint_mode: FingerprintMode,
    /// `assets.packs = "on"`.
    pub packs: bool,
    /// `assets.pack_shared_min_lands`.
    pub pack_shared_min_lands: u32,
}

impl Config {
    /// Resolves relative paths against `home`.
    pub fn resolve_paths(&self, home: &Path) -> ResolvedPaths {
        let rel = |p: &Path| {
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                home.join(p)
            }
        };
        let relstr = |p: &str| rel(Path::new(p));
        let database = rel(&self.database.path);
        ResolvedPaths {
            home: home.to_path_buf(),
            resource: relstr(&self.paths.resource_dir),
            overrides: relstr(&self.paths.overrides_dir),
            npc_scripts: relstr(&self.paths.npc_scripts_dir),
            cache: relstr(&self.paths.cache_dir),
            logs: relstr(&self.log.dir),
            data: database
                .parent()
                .map_or_else(|| home.join("data"), Path::to_path_buf),
            database,
            backups: rel(&self.backup.dir),
            audit: rel(&self.audit.dir),
            replay: relstr(&self.debug.replay_dir),
        }
    }

    /// The game thread's slice: the flush cadence by the R5 formula, the resolved zone name and the five
    /// rule sections. `zone_name` is the IANA name `server.timezone` resolved to at startup.
    pub fn game_config(&self, zone_name: &str) -> GameConfig {
        GameConfig::new(self.net.flush_interval_ms, zone_name, self.rules())
            .with_max_batch_bytes(self.net.max_batch_bytes as usize)
            .with_save(self.save.clone())
            .with_retention(self.retention.clone())
            // an invalid `[gm]` section is refused at startup (`validate`); the projection falls back to no grants
            .with_gm(self.gm.typed().unwrap_or_default())
            // `[guild] create_requires_quest` (PL-149): the Helena dialog reads QUEST_CREGUILD as completed with `false`
            .with_guild_create_requires_quest(self.guild.create_requires_quest)
    }

    /// The persistence settings with the init-only snapshot of this run (13 §15.1).
    pub fn persistence(&self) -> PersistenceConfig {
        PersistenceConfig {
            database: self.database.clone(),
            save: self.save.clone(),
            backup: self.backup.clone(),
            audit: self.audit.clone(),
            retention: self.retention.clone(),
            init_only: init_only_snapshot(&self.rules()),
        }
    }

    /// The VFS roots (14 §2.1) for `home`.
    pub fn vfs_config(&self, home: &Path) -> VfsConfig {
        let p = self.resolve_paths(home);
        VfsConfig::new(p.resource).with_overrides(p.overrides)
    }

    /// The loading options of the data loader (14 §5.5): the rule sections plus `[data]`.
    pub fn data_options(&self) -> DataOptions {
        DataOptions {
            cp1252_lossy: self.data.cp1252_lossy,
            strict: self.data.strict,
            ..DataOptions::from_rules(&self.rules())
        }
    }

    /// The asset pool and builder settings (Plan/24).
    pub fn asset_config(&self, home: &Path) -> AssetConfig {
        AssetConfig {
            cache_dir: self.resolve_paths(home).cache,
            startup_threads: self.runtime.startup_threads,
            asset_threads: self.runtime.asset_threads,
            nice: self.runtime.asset_nice,
            prewarm: self.assets.prewarm,
            public_assets: self.web.public_assets,
            gzip_raw: match self.assets.gzip_raw.as_str() {
                "lazy" => GzipRaw::Lazy,
                "off" => GzipRaw::Off,
                _ => GzipRaw::All,
            },
            disk_budget_mb: self.assets.disk_budget_mb,
            interactive_timeout_s: self.assets.interactive_timeout_s,
            fingerprint_mode: if self.assets.fingerprint_mode == "content" {
                FingerprintMode::Content
            } else {
                FingerprintMode::Stat
            },
            packs: self.assets.packs == "on",
            pack_shared_min_lands: self.assets.pack_shared_min_lands,
        }
    }

    /// The gateway limits (Plan/10 §7).
    pub fn gateway_config(&self) -> GatewayConfig {
        let n = &self.net;
        GatewayConfig {
            max_client_msg_bytes: n.max_client_msg_bytes,
            strict_decode: n.strict_decode,
            client_msgs_per_sec: n.client_msgs_per_sec,
            client_msgs_burst: n.client_msgs_burst,
            rate_kick_factor: n.rate_kick_factor,
            max_batch_bytes: n.max_batch_bytes,
            outbound_queue_frames: n.outbound_queue_frames,
            outbound_queue_bytes: n.outbound_queue_bytes,
            ws_ping_interval_s: n.ws_ping_interval_s,
            ws_idle_timeout_s: n.ws_idle_timeout_s,
            timeout_hello_s: n.timeout_hello_s,
            timeout_entering_s: n.timeout_entering_s,
            timeout_charselect_idle_s: n.timeout_charselect_idle_s,
            allowed_origins: n.allowed_origins.clone(),
            upgrade_per_ip_per_sec: n.upgrade_per_ip_per_sec,
            upgrade_per_ip_burst: n.upgrade_per_ip_burst,
            max_conns_per_ip: n.max_conns_per_ip,
            ip_block: n.ip_block.clone(),
            trusted_proxies: self.web.trusted_proxies.clone(),
            client_warn_per_min: self.log.client_warn_per_min,
        }
    }

    /// The initial `LiveConfig` (generation 0: rates, events, empty `event_flags`); `runtime_override`
    /// rows are applied on top by WP-OPS-20.
    pub fn live_config(&self) -> LiveConfig {
        LiveConfig::from_rules(&self.rules())
    }
}
