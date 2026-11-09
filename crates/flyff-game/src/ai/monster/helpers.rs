//! The shared movement helpers of the monster AI (Plan/18 §8.8, §9.10): `MoveToDst`, `SetStop`, `IsEndStop`,
//! `IsInRange`, `MoveToRandom`, `SetTarget`, `DoReturnToBegin`, the HP seam and the target views.

use super::{MonsterAi, MonsterState, RANGE_MOVE, SPEED_FACTOR_RETURN};
use crate::ai::ctx::{AiCmd, AiCx};
use crate::ai::rng::x_rand;
use crate::movement::{IntentAudience, intents};
use crate::mover::Mover;
use flyff_common::math::Vec3;
use flyff_common::{GameMs, ObjId};
use flyff_data::mover_ex::MoverEx;
use flyff_data::props::mover::MoverProp;
use flyff_proto::{EncodedMsg, pb};
use flyff_rules::RulesData;
use flyff_rules::combat::mul_div;
use flyff_rules::consts::dst::{DST_HP, DST_HP_MAX, DST_HP_MAX_RATE, DST_MP};
use flyff_rules::stats::{StatInputs, final_max, npc_max_hp_origin};
use flyff_sim::ActMsg;
use flyff_sim::types::OBJMSG_STAND;
use std::sync::LazyLock;

/// The `propMoverEx` block of a mover without one (the values the original sets before it reads a block).
static NO_EX: LazyLock<MoverEx> = LazyLock::new(MoverEx::default);

/// The `propMoverEx` block of `m`'s prop (`NO_EX` when the prop is unknown).
pub fn mover_ex<'a>(m: &Mover, cx: &'a AiCx<'_>) -> &'a MoverEx {
    cx.shared.data.mover(m.prop_id()).map_or(&*NO_EX, |p| &p.ex)
}

/// What the AI knows about a target mover (a player of the layer, or any other mover the start-of-tick
/// snapshot holds). `None` from [`target_view`] is the original's `IsValidObj(pTarget) == FALSE` (the mover is
/// gone, deleted or left the world).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TargetView {
    /// The mover.
    pub id: ObjId,
    /// `GetPos()`.
    pub pos: Vec3,
    /// `IsDie()`.
    pub die: bool,
    /// `m_pActMover->IsFly()`.
    pub fly: bool,
    /// `m_idparty` (0 = none; only players have one).
    pub party: u32,
    /// `IsMode(TRANSPARENT_MODE)`.
    pub transparent: bool,
    /// `IsRegionAttr(RA_SAFETY)`.
    pub in_safety: bool,
    /// The target is a player.
    pub is_player: bool,
    /// `GetJob()` (-1 for non-players).
    pub job: i32,
    /// The item in the right hand of a player (0 = bare hands and non-players): RUNAWAY keeps away from the
    /// range of the target's weapon.
    pub hand_item: u32,
    /// `DST_HAWKEYE_RATE` of a player.
    pub hawkeye: i32,
}

/// The view of mover `id`: a player of the layer from the scan view, any other awake mover from the snapshot.
pub fn target_view(cx: &AiCx<'_>, id: ObjId) -> Option<TargetView> {
    if let Some(p) = cx.scan.get(id) {
        return Some(TargetView {
            id,
            pos: p.pos,
            die: p.die,
            fly: p.fly,
            party: p.party,
            transparent: p.transparent,
            in_safety: p.in_safety_region,
            is_player: true,
            job: p.job,
            hand_item: p.hand_item,
            hawkeye: p.hawkeye,
        });
    }
    cx.lookup.snapshot(id).map(|s| TargetView {
        id,
        pos: s.pos,
        die: !s.alive,
        fly: s.is_fly,
        party: 0,
        transparent: false,
        in_safety: false,
        is_player: s.is_player,
        job: -1,
        hand_item: 0,
        hawkeye: 0,
    })
}

/// `CActionMover::IsMove()` as the original AI saw it. The original steered (`ProcessMove`) **before** the AI
/// pass of a tick, so a mover that had a destination was in a move state by then. The shared sim steers inside
/// `step_frame` (31 §3.4: after the AI pass, on frames divisible by four), so a destination set at the previous
/// barrier has not made the state a move state yet: a destination that `ProcessMove` would steer toward counts
/// as moving (otherwise `IsEndStop` would start a second random walk right after a collision side step).
pub fn is_moving(m: &Mover) -> bool {
    m.sim.act.state.is_move() || (!m.sim.is_empty_dest() && !m.sim.act.state.is_act_attack())
}

/// `CMover::IsDie()`: the action mover is dead or the hit points are 0.
pub fn mover_is_die(m: &Mover) -> bool {
    m.sim.act.state.is_die() || m.mv.hp == 0
}

