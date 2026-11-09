//! The client-side simulation host: the core of the browser `SimWorld` (Plan/31 §3.5, §15, §17; WP-SIM-13..17).
//!
//! `flyff-wasm` is a thin `wasm-bindgen` shell around [`ClientSim`]; everything with behaviour lives here so
//! that native tests, the vector runner (`testkit`), bots and `/dev/sim` replays execute the SAME code as the
//! browser (the determinism contract of D4 and WP-SIM-17).
//!
//! | module | content |
//! |---|---|
//! | [`commands`] | the command-buffer decoder (§17.4) |
//! | [`decor`] | animated decor entries and the render pose of ctrls (§13.7, `DECOR_*` / `CTRL_*` ops) |
//! | [`events`] | the JS event buffer and the `SimEvent` export table (§17.6) |
//! | [`local_pet`] | the controller of the local system pets (`CAIEgg`, §7.9 of the client plan) |
//! | [`view`] | the render view with palettes, render-space matrices and the report area (§17.7) |
//! | [`preview`] | preview movers (§15.1) |
//! | `record` (feature `dev-tools`) | the command/input recorder of `/dev/sim` (§20.6) |
//!
//! One frame of [`ClientSim::step`] is `step 1..7` of §3.5: due commands, the control step of the active
//! mover, the start-of-frame snapshot, every world mover's `step_frame` (frame-major, insertion order),
//! preview movers, the frame barrier (collision responses, command arrivals, queue drains, event export),
//! render smoothing, frame counter.

pub mod commands;
pub mod decor;
pub mod events;
pub mod local_pet;
pub mod preview;

pub mod view;

use crate::act::ActMsg;
use crate::anim::motion_table::{ModelKey, MotionTable};
use crate::bounds::{ModelBounds, radius};
use crate::collision::ctrl_models::CtrlModelSet;
use crate::collision::dynamic::{CtrlCollider, DynamicColliders};
use crate::collision::world::{CollisionWorld, InitialLands, WorldDesc};
use crate::control::ActiveControl;
use crate::mover::{SimMover, SimMoverParams, SkillCtx};
use crate::num::Vec3;
use crate::pose::{PoseLibrary, PoseState};
use crate::projectile::{Launch, ProjStep, Projectile, ProjectileKind, SpawnArgs};
use crate::remote::{self, Keyframe, PosHistory};
use crate::step::{
    EventMode, FrameSnapshot, HitKind, MoverSnapshot, NoRules, SimEnv, SimEvent, SimEvents,
    SimFlags, SimRole, SimRules, step_frame,
};
use crate::sync::{self, IntentOp};
use crate::types::*;
use crate::wire::{self, reason};
use commands::{AddMover, Cmd, ProjectileSpawn};
use decor::DecorSet;
use events::{EventBuffer, export};
use flyff_common::ObjId;
use flyff_common::SimFrame;
use flyff_common::rng::Pcg32Rng;
use preview::PreviewMover;
use std::collections::BTreeMap;
use std::sync::Arc;
use view::ViewBuffer;

/// `MAX_STEPS_PER_CALL`: the client `step(n)` cap (Plan/31 §3.1).
pub const MAX_STEPS_PER_CALL: u32 = 60;
/// The largest remote extrapolation cap accepted from the configuration words.
pub const MAX_EXTRAP_CAP: u32 = 120;
/// The most frames a late `PROJECTILE_SPAWN` is caught up (2 s, like an intent).
pub const PROJECTILE_FF_CAP: u32 = 120;
/// The most projectiles the host keeps alive (a stalled or hostile stream cannot grow the list).
pub const MAX_PROJECTILES: usize = 512;

/// `SimWorld::new` configuration words: `[SimFlags bits, hit_attr_rule, seed, remote_extrap_cap_frames]`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClientConfig {
    /// The simulation switches (identical on server and client).
    pub flags: SimFlags,
    /// Seed of the cosmetic RNG.
    pub seed: u32,
    /// `fidelity.remote_extrap_cap_frames`.
    pub extrap_cap: u32,
}

impl ClientConfig {
    /// Parses the four configuration words.
    pub fn from_words(words: &[u32]) -> Result<ClientConfig, SimError> {
        let [bits, rule, seed, cap] = *words
            .first_chunk::<4>()
            .ok_or(SimError::BadCommand("SimWorld needs 4 configuration words"))?;
        Ok(ClientConfig {
            flags: wire::sim_flags_from_words(bits, rule),
            seed,
            extrap_cap: cap.min(MAX_EXTRAP_CAP),
        })
    }
}

/// A world mover: the sim state plus the client-side pose and history.
#[derive(Clone, Debug)]
pub struct ClientMover {
    /// The simulation state.
    pub sim: SimMover,
    /// The model-space bones of the last evaluation.
    pub pose: PoseState,
    /// The 128-frame position history of the keyframe rule.
    pub hist: PosHistory,
    /// Skeleton id (`0` = none).
    pub skel_id: u32,
    /// The renderer wants the pose (`SET_VISIBLE`).
    pub visible: bool,
    /// The renderer wants the attachment record.
    pub want_attach: bool,
    pub(crate) events: SimEvents,
}

/// A projectile the host steps: the visual twin of a server projectile (Plan/31 §14, §17.7).
#[derive(Clone, Debug, PartialEq)]
pub struct ClientProjectile {
    /// The flight state of the shared kinematics.
    pub p: Projectile,
    /// The position at the start of the last step (display interpolation).
    pub prev_pos: Vec3,
}

/// A frame-stamped command waiting for its local frame.
#[derive(Clone, Debug)]
struct Queued {
    server_frame: u64,
    cmd: Cmd,
}

/// The `SimWorld` core.
pub struct ClientSim {
    flags: SimFlags,
    extrap_cap: u32,
    seed: u32,
    rng: Pcg32Rng,
    frame: u64,
    offset: i64,
    world_id: u32,
    world: CollisionWorld,
    dynamic: DynamicColliders,
    ctrl_models: Arc<CtrlModelSet>,
    decor: DecorSet,
    motions: MotionTable,
    bounds: ModelBounds,
    poses: PoseLibrary,
    movers: Vec<ClientMover>,
    previews: Vec<PreviewMover>,
    projectiles: Vec<ClientProjectile>,
    active: Option<ObjId>,
    control: ActiveControl,
    queue: Vec<Queued>,
    snap: Option<FrameSnapshot>,
    chunk_bytes: BTreeMap<(u32, u32), usize>,
    out: EventBuffer,
    view: ViewBuffer,
}

/// A ctrl as a destination object: its origin, the radius of its model box under its scale (`GetRadius()`), standing still.
fn ctrl_target(c: &CtrlCollider) -> (ObjId, MoverSnapshot) {
    let m = |i: usize| c.world.0.get(i).copied().unwrap_or(0.0);
    let scale = crate::num::sqrtf(m(0) * m(0) + m(1) * m(1) + m(2) * m(2));
    (
        c.id,
        MoverSnapshot {
            pos: Vec3::new(m(12), m(13), m(14)),
            radius: radius(&c.aabb, scale),
            alive: true,
            is_fly: false,
            speed: 0.0,
            angle: 0.0,
            bounds: c.aabb,
            scale,
            is_player: false,
        },
    )
}

/// The collision world used while no world is entered (every query behaves as `Pending`).
fn no_world() -> CollisionWorld {
    let desc = WorldDesc {
        mpu: 4,
        lands_w: 1,
        lands_h: 1,
        fly_allowed: false,
        housing: false,
    };
    match CollisionWorld::new(desc, InitialLands::Pending) {
        Ok(w) => w,
        // the description above is valid; unreachable in practice, and a constructor cannot fail here
        Err(_) => unreachable_world(),
    }
}

