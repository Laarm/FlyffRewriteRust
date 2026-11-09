//! Per-account flush tracking (Plan/13 §16.5, §19): `wait_flushed(account, gen)` resolves once a save with
//! `gen' >= gen` of that account has been committed; an account whose save failed permanently is *poisoned*
//! and its waiters fail with `SaveFailed` until an admin releases it (Plan/13 §16.6).
//!
//! The waiter is registered under the same lock that reads the state, so a commit can never slip between
//! the check and the registration (no lost wake-ups). Timeouts are applied by the caller
//! (`[database] flush_wait_timeout_ms`, see [`crate::PersistHandle::wait_flushed_timeout`]).

use crate::error::{DbError, DbErrorKind};
use flyff_common::rt::{Reply, ReplyRx, reply_pair};
use flyff_common::{AccountId, SaveGen, UnixTime};
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Waiters older than this are dropped on the next registration: their callers gave up long ago (the
/// tracker cannot see a dropped receiver).
const WAITER_MAX_AGE: Duration = Duration::from_secs(600);

struct Waiter {
    want: SaveGen,
    since: Instant,
    reply: Reply<Result<(), DbError>>,
}

#[derive(Default)]
struct Inner {
    flushed: BTreeMap<AccountId, SaveGen>,
    poisoned: BTreeMap<AccountId, UnixTime>,
    waiters: BTreeMap<AccountId, Vec<Waiter>>,
    closed: bool,
}

/// Tracks the highest committed generation per account, wakes waiters and remembers poisoned accounts.
#[derive(Default)]
pub struct FlushTracker {
    inner: Mutex<Inner>,
}

fn save_failed(account: AccountId) -> DbError {
    DbError::new(
        DbErrorKind::SaveFailed,
        format!("character data of account {account} could not be saved; contact an administrator"),
    )
}

impl FlushTracker {
    /// Highest committed generation of `account` (`SaveGen(0)` when none).
    pub fn flushed_gen(&self, account: AccountId) -> SaveGen {
        self.inner
            .lock()
            .map(|g| g.flushed.get(&account).copied().unwrap_or(SaveGen::ZERO))
            .unwrap_or(SaveGen::ZERO)
    }

    /// A receiver that resolves `Ok` when `account` has committed generation `gen` (immediately when it
    /// already has) and `Err(SaveFailed)` immediately, or as soon as it happens, while the account is poisoned.
    /// After [`FlushTracker::close`] pending waiters get `ShuttingDown`.
    pub fn wait_flushed(
        &self,
        account: AccountId,
        save_gen: SaveGen,
    ) -> ReplyRx<Result<(), DbError>> {
        let (tx, rx) = reply_pair();
        let Ok(mut inner) = self.inner.lock() else {
            let _ = tx.send(Err(DbError::shutting_down()));
            return rx;
        };
        if inner.poisoned.contains_key(&account) {
            let _ = tx.send(Err(save_failed(account)));
            return rx;
        }
        let have = inner
            .flushed
            .get(&account)
            .copied()
            .unwrap_or(SaveGen::ZERO);
        if have >= save_gen {
            let _ = tx.send(Ok(()));
        } else if inner.closed {
            let _ = tx.send(Err(DbError::shutting_down()));
        } else {
            let now = Instant::now();
            inner.waiters.retain(|_, list| {
                list.retain(|w| now.duration_since(w.since) < WAITER_MAX_AGE);
                !list.is_empty()
            });
            inner.waiters.entry(account).or_default().push(Waiter {
                want: save_gen,
                since: now,
                reply: tx,
            });
        }
        rx
    }

    /// Records a commit of `gen` for `account` and wakes the satisfied waiters (writer thread).
    pub fn advance(&self, account: AccountId, save_gen: SaveGen) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        let slot = inner.flushed.entry(account).or_insert(SaveGen::ZERO);
        if save_gen > *slot {
            *slot = save_gen;
        }
        let have = *slot;
        if let Some(list) = inner.waiters.get_mut(&account) {
            let (ready, waiting): (Vec<Waiter>, Vec<Waiter>) = std::mem::take(list)
                .into_iter()
                .partition(|w| have >= w.want);
            *list = waiting;
            for w in ready {
                let _ = w.reply.send(Ok(()));
            }
            if list.is_empty() {
                inner.waiters.remove(&account);
            }
        }
    }

    /// Marks `account` as poisoned (its last save failed permanently): pending and future waiters fail with
    /// `SaveFailed` until [`FlushTracker::release`] (Plan/13 §16.6).
    pub fn poison(&self, account: AccountId, since: UnixTime) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        inner.poisoned.entry(account).or_insert(since);
        for w in inner.waiters.remove(&account).unwrap_or_default() {
            let _ = w.reply.send(Err(save_failed(account)));
        }
    }

    /// Lifts the poison of `account` (admin console `db release <account>`). Returns whether it was poisoned.
    pub fn release(&self, account: AccountId) -> bool {
        self.inner
            .lock()
            .map(|mut g| g.poisoned.remove(&account).is_some())
            .unwrap_or(false)
    }

    /// `true` while `account` is poisoned.
    pub fn is_poisoned(&self, account: AccountId) -> bool {
        self.inner
            .lock()
            .map(|g| g.poisoned.contains_key(&account))
            .unwrap_or(false)
    }

    /// The poisoned accounts and since when, ordered by account.
    pub fn poisoned(&self) -> Vec<(AccountId, UnixTime)> {
        self.inner
            .lock()
            .map(|g| g.poisoned.iter().map(|(a, t)| (*a, *t)).collect())
            .unwrap_or_default()
    }

    /// Fails all pending waiters with `ShuttingDown` and refuses new ones (writer stopped).
    pub fn close(&self) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        inner.closed = true;
        for (_, list) in std::mem::take(&mut inner.waiters) {
            for w in list {
                let _ = w.reply.send(Err(DbError::shutting_down()));
            }
        }
    }
}
