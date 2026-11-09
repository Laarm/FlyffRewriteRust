//! [`AdminShared`]: the state of the admin console that is not a request (Plan/25 §1, §11.8, §12; WP-ADMIN-01/02).
//!
//! It owns the session table, the capability table, the live guard policy, the per-identity limits, the idempotency
//! guard, the account-event channel and the counters, and is built once at start from the configuration
//! ([`AdminShared::from_config`]). `AdminOps` carries it; `reload` calls [`AdminShared::apply_config`] for the live
//! keys (`admin.session_idle_min`, `admin.session_max_h`, `admin.reason_presets`, `admin.mutations_per_s`,
//! `admin.confirm_*`, `admin.max_objects_per_action`, `admin.max_ws_per_identity`, `admin.max_map_subs_*`,
//! `admin.totp_required`).

use super::account_events::AccountEvents;
use super::guard::{Idempotency, LivePolicy, Policy, StreamPermits, TokenBuckets};
use super::identity::{CapTable, StaffResolver};
use super::metrics::AdminMetrics;
use super::session::{AdminSessions, AssetGate, SessionSettings};
use crate::config::Config;
use crate::lobby::online::LobbyClock;
use flyff_common::gm::GmConfig;
use flyff_db::PersistHandle;
use std::sync::Arc;

/// The shared state of the admin console.
pub struct AdminShared {
    /// The sessions behind the `flyff_admin` cookie.
    pub sessions: Arc<AdminSessions>,
    /// The counters of the console.
    pub metrics: Arc<AdminMetrics>,
    /// A ban, an authority change or a password change of an account ends its sessions (25 §1.8).
    pub events: AccountEvents,
    /// The minimum letter of every capability (with the `[admin.capabilities]` overrides).
    pub caps: CapTable,
    /// The live values the guards read.
    pub policy: LivePolicy,
    /// The authority of staff sessions and of rank-protection targets.
    pub resolver: StaffResolver,
    /// The mutation buckets (`admin.mutations_per_s`).
    pub buckets: TokenBuckets,
    /// The streaming permits (one stream per identity).
    pub streams: Arc<StreamPermits>,
    /// The `Idempotency-Key` guard.
    pub idem: Idempotency,
    /// The admin WebSocket gateway's shared state: limits, counters, connection ids (`ws.rs`).
    pub ws: Arc<super::ws::WsHub>,
    /// The dashboard: metrics history, alerts, the activity feed (WP-ADMIN-25).
    pub dash: Arc<super::history::Dash>,
    /// The log ring buffer behind `LogSubscribe` (WP-ADMIN-26): the process-wide ring unless a test installs its own.
    pub logs: Arc<super::logs::LogRing>,
    /// What the items area keeps between requests: the catalogue metadata and the economy cache (WP-ADMIN-20/22).
    pub items: super::api::items::ItemsState,
    /// The audit viewer's store, created at the first query (WP-ADMIN-29).
    pub audit_store: std::sync::OnceLock<Arc<super::audit_index::AuditStore>>,
    /// The process supervisor, when one started the server (`systemd`): what makes "restart" meaningful (25 §9.7).
    pub supervisor: Option<&'static str>,
    /// A restart was asked for: the exit code is `exit::RESTART` (a supervisor starts the server again).
    pub restart_requested: std::sync::atomic::AtomicBool,
    /// The clock of sessions and buckets (tests step it).
    pub clock: LobbyClock,
    /// `admin.staff_login`.
    pub staff_login: bool,
    /// `admin.owner_loopback`.
    pub owner_loopback: bool,
    /// `admin.allow_remote`.
    pub allow_remote: bool,
    /// Staff TOTP: pending setups, the replay rule and the per-account lock (WP-ADMIN-28).
    pub totp: super::totp::TotpGuard,
    /// `admin.totp_required` (live): remote staff sign-ins need a code (25 §1.3).
    pub totp_required: std::sync::atomic::AtomicBool,
}

impl std::fmt::Debug for AdminShared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdminShared").finish_non_exhaustive()
    }
}

/// The inputs of [`AdminShared::from_config`] that are not configuration.
pub struct SharedInit {
    /// The persistence handle (the idempotency rows).
    pub persist: PersistHandle,
    /// The clock.
    pub clock: LobbyClock,
    /// The asset-cookie side of sessions, where the server can issue them.
    pub assets: Option<AssetGate>,
    /// The channel the lobby's `AccountAdmin` publishes to (shared so the sessions end on its events).
    pub events: AccountEvents,
}

/// The policy values of a configuration.
pub fn policy_of(cfg: &Config) -> Policy {
    let a = &cfg.admin;
    Policy {
        reason_presets: a.reason_presets.clone(),
        confirm_gold_threshold: a.confirm_gold_threshold,
        confirm_item_threshold: a.confirm_item_threshold,
        max_objects_per_action: a.max_objects_per_action,
        mutations_per_s: a.mutations_per_s,
    }
}

impl AdminShared {
    /// Builds the state from the configuration. A bad `[admin.capabilities]` table is reported (the loader refuses the
    /// start with the same messages).
    pub fn from_config(cfg: &Config, init: SharedInit) -> Result<Arc<AdminShared>, Vec<String>> {
        let caps = CapTable::with_overrides(&cfg.admin.capabilities)?;
        let gm: GmConfig = cfg.gm.typed().unwrap_or_default();
        let sessions = AdminSessions::new(
            init.clock.clone(),
            SessionSettings::from_minutes_hours(
                cfg.admin.session_idle_min,
                cfg.admin.session_max_h,
            ),
            init.assets,
        );
        let ws = super::ws::WsHub::new(super::ws::WsHub::limits_of(cfg));
        ws.set_view_limits(super::ws::WsHub::view_limits_of(cfg));
        Ok(Arc::new(AdminShared {
            sessions,
            metrics: Arc::new(AdminMetrics::default()),
            events: init.events,
            caps,
            policy: LivePolicy::new(policy_of(cfg)),
            resolver: StaffResolver::new(&gm, cfg.features.servers_authorize),
            buckets: TokenBuckets::default(),
            streams: Arc::new(StreamPermits::default()),
            idem: Idempotency::new(init.persist, init.clock.clone()),
            ws,
            dash: Arc::new(super::history::Dash::new(super::alerts::AlertConfig::of(
                cfg,
            ))),
            logs: Arc::clone(super::logs::global()),
            items: super::api::items::ItemsState::default(),
            audit_store: std::sync::OnceLock::new(),
            supervisor: super::api::server::supervisor_of_process(),
            restart_requested: std::sync::atomic::AtomicBool::new(false),
            clock: init.clock,
            staff_login: cfg.admin.staff_login,
            owner_loopback: cfg.admin.owner_loopback,
            allow_remote: cfg.admin.allow_remote,
            totp: super::totp::TotpGuard::default(),
            totp_required: std::sync::atomic::AtomicBool::new(cfg.admin.totp_required),
        }))
    }

    /// Applies the live `[admin]` keys of a reloaded configuration.
    pub fn apply_config(&self, cfg: &Config) {
        self.sessions
            .set_settings(SessionSettings::from_minutes_hours(
                cfg.admin.session_idle_min,
                cfg.admin.session_max_h,
            ));
        self.policy.set(policy_of(cfg));
        self.ws.apply_config(cfg);
        self.dash
            .alerts
            .set_config(super::alerts::AlertConfig::of(cfg));
        self.totp_required.store(
            cfg.admin.totp_required,
            std::sync::atomic::Ordering::Relaxed,
        );
    }
}
