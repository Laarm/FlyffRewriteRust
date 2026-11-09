//! The generic monster AI, `CAIMonster` (Plan/18 §9, ref/12 §9; WP-AI-08, WP-AI-09): the state map IDLE, STAND
//! and RAGE with their messages, wandering, return-home, the chase with its leashes and the target switching.
//! `src:_AIInterface/AIMonster.cpp`.
//!
//! ```text
//!   spawn ──► IDLE / STAND ──scan finds a player, or Damage──► RAGE ──target gone / leash / flight / safety──┐
//!                ▲                                                                                          │
//!                └────────────────────────────── SetState(base) ◄──────────────────────────────────────────┘
//! ```
//!
//! The per-mover state is [`MonsterAi`]; it lives in the [`AiSlot`](crate::ai::AiSlot) of `MonsterExt.ai`. A
//! handler runs with the brain taken out of the slot (so it can hold the mover mutably next to its own
//! state) and recurses through [`MonsterAi::send`], which enforces the depth limit of 18 §8.3 rule 4.
//!
//! | file | content |
//! |---|---|
//! | `idle.rs` | `StateIdle`, `MoveProcessIdle` |
//! | `stand.rs` | `StateStand`, `MoveProcessStand` |
//! | `rage.rs` | `StateRage`, `MoveProcessRage` (WP-AI-09) |
//! | `helpers.rs` | the shared movement helpers of 18 §8.8, return-home, HP, target views |
//! | `attack.rs` | `SelectAttackType`, `GetAtkMethod_Near/Far`, `DoAttack`, `SubAttackChance`, the Recovery AI (WP-AI-10) |
//! | `runaway.rs`, `summon.rs` | `StateRunaway`; `CallHelper` and `SubSummonProcess` (WP-AI-11) |

pub mod attack;
pub mod helpers;
pub mod idle;
pub mod rage;
pub mod runaway;
pub mod stand;
pub mod summon;

use super::ctx::AiCx;
use super::msg::AiMsg;
use crate::mover::Mover;
use flyff_common::math::Vec3;
use flyff_common::{GameMs, ObjId};

/// `RANGE_MOVE`: the wander radius around the spawn point (x3 for flying NPCs).
pub const RANGE_MOVE: f32 = 30.0;
/// `RANGE_RETURN_TO_BEGIN`: the leash: `RANGE_MOVE + 120` from the spawn point (150 m, 210 m flying) and 120 m
/// from the position of the last hit taken.
pub const RANGE_RETURN_TO_BEGIN: f32 = 120.0;
/// `s_tmAttack` (`CHASE_TIMER`): the stuck-check period and the "aggro refresh" after a hit, 15 s.
pub const CHASE_TIMER_MS: u64 = 15_000;
/// The speed factor of the chase (`MoveToDst(id)`).
pub const SPEED_FACTOR_CHASE: f32 = 2.0;
/// The speed factor of the way home (`DoReturnToBegin`).
pub const SPEED_FACTOR_RETURN: f32 = 2.66;
/// A return in IDLE is complete within this distance (xz) of the spawn point.
pub const RETURN_DONE_RANGE_IDLE: f32 = 7.0;
/// A return in STAND is complete within this distance (xz) of the spawn point.
pub const RETURN_DONE_RANGE_STAND: f32 = 0.1;
/// IDLE warps home when a return takes this long.
pub const RETURN_WARP_AFTER_IDLE_MS: u64 = 20_000;
/// STAND warps home when a return takes this long (`__AI_0905` is off).
pub const RETURN_WARP_AFTER_STAND_MS: u64 = 10_000;
/// A monster that moved at most this far (xz) in `CHASE_TIMER_MS` while chasing is stuck.
pub const STUCK_MOVE_MAX: f32 = 2.0;
/// The `ScanTargetNext` range (heightmap units) of the open world.
pub const NEXT_SCAN_RANGE: i32 = 15;
/// The `ScanTargetNext` range of instance dungeons.
pub const NEXT_SCAN_RANGE_INSTANCE: i32 = 160;
/// The recursion limit of `send` (18 §8.3 rule 4).
pub const MAX_DEPTH: u8 = 8;

