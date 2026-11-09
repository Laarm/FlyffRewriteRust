//! Runtime overrides of the live configuration (Plan/10 §8.6, 23 §8.5; the minimal form of WP-OPS-20).
//!
//! The runtime-class keys are `rates.*` and the event flags. A GM or administrator change (`AdminCmd::SetRate`,
//! `SetEvent`, `SetRuntime`, `UnsetRuntime`: the console words `set`/`unset`, `POST /admin/api/rates/{key}`, ...)
//! builds the next [`LiveConfig`] generation from the current one and, with `fidelity.persist_gm_overrides`, is
//! written to the `runtime_override` table, so the value survives a restart. At start the rows are applied on top
//! of the file values ([`apply_rows`]); invalid or unknown rows are warned about and ignored, never deleted.
//!
//! Precedence (13 §5.1): built-in default, `flyff.toml`, `runtime_override`, live change (which writes the row).
//! `unset` deletes the override and makes the file value effective at once.
//!
//! A reload of `flyff.toml` (WP-OPS-16) rebuilds the generation from the new file values and puts every override in
//! force back on top ([`rebase`]), so an override still wins over the file (rank 6 over rank 2, 10 §8.2) and `unset`
//! afterwards restores the *new* file value. The consumers (event flags, then the `GameRate` broadcast of 23 §8.5) are
//! called in a fixed order by `Game::replace_live`; there is no consumer registry (WP-OPS-20's deferral stands).

use crate::live::{LiveConfig, LiveOrigin};
use flyff_common::config::{EventDefaults, EventId, Rates, RulesConfig};
use flyff_db::RuntimeOverrideRecord;
use std::collections::BTreeMap;

/// Highest event id that has an override key (`events.flag.<id>`; ids from 500 are script-defined, 03 §5.2).
pub const MAX_FLAG_ID: u16 = 499;

/// One settable rate (23 §8.5).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RateKey {
    /// The short key of `/rates` and `PUT /admin/api/rates/{key}`.
    pub short: &'static str,
    /// The `flyff.toml` key.
    pub key: &'static str,
    /// Smallest allowed value.
    pub min: f32,
    /// `true`: the smallest value itself is refused (`0.0 < v`).
    pub min_exclusive: bool,
    /// Largest allowed value.
    pub max: f32,
}

/// The seven rates and their ranges (23 §8.5).
pub const RATE_KEYS: [RateKey; 7] = [
    rate("exp", "rates.monster_exp", 0.0, true, 1000.0),
    rate("drop", "rates.item_drop", 0.0, true, 1000.0),
    rate("gold", "rates.gold_drop", 0.0, true, 1000.0),
    rate("hit", "rates.monster_hit", 0.0, true, 1000.0),
    rate("shop", "rates.shop_cost", 0.5, false, 2.0),
    rate("party", "rates.party_exp", 0.0, true, 1000.0),
    rate("aggressive", "rates.monster_aggressive", 0.0, false, 1.0),
];

const fn rate(
    short: &'static str,
    key: &'static str,
    min: f32,
    min_exclusive: bool,
    max: f32,
) -> RateKey {
    RateKey {
        short,
        key,
        min,
        min_exclusive,
        max,
    }
}

impl RateKey {
    /// The key of `name`: the short key or the full config key, case-insensitive.
    pub fn find(name: &str) -> Option<&'static RateKey> {
        let n = name.trim().to_ascii_lowercase();
        RATE_KEYS.iter().find(|k| k.short == n || k.key == n)
    }

    /// `true` when `v` lies in the allowed range.
    pub fn allows(&self, v: f32) -> bool {
        v.is_finite()
            && v <= self.max
            && if self.min_exclusive {
                v > self.min
            } else {
                v >= self.min
            }
    }

    /// The configured value (`None` for the unset `Option` rates).
    pub fn get(&self, rates: &Rates) -> Option<f32> {
        match self.short {
            "exp" => rates.monster_exp,
            "drop" => rates.item_drop,
            "gold" => rates.gold_drop,
            "hit" => rates.monster_hit,
            "shop" => rates.shop_cost,
            "party" => Some(rates.party_exp),
            _ => Some(rates.monster_aggressive),
        }
    }

    /// Sets (or, with `None` on an `Option` rate, clears) the value.
    pub fn set(&self, rates: &mut Rates, v: Option<f32>) {
        match self.short {
            "exp" => rates.monster_exp = v,
            "drop" => rates.item_drop = v,
            "gold" => rates.gold_drop = v,
            "hit" => rates.monster_hit = v,
            "shop" => rates.shop_cost = v,
            "party" => rates.party_exp = v.unwrap_or(Rates::default().party_exp),
            _ => rates.monster_aggressive = v.unwrap_or(Rates::default().monster_aggressive),
        }
    }

    /// The value the data files give: `Rates::default()` (the `Option` rates are 1.0 when unset).
    pub fn data_default(&self) -> f32 {
        self.get(&Rates::default()).unwrap_or(1.0)
    }

    /// The effective value (an unset `Option` rate is 1.0).
    pub fn effective(&self, rates: &Rates) -> f32 {
        self.get(rates).unwrap_or(1.0)
    }
}