#[allow(clippy::panic)]
fn unreachable_world() -> ! {
    panic!("the placeholder world description is valid") // det-ok: constant description, validated by tests
}

/// Builds the environment of one frame from its parts (disjoint field borrows of [`ClientSim`]).
#[allow(clippy::too_many_arguments)]
fn make_env<'a>(
    flags: SimFlags,
    world: &'a CollisionWorld,
    dynamic: &'a DynamicColliders,
    motions: &'a MotionTable,
    lookup: &'a FrameSnapshot,
    frame: u64,
    mode: EventMode,
) -> SimEnv<'a> {
    SimEnv {
        frame: SimFrame(frame),
        role: SimRole::Client,
        flags,
        world,
        dynamic,
        motions,
        lookup,
        mode,
    }
}

impl ClientSim {
    /// `new SimWorld(config)`.
    pub fn new(words: &[u32]) -> Result<ClientSim, SimError> {
        let cfg = ClientConfig::from_words(words)?;
        Ok(ClientSim::with_config(cfg))
    }

    /// A host from a parsed configuration.
    pub fn with_config(cfg: ClientConfig) -> ClientSim {
        ClientSim {
            flags: cfg.flags,
            extrap_cap: cfg.extrap_cap,
            seed: cfg.seed,
            rng: Pcg32Rng::new(u64::from(cfg.seed), 1),
            frame: 0,
            offset: 0,
            world_id: 0,
            world: no_world(),
            dynamic: DynamicColliders::new(),
            ctrl_models: Arc::new(CtrlModelSet::default()),
            decor: DecorSet::new(),
            motions: MotionTable::default(),
            bounds: ModelBounds::default(),
            poses: PoseLibrary::new(),
            movers: Vec::new(),
            previews: Vec::new(),
            projectiles: Vec::new(),
            active: None,
            control: ActiveControl::new(),
            queue: Vec::new(),
            snap: None,
            chunk_bytes: BTreeMap::new(),
            out: EventBuffer::new(),
            view: ViewBuffer::new(),
        }
    }

    // -------------------------------------------------------------------------------------------
    // world and data loads (Plan/31 §17.3)
    // -------------------------------------------------------------------------------------------

    /// `set_world([world_id, mpu, lands_w, lands_h, fly_allowed, housing])`: drops every chunk, world mover,
    /// ctrl collider, decor entry and projectile and starts all lands `Pending`; `world_id == 0` means "no
    /// world" (the other words are ignored, preview movers stay).
    pub fn set_world(&mut self, desc: &[f32]) -> Result<(), SimError> {
        let g = |i: usize| desc.get(i).copied();
        let id = g(0).ok_or(SimError::BadWorld("set_world needs 6 values"))?;
        let whole = |v: f32| -> Result<u32, SimError> {
            if v.is_finite() && v >= 0.0 && v.fract() == 0.0 && v <= 4_294_967_040.0 {
                Ok(v as u32) // BOUNDS: range checked
            } else {
                Err(SimError::BadWorld(
                    "set_world values must be non-negative integers",
                ))
            }
        };
        let world_id = whole(id)?;
        let (world, new_id) = if world_id == 0 {
            (no_world(), 0)
        } else {
            if desc.len() < 6 {
                return Err(SimError::BadWorld("set_world needs 6 values"));
            }
            let wd = WorldDesc {
                mpu: whole(g(1).unwrap_or(0.0))?,
                lands_w: whole(g(2).unwrap_or(0.0))?,
                lands_h: whole(g(3).unwrap_or(0.0))?,
                fly_allowed: g(4).unwrap_or(0.0) != 0.0,
                housing: g(5).unwrap_or(0.0) != 0.0,
            };
            (CollisionWorld::new(wd, InitialLands::Pending)?, world_id)
        };
        // nothing changes before the description was accepted
        self.dynamic = if new_id == 0 {
            DynamicColliders::new()
        } else {
            DynamicColliders::for_world(world.mpu, world.lands_w, world.lands_h)
        };
        self.dynamic.set_models(Arc::clone(&self.ctrl_models));
        let preview_ids: Vec<ObjId> = self.previews.iter().map(|p| p.sim.id).collect();
        self.decor.clear_world(&preview_ids);
        self.world = world;
        self.world_id = new_id;
        self.chunk_bytes.clear();
        self.movers.clear();
        self.projectiles.clear();
        self.queue.clear();
        self.active = None;
        self.control = ActiveControl::new();
        self.snap = None;
        self.out.clear();

        Ok(())
    }

    /// The id of the world entered (`0` = none).
    pub fn world_id(&self) -> u32 {
        self.world_id
    }

    /// `load_motion_table(FLMT)`.
    pub fn load_motion_table(&mut self, bytes: &[u8]) -> Result<(), SimError> {
        self.motions = MotionTable::decode(bytes)?;
        self.snap = None;

        Ok(())
    }

    /// `load_model_bounds(FLMB)`.
    pub fn load_model_bounds(&mut self, bytes: &[u8]) -> Result<(), SimError> {
        self.bounds = ModelBounds::decode(bytes)?;

        Ok(())
    }

    /// `load_ctrl_models(FLYK)` (Plan/31 §10.10, §17.3): the dynamic collision shapes of every ctrl model, once
    /// per session. Colliders already registered are re-posed against the new set.
    pub fn load_ctrl_models(&mut self, bytes: &[u8]) -> Result<(), SimError> {
        let set = Arc::new(CtrlModelSet::decode(bytes)?);
        self.dynamic.set_models(Arc::clone(&set));
        self.ctrl_models = set;

        Ok(())
    }

    /// The dynamic ctrl colliders (queries, tests).
    pub fn colliders(&self) -> &DynamicColliders {
        &self.dynamic
    }

    /// The decor and ctrl render entries.
    pub fn decor(&self) -> &DecorSet {
        &self.decor
    }

    /// `load_chunk(lx, lz, FLYC)`: `Pending -> Loaded` (re-validated, O(n)).
    pub fn load_chunk(&mut self, lx: u32, lz: u32, bytes: &[u8]) -> Result<(), SimError> {
        if self.world_id == 0 {
            return Err(SimError::BadWorld("no world entered"));
        }
        self.world.load_chunk(lx, lz, bytes)?;
        self.chunk_bytes.insert((lx, lz), bytes.len());

        Ok(())
    }

    /// `unload_chunk(lx, lz)`: back to `Pending`.
    pub fn unload_chunk(&mut self, lx: u32, lz: u32) {
        if self.world.set_slot(lx, lz, InitialLands::Pending).is_ok() {
            self.chunk_bytes.remove(&(lx, lz));
        }
    }

    /// `add_skeleton(skel_id, FSK1)`.
    pub fn add_skeleton(&mut self, skel_id: u32, fsk1: &[u8]) -> Result<(), SimError> {
        self.poses.add_skeleton(skel_id, fsk1)?;

        Ok(())
    }

    /// `add_clip(clip_id, FMO1)`.
    pub fn add_clip(&mut self, clip_id: u32, fmo1: &[u8]) -> Result<(), SimError> {
        self.poses.add_clip(clip_id, fmo1)?;

        Ok(())
    }

    /// The skeleton/clip library (tests, previews).
    pub fn poses(&self) -> &PoseLibrary {
        &self.poses
    }

    /// The collision world (queries).
    pub fn world(&self) -> &CollisionWorld {
        &self.world
    }

