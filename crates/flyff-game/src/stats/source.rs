//! Applying and removing DST sources on a mover (Plan/16 §4.5-§4.8, WP-MOVER-05).
//!
//! The functions here are the mover-level face of [`DstState`](super::dst::DstState): they compute the base of the rate composites,
//! run the ledger, turn its emissions into the legacy messages (`SetDestParam`, `ResetDestParam`,
//! `ResetDestParamSync`, `SetPointParam` for the immediate pool effects) and keep the sim's copy of the
//! DST-derived inputs current (`sim.params`, 31 §5.4). Docs 17 (buffs), 19 (equipment, pets, items) and 16
//! itself (SM timers, sets) call these; nobody else touches `mv.dst`.

use super::dst::{DstCtx, DstEmit, DstSource};
use super::env::{StatsCore, max_of, origin_max, point_param, refresh_sim_params};
use super::out::Out;
use super::pool::set_point_param;
use crate::mover::Mover;
use flyff_proto::pb;

/// `SetDestParam(dst, adj, chg)` for the slots of one source (an item prop's three slots, a buff's two):
/// composites expand, the aggregate changes, the ledger records, the messages are queued. `send = false`
/// suppresses the DST messages (the join batch and `redo_equip` at join: the spawn snapshot carries the result,
/// 16 §4.7 rule 1). Pool effects (`DST_HP` etc.) always run.
pub fn apply_source(
    m: &mut Mover,
    c: &StatsCore,
    src: DstSource,
    slots: &[(u32, i32, i32)],
    send: bool,
    out: &mut Out,
) {
    let origin = origin_max(m, c);
    let mut emits = Vec::new();
    {
        let mut ctx = DstCtx {
            origin,
            heal_cnt: &mut m.status.heal_cnt,
            heal_ticks: c.heal_ticks,
            emit: &mut emits,
            send,
        };
        m.mv.dst.apply_all(src, slots, &mut ctx);
    }
    finish(m, c, emits, out);
}

/// One slot: `SetDestParam(dst, adj, chg)`.
pub fn apply_dst(
    m: &mut Mover,
    c: &StatsCore,
    src: DstSource,
    slot: (u32, i32, i32),
    send: bool,
    out: &mut Out,
) {
    apply_source(m, c, src, &[slot], send, out);
}

/// Reverts everything `src` applied (`ResetDestParam` for each slot, exactly, 16 §4.5 / PL-080). A source that
/// holds nothing does nothing.
pub fn remove_source(m: &mut Mover, c: &StatsCore, src: DstSource, send: bool, out: &mut Out) {
    if !m.mv.dst.has(src) {
        return;
    }
    let origin = origin_max(m, c);
    let mut emits = Vec::new();
    {
        let mut ctx = DstCtx {
            origin,
            heal_cnt: &mut m.status.heal_cnt,
            heal_ticks: c.heal_ticks,
            emit: &mut emits,
            send,
        };
        m.mv.dst.remove(src, &mut ctx);
    }
    finish(m, c, emits, out);
}

/// Replaces the modifiers of `src` by `slots`: remove, then apply (equipment changes, buff refresh).
pub fn replace_source(
    m: &mut Mover,
    c: &StatsCore,
    src: DstSource,
    slots: &[(u32, i32, i32)],
    send: bool,
    out: &mut Out,
) {
    remove_source(m, c, src, send, out);
    apply_source(m, c, src, slots, send, out);
}

/// Queues the messages of the emissions in order, performs the pool effects, refreshes the sim inputs.
fn finish(m: &mut Mover, c: &StatsCore, emits: Vec<DstEmit>, out: &mut Out) {
    for e in emits {
        match e {
            DstEmit::Set { dst, adj, chg } => out.near(pb::SetDestParam {
                dst: dst as i32,
                adj,
                chg,
            }),
            DstEmit::Reset { dst, adj } => out.near(pb::ResetDestParam {
                dst: dst as i32,
                adj,
            }),
            DstEmit::ResetSync { dst, adj, total } => out.near(pb::ResetDestParamSync {
                dst: dst as i32,
                adj,
                total,
            }),
            DstEmit::Pool { dst, adj } => pool_effect(m, c, dst, adj, out),
        }
    }
    refresh_sim_params(m, c);
}

/// `SetDestParam(DST_HP / DST_MP / DST_FP, adj)` (`MoverParam.cpp:2283-2306`): `adj == -1` fills the pool,
/// otherwise `SetPointParam(dst, getter + adj)`.
fn pool_effect(m: &mut Mover, c: &StatsCore, dst: u32, adj: i32, out: &mut Out) {
    let value = if adj == -1 {
        max_of(m, c, dst)
    } else {
        point_param(m, c, dst).wrapping_add(adj)
    };
    set_point_param(m, c, dst, value, false, out);
}
