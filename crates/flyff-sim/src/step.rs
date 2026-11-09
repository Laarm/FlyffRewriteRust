//! The frame stepper and its environment (Plan/31 §3, §16.2; WP-SIM-04/06).
//!
//! [`step_frame`] advances ONE mover by one 1/60 s frame in the order of §3.2; cross-mover reads go
//! through the start-of-frame [`FrameSnapshot`] (§3.3); everything the game has to react to comes out as
//! [`SimEvent`]s drained at the frame barrier (§3.4).

use crate::anim::motion_table::MotionTable;
use crate::collision::dynamic::DynamicColliders;
use crate::collision::world::CollisionWorld;
use crate::mover::SimMover;
use crate::num::Vec3;
use crate::types::{MoverKind, OBJSTA_DEAD, ObjMsg};
use flyff_common::ObjId;
use flyff_common::SimFrame;
pub use flyff_common::config::SimFlags;
use flyff_common::rng::GameRng;

/// Which half of the system runs the sim (`SimRole`): selects the `[S]`/`[C]` branches of Plan/31 §6-§7.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SimRole {
    /// The authoritative server.
    Server,
    /// The browser client.
    Client,
}

/// How events are produced (`EventMode`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EventMode {
    /// Everything is emitted.
    Normal,
    /// Fast-forward: cosmetic events (sound, quake, footsteps, effects) are suppressed; state transitions
    /// and steering/arrival events are kept.
    Silent,
}

/// A start-of-frame view of another mover (Plan/31 §3.3).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct MoverSnapshot {
    /// Position.
    pub pos: Vec3,
    /// `GetRadius()` (§11.5).
    pub radius: f32,
    /// `!DIE_ALL` and not deleted.
    pub alive: bool,
    /// The mover flies.
    pub is_fly: bool,
    /// The mover's own `get_speed` (pets copy it with `improved_pets`).
    pub speed: f32,
    /// Yaw.
    pub angle: f32,
    /// Model AABB.
    pub bounds: crate::collision::geom::Aabb,
    /// Scale.
    pub scale: f32,
    /// Player (projectile targets).
    pub is_player: bool,
}

/// Read access to other movers during a frame (always a snapshot taken before any mover stepped).
pub trait MoverLookup {
    /// The snapshot of mover `id`, if it exists.
    fn snapshot(&self, id: ObjId) -> Option<MoverSnapshot>;
}

/// A lookup that knows no mover (tests and single-mover scenes).
#[derive(Clone, Copy, Debug, Default)]
pub struct EmptyLookup;

impl MoverLookup for EmptyLookup {
    fn snapshot(&self, _id: ObjId) -> Option<MoverSnapshot> {
        None
    }
}

/// The frame snapshot: independent of the insertion order of the movers (sorted by id).
#[derive(Clone, Debug, Default)]
pub struct FrameSnapshot {
    entries: Vec<(ObjId, MoverSnapshot)>,
}

impl FrameSnapshot {
    /// Captures every mover before frame `F` is stepped.
    pub fn capture<'a>(
        movers: impl Iterator<Item = &'a SimMover>,
        flags: &SimFlags,
    ) -> FrameSnapshot {
        let mut entries: Vec<(ObjId, MoverSnapshot)> =
            movers.map(|m| (m.id, snapshot_of(m, flags))).collect();
        entries.sort_by_key(|(id, _)| *id);
        FrameSnapshot { entries }
    }

    /// Adds destination targets that are not movers (the client's ctrls: `CMD_SetUseItem` walks to a ctrl, and `process_move`
    /// stands still with `DestObjLost` for an id the lookup does not know). An id that a mover already has keeps the mover.
    pub fn with_statics(
        mut self,
        statics: impl IntoIterator<Item = (ObjId, MoverSnapshot)>,
    ) -> FrameSnapshot {
        let before = self.entries.len();
        for (id, s) in statics {
            if self.entries.iter().take(before).all(|(i, _)| *i != id) {
                self.entries.push((id, s));
            }
        }
        if self.entries.len() != before {
            self.entries.sort_by_key(|(id, _)| *id);
        }
        self
    }

    /// Number of movers captured.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` when nothing was captured.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl MoverLookup for FrameSnapshot {
    fn snapshot(&self, id: ObjId) -> Option<MoverSnapshot> {
        self.entries
            .binary_search_by_key(&id, |(i, _)| *i)
            .ok()
            .and_then(|i| self.entries.get(i))
            .map(|(_, s)| *s)
    }
}