    /// `land_height(x, z)`.
    pub fn land_height(&self, x: f32, z: f32) -> f32 {
        self.world.land_height(x, z)
    }

    /// `full_height(x, y, z)`.
    pub fn full_height(&self, x: f32, y: f32, z: f32) -> f32 {
        self.world.full_height(Vec3::new(x, y, z))
    }

    /// `ray_cast([ox, oy, oz, dx, dy, dz, max_t, mask])` (Plan/31 §10.11, §17.3) -> `[hit, x, y, z, t, kind,
    /// id (u32 bits), nx, ny, nz]` (LH). `mask` is `RAY_TERRAIN 1 | RAY_STATIC 2 | RAY_CTRL 4` as a float; a
    /// short or non-finite input is a miss. Pass a unit `dir` to get `t` as a distance.
    pub fn ray_cast(&self, ray: &[f32]) -> [f32; 10] {
        use crate::collision::client_query::{RAY_CTRL, RAY_STATIC, RAY_TERRAIN};
        let miss = [0.0_f32; 10];
        let [ox, oy, oz, dx, dy, dz, max_t, mask] = match ray.get(..8) {
            Some(&[a, b, c, d, e, f, g, h]) => [a, b, c, d, e, f, g, h],
            _ => return miss,
        };
        if !mask.is_finite() || mask < 0.0 {
            return miss;
        }
        let mask = (mask as u32) & (RAY_TERRAIN | RAY_STATIC | RAY_CTRL); // BOUNDS: finite and non-negative
        match self.world.ray_cast(
            &self.dynamic,
            Vec3::new(ox, oy, oz),
            Vec3::new(dx, dy, dz),
            max_t,
            mask,
        ) {
            Some(h) => [
                1.0,
                h.pos.x,
                h.pos.y,
                h.pos.z,
                h.t,
                f32::from(h.kind),
                f32::from_bits(h.id),
                h.normal.x,
                h.normal.y,
                h.normal.z,
            ],
            None => miss,
        }
    }

    /// `camera_check_bound([cam x,y,z, look_at x,y,z])` (Plan/43 §8.4) -> `[crash (0/1), out x,y,z, len]`.
    /// A short or non-finite input answers "no crash" with the requested position.
    pub fn camera_check_bound(&self, v: &[f32]) -> [f32; 5] {
        let Some(&[cx, cy, cz, lx, ly, lz]) = v.get(..6) else {
            return [0.0; 5];
        };
        let cam = Vec3::new(cx, cy, cz);
        let look = Vec3::new(lx, ly, lz);
        let finite = |p: Vec3| p.x.is_finite() && p.y.is_finite() && p.z.is_finite();
        if !finite(cam) || !finite(look) {
            return [0.0, cx, cy, cz, 0.0];
        }
        let b = self.world.camera_check_bound(cam, look);
        [
            if b.crash { 1.0 } else { 0.0 },
            b.out.x,
            b.out.y,
            b.out.z,
            b.len,
        ]
    }

    /// `blink_point([from x,y,z, to x,y,z, range])` (Plan/43 §7.10, §10.2; WP-INPUT-13) -> `[x, y, z, ok]`: the
    /// Blinkpool ground point of [`CollisionWorld::blink_point`]; `ok == 0` means the caster's own position. A
    /// short or non-finite input answers `[0, 0, 0, 0]` / the caster position.
    pub fn blink_point(&self, v: &[f32]) -> [f32; 4] {
        let Some(&[fx, fy, fz, tx, ty, tz, range]) = v.get(..7) else {
            return [0.0; 4];
        };
        let from = Vec3::new(fx, fy, fz);
        let r = self.world.blink_point(from, Vec3::new(tx, ty, tz), range);
        [r.pos.x, r.pos.y, r.pos.z, if r.ok { 1.0 } else { 0.0 }]
    }

    /// `target_check(target, range, cone_cos)` (Plan/43 §10.2; WP-INPUT-13): the bits of
    /// [`wire::query`] for the active mover and mover `target`.
    ///
    /// * `IN_RANGE` (1): `IsRangeObj(active, target, range)` (the model radii count, 31 §11.5).
    /// * `IN_CONE` (2): `dot(AngleToVector(angle, -angle_x, 1), normalize(target - pos)) >= cone_cos`, the flying
    ///   wand cone of `WndWorldControlPlayer.cpp:803-836`.
    /// * `AS_BACK_OK` (4): the back-attack rule of `ProcessMoveArrival` (`MoverMove.cpp:130-160`): with
    ///   `a = normalize(target.pos - pos)` and `b = normalize(AngleToVectorXZ(target.angle, 3))` the dot product
    ///   is at least `0.3`.
    ///
    /// No active mover, an unknown target or a non-finite input answers `0`.
    pub fn target_check(&self, target: ObjId, range: f32, cone_cos: f32) -> u32 {
        use crate::bounds::is_range_obj;
        use crate::num::{angle_to_vector, angle_to_vector_xz, length_sq, normalize_or_zero};
        let (Some(active), Some(t)) = (
            self.active.and_then(|id| self.mover(id)),
            self.mover(target),
        ) else {
            return 0;
        };
        let (me, other) = (&active.sim, &t.sim);
        let finite = |p: Vec3| p.x.is_finite() && p.y.is_finite() && p.z.is_finite();
        if !finite(me.pos) || !finite(other.pos) || !range.is_finite() {
            return 0;
        }
        let mut bits = 0;
        if is_range_obj(
            me.pos,
            radius(&me.params.bounds, me.params.scale),
            other.pos,
            radius(&other.params.bounds, other.params.scale),
            range,
        ) {
            bits |= wire::query::IN_RANGE;
        }
        let to_target = normalize_or_zero(other.pos - me.pos);
        let front = angle_to_vector(me.angle, -me.angle_x, 1.0);
        let dot = |a: Vec3, b: Vec3| (a.x * b.x + a.y * b.y) + a.z * b.z;
        if dot(front, to_target) >= cone_cos {
            bits |= wire::query::IN_CONE;
        }
        // an object on top of the caster has no direction: the zero vector never passes the 0.3 rule
        if length_sq(to_target) > 0.0 {
            let facing = normalize_or_zero(angle_to_vector_xz(other.angle, 3.0));
            if dot(to_target, facing) >= 0.3 {
                bits |= wire::query::AS_BACK_OK;
            }
        }
        bits
    }

