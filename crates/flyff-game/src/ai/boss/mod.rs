//! The boss AIs (Plan/18 §10; `AIClockWorks.cpp`, `AIBigMuscle.cpp`, `AIKrrr.cpp`, `AIBear.cpp`): the common
//! skeleton of the four boss brains and everything ClockWorks-specific (task boss-ai-core, WP-AI-18); BigMuscle,
//! Krrr, Bear and Meteonyker are WP-AI-19 .. WP-AI-21 and fill the seam arms left in [`rage`] and [`hits`].
//!
//! ```text
//!   InitAI ──► RAGE ◄──── IDLE (Init / Damage forward to RAGE; bosses have no idle pattern)
//!                │
//!   Ready ─(75 ticks / AppearEnded)─► Moving ─(re-attack time, target, band table)─► Tracking ─Arrival─► Attacking
//!                                        ▲                                              │(3 s)             │
//!                                        └────────── EndMeleeAttack / swing over / 15 s ◄─┴──────────────────┘
//! ```
//!
//! | file | content |
//! |---|---|
//! | `mod.rs` | the enums and [`BossAi`], the brain's entry points ([`init_ai`], [`route`], [`send`]) and `AiSlot::boss()` |
//! | `tables.rs` | the pure decision tables: band table, arrival mapping, target roll, with the original draw order |
//! | `rage.rs` | `StateRage`, `MoveProcessRage`, `SelectTarget`, `CMD_SetMeleeAttack` and the four per-kind hook functions |
//! | `clockworks.rs` | the ClockWorks defence mode, its four hit handlers and the laser continuation |
//! | `hits.rs` | the hit dispatchers 17 calls (`OnAttackMelee`, `OnAttackMeleeContinue`), `OnDamage`, `GetEventPos` |
//! | `pose.rs` | [`pose::BossPoseSource`]: the rigs of the bosses and the frame a hit saw |
//! | `bigmuscle.rs`, `krrr.rs`, `bear.rs` | the openers, arrival shouts and hit handlers of BigMuscle, Krrr and the Bear / Mushmoot (boss-ai-b, WP-AI-19, -20) |
//! | `fx.rs` | [`fx::BossFx`] (announcements and skills that leave the acting mover), its runner, the hit-list executor (boss-ai-b) |
//!
//! **Brain in the slot.** As the monster brain, a boss brain lives in the [`AiSlot`] of the
//! mover and runs with the brain taken out (`crate::ai::with_boss_brain`), so a handler holds `&mut BossAi` next
//! to `&mut Mover`. The free functions [`init_ai`], [`route`] and [`send`] are the outside entry points that
//! `ai::{on_added, route_messages, send, post}` delegate to. The hit handlers run at the barrier, when the brain
//! is back in its slot (`AiSlot::boss_mut`).
//!
//! **Delivery.** `SetState` switches the handler and delivers `Init` to it in the same call (1 INIT is a deaf
//! handler, 3 IDLE, 4 RAGE and 5 SUPER_RAGE the RAGE handler). **FIX (spec-review ruling 28, owner question 2):**
//! a `SetState` to an id without a handler (2 APPEAR; 7 = `CAIMonster`'s RAGE, which `CallHelper` of a nearby
//! helper-calling monster sends to ANY mover) leaves the current handler in place; the original set
//! `m_pStateFunc = NULL` and the boss stood deaf until it died.

pub mod bear;
pub mod bigmuscle;
pub mod clockworks;
pub mod fx;
pub mod hits;
pub mod krrr;
pub mod pose;
pub mod rage;
pub mod tables;

use crate::ai::{AiBrain, AiCx, AiMsg, AiSlot, BossKind};
use crate::mover::Mover;
use flyff_common::math::Vec3;
use flyff_common::{GameMs, ObjId};

/// The recursion limit of [`BossAi::send`] (18 §8.3 rule 4, as `MonsterAi::send`).
pub const MAX_DEPTH: u8 = 8;

