//! Runtime-class configuration (Plan/10 §8.6): the `[rates]` and `[events]` values the game uses, replaced
//! as a whole by `GameInput::ReloadLive`. The mechanism around it (startup `runtime_override` loading,
//! consumers, `persist_override`) is WP-OPS-20; this module owns the value type the scheduler gates and
//! the tick loop read.

use flyff_common::config::{EventDefaults, EventId, Rates, RulesConfig};
use std::collections::BTreeMap;

/// Where a [`LiveConfig`] generation came from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum LiveOrigin {
    /// Built at startup from file, env, flags and the `runtime_override` rows.
    #[default]
    Startup,
    /// `reload` of `flyff.toml` (console, `POST /admin/api/reload`, SIGHUP).
    FileReload,
    /// A GM/admin/console change of one key.
    Override {
        /// Dotted key.
        key: String,
        /// `gm:<account>`, `admin:owner:local`, `admin:owner:token`, `admin:staff:<account>`, `admin:console` or `admin:cli`.
        set_by: String,
    },
}

/// Live-reloadable configuration generation (`[rates]`, `[events]`, `events.flag.<id>`; Plan/10 §8.6).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LiveConfig {
    /// Monotonic generation number (`0` = startup).
    pub generation: u64,
    /// `[rates]`.
    pub rates: Rates,
    /// `[events]`: the initial values of the named event slots.
    pub events: EventDefaults,
    /// `events.flag.<id>` overrides and values changed at runtime, ids `0..=499` (JSON number 0–255).
    pub event_flags: BTreeMap<u16, u8>,
    /// Origin of this generation.
    pub origin: LiveOrigin,
}

impl LiveConfig {
    /// The initial generation (0) from the startup rules, with an empty `event_flags` (WP-OPS-02).
    pub fn from_rules(rules: &RulesConfig) -> Self {
        Self {
            generation: 0,
            rates: rules.rates.clone(),
            events: rules.events.clone(),
            event_flags: BTreeMap::new(),
            origin: LiveOrigin::Startup,
        }
    }

    /// The effective state of an event slot: an `event_flags` override when present, else the
    /// `[events]` default (0 for slots without a key).
    pub fn event_state(&self, id: EventId) -> u8 {
        self.event_flags
            .get(&id.id())
            .copied()
            .or_else(|| {
                self.events
                    .initial_flags()
                    .iter()
                    .find(|(i, _)| *i == id)
                    .map(|(_, v)| *v)
            })
            .unwrap_or(0)
    }
}

/// A subsystem that reacts to a change of the live configuration (22: event flags, 23: `GameRate`
/// broadcast). Registered at `Game` construction by WP-OPS-20.
pub trait LiveConsumer {
    /// Called on the game thread with the previous and the new generation.
    fn on_live_change(
        &mut self,
        ctx: &mut crate::ctx::GameCtx<'_>,
        old: &LiveConfig,
        new: &LiveConfig,
    );
}

impl crate::tick::Game {
    /// Puts a new `LiveConfig` generation in force (`GameInput::ReloadLive`, every GM/admin/console change) and
    /// calls the registered [`LiveConsumer`]s with the old and the new generation, in registration order: doc 22's
    /// event-flag store (`SetLocalEvent` broadcast) and doc 23's `RatesConsumer` (the `GameRate` broadcast).
    pub(crate) fn replace_live(&mut self, new: std::sync::Arc<LiveConfig>) {
        let old = std::mem::replace(&mut self.live, new);
        let new = std::sync::Arc::clone(&self.live);
        let (svc, mut ctx) = self.split();
        svc.event.on_live_change(&mut ctx, &old, &new);
        // doc 23's consumer: the `GameRate` of every rate that changed (23 §8.5)
        crate::gm::rates::RatesConsumer.on_live_change(&mut ctx, &old, &new);
    }

    /// `GameInput::ReloadLive` (Plan/10 §8.6, §13.2). A [`LiveOrigin::FileReload`] payload carries the values of the
    /// reloaded `flyff.toml` (WP-OPS-16) and goes through [`Game::apply_file_reload`]; any other origin replaces the
    /// configuration verbatim (the tests that inject a whole generation rely on it).
    pub(crate) fn on_reload_live(&mut self, cfg: std::sync::Arc<LiveConfig>) {
        if cfg.origin == LiveOrigin::FileReload {
            self.apply_file_reload(&cfg);
        } else {
            tracing::info!(generation = cfg.generation, "live configuration reloaded");
            self.replace_live(cfg);
        }
    }

    /// The runtime keys of a reloaded `flyff.toml` (Plan/10 §8.6; `file.rates` and `file.events` are what the loader
    /// found, the rest of the payload is ignored): they become the file base (`cfg.rules`, what `unset` and `/rates`
    /// read), then the next generation is the file values with every override in force on top, so an override wins
    /// and the file value is only stored behind it ([`overrides::rebase`](crate::overrides::rebase)). The
    /// generation number is ours (`live.generation + 1`); nothing changing means no generation and no consumer
    /// call. Both persist modes keep the overrides in force, and no `runtime_override` row is written or deleted.
    pub(crate) fn apply_file_reload(&mut self, file: &LiveConfig) {
        self.cfg.rules.rates = file.rates.clone();
        self.cfg.rules.events = file.events.clone();
        match crate::overrides::rebase(&self.live, &file.rates, &file.events, &self.overrides) {
            crate::overrides::Applied::New(next) => {
                tracing::info!(
                    generation = next.generation,
                    "live configuration reloaded from flyff.toml"
                );
                self.replace_live(std::sync::Arc::new(next));
            }
            crate::overrides::Applied::Unchanged => {
                tracing::debug!("flyff.toml reloaded: the runtime keys are unchanged");
            }
        }
    }
}
