//! `AdminOps::reload`: the one algorithm behind console `reload`, `POST /admin/api/reload`, SIGHUP and the GM
//! command `/reload config` (Plan/10 §8.6, §13; WP-OPS-16), and its read-only twin [`AdminOps::reload_preview`]
//! (`GET /admin/api/reload`, the "Compare with flyff.toml" panel of Plan/25 §9.5).
//!
//! 1. one reload at a time (`reload_lock`: a SIGHUP storm queues, nothing is coalesced);
//! 2. `config::load` of the same sources `serve` used, on a blocking thread; any error: nothing is applied;
//! 3. [`config::reload::diff`] sorts the differing keys into *apply* (a tokio-side holder), *runtime* (`[rates]`,
//!    `[events]`, for the game thread) and *restart*;
//! 4. the runtime keys: the game says which of them an override holds (`overridden`, kept) and takes the file values
//!    in exactly one `GameInput::ReloadLive` (the game rebases the overrides on top, `Game::on_reload_live`);
//! 5. `merged` = the running configuration with the applied keys taken from the file; every holder is applied once
//!    from `merged` (never from the raw file: a restart-class key of the same edit must not leak in);
//! 6. `current` becomes `merged`, the log gets the summary line, the audit `server.config_reload`.

use crate::admin::ops::{AdminOps, ReloadReport, SEND_TIMEOUT};
use crate::audit::Actor;
use crate::config::reload::{self as classes, Applier};
use crate::config::{self, Config};
use crate::lobby::LobbySettings;
use flyff_game::input::{AdminActor, AdminCmd, AdminOutput};
use flyff_game::{GameInput, LiveConfig, LiveOrigin};
use serde_json::json;
use std::net::IpAddr;
use std::sync::Arc;
use std::sync::atomic::Ordering;

/// Who asked for the reload (the `by` of the `server.config_reload` audit record, in the style of
/// `server.shutdown_requested`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReloadBy {
    /// The stdin console.
    Console,
    /// A CLI subcommand.
    Cli,
    /// The admin API (`POST /admin/api/reload`, `POST /admin/api/console`) from this address.
    AdminApi {
        /// The client address.
        ip: IpAddr,
    },
    /// `SIGHUP`.
    Signal,
    /// The GM command `/reload config`: the issuer label exactly as the GM layer gives it (`gm:<account id>`,
    /// `console`, `owner:local`, `staff:<account>`, ...).
    Gm(String),
}

impl ReloadBy {
    /// `console`, `cli`, `admin-api:<ip>`, `signal:SIGHUP` or the GM label verbatim.
    pub fn label(&self) -> String {
        match self {
            ReloadBy::Console => "console".to_owned(),
            ReloadBy::Cli => "cli".to_owned(),
            ReloadBy::AdminApi { ip } => format!("admin-api:{ip}"),
            ReloadBy::Signal => "signal:SIGHUP".to_owned(),
            ReloadBy::Gm(label) => label.clone(),
        }
    }

    /// The audit actor: the address of an HTTP caller, else nothing.
    pub fn audit_actor(&self) -> Actor {
        match self {
            ReloadBy::AdminApi { ip } => Actor::ip(ip),
            _ => Actor::none(),
        }
    }
}

impl From<AdminActor> for ReloadBy {
    fn from(actor: AdminActor) -> Self {
        match actor {
            AdminActor::Console => ReloadBy::Console,
            AdminActor::Cli => ReloadBy::Cli,
            AdminActor::Owner { remote, .. } | AdminActor::Staff { remote, .. } => {
                ReloadBy::AdminApi { ip: remote }
            }
        }
    }
}

/// How many key names a group of the summary line lists before it says "+N more".
const LISTED_KEYS: usize = 6;

