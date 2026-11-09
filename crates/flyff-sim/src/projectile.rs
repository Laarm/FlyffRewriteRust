//! Shared projectile kinematics (Plan/31 §14, Plan/17 §4.3, ref/20 §7.4; PL-015, WP-SIM-22).
//!
//! The server simulates every projectile per 1/60 s frame from the attacker's hit frame, the browser
//! renders the same flight from `ProjectileSpawn` with THIS step function, so both sides agree frame for
//! frame (native and wasm32 bit for bit: `f32` arithmetic in the order of `src:_Common/Sfx.cpp`, `libm`
//! trigonometry). Which kind an attack uses, the spawn origin, the speed (`features.faster_projectiles`) and
//! the damage delay are doc 17's inputs; this module owns what happens between the spawn and the contact.
//!
//! Frame protocol: a projectile spawned with `server_frame = S` stands at its origin at the START of frame
//! `S`; [`projectile_step`] is called once per frame after the movers of that frame have stepped (the
//! target position is the one of the END of frame `S`), so the first move happens in frame `S`
//! (Plan/17 §4.1: "spawned at the hit frame `F` => first step at `F + 1`", `S = F + 1`).
//!
//! | kind | destination (re-read every frame while the target exists) | move | contact |
//! |---|---|---|---|
//! | `Shoot` (`CSfxShoot`) | target `+ (0, 1, 0)` | homing, `speed` | after the move `\|p - d\|^2 < 0.2^2` |
//! | `WandAtk1` | target `+ (0, \|bbox.max.y\| / 2 * scale, 0)` | homing | `< r^2`, `r = ((\|dx\| + \|dz\|) / 2 * scale) / 3` of the target box, fixed at the first step |
//! | `WandAtkAir` | target `+ (0, 1, 0)` | inertial (see `step_wand_air`) | distance BEFORE the move `< R^2`, `R = 3.0` vs NPCs, `0.6` vs players |
//! | `RangeAtk1`, `RangeAtkJunkbow` | target `+ (0, 1, 0)` | homing | `< 0.4^2` |
//! | `MagStrongWind` | target | homing | `\|p - d\| < 0.2` |
//! | `MagSwordWind` | target | homing | `\|p - d\| < 0.4` |
//! | `MagFireBoomerang`, `MagIceMissile`, `MagLightningBall`, `MagSpikeStone` | target `+ (0, 0.3, 0)` | homing | `\|p - d\| < 0.2` |
//! | `ShootWave` | target (the projectile's `y` is forced to the target's) | homing | `< 0.4^2` |
//! | `YoyoAtk` | spline path, then back to the hand | see [`Yoyo`] | reaches the hand |
//! | `AtkStraight` | fixed | `pos += vel` | `IsRangeObj(dest, 0)` |
//! | `MagHotAir` | none | none | the first step returns `Hit` |

use crate::bounds::is_range_point;
use crate::num::{
    Vec3, acosf, angle_to_vector, atan2f, cosf, cross, dot, length, length_sq, normalize_or_zero,
    sinf, to_degree, x_get_degree,
};
use crate::pose::quat::{Mat44, Quat, mat_from_quat, transform_coord};
use crate::step::{MoverLookup, MoverSnapshot};
use flyff_common::ObjId;

/// Frames of the generic safety timeout (`SEC1 * 10`, PL-015: the legacy server discarded unpaired hit
/// information after 10 s; `CSfxItemWandAtk1` and the arrows have the same literal).
pub const TIMEOUT_FRAMES: u32 = 600;
/// `CSfxItemWandAtkAir`: `m_nFrame >= 60 * 8` explodes without damage.
pub const WAND_AIR_TIMEOUT_FRAMES: u32 = 480;
/// `CSfxAtkStraight`: `m_nFrame > 60 * 3`.
pub const ATK_STRAIGHT_TIMEOUT_FRAMES: u32 = 180;
/// `CSfxItemYoyoAtk`: `m_nFrame >= SEC1 * 5`.
pub const YOYO_TIMEOUT_FRAMES: u32 = 300;
/// Multi-shot spacing: one shot every 4th global frame (`(g_nProcessCnt & 3) == 0`, PL-015).
pub const MULTI_SHOT_PERIOD: u64 = 4;
/// Coordinates beyond this are clamped at spawn (the biggest world is below 10^5 units): keeps every squared
/// distance finite whatever a hostile record says.
pub const COORD_LIMIT: f32 = 1.0e6;
/// The largest per-frame speed accepted at spawn (the real ones are at most 2.0).
pub const MAX_SPEED: f32 = 64.0;
/// Speed of `WAND_ATK_AIR`'s launch impulse (`ShootSfx(angle, -angleX, 0.75)`).
pub const WAND_AIR_LAUNCH_SPEED: f32 = 0.75;

