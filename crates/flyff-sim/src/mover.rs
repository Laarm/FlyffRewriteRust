//! The per-mover simulation state (Plan/31 §5; WP-SIM-04): [`SimMover`], the `CActionMover` port
//! [`ActionMover`], destinations, the inputs the owner writes ([`SimMoverParams`]) and the attack message
//! queues.
//!
//! `SimMover` is embedded in `flyff_game::mover::Mover` on the server and in `SimWorld` in the browser.
//! Everything the sim needs from the game (speeds, flags, equipment-derived motion offsets, bounds) is an
//! input written by the owner through [`SimMoverParams`]; nothing is derived inside the sim.

use crate::anim::clock::MotionClock;
use crate::anim::motion_table::{ClipId, ModelKey};
use crate::collision::geom::Aabb;
use crate::num::Vec3;
use crate::types::{
    MoverFlags, MoverKind, NULL_ID, OBJACT_NONE, OBJMSG_STAND, ObjMsg, ObjState, StateFlags, defs,
};
use flyff_common::ObjId;

/// The null object id used for "no destination object" and "no target".
pub const NO_OBJ: ObjId = ObjId::NONE;

/// `MAX_ACTMSG`: capacity of the attack message queues.
pub const MAX_ACTMSG: usize = 0xFF;

/// Weapon in the active hand, for the motion offsets of `SetMotion` (Plan/31 §12.3).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct WeaponStance {
    /// `ItemProp.dwItemKind3` (`IK3_*`).
    pub kind3: u32,
    /// `dwHanded == HD_TWO`.
    pub two_handed: bool,
    /// `IsDualWeapon()`.
    pub dual: bool,
    /// The mover is one of the master models (`MI_CROWNIBLIS`, `MI_CROWNSHADE`, `MI_CROWNBUBBLE`,
    /// `MI_DWARPETMAS`).
    pub master_model: bool,
    /// The job is `JOB_PSYCHIKEEPER` (or its master/hero): wands use `MTI_STAND_11`.
    pub psy_job: bool,
}

impl WeaponStance {
    /// `II_WEA_HAN_HAND` (`IK3_HAND`): the item of a player without a weapon (`GetActiveHandItemProp`).
    /// `IK3_HAND` is in none of the offset cases of `SetMotion`, so a bare-handed player gets the default
    /// `+200` (Plan/31 §12.3).
    pub const BARE_HANDS: WeaponStance = WeaponStance {
        kind3: defs::IK3_HAND,
        two_handed: false,
        dual: false,
        master_model: false,
        psy_job: false,
    };

    /// `IK3_VIRTUAL`: the virtual item (`dwAtk1`, `II_WEA_MOB_*_ATK1`) every monster and NPC holds
    /// (`propMover.txt`/`propItem.txt`: all 278 `II_WEA_MOB_*` items are `IK3_VIRTUAL`). It is in none of
    /// the offset cases either and the mover is not a player, so monsters get `+0`.
    pub const VIRTUAL_ITEM: WeaponStance = WeaponStance {
        kind3: defs::IK3_VIRTUAL,
        two_handed: false,
        dual: false,
        master_model: false,
        psy_job: false,
    };
}

/// The ridden item of a flying player (`PARTS_RIDE`).
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct RideParams {
    /// Item index.
    pub item: u32,
    /// `fFlightSpeed`.
    pub flight_speed: f32,
    /// `fLRAngle`: turn rate (degrees per frame).
    pub lr_angle: f32,
    /// `fTBAngle`: pitch rate (degrees per frame).
    pub tb_angle: f32,
    /// `dwUseMotion`: base of the fly motions (`+ MTA_*`).
    pub use_motion: u32,
    /// `dwFFuelReMax`.
    pub f_fuel_re_max: i32,
}