    /// `sample_force_path(id, grip, hit_slow, [f1 x,y,z, f2 x,y,z])` (Plan/31 §13.8, §17.3; WP-SIM-29): the
    /// sword-trail edge points of the whole swing of mover `id` (world or preview) on a COPY of its motion
    /// clock, `[n, e1 x,y,z, e2 x,y,z, ...]` in model space, three.js convention (`z` negated). `grip` is 0
    /// right hand, 1 left hand, 2 shield, 3 knuckle; `f1`/`f2` are the force points of the weapon `.o3d`
    /// (left-handed object space). A mover without a skeleton or clip answers `[0]`; an unknown mover, grip
    /// or a malformed `force` is an error.
    pub fn sample_force_path(
        &self,
        id: ObjId,
        grip: u32,
        hit_slow: bool,
        force: &[f32],
    ) -> Result<Vec<f32>, SimError> {
        use crate::pose::force::{ForceParams, Grip, sample_force_path};
        use crate::pose::palette::render_pos;
        let grip =
            Grip::from_code(grip).ok_or(SimError::BadCommand("sample_force_path: unknown grip"))?;
        let Some(&[ax, ay, az, bx, by, bz]) = force.get(..6) else {
            return Err(SimError::BadCommand(
                "sample_force_path: force needs 6 values",
            ));
        };
        let (f1, f2) = (Vec3::new(ax, ay, az), Vec3::new(bx, by, bz));
        let finite = |v: Vec3| v.x.is_finite() && v.y.is_finite() && v.z.is_finite();
        if !finite(f1) || !finite(f2) {
            return Err(SimError::BadCommand(
                "sample_force_path: non-finite force point",
            ));
        }
        let (sim, skel_id) = match self.movers.iter().find(|m| m.sim.id == id) {
            Some(m) => (&m.sim, m.skel_id),
            None => match self.previews.iter().find(|p| p.sim.id == id) {
                Some(p) => (&p.sim, p.skel_id),
                None => {
                    return Err(SimError::BadCommand("sample_force_path: unknown mover"));
                }
            },
        };
        let mut out = vec![0.0_f32];
        let Some(skel) = self.poses.skeleton(skel_id) else {
            return Ok(out);
        };
        let clip = sim.motion.clip.and_then(|c| self.poses.clip(c));
        let prev = sim.motion.prev_clip.and_then(|c| self.poses.clip(c));
        let path = sample_force_path(
            skel,
            clip,
            prev,
            &sim.motion,
            &ForceParams {
                grip,
                hit_slow,
                f1,
                f2,
                ani_speed: sim.ani_speed,
                rule: self.flags.hit_attr_rule,
            },
        );
        out.reserve(path.len() * 6);
        for (a, b) in &path {
            let (a, b) = (render_pos(*a), render_pos(*b));
            out.extend_from_slice(&[a.x, a.y, a.z, b.x, b.y, b.z]);
        }
        if let Some(n) = out.first_mut() {
            *n = path.len() as f32; // BOUNDS: at most MAX_FORCE_POINTS = 512, exact in f32
        }
        Ok(out)
    }

    /// `height_attr(x, z)` (Plan/31 §10.3): the attribute class of the cell, `-1` outside the world.
    pub fn height_attr(&self, x: f32, z: f32) -> i32 {
        self.world.height_attr(x, z)
    }

    /// `range_point(id, point, r)` (Plan/31 §10.11): `IsRangeObj(mover, point, r)`; `false` for an unknown mover.
    pub fn range_point(&self, id: ObjId, point: Vec3, r: f32) -> bool {
        self.mover(id).is_some_and(|m| {
            crate::bounds::is_range_point(
                m.sim.pos,
                radius(&m.sim.params.bounds, m.sim.params.scale),
                point,
                r,
            )
        })
    }

    /// `reset_render_smoothing()`: the smoothed state restarts at the simulated one (hidden-tab return, own
    /// `SetPos`; PL-009, PL-014).
    pub fn reset_render_smoothing(&mut self) {
        for m in &mut self.movers {
            remote::reset_smoothing(&mut m.sim);
        }
    }

    /// `memory_report()`: `[chunks, movers, clips, bytes_static, bytes_clips, decor, ctrls_without_model]`.
    pub fn memory_report(&self) -> [u32; 7] {
        let clamp = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        [
            clamp(self.world.loaded_count()),
            clamp(self.movers.len() + self.previews.len()),
            clamp(self.poses.clip_count()),
            clamp(self.chunk_bytes.values().sum::<usize>()),
            clamp(self.poses.clip_bytes()),
            clamp(self.decor.len() + self.decor.ctrl_len()),
            self.dynamic.without_model(),
        ]
    }

    // -------------------------------------------------------------------------------------------
    // accessors
    // -------------------------------------------------------------------------------------------

    /// The local frame the next [`ClientSim::step`] starts at.
    pub fn frame(&self) -> u64 {
        self.frame
    }

    /// The clock offset (`server_frame - local frame`).
    pub fn clock_offset(&self) -> i64 {
        self.offset
    }

    /// The world movers in insertion order.
    pub fn movers(&self) -> &[ClientMover] {
        &self.movers
    }

    /// The preview movers in insertion order.
    pub fn previews(&self) -> &[PreviewMover] {
        &self.previews
    }

    /// The live projectiles in spawn order (finished ones stay for one more frame, flagged `done`).
    pub fn projectiles(&self) -> &[ClientProjectile] {
        &self.projectiles
    }

    /// The world mover `id`.
    pub fn mover(&self, id: ObjId) -> Option<&ClientMover> {
        self.movers.iter().find(|m| m.sim.id == id)
    }

    /// The id of the active (own) mover.
    pub fn active(&self) -> Option<ObjId> {
        self.active
    }

    /// The control state of the active mover (report, pending correction).
    pub fn control(&self) -> &ActiveControl {
        &self.control
    }

    /// The motion table.
    pub fn motions(&self) -> &MotionTable {
        &self.motions
    }

    /// The simulation switches.
    pub fn flags(&self) -> SimFlags {
        self.flags
    }

    /// The seed of the cosmetic RNG.
    pub fn seed(&self) -> u32 {
        self.seed
    }

    fn mover_mut(&mut self, id: ObjId) -> Option<&mut ClientMover> {
        self.movers.iter_mut().find(|m| m.sim.id == id)
    }

    fn snapshot(&mut self) -> FrameSnapshot {
        self.snap.take().unwrap_or_else(|| self.capture_snapshot())
    }

    /// The start-of-frame lookup: every world mover plus the ctrls (a ctrl can be the destination object of `CMD_SetUseItem`;
    /// without it the walk to a chest or a gate ends at once with `DestObjLost`).
    fn capture_snapshot(&self) -> FrameSnapshot {
        FrameSnapshot::capture(self.movers.iter().map(|m| &m.sim), &self.flags)
            .with_statics(self.dynamic.iter().map(ctrl_target))
    }

    // -------------------------------------------------------------------------------------------
    // commands
    // -------------------------------------------------------------------------------------------

    /// `apply_commands(buffer)`: decodes the whole buffer first (any framing or content error leaves the
    /// world untouched), then applies the records in order, frame-stamped ones now when their frame has
    /// passed and later otherwise. Returns the number of records accepted (applied or queued; unknown ops,
    /// ops of unbuilt subsystems and records for unknown movers are not counted).
    pub fn apply_commands(&mut self, buf: &[u8]) -> Result<u32, SimError> {
        let decoded = commands::decode_buffer(buf)?;

        let mut accepted = 0u32;
        for (_, cmd) in decoded {
            let Some(cmd) = cmd else { continue };
            if self.submit(cmd)? {
                accepted += 1;
            }
        }
        Ok(accepted)
    }

    /// Applies or queues one decoded command; `Ok(false)` when it was ignored.
    pub fn submit(&mut self, cmd: Cmd) -> Result<bool, SimError> {
        if matches!(cmd, Cmd::Unsupported(_)) {
            return Ok(false);
        }
        // the frame of an `ACT` addressed to a preview mover is ignored: it plays at once (Plan/31 §15.1)
        if let Cmd::Act { id, .. } = &cmd
            && self.previews.iter().any(|p| p.sim.id == *id)
        {
            return self.exec(cmd, 0);
        }
        match cmd.server_frame() {
            Some(sf) => {
                let local = i128::from(sf) - i128::from(self.offset);
                let now = i128::from(self.frame);
                if local <= now {
                    let lag = u64::try_from(now - local).unwrap_or(u64::MAX);
                    self.exec(cmd, lag)
                } else {
                    self.queue.push(Queued {
                        server_frame: sf,
                        cmd,
                    });
                    Ok(true)
                }
            }
            None => self.exec(cmd, 0),
        }
    }