/// The behaviour class of a projectile (`ProjectileKind` of Plan/11 §8.3; the numbers are wire values).
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum ProjectileKind {
    /// `CSfxShoot` (generic homing shot).
    Shoot = 1,
    /// `CSfxItemWandAtk1` (wand bolt).
    WandAtk1 = 2,
    /// `CSfxItemWandAtkAir` (wand bolt of a flying attacker, inertial).
    WandAtkAir = 3,
    /// `CSfxItemRangeAtk1` (arrow).
    RangeAtk1 = 4,
    /// `CSfxItemRangeAtk_JunkBow`.
    RangeAtkJunkbow = 5,
    /// `CSfxSkillMagStrongWind`.
    MagStrongWind = 6,
    /// `CSfxSkillMagSwordWind`.
    MagSwordWind = 7,
    /// `CSfxSkillMagFireBoomerang`.
    MagFireBoomerang = 8,
    /// `CSfxSkillMagIceMissile`.
    MagIceMissile = 9,
    /// `CSfxSkillMagLightningBall`.
    MagLightningBall = 10,
    /// `CSfxSkillMagSpikeStone`.
    MagSpikeStone = 11,
    /// `CSfxShootWave`.
    ShootWave = 12,
    /// `CSfxItemYoyoAtk` (client visual).
    YoyoAtk = 13,
    /// `CSfxAtkStraight` (fixed destination).
    AtkStraight = 14,
    /// `CSfxSkillMagHotAir` (no flight).
    MagHotAir = 15,
}

impl ProjectileKind {
    /// Every kind in wire order.
    pub const ALL: [ProjectileKind; 15] = [
        ProjectileKind::Shoot,
        ProjectileKind::WandAtk1,
        ProjectileKind::WandAtkAir,
        ProjectileKind::RangeAtk1,
        ProjectileKind::RangeAtkJunkbow,
        ProjectileKind::MagStrongWind,
        ProjectileKind::MagSwordWind,
        ProjectileKind::MagFireBoomerang,
        ProjectileKind::MagIceMissile,
        ProjectileKind::MagLightningBall,
        ProjectileKind::MagSpikeStone,
        ProjectileKind::ShootWave,
        ProjectileKind::YoyoAtk,
        ProjectileKind::AtkStraight,
        ProjectileKind::MagHotAir,
    ];

    /// The wire value.
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    /// The kind of a wire value (`0` and unknown values are `None`).
    pub const fn from_u8(v: u8) -> Option<ProjectileKind> {
        Some(match v {
            1 => ProjectileKind::Shoot,
            2 => ProjectileKind::WandAtk1,
            3 => ProjectileKind::WandAtkAir,
            4 => ProjectileKind::RangeAtk1,
            5 => ProjectileKind::RangeAtkJunkbow,
            6 => ProjectileKind::MagStrongWind,
            7 => ProjectileKind::MagSwordWind,
            8 => ProjectileKind::MagFireBoomerang,
            9 => ProjectileKind::MagIceMissile,
            10 => ProjectileKind::MagLightningBall,
            11 => ProjectileKind::MagSpikeStone,
            12 => ProjectileKind::ShootWave,
            13 => ProjectileKind::YoyoAtk,
            14 => ProjectileKind::AtkStraight,
            15 => ProjectileKind::MagHotAir,
            _ => return None,
        })
    }