/// Inputs written by the owner, never derived inside the sim (Plan/31 §5.4).
#[derive(Clone, Debug, PartialEq)]
pub struct SimMoverParams {
    /// Class of the mover.
    pub kind: MoverKind,
    /// `m_dwIndex` (`MI_*`).
    pub mover_index: u32,
    /// `dwAI` (`AII_*`).
    pub ai: u32,
    /// `OBJTYPE_FLYING` (prop.flying == 1 and not a player): ground tables with 3-D moves.
    pub flying_npc: bool,
    /// `dwClass`.
    pub rank: u32,
    /// `bKillable`.
    pub killable: bool,
    /// `m_pActMover->m_fSpeed` (prop.speed; players 0.1).
    pub base_speed: f32,
    /// `m_fSpeedFactor` (1.0 normal, AI 2.0 chasing, 2.66 returning).
    pub speed_factor: f32,
    /// `GetAdjParam(DST_SPEED)` percent.
    pub speed_adj_pct: i32,
    /// `GetChgParam(DST_SPEED) != 0x7FFFFFFF`: rooted.
    pub speed_chg_set: bool,
    /// `GetParam(DST_JUMPING, 0)`.
    pub jumping: i32,
    /// `GetAdjParam(DST_CHRSTATE)` (`CHS_*`).
    pub chr_state: u32,
    /// `m_dwMode` (`MATCHLESS_MODE`, `NO_ATTACK_MODE`, ...).
    pub mode: u32,
    /// `IsStateMode(STATE_BASEMOTION_MODE)`.
    pub base_motion_mode: bool,
    /// `GetAttackSpeed()` (0.1-2.0).
    pub attack_speed: f32,
    /// `GetCastingAniSpeed()` (0.1-2.0).
    pub casting_ani_speed: f32,
    /// `MoverProp.frame` (the loader maps -1 to 1.0).
    pub frame_factor: f32,
    /// `m_nHitPoint > 0`.
    pub hp_positive: bool,
    /// Character level (combo requirements).
    pub level: u32,
    /// Weapon of the active hand (`None` = no item prop).
    pub stance: Option<WeaponStance>,
    /// The ridden item.
    pub ride: Option<RideParams>,
    /// The motion model (`OT_MOVER`, index).
    pub model_key: ModelKey,
    /// The model AABB (`m_pModel->m_vMin/m_vMax`; the union of the parts for players).
    pub bounds: Aabb,
    /// `m_vScale.x`.
    pub scale: f32,
    /// `DST_HAWKEYE_RATE` (attack range multiplier).
    pub hawkeye_rate: i32,
    /// `HasBuffByIk3(IK3_TEXT_DISGUISE)` (client replaces skill motions by `MTI_ATK1`).
    pub disguised: bool,
    /// `MoverProp.flying` (walk factor).
    pub prop_flying: u32,
    /// `AII_PET`/`AII_EGG`.
    pub is_pet: bool,
    /// The followed mover (`m_idDest`) of a pet.
    pub pet_owner: ObjId,
}

impl SimMoverParams {
    /// Deterministic defaults for a mover of `kind`: a player at the standard speed `0.1`, monsters at
    /// `0.075`, the weapon stance of an unarmed player (`BARE_HANDS`) or of a monster (`VIRTUAL_ITEM`),
    /// everything else neutral. A mover is steppable and can fight as soon as it is added.
    pub fn default_for(kind: MoverKind) -> SimMoverParams {
        let (ai, base_speed, mover_index) = match kind {
            MoverKind::Player => (defs::AII_NONE, 0.1, defs::MI_MALE),
            MoverKind::Monster => (defs::AII_MONSTER, 0.075, 0),
            MoverKind::Npc => (defs::AII_MOVER, 0.075, 0),
            MoverKind::Pet => (defs::AII_PET, 0.1, 0),
        };
        SimMoverParams {
            kind,
            mover_index,
            ai,
            flying_npc: false,
            rank: defs::RANK_NORMAL,
            killable: true,
            base_speed,
            speed_factor: 1.0,
            speed_adj_pct: 0,
            speed_chg_set: false,
            jumping: 0,
            chr_state: 0,
            mode: 0,
            base_motion_mode: false,
            attack_speed: 1.0,
            casting_ani_speed: 1.0,
            frame_factor: 1.0,
            hp_positive: true,
            level: 1,
            // every mover holds an item in the original (`II_WEA_HAN_HAND` when a player is unarmed, the
            // virtual item `dwAtk1` for everything else); `None` is the error case of `SetMotion` and
            // leaves the motion unchanged, so it is never a default
            stance: Some(if kind == MoverKind::Player {
                WeaponStance::BARE_HANDS
            } else {
                WeaponStance::VIRTUAL_ITEM
            }),
            ride: None,
            model_key: ModelKey {
                ot: defs::OT_MOVER,
                index: mover_index,
            },
            bounds: Aabb {
                min: Vec3::new(-0.5, 0.0, -0.5),
                max: Vec3::new(0.5, 1.8, 0.5),
            },
            scale: 1.0,
            hawkeye_rate: 0,
            disguised: false,
            prop_flying: 0,
            is_pet: kind == MoverKind::Pet,
            pet_owner: NO_OBJ,
        }
    }
}

