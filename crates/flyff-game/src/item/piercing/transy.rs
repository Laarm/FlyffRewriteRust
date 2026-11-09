//! Item transy (Plan/19 §12.8, WP-ITEM-14; ref/13 §18.9, ref/07b §8.5; src:WORLDSERVER/ItemUpgrade.cpp:1433-1542):
//! an armour becomes its opposite-gender twin and keeps everything else.
//!
//! `ItemTransy{target, scroll, new_item_id, cash}`. Only the cash path exists (the penya path is compiled out
//! of the original: `cash == false` does nothing). The target must be a usable, unworn entry; the scroll a
//! usable `II_CHR_SYS_SCR_ITEMTRANSY_A` (target level limit up to 60) or `_B` (61 and up).
//! `flyff_rules::item::transy::can_transy` decides whether `new_item_id` is the twin. A request that fails any
//! test answers nothing and consumes nothing, exactly like the original.
//!
//! **Message order** (`RunItemTransy` then `OnItemTransy`): the success sound and effect, the text
//! `TID_GAME_ITEM_TRANSY_SUCCESS "old" "new"`, the removal of the old item (`UpdateItem` count 0), the new item
//! (`CreateItem`; upgrade level, element, options, sockets, flags, keep time and the **serial** are kept, the
//! endurance is the new prop's), then the removal of the scroll. The creation cannot fail: the removal freed the
//! slot. Persistence: urgent save; audit `item.upgrade{kind: "transy"}`, `item.destroy` of the old item and
//! `item.create{source: "transy"}` of the new one.

use super::{PierceCx, equipped, usable};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::fx::Fx;
use crate::item::model::Item;
use crate::item::ops::{ItemOrigin, RemoveReason, core as core_ops};
use crate::item::ultimate::{feedback, with_actor};
use crate::mover::Mover;
use flyff_common::defs::text::{TID_GAME_EQUIPPUT, TID_GAME_ITEM_TRANSY_SUCCESS};
use flyff_proto::pb;
use flyff_rules::item::transy::{can_transy, scroll_fits};
use serde_json::json;

/// What an `ItemTransy` request did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransyOutcome {
    /// Nothing happened (a foreign or reserved entry, the penya path, a wrong scroll, no twin).
    Ignored,
    /// `TID_GAME_EQUIPPUT`: the target is worn.
    Equipped,
    /// The armour was swapped; the scroll is gone.
    Done,
}

/// `OnItemTransy` on one mover.
pub fn item_transy(
    m: &mut Mover,
    cx: &mut PierceCx<'_>,
    target: u32,
    scroll: u32,
    change_id: u32,
    cash: bool,
    fx: &mut Fx,
) -> TransyOutcome {
    let Some((tid, item)) = usable(m, target) else {
        return TransyOutcome::Ignored;
    };
    if equipped(m, tid) {
        fx.text(TID_GAME_EQUIPPUT);
        return TransyOutcome::Equipped;
    }
    // the penya path (`__ITEMTRANSY_PENYA`) is compiled out: nothing happens
    if !cash {
        return TransyOutcome::Ignored;
    }
    let Some((sid, sc)) = usable(m, scroll) else {
        return TransyOutcome::Ignored;
    };
    let env = cx.env;
    let data = env.data();
    let (Some(from), Some(to)) = (data.item(item.item_id), data.item(change_id)) else {
        return TransyOutcome::Ignored;
    };
    if !scroll_fits(sc.item_id, from.limit_level1) || !can_transy(from, to) {
        return TransyOutcome::Ignored;
    }
    let before = item.clone();
    let scroll_item = sc.item_id;
    let mut copy: Item = item.clone();
    copy.item_id = change_id;
    copy.endurance = to.endurance as i32;
    copy.count = 1;
    let (old_name, new_name) = (from.name.clone(), to.name.clone());
    feedback(m, fx, true);
    fx.defined(
        TID_GAME_ITEM_TRANSY_SUCCESS,
        vec![format!("\"{old_name}\""), format!("\"{new_name}\"")],
    );
    core_ops::remove_item(m, env, tid, 1, RemoveReason::Other("transy"), fx);
    let made = core_ops::create_item(m, env, cx.serials, copy.clone(), ItemOrigin::Transy, fx);
    core_ops::remove_item(m, env, sid, 1, RemoveReason::Other("transy"), fx);
    fx.mark_urgent();
    fx.audit(
        flyff_db::audit::ev::ITEM_UPGRADE,
        json!({
            "kind": "transy",
            "result": "success",
            "before": crate::item::fx::item_json(&before),
            "after": crate::item::fx::item_json(&copy),
            "materials": [{ "item": scroll_item, "count": 1 }],
            "created": made.is_ok(),
        }),
    );
    TransyOutcome::Done
}

/// `ItemTransy`.
pub fn on_item_transy(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::ItemTransy,
) -> TransyOutcome {
    with_actor(svc, ctx, who, |m, env, serials, rng, now, fx| {
        let mut cx = PierceCx {
            env,
            serials,
            rng,
            now,
        };
        item_transy(
            m,
            &mut cx,
            msg.item_obj_id,
            msg.scroll_obj_id,
            msg.change_item_id,
            msg.cash,
            fx,
        )
    })
    .unwrap_or(TransyOutcome::Ignored)
}
