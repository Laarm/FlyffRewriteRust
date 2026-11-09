//! The Meteonyker (`CAIMeteonyker`, `MI_DU_METEONYKER2/3/4` of the dungeon `WI_DUNGEON_VOLCANERED`; Plan/18 §11, ref/12
//! §10.4; `aimeteonyker.cpp`; task boss-ai-b, WP-AI-21). The plain `MI_DU_METEONYKER` runs `CAIMonster`; only its
//! hit dispatch ([`on_hit`]) is shared.
//!
//! ```text
//!   InitAI ──► line state (IDLE) ──scan / Damage──► RAGE ──target gone / safety / stuck / transparent──► line state
//!                 ▲                                  │ every 3 s: SP cycle ──► pull (Summon) / burn, or the retaliation
//!                 └───── walk home (10 m, no HP restore) ◄──┘ ...  the attack runs on the next Arrival
//! ```
//!
//! **State map.** The original's map has only INIT (deaf), IDLE and RAGE; its line state 8 (STAND) would leave the
//! brain deaf (the handler pointer stays NULL). FIX: any unmapped state runs the IDLE handler; the "STAND => return
//! home" branches still test the line state (8) as the original. A `SetState{7}` from a helper-calling monster is the
//! Meteonyker's own RAGE (mapped).
//!
//! **Cadence.** The RAGE process runs every tick; the attack machinery every 3 s (`tm_attack_delay`): clear the
//! one-kill mode, [`MeteonykerAi::process_sp_attack`] (call 19 of 20 announces the special, call 20 executes it,
//! every 10th call of the retaliation hits the top damage dealer), else the ordinary swing (`atk = ATK1` and
//! `CMD_SetMeleeAttack(target, 0)`); the swing itself starts on `Arrival` with the prepared `atk`.
//!
//! **Differences from `CAIMonster`.** The chase has no speed factor 2.0 ([`MeteonykerAi::move_to_obj`]), the range
//! tests are 3-D ([`in_range_3d`]), the way home ends within 10 m without restoring hit points or forgetting
//! enemies (Q23), a Meteonyker on its way home takes no damage ([`crate::combat::arbiter`], `IsReturnToBegin`), it
//! heals 50 hit points every 2 s while idle, `ScanTargetNext` uses 15 m also in instance dungeons, and a target or
//! dealer in another layer of the world counts as gone (the AI sees its own layer; the original accepted it).
//!
//! **Hits.** [`on_hit`] / [`on_continue`] (`hits`) run at the barrier over the pure [`hit_effects`] /
//! [`continue_effects`].

mod hits;

pub use hits::{
    BURN_LEVEL, BURN_RANGE, BURN_SKILL, STUN_LEVEL, STUN_SKILL, continue_effects, hit_effects,
    on_continue, on_hit,
};

use super::boss::fx::BossFx;
use super::boss::{rage, tables};
use super::ctx::{AiCmd, AiCx, emote};
use super::monster::helpers::{TargetView, inc_hp, max_hp, mover_ex, mover_is_die, target_view};
use super::monster::{
    MAX_DEPTH, MonsterAi, MonsterState, NEXT_SCAN_RANGE, RANGE_MOVE, RETURN_WARP_AFTER_IDLE_MS,
    STUCK_MOVE_MAX,
};
use super::msg::AiMsg;
use super::rng::{x_rand, x_random};
use super::scan::{
    JOB_ALL, scan_target, scan_target_next, scan_target_over_healer, scan_target_strong,
};
use super::{AiBrain, AiSlot, BossKind};
use crate::combat::action::npc_melee_msg;
use crate::movement::intents;
use crate::mover::Mover;
use crate::world::grid::LinkSet;
use flyff_common::defs::text::{
    TID_GAME_BOSS_METEONYKER_MSG_01, TID_GAME_BOSS_METEONYKER_MSG_02,
    TID_GAME_BOSS_METEONYKER_MSG_03,
};
use flyff_common::math::Vec3;
use flyff_common::{GameMs, ObjId};
use flyff_rules::combat::rng::x_random_range;
use flyff_rules::consts::ONEKILL_MODE;
use flyff_rules::consts::job::JOB_MERCENARY;
use flyff_sim::kin::own_speed;
use flyff_sim::num::{cosf, sinf, to_radian};
use flyff_sim::types::{
    HATTR_NOMOVE, HATTR_NOWALK, OBJMSG_ATK1, OBJMSG_ATK2, OBJMSG_ATK3, OBJMSG_ATK4, OBJMSG_NONE,
    OBJSTA_DMG_FLY_ALL,
};
use std::collections::BTreeMap;

