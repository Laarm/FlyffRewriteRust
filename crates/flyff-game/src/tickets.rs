//! Outstanding `DbCmd` continuations keyed by ticket (Plan/10 §4.7, WP-OPS-06).
//!
//! The game thread allocates [`Ticket`]s monotonically; a command that wants an answer registers a
//! [`Pending`] continuation, and phase 1.3 turns every `DbReply` into a `take` + continuation. Tickets are part
//! of the replay stream (13 §16.2). One [`Pending`] variant exists per subsystem continuation; the owning WPs
//! add theirs, or use [`Pending::Service`] with a closure.

use crate::ctx::{GameCtx, Services};
use crate::input::{AdminOutput, CharOpError};
use flyff_common::CharacterId;
use flyff_common::rt::Reply;
use flyff_db::{AtomicOutcome, DbError, DbErrorKind, GameReadResult, Ticket};
use std::collections::BTreeMap;

/// What a ticket's `DbReply` carried.
#[derive(Debug)]
pub enum TicketResult {
    /// An atomic batch or flush committed.
    Committed(AtomicOutcome),
    /// It failed.
    Failed(DbError),
    /// A serial block was reserved.
    SerialBlock(std::ops::Range<u64>),
    /// A game read finished.
    ReadDone(Result<GameReadResult, DbError>),
}

type Continuation = Box<dyn FnOnce(&mut Services, &mut GameCtx<'_>, TicketResult) + Send + 'static>;

/// A continuation owned by a service (party, guild, mail, ...; docs 19–22). Runs unscoped on the game thread.
pub struct ServiceTicket(Continuation);

impl ServiceTicket {
    /// Wraps a continuation.
    pub fn new(
        f: impl FnOnce(&mut Services, &mut GameCtx<'_>, TicketResult) + Send + 'static,
    ) -> Self {
        Self(Box::new(f))
    }

    /// Runs the continuation.
    pub fn run(self, svc: &mut Services, ctx: &mut GameCtx<'_>, result: TicketResult) {
        (self.0)(svc, ctx, result);
    }
}

impl std::fmt::Debug for ServiceTicket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ServiceTicket(..)")
    }
}

/// Why a flush was requested.
#[derive(Debug)]
pub enum FlushPurpose {
    /// The shutdown flush (10 §15.2): the game exits when it commits.
    Shutdown,
    /// `AdminCmd::SaveAll`: answer when everything queued before it committed.
    SaveAll {
        /// The admin reply.
        reply: Reply<AdminOutput>,
        /// Characters whose snapshot was queued.
        affected: u32,
    },
}

/// One outstanding continuation (10 §4.7).
#[derive(Debug)]
pub enum Pending {
    /// `CreateCharacter`: answer with the created id.
    CharacterCreate {
        /// The lobby's reply.
        reply: Reply<Result<CharacterId, CharOpError>>,
    },
    /// `DeleteCharacter`.
    CharacterDelete {
        /// The lobby's reply.
        reply: Reply<Result<(), CharOpError>>,
    },
    /// `SerialAllocator` refill (13 §15.1).
    SerialBlock,
    /// A flush with a purpose.
    Flush(FlushPurpose),
    /// A service continuation.
    Service(ServiceTicket),
}

/// Maps a persistence failure to what the lobby tells the player (12 §9.5, §10.2).
pub fn char_op_error(e: &DbError) -> CharOpError {
    match e.kind {
        DbErrorKind::NameTaken => CharOpError::NameTaken,
        DbErrorKind::SlotTaken => CharOpError::SlotTaken,
        DbErrorKind::NotFound => CharOpError::NotFound,
        DbErrorKind::ShuttingDown => CharOpError::ShuttingDown,
        _ => CharOpError::Internal,
    }
}

/// The ticket table.
#[derive(Debug)]
pub struct TicketRegistry {
    next: u64,
    pending: BTreeMap<u64, Pending>,
}

impl Default for TicketRegistry {
    fn default() -> Self {
        Self {
            next: 1,
            pending: BTreeMap::new(),
        }
    }
}

impl TicketRegistry {
    /// Allocates the next ticket (monotonic) and registers `p` under it.
    pub fn issue(&mut self, p: Pending) -> Ticket {
        let t = Ticket(self.next);
        self.next += 1;
        self.pending.insert(t.0, p);
        t
    }

    /// Registers the continuation of a ticket that [`TicketRegistry::issue_untracked`] allocated earlier: for
    /// handlers that need their ticket before the continuation exists (a name reservation is made under it).
    pub fn register(&mut self, ticket: Ticket, p: Pending) {
        self.pending.insert(ticket.0, p);
    }

    /// Allocates a ticket without a continuation (fire-and-forget commands that still want ordering).
    pub fn issue_untracked(&mut self) -> Ticket {
        let t = Ticket(self.next);
        self.next += 1;
        t
    }

    /// Removes and returns the continuation of `t`.
    pub fn take(&mut self, t: Ticket) -> Option<Pending> {
        self.pending.remove(&t.0)
    }

    /// Number of outstanding continuations.
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    /// `true` when nothing is outstanding.
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}