    /// The `PROJECTILE_KIND_*` name without the prefix (vectors and diagnostics).
    pub const fn name(self) -> &'static str {
        match self {
            ProjectileKind::Shoot => "SHOOT",
            ProjectileKind::WandAtk1 => "WAND_ATK1",
            ProjectileKind::WandAtkAir => "WAND_ATK_AIR",
            ProjectileKind::RangeAtk1 => "RANGE_ATK1",
            ProjectileKind::RangeAtkJunkbow => "RANGE_ATK_JUNKBOW",
            ProjectileKind::MagStrongWind => "MAG_STRONG_WIND",
            ProjectileKind::MagSwordWind => "MAG_SWORD_WIND",
            ProjectileKind::MagFireBoomerang => "MAG_FIRE_BOOMERANG",
            ProjectileKind::MagIceMissile => "MAG_ICE_MISSILE",
            ProjectileKind::MagLightningBall => "MAG_LIGHTNING_BALL",
            ProjectileKind::MagSpikeStone => "MAG_SPIKE_STONE",
            ProjectileKind::ShootWave => "SHOOT_WAVE",
            ProjectileKind::YoyoAtk => "YOYO_ATK",
            ProjectileKind::AtkStraight => "ATK_STRAIGHT",
            ProjectileKind::MagHotAir => "MAG_HOT_AIR",
        }
    }

    /// The kind named `name` (as [`ProjectileKind::name`]).
    pub fn from_name(name: &str) -> Option<ProjectileKind> {
        ProjectileKind::ALL
            .iter()
            .copied()
            .find(|k| k.name() == name)
    }

    /// The static description of the kind.
    pub const fn info(self) -> KindInfo {
        let (faster, legacy, contact, max_age, delay) = match self {
            ProjectileKind::Shoot => (0.4, 0.2, 0.2, TIMEOUT_FRAMES, 0),
            ProjectileKind::WandAtk1 => (0.4, 0.2, 0.0, TIMEOUT_FRAMES, 1),
            ProjectileKind::WandAtkAir => (
                WAND_AIR_LAUNCH_SPEED,
                WAND_AIR_LAUNCH_SPEED,
                0.6,
                WAND_AIR_TIMEOUT_FRAMES,
                0,
            ),
            ProjectileKind::RangeAtk1 | ProjectileKind::RangeAtkJunkbow => {
                (0.7, 0.7, 0.4, TIMEOUT_FRAMES, 0)
            }
            ProjectileKind::MagStrongWind => (0.4, 0.2, 0.2, TIMEOUT_FRAMES, 1),
            ProjectileKind::MagSwordWind => (0.4, 0.2, 0.4, TIMEOUT_FRAMES, 1),
            ProjectileKind::MagFireBoomerang
            | ProjectileKind::MagIceMissile
            | ProjectileKind::MagLightningBall
            | ProjectileKind::MagSpikeStone => (0.4, 0.2, 0.2, TIMEOUT_FRAMES, 1),
            ProjectileKind::ShootWave => (0.45, 0.25, 0.4, TIMEOUT_FRAMES, 0),
            ProjectileKind::YoyoAtk => (0.3, 0.3, 0.0, YOYO_TIMEOUT_FRAMES, 0),
            ProjectileKind::AtkStraight => (2.0, 2.0, 0.0, ATK_STRAIGHT_TIMEOUT_FRAMES, 0),
            ProjectileKind::MagHotAir => (0.0, 0.0, 0.0, 0, 0),
        };
        KindInfo {
            kind: self,
            name: self.name(),
            speed_faster: faster,
            speed_legacy: legacy,
            contact_radius: contact,
            max_age,
            damage_delay: delay,
            dest_offset_y: match self {
                ProjectileKind::Shoot
                | ProjectileKind::WandAtkAir
                | ProjectileKind::RangeAtk1
                | ProjectileKind::RangeAtkJunkbow => 1.0,
                ProjectileKind::MagFireBoomerang
                | ProjectileKind::MagIceMissile
                | ProjectileKind::MagLightningBall
                | ProjectileKind::MagSpikeStone => 0.3,
                _ => 0.0,
            },
        }
    }

    /// The per-frame speed of the kind: the `features.faster_projectiles` value when `faster`, else the
    /// legacy one (the value that travels in `ProjectileSpawn.speed`; Plan/17 §4.3).
    pub const fn speed(self, faster: bool) -> f32 {
        let i = self.info();
        if faster {
            i.speed_faster
        } else {
            i.speed_legacy
        }
    }

    /// `true` when the kind homes on a target (re-reads the destination every frame).
    pub const fn homes(self) -> bool {
        !matches!(
            self,
            ProjectileKind::AtkStraight | ProjectileKind::MagHotAir | ProjectileKind::YoyoAtk
        )
    }
}

/// The data row of a kind (the cross-check source of doc 17's `projectile_kind.rs`, WP-COMBAT-07).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct KindInfo {
    /// The kind.
    pub kind: ProjectileKind,
    /// `PROJECTILE_KIND_*` name without the prefix.
    pub name: &'static str,
    /// Speed per frame with `features.faster_projectiles` (`__FASTER_PROJECTILES`).
    pub speed_faster: f32,
    /// Speed per frame without it.
    pub speed_legacy: f32,
    /// The fixed contact radius (`0` where it is computed: `WandAtk1` from the target box, `AtkStraight`
    /// from the model radius, the yo-yo by its path).
    pub contact_radius: f32,
    /// The timeout in frames (`0` = no flight).
    pub max_age: u32,
    /// Frames between contact and damage in the legacy classes (the hit sfx plays first; Plan/17 §4.3).
    /// Informational: doc 17 schedules the damage.
    pub damage_delay: u32,
    /// The constant `y` offset added to the target position for the destination (`WandAtk1` uses the
    /// target box instead, `ShootWave` and the winds none).
    pub dest_offset_y: f32,
}

