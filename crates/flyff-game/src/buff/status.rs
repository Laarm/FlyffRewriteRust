//! Abnormal states of a mover (Plan/17 §7.7; `MoverParam.cpp:2026-2208`, `Mover.cpp:1199-1303`; WP-SKILL-07):
//! the counters, the `Set*` functions that `ApplyParam` and `RemoveDebuff` call and the per-tick work.
//!
//! A state is a `DST_CHRSTATE` bit held by a [`DstSource`] - the buff that carries it, or the counter-driven
//! [`DstSource::Abnormal`] - so the exact revert of the ledger ends it. The `Set*` functions of the original are
//! here with their immunity rules (`DST_IMMUNITY`, the boss ranks).
//!
//! **Per tick** ([`process_abnormal_state`], `CMover::ProcessAbnormalState`, run for every awake mover with a
//! state by [`super::tick::process_abnormal_all`] right after the world phase):
//!
//! | state | what a tick does |
//! |---|---|
//! | stun, dark cover, dark | a counter set by `Set*` (not buff-driven: 0) counts down; at zero the state ends (`SetStun{0}` goes out for a stun) |
//! | poison, bleeding | while the bit is set and the mover is alive: `n = (unit_ms / 1000.0 * 15.0) as i32`; `n <= 0` ends the state; on `process count % n == 0` a forced hit (`AF_FORCE`, the stored damage) from the stored attacker if it is still a mover; a direct application's `ticks_left` counts down |
//! | `DST_HEAL` | with `adj(DST_HEAL) > 0` the counter counts down; at zero, if alive: `features.heal_over_time` adds `adj` to the HP and re-arms 45 ticks, else sets the HP to `adj` and re-arms 90 |
//!
//! The buff-driven states (every shipped skill: the counters are zeroed after `ApplyParam`) end with their
//! buff. `Mover::count` is incremented by the tick tail, which runs before this pass, so the phase of the
//! poison test is `count - 1` (the original increments at the very end of the mover's process).

use super::core::BuffCx;
use crate::combat::SimFx;
use crate::combat::state::is_live;
use crate::follow::Followup;
use crate::item::fx::Fx;
use crate::mover::Mover;
use crate::stats::dst::DstSource;
use crate::stats::env::prop_of;
use crate::stats::pool::set_point_param;
use crate::stats::source::{apply_dst, remove_source};
use flyff_common::ObjId;
use flyff_common::defs::attribute::{RANK_MIDBOSS, RANK_SUPER};
use flyff_proto::pb;
use flyff_rules::consts::NULL_CHGPARAM;
use flyff_rules::consts::dst::*;

/// `PROCESS_COUNT`: server ticks per second.
const TICKS_PER_SEC: f32 = 15.0;

/// A damage-over-time state (poison, bleeding).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dot {
    /// Who deals the damage (`m_idPoisonAttacker`); `None` ends the damage.
    pub attacker: Option<ObjId>,
    /// Damage per tick of the state (`m_wPoisonDamage`).
    pub damage: i32,
    /// The interval in ms (`m_tmPoisonUnit`).
    pub unit_ms: u32,
    /// Ticks left; 0 = driven by the buff that holds the bit.
    pub ticks_left: u16,
}

/// Abnormal-state counters (`m_nHealCnt`, stun, poison, ...). The heal counter belongs to the DST engine
/// (16 §4.2 step 4); the rest are set here and counted down by WP-SKILL-07.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StatusEffects {
    /// `m_nHealCnt`: ticks until the next `DST_HEAL` tick (`flyff_rules::consts::HEAL_TICKS`).
    pub heal_cnt: i16,
    /// `m_wStunCnt`: ticks until the stun ends; 0 with a buff-driven stun.
    pub stun_ticks: u16,
    /// `m_wDarkCover`: ticks of the invisibility; 0 when buff-driven.
    pub dark_cover_ticks: u16,
    /// The poison.
    pub poison: Option<Dot>,
    /// The bleeding.
    pub bleeding: Option<Dot>,
    /// Darkness: the hit-rate change (`m_wDarkVal`, negative) and the ticks left (0 when buff-driven).
    pub dark: Option<(i32, u16)>,
}