fn group(label: &str, keys: &[String]) -> Option<String> {
    if keys.is_empty() {
        return None;
    }
    let mut names: Vec<&str> = keys.iter().take(LISTED_KEYS).map(String::as_str).collect();
    let more = keys.len().saturating_sub(LISTED_KEYS);
    let more_text = format!("+{more} more");
    if more > 0 {
        names.push(&more_text);
    }
    Some(format!("{label} {} ({})", keys.len(), names.join(", ")))
}

/// The log line and the answer of the GM command (`reload: changed 2 (auth.max_in_world, rates.monster_exp);
/// overridden 1 (rates.item_drop); restart required 1 (net.bind)`; `reload: nothing changed`; with errors
/// `reload failed: <first error> (+N more)`, followed by what was applied anyway). Each list is cut after six names.
pub fn summary_line(report: &ReloadReport) -> String {
    let groups: Vec<String> = [
        group("changed", &report.changed),
        group("overridden", &report.overridden),
        group("restart required", &report.restart_required),
    ]
    .into_iter()
    .flatten()
    .collect();
    let Some(first) = report.errors.first() else {
        return if groups.is_empty() {
            "reload: nothing changed".to_owned()
        } else {
            format!("reload: {}", groups.join("; "))
        };
    };
    let mut line = format!("reload failed: {first}");
    if report.errors.len() > 1 {
        line.push_str(&format!(" (+{} more)", report.errors.len() - 1));
    }
    if !groups.is_empty() {
        // some keys were applied although another part failed: say so
        line.push_str(&format!("; applied: {}", groups.join("; ")));
    }
    line
}

/// The lines of the console word `reload` and of the page: `error: ...`, `changed: ...` (`-` when nothing),
/// `kept by an override: ...`, `restart required: ...`, `warning: ...`.
pub fn report_lines(report: &ReloadReport) -> Vec<String> {
    let mut lines: Vec<String> = report
        .errors
        .iter()
        .map(|e| format!("error: {e}"))
        .collect();
    lines.push(format!(
        "changed: {}",
        if report.changed.is_empty() {
            "-".to_owned()
        } else {
            report.changed.join(", ")
        }
    ));
    if !report.overridden.is_empty() {
        lines.push(format!(
            "kept by an override: {}",
            report.overridden.join(", ")
        ));
    }
    if !report.restart_required.is_empty() {
        lines.push(format!(
            "restart required: {}",
            report.restart_required.join(", ")
        ));
    }
    lines.extend(report.warnings.iter().map(|w| format!("warning: {w}")));
    lines
}

fn owned(keys: &[&str]) -> Vec<String> {
    keys.iter().map(|k| (*k).to_owned()).collect()
}

/// What the loader found: the new configuration and its warnings (retired keys, reserved keys).
struct Loaded {
    config: Config,
    warnings: Vec<String>,
}

impl AdminOps {
    /// Re-reads `flyff.toml` (and the environment, the flags and the start options: the pins win over the file) and
    /// applies it as described in the module documentation. The keys that are applied now: `net.ip_block`,
    /// `net.allowed_origins`, the six reloadable `[auth]` keys, `log.level`, `log.client_warn_per_min`,
    /// `server.shutdown_countdown_s`, the live `[admin]` keys, and the runtime-class keys (`[rates]`, `[events]`)
    /// unless an override is in force. Every other changed key is `restart_required`. A file that does not load
    /// changes nothing. Returns after the `ReloadLive` is in the game inbox, so whatever the caller sends through
    /// `game_tx` afterwards is handled after it.
    pub async fn reload(&self, by: ReloadBy) -> ReloadReport {
        let _one_at_a_time = self.reload_lock.lock().await;
        let report = self.reload_locked().await;
        for key in &report.restart_required {
            tracing::warn!("restart required for {key}");
        }
        for w in &report.warnings {
            tracing::warn!("flyff.toml: {w}");
        }
        if report.errors.is_empty() {
            tracing::info!("{}", summary_line(&report));
        } else {
            tracing::warn!("{}", summary_line(&report));
        }
        // `server.config_reload` after every reload, successful or not (Plan/10 §11)
        self.audit.emit(
            "server.config_reload",
            &by.audit_actor(),
            json!({
                "changed_keys": report.changed,
                "overridden_keys": report.overridden,
                "restart_required_keys": report.restart_required,
                "errors": report.errors,
                "by": by.label(),
            }),
        );
        report
    }