/// What one [`projectile_step`] did.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ProjStep {
    /// Still in flight.
    Flying,
    /// Contact at `pos` (the projectile is done; doc 17 applies the damage after the kind's delay).
    Hit {
        /// The position of the projectile at the contact.
        pos: Vec3,
    },
    /// Removed without contact: timeout, or the target/source vanished (no damage).
    Expired,
}

/// The launch orientation of an inertial or straight projectile: the attacker's yaw and pitch
/// (`angle_x`, negative = nose up), i.e. `AngleToVector(yaw, -pitch, speed)`.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Launch {
    /// Yaw in degrees.
    pub yaw: f32,
    /// Pitch in degrees (the mover's `angle_x`).
    pub pitch: f32,
}

/// Everything [`Projectile::spawn`] needs.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct SpawnArgs {
    /// The projectile id (`ProjectileSpawn.projectile_id`).
    pub id: u32,
    /// The behaviour class.
    pub kind: ProjectileKind,
    /// The position at the start of the spawn frame.
    pub origin: Vec3,
    /// The initial destination (target `+ offset` or the fixed point).
    pub dest: Vec3,
    /// The homing target (`ObjId::NONE` for fixed destinations).
    pub target: ObjId,
    /// The shooter (the yo-yo returns to its hand; `ObjId::NONE` for the other kinds).
    pub source: ObjId,
    /// Per-frame speed (`<= 0` or not finite: the `faster` table value of the kind).
    pub speed: f32,
    /// The launch orientation (`WandAtkAir`, `AtkStraight`); `None` aims from `origin` at `dest`.
    pub launch: Option<Launch>,
    /// The radius of the projectile's own model (`AtkStraight`'s `IsRangeObj`; `0` when unknown).
    pub model_radius: f32,
}

impl SpawnArgs {
    /// Arguments for a homing kind with the table speed.
    pub fn new(kind: ProjectileKind, origin: Vec3, dest: Vec3, target: ObjId) -> SpawnArgs {
        SpawnArgs {
            id: 0,
            kind,
            origin,
            dest,
            target,
            source: ObjId::NONE,
            speed: 0.0,
            launch: None,
            model_radius: 0.0,
        }
    }
}

/// The yo-yo flight state (`CSfxItemYoyoAtk`): the pre-computed spline, the phase and the pause.
#[derive(Clone, PartialEq, Debug)]
pub struct Yoyo {
    /// The world-space spline points (`m_aSpline`, 24 of them).
    pub path: Vec<Vec3>,
    /// `m_nStep`: 0 flying to the path start, 1 along the path, 2 back to the hand.
    pub step: u8,
    /// `m_nCount`: the next path point.
    pub count: u32,
    /// `m_nDelayCount`: frames to wait before the path phase.
    pub delay: u32,
}

/// One live projectile.
#[derive(Clone, PartialEq, Debug)]
pub struct Projectile {
    /// `projectile_id`.
    pub id: u32,
    /// The behaviour class.
    pub kind: ProjectileKind,
    /// Position.
    pub pos: Vec3,
    /// Velocity (`m_vDelta`; the inertial and straight kinds).
    pub vel: Vec3,
    /// The destination (re-read from the target every frame while it exists).
    pub dest: Vec3,
    /// The homing target.
    pub target: ObjId,
    /// The shooter.
    pub source: ObjId,
    /// Per-frame speed.
    pub speed: f32,
    /// The contact radius currently in use (`WandAtk1`: computed at the first step; `AtkStraight`: the
    /// model radius).
    pub radius: f32,
    /// Frames stepped.
    pub age: u32,
    /// Timeout in frames.
    pub max_age: u32,
    /// The projectile has ended ([`Projectile::hit`] says how).
    pub done: bool,
    /// The projectile ended with a contact.
    pub hit: bool,
    /// The unit direction of the last move (render orientation; zero before the first move).
    pub dir: Vec3,
    /// The yo-yo state.
    pub yoyo: Option<Box<Yoyo>>,
}

/// What the caller supplies besides the projectile for one step.
#[derive(Clone, Copy, Debug, Default)]
pub struct StepInput<'a> {
    /// The target's start-of-frame (or end-of-frame, see the module docs) snapshot; `None` = gone.
    pub target: Option<&'a MoverSnapshot>,
    /// The shooter (the yo-yo ends when it is gone).
    pub source: Option<&'a MoverSnapshot>,
    /// The shooter's hand position (the yo-yo's way back); defaults to the shooter's position `+ (0, 1, 0)`.
    pub hand: Option<Vec3>,
}