/// A destination (`m_vDestPos`/`m_idDest`, Plan/31 §5.3).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Dest {
    /// Destination position; `x == 0 && z == 0` means none (`IsEmptyDestPos`).
    pub pos: Vec3,
    /// Destination object; `NULL_ID` = none.
    pub obj: ObjId,
    /// `m_fArrivalRange`.
    pub arrival_range: f32,
    /// `m_bForward`.
    pub forward: bool,
    /// `m_bPositiveX`: the sign of `pos.x - dest.x` when the destination was set.
    pub positive_x: bool,
    /// `m_bPositiveZ`.
    pub positive_z: bool,
}

impl Default for Dest {
    fn default() -> Self {
        Dest {
            pos: Vec3::ZERO,
            obj: NO_OBJ,
            arrival_range: 0.0,
            forward: true,
            positive_x: false,
            positive_z: false,
        }
    }
}

/// Scripted move-pattern state (`m_nMovePattern`, `m_nMoveEvent`, `m_nMoveEventCnt`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct MovePatternState {
    /// Pattern id (0 = none).
    pub pattern: i32,
    /// Event index within the pattern.
    pub event: i32,
    /// Frames in the current event.
    pub cnt: i32,
}

/// Context of the running skill (written by doc 17 before sending MELEESKILL/MAGICSKILL/MAGICCASTING).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SkillCtx {
    /// Skill id.
    pub skill: u32,
    /// Skill level.
    pub level: u32,
    /// Target.
    pub target: ObjId,
    /// `AddSkillProp.nSkillCount`.
    pub skill_count: i32,
    /// `OBJACT_*`.
    pub act: u32,
    /// `m_dwCastingEndTick` in frames.
    pub casting_end_frame: u64,
    /// `m_nCastingSKillID`.
    pub casting_skill: u32,
}

impl Default for SkillCtx {
    fn default() -> Self {
        SkillCtx {
            skill: 0,
            level: 0,
            target: NO_OBJ,
            skill_count: 0,
            act: OBJACT_NONE,
            casting_end_frame: 0,
            casting_skill: 0,
        }
    }
}

/// `m_oaCmd` of the active mover (client).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ActiveCmd {
    /// `OBJACT_*`.
    pub act: u32,
    /// `GetCmdParam(0..3)`.
    pub params: [i32; 3],
}

impl Default for ActiveCmd {
    fn default() -> Self {
        ActiveCmd {
            act: OBJACT_NONE,
            params: [0; 3],
        }
    }
}

/// Inputs of the client's own player that the state machine reads (written by the local control layer,
/// Plan/31 §15.2; all-default on remote movers and on the server).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LocalInput {
    /// The attack button is held (`CTRLMSG_LDOWN` / `m_bLButtonDown`).
    pub lmb_held: bool,
    /// Auto-attack is on (`m_bAutoAttack`).
    pub auto_attack: bool,
    /// No skill is queued on the taskbar (`GetNextSkill() == NEXTSKILL_NONE`).
    pub next_skill_none: bool,
}

impl Default for LocalInput {
    fn default() -> Self {
        LocalInput {
            lmb_held: false,
            auto_attack: false,
            next_skill_none: true,
        }
    }
}

/// Client-only render smoothing state (`CMover::Interpolate`, Plan/31 §15.5; unused on the server).
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct RenderSmooth {
    /// Smoothed position (`m_vScrPos`).
    pub scr_pos: Vec3,
    /// Smoothed yaw.
    pub scr_angle: f32,
    /// Smoothed pitch.
    pub scr_angle_x: f32,
    /// Previous-frame position for display interpolation.
    pub prev_pos: Vec3,
    /// Previous-frame yaw.
    pub prev_angle: f32,
}