/// `GetMaxHitPoint()` of a monster. **Seam** for doc 16's `stats::max_hp` (WP-MOVER-04/05): until it exists the
/// NPC formula of `flyff_rules::stats` over the empty DST table is applied, as `Mover::new_monster` does at
/// construction; a mover whose prop is unknown keeps its current hit points as its maximum.
pub fn max_hp(m: &Mover, cx: &AiCx<'_>) -> i32 {
    match cx.shared.data.mover(m.prop_id()) {
        Some(prop) => {
            let origin = npc_max_hp_origin(prop.add_hp, 1.0, m.mv.rates.hp);
            final_max(&m.mv.dst.table, DST_HP_MAX, DST_HP_MAX_RATE, origin)
        }
        None => m.mv.hp.max(1),
    }
}

/// The stat inputs of an NPC (no weapon): what the pure formulas of `flyff_rules::stats` read.
fn npc_inputs<'a>(m: &'a Mover, prop: &'a MoverProp) -> StatInputs<'a> {
    StatInputs {
        is_player: false,
        level: m.mv.level,
        job: m.mv.job,
        base: m.mv.base,
        dst: &m.mv.dst.table,
        equip_cache: m.mv.equip_cache,
        weapon: None,
        left_weapon: None,
        mover_prop: prop,
        rates: m.mv.rates,
        sm_attack_up: false,
    }
}

/// The rule tables of a player-less formula (an NPC reads none of them).
static NO_RULES_DATA: LazyLock<RulesData> = LazyLock::new(RulesData::default);

/// `GetMaxManaPoint()` of a monster. **Seam** for doc 16's `stats::env::max_mp`.
pub fn max_mp(m: &Mover, cx: &AiCx<'_>) -> i32 {
    match cx.shared.data.mover(m.prop_id()) {
        Some(prop) => flyff_rules::stats::max_mp(&npc_inputs(m, prop), &NO_RULES_DATA),
        None => m.mv.mp.max(1),
    }
}

/// `GetHPRecovery()` of a monster. **Seam** for doc 16's `stats::env::recoveries`.
pub fn hp_recovery(m: &Mover, cx: &AiCx<'_>) -> i32 {
    match cx.shared.data.mover(m.prop_id()) {
        Some(prop) => flyff_rules::stats::hp_recovery(&npc_inputs(m, prop), &NO_RULES_DATA),
        None => 0,
    }
}

/// `SetPointParam(dst, v)` for the pools of a monster: `v` below 0 becomes 0, above `max` becomes `max`; the
/// viewers hear `SetPointParam` when the stored value changed (`MoverParam.cpp:555-590`).
fn set_pool(m: &mut Mover, cx: &mut AiCx<'_>, dst: u32, v: i32, max: i32) {
    let v = v.max(0).min(max);
    let slot = if dst == DST_HP {
        &mut m.mv.hp
    } else {
        &mut m.mv.mp
    };
    if *slot == v {
        return;
    }
    *slot = v;
    let msg = EncodedMsg::new(
        m.core.id.0,
        pb::SetPointParam {
            dst: dst as i32,
            value: v,
        },
    );
    intents::deliver(&mut *cx.sink, m, IntentAudience::Viewers, &msg);
}

/// `IncHitPoint(n)`: `SetPointParam(DST_HP, hp + n)`. **Seam** for doc 16's `stats::pool::inc_hp`.
pub fn inc_hp(m: &mut Mover, cx: &mut AiCx<'_>, n: i32) {
    let max = max_hp(m, cx);
    let v = m.mv.hp.wrapping_add(n);
    set_pool(m, cx, DST_HP, v, max);
}

/// `IncManaPoint(n)`: `SetPointParam(DST_MP, mp + n)`. **Seam** for doc 16's `stats::pool::inc_mp`.
pub fn inc_mp(m: &mut Mover, cx: &mut AiCx<'_>, n: i32) {
    let max = max_mp(m, cx);
    let v = m.mv.mp.wrapping_add(n);
    set_pool(m, cx, DST_MP, v, max);
}

/// `GetHitPointPercent(100)`: `MulDiv(hp, 100, max)` (rounded to nearest; 0 when the maximum is 0).
pub fn hp_percent(m: &Mover, cx: &AiCx<'_>) -> i32 {
    let max = max_hp(m, cx);
    if max == 0 {
        return 0;
    }
    mul_div(m.mv.hp, 100, max)
}

/// `SetPointParam(DST_HP, GetMaxHitPoint())`: sets the hit points to the maximum and tells the viewers when
/// they changed (`AddSetPointParam`). **Seam** for doc 16's `stats::set_hp`.
pub fn heal_full(m: &mut Mover, cx: &mut AiCx<'_>) {
    let max = max_hp(m, cx);
    if m.mv.hp != max {
        m.mv.hp = max;
        let msg = EncodedMsg::new(
            m.core.id.0,
            pb::SetPointParam {
                dst: DST_HP as i32,
                value: max,
            },
        );
        intents::deliver(&mut *cx.sink, m, IntentAudience::Viewers, &msg);
    }
}

/// `RemoveAllEnemies()`: forgets everybody who hit me. The list is cleared here; the enemies learn about it (their
/// destination object and melee target are cleared, an enemy monster hears `DstDie`) through
/// [`AiCmd::RemoveEnemies`], which the pipeline hands to doc 17's `combat::hate::remove_enemy`.
pub fn remove_all_enemies(m: &mut Mover, cx: &mut AiCx<'_>) {
    let enemies = m.combat.hate.ids();
    m.combat.hate.clear();
    if !enemies.is_empty() {
        cx.cmds.push(AiCmd::RemoveEnemies {
            who: m.id(),
            enemies,
        });
    }
}