/// The keys of `[events]` and the event ids they initialise (same order as `EventDefaults::initial_flags`).
pub const EVENT_KEYS: [(&str, EventId); 15] = [
    ("events.eve_18", EventId::Eve18),
    ("events.guild_bank", EventId::GuildInventory),
    ("events.guild_war", EventId::GuildWar),
    ("events.wormon", EventId::Wormon),
    ("events.respawn", EventId::Respawn),
    ("events.pk", EventId::Pk),
    ("events.pk_cost", EventId::PkCost),
    ("events.steal", EventId::Steal),
    ("events.guild_combat", EventId::GuildCombat),
    ("events.drop_item_remove", EventId::DropItemRemove),
    ("events.recommend", EventId::Recommend),
    ("events.guild_combat_1to1", EventId::GuildCombat1to1),
    ("events.arena", EventId::Arena),
    ("events.secret_room", EventId::SecretRoom),
    ("events.rainbow_race", EventId::RainbowRace),
];

/// The `[events]` key of an event id, when it has one.
pub fn event_key(id: u16) -> Option<&'static str> {
    EVENT_KEYS
        .iter()
        .find(|(_, e)| e.id() == id)
        .map(|(k, _)| *k)
}

/// A recognised runtime-class key.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RuntimeKey {
    /// `rates.*`.
    Rate(&'static RateKey),
    /// An event flag: `events.<name>` or `events.flag.<id>`.
    Event(u16),
}

/// Why a runtime change was refused.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum OverrideError {
    /// Not a runtime-class key.
    #[error("'{0}' is not a runtime key (rates.*, events.*)")]
    UnknownKey(String),
    /// The JSON value has the wrong type.
    #[error("{0}")]
    BadType(String),
    /// The value is outside the allowed range.
    #[error("value out of range")]
    OutOfRange,
    /// Event 1 (`events.eve_18`) is not settable at runtime (23 §8.2).
    #[error("event 1 cannot be changed at runtime")]
    Protected,
}

/// Resolves a key: a rate (short or full key), `events.<name>` or `events.flag.<id>`.
pub fn parse_key(key: &str) -> Result<RuntimeKey, OverrideError> {
    let k = key.trim().to_ascii_lowercase();
    if let Some(r) = RateKey::find(&k) {
        return Ok(RuntimeKey::Rate(r));
    }
    if let Some((_, e)) = EVENT_KEYS.iter().find(|(name, _)| *name == k) {
        return Ok(RuntimeKey::Event(e.id()));
    }
    if let Some(id) = k.strip_prefix("events.flag.") {
        return match id.parse::<u16>() {
            Ok(id) if id <= MAX_FLAG_ID => Ok(RuntimeKey::Event(id)),
            _ => Err(OverrideError::UnknownKey(key.to_owned())),
        };
    }
    Err(OverrideError::UnknownKey(key.to_owned()))
}

/// The canonical key a change is stored under: the rate's config key, the `[events]` key when the id has one,
/// else `events.flag.<id>` (23 §8.5).
pub fn canonical_key(k: RuntimeKey) -> String {
    match k {
        RuntimeKey::Rate(r) => r.key.to_owned(),
        RuntimeKey::Event(id) => {
            event_key(id).map_or_else(|| format!("events.flag.{id}"), str::to_owned)
        }
    }
}

fn number_of(v: &serde_json::Value) -> Result<f64, OverrideError> {
    v.as_f64()
        .ok_or_else(|| OverrideError::BadType("expected a number".to_owned()))
}

/// The event state of a JSON value: a bool or a number 0..=255.
fn state_of(v: &serde_json::Value) -> Result<u8, OverrideError> {
    if let Some(b) = v.as_bool() {
        return Ok(u8::from(b));
    }
    v.as_u64()
        .and_then(|n| u8::try_from(n).ok())
        .ok_or_else(|| {
            OverrideError::BadType("expected true, false or a number 0..=255".to_owned())
        })
}

fn next_generation(live: &LiveConfig, key: &str, set_by: &str) -> LiveConfig {
    let mut next = live.clone();
    next.generation = live.generation.saturating_add(1);
    next.origin = LiveOrigin::Override {
        key: key.to_owned(),
        set_by: set_by.to_owned(),
    };
    next
}