/// Creates a projectile with the plan's argument list (Plan/31 §14): the kind's table values, the speed
/// given, the launch aimed from `origin` at `dest`. `target_snap` fixes the contact radius of `WandAtk1`
/// early (it is otherwise taken at the first step).
pub fn projectile_spawn(
    kind: ProjectileKind,
    origin: Vec3,
    dest: Vec3,
    target: ObjId,
    speed: f32,
    target_snap: Option<&MoverSnapshot>,
) -> Projectile {
    let mut a = SpawnArgs::new(kind, origin, dest, target);
    a.speed = speed;
    let mut p = Projectile::spawn(&a);
    if let Some(t) = target_snap {
        p.fix_wand_radius(t);
    }
    p
}

/// One frame of flight with the plan's argument list (Plan/31 §14).
pub fn projectile_step(p: &mut Projectile, target: Option<&MoverSnapshot>) -> ProjStep {
    p.step(&StepInput {
        target,
        ..StepInput::default()
    })
}

fn finite3(v: Vec3) -> bool {
    v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
}

/// A position made safe: non-finite components become `0`, the others are clamped to [`COORD_LIMIT`].
fn tame(v: Vec3) -> Vec3 {
    let f = |x: f32| {
        if x.is_finite() {
            x.clamp(-COORD_LIMIT, COORD_LIMIT)
        } else {
            0.0
        }
    };
    Vec3::new(f(v.x), f(v.y), f(v.z))
}

impl Projectile {
    /// A projectile at the start of its spawn frame.
    pub fn spawn(a: &SpawnArgs) -> Projectile {
        let info = a.kind.info();
        let speed = if a.speed.is_finite() && a.speed > 0.0 {
            a.speed.min(MAX_SPEED)
        } else {
            info.speed_faster
        };
        let origin = if finite3(a.origin) {
            tame(a.origin)
        } else {
            Vec3::ZERO
        };
        let dest = if finite3(a.dest) {
            tame(a.dest)
        } else {
            origin
        };
        let mut p = Projectile {
            id: a.id,
            kind: a.kind,
            pos: origin,
            vel: Vec3::ZERO,
            dest,
            target: a.target,
            source: a.source,
            speed,
            radius: match a.kind {
                // `m_fRadiusXZ = 0.2f` until the first step reads the target box
                ProjectileKind::WandAtk1 => 0.2,
                ProjectileKind::AtkStraight => a.model_radius,
                _ => info.contact_radius,
            },
            age: 0,
            max_age: info.max_age,
            done: false,
            hit: false,
            dir: Vec3::ZERO,
            yoyo: None,
        };
        match a.kind {
            ProjectileKind::WandAtkAir => {
                let l = a.launch.unwrap_or_else(|| aim(origin, dest));
                p.vel = angle_to_vector(l.yaw, -l.pitch, speed);
            }
            ProjectileKind::AtkStraight => {
                let l = a.launch.unwrap_or_else(|| aim(origin, dest));
                p.vel = angle_to_vector(l.yaw, -l.pitch, speed);
            }
            ProjectileKind::YoyoAtk => {
                // the path needs the shooter's position: `Projectile::make_yoyo_path`
            }
            _ => {}
        }
        p.dir = normalize_or_zero(p.vel);
        p
    }

    /// `CSfxItemYoyoAtk::MakePath`: builds the spline from the hand position (the spawn origin) toward
    /// `dest`, anchored at the shooter's `mover_pos` (x and z) and the origin's height (`mWorld._42 =
    /// vPosPos.y`). Called by the spawner right after [`Projectile::spawn`] for [`ProjectileKind::YoyoAtk`].
    pub fn make_yoyo_path(&mut self, mover_pos: Vec3) {
        self.yoyo = Some(Box::new(Yoyo {
            path: yoyo_path(self.pos, self.dest, mover_pos),
            step: 0,
            count: 0,
            delay: 0,
        }));
    }

    /// `CSfxItemWandAtk1::Process` frame 0: `r = ((|dx| + |dz|) / 2 * scale) / 3` of the target box.
    fn fix_wand_radius(&mut self, t: &MoverSnapshot) {
        let len_x = (t.bounds.max.x - t.bounds.min.x).abs();
        let len_z = (t.bounds.max.z - t.bounds.min.z).abs();
        let mut r = (len_x + len_z) / 2.0;
        r *= t.scale;
        r /= 3.0;
        self.radius = r;
    }

