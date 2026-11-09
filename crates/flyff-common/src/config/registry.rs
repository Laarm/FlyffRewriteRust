//! The key registry types and lookups (Plan/03 §4.1, WP-CORE-90). The registry itself,
//! [`KEY_REGISTRY`], is generated from the single key table in [`tables`](super::tables).

pub use super::tables::KEY_REGISTRY;
use super::tables::RulesConfig;

/// How a key may change at runtime (Plan/03 §4.1).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum KeyClass {
    /// Read at start; a change needs a restart.
    Startup,
    /// May be changed while running (`[rates]`, `[events]`, GM `set`).
    Runtime,
    /// Fixed when the database is created; checked against `meta.init.*` on every start.
    InitOnly,
}

/// Which side of the system consumes a key.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum KeySide {
    /// Server only.
    Server,
    /// Browser client only.
    Client,
    /// Server and client.
    Both,
    /// The shared simulation (`flyff-sim`); the key is part of `SimFlags`.
    Sim,
}

/// Metadata of one configuration key (Plan/03 §4.1; `doc`/`ty`/`section`/`name` added for the template
/// generator of 10 §8.7, Open question 6).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KeyMeta {
    /// Dotted key, e.g. `"features.auto_skill_max"`.
    pub key: &'static str,
    /// Section part of the key (`"features"`).
    pub section: &'static str,
    /// Key part inside the section (`"auto_skill_max"`).
    pub name: &'static str,
    /// Runtime mutability.
    pub class: KeyClass,
    /// Consumer side.
    pub side: KeySide,
    /// Copied into `ClientFeatureConfig` (the C flag).
    pub client_delivered: bool,
    /// Legacy reference, e.g. `"ServerCommon.h:8 __AUTO_SKILL_MAX"`.
    pub legacy: &'static str,
    /// Owning plan document(s), e.g. `"16-movers-stats-progression"`; several are joined with `/`.
    pub owner_doc: &'static str,
    /// Parity-ledger row, e.g. `"PL-070"` or `"KQ-03"`.
    pub ledger: Option<&'static str>,
    /// Accepted but not implemented in v1: `true` warns and behaves as `false`.
    pub reserved: bool,
    /// One-line description (the "Effect" column; template comments).
    pub doc: &'static str,
    /// Rust type of the value as written in the table (`"bool"`, `"u32"`, `"Option<f32>"`, …).
    pub ty: &'static str,
}

/// The sections owned by Plan/03, in template order.
pub const SECTIONS: [&str; 5] = ["features", "gameplay", "rates", "events", "fidelity"];

/// The registry entry of `key` (`"rates.party_exp"`).
pub fn key_meta(key: &str) -> Option<&'static KeyMeta> {
    KEY_REGISTRY.iter().find(|m| m.key == key)
}

/// The registry entries of one section, in table order.
pub fn keys_of(section: &str) -> impl Iterator<Item = &'static KeyMeta> + '_ {
    KEY_REGISTRY.iter().filter(move |m| m.section == section)
}

/// `true` when `key` is a runtime-class key (Plan/03 §4.1, 10 §8.6): every `[rates]` and `[events]`
/// key, plus the override-only form `events.flag.<id>` for the unnamed event ids `0..=499`.
pub fn is_runtime_key(key: &str) -> bool {
    if let Some(id) = key.strip_prefix("events.flag.") {
        return !id.is_empty()
            && id.bytes().all(|b| b.is_ascii_digit())
            && id.len() <= 3
            && id
                .parse::<u16>()
                .is_ok_and(|n| n < super::events::EVENT_GENERIC_BASE);
    }
    key_meta(key).is_some_and(|m| m.class == KeyClass::Runtime)
}

/// The reserved keys that are `true` in `rules` (startup warning "reserved, not implemented"; the
/// behaviour stays as `false`).
pub fn reserved_enabled(rules: &RulesConfig) -> Vec<&'static str> {
    KEY_REGISTRY
        .iter()
        .filter(|m| m.reserved && rules.bool_by_key(m.key) == Some(true))
        .map(|m| m.key)
        .collect()
}

/// Forces every reserved key to `false` (the effective value) and returns the keys that were `true`.
pub fn clamp_reserved(rules: &mut RulesConfig) -> Vec<&'static str> {
    let enabled = reserved_enabled(rules);
    for key in &enabled {
        rules.set_bool_by_key(key, false);
    }
    enabled
}

/// The init-only values recorded in the database `meta` table when it is created and compared at every
/// start (Plan/03 §4.1 class I, 13 §15.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InitOnlyValues {
    /// `features.inventory_168`: 168 bag slots instead of the vanilla 42.
    pub inventory_168: bool,
    /// `features.bigger_pocket`: pockets of 18/42/42 slots (vanilla 6/24/24).
    pub bigger_pocket: bool,
    /// `gameplay.max_pack_item`: the stack size cap of stackable items.
    pub max_pack_item: u32,
}

impl Default for InitOnlyValues {
    /// The repack defaults (Plan/03 §4.2, §4.5).
    fn default() -> Self {
        init_only_snapshot(&RulesConfig::default())
    }
}

/// One differing init-only value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitOnlyDiff {
    /// Dotted key.
    pub key: &'static str,
    /// Value recorded when the database was created.
    pub stored: String,
    /// Value in the current configuration.
    pub configured: String,
}

/// The configured init-only values differ from the stored ones: container sizes and stack limits shape
/// stored data, so the server refuses to start (exit 78).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitOnlyMismatch(pub Vec<InitOnlyDiff>);

impl core::fmt::Display for InitOnlyMismatch {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for (i, d) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str("; ")?;
            }
            write!(
                f,
                "{} is {} but the database was created with {}",
                d.key, d.configured, d.stored
            )?;
        }
        f.write_str(
            " (init-only keys cannot change once the database exists: restore the old value or use a new data directory)",
        )
    }
}

impl std::error::Error for InitOnlyMismatch {}

/// The init-only values of `rules` (`init_only_snapshot`, Plan/03 WP-CORE-90).
pub fn init_only_snapshot(rules: &RulesConfig) -> InitOnlyValues {
    InitOnlyValues {
        inventory_168: rules.features.inventory_168,
        bigger_pocket: rules.features.bigger_pocket,
        max_pack_item: rules.gameplay.max_pack_item,
    }
}

/// Compares the stored init-only values with the current ones; every difference is reported.
pub fn check_init_only(
    stored: &InitOnlyValues,
    current: &InitOnlyValues,
) -> Result<(), InitOnlyMismatch> {
    let mut diffs = Vec::new();
    if stored.inventory_168 != current.inventory_168 {
        diffs.push(InitOnlyDiff {
            key: "features.inventory_168",
            stored: stored.inventory_168.to_string(),
            configured: current.inventory_168.to_string(),
        });
    }
    if stored.bigger_pocket != current.bigger_pocket {
        diffs.push(InitOnlyDiff {
            key: "features.bigger_pocket",
            stored: stored.bigger_pocket.to_string(),
            configured: current.bigger_pocket.to_string(),
        });
    }
    if stored.max_pack_item != current.max_pack_item {
        diffs.push(InitOnlyDiff {
            key: "gameplay.max_pack_item",
            stored: stored.max_pack_item.to_string(),
            configured: current.max_pack_item.to_string(),
        });
    }
    if diffs.is_empty() {
        Ok(())
    } else {
        Err(InitOnlyMismatch(diffs))
    }
}