/// A queued melee attack message (`CMeleeAtkMsgq`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QueuedAct {
    /// `OBJMSG_*`.
    pub msg: ObjMsg,
    /// Parameter 1 (target).
    pub p1: i32,
    /// Parameter 2.
    pub p2: i32,
    /// Parameter 3.
    pub p3: i32,
}

/// Kind of a queued magic/range attack.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MagicKind {
    /// A wand attack with its power (`nParam2 == 0`).
    Magic {
        /// `nMagicPower`.
        power: i32,
    },
    /// A ranged attack with its item (`nParam2 != 0`).
    Range {
        /// Item index.
        item: i32,
    },
}

/// A queued magic/range attack (`CMagicAtkMsgq`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QueuedMagic {
    /// Target.
    pub target: ObjId,
    /// Kind and its parameter.
    pub kind: MagicKind,
    /// `idSfxHit`.
    pub sfx_hit: u32,
}

/// A fixed-capacity ring buffer that drops new entries when full (`MAX_ACTMSG`).
#[derive(Clone, Debug, PartialEq)]
pub struct RingQueue<T, const N: usize> {
    items: std::collections::VecDeque<T>,
}

impl<T, const N: usize> Default for RingQueue<T, N> {
    fn default() -> Self {
        RingQueue {
            items: std::collections::VecDeque::new(),
        }
    }
}

impl<T: Copy, const N: usize> RingQueue<T, N> {
    /// Pushes at the tail; returns `false` (and drops the entry) when the queue is full.
    pub fn push(&mut self, v: T) -> bool {
        if self.items.len() >= N {
            return false;
        }
        self.items.push_back(v);
        true
    }
    /// The head entry.
    pub fn head(&self) -> Option<T> {
        self.items.front().copied()
    }
    /// Removes the head entry.
    pub fn pop(&mut self) -> Option<T> {
        self.items.pop_front()
    }
    /// `true` when empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    /// Number of queued entries.
    pub fn len(&self) -> usize {
        self.items.len()
    }
    /// Removes every entry.
    pub fn clear(&mut self) {
        self.items.clear();
    }
}

/// The attack message queues of one mover (Plan/31 §6.6).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ActMsgQueues {
    /// Queued melee attack messages.
    pub melee: RingQueue<QueuedAct, MAX_ACTMSG>,
    /// Queued magic/range attacks.
    pub magic: RingQueue<QueuedMagic, MAX_ACTMSG>,
}

/// The `CActionMover` port: state word, flags and per-frame velocities (Plan/31 §5.1).
#[derive(Clone, Debug, PartialEq)]
pub struct ActionMover {
    /// `m_dwState`.
    pub state: ObjState,
    /// `m_dwStateFlag`.
    pub flags: StateFlags,
    /// `m_vDelta`: per-frame velocity.
    pub delta: Vec3,
    /// `m_vDeltaE`: external force (knock-back, dash).
    pub delta_e: Vec3,
    /// `m_vDeltaAccu`: the movement of this frame (one frame; the server's 4-frame accumulation is gone).
    pub delta_accu: Vec3,
    /// `m_vPosLast`: the position after the last collision pass.
    pub pos_last: Vec3,
    /// `m_bGround`: 0 airborne, 1 standing, 2 touching while sliding.
    pub ground: u8,
    /// `m_bMove`.
    pub moving: bool,
    /// Flying `m_fAccPower`.
    pub acc_power: f32,
    /// Flying `m_fTurnAngle`.
    pub turn_angle: f32,
    /// `m_fDistance`.
    pub distance: f32,
    /// `m_nCount` (frames).
    pub count: i32,
    /// `m_nCastingTime` (frames = `p3 * 4`).
    pub casting_time: i32,
    /// `m_nShootCnt`.
    pub shoot_cnt: i32,
    /// `m_nMotionHitCount`.
    pub motion_hit_count: i32,
    /// `m_nDeadCnt` (server NPC: 180 frames).
    pub dead_cnt: i32,
    /// `RideMissing` was raised for the current flight (the event fires once per mount).
    pub ride_missing_sent: bool,
    /// `m_fCurrentHeight`.
    pub current_height: f32,
    /// `m_dwAtkFlags`.
    pub atk_flags: u32,
    /// `m_idTarget`.
    pub id_target: ObjId,
    /// `m_objidHit`.
    pub objid_hit: ObjId,
    /// `m_nMotionEx`.
    pub motion_ex: i32,
    /// The attack message queues.
    pub queues: ActMsgQueues,
}