/// The states of `CAIMonster` (`STATE_*`, `AIMonster.cpp:30-56`). WANDER and EVADE have empty handlers, PURSUE is
/// not in the state map; all of them ignore every message.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MonsterState {
    /// `STATE_INIT`: the initial state; ignores everything until `InitAI` switches it.
    #[default]
    Init = 1,
    /// `STATE_IDLE`: wander around the spawn point, scan for targets.
    Idle = 2,
    /// `STATE_WANDER` (empty).
    Wander = 3,
    /// `STATE_PURSUE` (unmapped).
    Pursue = 4,
    /// `STATE_EVADE` (empty).
    Evade = 5,
    /// `STATE_RUNAWAY`: flee below `ex.runaway_hp` (unused in V15 data; WP-AI-11).
    Runaway = 6,
    /// `STATE_RAGE`: chase and attack the target.
    Rage = 7,
    /// `STATE_STAND`: stay at the spawn spot and angle, scan for targets.
    Stand = 8,
    /// `STATE_PATROL` (no data; mapped to STAND at load, 18 §9.13).
    Patrol = 9,
    /// `STATE_RAGE_PATROL` (no data).
    RagePatrol = 10,
}

impl MonsterState {
    /// The state of a legacy id; `None` for an id outside `1..=10`.
    pub const fn from_u32(v: u32) -> Option<MonsterState> {
        Some(match v {
            1 => MonsterState::Init,
            2 => MonsterState::Idle,
            3 => MonsterState::Wander,
            4 => MonsterState::Pursue,
            5 => MonsterState::Evade,
            6 => MonsterState::Runaway,
            7 => MonsterState::Rage,
            8 => MonsterState::Stand,
            9 => MonsterState::Patrol,
            10 => MonsterState::RagePatrol,
            _ => return None,
        })
    }

    /// The base state a respawn line's `m_dwAiState` stands for: 2 IDLE, 8 STAND, 9 PATROL -> STAND (v1 has no
    /// patrol, 18 §9.13); anything else is IDLE.
    pub const fn base_of(line_state: u32) -> MonsterState {
        match line_state {
            8 | 9 => MonsterState::Stand,
            _ => MonsterState::Idle,
        }
    }
}

/// The per-monster state of `CAIMonster` (18 §9.1 `MonsterAi`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MonsterAi {
    /// The current state handler (`m_pStateFunc`).
    pub state: MonsterState,
    /// The base state (`m_dwAIInterfaceState`): IDLE or STAND.
    pub base_state: MonsterState,
    /// `InitAI` ran.
    pub inited: bool,
    /// `m_vPosBegin`: the spawn position.
    pub spawn_pos: Vec3,
    /// `m_fAngleBegine`: the spawn angle.
    pub spawn_angle: f32,
    /// `m_bReturnToBegin`.
    pub returning: bool,
    /// `m_tmReturnToBegin`.
    pub tm_return: GameMs,
    /// `m_dwIdTarget` (`None` = `NULL_ID`).
    pub target: Option<ObjId>,
    /// `m_uParty`: the party of the target at acquisition (0 = none).
    pub target_party: u32,
    /// `m_tmAttack`: `now + 15 s`, the stuck check and the aggro refresh.
    pub tm_attack: GameMs,
    /// `m_tmAttackDelay`.
    pub tm_attack_delay: GameMs,
    /// `m_tmMove`: stopped until.
    pub tm_move: GameMs,
    /// `m_vOldPos`: the stuck reference.
    pub old_pos: Vec3,
    /// `m_vPosDamage` (`None` = the all-zero sentinel).
    pub pos_damage: Option<Vec3>,
    /// `m_bGoTarget`.
    pub go_target: bool,
    /// `m_bRangeAttack`.
    pub range_attack: bool,
    /// `m_dwAtkMethod`.
    pub atk_method: u8,
    /// `m_bTargetNoMovePos`.
    pub target_no_move_pos: bool,
    /// `m_bFirstRunaway`.
    pub first_runaway: bool,
    /// `m_bCallHelper`.
    pub call_helper_done: bool,
    /// `m_tmHelp` (`None` = 0, "call now").
    pub tm_help: Option<GameMs>,
    /// `m_tmSummon`.
    pub tm_summon: GameMs,
    /// `m_vecIdSummon`.
    pub summons: Vec<ObjId>,
    /// `m_dwTickRecovery`: the next self-heal of a running-away monster (RUNAWAY, every 2 s).
    pub tm_recovery: GameMs,
    /// The `attack_first_range` error was logged for this monster (FIX: once, not per tick).
    pub bad_range_logged: bool,
    /// The "attack item does not exist" error was logged for this monster (once, not per tick).
    pub bad_atk_logged: bool,
    /// The recursion depth of [`MonsterAi::send`].
    pub depth: u8,
}

