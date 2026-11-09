//! Exchange scripts (Plan/19 §16, WP-ECON-07; ref/13 §22, ref/07c §9; src:_Common/Exchange.cpp:424-519,
//! WORLDSERVER/DPSrvr.cpp:9856; VR-012, VR-024, PL-074, KQ-27): `ExchangeReq{mmi_id, set_index}` runs one row of
//! `Exchange_Script.txt` (`GameData.exchange`, WP-DATA-11): the player hands in items, penya and campus points and
//! receives the weighted pay items.
//!
//! * **Request** (**FIX** VR-012 / VR-024: the original checked nothing): the player is alive, is not trading (any
//!   trade partner, a shop window included), is not vending, and an NPC offering the menu `mmi_id` stands within
//!   `gameplay.npc_menu_range` in the same world and layer. Anything else is ignored without an answer.
//! * **Answer** `ExchangeNtf{state: 1, result}`: 0 success, 1 failed (no such menu or row), 2 not enough room, 3
//!   condition not met. The client shows the `RESULTMSG` texts of the set itself.
//! * **Condition** ([`check_condition`]): every `CONDITION` item must be owned with its count (the count is
//!   `PlayerItems::quest_item_count`: **0 while any inventory item is reserved**, KQ-27), `PENYA`
//!   (`II_GOLD_SEED1`) against the wallet, campus points through the [`CampusHooks`] seam of doc 21. Every failing
//!   entry sends `TID_EXCHANGE_FAIL [item name, n]` (`TID_GAME_EXCHANGE_CAMPUSPOINT_FAIL` for points) when the set has
//!   fewer than two `RESULTMSG` entries.
//! * **Pay pick** ([`pick_pay`]): `PAY n` picks distinct entries without replacement, weighted by their
//!   per-million probabilities (the loop of `GetPayItemList` draw for draw).
//! * **Room** ([`has_room`], KEEP the lenient legacy estimate): empty bag slots + slots freed by fully removed
//!   stacks - the sum of `floor(count / pack_max)` of the pay items; `<= 0` answers 2 and nothing changes.
//! * **Commit**: the `REMOVE` items and penya go, campus points are deducted through the hook, the pay items are
//!   created (`flags` from the column, `charged` from the prop, [`ItemOrigin::Exchange`]); a pay item that does not
//!   fit despite the estimate is lost (KEEP, logged). Audit `item.create{source: "exchange"}` and
//!   `item.destroy{reason: "exchange"}`; urgent snapshot (19 §20.2).

use crate::combat::state::is_die;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::fx::Fx;
use crate::item::gold::core::add_gold;
use crate::item::model::{Item, ItemFlags};
use crate::item::ops::{CtxSerials, ItemOrigin, RemoveReason, core as ops};
use crate::player::PlayerExt;
use flyff_common::defs::define::POINTTYPE_CAMPUS;
use flyff_common::defs::item::II_GOLD_SEED1;
use flyff_common::defs::text::{
    TID_EXCHANGE_FAIL, TID_EXCHANGE_SUCCESS, TID_GAME_EXCHANGE_CAMPUSPOINT_FAIL,
};
use flyff_common::rng::GameRng;
use flyff_data::GameData;
use flyff_data::items_ext::exchange::{ExchangePay, ExchangeSet};
use flyff_db::audit::ev;
use flyff_proto::pb;
use serde_json::json;

/// `EXCHANGE_RESULT`: the only state of `ExchangeNtf` the client handles.
pub const EXCHANGE_RESULT: u32 = 1;

/// The result codes of `ExchangeNtf` (`EXCHANGE_SUCCESS` ...).
pub mod result {
    /// Done.
    pub const SUCCESS: i32 = 0;
    /// No such menu or row.
    pub const FAILED: i32 = 1;
    /// Not enough room in the bag.
    pub const INVENTORY_FAILED: i32 = 2;
    /// A condition item, penya or point is missing.
    pub const CONDITION_FAILED: i32 = 3;
}

/// The campus-point seam of the exchange scripts (doc 21 implements it with the campus service; Plan/19 §16:
/// "campus points via doc 21"). The default [`NoCampus`] has no points.
pub trait CampusHooks: Send + std::fmt::Debug {
    /// `GetPointByType(POINTTYPE_CAMPUS)`: the campus points of `who`.
    fn campus_points(&self, p: &PlayerExt) -> i64;

