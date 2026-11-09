//! The trade commit (Plan/19 §8.3, VR-018; src:_Common/MoverItem.cpp:351-518): validate everything without
//! mutating, simulate the whole exchange on clones of the two inventories, then install the clones in one
//! panic-free block and queue exactly one `AtomicBatch{Trade, [snap(a), snap(b)]}`.
//!
//! **Validate** ([`prepare`]): both avatars online in the same world with consistent partner links; every offer
//! still there (same storage entry, serial and prop, `reserved == offered <= count`) and still tradeable by
//! `flyff_rules::item::tradeable` (an expiry may have happened); the penya on both sides (`offer <= wallet`,
//! `wallet - give + get` inside `[0, GOLD_MAX]`, `CheckTradeGold`); the legacy space rule (free bag slots >= the
//! number of entries the other side offers, stacking ignored, KQ-15); both snapshots buildable. The fresh serials
//! of the split stacks are allocated here, so the commit has no fallible allocation.
//!
//! **Simulate**: the same `take` / `put` primitives the commit uses run on cloned containers; a failure here
//! means the checks above were wrong, which is a bug: logged as an error and answered with the error form.
//!
//! **Commit** ([`apply`]): install the simulated inventories, move the penya, clear the links, send the explicit
//! deltas (`UpdateItem` for every stack a side gave, `CreateItem` for every entry it received, `SetPointParam`
//! DST_GOLD to itself) and `TradeConsent` (PL-063), build the two snapshots with fresh generations, queue the
//! batch, write the audit lines. Nothing in it can fail or panic; a panic in the validate phase leaves both
//! players untouched because nothing was changed yet.

use super::cancel::{Cancel, cancel_session};
use super::moves::{
    Arrived, MoveError, PoolSerials, Taken, create_msg, put_stack, serials_needed, take_stack,
    update_msg,
};
use super::snap::{atomic_snapshot, can_snapshot};
use super::{TradeId, TradeSession, send};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::econ::TradePartner;
use crate::item::container::{ItemContainer, StorageId};
use crate::item::fx::Fx;
use crate::item::gold::GOLD_MAX;
use crate::item::ops::CtxSerials;
use flyff_common::{CharacterId, ObjId};
use flyff_db::AtomicBatch;
use flyff_db::audit::econ::trade_complete;
use flyff_proto::pb;
use flyff_rules::item::{TradeView, tradeable};

/// Why a commit was refused (every one of them ends the trade with the error form).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CommitError {
    /// The session or an avatar is gone.
    #[error("the session or an avatar is gone")]
    Gone,
    /// The players are not in the same world or the links are broken.
    #[error("the players are not in the same world or the links are broken")]
    World,
    /// An offer is not what it was when it was put (moved, changed, no longer tradeable).
    #[error("side {side} slot {slot}: the offer is no longer valid")]
    Offer {
        /// 0 requester, 1 accepter.
        side: usize,
        /// The trade slot.
        slot: usize,
    },
    /// The penya do not add up (`CheckTradeGold`).
    #[error("the penya of side {0} do not add up")]
    Gold(usize),
    /// A side has too few free slots for what the other offers.
    #[error("side {0} has too few free slots")]
    Space(usize),
    /// A snapshot cannot be built (tainted session).
    #[error("a snapshot cannot be built")]
    Snapshot,
    /// A fresh serial could not be allocated.
    #[error("no serial: {0}")]
    Serial(String),
    /// The simulation failed although the checks passed: a bug.
    #[error("the simulated exchange failed: {0}")]
    Simulation(String),
}

impl CommitError {
    /// `true` for the errors that are bugs (logged at error level).
    pub fn is_bug(&self) -> bool {
        matches!(self, CommitError::Simulation(_))
    }
}

/// Everything the commit does, decided without mutating any player.
struct Plan {
    ids: [ObjId; 2],
    chars: [CharacterId; 2],
    inv: [ItemContainer; 2],
    gold: [i32; 2],
    offer_gold: [i64; 2],
    taken: [Vec<Taken>; 2],
    arrived: [Vec<Arrived>; 2],
}

/// What [`prepare`] reads before the serials are allocated.
struct Facts {
    session: TradeSession,
    inv: [ItemContainer; 2],
    wallet: [i64; 2],
    moves: [Vec<(StorageId, u16)>; 2],
}