    /// [`Projectile::step`] with the target and the shooter looked up in a snapshot (the usual call of the
    /// server's projectile phase and of the client host): `lookup` holds the positions the projectile must
    /// home on (the end-of-frame positions, see the module docs).
    pub fn step_in(&mut self, lookup: &dyn MoverLookup) -> ProjStep {
        let target = lookup.snapshot(self.target);
        let source = lookup.snapshot(self.source);
        self.step(&StepInput {
            target: target.as_ref(),
            source: source.as_ref(),
            hand: None,
        })
    }

    /// One frame of flight. A finished projectile answers its outcome again without moving.
    pub fn step(&mut self, input: &StepInput<'_>) -> ProjStep {
        if self.done {
            return self.outcome();
        }
        let r = match self.kind {
            ProjectileKind::Shoot
            | ProjectileKind::RangeAtk1
            | ProjectileKind::RangeAtkJunkbow
            | ProjectileKind::MagStrongWind
            | ProjectileKind::MagSwordWind
            | ProjectileKind::MagFireBoomerang
            | ProjectileKind::MagIceMissile
            | ProjectileKind::MagLightningBall
            | ProjectileKind::MagSpikeStone
            | ProjectileKind::ShootWave => self.step_homing(input),
            ProjectileKind::WandAtk1 => self.step_wand(input),
            ProjectileKind::WandAtkAir => self.step_wand_air(input),
            ProjectileKind::AtkStraight => self.step_straight(),
            ProjectileKind::YoyoAtk => self.step_yoyo(input),
            ProjectileKind::MagHotAir => self.finish_hit(),
        };
        if r == ProjStep::Flying && self.age >= self.max_age {
            self.done = true;
            return ProjStep::Expired;
        }
        r
    }

    fn outcome(&self) -> ProjStep {
        if self.hit {
            ProjStep::Hit { pos: self.pos }
        } else {
            ProjStep::Expired
        }
    }

    fn finish_hit(&mut self) -> ProjStep {
        self.done = true;
        self.hit = true;
        ProjStep::Hit { pos: self.pos }
    }

    fn finish_expired(&mut self) -> ProjStep {
        self.done = true;
        self.hit = false;
        ProjStep::Expired
    }

    /// `pos += normalize(dest - pos) * speed`; records the direction.
    fn move_homing(&mut self) {
        let d = normalize_or_zero(self.dest - self.pos);
        self.pos += d * self.speed;
        self.dir = d;
    }

    /// The destination of the homing kinds from the target snapshot (`prj.GetCtrl(m_idDest)` valid).
    fn refresh_dest(&mut self, t: &MoverSnapshot) {
        let up = |dy: f32| Vec3::new(t.pos.x, t.pos.y + dy, t.pos.z);
        self.dest = match self.kind {
            ProjectileKind::Shoot
            | ProjectileKind::RangeAtk1
            | ProjectileKind::RangeAtkJunkbow
            | ProjectileKind::WandAtkAir => up(1.0),
            ProjectileKind::MagFireBoomerang
            | ProjectileKind::MagIceMissile
            | ProjectileKind::MagLightningBall
            | ProjectileKind::MagSpikeStone => up(0.3),
            _ => t.pos,
        };
    }

    /// `CSfxShoot::Process` and its subclasses (arrows, winds, missiles, the wave).
    fn step_homing(&mut self, input: &StepInput<'_>) -> ProjStep {
        match input.target {
            Some(t) => self.refresh_dest(t),
            None => {
                // arrows are deleted when the target vanishes; the others fly on to the last destination
                if matches!(
                    self.kind,
                    ProjectileKind::RangeAtk1 | ProjectileKind::RangeAtkJunkbow
                ) {
                    return self.finish_expired();
                }
            }
        }
        self.move_homing();
        if self.kind == ProjectileKind::ShootWave {
            // `vPos.y = m_SfxObj2.m_vPos.y`: the wave runs at the target's height
            self.pos.y = self.dest.y;
        }
        let touched = match self.kind {
            ProjectileKind::Shoot => length_sq(self.pos - self.dest) < 0.2_f32 * 0.2_f32,
            ProjectileKind::RangeAtk1
            | ProjectileKind::RangeAtkJunkbow
            | ProjectileKind::ShootWave => length_sq(self.pos - self.dest) < 0.4_f32 * 0.4_f32,
            ProjectileKind::MagSwordWind => length(self.pos - self.dest) < 0.4,
            // StrongWind and the four missiles
            _ => length(self.dest - self.pos) < 0.2,
        };
        self.age += 1;
        if touched {
            return self.finish_hit();
        }
        ProjStep::Flying
    }