/// The boss states (`STATE_*` of the boss AIs): `SuperRage` runs the RAGE handler.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BossState {
    /// `STATE_INIT`.
    #[default]
    Init = 1,
    /// `STATE_APPEAR`.
    Appear = 2,
    /// `STATE_IDLE`.
    Idle = 3,
    /// `STATE_RAGE`.
    Rage = 4,
    /// `STATE_SUPER_RAGE` (the RAGE handler).
    SuperRage = 5,
}

impl BossState {
    /// The state of a legacy `STATE_*` id; `None` for an id outside `1..=5`.
    pub const fn from_u32(v: u32) -> Option<BossState> {
        Some(match v {
            1 => BossState::Init,
            2 => BossState::Appear,
            3 => BossState::Idle,
            4 => BossState::Rage,
            5 => BossState::SuperRage,
            _ => return None,
        })
    }

    /// `true` for the states that have a handler in the original's state map (`STATE_APPEAR` has none).
    pub const fn is_mapped(self) -> bool {
        !matches!(self, BossState::Appear)
    }
}

/// The phase of a boss in RAGE (`EPT_*`; `EPT_ATTACK_END` is dead code in the original).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BossEvent {
    /// `EPT_READY` 0: the appear motion plays.
    #[default]
    Ready,
    /// `EPT_MOVING` 1: waiting for the re-attack timer and choosing the next target and attack.
    Moving,
    /// `EPT_ATTACKING` 2: a swing is playing.
    Attacking,
    /// `EPT_TRACKING` 3 (Krrr 4): walking to the arrival range of the chosen attack.
    Tracking,
}

/// The attacks of the four bosses (`CAT_*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BossAttack {
    /// ClockWorks `CAT_SLASH`.
    Slash,
    /// ClockWorks `CAT_LASER`.
    Laser,
    /// ClockWorks `CAT_SHOOT`.
    Shoot,
    /// ClockWorks `CAT_QUAKE`.
    Quake,
    /// BigMuscle / Krrr / Bear `CAT_NORMAL`.
    Normal,
    /// BigMuscle `CAT_NORMAL2`.
    Normal2,
    /// BigMuscle `CAT_QUAKEDOUBLE`.
    QuakeDouble,
    /// BigMuscle `CAT_QUAKE_ONE`.
    QuakeOne,
    /// Krrr `CAT_DOUBLE_CRASH`.
    DoubleCrash,
    /// Krrr `CAT_SUMMONS`.
    Summons,
    /// Bear `CAT_LOOT`.
    Loot,
    /// Bear `CAT_BUFFTIME`.
    BuffTime,
}

/// How close a boss gets before it swings (`fArrivalRange`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Reach {
    /// The boss's own `GetRadiusXZ()` (the default of the original).
    Radius,
    /// A fixed distance in metres.
    Metres(f32),
}

impl Reach {
    /// The arrival range for a boss with the horizontal radius `radius`.
    pub fn metres(self, radius: f32) -> f32 {
        match self {
            Reach::Radius => radius,
            Reach::Metres(m) => m,
        }
    }
}

/// `Meteor` event of Krrr: a ten-strike meteor shower aimed at the player the opener picked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeteorEvent {
    /// `m_idEventTarget`.
    pub event_target: ObjId,
    /// Strikes left (starts at 10; one strike per `x_random(20) == 0`).
    pub remaining: u32,
}

