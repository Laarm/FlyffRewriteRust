//! The lord events (Plan/22 §9.5; WP-EVENT-11): global EXP and drop boosts that a player pays for with penya and
//! Perins and that run for 60 minutes.
//!
//! * [`create_event`] (`LEventCreateReq{e_index, i_index}`): validated in the order of `lordevent::CreateRequirements`
//!   (`slord.cpp:222-247`), then ONE commit block: the cost is taken (Perins first), the component is appended, the
//!   factors are recomputed, `lord_event_create` goes out with a snapshot after the debit and `LEventCreateNtf` to
//!   everybody. FIX (PL-103): a player who already runs an event, or a full list, is refused BEFORE any gold leaves;
//!   the original charged first and the database refused.
//! * [`minute`] (P4, after the election step): every event loses a minute, the rows are written, ONE `LEventTick` with
//!   every entry (the zeros included) goes to everybody, the zeros are removed and the factors recomputed. While no
//!   event runs nothing is sent (the original sent an empty tick every minute; the client is equivalent).
//! * [`initialize`] (`/LEventInitialize`, the admin console): every event is cleared.
//!
//! The factors are `lord_exp = 1 + sum fE` and `lord_item = 1 + sum fI` (f32, list order) in
//! `EventModifiers`; the EXP and drop code read them from there ([`publish_factors`]).

use super::{LEVENT_TICKS, LordEvent};
use crate::chat::ingress::notice;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::econ::mail::commit::{can_snapshot, snapshot_for_batch};
use crate::item::fx::Fx;
use crate::item::gold::total::{remove_total_gold, total_gold};
use crate::outbox::Audience;
use crate::social::party::now_secs;
use flyff_common::ObjId;
use flyff_common::defs::text::{
    TID_GAME_L_EVENT_CREATE_E001, TID_GAME_L_EVENT_CREATE_E002, TID_GAME_L_EVENT_CREATE_E003,
    TID_GAME_L_EVENT_CREATE_E004,
};
use flyff_data::social::lord::LordTables;
use flyff_db::audit::content as audit;
use flyff_db::{AtomicBatch, DbOp, LordEventRecord};
use flyff_proto::pb;

/// `(1 + sum fE, 1 + sum fI)` of `events` (f32, in list order: `ILordEvent::GetEFactor()` / `GetIFactor()`).
pub fn factors_of(events: &[LordEvent]) -> (f32, f32) {
    let mut exp = 1.0_f32;
    let mut item = 1.0_f32;
    for e in events {
        exp += e.exp_factor;
        item += e.item_factor;
    }
    (exp, item)
}

/// `ILordEvent::GetCost( e, i )` for the signed indexes of the wire: a negative index counts as out of range (the
/// original read before the vector: undefined behaviour) and costs 0, which is refused with `E004`.
pub fn event_cost(tables: &LordTables, e_index: i32, i_index: i32) -> i64 {
    match (usize::try_from(e_index), usize::try_from(i_index)) {
        (Ok(e), Ok(i)) => tables.event_cost(e, i),
        _ => 0,
    }
}

/// Writes the factors of the running events into the event modifiers (`lord_exp`, `lord_item`): after the load, a
/// create, a tick and an initialize.
pub fn publish_factors(s: &mut Services) {
    let (exp, item) = s.event.lord.factors();
    s.event.lua_events.set_lord_factors(exp, item);
}

fn defined(ctx: &mut GameCtx<'_>, who: PlayerRef, tid: u32) {
    notice::defined(ctx, who, tid, Vec::new());
}

