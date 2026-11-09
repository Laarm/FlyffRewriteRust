//! The server movement pipeline (Plan/18 §2-§5; WP-AI-01 .. WP-AI-04): the native sim per mover, the 60 Hz
//! sub-frame pipeline inside the world frame, `PlayerMoveState` and the click-to-move requests, NPC intents,
//! keyframes and the spawn replay.
//!
//! | module | content |
//! |---|---|
//! | [`pipeline`] | [`MovementHooks`]: the [`WorldHooks`](crate::world::WorldHooks) of phase 2 (AI pass, frame snapshot, `step_frame`, barrier) |
//! | [`events`] | the `SimEvent` dispatch table of 18 §3.4 |
//! | [`intents`] | the pending queue, stamp capture (18 §5.2), the intent emitters, the spawn replay (§5.4), `set_player_dest_obj` (§5.9) |
//! | [`keyframe`] | `MoverKeyframe`: periodic, settle and dev ghost keyframes (§5.3) |
//! | [`corrections`] | NPC collision / drop corrections, `SetPos` warps, push (§5.5-§5.7) |
//! | [`player`] | `PlayerMoveState`, `DestPosReq`, `SetDestObjReq`, `MotionReq` (§4) |
//! | [`flying`] | take-off and landing gates, the ride sync, the no-fly dismount and the turbo-off broadcast (§6) |
//!
//! **Stamps.** Every message of this module carries the frame at which the state it describes was valid
//! (`server_frame`, 31 §2.4: "the start of frame F, before `step_frame(F)`"). Intents are queued on the mover
//! ([`MoveSync::pending`]) and captured right before the mover's next `step_frame(F)`: an intent raised in the
//! AI pass or in phase 1 is stamped `4n`, one raised at the barrier of frame `F` is stamped `F + 1` (for
//! `F = 4n + 3` that is the first frame of the next tick). [`effect_frame`] tells other modules which frame
//! an effect applied to a mover right now takes.
//!
//! **Frame-major order.** The world driver steps frame-major (31 §3.4: all movers frame `F`, then the barrier,
//! then `F + 1`), which is required for server/client identity. Doc 18 §3.3 sketches a mover-major order in
//! which an effect on a later slot lands at `4n` and on an earlier slot at `4n + 4`; with the barrier that
//! distinction disappears: an effect raised in the AI pass or in phase 1 is `4n` for every mover, an effect
//! raised at the barrier of frame `F` is `F + 1` for every mover (NOTES "Movement pipeline").

pub mod corrections;
pub mod events;
pub mod flying;
pub mod intents;
pub mod keyframe;
pub mod pipeline;
pub mod player;

pub use pipeline::{MoveConsumers, MovementHooks};

use crate::world::object::{MoverKey, ObjectStore};
use flyff_common::config::SimFlags;
use flyff_common::math::Vec3;
use flyff_common::{ObjId, SimFrame, Tick};
use flyff_data::GameData;
use flyff_sim::MotionTable;
use flyff_sim::bounds::ModelBounds;
use flyff_sim::collision::geom::Aabb;
use flyff_sim::step::{FrameSnapshot, MoverLookup, MoverSnapshot, SimEvents, snapshot_of};
use std::sync::Arc;

/// `PLAYER_STAMP_MAX_AGE`: a client stamp older than this many frames is clamped to `now - 37`
/// (`MAX_CORR_SIZE_150`, KQ-28).
pub const PLAYER_STAMP_MAX_AGE: u64 = 37;

// ---- configuration and shared data ---------------------------------------------------------------------------

