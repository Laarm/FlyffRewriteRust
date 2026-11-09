//! The online registry (Plan/12 §7; WP-AUTH-03): one entry per logged-in account, created atomically at login
//! and held through character select, enter world, the logout linger and **until the final logout snapshot
//! is committed**. It is also the table of live session tokens.
//!
//! The registry is **synchronous**: one `parking_lot::Mutex<RegistryInner>`, held for microseconds and never
//! across an `.await` (rule LB1). Every transition of the table of §7.3 is one method that changes the state,
//! and returns the side effects as [`RegistryAction`]s which the caller executes after the lock is released
//! (`LobbyCore::run_actions`): kick a socket, send `GameInput::KickAccount`, spawn a drain task, tell a
//! session task. Waiting (for a flush, a takeover, an enter task) is done by the async callers; they re-check
//! the state after every wait.
//!
//! Invariants (tested, [`OnlineRegistry::check_invariants`]):
//!
//! * I1 at most one entry per account; at most one live token per entry; the token table holds exactly the
//!   live tokens,
//! * I2 `in_world` = the number of entries in `Entering | InWorld`,
//! * I3 a character is never loaded while an older snapshot of the account is queued: an entry leaves
//!   `Draining`/`Claimed` only after its `last_gen` is flushed (the callers wait for it before `Join`),
//! * I4 an entry is removed only from `Draining{save_gen}` after the flush of `save_gen` resolved, or from `Claimed`
//!   with `last_gen` already flushed,
//! * I5 a token is valid iff its entry exists, `!revoked`, is not `Draining`, and (a socket is attached or
//!   `now - socket_gone_at <= auth.token_grace_s`).
//!
//! Deviations from the plan's sketch: the change notification is one registry-wide `Notify` (so a waiter can
//! also wait for an entry to disappear); `RegistryEntry.token` is an `Option` (a revoked entry has no live
//! token); `Entering`/`InWorld` carry the session that sent `EnterWorld`.

use super::accounts::AccountName;
use super::tokens::{SessionToken, SessionTokens, TokenHash};
use flyff_common::gm::Authority;
use flyff_common::rt::{CloseReason, KickReason, LostCause};
use flyff_common::{AccountId, CharacterId, SaveGen, SessionId, UnixTime};
use flyff_db::PersistHandle;
use flyff_game::input::JoinFailReason;
use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::Notify;

// ---- clock ------------------------------------------------------------------------------------------------

/// The time source of the lobby: monotonic milliseconds for grace periods and Unix seconds for the throttles
/// and the stored times. A manual clock lets tests step time without sleeping.
#[derive(Clone, Debug)]
pub struct LobbyClock(Arc<ClockKind>);

#[derive(Debug)]
enum ClockKind {
    System { start: Instant },
    Manual { ms: AtomicU64, unix_base: i64 },
}

impl LobbyClock {
    /// The real clock.
    pub fn system() -> LobbyClock {
        LobbyClock(Arc::new(ClockKind::System {
            start: Instant::now(),
        }))
    }

    /// A manual clock starting at `unix_start` seconds and 0 ms; [`LobbyClock::advance`] moves it.
    pub fn manual(unix_start: i64) -> LobbyClock {
        LobbyClock(Arc::new(ClockKind::Manual {
            ms: AtomicU64::new(0),
            unix_base: unix_start,
        }))
    }

    /// Moves a manual clock forward (no effect on the system clock).
    pub fn advance(&self, by: Duration) {
        if let ClockKind::Manual { ms, .. } = &*self.0 {
            ms.fetch_add(
                u64::try_from(by.as_millis()).unwrap_or(u64::MAX),
                Ordering::SeqCst,
            );
        }
    }

    /// Monotonic milliseconds since the clock was created.
    pub fn now_ms(&self) -> u64 {
        match &*self.0 {
            ClockKind::System { start } => {
                u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)
            }
            ClockKind::Manual { ms, .. } => ms.load(Ordering::SeqCst),
        }
    }

    /// Wall-clock Unix seconds.
    pub fn unix(&self) -> UnixTime {
        match &*self.0 {
            ClockKind::System { .. } => UnixTime(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX)),
            ),
            ClockKind::Manual { ms, unix_base } => UnixTime(
                unix_base
                    .saturating_add(i64::try_from(ms.load(Ordering::SeqCst) / 1000).unwrap_or(0)),
            ),
        }
    }

    /// Wall-clock Unix milliseconds.
    pub fn unix_ms(&self) -> i64 {
        match &*self.0 {
            ClockKind::System { .. } => std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX)),
            ClockKind::Manual { ms, unix_base } => unix_base
                .saturating_mul(1000)
                .saturating_add(i64::try_from(ms.load(Ordering::SeqCst)).unwrap_or(0)),
        }
    }
}

/// What the registry needs to know about committed saves (`PersistHandle::flushed_gen`; a fake in tests).
pub trait FlushView: Send + Sync {
    /// The highest committed save generation of `account` (`SaveGen(0)` when none).
    fn flushed_gen(&self, account: AccountId) -> SaveGen;
}

impl FlushView for PersistHandle {
    fn flushed_gen(&self, account: AccountId) -> SaveGen {
        PersistHandle::flushed_gen(self, account)
    }
}

// ---- entry ------------------------------------------------------------------------------------------------

/// Set by a forced login, a ban or a socket loss to abort an enter task that has not sent `Join` yet
/// (rule EW1).
#[derive(Clone, Debug, Default)]
pub struct CancelFlag(Arc<AtomicBool>);

