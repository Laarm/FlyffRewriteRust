//! The game-side session record (Plan/10 §6.1, WP-OPS-06): the ONE table of avatars and their sessions.
//!
//! [`SessionTable`] is keyed by the avatar's `ObjId` (one entry per avatar in the world: bound, lingering or
//! leaving) with three indexes: bound sessions, accounts and the linger deadlines. The sink and the per-session
//! encoded queue are **not** here: they live in the session's `Outbox` slot (the only place a sink is stored
//! on the game thread). Doc 12's rules (`on_join`, `leave_mode`, `ReqLeave`, `finish_leave`, `on_reattach`,
//! play time) are functions over these types behind the [`rules::SessionRules`] trait; no other table or
//! `PlayerSession` struct exists.
//!
//! All deadlines are `GameMs` (tick-derived, PL-012). Iteration is deterministic: avatars by `ObjId`, bound
//! sessions by `SessionId`, linger deadlines by `(deadline, ObjId)`.

pub mod characters;
pub mod live;
pub mod rules;

use crate::input::{AfterLeave, Authority};
use crate::thread_guard::GameThreadGuard;
use flyff_common::log_budget::LogBudget;
use flyff_common::{AccountId, CharacterId, GameMs, ObjId, SessionId, Tick, UnixMs};
use smallvec::SmallVec;
use std::collections::{BTreeMap, BTreeSet};

pub use characters::{
    CharacterDeleteHooks, CharacterRules, NoDeleteHooks, on_create_character, on_delete_character,
};
pub use live::{WorldRules, leave_mode, on_req_leave, on_return_to_char_select};
pub use rules::{
    JoinDone, JoinInfo, LeaveMode, LeaveTrigger, SessionRules, StubRules, begin_leave,
};

/// One avatar of the world and the session state around it (10 §6.1).
#[derive(Clone, Debug)]
pub struct SessionEntry {
    /// The avatar (a player `ObjId`, stable for the whole stay in the world, kept across re-attach).
    pub player: ObjId,
    /// Owning account.
    pub account: AccountId,
    /// Character.
    pub character: CharacterId,
    /// The bound socket session; `None` after a leave trigger unbound it (lingering / detached).
    pub session: Option<SessionId>,
    /// The session this avatar was last bound to (kept after the unbind; reported in `NetEvent::Left`).
    pub last_session: Option<SessionId>,
    /// Authority of the account at join (12 §1).
    pub authority: Authority,
    /// `Hello.client_build` (diagnostics).
    pub client_build: String,
    /// `ReqLeave` penalty time (legacy `m_dwLeavePenatyTime`, 12 §12.3).
    pub leave_penalty_at: Option<GameMs>,
    /// Linger deadline (legacy `m_dwDestroyTime`, 12 §12.2); kept in sync with the table's index.
    pub linger_until: Option<GameMs>,
    /// What follows the leave: `CharSelect` after `ReturnToCharSelect`, else `Close`.
    pub leave_next: Option<AfterLeave>,
    /// The leave was triggered by the shutdown countdown (the final snapshot's reason is `Shutdown`).
    pub leaving_for_shutdown: bool,
    /// Why the avatar left, for the `session.logout` audit line (`disconnect`, `char_select`, `takeover`, `kick`,
    /// `ban`, `shutdown`, `panic`); set by the first leave trigger, overridden by `shutdown` and `panic`.
    pub leave_reason: Option<&'static str>,
    /// Tick of the `Join` commit (diagnostics, digest).
    pub joined_at: Tick,
    /// Wall-clock sample of that tick (play time, PL-038, 12 §12.6); kept on re-attach.
    pub joined_at_unix: UnixMs,
    /// `characters.play_time_s` at load.
    pub play_time_base_s: i64,
    /// A player-scoped panic happened (14.3): never snapshot this avatar again.
    pub tainted: bool,
    /// The gateway's median client-to-server frame offset (`GameInput::ClockOffset`, 10 §4.6).
    pub clock_offset_frames: i64,
    /// Per-session client-WARN limiter (10 §10.4).
    pub warn_budget: LogBudget,
    /// The text of the next `Kick` frame (`Kick.detail`, at most 255 bytes): set by `/kickall <message>` before
    /// the leave request, taken by `kick_sink`.
    pub kick_detail: Option<String>,
}

/// The state of an entry, derived from its fields and never stored (10 §6.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameSessionState {
    /// A socket is bound.
    Active,
    /// No socket, waiting for the linger deadline (re-attach possible).
    Lingering {
        /// When the avatar is removed.
        until: GameMs,
    },
    /// No socket and no deadline (between the unbind and the leave decision).
    Detached,
}