/// The next generation with `key` set to `value`. A change that leaves every value as it was is
/// [`Applied::Unchanged`] (no new generation).
pub fn set_value(
    live: &LiveConfig,
    key: RuntimeKey,
    value: &serde_json::Value,
    set_by: &str,
) -> Result<Applied, OverrideError> {
    let canonical = canonical_key(key);
    let mut next = next_generation(live, &canonical, set_by);
    match key {
        RuntimeKey::Rate(r) => {
            let v = number_of(value)? as f32;
            if !r.allows(v) {
                return Err(OverrideError::OutOfRange);
            }
            r.set(&mut next.rates, Some(v));
        }
        RuntimeKey::Event(id) => {
            if id == EventId::Eve18.id() {
                return Err(OverrideError::Protected);
            }
            next.event_flags.insert(id, state_of(value)?);
        }
    }
    Ok(if same_values(live, &next) {
        Applied::Unchanged
    } else {
        Applied::New(next)
    })
}

/// The next generation with the override of `key` removed: the file value (`base`) is effective again.
pub fn clear_value(
    live: &LiveConfig,
    base: &RulesConfig,
    key: RuntimeKey,
    set_by: &str,
) -> Applied {
    let canonical = canonical_key(key);
    let mut next = next_generation(live, &canonical, set_by);
    match key {
        RuntimeKey::Rate(r) => r.set(&mut next.rates, r.get(&base.rates)),
        RuntimeKey::Event(id) => {
            next.event_flags.remove(&id);
        }
    }
    if same_values(live, &next) {
        Applied::Unchanged
    } else {
        Applied::New(next)
    }
}

/// Outcome of a change: a new generation, or nothing changed.
#[derive(Clone, Debug, PartialEq)]
pub enum Applied {
    /// The next generation.
    New(LiveConfig),
    /// The value was already in force.
    Unchanged,
}

fn same_values(a: &LiveConfig, b: &LiveConfig) -> bool {
    a.rates == b.rates && a.events == b.events && a.event_flags == b.event_flags
}

/// The `runtime_override` rows in force (key to value), as the game keeps them to answer "overridden?".
pub type RowsInForce = BTreeMap<String, serde_json::Value>;

/// The next generation after a reload of `flyff.toml` (Plan/10 §8.6, WP-OPS-16): the file's `[rates]` and `[events]`
/// with an empty `event_flags`, then every override in `in_force` applied on top with [`parse_key`] and
/// [`set_value`]. A key that no longer parses or a value that is now refused is warned about and skipped, never
/// deleted. The generation is `live.generation + 1` with origin [`LiveOrigin::FileReload`]; [`Applied::Unchanged`]
/// when rates, events and event flags all equal the ones in force (no generation, no consumer call).
///
/// An `event_flags` entry that is not in `in_force` (only a test can inject one) is dropped.
pub fn rebase(
    live: &LiveConfig,
    file_rates: &Rates,
    file_events: &EventDefaults,
    in_force: &RowsInForce,
) -> Applied {
    let mut next = LiveConfig {
        generation: live.generation,
        rates: file_rates.clone(),
        events: file_events.clone(),
        event_flags: BTreeMap::new(),
        origin: LiveOrigin::FileReload,
    };
    for (key, value) in in_force {
        let applied = parse_key(key).and_then(|k| set_value(&next, k, value, "file reload"));
        match applied {
            Ok(Applied::New(n)) => next = n,
            Ok(Applied::Unchanged) => {}
            Err(e) => tracing::warn!(
                key = %key,
                "the override in force is not applied on top of the reloaded file: {e}"
            ),
        }
    }
    if same_values(live, &next) {
        return Applied::Unchanged;
    }
    next.generation = live.generation.saturating_add(1);
    next.origin = LiveOrigin::FileReload;
    Applied::New(next)
}

/// Applies the stored rows on top of the file values at start (10 §8.6, S8). Returns the initial generation
/// (generation 0), the rows that were valid, and one warning per row that was ignored.
pub fn apply_rows(
    rules: &RulesConfig,
    rows: &[RuntimeOverrideRecord],
) -> (LiveConfig, RowsInForce, Vec<String>) {
    let mut live = LiveConfig::from_rules(rules);
    let mut kept = RowsInForce::new();
    let mut warnings = Vec::new();
    for row in rows {
        let key = match parse_key(&row.key) {
            Ok(k) => k,
            Err(e) => {
                warnings.push(format!("runtime_override '{}' ignored: {e}", row.key));
                continue;
            }
        };
        match set_value(&live, key, &row.value, &row.set_by) {
            Ok(Applied::New(next)) => {
                live = next;
                kept.insert(canonical_key(key), row.value.clone());
            }
            Ok(Applied::Unchanged) => {
                kept.insert(canonical_key(key), row.value.clone());
            }
            Err(e) => warnings.push(format!("runtime_override '{}' ignored: {e}", row.key)),
        }
    }
    live.generation = 0;
    live.origin = LiveOrigin::Startup;
    (live, kept, warnings)
}