/// `(short)((ms / 1000.0f) * PROCESS_COUNT)`.
fn ticks_of(ms: u32) -> u16 {
    ((ms as f32 / 1000.0) * TICKS_PER_SEC) as i16 as u16
}

fn immune(m: &Mover, bit: u32) -> bool {
    m.mv.dst.table.adj(DST_IMMUNITY) as u32 & bit != 0
}

fn boss_rank(m: &Mover, cx: &BuffCx<'_>) -> bool {
    let class = prop_of(m, cx.stats()).class;
    class == RANK_SUPER || class == RANK_MIDBOSS
}

fn set_bit(m: &mut Mover, cx: &BuffCx<'_>, src: DstSource, bit: u32, send: bool, fx: &mut Fx) {
    apply_dst(
        m,
        cx.stats(),
        src,
        (DST_CHRSTATE, bit as i32, NULL_CHGPARAM),
        send,
        &mut fx.out,
    );
}

/// `SetDarkCover(TRUE, ms)`: invisibility; `false` when the mover is immune.
pub fn set_dark_cover(
    m: &mut Mover,
    cx: &BuffCx<'_>,
    src: DstSource,
    ms: u32,
    fx: &mut Fx,
) -> bool {
    if immune(m, CHS_INVISIBILITY) {
        return false;
    }
    set_bit(m, cx, src, CHS_INVISIBILITY, true, fx);
    m.status.dark_cover_ticks = ticks_of(ms);
    true
}

/// `SetDarkCover(FALSE)`: ends a counter-driven invisibility (a buff-driven one ends with its buff).
pub fn clear_dark_cover(m: &mut Mover, cx: &BuffCx<'_>, fx: &mut Fx) {
    remove_source(
        m,
        cx.stats(),
        DstSource::Abnormal {
            state: CHS_INVISIBILITY,
        },
        true,
        &mut fx.out,
    );
    m.status.dark_cover_ticks = 0;
}

/// `SetStun(TRUE, ms)`: refused for `RANK_SUPER` and `RANK_MIDBOSS` and with the stun immunity. The state bit is
/// applied without a message, the sim starts the groggy action, `SetStun{ms}` goes to everybody around.
pub fn set_stun(m: &mut Mover, cx: &BuffCx<'_>, src: DstSource, ms: u32, fx: &mut Fx) -> bool {
    if boss_rank(m, cx) || immune(m, CHS_STUN) {
        return false;
    }
    set_bit(m, cx, src, CHS_STUN, false, fx);
    let id = m.id();
    fx.then(Followup::Sim {
        mover: id,
        fx: SimFx::Stun,
    });
    m.status.stun_ticks = ticks_of(ms);
    fx.out.near(pb::SetStun { duration_ms: ms });
    true
}

/// `SetStun(FALSE)`: ends a counter-driven stun.
pub fn clear_stun(m: &mut Mover, cx: &BuffCx<'_>, fx: &mut Fx) {
    remove_source(
        m,
        cx.stats(),
        DstSource::Abnormal { state: CHS_STUN },
        false,
        &mut fx.out,
    );
    m.status.stun_ticks = 0;
    fx.out.near(pb::SetStun { duration_ms: 0 });
}

/// `SetPoison(TRUE, attacker, ms, unit_ms, damage)`.
#[allow(clippy::too_many_arguments)]
pub fn set_poison(
    m: &mut Mover,
    cx: &BuffCx<'_>,
    src: DstSource,
    attacker: Option<ObjId>,
    ms: u32,
    unit_ms: u32,
    damage: i32,
    fx: &mut Fx,
) -> bool {
    if immune(m, CHS_POISON) {
        return false;
    }
    set_bit(m, cx, src, CHS_POISON, true, fx);
    m.status.poison = Some(Dot {
        attacker,
        damage,
        unit_ms,
        ticks_left: ticks_of(ms),
    });
    true
}

