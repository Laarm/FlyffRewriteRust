//! The `Outbox`: the fan-out service of the game thread (Plan/10 §6.2–§6.4, 01 §6.4, WP-OPS-06).
//!
//! One [`SessionSlot`] per session holds the sink and the per-session [`SessionOutbox`] of encoded messages
//! (11 §3.4) — the ONLY place a sink lives on the game thread. A broadcast resolves its [`Audience`] through
//! an [`AudienceResolver`] (doc 15), encodes the `ServerMsg` **once** and appends the same bytes to every
//! recipient's slot. Every S→C byte of a session passes through its single `SessionOutbox` and its single sink
//! queue, which gives the ordering guarantees G1–G8 of 11 §3.6.
//!
//! * **Staging** (`stage_begin` / `stage_commit` / `stage_abort`): inside a player-scoped handler every push and
//!   urgent mark is recorded in call order; a commit replays them into the slots, an abort drops them, so a
//!   state change that is discarded after a panic never leaks a message. Audiences are resolved at call time,
//!   so a committed staging produces exactly the pushes an unstaged run would have produced.
//! * **Flush** (`flush_tick`, `flush_urgent`, `flush_now`): `take_frames` splits at message boundaries
//!   (`net.max_batch_bytes`) and every frame goes to the slot's [`SessionSink`] without ever blocking. The
//!   reports (`full`: the sink has already closed itself as a slow consumer, `closed`: the socket is gone) are
//!   turned into deferred `SessionLost` inputs by the runtime (10 §6.4).
//! * **Lingering**: a slot without a sink drops every push (the original suppresses `Notify` while
//!   `m_dwDestroyTime != 0`, ref/08 §6.4).

pub mod audience;

use crate::thread_guard::GameThreadGuard;
use flyff_common::rt::{CloseReason, SessionSink, SinkError};
use flyff_common::{ObjId, SessionId, SimFrame, Tick};
use flyff_proto::{EncodedMsg, SessionOutbox, pb};
use smallvec::SmallVec;
use std::collections::{BTreeMap, BTreeSet};

pub use audience::{Audience, AudienceResolver, GameView};

/// Whether a slot still has a socket behind it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotMode {
    /// A sink is bound; pushes are queued.
    Live,
    /// No sink (the socket is gone, the avatar lingers); pushes are dropped.
    Lingering,
}

/// One session of the game thread: its sink and its queue of encoded messages.
pub struct SessionSlot {
    /// The avatar bound to this slot (`None` until the join commit, and for sessions without an avatar).
    pub player: Option<ObjId>,
    out: SessionOutbox,
    sink: Option<Box<dyn SessionSink>>,
    /// Live or lingering.
    pub mode: SlotMode,
}

impl SessionSlot {
    /// Number of messages waiting for the next flush.
    pub fn pending(&self) -> usize {
        self.out.len()
    }

    /// Bytes waiting for the next flush.
    pub fn pending_bytes(&self) -> usize {
        self.out.queued_bytes()
    }
}

/// What a flush did (10 §6.4).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FlushReport {
    /// Sessions whose sink queue was full (the sink has already closed itself as `SlowConsumer`), ascending.
    pub full: Vec<SessionId>,
    /// Sessions whose socket was already closed, ascending.
    pub closed: Vec<SessionId>,
    /// Frames handed to sinks.
    pub frames: u64,
    /// Bytes handed to sinks.
    pub bytes: u64,
}

impl FlushReport {
    /// `true` when nothing was sent and nothing failed.
    pub fn is_empty(&self) -> bool {
        self.full.is_empty() && self.closed.is_empty() && self.frames == 0
    }

    /// Adds `other` to this report.
    pub fn merge(&mut self, other: FlushReport) {
        self.full.extend(other.full);
        self.closed.extend(other.closed);
        self.frames += other.frames;
        self.bytes += other.bytes;
    }
}

