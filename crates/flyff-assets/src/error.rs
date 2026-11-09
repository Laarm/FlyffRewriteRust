//! [`AssetError`] and [`AssetErrorKind`] (Plan/24 §7.3, failure policy §7.6).

use std::fmt;

/// What went wrong. The HTTP layer maps this to a status: `NotFound` 404, `BadKey` 400, `Build` and
/// `Io` 500 (+ `X-Flyff-Asset-Error: <as_str>`), `Timeout` 503, `Disabled` 404 with a clear body.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AssetErrorKind {
    /// The kind has no such key / source file / partition.
    NotFound,
    /// The source exists but cannot be converted (also: "gzip is not beneficial" for a `gz` job).
    NotConvertible,
    /// The key does not match the kind's grammar (Plan/24 §9.2).
    BadKey,
    /// A builder failed (parser error, panic, missing required input).
    Build,
    /// A cache I/O error that is not covered by the in-memory fallback.
    Io,
    /// The kind is not registered at this milestone (or switched off by config).
    Disabled,
    /// The disk budget refuses this optional artifact (Plan/24 §8.7).
    OverBudget,
    /// The pool is shutting down; the job was dropped.
    ShuttingDown,
    /// A caller-side wait ran out (`get_blocking`); the job keeps running.
    Timeout,
}

impl AssetErrorKind {
    /// Stable lower-case name (`X-Flyff-Asset-Error`, report JSON).
    pub const fn as_str(self) -> &'static str {
        match self {
            AssetErrorKind::NotFound => "not_found",
            AssetErrorKind::NotConvertible => "not_convertible",
            AssetErrorKind::BadKey => "bad_key",
            AssetErrorKind::Build => "build",
            AssetErrorKind::Io => "io",
            AssetErrorKind::Disabled => "disabled",
            AssetErrorKind::OverBudget => "over_budget",
            AssetErrorKind::ShuttingDown => "shutting_down",
            AssetErrorKind::Timeout => "timeout",
        }
    }
}

impl fmt::Display for AssetErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An error of the asset framework. `Clone` because one failed job answers every waiter.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{kind}: {detail}")]
pub struct AssetError {
    /// The class of the failure.
    pub kind: AssetErrorKind,
    /// Human-readable detail naming the file or key.
    pub detail: String,
}

impl AssetError {
    /// An error of `kind` with `detail`.
    pub fn new(kind: AssetErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    /// [`AssetErrorKind::NotFound`].
    pub fn not_found(detail: impl Into<String>) -> Self {
        Self::new(AssetErrorKind::NotFound, detail)
    }

    /// [`AssetErrorKind::NotConvertible`].
    pub fn not_convertible(detail: impl Into<String>) -> Self {
        Self::new(AssetErrorKind::NotConvertible, detail)
    }

    /// [`AssetErrorKind::BadKey`].
    pub fn bad_key(detail: impl Into<String>) -> Self {
        Self::new(AssetErrorKind::BadKey, detail)
    }

    /// [`AssetErrorKind::Build`]: what a builder returns for unreadable or malformed input; the detail
    /// MUST name the file (Plan/24 B2).
    pub fn build(detail: impl Into<String>) -> Self {
        Self::new(AssetErrorKind::Build, detail)
    }

    /// [`AssetErrorKind::Disabled`].
    pub fn disabled(detail: impl Into<String>) -> Self {
        Self::new(AssetErrorKind::Disabled, detail)
    }

    /// [`AssetErrorKind::ShuttingDown`].
    pub fn shutting_down() -> Self {
        Self::new(AssetErrorKind::ShuttingDown, "asset pool is shutting down")
    }

    /// An [`AssetErrorKind::Io`] error for `what` (a path or an action).
    pub fn io(what: impl fmt::Display, err: &std::io::Error) -> Self {
        Self::new(AssetErrorKind::Io, format!("{what}: {err}"))
    }
}