/// `SetPoison(FALSE)`.
pub fn clear_poison(m: &mut Mover, cx: &BuffCx<'_>, fx: &mut Fx) {
    remove_source(
        m,
        cx.stats(),
        DstSource::Abnormal { state: CHS_POISON },
        true,
        &mut fx.out,
    );
    m.status.poison = None;
}

/// `SetBleeding(TRUE, attacker, ms, unit_ms, damage)`.
#[allow(clippy::too_many_arguments)]
pub fn set_bleeding(
    m: &mut Mover,
    cx: &BuffCx<'_>,
    src: DstSource,
    attacker: Option<ObjId>,
    ms: u32,
    unit_ms: u32,
    damage: i32,
    fx: &mut Fx,
) -> bool {
    if immune(m, CHS_BLEEDING) {
        return false;
    }
    set_bit(m, cx, src, CHS_BLEEDING, true, fx);
    m.status.bleeding = Some(Dot {
        attacker,
        damage,
        unit_ms,
        ticks_left: ticks_of(ms),
    });
    true
}

/// `SetBleeding(FALSE)`.
pub fn clear_bleeding(m: &mut Mover, cx: &BuffCx<'_>, fx: &mut Fx) {
    remove_source(
        m,
        cx.stats(),
        DstSource::Abnormal {
            state: CHS_BLEEDING,
        },
        true,
        &mut fx.out,
    );
    m.status.bleeding = None;
}

/// `SetDark(TRUE, ms, adj_hit_rate)`: the state bit and the hit-rate change under one source.
pub fn set_dark(
    m: &mut Mover,
    cx: &BuffCx<'_>,
    src: DstSource,
    ms: u32,
    adj_hit_rate: i32,
    fx: &mut Fx,
) -> bool {
    if immune(m, CHS_DARK) {
        return false;
    }
    set_bit(m, cx, src, CHS_DARK, true, fx);
    apply_dst(
        m,
        cx.stats(),
        src,
        (DST_ADJ_HITRATE, adj_hit_rate, NULL_CHGPARAM),
        true,
        &mut fx.out,
    );
    m.status.dark = Some((adj_hit_rate, ticks_of(ms)));
    true
}

/// `SetDark(FALSE)`: ends a counter-driven darkness (the bit and the hit-rate change).
pub fn clear_dark(m: &mut Mover, cx: &BuffCx<'_>, fx: &mut Fx) {
    remove_source(
        m,
        cx.stats(),
        DstSource::Abnormal { state: CHS_DARK },
        true,
        &mut fx.out,
    );
    m.status.dark = None;
}

/// `CMover::RemoveDebuff(state)` for the states that are not held by a buff (the buffs are removed by the
/// caller first, `RemoveChrStateBuffs`): the direct counters of the states named by `state`.
pub fn clear_direct(m: &mut Mover, cx: &BuffCx<'_>, state: u32, fx: &mut Fx) {
    if state & CHS_DARK != 0 {
        clear_dark(m, cx, fx);
    }
    if state & CHS_POISON != 0 {
        clear_poison(m, cx, fx);
    }
    if state & CHS_STUN != 0 {
        clear_stun(m, cx, fx);
    }
    if state & CHS_BLEEDING != 0 {
        clear_bleeding(m, cx, fx);
    }
}

// ---- the per-tick work ---------------------------------------------------------------------------------------------

/// `true` when [`process_abnormal_state`] has anything to do for `m`: a counter, a poison or bleeding, the
/// state bits, or a `DST_HEAL` adjustment.
pub fn needs_pass(m: &Mover) -> bool {
    let s = &m.status;
    s.stun_ticks > 0
        || s.dark_cover_ticks > 0
        || s.poison.is_some()
        || s.bleeding.is_some()
        || s.dark.is_some()
        || m.mv.dst.table.adj(DST_CHRSTATE) as u32 & (CHS_POISON | CHS_BLEEDING) != 0
        || m.mv.dst.table.adj(DST_HEAL) > 0
}

/// Which damage-over-time state.
#[derive(Clone, Copy)]
enum Which {
    Poison,
    Bleeding,
}