    /// What a reload would do now, without doing it (`GET /admin/api/reload`): the same report, nothing sent,
    /// merged, applied, logged or audited.
    pub async fn reload_preview(&self) -> ReloadReport {
        let _one_at_a_time = self.reload_lock.lock().await;
        let mut report = ReloadReport::default();
        let loaded = match self.load_new().await {
            Ok(l) => l,
            Err(errors) => {
                report.errors = errors;
                return report;
            }
        };
        report.warnings = loaded.warnings;
        let old = Arc::clone(&self.current.lock());
        let diff = classes::diff(&old, &loaded.config);
        report.changed = diff.apply.iter().map(|(k, _)| (*k).to_owned()).collect();
        report.restart_required = owned(&diff.restart);
        if !diff.runtime.is_empty() {
            match self.classify_runtime(&diff.runtime).await {
                Ok((changed, overridden)) => {
                    report.changed.extend(changed);
                    report.overridden = overridden;
                }
                Err(e) => report.errors.push(runtime_unreachable(&e.to_string())),
            }
        }
        report
    }

    /// `config::load` of the reload source on a blocking thread (the lock is its single permit).
    async fn load_new(&self) -> Result<Loaded, Vec<String>> {
        let src = self.reload_source.clone();
        let loaded = tokio::task::spawn_blocking(move || {
            config::load(&src.home, &src.cli, &src.env, &src.overrides)
        })
        .await;
        match loaded {
            Ok(Ok(l)) => Ok(Loaded {
                config: l.config,
                warnings: l.warnings.iter().map(ToString::to_string).collect(),
            }),
            Ok(Err(errors)) => Err(errors.iter().map(ToString::to_string).collect()),
            Err(e) => Err(vec![format!(
                "the configuration loader did not finish: {e}"
            )]),
        }
    }

    /// Splits the differing runtime keys into the ones that take the file value now and the ones an override
    /// holds, by asking the game (a `set` between this read and the game's rebase only makes the report stale: the
    /// game rebases with the overrides in force at its tick).
    async fn classify_runtime(
        &self,
        runtime: &[&str],
    ) -> Result<(Vec<String>, Vec<String>), crate::admin::OpsError> {
        let rates = match self.game(AdminCmd::GetRates, AdminActor::Console).await? {
            AdminOutput::Rates(r) => r,
            _ => Vec::new(),
        };
        let events = match self.game(AdminCmd::GetEvents, AdminActor::Console).await? {
            AdminOutput::Events(e) => e,
            _ => Vec::new(),
        };
        let (mut changed, mut overridden) = (Vec::new(), Vec::new());
        for key in runtime {
            let held = rates
                .iter()
                .any(|r| r.config_key == *key && r.overridden.is_some())
                || events.iter().any(|e| e.key == Some(*key) && e.overridden);
            if held {
                overridden.push((*key).to_owned());
            } else {
                changed.push((*key).to_owned());
            }
        }
        Ok((changed, overridden))
    }

    /// The one `GameInput::ReloadLive` of a reload: the file's `[rates]` and `[events]` (every key of both sections
    /// is a runtime key, so these are the values `merged` holds afterwards).
    async fn send_runtime(&self, new: &Config) -> Result<(), crate::admin::OpsError> {
        let live = LiveConfig {
            generation: 0,
            rates: new.rates.clone(),
            events: new.events.clone(),
            event_flags: std::collections::BTreeMap::new(),
            origin: LiveOrigin::FileReload,
        };
        match tokio::time::timeout(
            SEND_TIMEOUT,
            self.game_tx
                .send_async(GameInput::ReloadLive(Arc::new(live))),
        )
        .await
        {
            Ok(Ok(())) => Ok(()),
            Ok(Err(_)) => Err(crate::admin::OpsError::GameGone),
            Err(_) => Err(crate::admin::OpsError::Timeout),
        }
    }