/// Counters of the outbox (`/metrics` and tests).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OutboxStats {
    /// `EncodedMsg` encodes performed (one per `to_session`, one per non-empty broadcast).
    pub encodes: u64,
    /// Encoded bytes.
    pub encoded_bytes: u64,
    /// Messages appended to slot queues.
    pub pushes: u64,
    /// Pushes dropped because the session has no slot.
    pub dropped_no_slot: u64,
    /// Pushes dropped because the slot lingers (no sink).
    pub dropped_lingering: u64,
    /// Frames sent to sinks.
    pub frames: u64,
    /// Bytes sent to sinks.
    pub bytes: u64,
    /// Sinks that answered `Full`.
    pub full: u64,
    /// Sinks that answered `Closed`.
    pub closed: u64,
    /// Staged scopes committed.
    pub stage_commits: u64,
    /// Staged scopes aborted.
    pub stage_aborts: u64,
}

enum Staged {
    Push(SessionId, EncodedMsg),
    Urgent(SessionId),
}

/// The fan-out service (10 §6.2).
pub struct Outbox {
    slots: BTreeMap<SessionId, SessionSlot>,
    by_player: BTreeMap<ObjId, SessionId>,
    urgent: BTreeSet<SessionId>,
    staging: Option<Vec<Staged>>,
    flush_every: u64,
    /// Counters.
    pub stats: OutboxStats,
    guard: GameThreadGuard,
}

impl Default for Outbox {
    fn default() -> Self {
        Self::new(1)
    }
}

impl std::fmt::Debug for Outbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Outbox")
            .field("slots", &self.slots.len())
            .field("urgent", &self.urgent.len())
            .field("staging", &self.staging.is_some())
            .field("flush_every", &self.flush_every)
            .finish_non_exhaustive()
    }
}

impl Outbox {
    /// An outbox that flushes every `flush_every` ticks (`GameConfig.flush_every_ticks`, 10 §6.4).
    pub fn new(flush_every: u64) -> Self {
        Self {
            slots: BTreeMap::new(),
            by_player: BTreeMap::new(),
            urgent: BTreeSet::new(),
            staging: None,
            flush_every: flush_every.max(1),
            stats: OutboxStats::default(),
            guard: GameThreadGuard::new(),
        }
    }

    /// Sets the flush cadence (ticks between periodic flushes).
    pub fn set_flush_every(&mut self, flush_every: u64) {
        self.flush_every = flush_every.max(1);
    }

    /// The flush cadence in ticks.
    pub fn flush_every(&self) -> u64 {
        self.flush_every
    }

    // ---- slot lifecycle (runtime and doc 12 rule functions) --------------------------------------------

    /// `Join` commit: registers a session with its sink. A second attach of the same session is a
    /// programming error (debug assertion); the old slot is replaced.
    pub fn attach(&mut self, sid: SessionId, sink: Box<dyn SessionSink>, player: Option<ObjId>) {
        self.guard.check();
        debug_assert!(
            !self.slots.contains_key(&sid),
            "session {sid:?} attached twice"
        );
        if let Some(old) = self.slots.remove(&sid)
            && let Some(p) = old.player
        {
            self.by_player.remove(&p);
        }
        if let Some(p) = player {
            self.by_player.insert(p, sid);
        }
        self.slots.insert(
            sid,
            SessionSlot {
                player,
                out: SessionOutbox::new(),
                sink: Some(sink),
                mode: SlotMode::Live,
            },
        );
    }

    /// Binds the avatar of a session after the join commit.
    pub fn set_player(&mut self, sid: SessionId, player: ObjId) {
        self.guard.check();
        if let Some(slot) = self.slots.get_mut(&sid) {
            if let Some(old) = slot.player.take() {
                self.by_player.remove(&old);
            }
            slot.player = Some(player);
            self.by_player.insert(player, sid);
        }
    }

    /// Re-attach (12 RA2): the slot of `old` moves to `new` with the new (gated) sink; the pending entries of
    /// `old` are discarded and the mode is `Live`. A still-open old sink is closed as `Replaced`. Without a slot
    /// for `old` a fresh slot for `new` is created. Returns `false` when `new` is already in use.
    pub fn rebind(&mut self, old: SessionId, new: SessionId, sink: Box<dyn SessionSink>) -> bool {
        self.guard.check();
        if old != new && self.slots.contains_key(&new) {
            return false;
        }
        let player = match self.slots.remove(&old) {
            Some(slot) => {
                if let Some(old_sink) = slot.sink {
                    old_sink.close(CloseReason::Replaced);
                }
                slot.player
            }
            None => None,
        };
        self.urgent.remove(&old);
        if let Some(p) = player {
            self.by_player.insert(p, new);
        }
        self.slots.insert(
            new,
            SessionSlot {
                player,
                out: SessionOutbox::new(),
                sink: Some(sink),
                mode: SlotMode::Live,
            },
        );
        true
    }