/// What the server hands to the game thread for the movement pipeline (`Handlers.movement`): the derived
/// tables the sim reads and the two settings that live outside `GameConfig` (`net.npc_keyframe_hz`,
/// `debug.dev_surfaces`). `Default` = no motion table (movers keep their clips unresolved), 1 Hz keyframes,
/// no dev surfaces.
#[derive(Clone, Debug)]
pub struct MoveInit {
    /// `ServerDerived.motions` (`None` = an empty table).
    pub motions: Option<Arc<MotionTable>>,
    /// `ServerDerived.bounds` (`None` = the default model box of the sim).
    pub bounds: Option<Arc<ModelBounds>>,
    /// `net.npc_keyframe_hz`: 0 = off, else every `max(1, round(15 / hz))` ticks (18 §5.3).
    pub npc_keyframe_hz: u32,
    /// `--dev` / `debug.dev_surfaces`: one ghost keyframe per tick for moving NPCs, in debug builds only.
    pub dev_surfaces: bool,
}

impl Default for MoveInit {
    fn default() -> Self {
        MoveInit {
            motions: None,
            bounds: None,
            npc_keyframe_hz: 1,
            dev_surfaces: false,
        }
    }
}

/// The immutable data of the pipeline, shared by the phase hooks and the client-message handlers
/// (`Worlds.movement`).
pub struct MoveShared {
    /// The loaded game data (model file names for the bounds lookup).
    pub data: Arc<GameData>,
    /// The motion table the sim resolves clips with.
    pub motions: Arc<MotionTable>,
    /// The model bounds table.
    pub bounds: Option<Arc<ModelBounds>>,
    /// The sim feature switches (identical to the browser's).
    pub flags: SimFlags,
    /// `gameplay.move_guard_dist_sq` (VR-080).
    pub guard_dist_sq: f32,
    /// Keyframe period in ticks, 0 = off.
    pub keyframe_period: u64,
    /// Dev ghost keyframes requested (effective in debug builds only).
    pub dev_keyframes: bool,
    // ---- boss (doc 18 §10.4, task boss-ai-core) ----
    /// The rigs of the bosses (`ServerDerived.rigs`, wrapped by the server): the ClockWorks hit handlers read the
    /// event points and bones of their model through it. `None` = no pose data (the hits use the boss position).
    pub boss_poses: Option<Arc<dyn crate::ai::boss::pose::BossPoseSource>>,
}

impl MoveShared {
    /// The pipeline data of a game: `init` from the server, `cfg` for the guard and the sim flags.
    pub fn new(
        init: &MoveInit,
        cfg: &crate::config::GameConfig,
        data: &Arc<GameData>,
    ) -> MoveShared {
        let hz = u64::from(init.npc_keyframe_hz.min(15));
        MoveShared {
            data: Arc::clone(data),
            motions: init.motions.clone().unwrap_or_default(),
            bounds: init.bounds.clone(),
            flags: SimFlags::from_rules(&cfg.rules),
            guard_dist_sq: cfg.rules.gameplay.move_guard_dist_sq,
            keyframe_period: (15 + hz / 2).checked_div(hz).map_or(0, |k| k.max(1)),
            dev_keyframes: init.dev_surfaces,
            boss_poses: None,
        }
    }
}

impl Default for MoveShared {
    fn default() -> Self {
        MoveShared::new(
            &MoveInit::default(),
            &crate::config::GameConfig::default(),
            &Arc::default(),
        )
    }
}

impl std::fmt::Debug for MoveShared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MoveShared")
            .field("motion_clips", &self.motions.clips.len())
            .field("bounds", &self.bounds.as_ref().map(|b| b.len()))
            .field("guard_dist_sq", &self.guard_dist_sq)
            .field("keyframe_period", &self.keyframe_period)
            .field("dev_keyframes", &self.dev_keyframes)
            .field("boss_poses", &self.boss_poses.is_some())
            .finish_non_exhaustive()
    }
}