impl Default for ActionMover {
    fn default() -> Self {
        ActionMover {
            state: ObjState(0),
            flags: StateFlags(0),
            delta: Vec3::ZERO,
            delta_e: Vec3::ZERO,
            delta_accu: Vec3::ZERO,
            pos_last: Vec3::ZERO,
            ground: 0,
            moving: false,
            acc_power: 0.0,
            turn_angle: 0.0,
            distance: 0.0,
            count: 0,
            casting_time: 0,
            shoot_cnt: 0,
            motion_hit_count: 0,
            dead_cnt: 0,
            ride_missing_sent: false,
            current_height: 0.0,
            atk_flags: 0,
            id_target: NO_OBJ,
            objid_hit: NO_OBJ,
            motion_ex: -1,
            queues: ActMsgQueues::default(),
        }
    }
}

/// Blend state of the last motion change (`m_bMotionBlending`/`m_fBlendWeight`).
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Blend {
    /// Blending between the old and the new motion.
    pub active: bool,
    /// Weight of the new motion (`+0.05` per frame, 20 frames per transition).
    pub weight: f32,
}

/// The current motion (Plan/31 §12.2).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct MotionState {
    /// `m_dwMotion`: the UN-OFFSET motion id (`u32::MAX` = -1); this is what the wire carries.
    pub id: u32,
    /// The offset id actually loaded (for render/debug).
    pub resolved: u32,
    /// `m_dwMotionOption`.
    pub option: u32,
    /// `m_pMotion` (`None` = the clip file is absent).
    pub clip: Option<ClipId>,
    /// `m_pMotionOld`.
    pub prev_clip: Option<ClipId>,
    /// `m_nFrameOld`.
    pub prev_frame: i32,
    /// The frame clock.
    pub clock: MotionClock,
    /// Blend state.
    pub blend: Blend,
}

impl Default for MotionState {
    fn default() -> Self {
        MotionState {
            id: u32::MAX,
            resolved: u32::MAX,
            option: 0,
            clip: None,
            prev_clip: None,
            prev_frame: 0,
            clock: MotionClock::new(),
            blend: Blend::default(),
        }
    }
}

/// Everything the simulation keeps per mover (Plan/31 §5.1).
#[derive(Clone, Debug, PartialEq)]
pub struct SimMover {
    /// Object id.
    pub id: ObjId,
    /// Position.
    pub pos: Vec3,
    /// Yaw in degrees (quantised, `[0, 360)`).
    pub angle: f32,
    /// Pitch in degrees (not wrapped; negative = nose up).
    pub angle_x: f32,
    /// The action mover.
    pub act: ActionMover,
    /// `CMover::m_dwFlag`.
    pub mvrf: MoverFlags,
    /// `m_wStunCnt`.
    pub stun_cnt: i16,
    /// Destination.
    pub dest: Dest,
    /// `m_fDestAngle`, `-1.0` = none.
    pub dest_angle: f32,
    /// `m_bLeft`.
    pub dest_left: bool,
    /// `m_dwMotionArrive`: the message sent on arrival (default `STAND`).
    pub motion_arrive: ObjMsg,
    /// Move pattern.
    pub pattern: MovePatternState,
    /// Motion state.
    pub motion: MotionState,
    /// `m_fAniSpeed`.
    pub ani_speed: f32,
    /// `m_nAtkCnt` (owner-incremented).
    pub atk_cnt: i32,
    /// `m_nFuel` (-1 = unset).
    pub fuel: i32,
    /// `m_tmAccFuel` (ms).
    pub tm_acc_fuel: i32,
    /// Running skill context.
    pub skill: SkillCtx,
    /// Active-mover command.
    pub cmd: ActiveCmd,
    /// Client active mover: `REQ_*` bits.
    pub req_flags: u32,
    /// Client active mover: held buttons.
    pub input: LocalInput,
    /// The client's own player.
    pub is_active: bool,
    /// The flight auto-tracking target of the active mover (`SET_TRACKING`; with `MVRF_TRACKING`; the
    /// steering itself is `ProcessFlyTracking`, WP-SIM-25).
    pub tracking: ObjId,
    /// Client remote mover whose last intent was older than `INTENT_FF_CAP` frames: it stays "lagging"
    /// until the next keyframe (`apply_intent`, Plan/31 §15.4, 11 §9.3; cleared by WP-SIM-16).
    pub lagging: bool,
    /// Inputs written by the game.
    pub params: SimMoverParams,
    /// Client render smoothing.
    pub render: RenderSmooth,
}

