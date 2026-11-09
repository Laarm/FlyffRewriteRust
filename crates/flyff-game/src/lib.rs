//! `flyff-game` — tier G (Plan/10, 15–23): everything that runs on the **game thread**. No tokio, no
//! axum (rule X1, ADR-008): the crate talks to the outside world through flume channels
//! ([`input::GameInput`] in, [`input::NetEvent`] out) and per-session [`SessionSink`]s.
//!
//! * [`tick`]: `Game`, `GameInit`, the 15 Hz loop with its phase hooks and telemetry,
//! * [`input`] + [`dispatch`] + [`handlers`]: the cross-thread contract (C1/C3) and the dispatch of every
//!   `GameInput` variant behind plug-in traits (WP-OPS-06),
//! * [`session`]: the one game-side session table (`SessionTable`, `SessionEntry`) and the session rules traits,
//! * [`outbox`]: the fan-out service (slots, audiences, staging, urgent and per-tick flush),
//! * [`tickets`]: outstanding `DbCmd` continuations,
//! * [`world`]: the shared object header types (WP-WORLD-23); the rest of doc 15 arrives with its WPs,
//! * [`movement`] and [`ai`]: the server movement pipeline of doc 18 (sim per mover, `PlayerMoveState`, NPC intents,
//!   keyframes) and the AI plug point,
//! * [`panic`](mod@panic) (handler scopes), [`config`], [`sched`], [`rng`], [`live`] and, behind feature `testkit`,
//!   `testkit::TestWorld` with `testkit::OutboxCapture`.
//!
//! [`SessionSink`]: flyff_common::rt::SessionSink

#![deny(clippy::iter_over_hash_type)]
#![warn(missing_docs)]

pub mod admin;
pub mod admin_ops;
pub mod ai;
pub mod audit;
pub mod buff;
pub mod chat;
pub mod combat;
pub mod config;
pub mod ctx;
pub mod dispatch;
pub mod econ;
pub mod event;
pub mod follow;
pub mod gm;
pub mod handlers;
pub mod input;
pub mod item;
pub mod live;
pub mod movement;
pub mod mover;
pub mod npc;
pub mod outbox;
pub mod overrides;
pub mod panic;
pub mod persist;
pub mod player;
pub mod presence;
pub mod quest;
pub mod rng;
pub mod sched;
pub mod session;
pub mod shutdown;
pub mod skill;
pub mod social;
pub mod stats;
pub mod stats_pub;
pub mod taskbar;
pub mod thread_guard;
pub mod tick;
pub mod tickets;
pub mod world;

pub use config::GameConfig;
pub use ctx::{GameCtx, PlayerRef, Services, Worlds};
pub use dispatch::DispatchStats;
pub use handlers::{AdminHandler, ClientCall, ClientRouter, DefaultRouter, Handlers, NoAdmin};
pub use input::{GameInput, NetEvent};
pub use live::{LiveConfig, LiveConsumer, LiveOrigin};
pub use mover::Mover;
pub use outbox::{
    Audience, AudienceResolver, FlushReport, GameView, Outbox, OutboxStats, SessionSlot, SlotMode,
};
pub use rng::RngSet;
pub use sched::wall::{
    CatchUp, MemWallStore, MinuteCursor, WallFn, WallJob, WallSpec, WallStore, WallZone, ZoneError,
};
pub use sched::{
    DocId, DueJob, DueTimer, Gate, JobFn, JobGroup, PeriodicJob, Scheduler, SlotItem, TimerEvent,
    TimerId, TimerScope,
};
pub use session::{
    GameSessionState, JoinDone, JoinInfo, LeaveMode, LeaveTrigger, SessionEntry, SessionRules,
    SessionTable, StubRules,
};
pub use tick::{
    DeadlineGrid, Game, GameExit, GameInit, Hook, LocalStamp, TickClock, TickDriver,
    TickDriverKind, Wait, WallClockCell, WallClockSource, spawn_game_thread,
};
pub use tickets::{FlushPurpose, Pending, ServiceTicket, TicketRegistry, TicketResult};