/// `STATE_STAND` of the respawn line: the "walk home first" branches test it.
const LINE_STAND: u32 = MonsterState::Stand as u32;
/// `s_tmAttack`: 20 s of chase credit after an attack or a hit.
pub const CHASE_CREDIT_MS: u64 = 20_000;
/// `CB_RETALIATION`: the retaliation calls between two hits on the top dealer.
pub const RETALIATION_CALLS: i32 = 10;
/// The special attack cycle: call 19 announces, call 20 executes.
pub const SP_CYCLE: u32 = 20;
/// The attack machinery runs every 3 s.
pub const ATTACK_DELAY_MS: u64 = 3_000;
/// The idle regeneration: `IncHitPoint(50)` every 2 s.
pub const RECOVERY_HP: i32 = 50;
/// The period of the idle regeneration, ms.
pub const RECOVERY_MS: u64 = 2_000;
/// `ScanTarget(pMover, 40, JOB_ALL)` of `SelectTarget` without a last attacker.
pub const SELECT_SCAN_RANGE: i32 = 40;
/// The pull reaches players within this distance of the Meteonyker (3-D, squared 225, from the BOSS).
pub const PULL_RANGE_SQ: f32 = 225.0;
/// The way home is complete within this distance (3-D) of the spawn point.
pub const HOME_RANGE: f32 = 10.0;
/// The Meteonyker's hit percentages that arm the one-kill of the next retaliation.
pub const CRITICAL_PERCENTS: [u32; 4] = [80, 50, 30, 10];

/// The special attack the next [`MeteonykerAi::process_sp_attack`] execution performs (`m_nSPAttackType`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MeteoSp {
    /// 0: pull the players around into the boss's reach, then the area swing (ATK2).
    #[default]
    Summon,
    /// 1: the burn swing (ATK4) on the target.
    Burn,
}

/// The per-mover state of `CAIMeteonyker` (18 §11).
#[derive(Clone, Debug, PartialEq)]
pub struct MeteonykerAi {
    /// The `CAIMonster` fields the Meteonyker shares (state, spawn, returning, target, timers). Its `state` is
    /// INIT 1, IDLE 2 or RAGE 7 (any unmapped state runs IDLE).
    pub base: MonsterAi,
    /// `m_dwAIInterfaceState` of the respawn line, refreshed by every entry point.
    pub line: u32,
    /// `m_dwAtk`: the swing the next `Arrival` starts (`OBJMSG_ATK1 .. ATK4`, or `OBJMSG_NONE`).
    pub atk: u32,
    /// `m_idLastAttacker`.
    pub last_attacker: Option<ObjId>,
    /// `m_mapDamage`: the damage per attacker since the last retaliation (ascending id).
    pub damage: BTreeMap<ObjId, i64>,
    /// `m_dwHPOld`: the last hit-point percentage seen by a `Damage`.
    pub last_hp_pct: u32,
    /// `m_bCritical`: the next retaliation turns the one-kill mode on.
    pub critical: bool,
    /// `m_cbAttack`: the SP calls so far.
    pub cb_attack: u32,
    /// `m_cbRetaliation`: calls until the next retaliation.
    pub cb_retaliation: i32,
    /// `m_nSPAttackType`.
    pub sp_kind: MeteoSp,
    /// The skill rows `(skill, level)` and texts `(tid, u32::MAX)` whose absence was logged (once each).
    pub missing_logged: Vec<(u32, u32)>,
}

impl Default for MeteonykerAi {
    fn default() -> Self {
        MeteonykerAi {
            base: MonsterAi::new(),
            line: super::AI_STATE_DEFAULT,
            atk: OBJMSG_NONE,
            last_attacker: None,
            damage: BTreeMap::new(),
            last_hp_pct: 100,
            critical: false,
            cb_attack: 0,
            cb_retaliation: RETALIATION_CALLS,
            sp_kind: MeteoSp::Summon,
            missing_logged: Vec::new(),
        }
    }
}

/// `CAIMeteonyker::IsInRange`: the 3-D length, `|v|^2 <= r^2` (`CAIMonster`'s version zeroes y).
// the negated comparison is the original's (`fDistSq > fRange * fRange` -> FALSE), and NaN counts as in range
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn in_range_3d(v: Vec3, r: f32) -> bool {
    let d_sq = v.x * v.x + v.y * v.y + v.z * v.z;
    !(d_sq > r * r)
}

