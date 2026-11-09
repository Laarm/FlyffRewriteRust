//! Native-only plumbing of the cross-thread contract (Plan/01 §4.3-4.4): feature `rt`.
//!
//! Nothing in here may be compiled for `wasm32`. All channels are `flume` (ADR-010).

pub mod clock;
pub mod reply;
pub mod shutdown;
pub mod sink;
pub mod telemetry;

pub use clock::MonoClock;
pub use reply::{Reply, ReplyRx, reply_pair};
pub use shutdown::ShutdownHandle;
pub use sink::{CloseReason, KickReason, LostCause, SessionSink, SinkError};
pub use telemetry::{
    Counter2, Health, HealthState, Histogram, InputKind, MovementCounters, PhaseHistogram,
    PhaseSnapshot, Telemetry, TickPhase, WorldPhase,
};
