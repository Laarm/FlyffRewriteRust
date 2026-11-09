//! Panic scopes (Plan/10 §14, D2).
//!
//! * a **player-scoped** handler panic is caught: the session is poisoned (disconnected with
//!   `InternalError`) and **nothing it touched is persisted**; the state is restored from the last
//!   committed save on the next login,
//! * a **multi-party** panic (trade, vendor, mail, guild bank, ...) additionally poisons every
//!   participant (later WPs),
//! * an **unscoped** panic is not caught here: it unwinds to the tick loop, which flushes only the
//!   last committed state and ends with exit code 70.
//!
//! The [`PanicContext`] says WHERE the game thread was when it panicked: the tick, the phase, the session and
//! character of the input being handled, and the name of the unit of work (the proto name of a client message, the
//! kind of an input, the name of a job, a world sub-phase). The process-wide panic hook of `flyff-server` logs it,
//! [`catch`] hands the location to the scope's policy, and `server.panic` carries both (Plan/10 §11, §14.6).

use flyff_common::{CharacterId, SessionId};
use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Which blast radius a handler runs with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandlerScope {
    /// Handlers acting for one session.
    Player(SessionId),
    /// Handlers that mutate several parties atomically (validate-then-commit).
    MultiParty,
    /// No scope: a panic is fatal for the process.
    Unscoped,
}

/// Why a scoped handler failed (the panic payload message, when it was a string).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanicInfo {
    /// Panic message or `"<non-string payload>"`.
    pub message: String,
    /// `file:line:column` of the panic, when the process-wide hook of the server noted it
    /// ([`note_location`]); `None` in unit tests that run under the default hook.
    pub location: Option<String>,
}

/// Where the game thread is right now (Plan/10 §14.2 `PanicContext`). A plain `Copy` value in a thread-local, set
/// by the tick loop and the dispatch, read by the panic hook and by the scope policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PanicContext {
    /// The tick being run.
    pub tick: u64,
    /// The phase of the tick: `input`, `worlds`, `timers`, `flush`, `post`.
    pub phase: &'static str,
    /// The session of the input being handled.
    pub session: Option<SessionId>,
    /// The character of that session.
    pub character: Option<CharacterId>,
    /// The unit of work: the proto name of a client message, the kind of an input, the name of a job, a world
    /// sub-phase (`_process`, ...). Empty between units.
    pub what: &'static str,
}

impl PanicContext {
    /// No context: before the first tick and on every other thread.
    pub const EMPTY: PanicContext = PanicContext {
        tick: 0,
        phase: "",
        session: None,
        character: None,
        what: "",
    };
}

thread_local! {
    static CONTEXT: Cell<PanicContext> = const { Cell::new(PanicContext::EMPTY) };
    static LOCATION: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// The context of the calling thread.
pub fn context() -> PanicContext {
    CONTEXT.with(Cell::get)
}

/// A new phase of tick `tick` begins: the session, the character and the unit are reset.
pub fn set_phase(tick: u64, phase: &'static str) {
    CONTEXT.with(|c| {
        c.set(PanicContext {
            tick,
            phase,
            ..PanicContext::EMPTY
        });
    });
}

/// A unit of work begins: `what` names it, `session` and `character` say for whom.
pub fn set_unit(session: Option<SessionId>, character: Option<CharacterId>, what: &'static str) {
    CONTEXT.with(|c| {
        let mut v = c.get();
        v.session = session;
        v.character = character;
        v.what = what;
        c.set(v);
    });
}

/// The unit of work ended: the tick and the phase stay, the rest is cleared.
pub fn clear_unit() {
    set_unit(None, None, "");
}

/// Remembers the location of the panic in progress on this thread. Only the server's panic hook calls it
/// (`flyff_server::runtime`); the next [`catch`] on the thread takes it.
pub fn note_location(location: &std::panic::Location<'_>) {
    let text = format!(
        "{}:{}:{}",
        location.file(),
        location.line(),
        location.column()
    );
    LOCATION.with(|l| *l.borrow_mut() = Some(text));
}

fn take_location() -> Option<String> {
    LOCATION.with(|l| l.borrow_mut().take())
}

/// Runs `f` and converts a panic into `Err(PanicInfo)`. The caller applies the scope's policy.
pub fn catch<R>(f: impl FnOnce() -> R) -> Result<R, PanicInfo> {
    catch_unwind(AssertUnwindSafe(f)).map_err(|payload| {
        let message = payload
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "<non-string payload>".to_owned());
        PanicInfo {
            message,
            location: take_location(),
        }
    })
}