impl CancelFlag {
    /// A fresh, unset flag.
    pub fn new() -> CancelFlag {
        CancelFlag::default()
    }

    /// Sets the flag.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// `true` once [`CancelFlag::cancel`] was called.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// The state of an entry (12 §7.1).
#[derive(Clone, Debug)]
pub enum EntryState {
    /// Logged in, no avatar (character select, or between sockets).
    Claimed,
    /// `EnterWorld` accepted; the enter task has not sent `Join` yet.
    Entering {
        /// The chosen character.
        character: CharacterId,
        /// The session that sent `EnterWorld`.
        session: SessionId,
        /// Aborts the enter task.
        cancel: CancelFlag,
    },
    /// An avatar exists on the game thread (attached or lingering).
    InWorld {
        /// The character.
        character: CharacterId,
        /// The session that sent `Join`.
        session: SessionId,
    },
    /// Full logout; waiting for `wait_flushed(account, save_gen)`; the token is revoked.
    Draining {
        /// The generation that must be committed.
        save_gen: SaveGen,
    },
}

/// The shape of an [`EntryState`] without payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    /// [`EntryState::Claimed`].
    Claimed,
    /// [`EntryState::Entering`].
    Entering,
    /// [`EntryState::InWorld`].
    InWorld,
    /// [`EntryState::Draining`].
    Draining,
}

impl EntryState {
    /// The payload-free kind.
    pub fn kind(&self) -> EntryKind {
        match self {
            EntryState::Claimed => EntryKind::Claimed,
            EntryState::Entering { .. } => EntryKind::Entering,
            EntryState::InWorld { .. } => EntryKind::InWorld,
            EntryState::Draining { .. } => EntryKind::Draining,
        }
    }
}

/// One logged-in account (12 §7.1).
#[derive(Debug)]
pub struct RegistryEntry {
    /// The account.
    pub account: AccountId,
    /// Its normalised name.
    pub name: AccountName,
    /// Authority resolved at login (account + IP grants): `HelloAck.authority`.
    pub account_authority: Authority,
    /// The live token, `None` once revoked (kick, ban, takeover, drain).
    pub token: Option<TokenHash>,
    /// The state.
    pub state: EntryState,
    /// The WS session currently attached.
    pub socket: Option<SessionId>,
    /// Registry-clock milliseconds of the issue time, or of the moment the last socket closed.
    pub socket_gone_at: u64,
    /// Highest `final_gen` reported by `NetEvent::Left` (0 = nothing pending).
    pub last_gen: SaveGen,
    /// The token was revoked: the next `Left{Close}` goes to `Draining`.
    pub revoked: bool,
    /// A forced login waits for this entry (its takeover id).
    pub takeover: Option<u64>,
    /// An account ban is active (cached by `AccountAdmin::ban`; `Hello` answers `BANNED`).
    pub banned: bool,
    /// The client address of the login.
    pub ip: IpAddr,
}

/// A read-only copy of an entry (what callers and tests look at).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryView {
    /// The account.
    pub account: AccountId,
    /// Its normalised name.
    pub name: AccountName,
    /// The login authority.
    pub authority: Authority,
    /// The state kind.
    pub kind: EntryKind,
    /// The character of an `Entering`/`InWorld` entry.
    pub character: Option<CharacterId>,
    /// The generation a `Draining` entry waits for.
    pub draining_gen: Option<SaveGen>,
    /// The attached session.
    pub socket: Option<SessionId>,
    /// Highest pending save generation.
    pub last_gen: SaveGen,
    /// The token was revoked.
    pub revoked: bool,
    /// A forced login waits.
    pub takeover_pending: bool,
    /// The account is banned.
    pub banned: bool,
    /// `true` while the entry holds a live token.
    pub has_token: bool,
}

impl RegistryEntry {
    fn view(&self) -> EntryView {
        let (character, draining_gen) = match &self.state {
            EntryState::Claimed => (None, None),
            EntryState::Entering { character, .. } | EntryState::InWorld { character, .. } => {
                (Some(*character), None)
            }
            EntryState::Draining { save_gen } => (None, Some(*save_gen)),
        };
        EntryView {
            account: self.account,
            name: self.name.clone(),
            authority: self.account_authority,
            kind: self.state.kind(),
            character,
            draining_gen,
            socket: self.socket,
            last_gen: self.last_gen,
            revoked: self.revoked,
            takeover_pending: self.takeover.is_some(),
            banned: self.banned,
            has_token: self.token.is_some(),
        }
    }
}

// ---- actions ----------------------------------------------------------------------------------------------

/// A side effect of a registry transition, executed by the caller after the lock is released (rule LB1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegistryAction {
    /// Close the socket of `session` (the gateway sends the `Kick` frame that belongs to the reason).
    CloseSocket {
        /// The session.
        session: SessionId,
        /// The reason (`Kicked(DuplicateLogin)`, `Kicked(Banned)`, `Replaced`).
        reason: CloseReason,
    },
    /// `GameInput::KickAccount{account, reason}` (the game closes the socket and applies the leave rule of
    /// the reason, 12 §12.2).
    KickAccount {
        /// The account.
        account: AccountId,
        /// The reason.
        reason: KickReason,
    },
    /// Spawn the drain task: wait for the flush of `save_gen`, then remove the entry (13 §19 step 4).
    SpawnDrain {
        /// The account.
        account: AccountId,
        /// The generation to wait for.
        save_gen: SaveGen,
    },
    /// The avatar went back to character select: the session continues there (wait for the flush of
    /// `last_gen`, send the `CharList`).
    EnterCharSelect {
        /// The attached session.
        session: SessionId,
        /// The account.
        account: AccountId,
        /// The generation to wait for before the list is read.
        last_gen: SaveGen,
    },
    /// `NetEvent::JoinFailed`: `LobbyError(map(reason))`, the session is back at character select.
    JoinFailed {
        /// The session of the join.
        session: SessionId,
        /// The account.
        account: AccountId,
        /// Why.
        reason: JoinFailReason,
    },
    /// Revoke the login-gated asset cookies of the account (M8, 24 §13).
    RevokeAssets {
        /// The account.
        account: AccountId,
    },
}