    /// The socket is gone but the avatar stays: drops the sink and the pending entries; later pushes are
    /// dropped (10 §6.2).
    pub fn set_lingering(&mut self, sid: SessionId) {
        self.guard.check();
        if let Some(slot) = self.slots.get_mut(&sid) {
            slot.sink = None;
            slot.out = SessionOutbox::new();
            slot.mode = SlotMode::Lingering;
        }
        self.urgent.remove(&sid);
    }

    /// Leave completion, kick, panic, shutdown: removes the slot and hands back its sink (to close it or to
    /// drop it).
    pub fn remove(&mut self, sid: SessionId) -> Option<Box<dyn SessionSink>> {
        self.guard.check();
        self.urgent.remove(&sid);
        let slot = self.slots.remove(&sid)?;
        if let Some(p) = slot.player
            && self.by_player.get(&p) == Some(&sid)
        {
            self.by_player.remove(&p);
        }
        slot.sink
    }

    /// `true` if the session has a slot.
    pub fn contains(&self, sid: SessionId) -> bool {
        self.slots.contains_key(&sid)
    }

    /// The slot of a session.
    pub fn slot(&self, sid: SessionId) -> Option<&SessionSlot> {
        self.slots.get(&sid)
    }

    /// The session whose slot belongs to an avatar (also after the session was unbound from the table).
    pub fn slot_of(&self, player: ObjId) -> Option<SessionId> {
        self.by_player.get(&player).copied()
    }

    /// Number of slots.
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// `true` when no slot exists.
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Number of slots with a sink.
    pub fn live_len(&self) -> usize {
        self.slots
            .values()
            .filter(|s| s.mode == SlotMode::Live)
            .count()
    }

