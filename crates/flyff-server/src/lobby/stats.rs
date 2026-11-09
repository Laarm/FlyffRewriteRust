//! Lobby counters (Plan/12 §19.2; WP-AUTH-17): `flyff_lobby_login_total{result}`, `flyff_lobby_register_total{result}`,
//! `flyff_lobby_takeover_total{result}`, `flyff_lobby_reattach_total{result}`, `flyff_lobby_char_create_total{result}`,
//! `flyff_lobby_char_delete_total{result}` and the duration histograms `flyff_lobby_hash_wait_seconds`,
//! `flyff_lobby_hash_seconds`, `flyff_lobby_enter_world_seconds`, `flyff_lobby_flush_wait_seconds`. The text is
//! rendered by `metrics::render` (one exposition writer for the whole page); this module only counts.

use flyff_common::rt::Histogram;
use std::sync::atomic::{AtomicU64, Ordering};

/// The labels of `flyff_lobby_login_total{result=...}`: `ok` and the error codes of 12 §16.1.
pub const LOGIN_RESULTS: [&str; 13] = [
    "ok", "120", "121", "119", "134", "135", "103", "108", "109", "133", "136", "400", "other",
];

/// The labels of `flyff_lobby_register_total{result=...}`.
pub const REGISTER_RESULTS: [&str; 8] =
    ["ok", "100", "2000", "2002", "2003", "2004", "2005", "other"];

/// The labels of `flyff_lobby_takeover_total{result=...}`.
pub const TAKEOVER_RESULTS: [&str; 3] = ["ok", "timeout", "cancelled"];

/// The labels of `flyff_lobby_reattach_total{result=...}`.
pub const REATTACH_RESULTS: [&str; 2] = ["attached", "no_avatar"];

/// The labels of the character create and delete counters: `ok` and the `LobbyError` codes of 12 §16.2.
pub const CHAR_RESULTS: [&str; 14] = [
    "ok", "104", "105", "106", "109", "111", "112", "133", "134", "135", "136", "1316", "1319",
    "other",
];

fn slot(labels: &[&str], label: &str) -> usize {
    labels
        .iter()
        .position(|l| *l == label)
        .unwrap_or(labels.len().saturating_sub(1))
}

/// The counters.
#[derive(Debug, Default)]
pub struct LobbyStats {
    login: [AtomicU64; LOGIN_RESULTS.len()],
    register: [AtomicU64; REGISTER_RESULTS.len()],
    takeover: [AtomicU64; TAKEOVER_RESULTS.len()],
    reattach: [AtomicU64; REATTACH_RESULTS.len()],
    char_create: [AtomicU64; CHAR_RESULTS.len()],
    char_delete: [AtomicU64; CHAR_RESULTS.len()],
    /// Time a hash computation waited for one of the two permits.
    pub hash_wait: Histogram,
    /// Time of one hash computation.
    pub hash: Histogram,
    /// `EnterWorld` accepted until `Join` was sent.
    pub enter_world: Histogram,
    /// The flush waits of 13 §19 step 2 (login, `Hello`, `EnterWorld`, character-select returns).
    pub flush_wait: Histogram,
}

fn count<const N: usize>(a: &[AtomicU64; N], labels: &[&str], label: &str) {
    if let Some(c) = a.get(slot(labels, label)) {
        c.fetch_add(1, Ordering::Relaxed);
    }
}

fn read<const N: usize>(a: &[AtomicU64; N], labels: &[&str], label: &str) -> u64 {
    a.get(slot(labels, label))
        .map_or(0, |c| c.load(Ordering::Relaxed))
}

fn all<const N: usize>(
    a: &[AtomicU64; N],
    labels: &'static [&'static str],
) -> Vec<(&'static str, u64)> {
    labels
        .iter()
        .zip(a)
        .map(|(l, c)| (*l, c.load(Ordering::Relaxed)))
        .collect()
}

impl LobbyStats {
    /// Counts a login outcome (`"ok"` or the error code as text).
    pub fn login(&self, result: &str) {
        count(&self.login, &LOGIN_RESULTS, result);
    }

    /// Counts a registration outcome.
    pub fn register(&self, result: &str) {
        count(&self.register, &REGISTER_RESULTS, result);
    }

    /// Counts a takeover outcome.
    pub fn takeover(&self, result: &str) {
        count(&self.takeover, &TAKEOVER_RESULTS, result);
    }

    /// Counts a re-attach outcome (`attached` or `no_avatar`).
    pub fn reattach(&self, result: &str) {
        count(&self.reattach, &REATTACH_RESULTS, result);
    }

    /// Counts a character creation outcome (`ok` or the `LobbyError` code).
    pub fn char_create(&self, result: &str) {
        count(&self.char_create, &CHAR_RESULTS, result);
    }

    /// Counts a character deletion outcome (`ok` or the `LobbyError` code).
    pub fn char_delete(&self, result: &str) {
        count(&self.char_delete, &CHAR_RESULTS, result);
    }

    /// Adds one hash computation: milliseconds waited for the permit and spent computing.
    pub fn hash(&self, wait_ms: u64, compute_ms: u64) {
        self.hash_wait.observe_ns(wait_ms.saturating_mul(1_000_000));
        self.hash.observe_ns(compute_ms.saturating_mul(1_000_000));
    }

    /// The count of one login result (tests).
    pub fn login_count(&self, result: &str) -> u64 {
        read(&self.login, &LOGIN_RESULTS, result)
    }

    /// The count of one registration result (tests).
    pub fn register_count(&self, result: &str) -> u64 {
        read(&self.register, &REGISTER_RESULTS, result)
    }

    /// The count of one takeover result (tests).
    pub fn takeover_count(&self, result: &str) -> u64 {
        read(&self.takeover, &TAKEOVER_RESULTS, result)
    }

    /// The count of one re-attach result (tests).
    pub fn reattach_count(&self, result: &str) -> u64 {
        read(&self.reattach, &REATTACH_RESULTS, result)
    }

    /// The count of one character-creation result (tests).
    pub fn char_create_count(&self, result: &str) -> u64 {
        read(&self.char_create, &CHAR_RESULTS, result)
    }

    /// The count of one character-deletion result (tests).
    pub fn char_delete_count(&self, result: &str) -> u64 {
        read(&self.char_delete, &CHAR_RESULTS, result)
    }

    /// `(label, count)` of every login result.
    pub fn logins(&self) -> Vec<(&'static str, u64)> {
        all(&self.login, &LOGIN_RESULTS)
    }

    /// `(label, count)` of every registration result.
    pub fn registers(&self) -> Vec<(&'static str, u64)> {
        all(&self.register, &REGISTER_RESULTS)
    }

    /// `(label, count)` of every takeover result.
    pub fn takeovers(&self) -> Vec<(&'static str, u64)> {
        all(&self.takeover, &TAKEOVER_RESULTS)
    }

    /// `(label, count)` of every re-attach result.
    pub fn reattaches(&self) -> Vec<(&'static str, u64)> {
        all(&self.reattach, &REATTACH_RESULTS)
    }

    /// `(label, count)` of every character-creation result.
    pub fn char_creates(&self) -> Vec<(&'static str, u64)> {
        all(&self.char_create, &CHAR_RESULTS)
    }

    /// `(label, count)` of every character-deletion result.
    pub fn char_deletes(&self) -> Vec<(&'static str, u64)> {
        all(&self.char_delete, &CHAR_RESULTS)
    }
}