/// The snapshot of one mover (its own speed, no pet recursion).
pub fn snapshot_of(m: &SimMover, _flags: &SimFlags) -> MoverSnapshot {
    MoverSnapshot {
        pos: m.pos,
        radius: crate::bounds::radius(&m.params.bounds, m.params.scale),
        alive: !m.act.state.is_die(),
        is_fly: m.act.flags.fly() || m.params.flying_npc,
        speed: crate::kin::own_speed(m),
        angle: m.angle,
        bounds: m.params.bounds,
        scale: m.params.scale,
        is_player: m.params.kind == MoverKind::Player,
    }
}

/// Game-rule predicates the sim consults synchronously (no mutation by implementations).
pub trait SimRules {
    /// Server: `IsSteal` unless `EVE_STEAL` (doc 17); client: `false`.
    fn is_steal(&self, attacker: ObjId, target: ObjId) -> bool;
    /// Server `GetAttackResult` (hit or miss, doc 17); client: `true`.
    fn attack_result(&mut self, attacker: ObjId, target: ObjId, mop: u32) -> bool;
}

/// Rules that allow everything: nothing is stolen and every attack lands (the client's rule set).
#[derive(Clone, Copy, Debug, Default)]
pub struct NoRules;

impl SimRules for NoRules {
    fn is_steal(&self, _attacker: ObjId, _target: ObjId) -> bool {
        false
    }
    fn attack_result(&mut self, _attacker: ObjId, _target: ObjId, _mop: u32) -> bool {
        true
    }
}

/// What kind of hit an `AttrHit` event reports.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HitKind {
    /// Combo melee attack (`OnAttackMelee`).
    Melee,
    /// Melee skill hit.
    MeleeSkill,
    /// Magic skill hit.
    MagicSkill,
    /// Wand attack (`OnAttackMagic`).
    Wand,
    /// Ranged attack.
    Range,
    /// Special attack (`OnAttackSP`).
    Special,
    /// Flying melee.
    FlyMelee,
}

/// What a `RepeatShot` event repeats.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShotKind {
    /// A melee skill.
    MeleeSkill,
    /// A magic skill.
    MagicSkill,
}