impl MeteonykerAi {
    /// A Meteonyker before its first tick; `InitAI` fills the rest.
    pub fn new() -> MeteonykerAi {
        MeteonykerAi::default()
    }

    /// `Init()` + `InitAI` (`aimeteonyker.cpp:59-83`, merged and idempotent): the spawn position, no target, the
    /// 20 s chase credit, every other timer now, then `SetState(line state)`.
    pub fn init_ai(&mut self, m: &mut Mover, cx: &mut AiCx<'_>) {
        if self.base.inited {
            return;
        }
        let now = cx.now;
        self.base.inited = true;
        self.base.spawn_pos = m.sim.pos;
        self.base.spawn_angle = m.sim.angle;
        self.base.target = None;
        self.base.target_party = 0;
        self.base.returning = false;
        self.cb_retaliation = RETALIATION_CALLS;
        self.atk = OBJMSG_NONE;
        self.last_attacker = None;
        self.base.tm_attack = GameMs(now.0 + CHASE_CREDIT_MS);
        self.base.old_pos = Vec3::ZERO;
        self.base.tm_recovery = now;
        self.base.tm_attack_delay = now;
        self.base.tm_move = now;
        self.base.tm_return = now;
        self.base.pos_damage = None;
        self.cb_attack = 0;
        self.sp_kind = MeteoSp::Summon;
        self.last_hp_pct = 100;
        self.critical = false;
        self.damage.clear();
        let line = self.line;
        self.send(
            m,
            cx,
            AiMsg::SetState {
                state: line,
                param: None,
            },
        );
    }

    /// `SendAIMsg`: synchronous delivery. `SetState` switches the handler (1 INIT, 7 RAGE, anything else IDLE: the
    /// FIX of the module docs) and delivers `Init{state, param}`; everything else goes to the current handler. A
    /// message that nests deeper than [`MAX_DEPTH`] is dropped with a warning.
    pub fn send(&mut self, m: &mut Mover, cx: &mut AiCx<'_>, msg: AiMsg) {
        if self.base.depth >= MAX_DEPTH {
            tracing::warn!(id = ?m.id(), ?msg, "Meteonyker AI message dropped: recursion limit");
            return;
        }
        self.base.depth += 1;
        match msg {
            AiMsg::SetState { state, param } => {
                self.base.state = match state {
                    1 => MonsterState::Init,
                    7 => MonsterState::Rage,
                    _ => MonsterState::Idle,
                };
                self.dispatch(m, cx, AiMsg::Init { state, param });
            }
            other => self.dispatch(m, cx, other),
        }
        self.base.depth -= 1;
    }

    fn dispatch(&mut self, m: &mut Mover, cx: &mut AiCx<'_>, msg: AiMsg) {
        match self.base.state {
            MonsterState::Idle => self.idle(m, cx, msg),
            MonsterState::Rage => self.rage(m, cx, msg),
            // `StateInit` is a handler that does nothing
            _ => {}
        }
    }

    fn set_state(&mut self, m: &mut Mover, cx: &mut AiCx<'_>, state: u32, param: Option<ObjId>) {
        self.send(m, cx, AiMsg::SetState { state, param });
    }

    // ---- the helpers (`aimeteonyker.cpp:85-173, 792-812`) ---------------------------------------------------

    /// `MoveToDst(id)` WITHOUT the speed factor 2.0 (the line is commented out in the original): no-op when `id`
    /// already is the destination object, remembers the target with its party when it resolves, announces the
    /// destination object with range 0.
    pub fn move_to_obj(&mut self, m: &mut Mover, cx: &AiCx<'_>, id: ObjId) {
        if m.sim.dest.obj == id {
            return;
        }
        if let Some(t) = target_view(cx, id) {
            self.base.target = Some(id);
            self.base.target_party = t.party;
        }
        intents::set_dest_obj(m, id, 0.0);
    }

    /// `DoReturnToBegin(TRUE)`: the base's (returning, `tm_return = now`, target dropped, speed 2.66, walk to the
    /// spawn point).
    fn go_home(&mut self, m: &mut Mover, cx: &mut AiCx<'_>) {
        self.base.do_return_to_begin(m, cx, true);
    }

    /// `DoReturnToBegin(FALSE)`: the way home is over; the target is dropped. Unlike `CAIMonster` nothing is
    /// restored and no enemy is forgotten (the lines are commented out in the original; KQ-13 Q23).
    fn arrive_home(&mut self, m: &mut Mover) {
        self.base.returning = false;
        self.base.set_target(m, None, 0);
    }

