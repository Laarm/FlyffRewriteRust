//! Stat points (GP): allocation, the restat functions and their messages (Plan/16 §7, WP-MOVER-07; F-122, F-123,
//! VR-004, VR-087, PL-083).
//!
//! * [`inc_stat_level`]: `IncStatLevel` (ids 100 STR, 101 STA, 102 DEX, 103 INT, VR-004): one point into a stat,
//!   answered with `SetStatLevel` to the near set **only when a point was spent**;
//! * [`modify_status`]: `ModifyStatus` (VR-087): several points at once, answered with `SetState` to self;
//! * [`restat_full`] / [`restat_one`] / [`restat_one_low`]: the restat items (called by doc 19's item router): the
//!   stat falls back to 15, the points come back, the pools are set to their new maxima and sent as three forced
//!   `SetPointParam` (PL-083), then `SetState` to self.
//!
//! None of them touches the DST ledger: the stat sources are additive, a base-stat change cannot drift them
//! (PL-080). Every change calls [`stats_changed`].

use super::env::{StatsCore, refresh_sim_params};
use super::out::Out;
use super::pool::{fill_pools_silent, send_pools_absolute};
use crate::chat::ingress::notice::defined_text;
use crate::mover::Mover;
use flyff_proto::pb;
use flyff_rules::progression::{RESTAT_BASE, gp_total_for, restat_one_low as low_refund};

/// `IncStatLevel` id of STR.
pub const STAT_ID_STR: i32 = 100;
/// `IncStatLevel` id of STA.
pub const STAT_ID_STA: i32 = 101;
/// `IncStatLevel` id of DEX.
pub const STAT_ID_DEX: i32 = 102;
/// `IncStatLevel` id of INT.
pub const STAT_ID_INT: i32 = 103;

/// `TID_GAME_DONOTUSE_RESTATE`: a restat item with nothing to take back.
pub use flyff_common::defs::text::TID_GAME_DONOTUSE_RESTATE;

/// The stats of the restat items: `ReStateOne(kind)`, kind 0 = STR, 1 = STA, 2 = DEX, 3 = INT.
fn stat_slot(m: &mut Mover, kind: u8) -> Option<&mut i32> {
    match kind {
        0 => Some(&mut m.mv.base.str),
        1 => Some(&mut m.mv.base.sta),
        2 => Some(&mut m.mv.base.dex),
        3 => Some(&mut m.mv.base.int),
        _ => None,
    }
}

/// `stats_changed(m)` (16 §7.4): the derived-stat inputs of the sim (attack speed depends on DEX) follow the new
/// base stats. Base-stat changes never touch the DST ledger.
pub fn stats_changed(m: &mut Mover, c: &StatsCore) {
    refresh_sim_params(m, c);
}

/// `SetGrowthLearningPoint{remain_gp}` to self (`AddSetGrowthLearningPoint`): the remaining stat points changed
/// without the stats (level-up, GM commands; the level-up sequence is WP-MOVER-08's).
pub fn emit_growth_learning_point(m: &Mover, out: &mut Out) {
    let gp = m.player().map_or(0, |p| p.progression.stat_points);
    out.to_self(pb::SetGrowthLearningPoint {
        remain_gp: gp,
        remain_lp: 0,
    });
}

/// `SetState` to self with the current base stats and remaining points (`AddSetState`): answer of
/// `ModifyStatus` and the restat items, also sent after a level-up and by the GM commands.
pub fn emit_state(m: &Mover, out: &mut Out) {
    send_state(m, out);
}

/// `SetState` to self with the current base stats and remaining points (`AddSetState`).
fn send_state(m: &Mover, out: &mut Out) {
    let gp = m.player().map_or(0, |p| p.progression.stat_points);
    out.to_self(pb::SetState {
        str: m.mv.base.str,
        sta: m.mv.base.sta,
        dex: m.mv.base.dex,
        int: m.mv.base.int,
        remain_lp: 0,
        remain_gp: gp,
    });
}

/// `IncStatLevel{stat_id}` (VR-004, `MoverParam.cpp:960-1055`, `__S_ADD_RESTATE`): with at least one point left
/// the stat grows by 1 and the points shrink by 1 (no job table, no cap besides the point supply). Returns
/// `true` when a point was spent; an id outside 100..=103, a non-player and 0 points change nothing and send
/// nothing.
pub fn inc_stat_level(m: &mut Mover, c: &StatsCore, stat_id: i32, out: &mut Out) -> bool {
    let kind = match stat_id {
        STAT_ID_STR => 0,
        STAT_ID_STA => 1,
        STAT_ID_DEX => 2,
        STAT_ID_INT => 3,
        _ => return false,
    };
    let has_points = m.player().is_some_and(|p| p.progression.stat_points > 0);
    if !has_points {
        return false;
    }
    let value = match stat_slot(m, kind) {
        Some(v) => {
            *v = v.wrapping_add(1);
            *v
        }
        None => return false,
    };
    let remain_gp = match m.player_mut() {
        Some(p) => {
            p.progression.stat_points -= 1;
            p.progression.stat_points
        }
        None => return false,
    };
    out.near(pb::SetStatLevel {
        stat_id,
        value,
        remain_gp,
    });
    stats_changed(m, c);
    true
}

