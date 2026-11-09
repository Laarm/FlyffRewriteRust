//! `StateRage` and `MoveProcessRage` of the boss AIs (Plan/18 §10.1; `AIClockWorks.cpp:117-572`, shared by the
//! four kinds; task boss-ai-core, WP-AI-18): the RAGE handler, the process (the appear phase, the swing watch,
//! the chase timer, the re-attack gate with the band table), `SelectTarget`, `CMD_SetMeleeAttack` and the four
//! per-kind hook functions that BigMuscle, Krrr and Bear fill (WP-AI-19, -20).
//!
//! The draws of one cycle, in time order (world stream only, PL-013): AI pass: `select_target` (1 draw, only with
//! a last attacker) -> `choose_attack` (ClockWorks: 1 draw, only in the `[1.5 R^2, 3 R^2)` band) -> arrival
//! (ClockWorks: none; Krrr / Bear `Normal`: 1 draw); barrier of the swing's first frame: the hit roll of
//! `on_npc_attack_started`; barrier of the hit frame: the knock-up roll of ATK4, then the damage draws of the
//! victims; `EndMeleeAttack` at the end frame: `x_random(2000)`.

use super::tables::{self, TargetPick};
use super::{BossAi, BossAttack, BossEvent, Reach, bear, bigmuscle, clockworks, krrr};
use crate::ai::BossKind;
use crate::ai::ctx::AiCx;
use crate::ai::monster::helpers::{mover_is_die, target_view};
use crate::ai::msg::AiMsg;
use crate::ai::rng::x_random;
use crate::ai::scan::{JOB_ALL, scan_target, scan_target_over_healer, scan_target_strong};
use crate::combat::action::npc_melee_msg;
use crate::movement::intents;
use crate::mover::Mover;
use flyff_common::defs::itemkind::{IK2_TOOLS, IK3_YOYO};
use flyff_common::{GameMs, ObjId};
use flyff_rules::combat::attack_range;
use flyff_rules::consts::dst::DST_HAWKEYE_RATE;
use flyff_rules::consts::job::JOB_MERCENARY;
use flyff_sim::ActMsg;
use flyff_sim::types::{MTI_STAND, OBJMSG_APPEAR, OBJSTA_ACTION_ALL, OBJSTA_APPEAR};

/// `m_tmReattack + (2000 - m_tmAddReattack)`: the base of the re-attack wait, 2 s.
pub const REATTACK_BASE_MS: u64 = 2_000;
/// `PROCESS_COUNT * 5` (`PROCESS_COUNT = FRAME_PER_SEC / 4 = 15` on the server): the appear phase lasts 75 ticks.
pub const APPEAR_TICKS: u32 = 75;
/// A swing that has not ended after this long is given up (`m_tmTimeOver + SEC(15)`).
pub const SWING_TIMEOUT_MS: u64 = 15_000;
/// A chase that has not arrived after this long is given up (`m_tmTrace + 3000`).
pub const CHASE_TIMEOUT_MS: u64 = 3_000;
/// The last attacker is forgotten when it is farther than `sqrt(10)` radii away (`fRadiusSq * 10`).
pub const LAST_ATTACKER_FAR: f32 = 10.0;
/// The scan range of `ScanTargetStrong` / `ScanTargetOverHealer` in `SelectTarget`, metres.
pub const PICK_SCAN_RANGE: f32 = 50.0;

/// `GetRadiusXZ()` of the boss: `(|dx| + |dz|) / 4 * scale` of its model box.
pub fn radius_xz(m: &Mover) -> f32 {
    flyff_sim::bounds::radius_xz(&m.sim.params.bounds, m.sim.params.scale)
}

// ---- the per-kind hooks (BigMuscle, Krrr and Bear are WP-AI-19, -20) --------------------------------------------

/// The per-kind part of `InitAI` / `Init()`, before the switch to RAGE: nothing for ClockWorks, BigMuscle and
/// Krrr; the Bear starts its 30 s buff timer (`AIBear.cpp:94`, WP-AI-20: `bear::init(b, now)`).
pub fn init_extras(b: &mut BossAi, now: GameMs) {
    match b.kind {
        BossKind::ClockWorks | BossKind::BigMuscle | BossKind::Krrr => {}
        BossKind::Bear => bear::init(b, now),
    }
}

