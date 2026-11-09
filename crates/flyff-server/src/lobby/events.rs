//! The `NetEvent` consumer of the registry (Plan/12 §7.5; WP-AUTH-03): the synchronous body behind
//! `LobbyPort::on_net_event`. It updates the registry and returns the [`RegistryAction`]s that the caller
//! executes after the lock is released (spawn the drain task, tell the session task). It never blocks and
//! never awaits.

use super::online::{EntryState, OnlineRegistry, RegistryAction};
use flyff_common::rt::{CloseReason, KickReason};
use flyff_common::{AccountId, CharacterId, SaveGen, SessionId};
use flyff_game::input::{AfterLeave, JoinFailReason, NetEvent};

impl OnlineRegistry {
    /// One event of the game thread (C3).
    ///
    /// * `Left{session, account, character, final_gen, next}`: `last_gen = max(last_gen, final_gen)`,
    ///   `in_world -= 1`; `CharSelect` -> `Claimed` (and the attached session continues at character select);
    ///   `Close` -> `Draining{final_gen}` + a drain task when the entry was revoked (kick, ban, takeover), else
    ///   `Claimed` (the token stays valid for a reload, R4). A pending takeover wakes up.
    /// * `JoinFailed{session, reason}`: `Claimed`, `in_world -= 1`, and the session gets a `LobbyError`.
    pub fn on_net_event(&self, ev: &NetEvent) -> Vec<RegistryAction> {
        match ev {
            NetEvent::Left {
                session,
                account,
                character,
                final_gen,
                next,
            } => self.on_left(*session, *account, *character, *final_gen, *next),
            NetEvent::JoinFailed { session, reason } => self.on_join_failed(*session, *reason),
        }
    }

    fn on_left(
        &self,
        session: SessionId,
        account: AccountId,
        character: CharacterId,
        final_gen: SaveGen,
        next: AfterLeave,
    ) -> Vec<RegistryAction> {
        let mut guard = self.inner.lock();
        let inner = &mut *guard;
        let mut actions = Vec::new();
        let Some(e) = inner.by_account.get_mut(&account) else {
            tracing::error!(
                account = account.0,
                character = character.0,
                "NetEvent::Left for an account without a registry entry"
            );
            return actions;
        };
        let join_session = match &e.state {
            EntryState::Entering { session, .. } | EntryState::InWorld { session, .. } => {
                Some(*session)
            }
            EntryState::Claimed | EntryState::Draining { .. } => None,
        };
        if let Some(js) = join_session {
            inner.in_world = inner.in_world.saturating_sub(1);
            inner.by_join_session.remove(&js);
        }
        e.last_gen = e.last_gen.max(final_gen);
        match &e.state {
            EntryState::Draining { save_gen } => {
                // already draining (a late event): the generation to wait for only grows
                let save_gen = (*save_gen).max(final_gen);
                e.state = EntryState::Draining { save_gen };
            }
            _ => match next {
                AfterLeave::CharSelect => {
                    e.state = EntryState::Claimed;
                    if e.revoked && e.takeover.is_none() {
                        // a revoked entry must not resume at character select
                        e.state = EntryState::Draining {
                            save_gen: e.last_gen,
                        };
                        if let Some(s) = e.socket.take() {
                            inner.by_socket.remove(&s);
                            actions.push(RegistryAction::CloseSocket {
                                session: s,
                                reason: CloseReason::Kicked(KickReason::DuplicateLogin),
                            });
                        }
                        actions.push(RegistryAction::SpawnDrain {
                            account,
                            save_gen: e.last_gen,
                        });
                    } else if !e.revoked && e.socket == Some(session) {
                        actions.push(RegistryAction::EnterCharSelect {
                            session,
                            account,
                            last_gen: e.last_gen,
                        });
                    }
                }
                AfterLeave::Close => {
                    if e.revoked {
                        e.state = EntryState::Draining {
                            save_gen: final_gen,
                        };
                        actions.push(RegistryAction::SpawnDrain {
                            account,
                            save_gen: final_gen,
                        });
                    } else {
                        e.state = EntryState::Claimed;
                    }
                }
            },
        }
        drop(guard);
        self.changed().notify_waiters();
        actions
    }

    fn on_join_failed(&self, session: SessionId, reason: JoinFailReason) -> Vec<RegistryAction> {
        let mut guard = self.inner.lock();
        let inner = &mut *guard;
        let mut actions = Vec::new();
        let account = inner
            .by_join_session
            .get(&session)
            .or_else(|| inner.by_socket.get(&session))
            .copied();
        let Some(account) = account else {
            tracing::warn!(
                session = session.0,
                ?reason,
                "JoinFailed for an unknown session"
            );
            return actions;
        };
        let Some(e) = inner.by_account.get_mut(&account) else {
            return actions;
        };
        let was_world = matches!(
            e.state,
            EntryState::Entering { .. } | EntryState::InWorld { .. }
        );
        if !was_world {
            return actions;
        }
        inner.in_world = inner.in_world.saturating_sub(1);
        inner.by_join_session.remove(&session);
        e.state = EntryState::Claimed;
        if e.revoked && e.takeover.is_none() {
            e.state = EntryState::Draining {
                save_gen: e.last_gen,
            };
            actions.push(RegistryAction::SpawnDrain {
                account,
                save_gen: e.last_gen,
            });
        } else if !e.revoked {
            actions.push(RegistryAction::JoinFailed {
                session,
                account,
                reason,
            });
        }
        drop(guard);
        self.changed().notify_waiters();
        actions
    }
}