    /// "line state STAND => walk home first" in front of a `SetState(line state)`.
    fn leave_rage(&mut self, m: &mut Mover, cx: &mut AiCx<'_>) {
        if self.line == LINE_STAND {
            self.go_home(m, cx);
        }
        let line = self.line;
        self.set_state(m, cx, line, None);
    }

    /// `MoveToRandom(STATE_IDLE)`: a point `(x % 21 - 10, z % 21 - 10)` around the position (x drawn first). Within
    /// 30 m of the spawn point (the horizontal distance of the candidate): walk; on the way home: walk anyway; a
    /// Meteonyker that already is outside (3-D, unlike `CAIMonster`) starts the way home.
    pub fn move_to_random_idle(&mut self, m: &mut Mover, cx: &mut AiCx<'_>) {
        let mut v = m.sim.pos;
        let x1 = x_rand(cx.rng);
        let x2 = x_rand(cx.rng);
        v.x += ((x1 % 21) as i32 - 10) as f32;
        v.z += ((x2 % 21) as i32 - 10) as f32;
        let mut away = v - self.base.spawn_pos;
        away.y = 0.0;
        if in_range_3d(away, RANGE_MOVE) || self.base.returning {
            self.base.move_to_pos(m, v);
        } else if !in_range_3d(m.sim.pos - self.base.spawn_pos, RANGE_MOVE) {
            self.go_home(m, cx);
        }
    }

    // ---- STATE_IDLE (`aimeteonyker.cpp:180-293`) -------------------------------------------------------------

    fn idle(&mut self, m: &mut Mover, cx: &mut AiCx<'_>, msg: AiMsg) {
        match msg {
            AiMsg::Init { .. } => {
                if !in_range_3d(m.sim.pos - self.base.spawn_pos, RANGE_MOVE) {
                    self.go_home(m, cx);
                }
                self.base.set_stop(m, cx, 0);
            }
            AiMsg::InitTargetClear => self.base.set_target(m, None, 0),
            AiMsg::Process => self.move_process_idle(m, cx),
            AiMsg::Damage { attacker, .. } => {
                // damage is ignored while the Meteonyker is on its way home (KQ-13 Q3)
                if !self.base.returning {
                    self.move_to_obj(m, cx, attacker);
                    self.set_state(m, cx, MonsterState::Rage as u32, Some(attacker));
                    self.base.tm_attack = GameMs(cx.now.0 + CHASE_CREDIT_MS);
                    self.base.pos_damage = Some(m.sim.pos);
                }
            }
            AiMsg::Die => self.send(m, cx, AiMsg::Exit),
            AiMsg::Arrival { .. } => {
                let ms = if self.base.returning {
                    u64::from(x_random(cx.rng, 1000))
                } else {
                    5000 + u64::from(x_random(cx.rng, 1000))
                };
                self.base.set_stop(m, cx, ms);
            }
            // Collision, Exit and the rest: no-ops of the original
            _ => {}
        }
    }

    /// `MoveProcessIdle`: a dead or flying-through-the-air Meteonyker does nothing (not even the recovery).
    fn move_process_idle(&mut self, m: &mut Mover, cx: &mut AiCx<'_>) {
        if mover_is_die(m) || m.sim.act.state.bits() & OBJSTA_DMG_FLY_ALL != 0 {
            return;
        }
        if self.base.returning {
            // within 10 m (3-D) of the spawn point: the way is over (no HP restore); the 20 s warp check runs anyway
            if in_range_3d(self.base.spawn_pos - m.sim.pos, HOME_RANGE) {
                self.arrive_home(m);
                self.send(
                    m,
                    cx,
                    AiMsg::Arrival {
                        obj: None,
                        pos_arrival: false,
                    },
                );
            }
            if cx.now.0.saturating_sub(self.base.tm_return.0) >= RETURN_WARP_AFTER_IDLE_MS {
                self.base.warp_home(m, cx);
            }
        } else {
            if self.base.is_end_stop(m, cx) {
                self.move_to_random_idle(m, cx);
            }
            if mover_ex(m, cx).ai.scan_job != -1 {
                if m.monster().is_some_and(|e| e.active_attack) {
                    self.select_target(m, cx);
                }
                // a target kept from an earlier RAGE re-engages without a scan, a dead one too (KEEP)
                if let Some(t) = self.base.target.and_then(|id| target_view(cx, id)) {
                    cx.emote(m, emote::ATTACK);
                    self.set_state(m, cx, MonsterState::Rage as u32, Some(t.id));
                }
            }
        }
        self.process_recovery(m, cx);
    }