impl MonsterAi {
    /// A fresh brain in `STATE_INIT`; `InitAI` fills the rest on the first processed tick.
    pub fn new() -> MonsterAi {
        MonsterAi::default()
    }

    /// `InitAI` (18 §9.3): records the spawn position and angle, sets the `Init()` timers and switches to the
    /// base state `line_state` (the respawn line's `m_dwAiState`). Idempotent.
    pub fn init_ai(&mut self, m: &mut Mover, cx: &mut AiCx<'_>, line_state: u32) {
        if self.inited {
            return;
        }
        self.inited = true;
        let now = cx.now;
        self.spawn_pos = m.sim.pos;
        self.spawn_angle = m.sim.angle;
        self.base_state = MonsterState::base_of(line_state);
        if line_state == MonsterState::Patrol as u32 {
            // v1 has no patrol paths (all `.pat` files are empty, 18 §9.13): the monster stands instead
            tracing::warn!(id = ?m.id(), "AI line state PATROL is not supported: using STAND");
        }
        // `Init()`: no target, 15 s of chase credit, every other timer starts now, no help call yet
        self.target = None;
        self.target_party = 0;
        self.returning = false;
        self.tm_attack = GameMs(now.0 + CHASE_TIMER_MS);
        self.tm_attack_delay = now;
        self.tm_move = now;
        self.tm_return = now;
        self.tm_summon = now;
        self.tm_help = None;
        self.old_pos = Vec3::ZERO;
        self.pos_damage = None;
        let base = self.base_state;
        self.send(
            m,
            cx,
            AiMsg::SetState {
                state: base as u32,
                param: None,
            },
        );
    }

    /// `SendAIMsg`: synchronous delivery. `SetState` switches the handler and delivers `Init{state, param}` to the
    /// new one in the same call; everything else goes to the current handler. A message that nests deeper than
    /// [`MAX_DEPTH`] is dropped with a warning (FIX: guards against a loop that V15 logic never builds).
    pub fn send(&mut self, m: &mut Mover, cx: &mut AiCx<'_>, msg: AiMsg) {
        if self.depth >= MAX_DEPTH {
            tracing::warn!(id = ?m.id(), ?msg, "AI message dropped: recursion limit");
            return;
        }
        self.depth += 1;
        match msg {
            AiMsg::SetState { state, param } => {
                // an id without a handler leaves `m_pStateFunc` NULL: every later message is ignored
                self.state = MonsterState::from_u32(state).unwrap_or(MonsterState::Init);
                self.dispatch(m, cx, AiMsg::Init { state, param });
            }
            other => self.dispatch(m, cx, other),
        }
        self.depth -= 1;
    }

    fn dispatch(&mut self, m: &mut Mover, cx: &mut AiCx<'_>, msg: AiMsg) {
        match self.state {
            MonsterState::Idle => idle::handle(self, m, cx, msg),
            MonsterState::Stand => stand::handle(self, m, cx, msg),
            MonsterState::Rage => rage::handle(self, m, cx, msg),
            MonsterState::Runaway => runaway::handle(self, m, cx, msg),
            // INIT / WANDER / PURSUE / EVADE / PATROL / RAGE_PATROL: empty or unmapped
            _ => {}
        }
    }
}