impl SessionEntry {
    /// A fresh entry for a `Join` commit: bound to `session`, no leave state, not tainted.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        player: ObjId,
        account: AccountId,
        character: CharacterId,
        session: SessionId,
        authority: Authority,
        client_build: String,
        joined_at: Tick,
        joined_at_unix: UnixMs,
        play_time_base_s: i64,
    ) -> Self {
        Self {
            player,
            account,
            character,
            session: Some(session),
            last_session: Some(session),
            authority,
            client_build,
            leave_penalty_at: None,
            linger_until: None,
            leave_next: None,
            leaving_for_shutdown: false,
            leave_reason: None,
            joined_at,
            joined_at_unix,
            play_time_base_s,
            tainted: false,
            clock_offset_frames: 0,
            warn_budget: LogBudget::new(),
            kick_detail: None,
        }
    }

    /// The derived state: a bound session is `Active`, else a deadline makes it `Lingering`, else `Detached`.
    pub fn state(&self) -> GameSessionState {
        match (self.session, self.linger_until) {
            (Some(_), _) => GameSessionState::Active,
            (None, Some(until)) => GameSessionState::Lingering { until },
            (None, None) => GameSessionState::Detached,
        }
    }
}

/// The session table (10 §6.1). Owned by the game thread; mutators assert it in debug builds.
#[derive(Debug, Default)]
pub struct SessionTable {
    by_player: BTreeMap<ObjId, SessionEntry>,
    by_session: BTreeMap<SessionId, ObjId>,
    by_account: BTreeMap<AccountId, ObjId>,
    lingering: BTreeSet<(GameMs, ObjId)>,
    guard: GameThreadGuard,
}

impl SessionTable {
    /// An empty table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of avatars.
    pub fn len(&self) -> usize {
        self.by_player.len()
    }

    /// `true` when no avatar exists.
    pub fn is_empty(&self) -> bool {
        self.by_player.is_empty()
    }

    /// Number of avatars with a bound session.
    pub fn bound_len(&self) -> usize {
        self.by_session.len()
    }

    /// Number of avatars without a bound session (lingering or detached).
    pub fn unbound_len(&self) -> usize {
        self.by_player.len() - self.by_session.len().min(self.by_player.len())
    }

    /// `Join` commit. A duplicate avatar key is a programming error (debug assertion); in release the old
    /// entry is removed first so the indexes stay consistent.
    pub fn insert(&mut self, e: SessionEntry) {
        self.guard.check();
        debug_assert!(
            !self.by_player.contains_key(&e.player),
            "duplicate avatar {:?} in the session table",
            e.player
        );
        debug_assert!(
            !self.by_account.contains_key(&e.account),
            "account {:?} is already online (I3)",
            e.account
        );
        if self.by_player.contains_key(&e.player) {
            let _ = self.remove(e.player);
        }
        if let Some(s) = e.session {
            self.by_session.insert(s, e.player);
        }
        self.by_account.insert(e.account, e.player);
        if let Some(t) = e.linger_until {
            self.lingering.insert((t, e.player));
        }
        self.by_player.insert(e.player, e);
    }

    /// The entry of an avatar.
    pub fn get(&self, p: ObjId) -> Option<&SessionEntry> {
        self.by_player.get(&p)
    }

    /// The entry of an avatar, mutably. Do not change `session`, `account` or `linger_until` through this:
    /// use [`SessionTable::unbind`], [`SessionTable::rebind`] and [`SessionTable::set_linger`], which keep
    /// the indexes in sync.
    pub fn get_mut(&mut self, p: ObjId) -> Option<&mut SessionEntry> {
        self.guard.check();
        self.by_player.get_mut(&p)
    }

    /// The avatar bound to a session.
    pub fn player_of(&self, s: SessionId) -> Option<ObjId> {
        self.by_session.get(&s).copied()
    }

    /// The avatar of an account.
    pub fn player_of_account(&self, a: AccountId) -> Option<ObjId> {
        self.by_account.get(&a).copied()
    }

    /// The entry bound to a session.
    pub fn entry_of_session(&self, s: SessionId) -> Option<&SessionEntry> {
        self.player_of(s).and_then(|p| self.by_player.get(&p))
    }

    /// Every leave trigger starts here (12 §12.2): the entry stops being bound to `s`. A later input for `s`
    /// finds no session and is ignored; an unknown or already unbound session is a no-op (`None`).
    pub fn unbind(&mut self, s: SessionId) -> Option<ObjId> {
        self.guard.check();
        let p = self.by_session.remove(&s)?;
        if let Some(e) = self.by_player.get_mut(&p)
            && e.session == Some(s)
        {
            e.session = None;
        }
        Some(p)
    }