/// Something the game or the client host must react to (Plan/31 §16.4).
#[derive(Clone, PartialEq, Debug)]
pub enum SimEvent {
    /// Ground/fly collision result was non-zero (`OnActCollision`).
    Collided,
    /// An attribute undo or slide landing (server `AIMSG_COLLISION`).
    AttrBlocked,
    /// A ledge drop with `delta.y < -0.1` (`OnActDrop`).
    Dropped,
    /// SJUMP3 -> SJUMP4 (`OnActLanding`).
    Landed,
    /// A jump finished (server honor counter).
    JumpCompleted,
    /// Entered a `HATTR_DIE` cell.
    EnteredDeathZone,
    /// Entered a `HATTR_NOFLY` cell while flying.
    EnteredNoFly,
    /// The destination position was reached.
    ArrivedAtPos,
    /// The destination object was reached (`cmd` = `OBJACT_*` of the command).
    ArrivedAtObj {
        /// The destination object.
        target: ObjId,
        /// `OBJACT_*`.
        cmd: u32,
    },
    /// The destination object vanished.
    DestObjLost,
    /// The active mover chose a destination position (client report).
    DestPosChosen {
        /// The chosen position.
        pos: Vec3,
        /// `forward`.
        forward: bool,
    },
    /// `FORWARD` started (`AIMSG_BEGINMOVE`).
    BeganMove,
    /// A scripted move pattern finished.
    PatternDone,
    /// An attack message started an attack (`OnAttackStart`).
    AttackStarted {
        /// The target.
        target: ObjId,
        /// The `OBJMSG_*` that started it.
        msg: ObjMsg,
    },
    /// `target->SetJJim(this)`.
    JJim {
        /// The target.
        target: ObjId,
    },
    /// `RemoveInvisible()`.
    RemoveInvisible,
    /// A hit frame fired.
    AttrHit {
        /// What hit.
        kind: HitKind,
        /// The state word at the hit.
        state: u32,
        /// The target (`id_target` / `objid_hit`).
        target: ObjId,
        /// Hit index of multi-hit skills.
        hit_index: i32,
        /// The frame being stepped.
        frame: u64,
    },
    /// A repeated shot of a multi-shot skill.
    RepeatShot {
        /// Skill kind.
        kind: ShotKind,
        /// The frame being stepped.
        frame: u64,
    },
    /// Non-player melee continues after the hit frame (`OnAttackMeleeContinue`): the attack sub-state `dw` the
    /// emitting step saw and the two counters of that moment. The barrier sees the mover AFTER the step (the
    /// free-running `m_nCount` was incremented at the end of `process_action`, and on the end frame the attack
    /// state and `motion_hit_count` were reset after the emission), so a consumer that tests `count & 3` or the
    /// hit count (the ClockWorks laser, the Meteonyker fire breath) must read them here.
    MeleeContinue {
        /// The attack sub-state (`OBJSTA_ATK1..ATK4`).
        state: u32,
        /// `CActionMover::m_nCount` at the emission.
        count: i32,
        /// `m_nMotionHitCount` at the emission.
        hit_count: i32,
    },
    /// A non-player melee attack ended.
    EndMeleeAttack,
    /// A melee skill ended.
    MeleeSkillEnded,
    /// A magic skill ended.
    MagicSkillEnded,
    /// The owner should drain the magic attack queue.
    DrainMagicQueue,
    /// The appear scene ended.
    AppearEnded,
    /// The stun ended.
    StunEnded,
    /// The dead counter expired (DEAD -> DISAPPEAR).
    DeadCountExpired,
    /// The corpse may be deleted.
    Disappeared,
    /// The mover died.
    Died {
        /// The attacker (`ObjId::NONE` when unknown).
        attacker: ObjId,
    },
    /// The owner should stop the attacker (flying knock-back).
    AttackerStop {
        /// The attacker.
        attacker: ObjId,
    },
    /// A cast was cancelled.
    CastCancelled,
    /// Turbo ran out of fuel.
    TurboOff,
    /// The ride changed (`None` = dismounted).
    RideChanged {
        /// The ride item.
        item: Option<u32>,
    },
    /// A flying message arrived without ride parameters.
    RideMissing,
    /// Flight auto-tracking steered the active mover (`PostPlayerAngle(TRUE)`, client): the host reports
    /// `ANGLE` (throttled to one per 30 frames, Plan/31 §9.3, §15.3).
    FlyAngle,
    /// The active flying mover found its state word empty and sent `STAND`
    /// (`ProcessAction`: `SendPlayerMoved2`, client): the host reports `MOVED`.
    FlyMoved,
    /// A sound attribute fired (client).
    AttrSound {
        /// The animated object.
        obj: ObjId,
        /// Sound id.
        sound: i32,
        /// X.
        x: f32,
        /// Y.
        y: f32,
        /// Z.
        z: f32,
        /// Positional sound.
        positional: bool,
    },
    /// A quake attribute fired (client).
    AttrQuake {
        /// The animated object.
        obj: ObjId,
    },
    /// Camera shake (client).
    CameraQuake {
        /// Duration in frames.
        frames: i32,
        /// Strength.
        scale: f32,
    },
    /// Dust under a sliding mover (client).
    DustFx,
    /// Drift effect (client).
    DriftFx,
    /// Sword trail on (client).
    SwordTrail {
        /// `MOP_HITSLOW`.
        hit_slow: bool,
    },
    /// Sword trail off (client).
    SwordTrailClear,
    /// Boss charge effect (client).
    BossChargeFx {
        /// The attack state.
        state: u32,
    },
    /// Yoyo throw effect (client).
    YoyoThrowFx,
    /// Casting spark (client).
    CastSpark,
    /// The party stretching SFX after sitting down (client).
    SatDown,
    /// The renderer restarts the ride animation (client).
    RideFrameReset,
    /// The active mover started a melee/flying attack the client must report.
    LocalAttack {
        /// The attack message.
        msg: ObjMsg,
        /// The target.
        target: ObjId,
        /// `AF_*` flags returned by the message.
        flags: i32,
    },
    /// The active mover reached its command target.
    CmdArrived {
        /// `OBJACT_*`.
        cmd: u32,
        /// The target.
        target: ObjId,
    },
    /// A command was aborted.
    CmdAborted,
    /// Show a notice text.
    Notice {
        /// `TID_*`.
        tid: u32,
    },
    /// Cancel the cast UI.
    CastCancel,
    /// Release the left mouse button state.
    LmbRelease,
    /// A pending land was touched (client: raise the fetch priority).
    ChunkMissing {
        /// Land x.
        lx: u16,
        /// Land z.
        lz: u16,
    },
    /// A projectile hit.
    ProjectileHit {
        /// Projectile id.
        id: u32,
        /// Hit position.
        pos: Vec3,
    },
    /// A projectile expired.
    ProjectileExpired {
        /// Projectile id.
        id: u32,
    },
    /// A footstep (client).
    Footstep {
        /// The walking object.
        obj: ObjId,
        /// X.
        x: f32,
        /// Y.
        y: f32,
        /// Z.
        z: f32,
    },
}

