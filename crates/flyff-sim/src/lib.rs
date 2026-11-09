//! `flyff-sim` — tier S (Plan/31, D4): the deterministic CActionMover state machine, kinematics,
//! collision world and chunk codec, motion clock and pose evaluation. Runs natively on the server and
//! in the browser via WASM; bit-exact (no SIMD types, `libm` for transcendentals).
//!
//! The crate is wasm-safe and panic-free: no `unwrap`/`expect`/indexing on untrusted bytes, no
//! `std::fs`/`thread`/`time`, no hash collections in logic, no `mul_add`/`powi`, `f32` wherever the
//! original used `float`. See `NOTES.md` for the module map and the decisions taken where the plan
//! left a choice.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented
)]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]
#![deny(clippy::iter_over_hash_type)]
#![warn(missing_docs)]

pub mod act;
pub mod anim;
pub mod bounds;
pub mod client;
pub mod cmd;
pub mod collision;
pub mod control;
pub mod damage;
pub mod fly;
pub mod kin;
pub mod mover;
pub mod num;
pub mod pose;
pub mod projectile;
pub mod remote;
pub mod steer;
pub mod step;
pub mod sync;
pub mod types;
pub mod wire;

pub use act::ActMsg;
pub use anim::{MotionClock, MotionTable};
pub use collision::{CollisionWorld, LandChunk, decode_chunk, encode_chunk};
pub use flyff_common::config::HitAttrRule;
pub use mover::{ActionMover, SimMover, SimMoverParams};
pub use num::*;
pub use step::{
    EventMode, FrameSnapshot, MoverLookup, MoverSnapshot, SimEnv, SimEvent, SimEvents, SimFlags,
    SimRole, SimRules, step_frame,
};
pub use types::*;

/// `send_act_msg` (Plan/31 §6.1): sends one action message to a mover; returns the result code
/// (`ACT_OK` and friends, or the attack flags of `ATK1..ATK4`).
pub fn send_act_msg(
    m: &mut SimMover,
    env: &SimEnv<'_>,
    rules: &mut dyn SimRules,
    rng: &mut dyn flyff_common::rng::GameRng,
    msg: ActMsg,
    ev: &mut SimEvents,
) -> ActResult {
    let mut cx = step::Cx::new(env, rules, rng, ev);
    act::send_act_msg_cx(m, &mut cx, msg)
}

/// Crate marker used by the workspace smoke test and by `cargo xtask doctor`.
pub const CRATE_NAME: &str = "flyff-sim";