/// The per-boss state of 18 §10.1: the brain of the four boss AIs (`CAIClockWorks`, `CAIBigMuscle`, `CAIKrrr`,
/// `CAIBear`), kept in the [`AiSlot`] of the mover.
#[derive(Clone, Debug, PartialEq)]
pub struct BossAi {
    /// Which boss.
    pub kind: BossKind,
    /// The state handler.
    pub state: BossState,
    /// The phase inside RAGE.
    pub event: BossEvent,
    /// `m_nAppearCnt`: ticks of the appear motion (75 = `PROCESS_COUNT x 5` ends it).
    pub appear_cnt: u32,
    /// `m_tmReAttack`: when the last attack ended.
    pub tm_reattack: GameMs,
    /// `m_tmAddReattack`: the random extra wait in ms (`x_random(2000)`, or 2000 after a stall).
    pub add_reattack_ms: u32,
    /// `m_tmTrace`: when the current chase began (3 s gives up).
    pub tm_trace: GameMs,
    /// `m_tmTimeOver`: when the last swing began (15 s gives up on it).
    pub tm_time_over: GameMs,
    /// `m_idLastAttacker`.
    pub last_attacker: Option<ObjId>,
    /// `m_idTarget`.
    pub target: Option<ObjId>,
    /// `m_nAttackType`.
    pub attack: Option<BossAttack>,
    /// `m_bEventFlag[0]`: the opener ran.
    pub opener_done: bool,
    /// Krrr: the meteor shower.
    pub meteor: Option<MeteorEvent>,
    /// ClockWorks: the defence mode was entered (`DST_ADJDEF_RATE +800`, Q20).
    pub defense_mode: bool,
    /// Bear: the next `BuffTime` (`CTimer` of 30 s).
    pub buff_due: GameMs,
    /// BigMuscle: the position of hit 0 of ATK4 (a function-static in the original, Q18).
    pub atk4_pos: Option<Vec3>,
    /// `InitAI` ran.
    pub inited: bool,
    /// `m_vPosBegin`: the spawn position.
    pub spawn_pos: Vec3,
    /// The recursion depth of [`BossAi::send`].
    pub depth: u8,
    /// The pose data of this boss was missing and logged (once per boss object, 18 §10.4 / open question 2).
    pub pose_warned: bool,
    /// An arrival with no (or an unknown) attack type was logged (once; the original's `Error`).
    pub bad_attack_logged: bool,
    /// A `SetState` to an id without a handler was logged (once, debug level).
    pub bad_state_logged: bool,
    /// The skill rows `(skill, level)` and texts `(tid, u32::MAX)` whose absence [`fx::run`] already logged for this
    /// boss (boss-ai-b: a missing row logs once).
    pub missing_logged: Vec<(u32, u32)>,
}

impl BossAi {
    /// A boss of `kind` before its first tick.
    pub fn new(kind: BossKind) -> BossAi {
        BossAi {
            kind,
            state: BossState::Init,
            event: BossEvent::Ready,
            appear_cnt: 0,
            tm_reattack: GameMs(0),
            add_reattack_ms: 0,
            tm_trace: GameMs(0),
            tm_time_over: GameMs(0),
            last_attacker: None,
            target: None,
            attack: None,
            opener_done: false,
            meteor: None,
            defense_mode: false,
            buff_due: GameMs(0),
            atk4_pos: None,
            inited: false,
            spawn_pos: Vec3::ZERO,
            depth: 0,
            pose_warned: false,
            bad_attack_logged: false,
            bad_state_logged: false,
            missing_logged: Vec::new(),
        }
    }
}

// ---- the brain (task boss-ai-core, WP-AI-18) --------------------------------------------------------------------

impl BossAi {
    /// `InitAI` (18 §10.1): records the spawn position, starts the timers (`Init()`: `m_tmReattack`,
    /// `m_tmTrace` and `m_tmTimeOver` are now; `m_tmAddReattack` = `timeGetTime()` makes
    /// `m_tmReattack + (2000 - m_tmAddReattack)` wrap to 2000, so the first re-attack check passes at once:
    /// kept as `add_reattack_ms = 2000`), runs the per-kind init ([`rage::init_extras`], before the state
    /// switch like the original's `Init()` / `InitAI`) and switches to RAGE. Idempotent.
    pub fn init_ai(&mut self, m: &mut Mover, cx: &mut AiCx<'_>) {
        if self.inited {
            return;
        }
        self.inited = true;
        let now = cx.now;
        self.spawn_pos = m.sim.pos;
        self.tm_reattack = now;
        self.tm_trace = now;
        self.tm_time_over = now;
        self.add_reattack_ms = rage::REATTACK_BASE_MS as u32;
        rage::init_extras(self, now);
        self.send(
            m,
            cx,
            AiMsg::SetState {
                state: BossState::Rage as u32,
                param: None,
            },
        );
    }

