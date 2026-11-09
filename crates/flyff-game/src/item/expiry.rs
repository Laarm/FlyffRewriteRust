//! Keep-time expiry (Plan/19 §5.15, WP-ITEM-08; `CUser::CheckFiniteItem`, `User.cpp:7360-7395`).
//!
//! Once per second per player (the phase is `ObjId % 15` of the 15 Hz tick; the legacy timer was a wall-clock
//! second per user) every inventory item whose keep time has passed and that is not yet flagged is dealt with:
//!
//! * an **eat pet** (`IK3_PET`) is deactivated if it is summoned (ITEM-18) and deleted (`UI_NUM 0`, audit
//!   `item.destroy{expire}`);
//! * **everything else** is flagged `EXPIRED` and the owner is told (`UpdateItem(FLAG)`); a worn item loses its
//!   stats first (`reset_dest_param_equip`, except the left-hand weapon, which never granted any: the
//!   `__BLADELWEAPON0608` rule) but **stays equipped** and can be taken off (KQ-15). The active
//!   pet feed pocket loses its buff.
//!
//! The effects of the flag are elsewhere: no stats (`equip_dsts` answers nothing for an expired item), a
//! non-equipable item refuses its use with `TID_GAME_ITEM_EXPIRED` (`usage`), trade, vendor, ride and awakening
//! refuse expired items in their handlers.

use super::env::ItemEnv;
use super::fx::{Fx, item_json};
use super::model::{Item, ItemFlags};
use super::ops::{RemoveReason, UiParam, core as ops};
use crate::buff::core::{BuffCx, remove_fake};
use crate::buff::set::{BuffKey, BuffKind};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::mover::Mover;
use crate::stats::equip::reset_dest_param_equip;
use flyff_common::defs::item::II_SYS_SYS_SCR_PET_FEED_POCKET;
use flyff_common::defs::neuz::PARTS_LWEAPON;
use flyff_db::audit::ev;
use serde_json::json;

/// The period of the pass in ticks (one second).
pub const PERIOD_TICKS: u64 = 15;

/// `CheckFiniteItem` for one player: returns the number of items dealt with.
pub fn check_finite_items(m: &mut Mover, env: &ItemEnv<'_>, cx: &BuffCx<'_>, fx: &mut Fx) -> usize {
    let data = env.data();
    let now = env.now.0;
    let Some(p) = m.player() else { return 0 };
    let due: Vec<(u16, bool)> = p
        .items
        .inventory
        .iter()
        .filter(|(_, it)| it.is_expiring(now) && !it.is_expired())
        .map(|(id, it)| {
            let eat = data.item(it.item_id).is_some_and(Item::is_eat_pet);
            (id, eat)
        })
        .collect();
    for &(id, eat) in &due {
        if eat {
            // an eat pet is deactivated when it is the summoned one (ITEM-18), then it is gone
            if let Some(it) = m.player().and_then(|p| p.items.inventory.get_at_id(id)) {
                fx.audit(
                    ev::ITEM_DESTROY,
                    json!({ "item": item_json(it), "count": it.count, "reason": RemoveReason::Expire.tag() }),
                );
            }
            if m.player()
                .and_then(|p| p.items.summoned_eat_pet)
                .is_some_and(|(item, _)| item == id)
            {
                super::pets::eat::inactivate_and_remove(m, cx, fx);
            }
            ops::update_item(m, env, id, UiParam::Num, 0, 0, fx);
            continue;
        }
        let Some((flags, worn_part, item_id, has_keep)) = m.player().and_then(|p| {
            let inv = &p.items.inventory;
            let it = inv.get_at_id(id)?;
            let part = inv
                .is_equip(id)
                .then(|| {
                    inv.slot_of(id)
                        .map(|s| u32::from(s) - u32::from(inv.index_num()))
                })
                .flatten();
            Some((it.flags.0, part, it.item_id, it.keep_until > 0))
        }) else {
            continue;
        };
        // the flag first, so that the set bonuses recount without the item; the message order stays the
        // original's: the stat resets, then the flag update
        let mut flag_fx = Fx::new();
        ops::set_item_flags(m, env, id, flags | ItemFlags::EXPIRED, &mut flag_fx);
        if let Some(part) = worn_part
            && part != PARTS_LWEAPON
        {
            reset_dest_param_equip(m, env, part, &mut fx.out);
        }
        for o in flag_fx.out.take() {
            fx.out.push(o.aud, o.body);
        }
        // the pet feed pocket: an active timed pocket loses its buff
        if item_id == II_SYS_SYS_SCR_PET_FEED_POCKET && has_keep {
            remove_fake(m, cx, BuffKey::new(BuffKind::Item, item_id), fx);
        }
    }
    due.len()
}

/// The pass of one tick: the players whose phase it is (`(tick + id) % 15 == 0`).
pub fn process(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    let tick = ctx.clock.tick.0;
    let due: Vec<_> = ctx
        .sessions
        .iter()
        .map(|e| e.player)
        .filter(|id| (tick + u64::from(id.0)).is_multiple_of(PERIOD_TICKS))
        .collect();
    for id in due {
        let who = PlayerRef(id);
        let core = svc.items.handle();
        let env = core.env(ctx.clock.unix_ms.to_unix_time());
        let cx = BuffCx::new(env, ctx.clock.game_ms);
        let mut fx = Fx::new();
        let n = match ctx.worlds.player_mut(id) {
            Some(m) => check_finite_items(m, &env, &cx, &mut fx),
            None => continue,
        };
        if n > 0 {
            fx.mark_urgent();
        }
        fx.flush(svc, ctx, who);
    }
}
