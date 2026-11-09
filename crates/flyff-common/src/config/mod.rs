//! Configuration registry types shared by the server, the game and (client-side keys) the browser
//! (Plan/03 §4–§5, WP-CORE-90; stage1-review §5).
//!
//! * [`tables`] holds THE key table; its single macro invocation generates the structs [`Features`],
//!   [`Gameplay`], [`Rates`], [`EventDefaults`], [`Fidelity`], the aggregate [`RulesConfig`],
//!   [`KEY_REGISTRY`] (one [`KeyMeta`] per key) and the projections [`SimFlags`] and
//!   [`ClientFeatureConfig`],
//! * [`registry`] has the metadata types, the lookups, `is_runtime_key` and the init-only guard,
//! * [`features`], [`gameplay`], [`rates`], [`events`], [`fidelity`] carry the section-specific extras.
//!
//! Loading from TOML/env, `runtime_override` storage and the template are `flyff-server`'s (doc 10).

#[macro_use]
mod macros;
pub mod events;
pub mod features;
pub mod fidelity;
pub mod gameplay;
pub mod rates;
pub mod registry;
pub mod tables;

pub use events::{EVENT_GENERIC_BASE, EventId, MAX_EVENT};
pub use fidelity::HitAttrRule;
pub use registry::{
    InitOnlyDiff, InitOnlyMismatch, InitOnlyValues, KEY_REGISTRY, KeyClass, KeyMeta, KeySide,
    SECTIONS, check_init_only, clamp_reserved, init_only_snapshot, is_runtime_key, key_meta,
    keys_of, reserved_enabled,
};
pub use tables::{
    ClientFeatureConfig, ClientFeatures, ClientFidelity, ClientGameplay, EventDefaults, Features,
    Fidelity, Gameplay, Rates, RulesConfig, SimFlags,
};