/// Why [`OnlineRegistry::admin_hold`] refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoldRefusal {
    /// The account has an entry (it is logged in, or its logout is still draining): `409 character_online`.
    Online(EntryKind),
    /// Another administrator holds the account at this moment: retry.
    Held,
}

/// An admin hold on an account (see [`OnlineRegistry::admin_hold`]); released on drop.
pub struct AdminHold<'a> {
    registry: &'a OnlineRegistry,
    account: AccountId,
}

impl AdminHold<'_> {
    /// The held account.
    pub fn account(&self) -> AccountId {
        self.account
    }
}

impl std::fmt::Debug for AdminHold<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdminHold")
            .field("account", &self.account)
            .finish()
    }
}

impl Drop for AdminHold<'_> {
    fn drop(&mut self) {
        self.registry.inner.lock().holds.remove(&self.account);
    }
}

// ---- the registry -----------------------------------------------------------------------------------------

pub(super) struct RegistryInner {
    pub(super) by_account: BTreeMap<AccountId, RegistryEntry>,
    pub(super) tokens: SessionTokens,
    /// Entries in `Entering | InWorld`.
    pub(super) in_world: u32,
    /// Attached sockets.
    pub(super) by_socket: BTreeMap<SessionId, AccountId>,
    /// The sessions of `Entering | InWorld` entries (a `JoinFailed` event carries only the session).
    pub(super) by_join_session: BTreeMap<SessionId, AccountId>,
    /// Accounts reserved by an admin offline edit ([`OnlineRegistry::admin_hold`], 25 §4.6): no entry can be
    /// created for them while the hold exists.
    holds: std::collections::BTreeSet<AccountId>,
    next_takeover: u64,
}

/// The registry (12 §7.1).
pub struct OnlineRegistry {
    pub(super) inner: Mutex<RegistryInner>,
    pub(super) clock: LobbyClock,
    grace_ms: u64,
    changed: Notify,
}

/// What a login asks of the registry ([`OnlineRegistry::claim`]).
#[derive(Clone, Debug)]
pub struct ClaimRequest<'a> {
    /// The account.
    pub account: AccountId,
    /// Its normalised name.
    pub name: &'a AccountName,
    /// The authority resolved for the login.
    pub authority: Authority,
    /// The client address.
    pub ip: IpAddr,
    /// `force = true` of the request (the client confirmed "cut the existing connection").
    pub force: bool,
    /// `auth.max_online_accounts`.
    pub max_accounts: u32,
    /// The takeover id returned by an earlier [`ClaimStep::WaitTakeover`] of the same login.
    pub takeover: Option<u64>,
}

/// The answer of one claim attempt (12 §7.3, §14): either a verdict or something to wait for, after which the
/// caller calls [`OnlineRegistry::claim`] again.
#[derive(Debug)]
pub enum ClaimStep {
    /// A new token was issued (a fresh entry, or a silent takeover of an entry without socket or avatar).
    Issued {
        /// The secret; shown once in the login response.
        token: SessionToken,
        /// Side effects (kick the old socket).
        actions: Vec<RegistryAction>,
    },
    /// `409 / 103`: connected and `force` is not set (or another takeover is pending).
    Duplicate,
    /// `503 / 108`: `auth.max_online_accounts` reached and the account has no entry.
    Overflow,
    /// `500 / 136`: the OS random generator failed, no token could be issued.
    NoRandom,
    /// The entry is `Draining{save_gen}` and `save_gen` is not flushed yet: wait for the flush (13 §19), then claim again.
    WaitFlush {
        /// The generation to wait for.
        save_gen: SaveGen,
    },
    /// `Entering` + `force`: the enter task was cancelled and the old socket kicked; wait (at most 5 s) for the
    /// entry to leave `Entering`, then claim again.
    WaitEnter {
        /// Side effects (kick the old socket).
        actions: Vec<RegistryAction>,
    },
    /// `503 retry`: an administrator holds the account for an offline edit of one of its characters
    /// ([`OnlineRegistry::admin_hold`], 25 §4.6); the hold is short (one database commit).
    Held,
    /// `InWorld` + `force`: the entry is revoked and `KickAccount` is sent; wait (at most
    /// `auth.takeover_wait_s`) for the avatar's `Left`, then claim again with this id.
    WaitTakeover {
        /// Pass it back in [`ClaimRequest::takeover`].
        id: u64,
        /// Side effects (`KickAccount`).
        actions: Vec<RegistryAction>,
    },
}

/// The verdict of a `Hello` ([`OnlineRegistry::attach`]).
#[derive(Debug)]
pub enum AttachResult {
    /// Unknown, expired, revoked or malformed token (`HelloAck{BAD_TOKEN}`, close 4013).
    BadToken,
    /// The account is banned (`HelloAck{BANNED}`, close 4008); the socket is not attached.
    Banned,
    /// The socket is attached.
    Ok(AttachInfo),
}