/// Why a movement message was ignored (`flyff_move_ignored_total{reason}`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum IgnoreReason {
    /// The player is dead.
    Dead,
    /// A float field is not finite.
    NonFinite,
    /// `seq` is not newer than the last accepted one.
    Stale,
    /// Fly/ground mismatch between the message variant and the server copy.
    FlyMismatch,
    /// `BEHAVIOR` while the server copy is in an attack sub-state.
    AttackBusy,
    /// The 1 km guard (VR-080).
    Guard,
    /// `MOVE_REASON_UNSPECIFIED`.
    Unspecified,
    /// The target object does not exist (`SetDestObjReq`).
    NoTarget,
    /// `__TRAFIC_1222`: the same destination object again.
    Duplicate,
}

impl IgnoreReason {
    /// Every reason in counter order.
    pub const ALL: [IgnoreReason; 9] = [
        IgnoreReason::Dead,
        IgnoreReason::NonFinite,
        IgnoreReason::Stale,
        IgnoreReason::FlyMismatch,
        IgnoreReason::AttackBusy,
        IgnoreReason::Guard,
        IgnoreReason::Unspecified,
        IgnoreReason::NoTarget,
        IgnoreReason::Duplicate,
    ];

    /// The `reason` label of the metric.
    pub const fn label(self) -> &'static str {
        match self {
            IgnoreReason::Dead => "dead",
            IgnoreReason::NonFinite => "nonfinite",
            IgnoreReason::Stale => "stale",
            IgnoreReason::FlyMismatch => "fly_mismatch",
            IgnoreReason::AttackBusy => "attack_busy",
            IgnoreReason::Guard => "guard",
            IgnoreReason::Unspecified => "unspecified",
            IgnoreReason::NoTarget => "no_target",
            IgnoreReason::Duplicate => "duplicate",
        }
    }
}

/// The counters of 18 §17 (`/metrics` exports them; tests read them).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MoveStats {
    /// `flyff_move_states_total{reason}` by `MoveReason` number (0..=5).
    pub states: [u64; 6],
    /// `flyff_move_ignored_total{reason}` by [`IgnoreReason`] order.
    pub ignored: [u64; 9],
    /// `DestPosReq` accepted.
    pub dest_pos_reqs: u64,
    /// `SetDestObjReq` accepted.
    pub dest_obj_reqs: u64,
    /// `MotionReq` accepted / rejected.
    pub motion_reqs: [u64; 2],
    /// `flyff_npc_intents_total{kind}`: intents captured, by [`PendingIntent::kind`].
    pub intents: [u64; 12],
    /// `flyff_keyframes_total{kind}`: periodic, settle, dev.
    pub keyframes: [u64; 3],
    /// `flyff_npc_corrections_total`.
    pub corrections: u64,
    /// `flyff_sim_frames_total`: `step_frame` calls of the frame loop.
    pub sim_frames: u64,
    /// Sim events dispatched at the barriers.
    pub events: u64,
    /// `flyff_movers_awake`: awake movers of the last processed layer pass (gauge).
    pub awake: u64,
    /// Frames after which a non-finite position or velocity (driven there by a hostile state) was reset.
    pub sanitised: u64,
}

impl MoveStats {
    /// How often `reason` was ignored.
    pub fn ignored(&self, reason: IgnoreReason) -> u64 {
        self.ignored
            .iter()
            .zip(IgnoreReason::ALL)
            .find(|(_, r)| *r == reason)
            .map_or(0, |(n, _)| *n)
    }

    pub(crate) fn note_ignored(&mut self, reason: IgnoreReason) {
        if let Some(n) = self
            .ignored
            .iter_mut()
            .zip(IgnoreReason::ALL)
            .find(|(_, r)| *r == reason)
            .map(|(n, _)| n)
        {
            *n += 1;
        }
        tracing::debug!(reason = reason.label(), "movement message ignored");
    }

    /// Accepted `PlayerMoveState`s of one `MoveReason` number.
    pub fn states_of(&self, reason: i32) -> u64 {
        usize::try_from(reason)
            .ok()
            .and_then(|i| self.states.get(i))
            .copied()
            .unwrap_or(0)
    }
}