    /// Frame-stamped commands whose local frame has arrived.
    fn drain_due(&mut self) {
        if self.queue.is_empty() {
            return;
        }
        let now = i128::from(self.frame);
        let offset = i128::from(self.offset);
        let mut waiting = Vec::new();
        let queued = std::mem::take(&mut self.queue);
        for q in queued {
            let local = i128::from(q.server_frame) - offset;
            if local <= now {
                let lag = u64::try_from(now - local).unwrap_or(u64::MAX);
                // a queued record was validated when it was decoded; a failure here only drops it
                let _ = self.exec(q.cmd, lag);
            } else {
                waiting.push(q);
            }
        }
        // commands queued while draining (none today) keep their place behind the survivors
        waiting.append(&mut self.queue);
        self.queue = waiting;
    }

    /// Executes one command at the current frame with `lag = now - F` frames of catch-up.
    #[allow(clippy::too_many_lines)]
    fn exec(&mut self, cmd: Cmd, lag: u64) -> Result<bool, SimError> {
        let now = self.frame;
        let mut ev = SimEvents::new();
        let mut owner = ObjId::NONE;
        let accepted = match cmd {
            Cmd::Unsupported(_) => false,
            Cmd::SetClock(offset) => {
                self.offset = offset;
                true
            }
            Cmd::AddMover(a) => {
                self.add_mover(a)?;
                true
            }
            Cmd::RemoveMover(id) => {
                let before = self.movers.len() + self.previews.len();
                self.movers.retain(|m| m.sim.id != id);
                self.previews.retain(|p| p.sim.id != id);
                if self.active == Some(id) {
                    self.active = None;
                    self.control = ActiveControl::new();
                }
                self.decor.remove_attached_to(id);
                self.snap = None;
                before != self.movers.len() + self.previews.len()
            }
            Cmd::SetParams { id, words } => {
                let d = wire::params_from_words(&words)?;
                let mut p = d.params;
                p.bounds = self.bounds.union_entries(&d.bounds_entries)?;
                if let Some(m) = self.mover_mut(id) {
                    sync::set_params(&mut m.sim, p);
                    true
                } else if let Some(pm) = self.previews.iter_mut().find(|q| q.sim.id == id) {
                    sync::set_params(&mut pm.sim, p);
                    true
                } else {
                    false
                }
            }
            Cmd::SetActive(id) => {
                let known = self.movers.iter().any(|m| m.sim.id == id);
                for m in &mut self.movers {
                    m.sim.is_active = known && m.sim.id == id;
                }
                self.active = known.then_some(id);
                self.control = ActiveControl::new();
                known
            }
            Cmd::SetVisible {
                id,
                visible,
                want_attach,
            } => {
                if let Some(m) = self.mover_mut(id) {
                    m.visible = visible;
                    m.want_attach = want_attach;
                    true
                } else if let Some(p) = self.previews.iter_mut().find(|q| q.sim.id == id) {
                    p.visible = visible;
                    p.want_attach = want_attach;
                    true
                } else {
                    self.decor.set_ctrl_visible(id, visible)
                }
            }
            Cmd::SetSkeleton { id, skel_id, model } => {
                let key = ModelKey {
                    ot: model.0,
                    index: model.1,
                };
                if let Some(m) = self.mover_mut(id) {
                    m.skel_id = skel_id;
                    m.pose = PoseState::new();
                    m.sim.params.model_key = key;
                    m.sim.motion.id = u32::MAX; // reload the motion for the new model
                    m.sim.motion.clip = None;
                    true
                } else if let Some(p) = self.previews.iter_mut().find(|q| q.sim.id == id) {
                    p.skel_id = skel_id;
                    p.pose = PoseState::new();
                    p.sim.params.model_key = key;
                    p.sim.motion.id = u32::MAX;
                    p.sim.motion.clip = None;
                    true
                } else {
                    false
                }
            }
            Cmd::SetSkill {
                id,
                skill,
                level,
                target,
                skill_count,
                act,
            } => match self.mover_mut(id) {
                Some(m) => {
                    let keep = m.sim.skill;
                    sync::set_skill(
                        &mut m.sim,
                        SkillCtx {
                            skill,
                            level,
                            target,
                            skill_count,
                            act,
                            ..keep
                        },
                    );
                    true
                }
                None => false,
            },
            Cmd::MotionArrive { id, obj_msg } => match self.mover_mut(id) {
                Some(m) => {
                    sync::set_motion_arrive(&mut m.sim, obj_msg);
                    true
                }
                None => false,
            },
            // `OnSetFuel`: an empty tank stops the thrust (`ACC_STOP`), then the turbo fuel is stored
            Cmd::SetFuel { id, fuel, acc_fuel } => {
                self.exec_on_mover(id, &mut ev, &mut owner, |m, env, _, rules, rng, ev| {
                    m.sim.fuel = fuel;
                    if fuel == 0 {
                        crate::send_act_msg(
                            &mut m.sim,
                            env,
                            rules,
                            rng,
                            ActMsg::new(OBJMSG_ACC_STOP),
                            ev,
                        );
                    }
                    m.sim.tm_acc_fuel = acc_fuel;
                })
            }
            Cmd::CtrlAdd(c) => self.decor.ctrl_add(&c, &self.motions, &mut self.dynamic),
            Cmd::CtrlState {
                id,
                anim_frame,
                loop_bits,
                ..
            } => self
                .decor
                .ctrl_state(id, anim_frame, loop_bits, lag, now, &mut self.dynamic),
            Cmd::CtrlRemove(id) => {
                let a = self.dynamic.remove(id);
                let b = self.decor.ctrl_remove(id);
                a || b
            }
            Cmd::DecorAdd(a) => {
                decor::check_add(&a)?;
                self.decor.add(&a, &self.motions)
            }
            Cmd::DecorRemove(id) => self.decor.remove(id),
            Cmd::DecorVisible { id, visible } => self.decor.set_visible(id, visible),
            Cmd::Local(op) => {
                if self.active.is_some() {
                    self.control.queue(op);
                    true
                } else {
                    false
                }
            }
            Cmd::SetTracking(target) => match self.active {
                Some(id) => match self.mover_mut(id) {
                    Some(m) => {
                        if target.is_valid() {
                            m.sim.tracking = target;
                            m.sim.mvrf.add(MVRF_TRACKING);
                        } else {
                            m.sim.tracking = ObjId::NONE;
                            m.sim.mvrf.remove(MVRF_TRACKING);
                        }
                        true
                    }
                    None => false,
                },
                None => false,
            },
            Cmd::ProjectileSpawn(c) => self.spawn_projectile(&c, lag),
            Cmd::ProjectileEnd { id, hit, pos } => self.end_projectile(id, hit, pos),
            Cmd::SetPos { id, pos } => {
                if let Some(p) = self.previews.iter_mut().find(|q| q.sim.id == id) {
                    p.sim.pos = pos;
                    return Ok(true);
                }
                let snap = self.snapshot();
                let mut done = false;
                let ClientSim {
                    flags,
                    world,
                    dynamic,
                    motions,
                    movers,
                    rng,
                    ..
                } = self;
                if let Some(m) = movers.iter_mut().find(|m| m.sim.id == id) {
                    let env = make_env(
                        *flags,
                        world,
                        dynamic,
                        motions,
                        &snap,
                        now,
                        EventMode::Normal,
                    );
                    sync::teleport(&mut m.sim, &env, pos, &mut NoRules, rng, &mut ev);
                    m.hist.clear();
                    m.hist.record(now, m.sim.pos);
                    owner = id;
                    done = true;
                }
                self.snap = Some(snap);
                done
            }
            Cmd::State {
                id, reason: why, s, ..
            } => {
                let k = lag;
                self.exec_on_mover(id, &mut ev, &mut owner, |m, env, cap, rules, rng, ev| {
                    remote::apply_state(
                        &mut m.sim,
                        env,
                        &s,
                        why == reason::CORR,
                        k,
                        cap,
                        &mut m.hist,
                        rules,
                        rng,
                        ev,
                    );
                })
            }
            Cmd::Intent { id, stamp, op, .. } => {
                let k = lag;
                self.exec_on_mover(id, &mut ev, &mut owner, |m, env, _, rules, rng, ev| {
                    remote::apply_intent(
                        &mut m.sim,
                        env,
                        &stamp,
                        op_clone(&op),
                        k,
                        &mut m.hist,
                        rules,
                        rng,
                        ev,
                    );
                })
            }
            Cmd::Keyframe {
                id,
                frame,
                pos,
                angle,
                state,
            } => {
                let local = i128::from(frame) - i128::from(self.offset);
                let local = u64::try_from(local.max(0)).unwrap_or(0);
                self.exec_on_mover(id, &mut ev, &mut owner, |m, env, _, rules, rng, ev| {
                    let kf = Keyframe {
                        frame: local,
                        pos,
                        angle,
                        state,
                    };
                    remote::apply_keyframe(&mut m.sim, env, &kf, &mut m.hist, rules, rng, ev);
                })
            }
            Cmd::PushPower {
                id,
                angle,
                push_angle,
                power,
                ..
            } => {
                let k = lag;
                self.exec_on_mover(id, &mut ev, &mut owner, |m, env, cap, rules, rng, ev| {
                    sync::push_power(&mut m.sim, angle, push_angle, power);
                    let n = u32::try_from(k.min(u64::from(cap))).unwrap_or(cap);
                    remote::catch_up(&mut m.sim, env, n, &mut m.hist, rules, rng, ev);
                })
            }
            Cmd::Die {
                id,
                cause_msg,
                attacker,
                ..
            } => {
                let k = lag;
                let attacker_angle = self.mover(attacker).map(|a| a.sim.angle);
                self.exec_on_mover(id, &mut ev, &mut owner, |m, env, cap, rules, rng, ev| {
                    let at = env.frame.0.saturating_sub(k.min(u64::from(cap)));
                    let n = u32::try_from(k.min(u64::from(cap))).unwrap_or(cap);
                    // a dying mover has no hit points left: the knock-back ends in `DEAD`, never `DMG_LIVE`
                    m.sim.params.hp_positive = false;
                    {
                        let e = SimEnv {
                            frame: SimFrame(at),
                            role: env.role,
                            flags: env.flags,
                            world: env.world,
                            dynamic: env.dynamic,
                            motions: env.motions,
                            lookup: env.lookup,
                            mode: env.mode,
                        };
                        let mut cx = crate::step::Cx::new(&e, rules, rng, ev);
                        crate::act::damage::die(
                            &mut m.sim,
                            &mut cx,
                            cause_msg,
                            attacker_angle.map(|a| (attacker, a)),
                        );
                    }
                    remote::catch_up(&mut m.sim, env, n, &mut m.hist, rules, rng, ev);
                })
            }
            Cmd::DamageFx {
                defender,
                atk_flags,
                attacker,
                tail,
                attacker_angle: exact_angle,
                ..
            } => {
                let k = lag;
                let attacker_angle =
                    exact_angle.or_else(|| self.mover(attacker).map(|a| a.sim.angle));
                self.exec_on_mover(
                    defender,
                    &mut ev,
                    &mut owner,
                    |m, env, cap, rules, rng, ev| {
                        let n = u32::try_from(k.min(u64::from(cap))).unwrap_or(cap);
                        let at = env.frame.0.saturating_sub(u64::from(n));
                        if let Some((pos, angle)) = tail {
                            m.sim.pos = pos;
                            m.sim.angle = crate::num::set_angle_quantized(angle);
                        }
                        {
                            let e = SimEnv {
                                frame: SimFrame(at),
                                role: env.role,
                                flags: env.flags,
                                world: env.world,
                                dynamic: env.dynamic,
                                motions: env.motions,
                                lookup: env.lookup,
                                mode: env.mode,
                            };
                            let mut cx = crate::step::Cx::new(&e, rules, rng, ev);
                            crate::act::damage::set_damaged_motion(
                                &mut m.sim,
                                &mut cx,
                                atk_flags,
                                attacker_angle.map(|a| (attacker, a)),
                            );
                        }
                        remote::catch_up(&mut m.sim, env, n, &mut m.hist, rules, rng, ev);
                    },
                )
            }
            Cmd::Act {
                id,
                msg,
                queue_if_busy,
                ..
            } => {
                if self.previews.iter().any(|p| p.sim.id == id) {
                    return self.preview_act(id, &msg, queue_if_busy);
                }
                let k = lag;
                self.exec_on_mover(id, &mut ev, &mut owner, |m, env, cap, rules, rng, ev| {
                    remote::apply_act(
                        &mut m.sim,
                        env,
                        msg,
                        queue_if_busy,
                        k,
                        cap,
                        &mut m.hist,
                        rules,
                        rng,
                        ev,
                    );
                })
            }
        };
        self.export_events(owner, &mut ev, now);
        Ok(accepted)
    }