    /// `SendAIMsg`: synchronous delivery. `SetState` switches the handler and delivers `Init{state, param}` to
    /// the new one in the same call; everything else goes to the current handler. A `SetState` to an id without
    /// a handler leaves the current one in place (the FIX of the module docs). A message that nests deeper than
    /// [`MAX_DEPTH`] is dropped with a warning.
    pub fn send(&mut self, m: &mut Mover, cx: &mut AiCx<'_>, msg: AiMsg) {
        if self.depth >= MAX_DEPTH {
            tracing::warn!(id = ?m.id(), ?msg, "boss AI message dropped: recursion limit");
            return;
        }
        self.depth += 1;
        match msg {
            AiMsg::SetState { state, param } => {
                match BossState::from_u32(state).filter(|s| s.is_mapped()) {
                    Some(s) => {
                        self.state = s;
                        self.dispatch(m, cx, AiMsg::Init { state, param });
                    }
                    None => {
                        if !self.bad_state_logged {
                            self.bad_state_logged = true;
                            tracing::debug!(
                                id = ?m.id(),
                                state,
                                "boss AI: SetState to an unmapped state ignored (the original froze the boss)"
                            );
                        }
                    }
                }
            }
            other => self.dispatch(m, cx, other),
        }
        self.depth -= 1;
    }

    fn dispatch(&mut self, m: &mut Mover, cx: &mut AiCx<'_>, msg: AiMsg) {
        match self.state {
            BossState::Idle => idle(self, m, cx, msg),
            BossState::Rage | BossState::SuperRage => rage::handle(self, m, cx, msg),
            // STATE_INIT is a handler that does nothing; STATE_APPEAR is never entered (no handler)
            BossState::Init | BossState::Appear => {}
        }
    }
}

/// `StateIdle`: "bosses have no idle pattern": `Init` and `Damage` go straight to RAGE, `Die` exits; `Process`
/// is `MoveProcessIdle` (empty), the rest is ignored.
fn idle(b: &mut BossAi, m: &mut Mover, cx: &mut AiCx<'_>, msg: AiMsg) {
    match msg {
        AiMsg::Init { .. } | AiMsg::Damage { .. } => b.send(
            m,
            cx,
            AiMsg::SetState {
                state: BossState::Rage as u32,
                param: None,
            },
        ),
        AiMsg::Die => b.send(m, cx, AiMsg::Exit),
        _ => {}
    }
}

/// `true` when `m` runs a boss brain that is in its slot.
pub fn is_boss(m: &Mover) -> bool {
    m.monster()
        .is_some_and(|e| matches!(e.ai.brain, AiBrain::Boss(_)))
}

/// `InitAI` of a boss mover (`ai::on_added` delegates here). A no-op while the brain is out of its slot.
pub fn init_ai(m: &mut Mover, cx: &mut AiCx<'_>) {
    crate::ai::with_boss_brain(m, |b, m, _| b.init_ai(m, cx));
}

/// `RouteMessage()` of a boss mover (`ai::route_messages` delegates here): initialises a never-processed AI,
/// delivers the queued `Arrival`s in FIFO order, then `Process`.
pub fn route(m: &mut Mover, cx: &mut AiCx<'_>) {
    crate::ai::with_boss_brain(m, |b, m, _| {
        b.init_ai(m, cx);
        while let Some(msg) = m.monster_mut().and_then(|e| e.ai.queue.pop_front()) {
            b.send(m, cx, msg);
        }
        b.send(m, cx, AiMsg::Process);
    });
}

/// `SendAIMsg` to a boss mover (`ai::send` delegates here).
pub fn send(m: &mut Mover, msg: AiMsg, cx: &mut AiCx<'_>) {
    crate::ai::with_boss_brain(m, |b, m, _| {
        b.init_ai(m, cx);
        b.send(m, cx, msg);
    });
}

impl AiSlot {
    /// The boss brain, if this is one of the four boss AIs.
    pub fn boss(&self) -> Option<&BossAi> {
        match &self.brain {
            AiBrain::Boss(b) => Some(b),
            _ => None,
        }
    }

    /// The boss brain, mutably.
    pub fn boss_mut(&mut self) -> Option<&mut BossAi> {
        match &mut self.brain {
            AiBrain::Boss(b) => Some(b),
            _ => None,
        }
    }
}