    /// `ProcessRecovery`: +50 hit points every 2 s (Q23); it also runs while the way home is walked.
    fn process_recovery(&mut self, m: &mut Mover, cx: &mut AiCx<'_>) {
        if cx.now > self.base.tm_recovery {
            self.base.tm_recovery = GameMs(cx.now.0 + RECOVERY_MS);
            inc_hp(m, cx, RECOVERY_HP);
        }
    }

    // ---- STATE_RAGE (`aimeteonyker.cpp:295-389`) -------------------------------------------------------------

    fn rage(&mut self, m: &mut Mover, cx: &mut AiCx<'_>, msg: AiMsg) {
        match msg {
            AiMsg::Init { param, .. } => {
                if let Some(p) = param
                    && let Some(t) = target_view(cx, p)
                {
                    self.base.set_target(m, Some(p), t.party);
                }
                self.base.tm_attack = GameMs(cx.now.0 + CHASE_CREDIT_MS);
                self.base.tm_attack_delay = cx.now;
                self.base.set_stop(m, cx, 0);
            }
            AiMsg::InitTargetClear => {
                match scan_target_next(
                    cx,
                    m,
                    NEXT_SCAN_RANGE,
                    self.base.target,
                    self.base.target_party,
                ) {
                    Some(t) => self.set_state(m, cx, MonsterState::Rage as u32, Some(t)),
                    None => {
                        self.base.set_target(m, None, 0);
                        let line = self.line;
                        self.set_state(m, cx, line, None);
                    }
                }
            }
            AiMsg::Process => self.move_process_rage(m, cx),
            AiMsg::Damage { attacker, damage } => self.on_damage(m, cx, attacker, damage),
            AiMsg::Arrival { .. } => self.on_arrival(m, cx),
            AiMsg::Die => self.send(m, cx, AiMsg::Exit),
            // Collision, AttackMelee, EndAppear, EndMeleeAttack, DstDie, Exit: no-ops of the original
            _ => {}
        }
    }

    /// `AIMSG_DAMAGE` in RAGE: remember the attacker, add its damage to the map (0 is ignored) and arm the
    /// one-kill when the hit-point percentage lands on 80, 50, 30 or 10 (only on a change; read when the mail is
    /// delivered, i.e. after every hit of the frame: a threshold crossed and left inside one frame is missed).
    fn on_damage(&mut self, m: &mut Mover, cx: &mut AiCx<'_>, attacker: ObjId, damage: i32) {
        self.last_attacker = Some(attacker);
        if damage != 0 {
            *self.damage.entry(attacker).or_insert(0) += i64::from(damage);
        }
        // `(DWORD)( hp / ( maxhp * 0.01F ) )`: single precision, truncated
        let pct = (m.mv.hp as f32 / (max_hp(m, cx) as f32 * 0.01_f32)) as u32;
        if pct != self.last_hp_pct {
            if CRITICAL_PERCENTS.contains(&pct) {
                self.critical = true;
            }
            self.last_hp_pct = pct;
        }
    }

    /// `AIMSG_ARRIVAL` in RAGE: `DoAttackMelee(m_dwIdTarget, m_dwAtk, item)` with the item of the prepared swing
    /// (`atk1 .. atk4`; 0 = the hand item for `OBJMSG_NONE`). A target that does not resolve gives 0 WITHOUT the
    /// `STOP` (the `OBJID` overload); once the call went through (it returns 1 also when the sim refused the swing)
    /// the chase credit is renewed and the swing is used up. An arrival with `atk == NONE` stops the Meteonyker,
    /// renews the credit and sends no `MeleeAttackNtf`.
    fn on_arrival(&mut self, m: &mut Mover, cx: &mut AiCx<'_>) {
        let item = cx.data().mover(m.prop_id()).map_or(0, |p| match self.atk {
            OBJMSG_ATK1 => p.atk1,
            OBJMSG_ATK2 => p.atk2,
            OBJMSG_ATK3 => p.atk3,
            OBJMSG_ATK4 => p.atk4,
            _ => 0,
        });
        let started = match self.base.target {
            Some(t) if cx.lookup.snapshot(t).is_some() => {
                npc_melee_msg(m, cx, t, self.atk, Some(item))
            }
            _ => 0,
        };
        if started == 1 {
            self.base.tm_attack = GameMs(cx.now.0 + CHASE_CREDIT_MS);
            self.atk = OBJMSG_NONE;
        }
    }