    // -------------------------------------------------------------------------------------------
    // projectiles (Plan/31 §14, §17.4 PROJECTILE_*, WP-SIM-22)
    // -------------------------------------------------------------------------------------------

    /// `PROJECTILE_SPAWN`: the projectile stands at its origin at the start of its server frame; a late
    /// record is caught up by `min(lag, PROJECTILE_FF_CAP)` steps against the current positions (silent: a
    /// projectile that ends during the catch-up only raises its end event). An existing id is replaced.
    fn spawn_projectile(&mut self, c: &ProjectileSpawn, lag: u64) -> bool {
        self.projectiles.retain(|q| q.p.id != c.id);
        if self.projectiles.len() >= MAX_PROJECTILES {
            return false;
        }
        let shooter = self
            .mover(c.attacker)
            .map(|m| (m.sim.pos, m.sim.angle, m.sim.angle_x));
        let mut args = SpawnArgs::new(c.kind, c.origin, c.dest, c.target);
        args.id = c.id;
        args.source = c.attacker;
        args.speed = c.speed;
        if c.kind == ProjectileKind::WandAtkAir
            && let Some((_, yaw, pitch)) = shooter
        {
            args.launch = Some(Launch { yaw, pitch });
        }
        let mut p = Projectile::spawn(&args);
        if c.kind == ProjectileKind::YoyoAtk {
            p.make_yoyo_path(shooter.map_or(c.origin, |(pos, _, _)| pos));
        }
        let snap = self.snapshot();
        let steps = u32::try_from(lag)
            .unwrap_or(u32::MAX)
            .min(PROJECTILE_FF_CAP);
        let mut ended = None;
        for _ in 0..steps {
            let r = p.step_in(&snap);
            if r != ProjStep::Flying {
                ended = Some(r);
                break;
            }
        }
        self.snap = Some(snap);
        if let Some(r) = ended {
            self.export_projectile_end(p.id, r);
            return true;
        }
        self.projectiles
            .push(ClientProjectile { prev_pos: p.pos, p });
        true
    }