/// The poison / bleeding block of `ProcessAbnormalState`.
fn tick_dot(m: &mut Mover, cx: &BuffCx<'_>, which: Which, fx: &mut Fx) {
    let (bit, slot) = match which {
        Which::Poison => (CHS_POISON, m.status.poison),
        Which::Bleeding => (CHS_BLEEDING, m.status.bleeding),
    };
    if m.mv.dst.table.adj(DST_CHRSTATE) as u32 & bit == 0 || !is_live(m) {
        return;
    }
    // a state set without a `Set*` call has no unit: it ends at once
    let dot = slot.unwrap_or(Dot {
        attacker: None,
        damage: 0,
        unit_ms: 0,
        ticks_left: 0,
    });
    let every = ((dot.unit_ms as f32 / 1000.0_f32) * TICKS_PER_SEC) as i32;
    let clear = |m: &mut Mover, fx: &mut Fx| match which {
        Which::Poison => clear_poison(m, cx, fx),
        Which::Bleeding => clear_bleeding(m, cx, fx),
    };
    if every <= 0 {
        clear(m, fx);
        return;
    }
    // `m_nCount` is incremented at the end of the mover's process: the tail already did it
    let phase = m.mv.count.wrapping_sub(1) as i32;
    if phase % every == 0 {
        fx.then(Followup::ForceDamage {
            victim: m.id(),
            attacker: dot.attacker,
            amount: dot.damage,
        });
    }
    if dot.ticks_left > 0 {
        let left = dot.ticks_left - 1;
        let slot = match which {
            Which::Poison => &mut m.status.poison,
            Which::Bleeding => &mut m.status.bleeding,
        };
        if let Some(d) = slot {
            d.ticks_left = left;
        }
        if left == 0 {
            clear(m, fx);
        }
    }
}

/// `CMover::ProcessAbnormalState()` (see the module documentation). The forced hits of the poison and the
/// bleeding are follow-ups of `fx` (they need the damage pipeline).
pub fn process_abnormal_state(m: &mut Mover, cx: &BuffCx<'_>, fx: &mut Fx) {
    if m.status.stun_ticks > 0 {
        m.status.stun_ticks -= 1;
        if m.status.stun_ticks == 0 {
            clear_stun(m, cx, fx);
        }
    }
    if m.status.dark_cover_ticks > 0 {
        m.status.dark_cover_ticks -= 1;
        if m.status.dark_cover_ticks == 0 {
            clear_dark_cover(m, cx, fx);
        }
    }
    tick_dot(m, cx, Which::Poison, fx);
    if let Some((_, ticks)) = &mut m.status.dark
        && *ticks > 0
    {
        *ticks -= 1;
        if *ticks == 0 {
            clear_dark(m, cx, fx);
        }
    }
    tick_dot(m, cx, Which::Bleeding, fx);
    // a state that ended with its buff leaves its record behind: forget it
    let chr = m.mv.dst.table.adj(DST_CHRSTATE) as u32;
    if chr & CHS_POISON == 0 {
        m.status.poison = None;
    }
    if chr & CHS_BLEEDING == 0 {
        m.status.bleeding = None;
    }
    if chr & CHS_DARK == 0 {
        m.status.dark = None;
    }
    // the heal over time
    let heal = m.mv.dst.table.adj(DST_HEAL);
    if heal > 0 {
        if m.status.heal_cnt > 0 {
            m.status.heal_cnt -= 1;
        }
        if m.status.heal_cnt == 0 && is_live(m) {
            let stats = cx.stats();
            m.status.heal_cnt = stats.heal_ticks;
            // `__HEAL_OVER_TIME`: +adj every 3 s, else the HP is *set* to adj every 6 s
            let adds = stats.heal_ticks == flyff_rules::consts::HEAL_TICKS;
            let value = if adds {
                crate::stats::env::point_param(m, stats, DST_HP).wrapping_add(heal)
            } else {
                heal
            };
            set_point_param(m, stats, DST_HP, value, false, &mut fx.out);
        }
    }
}