    /// `MoveProcessRage` (`aimeteonyker.cpp:391-518`).
    fn move_process_rage(&mut self, m: &mut Mover, cx: &mut AiCx<'_>) {
        if mover_is_die(m) || m.sim.act.state.bits() & OBJSTA_DMG_FLY_ALL != 0 {
            return;
        }
        // 15 also in instance dungeons (`CAIMonster` uses 160 there; KEEP: `cx.instance` is ignored)
        let t = match self.base.target.and_then(|id| target_view(cx, id)) {
            Some(t) if !t.die => t,
            _ => {
                match scan_target_next(
                    cx,
                    m,
                    NEXT_SCAN_RANGE,
                    self.base.target,
                    self.base.target_party,
                ) {
                    Some(n) => self.set_state(m, cx, MonsterState::Rage as u32, Some(n)),
                    None => self.leave_rage(m, cx),
                }
                return;
            }
        };
        // the target stands in a safety zone: always walk home
        if t.in_safety {
            self.go_home(m, cx);
            let line = self.line;
            self.set_state(m, cx, line, None);
            return;
        }
        // a flying Meteonyker against a ground target (never fires: the models are ground movers)
        if m.sim.params.flying_npc && m.sim.params.flying_npc != t.fly {
            self.leave_rage(m, cx);
            return;
        }
        // every 20 s: stuck? teleport onto the target; a Meteonyker that WALKED more than 2 m without starting a
        // swing leaves RAGE (KEEP; `old_pos` starts at the origin)
        if cx.now.0 > self.base.tm_attack.0 {
            self.base.tm_attack = GameMs(cx.now.0 + CHASE_CREDIT_MS);
            let mut moved = m.sim.pos - self.base.old_pos;
            moved.y = 0.0;
            if in_range_3d(moved, STUCK_MOVE_MAX) {
                if own_speed(&m.sim) > 0.0 {
                    intents::warp_npc(m, t.pos);
                }
            } else {
                self.base.tm_return = cx.now;
                self.leave_rage(m, cx);
                return;
            }
            self.base.old_pos = m.sim.pos;
        }
        // the spawn and damage leashes are commented out in the original
        self.base.pos_damage.get_or_insert(m.sim.pos);
        if t.transparent {
            self.leave_rage(m, cx);
            return;
        }
        if cx.now.0 > self.base.tm_attack_delay.0 {
            self.base.tm_attack_delay = GameMs(cx.now.0 + ATTACK_DELAY_MS);
            m.mv.mode &= !ONEKILL_MODE;
            if self.process_sp_attack(m, cx) {
                return;
            }
            if self.base.target.is_none() {
                self.select_target(m, cx);
            }
            match self.base.target {
                Some(target) => {
                    self.atk = OBJMSG_ATK1;
                    rage::set_melee_attack_dest(m, cx, target, 0.0);
                }
                None => self.leave_rage(m, cx),
            }
        }
    }

    // ---- the special attacks (`aimeteonyker.cpp:536-674, 814-851`) --------------------------------------------

    /// `ProcessSPAttack`: `n = ++cb_attack % 20`; 19 announces the special (one raw draw: even = pull, odd =
    /// burn; the caption of the world, no return), 0 executes it (a `true` ends the call), then the retaliation.
    pub fn process_sp_attack(&mut self, m: &mut Mover, cx: &mut AiCx<'_>) -> bool {
        self.cb_attack = self.cb_attack.wrapping_add(1);
        let n = self.cb_attack % SP_CYCLE;
        if n == SP_CYCLE - 1 {
            self.sp_kind = if x_rand(cx.rng).is_multiple_of(2) {
                MeteoSp::Summon
            } else {
                MeteoSp::Burn
            };
            let tid = match self.sp_kind {
                MeteoSp::Summon => TID_GAME_BOSS_METEONYKER_MSG_01,
                MeteoSp::Burn => TID_GAME_BOSS_METEONYKER_MSG_02,
            };
            cx.cmds.push(AiCmd::Boss(BossFx::Caption {
                world: cx.world,
                tid,
                name_of: None,
            }));
        } else if n == 0 {
            let done = match self.sp_kind {
                MeteoSp::Summon => self.process_summon(m, cx),
                MeteoSp::Burn => self.process_burn(m, cx),
            };
            if done {
                return true;
            }
        }
        self.process_retaliation(m, cx)
    }

    /// The target as `GetTarget` + `IsValidObj` + `!IsDie` see it.
    fn live_target(&self, cx: &AiCx<'_>) -> Option<TargetView> {
        self.base
            .target
            .and_then(|id| target_view(cx, id))
            .filter(|t| !t.die)
    }

