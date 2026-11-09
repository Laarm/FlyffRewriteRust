//! `[fidelity]` (Plan/03 §4.7): the switches that reproduce the original behaviour when set to their
//! "original" value. The struct is generated in [`tables`](super::tables); this module owns the enum.

/// Which animation frames count as a hit (`[fidelity] hit_attr_rule`, D4).
#[derive(
    Clone, Copy, PartialEq, Eq, Hash, Debug, Default, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum HitAttrRule {
    /// The original `floor(frameCur)` window rule (KQ-01).
    #[default]
    Legacy,
    /// The alternative: every attribute in `(old, cur]`.
    Window,
}

pub use super::tables::Fidelity;