/// `IsInRange(v, r)` with `__JEFF_11_6`: the height is ignored, `|v|^2 <= r^2` (the original's
/// `fDistSq > fRange * fRange` is the negation).
// the negated comparison is the original's, and the NaN behaviour (in range) follows it
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn is_in_range(v: Vec3, r: f32) -> bool {
    let d_sq = v.x * v.x + v.z * v.z;
    !(d_sq > r * r)
}

impl MonsterAi {
    /// `SetTarget(id, party)`; clearing the target resets the speed factor to 1.0 (`__AI_0509`).
    pub fn set_target(&mut self, m: &mut Mover, id: Option<ObjId>, party: u32) {
        self.target = id;
        self.target_party = party;
        if id.is_none() {
            intents::set_speed_factor(m, 1.0);
        }
    }

    /// `SetStop(ms)`: no new walk for `ms`; the server copy stands and forgets its destination object (the
    /// destination position stays); nothing is broadcast.
    pub fn set_stop(&mut self, m: &mut Mover, cx: &mut AiCx<'_>, ms: u64) {
        self.tm_move = GameMs(cx.now.0.saturating_add(ms));
        cx.send_act(m, ActMsg::new(OBJMSG_STAND));
        m.sim.clear_dest_obj();
    }

    /// `IsEndStop()`: the stop time is over and the mover is not in a move state.
    pub fn is_end_stop(&self, m: &Mover, cx: &AiCx<'_>) -> bool {
        cx.now > self.tm_move && !is_moving(m)
    }

    /// `MoveToDst(pos)`: walk to `v` (no-op when that is already the destination).
    pub fn move_to_pos(&mut self, m: &mut Mover, v: Vec3) {
        intents::move_to_pos(m, v);
    }

    /// `MoveToDst(id)`: chase `id` at the chase speed. No-op when it already is the destination object;
    /// otherwise the speed factor is announced first, the target is remembered (with its party) when valid,
    /// and the destination object is announced.
    pub fn move_to_obj(&mut self, m: &mut Mover, cx: &AiCx<'_>, id: ObjId) {
        if m.sim.dest.obj == id {
            return;
        }
        // `SetSpeedFactor(2.0)` (inside `intents::move_to_obj`) precedes `SetTarget` in the original; the
        // latter only stores, so the order of the two is not observable
        if let Some(t) = target_view(cx, id) {
            self.target = Some(id);
            self.target_party = t.party;
        }
        intents::move_to_obj(m, id);
    }

    /// `MoveToRandom(state)` (18 §8.8): a point `(x % 21 - 10, z % 21 - 10)` metres around the current
    /// position (x drawn first). In IDLE it must stay within 30 m (90 m flying) of the spawn point; a monster that
    /// is returning accepts any point (collision avoidance); one that already is outside starts the way home.
    pub fn move_to_random(&mut self, m: &mut Mover, cx: &mut AiCx<'_>, state: MonsterState) {
        let mut v = m.sim.pos;
        let x1 = x_rand(cx.rng);
        let x2 = x_rand(cx.rng);
        v.x += ((x1 % 21) as i32 - 10) as f32;
        v.z += ((x2 % 21) as i32 - 10) as f32;
        if state == MonsterState::Idle {
            let mut range = RANGE_MOVE;
            if m.sim.params.flying_npc {
                range *= 3.0;
            }
            // inside the circle, or a monster that is on its way home (a random call then is collision
            // avoidance): walk; one that already is outside starts the way home
            if is_in_range(v - self.spawn_pos, range) || self.returning {
                self.move_to_pos(m, v);
            } else if !is_in_range(m.sim.pos - self.spawn_pos, range) {
                self.do_return_to_begin(m, cx, true);
            }
        } else {
            self.move_to_pos(m, v);
        }
    }

    /// `DoReturnToBegin(bReturn)` (18 §9.10). `true`: start the way home at speed factor 2.66 (target dropped).
    /// `false`: the way is over; target dropped, hit points restored, enemies forgotten.
    pub fn do_return_to_begin(&mut self, m: &mut Mover, cx: &mut AiCx<'_>, ret: bool) {
        if ret {
            self.returning = true;
            self.tm_return = cx.now;
            self.set_target(m, None, 0);
            intents::set_speed_factor(m, SPEED_FACTOR_RETURN);
            let home = self.spawn_pos;
            self.move_to_pos(m, home);
        } else {
            self.returning = false;
            self.set_target(m, None, 0);
            heal_full(m, cx);
            remove_all_enemies(m, cx);
        }
    }

    /// `MoveProcess*`'s `SetPos(spawn)`: the anti-stuck warp home, announced to the viewers.
    pub fn warp_home(&mut self, m: &mut Mover, cx: &AiCx<'_>) {
        intents::warp_npc(m, self.spawn_pos);
        self.tm_return = cx.now;
    }
}