    async fn reload_locked(&self) -> ReloadReport {
        let mut report = ReloadReport::default();
        let loaded = match self.load_new().await {
            Ok(l) => l,
            Err(errors) => {
                report.errors = errors;
                return report;
            }
        };
        let new = loaded.config;
        report.warnings = loaded.warnings;
        let old = Arc::clone(&self.current.lock());
        let diff = classes::diff(&old, &new);
        report.restart_required = owned(&diff.restart);
        report.changed = diff.apply.iter().map(|(k, _)| (*k).to_owned()).collect();
        let apply_keys: Vec<&str> = diff.apply.iter().map(|(k, _)| *k).collect();

        // the runtime keys: classify, then exactly one ReloadLive
        let mut runtime_sent = false;
        if !diff.runtime.is_empty() {
            match self.classify_runtime(&diff.runtime).await {
                Ok((changed, overridden)) => match self.send_runtime(&new).await {
                    Ok(()) => {
                        runtime_sent = true;
                        report.changed.extend(changed);
                        report.overridden = overridden;
                    }
                    Err(e) => report.errors.push(runtime_unreachable(&e.to_string())),
                },
                Err(e) => report.errors.push(runtime_unreachable(&e.to_string())),
            }
        }

        // the running configuration with the applied keys taken from the file; restart keys keep running
        let mut merge_keys = apply_keys;
        if runtime_sent {
            merge_keys.extend(diff.runtime.iter().copied());
        }
        let merged = match classes::merge(&old, &new, &merge_keys) {
            Ok(m) => m,
            Err(e) => {
                // nothing was applied; only the runtime keys may have left (the merge of the same keys cannot
                // fail where the loader succeeded, so this is a defect of the table code)
                report.errors.push(e);
                report
                    .changed
                    .retain(|k| diff.runtime.contains(&k.as_str()));
                return report;
            }
        };
        for applier in diff.appliers() {
            self.apply_one(applier, &merged);
        }
        *self.current.lock() = Arc::new(merged);
        report
    }

    /// One holder takes the value of its keys from `merged`.
    fn apply_one(&self, applier: Applier, merged: &Config) {
        match applier {
            Applier::IpBlock => {
                // the loader refused a bad network already
                self.http.live.set_ip_block(&merged.net.ip_block);
            }
            Applier::Origins => {
                self.http
                    .live
                    .set_allowed_origins(&merged.net.allowed_origins);
                self.gateway
                    .set_allowed_origins(&merged.net.allowed_origins);
            }
            Applier::Lobby => self.core.reload(LobbySettings::from_config(
                merged,
                &self.reload_source.zone_name,
            )),
            Applier::Admin => self.admin.apply_config(merged),
            Applier::LogFilter => {
                let rust_log = std::env::var("RUST_LOG").ok();
                if rust_log.as_deref().is_some_and(|v| !v.trim().is_empty()) {
                    // 10 §8.2: RUST_LOG wins over log.level; the key counts as changed, the filter stays
                    tracing::info!("log.level changed: the log filter stays RUST_LOG");
                } else if let Err(e) = self.log.set_filter(&merged.log.level) {
                    // the directive is valid (the loader parsed it): a process without a subscriber (tests)
                    tracing::debug!("log.level: {e:#}");
                }
            }
            Applier::ClientWarn => self
                .gateway
                .client_warn_per_min
                .store(merged.log.client_warn_per_min, Ordering::Relaxed),
            Applier::Countdown => self
                .coordinator
                .set_default_countdown(merged.server.shutdown_countdown_s),
            Applier::Current => {}
        }
    }
}

fn runtime_unreachable(why: &str) -> String {
    format!(
        "the game thread did not take the runtime keys ({why}); they are offered again by the next reload"
    )
}