impl SimEvent {
    /// Cosmetic events are suppressed in [`EventMode::Silent`].
    pub fn is_cosmetic(&self) -> bool {
        matches!(
            self,
            SimEvent::AttrSound { .. }
                | SimEvent::AttrQuake { .. }
                | SimEvent::Footstep { .. }
                | SimEvent::CameraQuake { .. }
                | SimEvent::DustFx
                | SimEvent::DriftFx
                | SimEvent::SwordTrail { .. }
                | SimEvent::SwordTrailClear
                | SimEvent::BossChargeFx { .. }
                | SimEvent::YoyoThrowFx
                | SimEvent::CastSpark
                | SimEvent::SatDown
                | SimEvent::RideFrameReset
                | SimEvent::Landed
        )
    }
}

/// The events of one mover, drained by the owner at the frame barrier.
#[derive(Clone, Debug, Default)]
pub struct SimEvents(Vec<SimEvent>);

impl SimEvents {
    /// An empty buffer.
    pub fn new() -> SimEvents {
        SimEvents(Vec::new())
    }
    /// Appends an event.
    pub fn push(&mut self, e: SimEvent) {
        self.0.push(e);
    }
    /// Removes and yields every event in emission order.
    pub fn drain(&mut self) -> std::vec::Drain<'_, SimEvent> {
        self.0.drain(..)
    }
    /// The events in emission order.
    pub fn iter(&self) -> std::slice::Iter<'_, SimEvent> {
        self.0.iter()
    }
    /// Number of events.
    pub fn len(&self) -> usize {
        self.0.len()
    }
    /// `true` when empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// Discards every event.
    pub fn clear(&mut self) {
        self.0.clear();
    }
    /// Applies `f` to the events emitted since the buffer held `start` events.
    pub fn patch_from(&mut self, start: usize, mut f: impl FnMut(&mut SimEvent)) {
        for e in self.0.iter_mut().skip(start) {
            f(e);
        }
    }
    /// `true` when an event equal to `e` was emitted.
    pub fn contains(&self, e: &SimEvent) -> bool {
        self.0.contains(e)
    }
}