    /// Session ids in ascending order.
    pub fn sessions(&self) -> impl Iterator<Item = SessionId> + '_ {
        self.slots.keys().copied()
    }

    // ---- pushes (feature code) ---------------------------------------------------------------------------

    /// Encodes `body` as a `ServerMsg{obj_id}` and queues it for one session.
    pub fn to_session(
        &mut self,
        sid: SessionId,
        obj_id: u32,
        body: impl Into<pb::server_msg::Msg>,
    ) {
        self.guard.check();
        if !self.live(sid) {
            return;
        }
        let m = self.encode(obj_id, body.into());
        self.push_one(sid, &m);
    }

    /// Queues an already encoded message for one session.
    pub fn push_encoded(&mut self, sid: SessionId, m: &EncodedMsg) {
        self.guard.check();
        if !self.live(sid) {
            return;
        }
        self.push_one(sid, m);
    }

    /// Queues an already encoded message for several sessions, in order.
    pub fn push_to(&mut self, sids: &[SessionId], m: &EncodedMsg) {
        self.guard.check();
        for &sid in sids {
            if self.live(sid) {
                self.push_one(sid, m);
            }
        }
    }

    /// Resolves `aud` now (15 §8.2), encodes the message **once** and queues the same bytes for every
    /// recipient (D3). `subject` is the wire `ServerMsg.obj_id` and the object whose viewer set `Near*`
    /// resolves; a `Near*` broadcast with subject 0 is a programming error (debug assertion, nothing sent).
    pub fn broadcast<R: AudienceResolver + ?Sized>(
        &mut self,
        r: &R,
        subject: ObjId,
        aud: Audience,
        body: impl Into<pb::server_msg::Msg>,
    ) {
        self.guard.check();
        let near = matches!(aud, Audience::NearInclSelf | Audience::NearExclSelf);
        debug_assert!(
            !(near && subject == ObjId::NULL),
            "Near* broadcast without a subject"
        );
        if near && subject == ObjId::NULL {
            return;
        }
        let mut recipients: SmallVec<[(SessionId, ObjId); 64]> = SmallVec::new();
        r.resolve(
            Some(subject).filter(|s| *s != ObjId::NULL),
            &aud,
            &mut recipients,
        );
        recipients.retain(|(sid, _)| self.live(*sid));
        if recipients.is_empty() {
            return;
        }
        let m = self.encode(subject.0, body.into());
        for (sid, _) in recipients {
            self.push_one(sid, &m);
        }
    }

    /// Requests a flush of the session right after the current handler (`Replace`, `Kick`, ...; 11 §3.5).
    pub fn mark_urgent(&mut self, sid: SessionId) {
        self.guard.check();
        if let Some(staged) = self.staging.as_mut() {
            staged.push(Staged::Urgent(sid));
        } else {
            self.apply_urgent(sid);
        }
    }

    fn live(&mut self, sid: SessionId) -> bool {
        match self.slots.get(&sid).map(|s| s.mode) {
            Some(SlotMode::Live) => true,
            Some(SlotMode::Lingering) => {
                self.stats.dropped_lingering += 1;
                false
            }
            None => {
                self.stats.dropped_no_slot += 1;
                false
            }
        }
    }

    fn encode(&mut self, obj_id: u32, body: pb::server_msg::Msg) -> EncodedMsg {
        let m = EncodedMsg::new(obj_id, body);
        self.stats.encodes += 1;
        self.stats.encoded_bytes += m.len() as u64;
        m
    }

    fn push_one(&mut self, sid: SessionId, m: &EncodedMsg) {
        if let Some(staged) = self.staging.as_mut() {
            staged.push(Staged::Push(sid, m.clone()));
        } else {
            self.apply_push(sid, m);
        }
    }

    fn apply_push(&mut self, sid: SessionId, m: &EncodedMsg) {
        match self.slots.get_mut(&sid) {
            Some(slot) if slot.mode == SlotMode::Live => {
                slot.out.push(m);
                self.stats.pushes += 1;
            }
            Some(_) => self.stats.dropped_lingering += 1,
            None => self.stats.dropped_no_slot += 1,
        }
    }

    fn apply_urgent(&mut self, sid: SessionId) {
        if let Some(slot) = self.slots.get_mut(&sid)
            && slot.mode == SlotMode::Live
        {
            slot.out.mark_urgent();
            self.urgent.insert(sid);
        }
    }

    // ---- staging (handler scopes, 10 §14.2) ----------------------------------------------------------------

    /// Opens a staging scope. A nested scope is a programming error (debug assertion).
    pub fn stage_begin(&mut self) {
        self.guard.check();
        debug_assert!(self.staging.is_none(), "nested outbox staging scope");
        self.staging = Some(Vec::new());
    }

    /// Replays the staged pushes and urgent marks into the slots, in call order.
    pub fn stage_commit(&mut self) {
        self.guard.check();
        let Some(staged) = self.staging.take() else {
            debug_assert!(false, "stage_commit without stage_begin");
            return;
        };
        self.stats.stage_commits += 1;
        for s in staged {
            match s {
                Staged::Push(sid, m) => self.apply_push(sid, &m),
                Staged::Urgent(sid) => self.apply_urgent(sid),
            }
        }
    }

    /// Drops everything staged: a discarded state change never leaks a message.
    pub fn stage_abort(&mut self) {
        self.guard.check();
        if self.staging.take().is_some() {
            self.stats.stage_aborts += 1;
        } else {
            debug_assert!(false, "stage_abort without stage_begin");
        }
    }

    /// `true` while a staging scope is open.
    pub fn is_staging(&self) -> bool {
        self.staging.is_some()
    }

    // ---- flush (runtime only) ---------------------------------------------------------------------------

    /// Sends the pending frames of the urgent sessions only (stamp `4n` in phase 1, `4n+4` after `_replace`).
    pub fn flush_urgent(&mut self, frame: SimFrame, max_batch: usize) -> FlushReport {
        self.guard.check();
        debug_assert!(self.staging.is_none(), "flush inside a staging scope");
        let mut report = FlushReport::default();
        let urgent = std::mem::take(&mut self.urgent);
        for sid in urgent {
            if let Some(slot) = self.slots.get_mut(&sid) {
                flush_slot(sid, slot, frame, max_batch, &mut report);
            }
        }
        self.account(&report);
        report
    }

    /// Sends the pending frames of one session (before its sink is closed).
    pub fn flush_now(&mut self, sid: SessionId, frame: SimFrame, max_batch: usize) -> FlushReport {
        self.guard.check();
        debug_assert!(self.staging.is_none(), "flush inside a staging scope");
        let mut report = FlushReport::default();
        self.urgent.remove(&sid);
        if let Some(slot) = self.slots.get_mut(&sid) {
            flush_slot(sid, slot, frame, max_batch, &mut report);
        }
        self.account(&report);
        report
    }

    /// Phase 4: every `flush_every` ticks all slots, otherwise only the urgent ones (10 §6.4).
    pub fn flush_tick(&mut self, tick: Tick, frame: SimFrame, max_batch: usize) -> FlushReport {
        self.guard.check();
        debug_assert!(self.staging.is_none(), "flush inside a staging scope");
        let due = tick.0.is_multiple_of(self.flush_every);
        let mut report = FlushReport::default();
        let urgent = std::mem::take(&mut self.urgent);
        for (sid, slot) in &mut self.slots {
            if due || urgent.contains(sid) {
                flush_slot(*sid, slot, frame, max_batch, &mut report);
            }
        }
        self.account(&report);
        report
    }

    fn account(&mut self, report: &FlushReport) {
        self.stats.frames += report.frames;
        self.stats.bytes += report.bytes;
        self.stats.full += report.full.len() as u64;
        self.stats.closed += report.closed.len() as u64;
    }

    /// Shutdown (10 §15.2): pushes `Kick{SHUTDOWN}` for `ServerShutdown`, flushes everything queued, closes every
    /// sink with `reason` and empties the table.
    pub fn close_all(&mut self, reason: CloseReason, frame: SimFrame, max_batch: usize) {
        self.guard.check();
        self.staging = None;
        if reason == CloseReason::ServerShutdown {
            let sids: Vec<SessionId> = self
                .slots
                .iter()
                .filter(|(_, s)| s.mode == SlotMode::Live)
                .map(|(sid, _)| *sid)
                .collect();
            let kick = EncodedMsg::new(
                0,
                pb::Kick {
                    reason: pb::kick::Reason::Shutdown as i32,
                    tid: 0,
                    detail: String::new(),
                    reconnect_after_ms: 30_000,
                },
            );
            for sid in sids {
                self.apply_push(sid, &kick);
            }
        }
        let mut report = FlushReport::default();
        for (sid, slot) in &mut self.slots {
            flush_slot(*sid, slot, frame, max_batch, &mut report);
        }
        self.account(&report);
        self.urgent.clear();
        self.by_player.clear();
        for slot in std::mem::take(&mut self.slots).into_values() {
            if let Some(sink) = slot.sink {
                sink.close(reason);
            }
        }
    }

    /// Closes every sink with `reason` **without** flushing (after an unscoped panic: the queued frames may
    /// describe inconsistent state and must not be sent).
    pub fn discard_all(&mut self, reason: CloseReason) {
        self.guard.check();
        self.staging = None;
        self.urgent.clear();
        self.by_player.clear();
        for slot in std::mem::take(&mut self.slots).into_values() {
            if let Some(sink) = slot.sink {
                sink.close(reason);
            }
        }
    }
}

/// Hands the pending frames of one slot to its sink (10 §6.4). A full or closed sink is reported once and the
/// slot's queue is cleared (nothing will ever be delivered); a lingering slot receives nothing.
fn flush_slot(
    sid: SessionId,
    slot: &mut SessionSlot,
    frame: SimFrame,
    max_batch: usize,
    report: &mut FlushReport,
) {
    if slot.out.is_empty() {
        return;
    }
    let Some(sink) = slot.sink.as_ref().filter(|_| slot.mode == SlotMode::Live) else {
        slot.out = SessionOutbox::new();
        return;
    };
    for f in slot.out.take_frames(frame.0, max_batch) {
        let len = f.len() as u64;
        match sink.send_batch(f) {
            Ok(()) => {
                report.frames += 1;
                report.bytes += len;
            }
            Err(SinkError::Full) => {
                report.full.push(sid);
                slot.out = SessionOutbox::new();
                return;
            }
            Err(SinkError::Closed) => {
                report.closed.push(sid);
                slot.out = SessionOutbox::new();
                return;
            }
        }
    }
}
