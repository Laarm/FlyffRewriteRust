//! A trivial wandering AI for tests and benches (WP-AI-01/04). It is **not** the monster brain of doc 18 §9: no
//! scans, no states, no return-home leash. It exists so that the movement pipeline has something that raises
//! intents: `MoveToRandom` of 18 §8.8 (`x1 = next_u32()`, `x2 = next_u32()`, candidate `spawn + (x1 % 21 - 10, 0,
//! x2 % 21 - 10)`) every `interval_ms` while the mover has no destination.

use super::{AiCx, AiDriver, AiMsg};
use crate::movement::intents;
use crate::mover::Mover;
use flyff_common::ObjId;
use flyff_common::math::Vec3;
use std::collections::BTreeMap;

/// Per-mover state of the wanderer.
#[derive(Clone, Copy, Debug)]
struct WanderState {
    /// The position the wander radius is measured from (recorded at `init`).
    spawn: Vec3,
    /// The game time (`GameMs`) of the next walk.
    next_ms: u64,
}

/// Walks every awake non-player mover to a random point around its spawn position every `interval_ms`.
#[derive(Clone, Debug)]
pub struct WanderAi {
    /// Pause between two walks in `GameMs` (the draw adds up to 2 s on top).
    pub interval_ms: u64,
    /// Half-width of the square the candidates fall in (the original's `±10`).
    pub radius: i32,
    state: BTreeMap<ObjId, WanderState>,
    /// Arrivals delivered to the AI (position and object), for tests.
    pub arrivals: u32,
    /// Collisions delivered to the AI, for tests.
    pub collisions: u32,
}

impl Default for WanderAi {
    fn default() -> Self {
        WanderAi::new(3_000)
    }
}

impl WanderAi {
    /// A wanderer pausing `interval_ms` between walks.
    pub fn new(interval_ms: u64) -> WanderAi {
        WanderAi {
            interval_ms,
            radius: 10,
            state: BTreeMap::new(),
            arrivals: 0,
            collisions: 0,
        }
    }
}

impl AiDriver for WanderAi {
    fn init(&mut self, m: &mut Mover, cx: &mut AiCx<'_>) {
        let jitter = u64::from(cx.rng.next_u32() % 2_000);
        self.state.insert(
            m.id(),
            WanderState {
                spawn: m.sim.pos,
                next_ms: cx.now.0 + self.interval_ms + jitter,
            },
        );
    }

    fn process(&mut self, m: &mut Mover, cx: &mut AiCx<'_>) {
        let Some(st) = self.state.get_mut(&m.id()) else {
            return;
        };
        if cx.now.0 < st.next_ms || !m.sim.is_empty_dest() || m.sim.act.state.is_die() {
            return;
        }
        // `MoveToRandom` (18 §8.8): x first, then z
        let span = u32::try_from(self.radius * 2 + 1).unwrap_or(21).max(1);
        let x1 = cx.rng.next_u32();
        let x2 = cx.rng.next_u32();
        let jitter = u64::from(cx.rng.next_u32() % 2_000);
        let r = self.radius;
        let cand = Vec3::new(
            st.spawn.x + ((x1 % span) as i32 - r) as f32,
            st.spawn.y,
            st.spawn.z + ((x2 % span) as i32 - r) as f32,
        );
        st.next_ms = cx.now.0 + self.interval_ms + jitter;
        intents::move_to_pos(m, cand);
    }

    fn message(&mut self, _m: &mut Mover, msg: AiMsg, _cx: &mut AiCx<'_>) {
        match msg {
            AiMsg::Arrival { .. } => self.arrivals += 1,
            AiMsg::Collision => self.collisions += 1,
            _ => {}
        }
    }
}