/// The pipeline state of the worlds: the shared data and the counters (`Worlds.movement`).
#[derive(Debug, Default)]
pub struct MovementState {
    /// The immutable data.
    pub shared: Arc<MoveShared>,
    /// The counters.
    pub stats: MoveStats,
    /// The AI mailbox (18 §8.6, [`crate::ai::mail`]): messages for movers, delivered by the pipeline at the next
    /// AI pass or barrier of the target's layer. Docs 15-22 fill it without holding an AI context.
    pub ai_mail: Vec<(ObjId, crate::ai::AiMsg)>,
    /// The combat runtime (Plan/17; `combat::world`): the damage seams, the death queue, the server
    /// projectiles and the per-tick environment snapshot. It lives here because both the world phases
    /// (`FrameCtx.movement`) and the client handlers (`GameCtx.worlds.movement`) reach this struct.
    pub combat: crate::combat::CombatWorld,
    /// The wall clock of the tick in unix seconds, sampled once before the world phase (the per-player 1 s timer
    /// of the tick tail needs it for the absolute SM timers, 16 §14.2).
    pub unix_now: i64,
    /// The requests of the flying code that need the services context (the dismounts at a no-fly border,
    /// 18 §6; [`flying::finish`]).
    pub fly: flying::FlyQueue,
}

impl MovementState {
    /// The state of a game built with `init`.
    pub fn new(
        init: &MoveInit,
        cfg: &crate::config::GameConfig,
        data: &Arc<GameData>,
    ) -> MovementState {
        MovementState {
            shared: Arc::new(MoveShared::new(init, cfg, data)),
            stats: MoveStats::default(),
            ai_mail: Vec::new(),
            combat: crate::combat::CombatWorld::default(),
            unix_now: 0,
            fly: flying::FlyQueue::default(),
        }
    }

    /// Installs the rigs of the bosses ([`MoveShared::boss_poses`]; `Game::new` passes `Handlers.boss_poses`).
    /// Call it **before the first clone of `shared`** (the pipeline and the phases hold clones of the `Arc` once
    /// the game runs): it edits the fresh `Arc` in place. Returns `false` and changes nothing when `shared` was
    /// already cloned.
    pub fn set_boss_poses(
        &mut self,
        poses: Option<Arc<dyn crate::ai::boss::pose::BossPoseSource>>,
    ) -> bool {
        match Arc::get_mut(&mut self.shared) {
            Some(shared) => {
                shared.boss_poses = poses;
                true
            }
            None => {
                tracing::warn!(
                    "MovementState::set_boss_poses called after `shared` was cloned: ignored"
                );
                false
            }
        }
    }
}

// ---- the per-mover bookkeeping ---------------------------------------------------------------------------------

/// Who receives a queued intent (`IntentAudience`, 18 §5.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntentAudience {
    /// `Audience::NearExclSelf` of the mover (the viewers; for an NPC identical to near).
    Viewers,
    /// `Audience::NearInclSelf` (the viewers and the player itself).
    ViewersInclSelf,
    /// One player (a viewer that just got the spawn).
    Viewer(ObjId),
}