/// The pre-step of every RAGE `Process` (18 §10.3): the ClockWorks enters its defence mode; BigMuscle, Krrr and
/// Bear run their opener (and Krrr its meteor tick): `{bigmuscle,krrr,bear}::pre_step(b, m, cx)` (WP-AI-19, -20).
pub fn pre_step(b: &mut BossAi, m: &mut Mover, cx: &mut AiCx<'_>) {
    match b.kind {
        BossKind::ClockWorks => clockworks::pre_step(b, m, cx),
        // ---- boss-ai-b ----
        BossKind::BigMuscle => bigmuscle::pre_step(b, m, cx),
        BossKind::Krrr => krrr::pre_step(b, m, cx),
        BossKind::Bear => bear::pre_step(b, m, cx),
    }
}

/// The override of the attack choice after [`tables::choose_attack`] did not give up the target (the Bear's
/// `BuffTime` when its timer ran out, `bear::after_choose(b, now)`, WP-AI-20); `None` keeps the choice.
pub fn after_choose(b: &mut BossAi, now: GameMs) -> Option<(BossAttack, Reach)> {
    match b.kind {
        BossKind::ClockWorks | BossKind::BigMuscle | BossKind::Krrr => None,
        // ---- boss-ai-b ----
        BossKind::Bear => bear::after_choose(b, now),
    }
}

/// What a boss does after its swing was accepted (the shouts, summons and captions of BigMuscle, Krrr and Bear:
/// `{bigmuscle,krrr,bear}::arrival_extras(b, m, cx, target)`, WP-AI-19, -20); nothing for ClockWorks.
pub fn arrival_extras(b: &BossAi, m: &Mover, cx: &mut AiCx<'_>, target: ObjId) {
    match b.kind {
        BossKind::ClockWorks => {}
        // ---- boss-ai-b ----
        BossKind::BigMuscle => bigmuscle::arrival_extras(b, m, cx, target),
        BossKind::Krrr => krrr::arrival_extras(b, m, cx, target),
        BossKind::Bear => bear::arrival_extras(b, m, cx, target),
    }
}

// ---- StateRage ---------------------------------------------------------------------------------------------------

/// The messages of `STATE_RAGE` / `STATE_SUPER_RAGE` (18 §10.1 table).
pub(super) fn handle(b: &mut BossAi, m: &mut Mover, cx: &mut AiCx<'_>, msg: AiMsg) {
    match msg {
        AiMsg::Process => move_process_rage(b, m, cx),
        // "remember who hit me a lot, to punish later"
        AiMsg::Damage { attacker, .. } => b.last_attacker = Some(attacker),
        AiMsg::Arrival { .. } => on_arrival(b, m, cx),
        AiMsg::EndAppear => b.event = BossEvent::Moving,
        AiMsg::EndMeleeAttack => {
            b.event = BossEvent::Moving;
            b.tm_reattack = cx.now;
            b.add_reattack_ms = x_random(cx.rng, REATTACK_BASE_MS as u32);
            b.target = None;
        }
        AiMsg::Die => b.send(m, cx, AiMsg::Exit),
        // Init, AttackMelee, BeginMove, Collision, DstDie, InitTargetClear, Exit: no-ops of the original
        _ => {}
    }
}