impl SimMover {
    /// `SimMover::new(id, pos, angle, params)`: a standing mover (`state` 0 steps into `STAND` on its first
    /// frame) with no destination.
    pub fn new(id: ObjId, pos: Vec3, angle: f32, params: SimMoverParams) -> SimMover {
        let act = ActionMover {
            pos_last: pos,
            ..ActionMover::default()
        };
        SimMover {
            id,
            pos,
            angle: crate::num::set_angle_quantized(angle),
            angle_x: 0.0,
            act,
            mvrf: MoverFlags(0),
            stun_cnt: 0,
            dest: Dest::default(),
            dest_angle: -1.0,
            dest_left: false,
            motion_arrive: OBJMSG_STAND,
            pattern: MovePatternState::default(),
            motion: MotionState::default(),
            ani_speed: 1.0,
            atk_cnt: 0,
            fuel: -1,
            tm_acc_fuel: 0,
            skill: SkillCtx::default(),
            cmd: ActiveCmd::default(),
            req_flags: 0,
            input: LocalInput::default(),
            is_active: false,
            tracking: NO_OBJ,
            lagging: false,
            params,
            render: RenderSmooth {
                scr_pos: pos,
                scr_angle: angle,
                scr_angle_x: 0.0,
                prev_pos: pos,
                prev_angle: angle,
            },
        }
    }

    /// `true` for player movers.
    #[inline]
    pub fn is_player(&self) -> bool {
        self.params.kind == MoverKind::Player
    }

    /// `IsEmptyDestPos()`: `dest.pos.x == 0 && dest.pos.z == 0`.
    #[inline]
    pub fn is_empty_dest_pos(&self) -> bool {
        self.dest.pos.x == 0.0 && self.dest.pos.z == 0.0
    }

    /// `IsEmptyDestObj()`.
    #[inline]
    pub fn is_empty_dest_obj(&self) -> bool {
        self.dest.obj.0 == NULL_ID
    }

    /// `IsEmptyDest()`.
    #[inline]
    pub fn is_empty_dest(&self) -> bool {
        self.is_empty_dest_pos() && self.is_empty_dest_obj()
    }

    /// `ClearDestPos()`.
    #[inline]
    pub fn clear_dest_pos(&mut self) {
        self.dest.pos = Vec3::ZERO;
    }

    /// `ClearDestObj()`.
    #[inline]
    pub fn clear_dest_obj(&mut self) {
        self.dest.obj = NO_OBJ;
        self.dest.arrival_range = 0.0;
    }

    /// `ClearDest()`: both.
    #[inline]
    pub fn clear_dest(&mut self) {
        self.clear_dest_obj();
        self.clear_dest_pos();
    }

    /// `SetDestPos(v, forward)`: records the sign of `pos - v` per axis (the arrival test of
    /// `process_move`) and clears the destination object. (The client's `DestPosReq` report for the active
    /// mover is raised by the caller.)
    pub fn set_dest_pos(&mut self, v: Vec3, forward: bool) {
        self.dest.pos = v;
        self.dest.forward = forward;
        let d = self.pos - v;
        self.dest.positive_x = d.x > 0.0;
        self.dest.positive_z = d.z > 0.0;
        self.clear_dest_obj();
    }

    /// `SetDestObj(id, range)`: clears the destination position (the client also resets the command).
    pub fn set_dest_obj(&mut self, id: ObjId, range: f32) {
        self.dest.obj = id;
        self.dest.arrival_range = range;
        self.clear_dest_pos();
        self.cmd.act = OBJACT_NONE;
    }

    /// `SetDestAngle(angle, left)`.
    #[inline]
    pub fn set_dest_angle(&mut self, a: f32, left: bool) {
        self.dest_angle = a;
        self.dest_left = left;
    }

    /// `ClearDestAngle()`: `dest_angle = -1`.
    #[inline]
    pub fn clear_dest_angle(&mut self) {
        self.dest_angle = -1.0;
    }
}