    /// `CSfxItemWandAtk1::Process`: the target must exist and be alive, the radius is read from its box at
    /// the first step.
    fn step_wand(&mut self, input: &StepInput<'_>) -> ProjStep {
        let Some(t) = input.target else {
            return self.finish_expired();
        };
        if !t.alive {
            return self.finish_expired();
        }
        let height = t.bounds.max.y.abs() / 2.0 * t.scale;
        self.dest = Vec3::new(t.pos.x, t.pos.y + height, t.pos.z);
        if self.age == 0 {
            self.fix_wand_radius(t);
        }
        self.move_homing();
        let touched = length_sq(self.pos - self.dest) < self.radius * self.radius;
        self.age += 1;
        if touched {
            return self.finish_hit();
        }
        ProjStep::Flying
    }

    /// `CSfxItemWandAtkAir::Process` (Plan/17 §4.3): the radius `R` of the contact test is `0.6` (`3.0`
    /// when the target is an NPC and still exists); the distance is measured BEFORE the move, and the
    /// contact is declared after the move of the same frame (the hit position is the new one). The
    /// inertia: `acc = AngleToVector(angXZ, angH, 0.007)` toward the destination is added while
    /// `|v|^2 < 0.45^2`, then `v *= 0.985`; a vanished target leaves the last destination; the explosion
    /// at 480 frames is an expiry.
    fn step_wand_air(&mut self, input: &StepInput<'_>) -> ProjStep {
        let mut radius = 0.6_f32;
        if let Some(t) = input.target {
            self.refresh_dest(t);
            if !t.is_player {
                radius = 3.0;
            }
        }
        let v_dist = self.dest - self.pos;
        let v_dist_xz = Vec3::new(v_dist.x, 0.0, v_dist.z);
        let ang_xz = to_degree(atan2f(v_dist.x, -v_dist.z));
        let dist_xz = length(v_dist_xz);
        let mut ang_h = to_degree(atan2f(dist_xz, v_dist.y));
        ang_h -= 90.0;
        ang_h = -ang_h;
        let acc = angle_to_vector(ang_xz, ang_h, 0.007);
        if length_sq(self.vel) < 0.45_f32 * 0.45_f32 {
            self.vel += acc;
        }
        self.vel = Vec3::new(self.vel.x * 0.985, self.vel.y * 0.985, self.vel.z * 0.985);
        self.pos += self.vel;
        self.dir = normalize_or_zero(self.vel);
        let touched = length_sq(v_dist) < radius * radius;
        self.age += 1;
        if touched {
            return self.finish_hit();
        }
        ProjStep::Flying
    }

    /// `CSfxAtkStraight::Process`: constant velocity, contact is `IsRangeObj(dest, 0)` of the projectile.
    fn step_straight(&mut self) -> ProjStep {
        self.pos += self.vel;
        self.dir = normalize_or_zero(self.vel);
        let touched = is_range_point(self.pos, self.radius, self.dest, 0.0);
        self.age += 1;
        if touched {
            return self.finish_hit();
        }
        ProjStep::Flying
    }

    /// `CSfxItemYoyoAtk::Process`: flies to the first path point (`0.3` per frame, `<= 0.05` squared), waits
    /// 6 frames, follows the path one point per frame, then returns to the hand (`0.4` per frame,
    /// `<= 0.1` squared = `Hit`). The shooter must exist.
    fn step_yoyo(&mut self, input: &StepInput<'_>) -> ProjStep {
        let Some(src) = input.source else {
            return self.finish_expired();
        };
        let Some(y) = self.yoyo.as_deref_mut() else {
            return self.finish_expired();
        };
        if y.delay > 0 {
            // `return` before `m_nFrame++`: the pause does not age the projectile
            y.delay -= 1;
            return ProjStep::Flying;
        }
        let before = self.pos;
        let mut arrived = false;
        match y.step {
            0 => {
                let first = y.path.first().copied().unwrap_or(self.pos);
                let d = normalize_or_zero(first - self.pos);
                self.pos += d * 0.3;
                if length_sq(first - self.pos) <= 0.05 {
                    y.step = 1;
                    y.delay = 6;
                }
            }
            1 => {
                if let Some(p) = y.path.get(y.count as usize).copied() {
                    self.pos = p;
                }
                y.count += 1;
                if y.count as usize >= y.path.len() {
                    y.step = 2;
                }
            }
            _ => {
                let hand = input
                    .hand
                    .unwrap_or_else(|| Vec3::new(src.pos.x, src.pos.y + 1.0, src.pos.z));
                let d = normalize_or_zero(hand - self.pos);
                self.pos += d * 0.4;
                arrived = length_sq(hand - self.pos) <= 0.1;
            }
        }
        let moved = normalize_or_zero(self.pos - before);
        if length_sq(moved) > 0.0 {
            self.dir = moved;
        }
        if arrived {
            return self.finish_hit();
        }
        self.age += 1;
        ProjStep::Flying
    }
}