    /// `PROJECTILE_END`: the server ended the projectile; a projectile that already ended here is ignored.
    fn end_projectile(&mut self, id: u32, hit: bool, pos: Vec3) -> bool {
        let before = self.projectiles.len();
        let live = self.projectiles.iter().any(|q| q.p.id == id && !q.p.done);
        self.projectiles.retain(|q| q.p.id != id);
        if !live {
            return before != self.projectiles.len();
        }
        let r = if hit {
            ProjStep::Hit { pos }
        } else {
            ProjStep::Expired
        };
        self.export_projectile_end(id, r);
        true
    }

    fn export_projectile_end(&mut self, id: u32, r: ProjStep) {
        let e = match r {
            ProjStep::Hit { pos } => SimEvent::ProjectileHit { id, pos },
            ProjStep::Expired => SimEvent::ProjectileExpired { id },
            ProjStep::Flying => return,
        };
        if let Some(x) = export(ObjId::NONE, &e, self.frame) {
            self.out.push(x);
        }
    }

    /// Step 7 of Plan/31 §3.5: every projectile moves one frame against the positions at the END of the
    /// frame just stepped. Projectiles that ended in the previous call are dropped first (they stay in the
    /// render view for one frame, flagged `HIT`/`EXPIRED`).
    fn step_projectiles(&mut self) {
        self.projectiles.retain(|q| !q.p.done);
        if self.projectiles.is_empty() {
            return;
        }
        let snap = self.capture_snapshot();
        let mut ended: Vec<(u32, ProjStep)> = Vec::new();
        for q in &mut self.projectiles {
            q.prev_pos = q.p.pos;
            let r = q.p.step_in(&snap);
            if r != ProjStep::Flying {
                ended.push((q.p.id, r));
            }
        }
        for (id, r) in ended {
            self.export_projectile_end(id, r);
        }
    }

    /// Runs `f` on world mover `id` with the environment of the current frame (Normal mode), the rules and the
    /// RNG; `owner` records which mover's events `ev` holds.
    fn exec_on_mover(
        &mut self,
        id: ObjId,
        ev: &mut SimEvents,
        owner: &mut ObjId,
        f: impl FnOnce(
            &mut ClientMover,
            &SimEnv<'_>,
            u32,
            &mut dyn SimRules,
            &mut Pcg32Rng,
            &mut SimEvents,
        ),
    ) -> bool {
        let snap = self.snapshot();
        let now = self.frame;
        let cap = self.extrap_cap;
        let ClientSim {
            flags,
            world,
            dynamic,
            motions,
            movers,
            rng,
            ..
        } = self;
        let mut done = false;
        if let Some(m) = movers.iter_mut().find(|m| m.sim.id == id) {
            let env = make_env(
                *flags,
                world,
                dynamic,
                motions,
                &snap,
                now,
                EventMode::Normal,
            );
            f(m, &env, cap, &mut NoRules, rng, ev);
            *owner = id;
            done = true;
        }
        self.snap = Some(snap);
        done
    }

    /// Moves the events of `owner` into the export buffer.
    fn export_events(&mut self, owner: ObjId, ev: &mut SimEvents, frame: u64) {
        for e in ev.drain() {
            if let Some(x) = export(owner, &e, frame) {
                self.out.push(x);
            }
        }
    }

    fn add_mover(&mut self, a: AddMover) -> Result<(), SimError> {
        // an existing id is replaced (a re-spawn after a lost `DelObj`)
        self.movers.retain(|m| m.sim.id != a.id);
        self.previews.retain(|p| p.sim.id != a.id);
        if self.active == Some(a.id) {
            self.active = None;
            self.control = ActiveControl::new();
        }
        self.snap = None;
        let model = ModelKey {
            ot: a.model.0,
            index: a.model.1,
        };
        if a.kind == wire::KIND_PREVIEW {
            let kind = if matches!(a.mover_index, defs::MI_MALE | defs::MI_FEMALE) {
                MoverKind::Player
            } else {
                MoverKind::Monster
            };
            let mut p = SimMoverParams::default_for(kind);
            p.mover_index = a.mover_index;
            p.model_key = model;
            p.scale = a.scale;
            let sim = SimMover::new(a.id, a.pos, a.angle, p);
            self.previews.push(PreviewMover::new(sim, a.skel_id));
            return Ok(());
        }
        let kind = match a.kind {
            wire::KIND_PLAYER => MoverKind::Player,
            wire::KIND_MONSTER => MoverKind::Monster,
            wire::KIND_NPC => MoverKind::Npc,
            _ => MoverKind::Pet,
        };
        let mut p = SimMoverParams::default_for(kind);
        p.mover_index = a.mover_index;
        p.model_key = model;
        p.scale = a.scale;
        let mut sim = SimMover::new(a.id, a.pos, a.angle, p);
        sim.angle_x = a.angle_x;
        sim.render.scr_angle_x = a.angle_x;
        let mut hist = PosHistory::new();
        hist.record(self.frame, a.pos);
        let mut mover = ClientMover {
            sim,
            pose: PoseState::new(),
            hist,
            skel_id: a.skel_id,
            visible: false,
            want_attach: false,
            events: SimEvents::new(),
        };
        if a.state != 0 {
            // a flyer joins in flight: the FLY flag is the spawn's, not a transition (`MODE_FLY` is the
            // equip path); without it the forced set would refuse the flight state (Plan/31 §6.5)
            if a.state_flags & OBJSTAF_FLY != 0 {
                mover.sim.act.flags.add(OBJSTAF_FLY);
            }
            let su = sync::StateUpdate {
                state: ObjState(a.state),
                flags: StateFlags(a.state_flags),
                motion: a.motion,
                ..sync::StateUpdate::standing(a.pos, a.angle)
            };
            let snap = self.snapshot();
            let env = make_env(
                self.flags,
                &self.world,
                &self.dynamic,
                &self.motions,
                &snap,
                self.frame,
                EventMode::Silent,
            );
            let mut ev = SimEvents::new();
            sync::apply_state(
                &mut mover.sim,
                &env,
                &su,
                &mut NoRules,
                &mut self.rng,
                &mut ev,
            );
            self.snap = Some(snap);
        }
        if a.is_active {
            mover.sim.is_active = true;
            for m in &mut self.movers {
                m.sim.is_active = false;
            }
            self.active = Some(a.id);
            self.control = ActiveControl::new();
        }
        self.movers.push(mover);
        self.snap = None;
        Ok(())
    }

    // -------------------------------------------------------------------------------------------
    // local control (Plan/31 §15.2, §17.5)
    // -------------------------------------------------------------------------------------------

    /// `local_input([buttons, mouse_dx, mouse_dy, level, req_flags])`: applies to the next stepped frame(s).
    pub fn local_input(&mut self, rec: &[i32]) {
        self.control.latch.update(rec);
    }

    /// `local_act(msg, p1..p5)`: `SendActMsg` on the active mover with its synchronous result (`0` without an
    /// active mover). The events it raises are exported.
    pub fn local_act(&mut self, msg: u32, p: [i32; 5]) -> i32 {
        let Some(id) = self.active else { return 0 };

        let snap = self.snapshot();
        let now = self.frame;
        let mut ev = SimEvents::new();
        let r = {
            let ClientSim {
                flags,
                world,
                dynamic,
                motions,
                movers,
                rng,
                ..
            } = self;
            match movers.iter_mut().find(|m| m.sim.id == id) {
                Some(m) => {
                    let env = make_env(
                        *flags,
                        world,
                        dynamic,
                        motions,
                        &snap,
                        now,
                        EventMode::Normal,
                    );
                    crate::send_act_msg(
                        &mut m.sim,
                        &env,
                        &mut NoRules,
                        rng,
                        ActMsg::with(msg, p),
                        &mut ev,
                    )
                }
                None => 0,
            }
        };
        self.snap = Some(snap);
        self.export_events(id, &mut ev, now);
        r
    }