    /// Re-attach (RA2): binds the new session to the same entry and clears `linger_until`,
    /// `leave_penalty_at` and `leave_next`. The `ObjId`, `joined_at_unix` and everything else stay.
    /// `false` when the avatar does not exist.
    pub fn rebind(&mut self, p: ObjId, s: SessionId) -> bool {
        self.guard.check();
        let Some(e) = self.by_player.get_mut(&p) else {
            return false;
        };
        if let Some(old) = e.session.take() {
            self.by_session.remove(&old);
        }
        if let Some(t) = e.linger_until.take() {
            self.lingering.remove(&(t, p));
        }
        e.leave_penalty_at = None;
        e.leave_next = None;
        e.session = Some(s);
        e.last_session = Some(s);
        self.by_session.insert(s, p);
        true
    }

    /// Sets (or clears) the linger deadline and keeps the deadline index in sync.
    pub fn set_linger(&mut self, p: ObjId, until: Option<GameMs>) {
        self.guard.check();
        let Some(e) = self.by_player.get_mut(&p) else {
            return;
        };
        if let Some(old) = e.linger_until {
            self.lingering.remove(&(old, p));
        }
        e.linger_until = until;
        if let Some(t) = until {
            self.lingering.insert((t, p));
        }
    }

    /// P18: the avatars whose deadline is `<= now`, ascending `(deadline, ObjId)`. They leave the deadline
    /// index (each is returned once); the entries stay until the caller finishes the leave (`remove`).
    pub fn pop_due_lingers(&mut self, now: GameMs) -> SmallVec<[ObjId; 4]> {
        self.guard.check();
        let mut due = SmallVec::new();
        while let Some(&(t, p)) = self.lingering.first() {
            if t > now {
                break;
            }
            self.lingering.remove(&(t, p));
            due.push(p);
        }
        due
    }

    /// End of `finish_leave`: removes the avatar and every index entry that points at it.
    pub fn remove(&mut self, p: ObjId) -> Option<SessionEntry> {
        self.guard.check();
        let e = self.by_player.remove(&p)?;
        if let Some(s) = e.session
            && self.by_session.get(&s) == Some(&p)
        {
            self.by_session.remove(&s);
        }
        if self.by_account.get(&e.account) == Some(&p) {
            self.by_account.remove(&e.account);
        }
        if let Some(t) = e.linger_until {
            self.lingering.remove(&(t, p));
        }
        Some(e)
    }

    /// Avatars in ascending `ObjId` order.
    pub fn iter(&self) -> impl Iterator<Item = &SessionEntry> {
        self.by_player.values()
    }

    /// Bound sessions in ascending `SessionId` order, with their avatar.
    pub fn bound(&self) -> impl Iterator<Item = (SessionId, ObjId)> + '_ {
        self.by_session.iter().map(|(s, p)| (*s, *p))
    }

    /// The earliest pending linger deadline, if any.
    pub fn next_linger_deadline(&self) -> Option<GameMs> {
        self.lingering.first().map(|(t, _)| *t)
    }

    /// Checks that the three indexes agree with the entries (tests, debug builds).
    pub fn check_invariants(&self) -> Result<(), String> {
        for (p, e) in &self.by_player {
            if e.player != *p {
                return Err(format!("entry keyed {p:?} claims player {:?}", e.player));
            }
            match e.session {
                Some(s) if self.by_session.get(&s) != Some(p) => {
                    return Err(format!("{p:?} bound to {s:?} but by_session disagrees"));
                }
                _ => {}
            }
            if self.by_account.get(&e.account) != Some(p) {
                return Err(format!("{p:?}: by_account disagrees for {:?}", e.account));
            }
        }
        for (s, p) in &self.by_session {
            match self.by_player.get(p) {
                Some(e) if e.session == Some(*s) => {}
                _ => return Err(format!("by_session {s:?} -> {p:?} has no matching entry")),
            }
        }
        for (a, p) in &self.by_account {
            match self.by_player.get(p) {
                Some(e) if e.account == *a => {}
                _ => return Err(format!("by_account {a:?} -> {p:?} has no matching entry")),
            }
        }
        for &(t, p) in &self.lingering {
            match self.by_player.get(&p) {
                Some(e) if e.linger_until == Some(t) => {}
                _ => return Err(format!("lingering ({t:?}, {p:?}) has no matching entry")),
            }
        }
        Ok(())
    }
}

/// P18 `session_linger_check` (10 §5.3, 12 §12.5): every tick, the avatars whose linger deadline passed are
/// handed to the leave driver (`LeaveTrigger::LingerExpired`, executed right after the job by the runtime, which
/// calls the rules' `finish_leave`), in ascending `(deadline, ObjId)` order.
pub fn linger_check(_svc: &mut crate::ctx::Services, ctx: &mut crate::ctx::GameCtx<'_>) {
    for player in ctx.sessions.pop_due_lingers(ctx.clock.game_ms) {
        ctx.request_leave(player, LeaveTrigger::LingerExpired);
    }
}