/// The aim of a shot from `from` at `to`: `(yaw, pitch)` in the mover convention (`pitch` negative = up);
/// a zero vector aims flat along the yaw 0 (`xGetDegree` of it would point straight up).
fn aim(from: Vec3, to: Vec3) -> Launch {
    let d = to - from;
    if length_sq(d) == 0.0 {
        return Launch {
            yaw: 0.0,
            pitch: 0.0,
        };
    }
    let (yaw, pitch) = x_get_degree(d);
    Launch { yaw, pitch }
}

/// `SplineSlerp(v1, v2, v3, v4, t)` (`Sfx.cpp:1078`): a Catmull-Rom interpolation with `alpha = 0`.
pub fn spline_slerp(v1: Vec3, v2: Vec3, v3: Vec3, v4: Vec3, t: f32) -> Vec3 {
    let t2 = t * t;
    let t3 = t2 * t;
    let half = (1.0_f32 - 0.0_f32) / 2.0_f32;
    let m0 = ((v2 - v1) + v3 - v2) * half;
    let m1 = ((v3 - v2) + v4 - v3) * half;
    let a = (2.0 * t3 - 3.0 * t2 + 1.0) * v2;
    let b = (t3 - 2.0 * t2 + t) * m0;
    let c = (t3 - t2) * m1;
    let d = (-2.0 * t3 + 3.0 * t2) * v3;
    a + b + c + d
}

/// `D3DXQuaternionRotationAxis(axis, angle)`: the axis is normalised first.
fn quat_axis_angle(axis: Vec3, angle: f32) -> Quat {
    let n = normalize_or_zero(axis);
    let half = angle * 0.5;
    let s = sinf(half);
    Quat::new(s * n.x, s * n.y, s * n.z, cosf(half))
}

/// `CSfxItemYoyoAtk::MakePath` (`Sfx.cpp:1121-1237`): seven control points in front of the shooter along
/// `-z` (length `L = clamp(|dest - src|^2, 12 (below 9), 49) / 5`, `L, L + 0.5, L + 1, L + 1.2, L + 1,
/// L + 0.5, L` with the heights `-0.2, -0.2, -0.1, 0, 0.1, 0.2, 0.2`), a Catmull-Rom spline through them
/// (`MAX_SF_SLERP = 3` sub-steps: 6 segments x 4 points), rotated from `-z` to the direction of `dest` and
/// translated to `mover_pos` with the height of `src`.
pub fn yoyo_path(src: Vec3, dest: Vec3, mover_pos: Vec3) -> Vec<Vec3> {
    let mut len = length_sq(dest - src);
    if len > 7.0 * 7.0 {
        len = 49.0;
    }
    if len < 3.0 * 3.0 {
        len = 12.0;
    }
    len /= 5.0;
    let at = |dist: f32, dy: f32| Vec3::new(0.0, dy, -dist);
    let ctrl = [
        at(len, -0.2),
        at(len + 0.5, -0.2),
        at(len + 1.0, -0.1),
        at(len + 1.2, 0.0),
        at(len + 1.0, 0.1),
        at(len + 0.5, 0.2),
        at(len, 0.2),
    ];
    let pick = |i: usize| ctrl.get(i).copied().unwrap_or(Vec3::ZERO);
    let mut spline: Vec<Vec3> = Vec::with_capacity(24);
    for i in 0..6usize {
        let a = i.saturating_sub(1);
        let d = (i + 2).min(6);
        for j in 0..4u32 {
            spline.push(spline_slerp(
                pick(a),
                pick(i),
                pick(i + 1),
                pick(d),
                j as f32 / 3.0,
            ));
        }
    }
    let dir = Vec3::new(0.0, 0.0, -1.0);
    let to = normalize_or_zero(dest - src);
    let axis = cross(dir, to);
    let theta = dot(dir, to);
    let q = quat_axis_angle(axis, acosf(theta.clamp(-1.0, 1.0)));
    let mut m: Mat44 = mat_from_quat(q);
    m.0[12] = mover_pos.x;
    m.0[13] = src.y;
    m.0[14] = mover_pos.z;
    spline.into_iter().map(|p| transform_coord(&m, p)).collect()
}