    /// `SendUpdateCampusPoint(idPlayer, points, FALSE, 'E')`: takes `points` away (the persistence op is the
    /// implementation's).
    fn remove_campus_points(&mut self, ctx: &mut GameCtx<'_>, who: PlayerRef, points: i64);

    /// The entry the exchange calls to pay `points`, with the social services of the game next to the hook
    /// (`Services.social` and `Services.econ` are disjoint borrows). The default is
    /// [`CampusHooks::remove_campus_points`]; the campus service overrides it (21 §12, WP-SOCIAL-22).
    fn spend_campus_points(
        &mut self,
        _social: &mut crate::social::Social,
        ctx: &mut GameCtx<'_>,
        who: PlayerRef,
        points: i64,
    ) {
        self.remove_campus_points(ctx, who, points);
    }
}

/// No campus: zero points, nothing to deduct.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NoCampus;

impl CampusHooks for NoCampus {
    fn campus_points(&self, _p: &PlayerExt) -> i64 {
        0
    }

    fn remove_campus_points(&mut self, _ctx: &mut GameCtx<'_>, _who: PlayerRef, _points: i64) {}
}

/// The exchange service (`Services.econ.exchange`).
#[derive(Debug)]
pub struct ExchangeService {
    /// The campus-point seam.
    pub campus: Box<dyn CampusHooks>,
}

impl Default for ExchangeService {
    fn default() -> Self {
        ExchangeService {
            campus: Box::new(NoCampus),
        }
    }
}

/// The two arguments of `TID_EXCHANGE_FAIL` / `TID_EXCHANGE_SUCCESS` ("... %s x%s."): the item name and the count
/// (the legacy call site quoted the name only to group the token).
fn named(data: &GameData, item: u32, count: i64) -> Vec<String> {
    let name = data.item(item).map_or("", |p| p.name.as_str());
    vec![name.to_owned(), count.to_string()]
}

/// `CExchange::CheckCondition`: does the player meet every condition of `set`? A failing entry queues its text
/// when the set has fewer than two `RESULTMSG` entries (the client shows its own result texts otherwise).
/// Every entry is evaluated, so every missing item is named.
pub fn check_condition(
    p: &PlayerExt,
    set: &ExchangeSet,
    data: &GameData,
    campus: &dyn CampusHooks,
    fx: &mut Fx,
) -> bool {
    let verbose = set.result_msgs.len() < 2;
    let mut ok = true;
    for &[item, need] in &set.conditions {
        let have = if item == II_GOLD_SEED1 {
            i64::from(p.items.gold)
        } else {
            i64::from(p.items.quest_item_count(item))
        };
        if have < i64::from(need) {
            if verbose && data.item(item).is_some() {
                fx.defined(TID_EXCHANGE_FAIL, named(data, item, i64::from(need)));
            }
            ok = false;
        }
    }
    for &[kind, need] in &set.condition_points {
        let have = if kind as u32 == POINTTYPE_CAMPUS {
            campus.campus_points(p)
        } else {
            0
        };
        if have < i64::from(need) {
            if verbose && kind as u32 == POINTTYPE_CAMPUS {
                fx.defined(TID_GAME_EXCHANGE_CAMPUSPOINT_FAIL, vec![need.to_string()]);
            }
            ok = false;
        }
    }
    ok
}

/// `CExchange::GetPayItemList`: picks entries of `pays` without replacement, weighted by their `prob` (per
/// million, normalised by the loader to sum to 1 000 000). The first roll is `random_below(1 000 000)`; an entry is
/// taken when the roll falls below the running sum of the probabilities; then the picked weight leaves the pool,
/// the pick stops at `pay_num` entries or when no weight is left, and the next roll is `random_below(remaining)`
/// over the pool restarted from its first entry.
pub fn pick_pay(pays: &[ExchangePay], pay_num: i32, rng: &mut dyn GameRng) -> Vec<ExchangePay> {
    let mut pool: Vec<&ExchangePay> = pays.iter().collect();
    let mut out: Vec<ExchangePay> = Vec::new();
    let mut remaining: i64 = 1_000_000;
    let mut roll = i64::from(rng.random_below(1_000_000));
    let mut sum: i64 = 0;
    let mut i = 0;
    while let Some(&e) = pool.get(i) {
        sum += i64::from(e.prob);
        if roll < sum {
            out.push(e.clone());
            if i64::try_from(out.len()).is_ok_and(|n| n == i64::from(pay_num)) {
                break;
            }
            remaining -= i64::from(e.prob);
            if remaining <= 0 {
                break;
            }
            roll = i64::from(rng.random_below(u32::try_from(remaining).unwrap_or(u32::MAX)));
            sum = 0;
            pool.remove(i);
            i = 0;
        } else {
            i += 1;
        }
    }
    out
}

/// `CExchange::IsFull` inverted (KEEP the lenient legacy estimate, Plan/19 §16): `true` when the bag has room.
/// `empty slots + slots freed by fully removed stacks - sum(floor(count / pack_max)) > 0`; an unstackable pay
/// item of count 1 counts as 0 slots.
pub fn has_room(
    p: &PlayerExt,
    data: &GameData,
    removes: &[[u32; 2]],
    pays: &[ExchangePay],
) -> bool {
    let mut empty = i64::try_from(p.items.inventory.empty_count()).unwrap_or(i64::MAX);
    for &[item, count] in removes {
        let mut remain = i64::from(count);
        for (_, it) in p.items.inventory.iter() {
            if it.is_usable() && it.item_id == item {
                if i64::from(it.count) <= remain {
                    empty += 1;
                }
                remain -= i64::from(it.count);
                if remain <= 0 {
                    break;
                }
            }
        }
    }
    for pay in pays {
        if let Some(prop) = data.item(pay.item) {
            empty -= i64::from(pay.count.max(0)) / i64::from(prop.pack_max.max(1));
        }
    }
    empty > 0
}

/// `ExchangeReq{mmi_id, set_index}` (see the module documentation).
pub fn on_exchange(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::ExchangeReq,
) {
    let core = svc.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    let data = env.data();
    let Ok(mmi) = u32::try_from(msg.mmi_id) else {
        return;
    };
    let world = {
        let Some(m) = ctx.player(who) else { return };
        let Some(p) = m.player() else { return };
        if is_die(m) || p.econ.has_partner() || p.econ.vending {
            return;
        }
        if ctx
            .worlds
            .close_npc(mmi, m.core.world, m.core.layer, m.core.pos)
            .is_none()
        {
            return;
        }
        m.core.world
    };
    let mut fx = Fx::new();
    let reply = |fx: &mut Fx, r: i32| {
        fx.out.to_self(pb::ExchangeNtf {
            state: EXCHANGE_RESULT,
            result: r,
        });
    };
    let set = data
        .exchange
        .menus
        .iter()
        .find(|menu| menu.menu == mmi)
        .and_then(|menu| {
            usize::try_from(msg.set_index)
                .ok()
                .and_then(|i| menu.sets.get(i))
        });
    let Some(set) = set else {
        reply(&mut fx, result::FAILED);
        fx.flush(svc, ctx, who);
        return;
    };
    // the condition, the pick and the room: nothing changes until all three passed
    let pays = {
        let Some(p) = ctx.worlds.player(who.0).and_then(|m| m.player()) else {
            return;
        };
        if !check_condition(p, set, data, &*svc.econ.exchange.campus, &mut fx) {
            reply(&mut fx, result::CONDITION_FAILED);
            fx.flush(svc, ctx, who);
            return;
        }
        let pays = pick_pay(&set.pays, set.pay_num, ctx.rng.world(world));
        if !has_room(p, data, &set.removes, &pays) {
            reply(&mut fx, result::INVENTORY_FAILED);
            fx.flush(svc, ctx, who);
            return;
        }
        pays
    };
    // ---- the commit ----
    {
        let Services { serials, .. } = &mut *svc;
        let mut src = CtxSerials::new(serials, ctx.persist, ctx.tickets);
        let Some(m) = ctx.worlds.player_mut(who.0) else {
            return;
        };
        for &[item, count] in &set.removes {
            if item == II_GOLD_SEED1 {
                add_gold(m, &env, -i64::from(count), true, &mut fx);
                fx.audit(
                    ev::ITEM_DESTROY,
                    json!({"item": null, "count": 0, "gold": count, "reason": RemoveReason::Exchange.tag()}),
                );
            } else {
                ops::remove_item_by_id(m, &env, item, Some(count), RemoveReason::Exchange, &mut fx);
            }
        }
        let verbose = set.result_msgs.len() < 2;
        for pay in &pays {
            let mut it = Item::new(
                pay.item,
                u16::try_from(pay.count.max(1)).unwrap_or(u16::MAX),
            );
            it.flags = ItemFlags(pay.flag);
            it.charged = data.item(pay.item).is_some_and(|prop| prop.charged != 0);
            match ops::create_item(m, &env, &mut src, it, ItemOrigin::Exchange, &mut fx) {
                Ok(_) => {
                    if verbose {
                        fx.defined(
                            TID_EXCHANGE_SUCCESS,
                            named(data, pay.item, i64::from(pay.count)),
                        );
                    }
                }
                Err(e) => {
                    // KEEP: the estimate is the legacy rule, a pay item that does not fit is lost
                    tracing::error!(item = pay.item, count = pay.count, error = %e, "exchange: a pay item could not be created");
                }
            }
        }
    }
    for &[kind, points] in &set.remove_points {
        if kind as u32 == POINTTYPE_CAMPUS {
            svc.econ.exchange.campus.spend_campus_points(
                &mut svc.social,
                ctx,
                who,
                i64::from(points),
            );
        }
    }
    fx.mark_urgent();
    reply(&mut fx, result::SUCCESS);
    fx.flush(svc, ctx, who);
}