/// `AIMSG_ARRIVAL` in RAGE: the chosen attack starts (`DoAttackMelee(OBJID)` with the mapped message and item).
fn on_arrival(b: &mut BossAi, m: &mut Mover, cx: &mut AiCx<'_>) {
    let mapped = b
        .attack
        .and_then(|a| tables::arrival(b.kind, a, &mut *cx.rng));
    let Some((msg, slot)) = mapped else {
        // `Error( "... AIMSG_ARRIVAL m_nAttackType=%d" )` and back to MOVING
        if !b.bad_attack_logged {
            b.bad_attack_logged = true;
            tracing::error!(
                id = ?m.id(),
                kind = ?b.kind,
                attack = ?b.attack,
                "boss AI: arrival with no (or an unknown) attack type"
            );
        }
        b.event = BossEvent::Moving;
        return;
    };
    // `if (m_idTarget != NULL_ID)`: without a target the arrival changes nothing
    let Some(target) = b.target else { return };
    let atk = cx
        .data()
        .mover(m.prop_id())
        .map_or([0; 4], |p| [p.atk1, p.atk2, p.atk3, p.atk4]);
    // `DoAttackMelee(OBJID)` (`Mover.h:1794-1801`): a target that does not resolve gives 0 BEFORE the `STOP`
    let started = if cx.lookup.snapshot(target).is_none() {
        0
    } else {
        npc_melee_msg(m, cx, target, msg, Some(slot.item(atk)))
    };
    if started == 1 {
        b.event = BossEvent::Attacking;
        b.tm_time_over = cx.now;
        arrival_extras(b, m, cx, target);
    } else {
        b.event = BossEvent::Moving;
    }
}

// ---- MoveProcessRage ---------------------------------------------------------------------------------------------

/// `MoveProcessRage` (`AIClockWorks.cpp:297-449`).
fn move_process_rage(b: &mut BossAi, m: &mut Mover, cx: &mut AiCx<'_>) {
    // a dead boss does nothing (the monster brains stop the same way)
    if mover_is_die(m) {
        return;
    }
    pre_step(b, m, cx);
    let now = cx.now.0;
    match b.event {
        BossEvent::Ready => {
            // "the roar": the server copy plays the appear motion, nothing is broadcast (KQ-13 Q19)
            if m.sim.act.ground == 1 && m.sim.act.state.action() != OBJSTA_APPEAR {
                cx.send_act(m, ActMsg::new(OBJMSG_APPEAR));
            }
            b.appear_cnt += 1;
            if b.appear_cnt >= APPEAR_TICKS {
                b.send(m, cx, AiMsg::EndAppear);
                cx.with_cx(m, |sim, c| sim.reset_state(c, OBJSTA_ACTION_ALL));
                b.appear_cnt = 0;
            }
        }
        BossEvent::Attacking => {
            let over = m.sim.motion.id == MTI_STAND
                || m.sim.motion.clock.end_frame
                || now >= b.tm_time_over.0.saturating_add(SWING_TIMEOUT_MS);
            if over {
                b.event = BossEvent::Moving;
                b.tm_reattack = cx.now;
                // not random here (the original's `xRandom( 2000 )` is commented out)
                b.add_reattack_ms = REATTACK_BASE_MS as u32;
                b.target = None;
            }
        }
        BossEvent::Tracking => {
            if now >= b.tm_trace.0.saturating_add(CHASE_TIMEOUT_MS) {
                b.event = BossEvent::Moving;
                // the target is cleared so that `SelectTarget` runs again
                b.target = None;
            }
        }
        BossEvent::Moving => moving(b, m, cx),
    }
}

/// The MOVING branch: the re-attack gate, the target, the band table and the chase order.
fn moving(b: &mut BossAi, m: &mut Mover, cx: &mut AiCx<'_>) {
    let gate = b
        .tm_reattack
        .0
        .saturating_add(REATTACK_BASE_MS.saturating_sub(u64::from(b.add_reattack_ms)));
    if cx.now.0 < gate {
        return;
    }
    let radius = radius_xz(m);
    let r2 = radius * radius;
    if b.target.is_none() && !select_target(b, m, cx) {
        return;
    }
    // `prj.GetMover(m_idTarget)` + `IsValidObj`: a dead player is still valid (KEEP)
    let Some(id) = b.target else { return };
    let Some(t) = target_view(cx, id) else {
        b.target = None;
        return;
    };
    let d = t.pos - m.sim.pos;
    let d2 = d.x * d.x + d.y * d.y + d.z * d.z;
    let Some((mut attack, mut reach)) =
        tables::choose_attack(b.kind, d2, r2, b.attack, &mut *cx.rng)
    else {
        // far away: the target is given up
        b.target = None;
        return;
    };
    if let Some((a, r)) = after_choose(b, cx.now) {
        attack = a;
        reach = r;
    }
    b.attack = Some(attack);
    b.tm_trace = cx.now;
    b.event = BossEvent::Tracking;
    set_melee_attack_dest(m, cx, id, reach.metres(radius));
}