/// Everything a frame step reads (Plan/31 §16.2).
pub struct SimEnv<'a> {
    /// The frame being stepped (state is "start of F" on entry).
    pub frame: SimFrame,
    /// Server or client.
    pub role: SimRole,
    /// Feature switches.
    pub flags: SimFlags,
    /// The collision world.
    pub world: &'a CollisionWorld,
    /// Dynamic ctrl colliders of the mover's layer.
    pub dynamic: &'a DynamicColliders,
    /// The motion table.
    pub motions: &'a MotionTable,
    /// Other movers (start-of-frame snapshot).
    pub lookup: &'a dyn MoverLookup,
    /// Event mode.
    pub mode: EventMode,
}

/// The per-call context threaded through the handlers (environment, rules, rng, event sink).
pub struct Cx<'a> {
    /// The environment.
    pub env: &'a SimEnv<'a>,
    /// Game-rule predicates.
    pub rules: &'a mut dyn SimRules,
    /// Cosmetic randomness (idle motions, damage motion choice).
    pub rng: &'a mut dyn GameRng,
    /// The event sink.
    pub ev: &'a mut SimEvents,
}

impl<'a> Cx<'a> {
    /// Bundles the arguments of the public entry points.
    pub fn new(
        env: &'a SimEnv<'a>,
        rules: &'a mut dyn SimRules,
        rng: &'a mut dyn GameRng,
        ev: &'a mut SimEvents,
    ) -> Cx<'a> {
        Cx {
            env,
            rules,
            rng,
            ev,
        }
    }

    /// `true` on the server role.
    #[inline]
    pub fn server(&self) -> bool {
        self.env.role == SimRole::Server
    }

    /// `true` on the client role.
    #[inline]
    pub fn client(&self) -> bool {
        self.env.role == SimRole::Client
    }

    /// The frame number being stepped.
    #[inline]
    pub fn frame(&self) -> u64 {
        self.env.frame.0
    }

    /// Emits an event (cosmetic ones are dropped in [`EventMode::Silent`]).
    pub fn emit(&mut self, e: SimEvent) {
        if self.env.mode == EventMode::Silent && e.is_cosmetic() {
            return;
        }
        self.ev.push(e);
    }
}

/// Advances `m` by one 1/60 s frame (`step_frame`, Plan/31 §3.2). State is "start of frame F" on entry
/// and "start of F + 1" on return; everything the game must react to is in `ev`.
pub fn step_frame(
    m: &mut SimMover,
    env: &SimEnv<'_>,
    rules: &mut dyn SimRules,
    rng: &mut dyn GameRng,
    ev: &mut SimEvents,
) {
    let mut cx = Cx::new(env, rules, rng, ev);
    step_frame_cx(m, &mut cx);
}

/// `step_frame` over an existing context.
pub fn step_frame_cx(m: &mut SimMover, cx: &mut Cx<'_>) {
    let first_event = cx.ev.len();
    // 1. destination steering / arrival tests (not while sitting)
    if !m.act.flags.sit() {
        crate::steer::process_move(m, cx);
    }
    // 2. animation speed
    crate::steer::ani_speed(m, cx.env);
    // 3. ProcessPreAction
    m.act.delta_accu = Vec3::ZERO;
    // 4. scripted move pattern
    crate::steer::process_move_pattern(m, cx);
    // 5. state processing + gravity/fly + external force
    crate::kin::process_action(m, cx);
    // 6. footstep window, frame clock, blending
    crate::anim::select::frame_move_mover(m, cx);
    // 7. collision applies delta_accu (one frame)
    crate::collision::ground::process(m, cx);
    // the sound and footstep events carry the position at the END of the step (Plan/31 §12.4)
    let (id, p) = (m.id, m.pos);
    cx.ev.patch_from(first_event, |e| match e {
        SimEvent::AttrSound { obj, x, y, z, .. } | SimEvent::Footstep { obj, x, y, z }
            if *obj == id =>
        {
            (*x, *y, *z) = (p.x, p.y, p.z);
        }
        _ => {}
    });
}

/// `true` for the dead state `OBJSTA_DEAD` word test used by sleepers.
#[inline]
pub fn is_dead_word(state: u32) -> bool {
    state & OBJSTA_DEAD != 0
}