/// A movement message about a mover that is waiting for its stamp (18 §5.2). The payload is what cannot be
/// read back from the sim at capture time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PendingIntent {
    /// `DestPosNtf`.
    DestPos {
        /// The destination.
        pos: Vec3,
        /// Walk forward to it.
        forward: bool,
    },
    /// `SetDestObjNtf`.
    DestObj {
        /// The destination object.
        target: ObjId,
        /// The stop distance.
        range: f32,
    },
    /// `DestAngle`.
    DestAngle {
        /// The wanted yaw.
        angle: f32,
        /// Turn left.
        left: bool,
    },
    /// `SetMovePattern` (position and angles come from the capture snapshot).
    MovePattern {
        /// The pattern id.
        pattern: i32,
    },
    /// `SetSpeedFactor`.
    SpeedFactor(f32),
    /// `SetPos` (the position comes from the capture snapshot).
    SetPos,
    /// `MoverStateNtf{CORR}` of an NPC after a collision or drop.
    Corr,
    /// `PushPower`.
    Push {
        /// The push direction.
        push_angle: f32,
        /// The push power.
        power: f32,
    },
    /// `SendActMsg`.
    ActMsg {
        /// `OBJMSG_*`.
        obj_msg: i32,
        /// The mover flies.
        is_fly: bool,
        /// `p1..p3`.
        p: [i32; 3],
    },
    /// `MotionNtf` (sit / stand, pickup).
    Motion(u32),
    /// `MoverStateNtf{BEHAVIOR}`: a server-forced pose.
    ForcePose,
    /// `MotionArrive` (patrol only, never in v1).
    MotionArrive(i32),
}

impl PendingIntent {
    /// The index of the intent kind in [`MoveStats::intents`].
    pub const fn kind(&self) -> usize {
        match self {
            PendingIntent::DestPos { .. } => 0,
            PendingIntent::DestObj { .. } => 1,
            PendingIntent::DestAngle { .. } => 2,
            PendingIntent::MovePattern { .. } => 3,
            PendingIntent::SpeedFactor(_) => 4,
            PendingIntent::SetPos => 5,
            PendingIntent::Corr => 6,
            PendingIntent::Push { .. } => 7,
            PendingIntent::ActMsg { .. } => 8,
            PendingIntent::Motion(_) => 9,
            PendingIntent::ForcePose => 10,
            PendingIntent::MotionArrive(_) => 11,
        }
    }
}

/// One entry of the pending queue.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PendingEntry {
    /// What to say.
    pub intent: PendingIntent,
    /// To whom.
    pub audience: IntentAudience,
}

/// The per-mover server bookkeeping of the movement pipeline (`MoveSync`, 18 §5.2), `Mover.sync`.
#[derive(Clone, Debug, Default)]
pub struct MoveSync {
    /// Intents waiting for their stamp, in enqueue order.
    pub pending: Vec<PendingEntry>,
    /// The sim events of this mover, drained at the barrier of each frame.
    pub events: SimEvents,
    /// Legacy NPC `m_nCorr < 0`: moving to an AI-issued destination (collision corrections armed).
    pub corr_armed: bool,
    /// Keyframe settle detection (18 §5.3).
    pub kf_was_moving: bool,
    /// The pipeline initialised this mover (bounds, AI `init`).
    pub inited: bool,
    /// Players: the last accepted `PlayerMoveState.seq` and the session it counted in (`seq` restarts with
    /// every WebSocket session, so a re-attached player starts again).
    pub last_seq: Option<(u64, u32)>,
    /// The frame an effect on this mover takes, valid while `pre_tick` is the current tick (18 §5.2 rule 1).
    pub next_pre_frame: u64,
    /// The tick `next_pre_frame` was written in.
    pub pre_tick: u64,
}

/// The frame at which an effect applied to `m` **right now** is honoured by the server simulation (18 §5.2
/// rule 1): `4n` in phase 1 and in the AI pass, `F + 1` at the barrier of frame `F`, `4n + 4` after the
/// frame loop (the tick tails and phases 3-5). A mover the sleep rule skipped keeps `4n` for the tick.
/// Doc 17 stamps `Damage.server_frame` with it, doc 15 its `SetPos`.
pub fn effect_frame(m: &crate::mover::Mover, now: Tick) -> SimFrame {
    if m.sync.pre_tick == now.0 {
        SimFrame(m.sync.next_pre_frame)
    } else {
        now.first_frame()
    }
}

// ---- frame lookups --------------------------------------------------------------------------------------------

/// The `MoverLookup` of one frame (31 §3.3): the start-of-frame snapshot of the awake movers plus the
/// objects they walk to that the sim must see but that are not awake movers (ground items, ctrls).
#[derive(Clone, Debug, Default)]
pub struct FrameLookup {
    snap: FrameSnapshot,
    extra: Vec<(ObjId, MoverSnapshot)>,
}