/// What an attached `Hello` learns about its entry.
#[derive(Clone, Debug)]
pub struct AttachInfo {
    /// The account.
    pub account: AccountId,
    /// Its name.
    pub name: AccountName,
    /// `HelloAck.authority`.
    pub authority: Authority,
    /// The state kind of the entry.
    pub kind: EntryKind,
    /// The character of an `Entering`/`InWorld` entry.
    pub character: Option<CharacterId>,
    /// Highest pending save generation: the character list waits for it.
    pub last_gen: SaveGen,
    /// The socket that was attached before (to be closed by the caller, `CloseReason::Replaced`).
    pub replaced: Option<SessionId>,
}

/// Why [`OnlineRegistry::begin_enter`] refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnterRefusal {
    /// No entry, or the token was revoked.
    NoEntry,
    /// The entry is not `Claimed`, or the session is not the attached socket.
    NotClaimed,
}

impl OnlineRegistry {
    /// An empty registry; `token_grace_s` is `auth.token_grace_s`.
    pub fn new(clock: LobbyClock, token_grace_s: u32) -> OnlineRegistry {
        OnlineRegistry {
            inner: Mutex::new(RegistryInner {
                by_account: BTreeMap::new(),
                tokens: SessionTokens::new(),
                in_world: 0,
                by_socket: BTreeMap::new(),
                by_join_session: BTreeMap::new(),
                holds: std::collections::BTreeSet::new(),
                next_takeover: 1,
            }),
            clock,
            grace_ms: u64::from(token_grace_s).saturating_mul(1000),
            changed: Notify::new(),
        }
    }

    /// The clock of the registry.
    pub fn clock(&self) -> &LobbyClock {
        &self.clock
    }

    /// Number of entries (the `max_online_accounts` measure).
    pub fn len(&self) -> usize {
        self.inner.lock().by_account.len()
    }

    /// `true` when nobody is logged in.
    pub fn is_empty(&self) -> bool {
        self.inner.lock().by_account.is_empty()
    }

    /// Entries in `Entering | InWorld` (the `max_in_world` measure).
    pub fn in_world(&self) -> u32 {
        self.inner.lock().in_world
    }

    /// A copy of the entry of `account`.
    pub fn view(&self, account: AccountId) -> Option<EntryView> {
        self.inner
            .lock()
            .by_account
            .get(&account)
            .map(RegistryEntry::view)
    }

    /// Copies of every entry, ascending by account (admin listings, tests).
    pub fn views(&self) -> Vec<EntryView> {
        self.inner
            .lock()
            .by_account
            .values()
            .map(RegistryEntry::view)
            .collect()
    }

    /// The client address of the login of `account` (the admin listings show it next to the avatar).
    pub fn ip_of(&self, account: AccountId) -> Option<IpAddr> {
        self.inner.lock().by_account.get(&account).map(|e| e.ip)
    }

    /// The registry-wide change notification (`Notify::notify_waiters` on every transition).
    pub fn changed(&self) -> &Notify {
        &self.changed
    }

    fn notify(&self) {
        self.changed.notify_waiters();
    }