/// The immutable half: every rule of §8.3 steps 1-4 on the current state.
fn gather(svc: &Services, ctx: &GameCtx<'_>, id: TradeId) -> Result<Facts, CommitError> {
    let core = svc.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    let now_unix = ctx.clock.unix_ms.to_unix_time().0;
    let session = svc.econ.trade.session(id).ok_or(CommitError::Gone)?.clone();
    let (a, b) = (session.a, session.b);
    let ma = ctx.worlds.player(a).ok_or(CommitError::Gone)?;
    let mb = ctx.worlds.player(b).ok_or(CommitError::Gone)?;
    let (pa, pb_) = (
        ma.player().ok_or(CommitError::Gone)?,
        mb.player().ok_or(CommitError::Gone)?,
    );
    if ma.core.world != mb.core.world
        || ma.core.layer != mb.core.layer
        || ma.core.world != session.world
        || pa.econ.partner != (TradePartner::Player { other: b })
        || pb_.econ.partner != (TradePartner::Player { other: a })
    {
        return Err(CommitError::World);
    }
    if !can_snapshot(ctx, a) || !can_snapshot(ctx, b) {
        return Err(CommitError::Snapshot);
    }
    let players = [pa, pb_];
    // step 2: the offers
    let mut moves: [Vec<(StorageId, u16)>; 2] = [Vec::new(), Vec::new()];
    for (i, p) in players.iter().enumerate() {
        for (slot, o) in session.side[i].offers() {
            let bad = CommitError::Offer { side: i, slot };
            let it = p
                .items
                .inventory
                .get_at_id(o.obj_id)
                .filter(|it| it.serial == o.serial && it.item_id == o.item_id)
                .ok_or_else(|| bad.clone())?;
            if o.count == 0
                || it.reserved != o.count
                || o.count > it.count
                || p.items.inventory.is_equip(o.obj_id)
                || p.items.is_using(o.obj_id)
            {
                return Err(bad);
            }
            let prop = env.data().item(it.item_id).ok_or_else(|| bad.clone())?;
            // the item is reserved by this very trade, so `reserved` does not count against it
            let view = TradeView {
                reserved: false,
                flags: it.flags.0,
                keep_until: it.keep_until,
                equipped: false,
                in_use: false,
                guild_id: it.guild_id,
                binds_by_option: it.is_binds(prop, env.cfg().only_armor_awakes),
            };
            if tradeable(&view, prop, now_unix).is_some() {
                return Err(bad);
            }
            moves[i].push((o.obj_id, o.count));
        }
    }
    // step 3: the penya (`CheckTradeGold`)
    let wallet = [i64::from(pa.items.gold), i64::from(pb_.items.gold)];
    for (i, &have) in wallet.iter().enumerate() {
        let give = session.side[i].gold;
        let after = have - give + session.side[1 - i].gold;
        if give < 0 || give > have || !(0..=GOLD_MAX).contains(&after) {
            return Err(CommitError::Gold(i));
        }
    }
    // step 4: the legacy space rule, stacking ignored
    for (i, p) in players.iter().enumerate() {
        if p.items.inventory.empty_count() < session.side[1 - i].offered() {
            return Err(CommitError::Space(i));
        }
    }
    Ok(Facts {
        inv: [pa.items.inventory.clone(), pb_.items.inventory.clone()],
        wallet,
        moves,
        session,
    })
}

fn map_move(e: MoveError) -> CommitError {
    match e {
        MoveError::Serial(s) => CommitError::Serial(s.to_string()),
        other => CommitError::Simulation(other.to_string()),
    }
}

