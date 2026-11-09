//! `AccountEvents` (Plan/25 §1.8; WP-ADMIN-01): what ends admin sessions when an account changes.
//!
//! A ban, an authority change or a password change of an account ends its admin sessions: the WebSocket closes
//! with `SessionEnded` and code 4001, the next REST call answers 401, the `flyff_assets` token is revoked. The
//! events come from three sources:
//!
//! * 12's [`AccountAdmin`](crate::lobby::accounts::AccountAdmin) (the API, the console words, the online CLI):
//!   it publishes after its commit; latency: the session is gone before the HTTP answer;
//! * the game thread's `/ban` and `/setauth` handlers, through the non-blocking `flume` side channel
//!   (`flyff_game::input::AccountEventSink`) that [`spawn_game_forwarder`] drains; latency: a few milliseconds;
//! * a poll of `account_bans`, `accounts.authority` and `accounts.password_hash` every 10 s for the accounts that
//!   hold a staff session ([`poll_once`], [`spawn_poller`]), for changes made behind the server's back (13 §2.3 allows
//!   external writes to them); latency: at most 10 s.
//!
//! [`spawn_session_ender`] subscribes to the broadcast channel and ends the sessions.

use super::session::{AdminSessions, EndReason};
use crate::lobby::online::LobbyClock;
use flyff_common::AccountId;
use flyff_db::{PersistHandle, queries};
use flyff_game::input::{AccountChange, AccountChangeKind};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;

/// How often the account tables are polled for changes the server did not make itself.
pub const POLL_EVERY: Duration = Duration::from_secs(10);

/// What changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AccountEventKind {
    /// An active ban was stored (or the account vanished).
    Banned,
    /// The account-level authority changed.
    Authority,
    /// The password changed.
    Password,
}

impl AccountEventKind {
    /// The end reason of the sessions of the account.
    pub fn end_reason(self) -> EndReason {
        match self {
            AccountEventKind::Banned => EndReason::Banned,
            AccountEventKind::Authority => EndReason::Authority,
            AccountEventKind::Password => EndReason::Password,
        }
    }
}

/// Where an event came from (metrics, tests).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EventSource {
    /// `AccountAdmin`.
    Admin,
    /// A game-thread GM command.
    Game,
    /// The 10 s poll.
    Poll,
}

/// One change of an account.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountEvent {
    /// The account.
    pub account: AccountId,
    /// What changed.
    pub kind: AccountEventKind,
    /// Who noticed.
    pub source: EventSource,
    /// The public handle of a session that stays (the one that changed its own password).
    pub keep_session: Option<String>,
}

/// The broadcast channel of account events.
#[derive(Clone, Debug)]
pub struct AccountEvents {
    tx: broadcast::Sender<AccountEvent>,
}

impl Default for AccountEvents {
    fn default() -> Self {
        Self::new()
    }
}

impl AccountEvents {
    /// A channel with no subscriber yet.
    pub fn new() -> AccountEvents {
        let (tx, _) = broadcast::channel(256);
        AccountEvents { tx }
    }

    /// Announces a change; never blocks, and does nothing when nobody listens.
    pub fn publish(&self, ev: AccountEvent) {
        let _ = self.tx.send(ev);
    }

    /// Announces `kind` for `account`.
    pub fn notify(&self, account: AccountId, kind: AccountEventKind, source: EventSource) {
        self.publish(AccountEvent {
            account,
            kind,
            source,
            keep_session: None,
        });
    }

    /// A receiver of the events published from now on.
    pub fn subscribe(&self) -> broadcast::Receiver<AccountEvent> {
        self.tx.subscribe()
    }
}

/// Ends the sessions of the account of every event it receives. The task ends when the channel closes.
pub fn spawn_session_ender(
    rt: &tokio::runtime::Handle,
    events: &AccountEvents,
    sessions: Arc<AdminSessions>,
) -> tokio::task::JoinHandle<()> {
    let mut rx = events.subscribe();
    rt.spawn(async move {
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    let n = sessions.end_account(
                        ev.account,
                        ev.kind.end_reason(),
                        ev.keep_session.as_deref(),
                    );
                    if n > 0 {
                        tracing::info!(
                            account = ev.account.0,
                            reason = ev.kind.end_reason().name(),
                            source = ?ev.source,
                            sessions = n,
                            "admin sessions ended"
                        );
                    }
                }
                // a burst overran the channel: the poll finds what was lost within 10 s
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(
                        missed = n,
                        "account events were dropped; the poll catches up"
                    );
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    })
}

/// Forwards the changes of the game thread's side channel into `events`.
pub fn spawn_game_forwarder(
    rt: &tokio::runtime::Handle,
    rx: flume::Receiver<AccountChange>,
    events: AccountEvents,
) -> tokio::task::JoinHandle<()> {
    rt.spawn(async move {
        while let Ok(change) = rx.recv_async().await {
            let kind = match change.kind {
                AccountChangeKind::Banned => AccountEventKind::Banned,
                AccountChangeKind::Authority => AccountEventKind::Authority,
            };
            events.notify(change.account, kind, EventSource::Game);
        }
    })
}

/// One pass of the poll: compares every staff session's account with the database and publishes an event for
/// each difference (an active ban or a vanished account, a different stored authority letter, a different password
/// hash). Returns the events it published.
pub async fn poll_once(
    persist: &PersistHandle,
    sessions: &AdminSessions,
    events: &AccountEvents,
    clock: &LobbyClock,
) -> Vec<AccountEvent> {
    let mut published = Vec::new();
    for (account, _name, _authority, snap) in sessions.staff_snapshots() {
        let now = clock.unix();
        let read = persist
            .read(move |c| {
                let rec = queries::account_by_id(c, account)?;
                let ban = match &rec {
                    Some(_) => queries::active_ban(c, account, now)?,
                    None => None,
                };
                Ok((rec, ban))
            })
            .await;
        let kind = match read {
            // an unreadable database is the persistence layer's alarm, not a reason to sign anybody out
            Err(e) => {
                tracing::debug!(account = account.0, "admin session poll: {e}");
                continue;
            }
            Ok((None, _)) => Some(AccountEventKind::Banned),
            Ok((Some(_), Some(_))) => Some(AccountEventKind::Banned),
            Ok((Some(rec), None)) => {
                if flyff_common::gm::Authority::from_db_byte(rec.authority) != snap.authority_db {
                    Some(AccountEventKind::Authority)
                } else if super::session::AccountSnapshot::digest(&rec.password_hash)
                    != snap.password_digest
                {
                    Some(AccountEventKind::Password)
                } else {
                    None
                }
            }
        };
        if let Some(kind) = kind {
            let ev = AccountEvent {
                account,
                kind,
                source: EventSource::Poll,
                keep_session: None,
            };
            events.publish(ev.clone());
            published.push(ev);
        }
    }
    published
}

/// Runs [`poll_once`] every `every`. The task holds the arguments and ends with the runtime.
pub fn spawn_poller(
    rt: &tokio::runtime::Handle,
    persist: PersistHandle,
    sessions: Arc<AdminSessions>,
    events: AccountEvents,
    clock: LobbyClock,
    every: Duration,
) -> tokio::task::JoinHandle<()> {
    rt.spawn(async move {
        let mut tick = tokio::time::interval(every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            if sessions.is_empty() {
                continue;
            }
            poll_once(&persist, &sessions, &events, &clock).await;
        }
    })
}