    /// Waits until `pred` holds for the entry of `account` (`None` = no entry), at most `timeout`; `true` when
    /// it held. The state is re-evaluated after every transition, so no wake-up is lost.
    pub async fn wait_for(
        &self,
        account: AccountId,
        timeout: Duration,
        mut pred: impl FnMut(Option<&EntryView>) -> bool,
    ) -> bool {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            // Register interest before looking at the state.
            notified.as_mut().enable();
            let view = self.view(account);
            if pred(view.as_ref()) {
                return true;
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                let view = self.view(account);
                return pred(view.as_ref());
            }
        }
    }

    // ---- admin holds ----------------------------------------------------------------------------------

    /// Reserves `account` for an admin offline edit of one of its characters (25 §4.6, 12 §7 addendum): `Err` while
    /// the account has an entry in **any** state (`Claimed`, `Entering`, `InWorld`, `Draining`: it is logged in or
    /// its final save is not drained yet) or another hold exists. While the returned guard lives, a login of the
    /// account is answered `503 retry` ([`ClaimStep::Held`]) and no entry can appear; dropping it releases the hold.
    pub fn admin_hold(&self, account: AccountId) -> Result<AdminHold<'_>, HoldRefusal> {
        let mut inner = self.inner.lock();
        if let Some(e) = inner.by_account.get(&account) {
            return Err(HoldRefusal::Online(e.state.kind()));
        }
        if !inner.holds.insert(account) {
            return Err(HoldRefusal::Held);
        }
        Ok(AdminHold {
            registry: self,
            account,
        })
    }

    /// `true` while an admin hold exists for `account`.
    pub fn is_held(&self, account: AccountId) -> bool {
        self.inner.lock().holds.contains(&account)
    }

    // ---- login ----------------------------------------------------------------------------------------

    /// One attempt of the claim of §7.3 / §14. Creates the entry atomically, silently takes over an entry
    /// without socket or avatar, and for every other state either refuses, or starts the forced takeover and
    /// says what to wait for. The caller repeats the call after each wait (the state is re-checked).
    pub fn claim(&self, req: &ClaimRequest<'_>, flush: &dyn FlushView) -> ClaimStep {
        let now = self.clock.now_ms();
        let mut guard = self.inner.lock();
        let inner = &mut *guard;
        if inner.holds.contains(&req.account) {
            return ClaimStep::Held;
        }
        let Some(entry) = inner.by_account.get_mut(&req.account) else {
            if u32::try_from(inner.by_account.len()).unwrap_or(u32::MAX) >= req.max_accounts {
                return ClaimStep::Overflow;
            }
            let Some((token, hash)) = issue(&mut inner.tokens, req.account) else {
                return ClaimStep::NoRandom;
            };
            inner.by_account.insert(
                req.account,
                RegistryEntry {
                    account: req.account,
                    name: req.name.clone(),
                    account_authority: req.authority,
                    token: Some(hash),
                    state: EntryState::Claimed,
                    socket: None,
                    socket_gone_at: now,
                    last_gen: SaveGen::ZERO,
                    revoked: false,
                    takeover: None,
                    banned: false,
                    ip: req.ip,
                },
            );
            drop(guard);
            self.notify();
            return ClaimStep::Issued {
                token,
                actions: Vec::new(),
            };
        };

        // one takeover at a time: only the login that started it may continue
        if entry.takeover.is_some() && entry.takeover != req.takeover {
            return ClaimStep::Duplicate;
        }
        let mut actions = Vec::new();
        match &entry.state {
            EntryState::Draining { save_gen } => {
                let save_gen = *save_gen;
                if flush.flushed_gen(req.account) < save_gen {
                    return ClaimStep::WaitFlush { save_gen };
                }
                // flushed: a fresh entry (last_gen = save_gen)
                entry.revoked = false;
                entry.banned = false;
                entry.takeover = None;
                entry.state = EntryState::Claimed;
                entry.last_gen = save_gen;
                if let Some(old) = entry.socket.take() {
                    // a socket that outlived its avatar (the game closes it asynchronously)
                    inner.by_socket.remove(&old);
                    actions.push(RegistryAction::CloseSocket {
                        session: old,
                        reason: CloseReason::Kicked(KickReason::DuplicateLogin),
                    });
                }
            }
            EntryState::Claimed => {
                if let Some(old) = entry.socket {
                    if !req.force {
                        return ClaimStep::Duplicate;
                    }
                    actions.push(RegistryAction::CloseSocket {
                        session: old,
                        reason: CloseReason::Kicked(KickReason::DuplicateLogin),
                    });
                    inner.by_socket.remove(&old);
                    entry.socket = None;
                }
            }
            EntryState::Entering { cancel, .. } => {
                if !req.force {
                    return ClaimStep::Duplicate;
                }
                cancel.cancel();
                if let Some(old) = entry.socket.take() {
                    actions.push(RegistryAction::CloseSocket {
                        session: old,
                        reason: CloseReason::Kicked(KickReason::DuplicateLogin),
                    });
                    inner.by_socket.remove(&old);
                    entry.socket_gone_at = now;
                }
                if let Some(hash) = entry.token.take() {
                    inner.tokens.revoke(&hash);
                }
                entry.revoked = true;
                drop(guard);
                self.notify();
                return ClaimStep::WaitEnter { actions };
            }
            EntryState::InWorld { .. } => {
                if !req.force {
                    return ClaimStep::Duplicate;
                }
                let (id, first) = match entry.takeover {
                    Some(id) => (id, false),
                    None => {
                        let id = inner.next_takeover;
                        inner.next_takeover += 1;
                        entry.takeover = Some(id);
                        (id, true)
                    }
                };
                if first {
                    entry.revoked = true;
                    if let Some(hash) = entry.token.take() {
                        inner.tokens.revoke(&hash);
                    }
                    actions.push(RegistryAction::KickAccount {
                        account: req.account,
                        reason: KickReason::DuplicateLogin,
                    });
                }
                drop(guard);
                self.notify();
                return ClaimStep::WaitTakeover { id, actions };
            }
        }
        // Claimed without (or with a just-kicked) socket: the old token dies, a new one is issued.
        if let Some(hash) = entry.token.take() {
            inner.tokens.revoke(&hash);
        }
        let Some((token, hash)) = issue(&mut inner.tokens, req.account) else {
            // no token without randomness: the entry stays revoked until the sweeper removes it
            entry.revoked = true;
            return ClaimStep::NoRandom;
        };
        entry.token = Some(hash);
        entry.account_authority = req.authority;
        entry.name = req.name.clone();
        entry.ip = req.ip;
        entry.socket_gone_at = now;
        entry.revoked = false;
        entry.takeover = None;
        entry.banned = false;
        drop(guard);
        self.notify();
        ClaimStep::Issued { token, actions }
    }

    /// Clears the takeover marker of a login that gave up or whose HTTP request was dropped (12 §14 step 3).
    /// Only the owner of the marker may clear it.
    pub fn cancel_takeover(&self, account: AccountId, id: u64) -> bool {
        let mut inner = self.inner.lock();
        let cleared = match inner.by_account.get_mut(&account) {
            Some(e) if e.takeover == Some(id) => {
                e.takeover = None;
                true
            }
            _ => false,
        };
        drop(inner);
        if cleared {
            self.notify();
        }
        cleared
    }

    // ---- tokens and sockets ---------------------------------------------------------------------------

    fn token_valid(&self, e: &RegistryEntry, now: u64) -> bool {
        !e.revoked
            && e.token.is_some()
            && !matches!(e.state, EntryState::Draining { .. })
            && (e.socket.is_some() || now.saturating_sub(e.socket_gone_at) <= self.grace_ms)
    }

    /// The account of a valid token (I5), without attaching anything.
    pub fn validate(&self, token: &SessionToken) -> Option<AccountId> {
        let hash = token.hash();
        let now = self.clock.now_ms();
        let inner = self.inner.lock();
        let account = inner.tokens.lookup(&hash)?;
        let e = inner.by_account.get(&account)?;
        (e.token == Some(hash) && self.token_valid(e, now)).then_some(account)
    }

    /// `Hello` (12 §8.2 steps 5-7): checks the token (I5), refuses a banned account, and attaches `session` as
    /// the socket of the entry, returning the socket that was attached before (to be kicked).
    pub fn attach(&self, token: &SessionToken, session: SessionId) -> AttachResult {
        let hash = token.hash();
        let now = self.clock.now_ms();
        let mut guard = self.inner.lock();
        let inner = &mut *guard;
        let Some(account) = inner.tokens.lookup(&hash) else {
            return AttachResult::BadToken;
        };
        let Some(e) = inner.by_account.get_mut(&account) else {
            return AttachResult::BadToken;
        };
        if e.token != Some(hash) || !self.token_valid(e, now) {
            return AttachResult::BadToken;
        }
        if e.banned {
            return AttachResult::Banned;
        }
        let replaced = e.socket.replace(session).filter(|old| *old != session);
        let character = match &e.state {
            EntryState::Entering { character, .. } | EntryState::InWorld { character, .. } => {
                Some(*character)
            }
            _ => None,
        };
        let info = AttachInfo {
            account,
            name: e.name.clone(),
            authority: e.account_authority,
            kind: e.state.kind(),
            character,
            last_gen: e.last_gen,
            replaced,
        };
        if let Some(old) = replaced {
            inner.by_socket.remove(&old);
        }
        // a session serves one account: an earlier binding of this session to another entry ends
        if let Some(prev) = inner.by_socket.insert(session, account)
            && prev != account
            && let Some(other) = inner.by_account.get_mut(&prev)
            && other.socket == Some(session)
        {
            other.socket = None;
            other.socket_gone_at = now;
        }
        drop(guard);
        self.notify();
        AttachResult::Ok(info)
    }

    /// The socket of `session` ended (`LobbyPort::on_session_end`, 12 §8.1): the entry loses its socket and the
    /// grace period starts; an enter task of that session is cancelled (rule EW1); a kick issued by the game
    /// (`Kicked(GmKick | Banned)`) revokes the token (12 §7.3 last row).
    pub fn socket_closed(&self, session: SessionId, cause: LostCause) -> Vec<RegistryAction> {
        let now = self.clock.now_ms();
        let mut guard = self.inner.lock();
        let inner = &mut *guard;
        let mut actions = Vec::new();
        let Some(account) = inner.by_socket.remove(&session) else {
            return actions;
        };
        let Some(e) = inner.by_account.get_mut(&account) else {
            return actions;
        };
        if e.socket == Some(session) {
            e.socket = None;
            e.socket_gone_at = now;
        }
        if let EntryState::Entering {
            cancel,
            session: enter,
            ..
        } = &e.state
            && *enter == session
        {
            cancel.cancel();
        }
        if let LostCause::Kicked(KickReason::GmKick | KickReason::Banned) = cause {
            let banned = matches!(cause, LostCause::Kicked(KickReason::Banned));
            revoke_entry(
                inner.by_account.get_mut(&account),
                &mut inner.tokens,
                banned,
            );
            if banned {
                actions.push(RegistryAction::RevokeAssets { account });
            }
            if let Some(e) = inner.by_account.get_mut(&account)
                && matches!(e.state, EntryState::Claimed)
            {
                e.state = EntryState::Draining {
                    save_gen: e.last_gen,
                };
                actions.push(RegistryAction::SpawnDrain {
                    account,
                    save_gen: e.last_gen,
                });
            }
        }
        drop(guard);
        self.notify();
        actions
    }

    // ---- enter world ----------------------------------------------------------------------------------

    /// `EnterWorld` accepted (12 §11.1 step 4): `Claimed` -> `Entering`, `in_world += 1`. `session` must be the
    /// attached socket. The returned flag aborts the enter task.
    pub fn begin_enter(
        &self,
        account: AccountId,
        session: SessionId,
        character: CharacterId,
    ) -> Result<CancelFlag, EnterRefusal> {
        let mut guard = self.inner.lock();
        let inner = &mut *guard;
        let e = inner
            .by_account
            .get_mut(&account)
            .ok_or(EnterRefusal::NoEntry)?;
        if e.revoked {
            return Err(EnterRefusal::NoEntry);
        }
        if !matches!(e.state, EntryState::Claimed) || e.socket != Some(session) {
            return Err(EnterRefusal::NotClaimed);
        }
        // a session joins one avatar (a session id is never shared by two accounts)
        if inner
            .by_join_session
            .get(&session)
            .is_some_and(|a| *a != account)
        {
            return Err(EnterRefusal::NotClaimed);
        }
        let cancel = CancelFlag::new();
        e.state = EntryState::Entering {
            character,
            session,
            cancel: cancel.clone(),
        };
        inner.in_world += 1;
        inner.by_join_session.insert(session, account);
        drop(guard);
        self.notify();
        Ok(cancel)
    }

    /// The enter task is about to send `Join` (12 §11.1 step 9): `Entering` -> `InWorld`. Call it **before** the
    /// send, so that a very fast `Left`/`JoinFailed` of the game finds `InWorld`; `false` when the entry was
    /// cancelled or changed in the meantime (the task aborts instead).
    pub fn enter_joined(&self, account: AccountId, session: SessionId) -> bool {
        let mut inner = self.inner.lock();
        let Some(e) = inner.by_account.get_mut(&account) else {
            return false;
        };
        let ok = match &e.state {
            EntryState::Entering {
                character,
                session: s,
                cancel,
            } if *s == session && !cancel.is_cancelled() && !e.revoked => {
                e.state = EntryState::InWorld {
                    character: *character,
                    session,
                };
                true
            }
            _ => false,
        };
        drop(inner);
        if ok {
            self.notify();
        }
        ok
    }

    /// The enter task gave up before or while sending `Join` (flush timeout, load error, cancel, socket closed,
    /// send failed): `Entering | InWorld` of that session -> `Claimed` (`Draining` when the entry was revoked),
    /// `in_world -= 1`.
    pub fn enter_aborted(&self, account: AccountId, session: SessionId) -> Vec<RegistryAction> {
        let mut guard = self.inner.lock();
        let inner = &mut *guard;
        let mut actions = Vec::new();
        let Some(e) = inner.by_account.get_mut(&account) else {
            return actions;
        };
        let matches_session = match &e.state {
            EntryState::Entering { session: s, .. } | EntryState::InWorld { session: s, .. } => {
                *s == session
            }
            _ => false,
        };
        if !matches_session {
            return actions;
        }
        inner.by_join_session.remove(&session);
        inner.in_world = inner.in_world.saturating_sub(1);
        e.state = EntryState::Claimed;
        if e.revoked {
            e.state = EntryState::Draining {
                save_gen: e.last_gen,
            };
            actions.push(RegistryAction::SpawnDrain {
                account,
                save_gen: e.last_gen,
            });
        }
        drop(guard);
        self.notify();
        actions
    }

    /// A re-attach succeeded (12 RA2/RA3, WP-AUTH-16): the game bound the avatar of the `InWorld` entry to the new
    /// socket `session`. The entry's join session becomes `session`, so the `Left` that ends the avatar (its
    /// session is the last socket of the avatar) finds the entry's attached socket again. `false` when the entry
    /// is gone or no longer `InWorld` (the lobby then falls back to the normal path).
    pub fn reattached(&self, account: AccountId, session: SessionId) -> bool {
        let mut guard = self.inner.lock();
        let inner = &mut *guard;
        let Some(e) = inner.by_account.get_mut(&account) else {
            return false;
        };
        let EntryState::InWorld {
            session: joined, ..
        } = &mut e.state
        else {
            return false;
        };
        let old = std::mem::replace(joined, session);
        if old != session {
            inner.by_join_session.remove(&old);
        }
        inner.by_join_session.insert(session, account);
        drop(guard);
        self.notify();
        true
    }

    // ---- bans -----------------------------------------------------------------------------------------

    /// A ban was committed (`AccountAdmin::ban`, 12 §5.7, §7.3): the token is revoked, the banned flag is
    /// cached, a character-select socket gets `Kick{BANNED}`, an avatar is kicked (immediate logout, 12 §12.2).
    pub fn on_ban(&self, account: AccountId) -> Vec<RegistryAction> {
        let mut guard = self.inner.lock();
        let inner = &mut *guard;
        let mut actions = vec![RegistryAction::RevokeAssets { account }];
        let Some(e) = inner.by_account.get_mut(&account) else {
            return actions;
        };
        revoke_entry(Some(&mut *e), &mut inner.tokens, true);
        match &e.state {
            EntryState::Claimed => {
                if let Some(s) = e.socket.take() {
                    inner.by_socket.remove(&s);
                    e.socket_gone_at = self.clock.now_ms();
                    actions.push(RegistryAction::CloseSocket {
                        session: s,
                        reason: CloseReason::Kicked(KickReason::Banned),
                    });
                }
                e.state = EntryState::Draining {
                    save_gen: e.last_gen,
                };
                actions.push(RegistryAction::SpawnDrain {
                    account,
                    save_gen: e.last_gen,
                });
            }
            EntryState::Entering { cancel, .. } => {
                cancel.cancel();
                if let Some(s) = e.socket.take() {
                    inner.by_socket.remove(&s);
                    e.socket_gone_at = self.clock.now_ms();
                    actions.push(RegistryAction::CloseSocket {
                        session: s,
                        reason: CloseReason::Kicked(KickReason::Banned),
                    });
                }
            }
            EntryState::InWorld { .. } => {
                actions.push(RegistryAction::KickAccount {
                    account,
                    reason: KickReason::Banned,
                });
            }
            EntryState::Draining { .. } => {}
        }
        drop(guard);
        self.notify();
        actions
    }

    /// A ban was lifted: the cached flag is cleared (the token stays revoked: the account logs in again).
    pub fn on_unban(&self, account: AccountId) {
        if let Some(e) = self.inner.lock().by_account.get_mut(&account) {
            e.banned = false;
        }
        self.notify();
    }

    // ---- sweeper and drain ----------------------------------------------------------------------------

    /// The 1 Hz sweep (12 §7.3 "`Claimed`, `socket = None`"): a `Claimed` entry whose last socket closed more
    /// than `auth.token_grace_s` ago is removed when its `last_gen` is flushed, else it becomes
    /// `Draining{last_gen}` (and a drain task is spawned).
    pub fn sweep(&self, flush: &dyn FlushView) -> Vec<RegistryAction> {
        let now = self.clock.now_ms();
        let mut guard = self.inner.lock();
        let inner = &mut *guard;
        let expired: Vec<AccountId> = inner
            .by_account
            .values()
            .filter(|e| {
                matches!(e.state, EntryState::Claimed)
                    && e.socket.is_none()
                    && now.saturating_sub(e.socket_gone_at) > self.grace_ms
            })
            .map(|e| e.account)
            .collect();
        let mut actions = Vec::new();
        for account in &expired {
            let Some(e) = inner.by_account.get_mut(account) else {
                continue;
            };
            if flush.flushed_gen(*account) >= e.last_gen {
                remove_entry(inner, *account);
            } else {
                if let Some(hash) = e.token.take() {
                    inner.tokens.revoke(&hash);
                }
                e.revoked = true;
                e.state = EntryState::Draining {
                    save_gen: e.last_gen,
                };
                actions.push(RegistryAction::SpawnDrain {
                    account: *account,
                    save_gen: e.last_gen,
                });
            }
        }
        drop(guard);
        if !expired.is_empty() {
            self.notify();
        }
        actions
    }

    /// The drain task's `wait_flushed(account, save_gen)` resolved `Ok`: the entry is removed if it is still
    /// `Draining{save_gen}`.
    pub fn drain_done(&self, account: AccountId, save_gen: SaveGen) -> bool {
        let mut inner = self.inner.lock();
        let still = matches!(
            inner.by_account.get(&account).map(|e| &e.state),
            Some(EntryState::Draining { save_gen: g }) if *g == save_gen
        );
        if still {
            remove_entry(&mut inner, account);
        }
        drop(inner);
        if still {
            self.notify();
        }
        still
    }

    // ---- invariants (tests) ---------------------------------------------------------------------------

    /// Checks I1 and I2 and the consistency of the indexes; `Err` names the first violation.
    pub fn check_invariants(&self) -> Result<(), String> {
        let inner = self.inner.lock();
        // an admin hold only exists for an account without an entry (25 §4.6)
        if let Some(held) = inner
            .holds
            .iter()
            .find(|a| inner.by_account.contains_key(a))
        {
            return Err(format!("{held:?} is held and has an entry"));
        }
        let mut tokens = 0usize;
        let mut world = 0u32;
        let mut sockets = 0usize;
        let mut joins = 0usize;
        for (account, e) in &inner.by_account {
            if e.account != *account {
                return Err(format!(
                    "entry key {account:?} != entry.account {:?}",
                    e.account
                ));
            }
            let live = e.token.is_some();
            if live {
                tokens += 1;
            }
            if let Some(h) = &e.token {
                if !inner.tokens.is_live_for(h, *account) {
                    return Err(format!("{account:?}: token is not in the token table"));
                }
                if e.revoked || matches!(e.state, EntryState::Draining { .. }) {
                    return Err(format!(
                        "{account:?}: revoked or draining entry keeps a token"
                    ));
                }
            } else if !e.revoked && !matches!(e.state, EntryState::Draining { .. }) {
                return Err(format!("{account:?}: live entry without token"));
            }
            match &e.state {
                EntryState::Entering { session, .. } | EntryState::InWorld { session, .. } => {
                    world += 1;
                    joins += 1;
                    if inner.by_join_session.get(session) != Some(account) {
                        return Err(format!("{account:?}: join session {session:?} not indexed"));
                    }
                }
                EntryState::Draining { .. } | EntryState::Claimed => {}
            }
            if let Some(s) = e.socket {
                sockets += 1;
                if inner.by_socket.get(&s) != Some(account) {
                    return Err(format!("{account:?}: socket {s:?} not indexed"));
                }
            }
        }
        if tokens != inner.tokens.len() {
            return Err(format!(
                "token table holds {} tokens, entries hold {tokens}",
                inner.tokens.len()
            ));
        }
        if world != inner.in_world {
            return Err(format!(
                "in_world is {}, entries say {world}",
                inner.in_world
            ));
        }
        if sockets != inner.by_socket.len() {
            return Err(format!(
                "socket index holds {}, entries hold {sockets}",
                inner.by_socket.len()
            ));
        }
        if joins != inner.by_join_session.len() {
            return Err(format!(
                "join index holds {}, entries hold {joins}",
                inner.by_join_session.len()
            ));
        }
        Ok(())
    }
}

