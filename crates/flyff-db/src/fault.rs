//! Fault injection for the persistence actor (Plan/13 §16.4, §26): the test hooks behind the acceptance
//! tests of WP-DB-04 (an `Expected` failure in the last op of a batch, transient failures and the retry
//! mode, `Bug` and `Corrupt` failures, a held writer for deterministic drains, a statement trace).
//!
//! In a shipped build ([`FaultHooks`] without the `testing` feature outside unit tests) every method is an
//! inlined no-op, so the writer carries no test logic.

use crate::error::DbError;
use std::sync::Arc;
use std::time::Duration;

/// What the writer is about to apply (the argument of an op fault closure).
#[derive(Clone, Copy, Debug)]
pub struct OpProbe<'a> {
    /// The label of the batch's `AtomicKind` (`"Save"` for a standalone snapshot command).
    pub batch: &'a str,
    /// `"snapshot"` or the `DbOp` variant name.
    pub what: &'a str,
    /// Position inside the batch: snapshots first, then ops.
    pub index: usize,
    /// Number of snapshots plus ops in the batch.
    pub count: usize,
}

/// A fault closure: `Some(error)` makes the probed step fail with that error.
pub type FaultFn = Box<dyn FnMut(&OpProbe<'_>) -> Option<DbError> + Send>;

mod imp {
    use super::*;

    /// Shipped builds carry no hooks.
    #[derive(Default)]
    pub struct FaultHooks;

    impl FaultHooks {
        pub(crate) fn no_faults() -> FaultHooks {
            FaultHooks
        }
        #[inline]
        pub(crate) fn wait_gate(&self) {}
        #[inline]
        pub(crate) fn probe(&self, _p: &OpProbe<'_>) -> Option<DbError> {
            None
        }
        #[inline]
        pub(crate) fn on_begin(&self) -> Option<DbError> {
            None
        }
        #[inline]
        pub(crate) fn on_commit(&self) -> Option<DbError> {
            None
        }
        #[inline]
        pub(crate) fn note_commit(&self) {}
        #[inline]
        pub(crate) fn backoff(&self) -> Option<Vec<Duration>> {
            None
        }
        #[inline]
        pub(crate) fn trace(&self, _line: &str) {}
        #[inline]
        pub(crate) fn should_panic(&self) -> bool {
            false
        }
    }
}

pub use imp::FaultHooks;

/// The hooks the writer uses when the caller supplied none.
pub(crate) fn no_faults() -> Arc<FaultHooks> {
    Arc::new(FaultHooks::no_faults())
}
