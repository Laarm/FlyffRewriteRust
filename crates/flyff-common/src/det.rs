//! Deterministic collections (Plan/02 §11.3).
//!
//! `std::collections::HashMap`/`HashSet` are disallowed in determinism tiers S/G/R because their
//! iteration order depends on a random seed. [`DetMap`] / [`DetSet`] iterate in **insertion order** and
//! use a fixed hasher, so their order is identical on every platform and across runs.
//!
//! Removal: use `shift_remove` when the iteration order is semantically visible; `swap_remove` is
//! still deterministic but reorders the tail.

/// Fixed-seed hasher used by [`DetMap`] / [`DetSet`].
pub type DetBuildHasher = core::hash::BuildHasherDefault<rustc_hash::FxHasher>;

/// Insertion-ordered map with a fixed hasher.
pub type DetMap<K, V> = indexmap::IndexMap<K, V, DetBuildHasher>;

/// Insertion-ordered set with a fixed hasher.
pub type DetSet<T> = indexmap::IndexSet<T, DetBuildHasher>;