/// Removes an entry with its token and its index rows (an entry in the world is never removed).
fn remove_entry(inner: &mut RegistryInner, account: AccountId) {
    if let Some(e) = inner.by_account.remove(&account) {
        if let Some(hash) = e.token {
            inner.tokens.revoke(&hash);
        }
        if let Some(s) = e.socket {
            inner.by_socket.remove(&s);
        }
        if let EntryState::Entering { session, .. } | EntryState::InWorld { session, .. } = e.state
        {
            inner.by_join_session.remove(&session);
            inner.in_world = inner.in_world.saturating_sub(1);
        }
    }
}

/// Issues a token for `account`; `None` only when the OS CSPRNG fails.
fn issue(tokens: &mut SessionTokens, account: AccountId) -> Option<(SessionToken, TokenHash)> {
    match tokens.issue(account) {
        Ok(pair) => Some(pair),
        Err(e) => {
            tracing::error!(error = %e, "the OS random generator failed: cannot issue a session token");
            None
        }
    }
}

/// Revokes the token of an entry (kick, ban, takeover): the token leaves the table and the entry is marked.
pub(super) fn revoke_entry(
    entry: Option<&mut RegistryEntry>,
    tokens: &mut SessionTokens,
    banned: bool,
) {
    if let Some(e) = entry {
        if let Some(hash) = e.token.take() {
            tokens.revoke(&hash);
        }
        e.revoked = true;
        if banned {
            e.banned = true;
        }
    }
}