impl FrameLookup {
    /// The snapshot of `awake` (arena keys) at the start of the frame.
    pub fn capture(store: &ObjectStore, awake: &[MoverKey], flags: &SimFlags) -> FrameLookup {
        let snap = FrameSnapshot::capture(
            awake
                .iter()
                .filter_map(|k| store.movers.get(*k))
                .map(|m| &m.sim),
            flags,
        );
        let mut extra: Vec<(ObjId, MoverSnapshot)> = Vec::new();
        for m in awake.iter().filter_map(|k| store.movers.get(*k)) {
            let dest = m.sim.dest.obj;
            if m.sim.is_empty_dest_obj() || dest == m.sim.id {
                continue;
            }
            if snap_has(&snap, &extra, dest) {
                continue;
            }
            if let Some(s) = object_snapshot(store, dest, flags) {
                extra.push((dest, s));
            }
        }
        extra.sort_by_key(|(id, _)| *id);
        FrameLookup { snap, extra }
    }

    /// A lookup that knows exactly the objects `ids` (the phase-1 handlers need the player's own destination
    /// only).
    pub fn for_objects(store: &ObjectStore, ids: &[ObjId], flags: &SimFlags) -> FrameLookup {
        let mut extra: Vec<(ObjId, MoverSnapshot)> = ids
            .iter()
            .filter_map(|id| object_snapshot(store, *id, flags).map(|s| (*id, s)))
            .collect();
        extra.sort_by_key(|(id, _)| *id);
        extra.dedup_by_key(|(id, _)| *id);
        FrameLookup {
            snap: FrameSnapshot::default(),
            extra,
        }
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.snap.len() + self.extra.len()
    }

    /// `true` when nothing is known.
    pub fn is_empty(&self) -> bool {
        self.snap.is_empty() && self.extra.is_empty()
    }
}

fn snap_has(snap: &FrameSnapshot, extra: &[(ObjId, MoverSnapshot)], id: ObjId) -> bool {
    snap.snapshot(id).is_some() || extra.iter().any(|(i, _)| *i == id)
}

impl MoverLookup for FrameLookup {
    fn snapshot(&self, id: ObjId) -> Option<MoverSnapshot> {
        if let Some(s) = self.snap.snapshot(id) {
            return Some(s);
        }
        self.extra
            .binary_search_by_key(&id, |(i, _)| *i)
            .ok()
            .and_then(|i| self.extra.get(i))
            .map(|(_, s)| *s)
    }
}

/// The snapshot of any live object: a mover as the sim sees it, an item or a ctrl as a still point with a small
/// box (their model bounds belong to docs 19 / WP-WORLD-18).
fn object_snapshot(store: &ObjectStore, id: ObjId, flags: &SimFlags) -> Option<MoverSnapshot> {
    if let Some(m) = store.mover(id) {
        return (!m.core.deleted).then(|| snapshot_of(&m.sim, flags));
    }
    let core = store.core(id)?;
    if core.deleted {
        return None;
    }
    let bounds = Aabb {
        min: Vec3::new(-0.3, 0.0, -0.3),
        max: Vec3::new(0.3, 0.6, 0.3),
    };
    Some(MoverSnapshot {
        pos: core.pos,
        radius: flyff_sim::bounds::radius(&bounds, core.scale),
        alive: true,
        is_fly: false,
        speed: 0.0,
        angle: core.angle,
        bounds,
        scale: core.scale,
        is_player: false,
    })
}

/// Squared 3-D distance in `f32` (the original `(v - GetPos()).Length²`).
pub(crate) fn dist_sq(a: Vec3, b: Vec3) -> f32 {
    let d = a - b;
    d.x * d.x + d.y * d.y + d.z * d.z
}
