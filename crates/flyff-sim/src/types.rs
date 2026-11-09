//! State word, flags and wire-visible constants (Plan/31 §4; WP-SIM-01).
//!
//! All values are wire-visible (`PlayerMoveState.state/state_flags/motion`, `MoverStateNtf`, `IntentStamp`,
//! `SendActMsg.obj_msg`, ...) and keep the legacy numbers (ref/12 port note 7). Rust names keep the legacy
//! spelling (Plan/02 §11.1): `OBJSTA_*`, `OBJSTAF_*`, `OBJMSG_*`, `MVRF_*`, `AF_*`, `MTI_*`, ...

use thiserror::Error;

/// Errors of the sim crate's decoders and constructors. Decoders never panic: every malformed input
/// maps to one of these (Plan/31 §2.2 D-6).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Error)]
pub enum SimError {
    /// A `FLYC` collision chunk failed validation (Plan/24 §10.7.7); the payload names the rule.
    #[error("bad collision chunk: {0}")]
    BadChunk(&'static str),
    /// A chunk does not belong to the world it is loaded into (`land_x/land_z/lands_w/lands_h/mpu`).
    #[error("collision chunk does not match the world description: {0}")]
    ChunkMismatch(&'static str),
    /// A `FLMT` motion table failed validation (Plan/31 §12.1).
    #[error("bad motion table: {0}")]
    BadMotionTable(&'static str),
    /// A `FLMB` model-bounds file failed validation (Plan/31 §11.5).
    #[error("bad model bounds: {0}")]
    BadModelBounds(&'static str),
    /// A `FLYK` ctrl-model file failed validation (Plan/24 §10.8).
    #[error("bad ctrl models: {0}")]
    BadCtrlModels(&'static str),
    /// An `FSK1` skeleton pack failed validation (Plan/30 §13.3, Plan/31 §13.1; `pose::SimSkeleton`).
    #[error("bad skeleton pack: {0}")]
    BadSkeleton(&'static str),
    /// An `FMO1` clip pack failed validation (Plan/30 §13.4, Plan/31 §13.1; `pose::SimClip`).
    #[error("bad clip pack: {0}")]
    BadClip(&'static str),
    /// Land coordinates outside the world.
    #[error("land coordinates outside the world")]
    LandOutOfRange,
    /// An invalid world description (zero or oversized dimensions, unsupported MPU).
    #[error("invalid world description: {0}")]
    BadWorld(&'static str),
    /// A malformed command-buffer record or `SET_PARAMS` block (`flyff_sim::wire`, Plan/31 §17.4).
    #[error("bad command: {0}")]
    BadCommand(&'static str),
}

// ---------------------------------------------------------------------------------------------------
// OBJSTA_* (u32 `m_dwState`, `_Common/MoverMsg.h:5-91`)
// ---------------------------------------------------------------------------------------------------

/// No state.
pub const OBJSTA_NONE: u32 = 0x0000_0000;
/// MOVE group: standing.
pub const OBJSTA_STAND: u32 = 0x0000_0001;
/// MOVE group: standing (EX).
pub const OBJSTA_STAND2: u32 = 0x0000_0002;
/// MOVE group: sitting.
pub const OBJSTA_SIT: u32 = 0x0000_0003;
/// MOVE group: walking/running forward.
pub const OBJSTA_FMOVE: u32 = 0x0000_0004;
/// MOVE group: walking backward.
pub const OBJSTA_BMOVE: u32 = 0x0000_0005;
/// MOVE group: strafe left (inactive).
pub const OBJSTA_LEFT: u32 = 0x0000_0006;
/// MOVE group: strafe right (inactive).
pub const OBJSTA_RIGHT: u32 = 0x0000_0007;
/// MOVE group: picking up an item.
pub const OBJSTA_PICKUP: u32 = 0x0000_0008;
/// MOVE group: strafe left (`__Y_INTERFACE_VER3`, inactive).
pub const OBJSTA_LMOVE: u32 = 0x0000_0009;
/// MOVE group: strafe right (`__Y_INTERFACE_VER3`, inactive).
pub const OBJSTA_RMOVE: u32 = 0x0000_000A;
/// MOVE group: running in place (client only).
pub const OBJSTA_STOP_RUN: u32 = 0x0000_000B;
/// Mask of the MOVE group.
pub const OBJSTA_MOVE_ALL: u32 = 0x0000_00FF;
/// `OBJSTA_MOVE_ALL & !OBJSTA_STAND`: "anything but standing" probe.
pub const OBJSTA_NOT_STAND: u32 = OBJSTA_MOVE_ALL & !OBJSTA_STAND;

/// TURN group: turn left.
pub const OBJSTA_LTURN: u32 = 0x0000_0100;
/// TURN group: turn right.
pub const OBJSTA_RTURN: u32 = 0x0000_0200;
/// Mask of the TURN group.
pub const OBJSTA_TURN_ALL: u32 = 0x0000_0300;
/// LOOK group: look up.
pub const OBJSTA_LOOKUP: u32 = 0x0000_0400;
/// LOOK group: look down.
pub const OBJSTA_LOOKDOWN: u32 = 0x0000_0800;
/// Mask of the LOOK group.
pub const OBJSTA_LOOK_ALL: u32 = 0x0000_0C00;

/// JUMP group (only `SJUMP1..4` are used).
pub const OBJSTA_FJUMP_READY: u32 = 0x0000_1000;
/// JUMP group.
pub const OBJSTA_FJUMP: u32 = 0x0000_2000;
/// JUMP group.
pub const OBJSTA_FFALL: u32 = 0x0000_3000;
/// JUMP group.
pub const OBJSTA_FLAND: u32 = 0x0000_4000;
/// JUMP group: take-off.
pub const OBJSTA_SJUMP1: u32 = 0x0000_5000;
/// JUMP group: rising.
pub const OBJSTA_SJUMP2: u32 = 0x0000_6000;
/// JUMP group: falling.
pub const OBJSTA_SJUMP3: u32 = 0x0000_7000;
/// JUMP group: landing.
pub const OBJSTA_SJUMP4: u32 = 0x0000_8000;
/// JUMP group.
pub const OBJSTA_BJUMP_READY: u32 = 0x0000_9000;
/// JUMP group.
pub const OBJSTA_BJUMP: u32 = 0x0000_A000;
/// JUMP group.
pub const OBJSTA_BFALL: u32 = 0x0000_B000;
/// JUMP group.
pub const OBJSTA_BLAND: u32 = 0x0000_C000;
/// Mask of the JUMP group.
pub const OBJSTA_JUMP_ALL: u32 = 0x0000_F000;

/// ATK group: combo attack 1.
pub const OBJSTA_ATK1: u32 = 0x0001_0000;
/// ATK group: combo attack 2.
pub const OBJSTA_ATK2: u32 = 0x0002_0000;
/// ATK group: combo attack 3.
pub const OBJSTA_ATK3: u32 = 0x0003_0000;
/// ATK group: combo attack 4.
pub const OBJSTA_ATK4: u32 = 0x0004_0000;
/// ATK group: wand attack.
pub const OBJSTA_ATK_MAGIC1: u32 = 0x0005_0000;
/// ATK group: ranged attack.
pub const OBJSTA_ATK_RANGE1: u32 = 0x0006_0000;
/// ATK group.
pub const OBJSTA_RANGE3: u32 = 0x0007_0000;
/// ATK group.
pub const OBJSTA_RANGE4: u32 = 0x0008_0000;
/// ATK group: melee skill.
pub const OBJSTA_ATK_MELEESKILL: u32 = 0x0009_0000;
/// ATK group: range skill.
pub const OBJSTA_ATK_RANGESKILL: u32 = 0x000A_0000;
/// ATK group: casting start.
pub const OBJSTA_ATK_CASTING1: u32 = 0x000B_0000;
/// ATK group: casting loop.
pub const OBJSTA_ATK_CASTING2: u32 = 0x000C_0000;
/// ATK group: magic skill release.
pub const OBJSTA_ATK_MAGICSKILL: u32 = 0x000D_0000;
/// ATK group: special attack 1 (monsters).
pub const OBJSTA_SP_ATK1: u32 = 0x0010_0000;
/// ATK group: special attack 2 (monsters).
pub const OBJSTA_SP_ATK2: u32 = 0x0020_0000;
/// Mask of the ATK group.
pub const OBJSTA_ATK_ALL: u32 = 0x00FF_0000;

/// DMG group: hit.
pub const OBJSTA_DMG: u32 = 0x0100_0000;
/// DMG group: knocked back.
pub const OBJSTA_DMG_FLY: u32 = 0x0200_0000;
/// DMG group: knocked down.
pub const OBJSTA_DMG_DIE: u32 = 0x0300_0000;
/// DMG group (reserved).
pub const OBJSTA_DMG_RESERVED1: u32 = 0x0400_0000;
/// DMG group (reserved).
pub const OBJSTA_DMG_RESERVED2: u32 = 0x0500_0000;
/// DMG group: getting up after a knock-down.
pub const OBJSTA_DMG_LIVE: u32 = 0x0600_0000;
/// Bit that marks every knock-back state (`DMG_FLY`, `DMG_DIE`, `DMG_LIVE`): a bit test.
pub const OBJSTA_DMG_FLY_ALL: u32 = 0x0200_0000;
/// DMG group: dead.
pub const OBJSTA_DEAD: u32 = 0x0800_0000;
/// DMG group: disappearing after death.
pub const OBJSTA_DISAPPEAR: u32 = 0x0900_0000;
/// DMG group: resurrection animation.
pub const OBJSTA_RESURRECTION: u32 = 0x0C00_0000;
/// DMG group (reserved).
pub const OBJSTA_DEAD_RESERVED1: u32 = 0x0D00_0000;
/// Bit that marks every dead state (`DEAD`, `DISAPPEAR`, `RESURRECTION`, `0xD`): a bit test.
pub const OBJSTA_DIE_ALL: u32 = 0x0800_0000;
/// Mask of the DMG group.
pub const OBJSTA_DMG_ALL: u32 = 0x0F00_0000;

/// ACTION group: collecting.
pub const OBJSTA_COLLECT: u32 = 0x1000_0000;
/// ACTION group: appear scene.
pub const OBJSTA_APPEAR: u32 = 0x2000_0000;
/// ACTION group: appear scene 2.
pub const OBJSTA_APPEAR2: u32 = 0x3000_0000;
/// ACTION group: stunned.
pub const OBJSTA_STUN: u32 = 0x4000_0000;
/// Mask of the ACTION group.
pub const OBJSTA_ACTION_ALL: u32 = 0xF000_0000;

/// The union of all group masks (`OBJSTA_ALL`).
pub const OBJSTA_ALL: u32 = OBJSTA_MOVE_ALL
    | OBJSTA_TURN_ALL
    | OBJSTA_LOOK_ALL
    | OBJSTA_JUMP_ALL
    | OBJSTA_ATK_ALL
    | OBJSTA_DMG_ALL
    | OBJSTA_ACTION_ALL;

/// The state word `m_dwState` with the bit-test helpers of `CAction` (KEEP-QUIRK semantics,
/// `_AIInterface/Action.h:306-330`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug)]
pub struct ObjState(pub u32);

impl ObjState {
    /// The raw word.
    #[inline]
    pub const fn bits(self) -> u32 {
        self.0
    }
    /// `IsState(mask)`: a BIT TEST (e.g. `is_state(FMOVE)` is true for `BMOVE`, `LEFT` and `RIGHT`).
    #[inline]
    pub const fn is_state(self, mask: u32) -> bool {
        self.0 & mask != 0
    }
    /// `DMG_FLY_ALL` bit test: true for `DMG_FLY`, `DMG_DIE`, `DMG_LIVE`.
    #[inline]
    pub const fn dmg_fly_all(self) -> bool {
        self.0 & OBJSTA_DMG_FLY_ALL != 0
    }
    /// `IsDie()`: true for `DEAD`, `DISAPPEAR`, `RESURRECTION` and `0xD`.
    #[inline]
    pub const fn is_die(self) -> bool {
        self.0 & OBJSTA_DIE_ALL != 0
    }
    /// `IsMove()`: equality on the MOVE group (`FMOVE`, `BMOVE`, `LEFT`, `RIGHT`).
    #[inline]
    pub const fn is_move(self) -> bool {
        let m = self.0 & OBJSTA_MOVE_ALL;
        m == OBJSTA_FMOVE || m == OBJSTA_BMOVE || m == OBJSTA_LEFT || m == OBJSTA_RIGHT
    }
    /// MOVE group value.
    #[inline]
    pub const fn move_(self) -> u32 {
        self.0 & OBJSTA_MOVE_ALL
    }
    /// TURN group value.
    #[inline]
    pub const fn turn(self) -> u32 {
        self.0 & OBJSTA_TURN_ALL
    }
    /// LOOK group value.
    #[inline]
    pub const fn look(self) -> u32 {
        self.0 & OBJSTA_LOOK_ALL
    }
    /// JUMP group value.
    #[inline]
    pub const fn jump(self) -> u32 {
        self.0 & OBJSTA_JUMP_ALL
    }
    /// ATK group value.
    #[inline]
    pub const fn atk(self) -> u32 {
        self.0 & OBJSTA_ATK_ALL
    }
    /// DMG group value.
    #[inline]
    pub const fn dmg(self) -> u32 {
        self.0 & OBJSTA_DMG_ALL
    }
    /// ACTION group value.
    #[inline]
    pub const fn action(self) -> u32 {
        self.0 & OBJSTA_ACTION_ALL
    }
    /// `IsActJump()`.
    #[inline]
    pub const fn is_act_jump(self) -> bool {
        self.jump() != 0
    }
    /// `IsActAttack()`.
    #[inline]
    pub const fn is_act_attack(self) -> bool {
        self.atk() != 0
    }
    /// `IsActDamage()`.
    #[inline]
    pub const fn is_act_damage(self) -> bool {
        self.dmg() != 0
    }
    /// `IsActTurn()`.
    #[inline]
    pub const fn is_act_turn(self) -> bool {
        self.turn() != 0
    }
    /// `IsActLook()`.
    #[inline]
    pub const fn is_act_look(self) -> bool {
        self.look() != 0
    }
    /// `IsAction()`.
    #[inline]
    pub const fn is_action(self) -> bool {
        self.action() != 0
    }
}

// ---------------------------------------------------------------------------------------------------
// OBJSTAF_* (u32 `m_dwStateFlag`)
// ---------------------------------------------------------------------------------------------------

/// State flag: combat stance.
pub const OBJSTAF_COMBAT: u32 = 0x01;
/// State flag: walk mode (otherwise run).
pub const OBJSTAF_WALK: u32 = 0x02;
/// State flag: sitting.
pub const OBJSTAF_SIT: u32 = 0x04;
/// State flag: flying.
pub const OBJSTAF_FLY: u32 = 0x08;
/// State flag: accelerating (flight).
pub const OBJSTAF_ACC: u32 = 0x10;
/// State flag: custom motion playing.
pub const OBJSTAF_ETC: u32 = 0x20;
/// State flag: sharp turn (unused).
pub const OBJSTAF_ACCTURN: u32 = 0x40;
/// State flag: turbo.
pub const OBJSTAF_TURBO: u32 = 0x80;

/// The state flag word `m_dwStateFlag`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug)]
pub struct StateFlags(pub u32);

impl StateFlags {
    /// The raw word.
    #[inline]
    pub const fn bits(self) -> u32 {
        self.0
    }
    /// `IsStateFlag(f)`: bit test.
    #[inline]
    pub const fn has(self, f: u32) -> bool {
        self.0 & f != 0
    }
    /// `AddStateFlag`.
    #[inline]
    pub fn add(&mut self, f: u32) {
        self.0 |= f;
    }
    /// `RemoveStateFlag`.
    #[inline]
    pub fn remove(&mut self, f: u32) {
        self.0 &= !f;
    }
    /// `IsRun()`: the walk flag is clear.
    #[inline]
    pub const fn is_run(self) -> bool {
        self.0 & OBJSTAF_WALK == 0
    }
    /// `IsWalk()`.
    #[inline]
    pub const fn is_walk(self) -> bool {
        self.0 & OBJSTAF_WALK != 0
    }
    /// `IsFly()`.
    #[inline]
    pub const fn fly(self) -> bool {
        self.0 & OBJSTAF_FLY != 0
    }
    /// `IsSit()`.
    #[inline]
    pub const fn sit(self) -> bool {
        self.0 & OBJSTAF_SIT != 0
    }
    /// Combat stance flag.
    #[inline]
    pub const fn combat(self) -> bool {
        self.0 & OBJSTAF_COMBAT != 0
    }
}

// ---------------------------------------------------------------------------------------------------
// MVRF_* (u32 `CMover::m_dwFlag`)
// ---------------------------------------------------------------------------------------------------

/// Mover flag: no flags.
pub const MVRF_NONE: u32 = 0x0;
/// Mover flag: a skill chain is running.
pub const MVRF_SKILL: u32 = 0x1;
/// Mover flag: critical mode (one shot).
pub const MVRF_CRITICAL: u32 = 0x2;
/// Mover flag: flight auto-tracking.
pub const MVRF_TRACKING: u32 = 0x4;
/// Mover flag: hit continues until the animation ends.
pub const MVRF_HITCONT: u32 = 0x20;
/// Mover flag: laser charging.
pub const MVRF_LASERCHARGE: u32 = 0x40;
/// Mover flag: movement forbidden.
pub const MVRF_NOMOVE: u32 = 0x100;
/// Mover flag: attacks forbidden.
pub const MVRF_NOATTACK: u32 = 0x200;
/// Mover flag: item use forbidden.
pub const MVRF_NOUSERITEM: u32 = 0x400;
/// Mover flag: animation frozen.
pub const MVRF_NOANI: u32 = 0x800;
/// `NOMOVE | NOATTACK | NOUSERITEM`.
pub const MVRF_NOACTION: u32 = MVRF_NOMOVE | MVRF_NOATTACK | MVRF_NOUSERITEM;
/// `NOANI | NOACTION`.
pub const MVRF_FREEZE: u32 = MVRF_NOANI | MVRF_NOACTION;
/// Mover flag: meditation sit latch.
pub const MVRF_MEDITATION_SIT: u32 = 0x1000;

/// The mover flag word `m_dwFlag` (the sim-relevant bits, Plan/31 §4.3).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug)]
pub struct MoverFlags(pub u32);

impl MoverFlags {
    /// The raw word.
    #[inline]
    pub const fn bits(self) -> u32 {
        self.0
    }
    /// Bit test (`m_dwFlag & f`).
    #[inline]
    pub const fn has(self, f: u32) -> bool {
        self.0 & f != 0
    }
    /// `m_dwFlag |= f`.
    #[inline]
    pub fn add(&mut self, f: u32) {
        self.0 |= f;
    }
    /// `m_dwFlag &= !f`.
    #[inline]
    pub fn remove(&mut self, f: u32) {
        self.0 &= !f;
    }
}

// ---------------------------------------------------------------------------------------------------
// OBJMSG_* (u32, `MoverMsg.h:105-166`) and OBJACT_*
// ---------------------------------------------------------------------------------------------------

/// An action message number (`enum OBJMSG`).
pub type ObjMsg = u32;

macro_rules! obj_msgs {
    ($($name:ident = $val:expr),+ $(,)?) => {
        $(#[doc = concat!("`OBJMSG_", stringify!($name), "` (", stringify!($val), ").")]
          pub const $name: ObjMsg = $val;)+
    };
}

obj_msgs! {
    OBJMSG_NONE = 0, OBJMSG_FORWARD = 1, OBJMSG_BACKWARD = 2, OBJMSG_STAND = 3, OBJMSG_SITDOWN = 4,
    OBJMSG_STANDUP = 5, OBJMSG_STOP = 6, OBJMSG_ASTOP = 7, OBJMSG_LTURN = 8, OBJMSG_RTURN = 9,
    OBJMSG_STOP_TURN = 10, OBJMSG_PICKUP = 11, OBJMSG_RESURRECTION = 12, OBJMSG_COLLECT = 13,
    OBJMSG_APPEAR = 14, OBJMSG_APPEAR2 = 15, OBJMSG_STUN = 16, OBJMSG_JUMP = 17,
    OBJMSG_MODE_COMBAT = 18, OBJMSG_MODE_PEACE = 19, OBJMSG_MODE_WALK = 20, OBJMSG_MODE_RUN = 21,
    OBJMSG_MODE_FLY = 22, OBJMSG_MODE_GROUND = 23, OBJMSG_MODE_TURBO_ON = 24,
    OBJMSG_MODE_TURBO_OFF = 25, OBJMSG_MODE_TURN = 26, OBJMSG_ACC_START = 27, OBJMSG_ACC_STOP = 28,
    OBJMSG_ATK1 = 29, OBJMSG_ATK2 = 30, OBJMSG_ATK3 = 31, OBJMSG_ATK4 = 32, OBJMSG_SP_ATK1 = 33,
    OBJMSG_SP_ATK2 = 34, OBJMSG_ATK_RANGE1 = 35, OBJMSG_ATK_MAGIC1 = 36, OBJMSG_MELEESKILL = 37,
    OBJMSG_MAGICSKILL = 38, OBJMSG_MAGICCASTING = 39, OBJMSG_DIE = 40, OBJMSG_DAMAGE = 41,
    OBJMSG_DAMAGE_FORCE = 42, OBJMSG_LOOKUP = 43, OBJMSG_LOOKDOWN = 44, OBJMSG_STOP_LOOK = 45,
    OBJMSG_MOTION = 46, OBJMSG_TURNMOVE = 47, OBJMSG_TURNMOVE2 = 48, OBJMSG_BASE_RADY = 49,
    OBJMSG_TEMP = 50, OBJMSG_TEMP2 = 51, OBJMSG_TEMP3 = 52, OBJMSG_DESTPOS = 53, OBJMSG_DESTOBJ = 54,
    OBJMSG_FALL = 55, OBJMSG_LFORWARD = 56, OBJMSG_RFORWARD = 57, OBJMSG_STOP_RUN = 58,
}

/// `OBJACT`: command executed after reaching a destination object.
pub const OBJACT_NONE: u32 = 0;
/// `OBJACT_MELEE_ATTACK`.
pub const OBJACT_MELEE_ATTACK: u32 = 1;
/// `OBJACT_MAGIC_ATTACK`.
pub const OBJACT_MAGIC_ATTACK: u32 = 2;
/// `OBJACT_RANGE_ATTACK`.
pub const OBJACT_RANGE_ATTACK: u32 = 3;
/// `OBJACT_SP_ATTACK`.
pub const OBJACT_SP_ATTACK: u32 = 4;
/// `OBJACT_USESKILL`.
pub const OBJACT_USESKILL: u32 = 5;
/// `OBJACT_USEITEM`.
pub const OBJACT_USEITEM: u32 = 6;
/// `OBJACT_COLLECT`.
pub const OBJACT_COLLECT: u32 = 7;

// ---------------------------------------------------------------------------------------------------
// SendActMsg return codes (Plan/31 §6.1)
// ---------------------------------------------------------------------------------------------------

/// `SendActMsg` result: positive = executed, 0 = ignored/busy, negatives = refusal reasons.
pub type ActResult = i32;
/// Executed (state changed): the caller may relay/report.
pub const ACT_OK: ActResult = 1;
/// Already in that state / busy: the server QUEUES attack messages.
pub const ACT_IGNORED: ActResult = 0;
/// Invalid or dead target, or jump in progress (movement messages).
pub const ACT_BAD_TARGET: ActResult = -1;
/// Ignore completely, do not relay (attacking / no-attack).
pub const ACT_DROP: ActResult = -2;
/// Blocked by a damage state.
pub const ACT_DAMAGED: ActResult = -3;
/// `STAND` while dead.
pub const ACT_DEAD: ActResult = -4;

// ---------------------------------------------------------------------------------------------------
// Attack flags, animation loop modes, motion options and attributes
// ---------------------------------------------------------------------------------------------------

/// Attack flag `AF_GENERIC`.
pub const AF_GENERIC: u32 = 0x1;
/// Attack flag `AF_MISS`.
pub const AF_MISS: u32 = 0x2;
/// Attack flag `AF_MAGIC`.
pub const AF_MAGIC: u32 = 0x8;
/// Attack flag `AF_MELEESKILL`.
pub const AF_MELEESKILL: u32 = 0x10;
/// Attack flag `AF_MAGICSKILL`.
pub const AF_MAGICSKILL: u32 = 0x20;
/// Attack flag `AF_CRITICAL1`.
pub const AF_CRITICAL1: u32 = 0x40;
/// Attack flag `AF_CRITICAL2`.
pub const AF_CRITICAL2: u32 = 0x80;
/// Attack flag `AF_PUSH`.
pub const AF_PUSH: u32 = 0x100;
/// Attack flag `AF_PARRY`.
pub const AF_PARRY: u32 = 0x200;
/// Attack flag `AF_RESIST`.
pub const AF_RESIST: u32 = 0x400;
/// Attack flag `AF_STUN`.
pub const AF_STUN: u32 = 0x800;
/// Attack flag `AF_BLOCKING`.
pub const AF_BLOCKING: u32 = 0x1000;
/// Attack flag `AF_FORCE`.
pub const AF_FORCE: u32 = 0x2000;
/// Attack flag `AF_RANGE`.
pub const AF_RANGE: u32 = 0x4000;
/// Attack flag `AF_MONSTER_SP_CLIENT`.
pub const AF_MONSTER_SP_CLIENT: u32 = 0x8000;
/// Attack flag `AF_FLYING`.
pub const AF_FLYING: u32 = 0x1000_0000;

/// `ANILOOP_1PLAY`: stop at the end.
pub const ANILOOP_1PLAY: u32 = 0x1;
/// `ANILOOP_CONT`: hold the last frame.
pub const ANILOOP_CONT: u32 = 0x2;
/// `ANILOOP_LOOP`: wrap around.
pub const ANILOOP_LOOP: u32 = 0x4;

/// Motion option `MOP_SWDFORCE`: sword trail.
pub const MOP_SWDFORCE: u32 = 1;
/// Motion option `MOP_FIXED`: do not add the weapon motion offset.
pub const MOP_FIXED: u32 = 2;
/// Motion option `MOP_NO_TRANS`: no blending.
pub const MOP_NO_TRANS: u32 = 4;
/// Motion option `MOP_HITSLOW`.
pub const MOP_HITSLOW: u32 = 8;

/// Motion attribute `MA_HIT`.
pub const MA_HIT: u32 = 1;
/// Motion attribute `MA_SOUND`.
pub const MA_SOUND: u32 = 2;
/// Motion attribute `MA_QUAKE`.
pub const MA_QUAKE: u32 = 4;

/// Legacy "no id" sentinel (`NULL_ID`).
pub const NULL_ID: u32 = 0xFFFF_FFFF;

/// `REQ_USESKILL`: the client awaits the server's `UseSkill` answer (`m_dwReqFlag`).
pub const REQ_USESKILL: u32 = 0x1;
/// `REQ_MOTION`: the client awaits the server's motion answer.
pub const REQ_MOTION: u32 = 0x2;

// Motion ids (`MTI_*`, Resource/defineNeuz.h) used by the sim.

/// `MTI_STAND`.
pub const MTI_STAND: u32 = 0;
/// `MTI_IDLE1`.
pub const MTI_IDLE1: u32 = 1;
/// `MTI_IDLE2`.
pub const MTI_IDLE2: u32 = 2;
/// `MTI_WALK`.
pub const MTI_WALK: u32 = 3;
/// `MTI_BACK`.
pub const MTI_BACK: u32 = 4;
/// `MTI_RUN`.
pub const MTI_RUN: u32 = 5;
/// `MTI_DMG1`.
pub const MTI_DMG1: u32 = 6;
/// `MTI_DMG2`.
pub const MTI_DMG2: u32 = 7;
/// `MTI_DIE1`.
pub const MTI_DIE1: u32 = 8;
/// `MTI_DMGLIVE`.
pub const MTI_DMGLIVE: u32 = 9;
/// `MTI_DMGDIE`.
pub const MTI_DMGDIE: u32 = 10;
/// `MTI_DMGFLY`.
pub const MTI_DMGFLY: u32 = 11;
/// `MTI_GROGGY`.
pub const MTI_GROGGY: u32 = 12;
/// `MTI_JUMP1`.
pub const MTI_JUMP1: u32 = 13;
/// `MTI_JUMP2`.
pub const MTI_JUMP2: u32 = 14;
/// `MTI_JUMP3`.
pub const MTI_JUMP3: u32 = 15;
/// `MTI_JUMP4`.
pub const MTI_JUMP4: u32 = 16;
/// `MTI_SIT`.
pub const MTI_SIT: u32 = 20;
/// `MTI_SITSTAND`.
pub const MTI_SITSTAND: u32 = 21;
/// `MTI_GETUP`.
pub const MTI_GETUP: u32 = 22;
/// `MTI_ATK1`.
pub const MTI_ATK1: u32 = 23;
/// `MTI_ATK2`.
pub const MTI_ATK2: u32 = 24;
/// `MTI_ATK3`.
pub const MTI_ATK3: u32 = 25;
/// `MTI_ATK4`.
pub const MTI_ATK4: u32 = 26;
/// `MTI_FSTAND1_A`.
pub const MTI_FSTAND1_A: u32 = 31;
/// `MTI_PICKUP`.
pub const MTI_PICKUP: u32 = 121;
/// `MTI_COLLECT`.
pub const MTI_COLLECT: u32 = 142;
/// `MTI_APPEAR`.
pub const MTI_APPEAR: u32 = 143;
/// `MTI_APPEAR2`.
pub const MTI_APPEAR2: u32 = 144;
/// `MTI_ASS_RESURRECTION`.
pub const MTI_ASS_RESURRECTION: u32 = 1049;
/// `MTI_STAND_02` (wand stance offset).
pub const MTI_STAND_02: u32 = 300;
/// `MTI_STAND_05` (staff stance offset).
pub const MTI_STAND_05: u32 = 600;
/// `MTI_STAND_11` (psykeeper wand stance offset).
pub const MTI_STAND_11: u32 = 2200;
/// `MTI_STAND_13` (bow stance offset).
pub const MTI_STAND_13: u32 = 2328;
/// `MTI_STAND_14` (yoyo stance offset).
pub const MTI_STAND_14: u32 = 2356;

// Fly motion offsets (`MTA_*`, added to the ride item's `use_motion`).

/// `MTA_FSTAND1`.
pub const MTA_FSTAND1: u32 = 0;
/// `MTA_FLTURN1`.
pub const MTA_FLTURN1: u32 = 3;
/// `MTA_FRTURN1`.
pub const MTA_FRTURN1: u32 = 5;
/// `MTA_FRUNNING1`.
pub const MTA_FRUNNING1: u32 = 7;
/// `MTA_FDMG1`.
pub const MTA_FDMG1: u32 = 9;
/// `MTA_FATK1`.
pub const MTA_FATK1: u32 = 10;

// Height attributes (`HATTR_*`, `landscape.h:28-37`).

/// `HATTR_NONE`.
pub const HATTR_NONE: i32 = 0;
/// `HATTR_NOWALK`.
pub const HATTR_NOWALK: i32 = 1;
/// `HATTR_NOFLY`.
pub const HATTR_NOFLY: i32 = 2;
/// `HATTR_NOMOVE`.
pub const HATTR_NOMOVE: i32 = 3;
/// `HATTR_DIE`.
pub const HATTR_DIE: i32 = 4;
/// Height attribute result of a position outside the world.
pub const HATTR_OUTSIDE: i32 = -1;

// Water types (`WTYPE_*`, compared with `==`, no mask; `landscape.h:88-93`).

/// `WTYPE_NONE`.
pub const WTYPE_NONE: u8 = 0;
/// `WTYPE_CLOUD`.
pub const WTYPE_CLOUD: u8 = 1;
/// `WTYPE_WATER`.
pub const WTYPE_WATER: u8 = 2;

// Attack-range codes (`AR_*`, the values of `defineAttribute.h` that `propItem.txt`/`propSkill.txt` columns carry;
// `get_attack_range` maps a code to metres). ref/12 §5.1 lists the METRES (2, 3, 4, 10, 15, 6, 18) next to the names;
// those are not the codes (integration B4: the sim used the metres as codes, so a bow's `AR_RANGE` 4 meant 4 m).

/// `AR_SHORT` (2 m).
pub const AR_SHORT: u32 = 1;
/// `AR_LONG` (3 m).
pub const AR_LONG: u32 = 2;
/// `AR_FAR` (4 m).
pub const AR_FAR: u32 = 3;
/// `AR_RANGE` (10 m).
pub const AR_RANGE: u32 = 4;
/// `AR_WAND` (15 m).
pub const AR_WAND: u32 = 5;
/// `AR_HRANGE` (6 m).
pub const AR_HRANGE: u32 = 6;
/// `AR_HWAND` (18 m).
pub const AR_HWAND: u32 = 7;

/// Legacy `#define`s the sim reads. They live in `flyff_common::defs` once the generator of Plan/14
/// (WP-DATA-19) has run; until then the handful of values the sim needs are fixed here (verified against
/// `Resource/define*.h`, `_Common/authorization.h`).
pub mod defs {
    /// `AII_NONE`.
    pub const AII_NONE: u32 = 0;
    /// `AII_MOVER`.
    pub const AII_MOVER: u32 = 1;
    /// `AII_MONSTER`.
    pub const AII_MONSTER: u32 = 2;
    /// `AII_HUMAN`.
    pub const AII_HUMAN: u32 = 3;
    /// `AII_CLOCKWORKS`.
    pub const AII_CLOCKWORKS: u32 = 4;
    /// `AII_PET`.
    pub const AII_PET: u32 = 5;
    /// `AII_EGG`.
    pub const AII_EGG: u32 = 9;

    /// `IK3_HAND`: bare hands (`II_WEA_HAN_HAND`).
    pub const IK3_HAND: u32 = 1;
    /// `IK3_SWD`.
    pub const IK3_SWD: u32 = 2;
    /// `IK3_AXE`.
    pub const IK3_AXE: u32 = 3;
    /// `IK3_CHEERSTICK`.
    pub const IK3_CHEERSTICK: u32 = 4;
    /// `IK3_KNUCKLEHAMMER`.
    pub const IK3_KNUCKLEHAMMER: u32 = 5;
    /// `IK3_WAND`.
    pub const IK3_WAND: u32 = 6;
    /// `IK3_STAFF`.
    pub const IK3_STAFF: u32 = 7;
    /// `IK3_VIRTUAL`: the virtual items of monsters (`II_WEA_MOB_*`).
    pub const IK3_VIRTUAL: u32 = 10;
    /// `IK3_YOYO`.
    pub const IK3_YOYO: u32 = 11;
    /// `IK3_BOW`.
    pub const IK3_BOW: u32 = 12;

    /// `CHS_STUN` (`GetAdjParam(DST_CHRSTATE)` bit).
    pub const CHS_STUN: u32 = 0x0000_0008;
    /// `CHS_LOOT`: rooted (the "Root" guard of the message table).
    pub const CHS_LOOT: u32 = 0x0008_0000;
    /// `CHS_SETSTONE`.
    pub const CHS_SETSTONE: u32 = 0x0010_0000;

    /// `MATCHLESS_MODE` (`m_dwMode`).
    pub const MATCHLESS_MODE: u32 = 0x0000_0001;
    /// `TRANSPARENT_MODE`.
    pub const TRANSPARENT_MODE: u32 = 0x0000_0002;
    /// `NO_ATTACK_MODE`.
    pub const NO_ATTACK_MODE: u32 = 0x0000_0040;
    /// `STATE_BASEMOTION_MODE`.
    pub const STATE_BASEMOTION_MODE: u32 = 0x0000_0004;

    /// `RANK_NORMAL`.
    pub const RANK_NORMAL: u32 = 2;
    /// `RANK_MIDBOSS`.
    pub const RANK_MIDBOSS: u32 = 5;
    /// `RANK_MATERIAL`.
    pub const RANK_MATERIAL: u32 = 6;
    /// `RANK_SUPER`.
    pub const RANK_SUPER: u32 = 7;

    /// `MI_MALE`.
    pub const MI_MALE: u32 = 11;
    /// `MI_FEMALE`.
    pub const MI_FEMALE: u32 = 12;
    /// `MI_CLOCKWORK1`.
    pub const MI_CLOCKWORK1: u32 = 164;
    /// `MI_BIGMUSCLE`.
    pub const MI_BIGMUSCLE: u32 = 165;
    /// `MI_KRRR`.
    pub const MI_KRRR: u32 = 166;
    /// `MI_MUSHMOOT`.
    pub const MI_MUSHMOOT: u32 = 167;
    /// `MI_DWARPETMAS`.
    pub const MI_DWARPETMAS: u32 = 202;
    /// `MI_CROWNIBLIS`.
    pub const MI_CROWNIBLIS: u32 = 203;
    /// `MI_CROWNSHADE`.
    pub const MI_CROWNSHADE: u32 = 204;
    /// `MI_CROWNBUBBLE`.
    pub const MI_CROWNBUBBLE: u32 = 205;
    /// `MI_DU_METEONYKER` (the red one; render-audio-m7: the client hit count of the boss swings).
    pub const MI_DU_METEONYKER: u32 = 715;
    /// `MI_DU_METEONYKER2`.
    pub const MI_DU_METEONYKER2: u32 = 753;
    /// `MI_DU_METEONYKER3`.
    pub const MI_DU_METEONYKER3: u32 = 754;
    /// `MI_DU_METEONYKER4`.
    pub const MI_DU_METEONYKER4: u32 = 755;
    /// `MI_LOADCLOCKWORK`.
    pub const MI_LOADCLOCKWORK: u32 = 822;
    /// `MI_NPC_SNOWMAN01`.
    pub const MI_NPC_SNOWMAN01: u32 = 1029;
    /// `MI_NPC_SNOWMAN02`.
    pub const MI_NPC_SNOWMAN02: u32 = 1030;

    /// `SI_BLD_DOUBLESW_SILENTSTRIKE`.
    pub const SI_BLD_DOUBLESW_SILENTSTRIKE: u32 = 138;
    /// `SI_BIL_KNU_BELIALSMESHING`.
    pub const SI_BIL_KNU_BELIALSMESHING: u32 = 152;
    /// `SI_ACR_YOYO_COUNTER`.
    pub const SI_ACR_YOYO_COUNTER: u32 = 204;
    /// `SI_KNT_HERO_DRAWING`.
    pub const SI_KNT_HERO_DRAWING: u32 = 238;

    /// `JOB_PSYCHIKEEPER` (the master and hero jobs follow the base job; see `psy_job` in the stance).
    pub const JOB_PSYCHIKEEPER: u32 = 12;

    /// Object types (`OT_*`, `Resource/define.h`).
    pub const OT_OBJ: u32 = 0;
    /// `OT_ANI`.
    pub const OT_ANI: u32 = 1;
    /// `OT_CTRL`.
    pub const OT_CTRL: u32 = 2;
    /// `OT_SFX`.
    pub const OT_SFX: u32 = 3;
    /// `OT_ITEM`.
    pub const OT_ITEM: u32 = 4;
    /// `OT_MOVER`.
    pub const OT_MOVER: u32 = 5;
}

/// Mover class of a [`crate::mover::SimMoverParams`] (`kind`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum MoverKind {
    /// A player character.
    #[default]
    Player,
    /// A monster.
    Monster,
    /// A non-hostile NPC.
    Npc,
    /// A pet.
    Pet,
}