/// `LEventCreateReq{e_index, i_index}` (22 §9.4; the NPC was checked by the handler). With `features.player_lordevent` any
/// player may start an event: the list has fewer than `gameplay.max_lord_event` components and the player has none
/// (`E002`); without it only the lord may (`E001`) and only one at a time (`E002`). Then a cost of 0 (unknown indexes,
/// the free pair) is `E004` and a cost above the penya plus Perins is `E003`.
pub fn create_event(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    e_index: i32,
    i_index: i32,
) {
    let Some(m) = ctx.player(who) else {
        return;
    };
    let Some(me) = m.player().map(|p| p.ident.character_id) else {
        return;
    };
    let own = s.event.lord.events.iter().any(|e| e.creator == me);
    if ctx.cfg.rules.features.player_lordevent {
        let max = usize::try_from(ctx.cfg.rules.gameplay.max_lord_event).unwrap_or(usize::MAX);
        if s.event.lord.events.len() >= max || own {
            return defined(ctx, who, TID_GAME_L_EVENT_CREATE_E002);
        }
    } else {
        if !s.event.lord.is_lord(me) {
            return defined(ctx, who, TID_GAME_L_EVENT_CREATE_E001);
        }
        if own {
            return defined(ctx, who, TID_GAME_L_EVENT_CREATE_E002);
        }
    }
    let cost = event_cost(&ctx.data.lord, e_index, i_index);
    if cost == 0 {
        return defined(ctx, who, TID_GAME_L_EVENT_CREATE_E004);
    }
    if cost > total_gold(m) {
        return defined(ctx, who, TID_GAME_L_EVENT_CREATE_E003);
    }
    if !can_snapshot(ctx, who) {
        tracing::debug!("a lord event of a player without a savable session was refused");
        return;
    }
    // a non-zero cost means both indexes are inside their tables
    let (Some(exp_row), Some(item_row)) = (
        usize::try_from(e_index)
            .ok()
            .and_then(|i| ctx.data.lord.event_exp.get(i).copied()),
        usize::try_from(i_index)
            .ok()
            .and_then(|i| ctx.data.lord.event_item.get(i).copied()),
    ) else {
        return;
    };

    // ---- the commit block: nothing below can fail ----
    let now = now_secs(ctx);
    let core = s.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    let mut fx = Fx::new();
    let perins = match ctx.worlds.player_mut(who.0) {
        Some(m) => remove_total_gold(m, &env, cost, &mut fx),
        None => return,
    };
    fx.flush(s, ctx, who);
    let Some((snap, generation)) = snapshot_for_batch(s, ctx, who) else {
        tracing::error!(
            cost,
            "a lord event lost its snapshot after the check: the cost is kept by the memory only"
        );
        return;
    };
    let (exp_factor, item_factor) = (exp_row.0, item_row.0);
    s.event.lord.events.push(LordEvent {
        creator: me,
        ticks: LEVENT_TICKS,
        exp_factor,
        item_factor,
        created_at: now,
    });
    publish_factors(s);
    let record = LordEventRecord {
        character_id: me,
        ticks_left: LEVENT_TICKS,
        exp_factor,
        item_factor,
        created_at: now,
    };
    super::send_player(
        s,
        ctx,
        who,
        Some(generation),
        "LordEventCreate",
        AtomicBatch::lord_event_create(snap, record),
    );
    ctx.broadcast(
        Some(&*s),
        ObjId::NULL,
        Audience::AllOnline,
        pb::LEventCreateNtf {
            component: Some(pb::LordEventComponent {
                tick: LEVENT_TICKS,
                id_player: me.0,
                exp_factor,
                item_factor,
            }),
        },
    );
    let ev = audit::lord_event(me, cost, exp_factor, item_factor);
    let ev = match crate::audit::actor_of(ctx, who) {
        Some(actor) => ev.by(actor),
        None => ev,
    };
    ctx.audit.emit(ev);
    tracing::info!(
        creator = me.0,
        cost,
        perins,
        "a lord event started: {exp_factor} EXP, {item_factor} drops"
    );
}

/// The lord-event ticks of P4 (`CLEvent::OnTimer`, `tlord.cpp:773-788`): every event loses a minute (never below 0);
/// the row of an event that is still running is rewritten, one that reached 0 is deleted; ONE `LEventTick` with every
/// entry (the zeros included) goes to everybody; then the zeros are removed and the factors recomputed.
pub fn minute(s: &mut Services, ctx: &mut GameCtx<'_>) {
    if s.event.lord.events.is_empty() {
        return;
    }
    let mut ticks = Vec::with_capacity(s.event.lord.events.len());
    let mut ops = Vec::with_capacity(s.event.lord.events.len());
    for e in &mut s.event.lord.events {
        e.ticks = (e.ticks - 1).max(0);
        ticks.push(pb::LordEventTickEntry {
            id_player: e.creator.0,
            tick: e.ticks,
        });
        ops.push(if e.ticks > 0 {
            DbOp::UpsertLordEvent(LordEventRecord {
                character_id: e.creator,
                ticks_left: e.ticks,
                exp_factor: e.exp_factor,
                item_factor: e.item_factor,
                created_at: e.created_at,
            })
        } else {
            DbOp::DeleteLordEvent {
                character_id: e.creator,
            }
        });
    }
    super::send_ops(s, ctx, "lord_event_tick", ops);
    ctx.broadcast(
        Some(&*s),
        ObjId::NULL,
        Audience::AllOnline,
        pb::LEventTick { ticks },
    );
    s.event.lord.events.retain(|e| e.ticks > 0);
    publish_factors(s);
}

/// `ILordEvent::Initialize` (`/LEventInitialize`, the admin console): every event is cleared (`ClearLordEvents`), the
/// factors are 1.0 again and `LEventInitialize` goes to everybody.
pub fn initialize(s: &mut Services, ctx: &mut GameCtx<'_>) {
    s.event.lord.events.clear();
    publish_factors(s);
    super::send_ops(s, ctx, "lord_event_clear", vec![DbOp::ClearLordEvents]);
    ctx.broadcast(
        Some(&*s),
        ObjId::NULL,
        Audience::AllOnline,
        pb::LEventInitialize {},
    );
}