    // -------------------------------------------------------------------------------------------
    // stepping (Plan/31 §3.5)
    // -------------------------------------------------------------------------------------------

    /// `step(frames)`: runs up to [`MAX_STEPS_PER_CALL`] frames and returns how many ran. The report area is
    /// cleared first and accumulates over the call (§15.3); works without a world (only previews run).
    pub fn step(&mut self, frames: u32) -> u32 {
        let n = frames.min(MAX_STEPS_PER_CALL);
        self.control.begin_call();
        for _ in 0..n {
            self.step_one();
        }
        n
    }

    fn step_one(&mut self) {
        let f = self.frame;
        // 1. commands whose local frame has arrived
        self.drain_due();
        // 2. control of the active mover
        let mut own_events = SimEvents::new();
        if let Some(id) = self.active {
            let snap = self.snapshot();
            let ClientSim {
                flags,
                world,
                dynamic,
                motions,
                movers,
                rng,
                control,
                ..
            } = self;
            if let Some(m) = movers.iter_mut().find(|m| m.sim.id == id) {
                let env = make_env(*flags, world, dynamic, motions, &snap, f, EventMode::Normal);
                let mut rules = NoRules;
                let mut cx = crate::step::Cx::new(&env, &mut rules, rng, &mut own_events);
                control.control_frame(&mut m.sim, &mut cx);
            }
            self.snap = Some(snap);
        }
        // 3. the start-of-frame snapshot (preview movers are not in it)
        let snap = self.capture_snapshot();
        // 3b. the local system pets look at their owners (`CAIEgg`, WP-CLIENT-31): a pass of its own before any mover steps, so that
        // the result does not depend on the insertion order
        local_pet::drive(&mut self.movers, &self.world);
        // 4. every world mover steps the frame (insertion order), previews advance their clocks
        {
            let ClientSim {
                flags,
                world,
                dynamic,
                motions,
                movers,
                previews,
                rng,
                ..
            } = self;
            let env = make_env(*flags, world, dynamic, motions, &snap, f, EventMode::Normal);
            for m in movers.iter_mut() {
                m.hist.record(f, m.sim.pos);
                step_frame(&mut m.sim, &env, &mut NoRules, rng, &mut m.events);
            }
            for p in previews.iter_mut() {
                p.step(&env, rng);
            }
        }
        // 5. the frame barrier
        self.barrier(f, &snap, own_events);
        // 6. render smoothing
        for m in &mut self.movers {
            remote::smooth(&mut m.sim);
        }
        // 7. projectiles (against the end-of-frame positions), the decor clocks and the ctrl colliders, then
        // the next frame
        self.step_projectiles();
        self.step_decor(f);
        self.snap = None;
        self.frame += 1;
    }

    /// Step 7 of Plan/31 §3.5 for decor and ctrls: one frame of every clock, the sounds, the collider poses.
    fn step_decor(&mut self, f: u64) {
        if self.decor.is_empty() {
            return;
        }
        let mut sounds = Vec::new();
        self.decor
            .step(f, &self.motions, &mut self.dynamic, &mut sounds);
        for e in sounds {
            self.out.push(e);
        }
    }

    /// The frame barrier of frame `f` (Plan/31 §3.5 step 5): the active mover's collision/arrival responses,
    /// the queue drains, and the export of the frame's events.
    fn barrier(&mut self, f: u64, snap: &FrameSnapshot, own_events: SimEvents) {
        let active = self.active;
        // control events of the start of the frame first (Notice, DestPosChosen, CastCancel)
        let mut own = own_events;
        if let Some(id) = active {
            self.export_events(id, &mut own, f);
        }
        for i in 0..self.movers.len() {
            let (id, events): (ObjId, Vec<SimEvent>) = match self.movers.get_mut(i) {
                Some(m) => (m.sim.id, m.events.drain().collect()),
                None => continue,
            };
            let mut drain_magic = false;
            let mut extra: Vec<SimEvent> = Vec::new();
            let mut scratch = SimEvents::new();
            let attacker_is_player = self.movers.get(i).is_some_and(|m| m.sim.is_player());
            let mut victims_unpaused: Vec<ObjId> = Vec::new();
            if active == Some(id) {
                let ClientSim {
                    flags,
                    world,
                    dynamic,
                    motions,
                    movers,
                    rng,
                    control,
                    ..
                } = self;
                if let Some(m) = movers.get_mut(i) {
                    let env = make_env(*flags, world, dynamic, motions, snap, f, EventMode::Normal);
                    let mut rules = NoRules;
                    let mut cx = crate::step::Cx::new(&env, &mut rules, rng, &mut scratch);
                    control.handle_events(&mut m.sim, &mut cx, &events, &mut extra);
                }
            }
            for e in &events {
                // `OnAttackMelee` (client): a monster's melee hit ends the hit-stop of its victim
                // (`pHitObj->m_pModel->m_nPause = 0`, `MoverActEvent.cpp:1747-1752`, applied as below); the
                // monster itself never pauses
                if let SimEvent::AttrHit {
                    kind: HitKind::Melee,
                    target,
                    ..
                } = e
                    && !attacker_is_player
                {
                    victims_unpaused.push(*target);
                }
                match e {
                    SimEvent::DrainMagicQueue => drain_magic = true,
                    // answered by the active mover's control (or internal)
                    SimEvent::Collided | SimEvent::Dropped => {}
                    SimEvent::CmdArrived { .. } if active == Some(id) => {}
                    other => {
                        if let Some(x) = export(id, other, f) {
                            self.out.push(x);
                        }
                    }
                }
            }
            for v in victims_unpaused {
                if let Some(m) = self.mover_mut(v) {
                    // FIX: the original sets the pause to 0, but the frozen clock still holds the window of
                    // the hit attribute, so the victim's swing would hit a second time on its next frame. The
                    // pause is cut to its last frame instead: the next frame decrements it and advances the
                    // clock past the window, exactly like a pause that ran out.
                    m.sim.motion.clock.pause = m.sim.motion.clock.pause.min(1);
                }
            }
            if drain_magic {
                let ClientSim {
                    flags,
                    world,
                    dynamic,
                    motions,
                    movers,
                    rng,
                    ..
                } = self;
                if let Some(m) = movers.get_mut(i) {
                    let env = make_env(*flags, world, dynamic, motions, snap, f, EventMode::Normal);
                    let mut rules = NoRules;
                    let mut cx = crate::step::Cx::new(&env, &mut rules, rng, &mut scratch);
                    remote::drain_magic_queue(&mut m.sim, &mut cx);
                }
            }
            for e in scratch.drain().chain(extra) {
                if let Some(x) = export(id, &e, f) {
                    self.out.push(x);
                }
            }
        }
    }

    /// `take_events()`: the event words (`[count, dropped, records...]`), valid until the next call; the
    /// queue is emptied.
    pub fn take_events(&mut self) -> &[u32] {
        self.out.take()
    }

    /// Number of events waiting to be taken.
    pub fn pending_events(&self) -> usize {
        self.out.pending()
    }

    /// `GetRadius()` of mover `id` (diagnostics).
    pub fn radius_of(&self, id: ObjId) -> Option<f32> {
        self.mover(id)
            .map(|m| radius(&m.sim.params.bounds, m.sim.params.scale))
    }
}

/// `IntentOp` is `Copy`; a named helper keeps the closure above readable.
fn op_clone(op: &IntentOp) -> IntentOp {
    *op
}