/// `ModifyStatus{str, sta, dex, int}` (VR-087, `DPSrvr.cpp:10475-10500`): every value `>= 0`, the sum (in `i64`)
/// above 0 and at most the remaining points, otherwise ignored. All four are added, the points shrink by the sum
/// and `SetState` goes to self. Returns `true` when applied; the caller runs the honor hooks.
pub fn modify_status(m: &mut Mover, c: &StatsCore, add: [i32; 4], out: &mut Out) -> bool {
    if add.iter().any(|v| *v < 0) {
        return false;
    }
    let sum: i64 = add.iter().map(|v| i64::from(*v)).sum();
    let Some(gp) = m.player().map(|p| i64::from(p.progression.stat_points)) else {
        return false;
    };
    if sum <= 0 || sum > gp {
        return false;
    }
    m.mv.base.str = m.mv.base.str.wrapping_add(add[0]);
    m.mv.base.sta = m.mv.base.sta.wrapping_add(add[1]);
    m.mv.base.dex = m.mv.base.dex.wrapping_add(add[2]);
    m.mv.base.int = m.mv.base.int.wrapping_add(add[3]);
    if let Some(p) = m.player_mut() {
        // `sum <= gp <= i32::MAX`
        p.progression.stat_points = (gp - sum) as i32;
    }
    send_state(m, out);
    stats_changed(m, c);
    true
}

/// The common tail of the three restat functions: pools to the new maxima (silent), `send_pools_absolute`
/// (PL-083: three forced `SetPointParam`), `SetState` to self, `stats_changed`.
fn finish_restat(m: &mut Mover, c: &StatsCore, out: &mut Out) {
    fill_pools_silent(m, c);
    send_pools_absolute(m, c, out);
    send_state(m, out);
    stats_changed(m, c);
}

/// A refused restat item (nothing to take back): the text goes to self, nothing else changes.
fn refuse(out: &mut Out) {
    out.to_self(defined_text(TID_GAME_DONOTUSE_RESTATE, Vec::new()));
}

/// `ReState` (`II_CHR_SYS_SCR_RESTATE`, 16 §7.3): all four stats back to 15 and the points recomputed from the
/// highest level ever reached ([`gp_total_for`]; the old points are replaced). Refused (nothing changes, the item
/// is not consumed) when all four stats are already 15. Returns the new point total, `None` when refused.
pub fn restat_full(m: &mut Mover, c: &StatsCore, out: &mut Out) -> Option<i32> {
    let b = m.mv.base;
    if [b.str, b.sta, b.dex, b.int]
        .iter()
        .all(|v| *v == RESTAT_BASE)
        || !m.is_player()
    {
        refuse(out);
        return None;
    }
    m.mv.base.str = RESTAT_BASE;
    m.mv.base.sta = RESTAT_BASE;
    m.mv.base.dex = RESTAT_BASE;
    m.mv.base.int = RESTAT_BASE;
    let (level, job) = (m.mv.level, m.mv.job);
    let gp = {
        let p = m.player_mut()?;
        p.progression.stat_points = gp_total_for(&c.rules, level, p.progression.death_level, job);
        p.progression.stat_points
    };
    finish_restat(m, c, out);
    Some(gp)
}

/// `ReStateOne(kind)` (`II_CHR_SYS_SCR_RESTATE_STR/STA/DEX/INT`): one stat back to 15, the difference becomes
/// points. Refused when that stat is already 15 (or `kind` is unknown). Returns the refund.
pub fn restat_one(m: &mut Mover, c: &StatsCore, kind: u8, out: &mut Out) -> Option<i32> {
    let Some(slot) = stat_slot(m, kind) else {
        refuse(out);
        return None;
    };
    if *slot == RESTAT_BASE {
        refuse(out);
        return None;
    }
    let refund = slot.wrapping_sub(RESTAT_BASE);
    *slot = RESTAT_BASE;
    if let Some(p) = m.player_mut() {
        p.progression.stat_points = p.progression.stat_points.wrapping_add(refund);
    }
    finish_restat(m, c, out);
    Some(refund)
}

/// `ReStateOneLow(kind)` (`..._LOW`): like [`restat_one`] but at most `RESTATE_LOW` (50) points come back: a stat
/// of 65 or more loses 50, a lower one everything above 15.
pub fn restat_one_low(m: &mut Mover, c: &StatsCore, kind: u8, out: &mut Out) -> Option<i32> {
    let Some(slot) = stat_slot(m, kind) else {
        refuse(out);
        return None;
    };
    if *slot == RESTAT_BASE {
        refuse(out);
        return None;
    }
    let (refund, new_value) = low_refund(*slot);
    *slot = new_value;
    if let Some(p) = m.player_mut() {
        p.progression.stat_points = p.progression.stat_points.wrapping_add(refund);
    }
    finish_restat(m, c, out);
    Some(refund)
}