/// Validate and simulate (§8.3 steps 1-4 and the clone simulation): no player is changed.
fn prepare(svc: &mut Services, ctx: &mut GameCtx<'_>, id: TradeId) -> Result<Plan, CommitError> {
    let Facts {
        session,
        mut inv,
        wallet,
        moves,
    } = gather(svc, ctx, id)?;
    // the fresh serials of the partial stacks, before anything changes
    let need = serials_needed(&inv[0], &moves[0]) + serials_needed(&inv[1], &moves[1]);
    let mut fresh = Vec::with_capacity(need);
    {
        let Services { serials, .. } = &mut *svc;
        let mut src = CtxSerials::new(serials, ctx.persist, ctx.tickets);
        for _ in 0..need {
            let s = crate::item::container::SerialSource::next_serial(&mut src)
                .map_err(|e| CommitError::Serial(e.to_string()))?;
            fresh.push(s);
        }
    }
    let core = svc.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    let mut pool = PoolSerials::new(fresh);
    // item_step1: both sides give; item_step2/3: both sides receive
    let mut taken: [Vec<Taken>; 2] = [Vec::new(), Vec::new()];
    for i in 0..2 {
        for &(obj, n) in &moves[i] {
            taken[i].push(take_stack(&mut inv[i], obj, n, 0, &mut pool).map_err(map_move)?);
        }
    }
    let mut arrived: [Vec<Arrived>; 2] = [Vec::new(), Vec::new()];
    for i in 0..2 {
        for t in &taken[1 - i] {
            arrived[i].push(put_stack(&mut inv[i], t, env.data(), &mut pool).map_err(map_move)?);
        }
    }
    let offer_gold = [session.side[0].gold, session.side[1].gold];
    let gold = |i: usize| (wallet[i] - offer_gold[i] + offer_gold[1 - i]) as i32;
    Ok(Plan {
        ids: [session.a, session.b],
        chars: session.chars,
        inv,
        gold: [gold(0), gold(1)],
        offer_gold,
        taken,
        arrived,
    })
}

/// The commit block: install the simulated inventories, move the penya, send the deltas, queue the batch.
fn apply(svc: &mut Services, ctx: &mut GameCtx<'_>, id: TradeId, plan: Plan) {
    let Plan {
        ids,
        chars,
        inv,
        gold,
        offer_gold,
        taken,
        arrived,
    } = plan;
    svc.econ.trade.remove(id);
    svc.econ.trade.stats.completed += 1;
    let core = svc.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    let names: [String; 2] = std::array::from_fn(|i| {
        ctx.worlds
            .player(ids[i])
            .and_then(|m| m.player())
            .map(|p| p.ident.name.clone())
            .unwrap_or_default()
    });
    let [inv_a, inv_b] = inv;
    for (i, inv) in [inv_a, inv_b].into_iter().enumerate() {
        let mut fx = Fx::new();
        if let Some(m) = ctx.worlds.player_mut(ids[i]) {
            if let Some(p) = m.player_mut() {
                p.items.inventory = inv;
                p.items.gold = gold[i];
                p.econ.partner = TradePartner::None;
                p.econ.interrupted = false;
            }
            for t in &taken[i] {
                fx.out.to_self(update_msg(t));
            }
            for a in &arrived[i] {
                fx.out.to_self(create_msg(a));
            }
            crate::stats::pool::emit_gold(m, env.stats(), &mut fx.out);
        }
        fx.flush(svc, ctx, PlayerRef(ids[i]));
        send(ctx, ids[i], ObjId::NULL, pb::TradeConsent {});
    }
    // one batch: [snap(a), snap(b)] (VR-018)
    match (
        atomic_snapshot(svc, ctx, ids[0]),
        atomic_snapshot(svc, ctx, ids[1]),
    ) {
        (Some(sa), Some(sb)) => ctx.persist.atomic(AtomicBatch::trade(sa, sb)),
        _ => {
            // cannot happen after `can_snapshot`; the periodic snapshot still covers both
            tracing::error!(
                a = ids[0].0,
                b = ids[1].0,
                "trade committed without a batch"
            );
            svc.saves.mark_urgent(ids[0]);
            svc.saves.mark_urgent(ids[1]);
        }
    }
    for i in 0..2 {
        let Some(actor) = crate::audit::actor_of(ctx, PlayerRef(ids[i])) else {
            continue;
        };
        let gave: Vec<_> = taken[i].iter().map(|t| t.item.to_record(0)).collect();
        let got: Vec<_> = taken[1 - i].iter().map(|t| t.item.to_record(0)).collect();
        ctx.audit.emit(trade_complete(
            actor,
            chars[1 - i],
            &names[1 - i],
            &gave,
            &got,
            offer_gold[i],
            offer_gold[1 - i],
        ));
    }
}

/// `TradeLastConfirm` (§8.3): commit the trade `id` or end it with the error form.
pub fn run(svc: &mut Services, ctx: &mut GameCtx<'_>, id: TradeId) {
    match prepare(svc, ctx, id) {
        Ok(plan) => apply(svc, ctx, id, plan),
        Err(e) => {
            if e.is_bug() {
                tracing::error!(error = %e, "trade commit: the simulation failed");
            } else {
                tracing::debug!(error = %e, "trade commit refused");
            }
            cancel_session(svc, ctx, id, Cancel::Error);
        }
    }
}