    /// `ProcessSummon(TRUE)`: the target must resolve and live (else `false`, no draw); `xRandom(1, 101)` is drawn
    /// and discarded (the call is unconditional: KEEP the stream position). Every live player within 15 m (3-D,
    /// from the Meteonyker) of the point 7 m in front of and 7 m above it, found in the legacy window around that
    /// point, is replaced onto it (`REPLACE_NORMAL`; a NOWALK / NOMOVE cell moves the point to 7 m above the
    /// Meteonyker, tested per player on the current point). The area swing (ATK2) follows on the next `Arrival`,
    /// which the zero-length walk to its own position does NOT raise when it already stands there (KEEP).
    fn process_summon(&mut self, m: &mut Mover, cx: &mut AiCx<'_>) -> bool {
        if self.live_target(cx).is_none() {
            return false;
        }
        let _ = x_random_range(cx.rng, 1, 101);
        let me = m.sim.pos;
        let theta = to_radian(m.sim.angle);
        let mut p = me + Vec3::new(sinf(theta) * 7.0, 7.0, -cosf(theta) * 7.0);
        let (world, layer) = (cx.world, cx.layer);
        cx.grid.for_each_legacy(p, 15, LinkSet::Players, |id| {
            let Some(pl) = cx.scan.get(id) else { return };
            if !pl.live {
                return;
            }
            let d = me - pl.pos;
            if d.x * d.x + d.y * d.y + d.z * d.z > PULL_RANGE_SQ {
                return;
            }
            let attr = cx.collision.height_attr(p.x, p.z);
            if attr == HATTR_NOWALK || attr == HATTR_NOMOVE {
                p = me;
                p.y += 7.0;
            }
            cx.cmds.push(AiCmd::Replace {
                player: id,
                world,
                layer,
                pos: p,
            });
        });
        self.atk = OBJMSG_ATK2;
        self.base.move_to_pos(m, me);
        true
    }

    /// `ProcessBurn(TRUE)`: the target must resolve and live (else `false`, no draw); `xRandom(1, 101)` is drawn and
    /// discarded (the hit-point test is short-circuited); the burn swing (ATK4) is prepared and the Meteonyker
    /// chases the target (no-op when it already is the destination object).
    fn process_burn(&mut self, m: &mut Mover, cx: &mut AiCx<'_>) -> bool {
        let Some(t) = self.live_target(cx) else {
            return false;
        };
        let _ = x_random_range(cx.rng, 1, 101);
        self.atk = OBJMSG_ATK4;
        self.move_to_obj(m, cx, t.id);
        true
    }

    /// `GetDealer`: the attacker with the most damage since the last retaliation (ascending id walk, `>=`, so ties
    /// go to the highest id) that still resolves, lives and is in this layer.
    fn dealer(&self, cx: &AiCx<'_>) -> Option<ObjId> {
        let mut best = 0_i64;
        let mut found = None;
        for (&id, &dmg) in &self.damage {
            if dmg >= best
                && let Some(t) = target_view(cx, id)
                && !t.die
            {
                found = Some(id);
                best = dmg;
            }
        }
        found
    }

    /// `ProcessRetaliation`: every 10th call the top dealer is announced (caption with its name), becomes the
    /// target and is hit with ATK3; a pending `critical` turns the one-kill mode on for that swing; the map is
    /// cleared. No dealer: nothing happens and the map is kept.
    fn process_retaliation(&mut self, m: &mut Mover, cx: &mut AiCx<'_>) -> bool {
        self.cb_retaliation -= 1;
        if self.cb_retaliation > 0 {
            return false;
        }
        self.cb_retaliation = RETALIATION_CALLS;
        let Some(dealer) = self.dealer(cx) else {
            return false;
        };
        let party = target_view(cx, dealer).map_or(0, |t| t.party);
        cx.cmds.push(AiCmd::Boss(BossFx::Caption {
            world: cx.world,
            tid: TID_GAME_BOSS_METEONYKER_MSG_03,
            name_of: Some(dealer),
        }));
        self.base.set_target(m, Some(dealer), party);
        self.atk = OBJMSG_ATK3;
        rage::set_melee_attack_dest(m, cx, dealer, 0.0);
        if self.critical {
            m.mv.mode |= ONEKILL_MODE;
            self.critical = false;
        }
        self.damage.clear();
        true
    }