/// `CMover::CMD_SetMeleeAttack(target, range)` on the server (`MoverMsg.cpp:427-454`, 18 §8.8): walk toward
/// `target` until `range` metres from it. The hand item of an NPC is its virtual item (`prop.atk1`): a tool
/// (`IK2_TOOLS`) cannot attack; a swing in progress or death ends the call; a yo-yo keeps its own range. An
/// unchanged destination object keeps its old range (`__TRAFIC_1222`, [`intents::set_dest_obj`]). A hand item
/// without a prop skips both item tests (the original dereferenced NULL).
pub fn set_melee_attack_dest(m: &mut Mover, cx: &AiCx<'_>, target: ObjId, range: f32) {
    let hand = cx.data().item(m.mv.virt_item);
    if hand.is_some_and(|p| p.item_kind2 == IK2_TOOLS) {
        return;
    }
    if m.sim.act.state.is_act_attack() || mover_is_die(m) {
        return;
    }
    let mut range = range;
    if let Some(p) = hand
        && p.item_kind3 == IK3_YOYO
    {
        range = attack_range(p.attack_range, m.mv.dst.table.param(DST_HAWKEYE_RATE, 0));
    }
    intents::set_dest_obj(m, target, range);
}

// ---- SelectTarget ------------------------------------------------------------------------------------------------

/// `SelectTarget` (`AIClockWorks.cpp:152-292`, the copy of each boss): chooses the next target. `false` when
/// there is nobody (no player in range, or the scanned one flies and the boss does not).
///
/// The last attacker is forgotten when it is gone or farther than `sqrt(10)` radii. Without one the boss scans
/// 80 m (ClockWorks) / 40 m (the others) around itself; with one it rolls once: the last attacker (50 %, 70 % for
/// the kinds that favour a Mercenary), the strongest player within 50 m (up to 74) or the over-healer within
/// 50 m, each falling back to the last attacker when nobody qualifies or the candidate flies.
pub fn select_target(b: &mut BossAi, m: &Mover, cx: &mut AiCx<'_>) -> bool {
    let radius = radius_xz(m);
    let r2 = radius * radius;
    let last = b.last_attacker.and_then(|id| target_view(cx, id));
    if b.last_attacker.is_some() {
        match last {
            None => b.last_attacker = None,
            Some(la) => {
                let d = la.pos - m.sim.pos;
                if d.x * d.x + d.y * d.y + d.z * d.z >= r2 * LAST_ATTACKER_FAR {
                    b.last_attacker = None;
                }
            }
        }
    }
    b.target = None;
    let flies = m.sim.params.flying_npc;
    // a candidate is taken when the boss flies or the candidate does not
    let fits = |cx: &AiCx<'_>, id: ObjId| flies || !target_view(cx, id).is_some_and(|t| t.fly);
    match (b.last_attacker, last) {
        (Some(la), Some(view)) => {
            let mercenary = view.job == JOB_MERCENARY as i32;
            let pick = tables::target_pick(b.kind, mercenary, &mut *cx.rng);
            let found = match pick {
                TargetPick::LastAttacker => None,
                TargetPick::Strongest => scan_target_strong(cx, m, PICK_SCAN_RANGE),
                TargetPick::OverHealer => scan_target_over_healer(cx, m, PICK_SCAN_RANGE),
            };
            b.target = Some(match found {
                Some(t) if fits(cx, t) => t,
                _ => la,
            });
            true
        }
        _ => {
            let range = tables::opening_scan_range(b.kind);
            match scan_target(cx, m, range, JOB_ALL, 0, 0, 0) {
                Some(t) if fits(cx, t) => {
                    b.target = Some(t);
                    true
                }
                _ => false,
            }
        }
    }
}
