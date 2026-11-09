//! The game thread's slice of the configuration (Plan/10 §8; derived from `Config` by `flyff-server`,
//! WP-OPS-02 `GameConfig` projection).

use flyff_common::config::RulesConfig;
use flyff_common::gm::GmConfig;
use flyff_db::config::{RetentionConfig, SaveConfig};

/// `net.max_batch_bytes` default (11 §1.2): 1 MiB.
pub const DEFAULT_MAX_BATCH_BYTES: usize = 1_048_576;

/// Startup-class settings of the game thread.
#[derive(Clone, Debug, PartialEq)]
pub struct GameConfig {
    /// Flush the outboxes every N ticks: `max(1, round(net.flush_interval_ms · 15 / 1000))` (R5).
    pub flush_every_ticks: u32,
    /// Seconds without a completed shutdown flush after which the game exits anyway.
    pub shutdown_flush_timeout_s: u32,
    /// `net.max_batch_bytes`: a frame of one session is split at message boundaries above this (11 §1.2).
    pub max_batch_bytes: usize,
    /// The resolved `server.timezone`: an IANA name (the server resolves `"local"` once at start).
    pub timezone: String,
    /// `[features]`, `[gameplay]`, `[fidelity]` (startup class) and the initial `[rates]`/`[events]`.
    pub rules: RulesConfig,
    /// `[save]` (Plan/13 §2.4, §17): the cadence of the periodic, urgent and world-change snapshots.
    pub save: SaveConfig,
    /// `[retention]` (Plan/13 §2.4, §23): the hour of the daily maintenance job and the age at which a
    /// soft-deleted character is purged (`persist::maintenance`, WP-DB-16).
    pub retention: RetentionConfig,
    /// `[gm]` (Plan/23 §1.3): the grants that override the database authority. The lobby resolves the authority
    /// of a login with them; the game thread reads them for `/setauth` and `AdminCmd::AuthorityChanged`
    /// (character selectors only: it does not know account names or addresses).
    pub gm: GmConfig,
    /// `guild.create_requires_quest` (restart class, Plan/21 §5.4, PL-149): with `false` (default) the Helena dialog
    /// reads `QUEST_CREGUILD` as completed. It lives in the server's `[guild]` section, not in `RulesConfig`.
    pub guild_create_requires_quest: bool,
}

impl Default for GameConfig {
    fn default() -> Self {
        Self::from_flush_interval_ms(67)
    }
}

impl GameConfig {
    /// The R5 formula `flush_every = max(1, round(net.flush_interval_ms · 15 / 1000))` (Plan/10 §6.4,
    /// normative): 67 → 1, 133 → 2, 200 → 3. Zone UTC, default rules.
    pub fn from_flush_interval_ms(ms: u32) -> Self {
        let every = (u64::from(ms) * 15 + 500) / 1000;
        Self {
            flush_every_ticks: u32::try_from(every.max(1)).unwrap_or(u32::MAX),
            max_batch_bytes: DEFAULT_MAX_BATCH_BYTES,
            shutdown_flush_timeout_s: 30,
            timezone: "UTC".to_owned(),
            rules: RulesConfig::default(),
            save: SaveConfig::default(),
            retention: RetentionConfig::default(),
            gm: GmConfig::default(),
            guild_create_requires_quest: false,
        }
    }

    /// The same configuration with another `[gm]` section.
    pub fn with_gm(mut self, gm: GmConfig) -> Self {
        self.gm = gm;
        self
    }

    /// The same configuration with another `guild.create_requires_quest`.
    pub fn with_guild_create_requires_quest(mut self, requires: bool) -> Self {
        self.guild_create_requires_quest = requires;
        self
    }

    /// The same configuration with another `net.max_batch_bytes`.
    pub fn with_max_batch_bytes(mut self, max_batch_bytes: usize) -> Self {
        self.max_batch_bytes = max_batch_bytes.max(1);
        self
    }

    /// The same configuration with another `[save]` section.
    pub fn with_save(mut self, save: SaveConfig) -> Self {
        self.save = save;
        self
    }

    /// The same configuration with another `[retention]` section.
    pub fn with_retention(mut self, retention: RetentionConfig) -> Self {
        self.retention = retention;
        self
    }

    /// The projection `flyff-server` builds: flush cadence from `net.flush_interval_ms`, the resolved zone
    /// name and the five rule sections.
    pub fn new(flush_interval_ms: u32, timezone: impl Into<String>, rules: RulesConfig) -> Self {
        Self {
            timezone: timezone.into(),
            rules,
            ..Self::from_flush_interval_ms(flush_interval_ms)
        }
    }
}