    /// `SelectTarget` (`aimeteonyker.cpp:708-790`): the last attacker is forgotten when it is gone, dead or
    /// `R^2 * 10` away; the target is cleared (speed 1.0); without a last attacker the nearest-first scan of 40 m
    /// decides (a flying player is no target for a ground Meteonyker: `false`), with one a roll of 0..99 (50 %, 70 %
    /// for a Mercenary) picks the last attacker, the strongest player or the over-healer within 50 m, falling back to
    /// the last attacker. `false` when nobody was found.
    pub fn select_target(&mut self, m: &mut Mover, cx: &mut AiCx<'_>) -> bool {
        let radius = rage::radius_xz(m);
        let r2 = radius * radius;
        let last = self
            .last_attacker
            .and_then(|id| target_view(cx, id))
            .filter(|la| {
                let d = la.pos - m.sim.pos;
                !la.die && d.x * d.x + d.y * d.y + d.z * d.z < r2 * rage::LAST_ATTACKER_FAR
            });
        if last.is_none() {
            self.last_attacker = None;
        }
        self.base.set_target(m, None, 0);
        let flies = m.sim.params.flying_npc;
        let fits = |cx: &AiCx<'_>, id: ObjId| flies || !target_view(cx, id).is_some_and(|t| t.fly);
        let Some(la) = last else {
            return match scan_target(cx, m, SELECT_SCAN_RANGE, JOB_ALL, 0, 0, 0) {
                Some(t) if fits(cx, t) => {
                    let party = target_view(cx, t).map_or(0, |v| v.party);
                    self.base.set_target(m, Some(t), party);
                    true
                }
                _ => false,
            };
        };
        // every kind but the ClockWorks favours a Mercenary: 70 instead of 50
        let pick =
            tables::target_pick(BossKind::Krrr, la.job == JOB_MERCENARY as i32, &mut *cx.rng);
        let found = match pick {
            tables::TargetPick::LastAttacker => None,
            tables::TargetPick::Strongest => scan_target_strong(cx, m, rage::PICK_SCAN_RANGE),
            tables::TargetPick::OverHealer => scan_target_over_healer(cx, m, rage::PICK_SCAN_RANGE),
        };
        let chosen = match found {
            Some(t) if fits(cx, t) => t,
            _ => la.id,
        };
        let party = target_view(cx, chosen).map_or(0, |v| v.party);
        self.base.set_target(m, Some(chosen), party);
        true
    }
}

// ---- the brain in the slot (the pattern of `ai::boss`) ----------------------------------------------------------------

/// `true` when `m` runs a Meteonyker brain that is in its slot.
pub fn is_meteonyker(m: &Mover) -> bool {
    m.monster()
        .is_some_and(|e| matches!(e.ai.brain, AiBrain::Meteonyker(_)))
}

/// `InitAI` of a Meteonyker mover (`ai::on_added` delegates here).
pub fn init_ai(m: &mut Mover, cx: &mut AiCx<'_>) {
    super::with_meteonyker_brain(m, |b, m, line| {
        b.line = line;
        b.init_ai(m, cx);
    });
}

/// `RouteMessage()` (`ai::route_messages` delegates here): initialises a never-processed AI, delivers the queued
/// `Arrival`s in FIFO order, then `Process`.
pub fn route(m: &mut Mover, cx: &mut AiCx<'_>) {
    super::with_meteonyker_brain(m, |b, m, line| {
        b.line = line;
        b.init_ai(m, cx);
        while let Some(msg) = m.monster_mut().and_then(|e| e.ai.queue.pop_front()) {
            b.send(m, cx, msg);
        }
        b.send(m, cx, AiMsg::Process);
    });
}

/// `SendAIMsg` to a Meteonyker mover (`ai::send` delegates here).
pub fn send(m: &mut Mover, msg: AiMsg, cx: &mut AiCx<'_>) {
    super::with_meteonyker_brain(m, |b, m, line| {
        b.line = line;
        b.init_ai(m, cx);
        b.send(m, cx, msg);
    });
}

impl AiSlot {
    /// The Meteonyker brain, if this is one.
    pub fn meteonyker(&self) -> Option<&MeteonykerAi> {
        match &self.brain {
            AiBrain::Meteonyker(b) => Some(b),
            _ => None,
        }
    }

    /// The Meteonyker brain, mutably.
    pub fn meteonyker_mut(&mut self) -> Option<&mut MeteonykerAi> {
        match &mut self.brain {
            AiBrain::Meteonyker(b) => Some(b),
            _ => None,
        }
    }
}
