//! Commercial element stones (Plan/19 §12.3, F-229; src:WORLDSERVER/DPSrvr.cpp:6302-6432): `CommercialElemReq`
//! applies an attack stone to a weapon or a defence stone to a suit (`element_stone_id` = the stone), or removes
//! the element with `II_CHR_SYS_SCR_TINEINEDSTONE`. A stone blocks the legacy upgrades of the item
//! (`TID_GAME_NOTUPGRADE`, see `handlers::on_enchant`). The SM-mode effect of a worn stone is `stats::sm`
//! (WP-ITEM-21): it starts when the item is equipped.

use super::txn::{
    Outcome, UpgradeCx, audit, distinct, equipped, item_at, material, take_one, usable,
};
use crate::item::fx::Fx;
use crate::item::model::Element;
use crate::mover::Mover;
use crate::world::fx::create_sfx_msg;
use flyff_common::defs::obj::XI_SYS_EXPAN01;
use flyff_common::defs::text::TID_GAME_EQUIPPUT;
use flyff_proto::pb;
use flyff_rules::item::smelt::{
    StoneKind, StoneTarget, commercial_stone, is_system_kind, stone_target,
};

/// The `m_nResistSMItemId` value that tells the client the element was removed.
pub const ELEMENT_REMOVED: i32 = 9999;

/// `OnCommercialElem`. Silent unless the original answered: a worn target or stone (`TID_GAME_EQUIPPUT`).
pub fn on_commercial_elem(
    m: &mut Mover,
    cx: &UpgradeCx<'_>,
    msg: &pb::CommercialElemReq,
    fx: &mut Fx,
) -> Outcome {
    let (Some(target), Some(stone)) = (usable(m, msg.item_obj_id), usable(m, msg.stone_obj_id))
    else {
        return Outcome::Ignored;
    };
    if !distinct(&[target, stone]) {
        return Outcome::Ignored;
    }
    let (Some(before), Some(stone_item)) =
        (item_at(m, target).cloned(), item_at(m, stone).cloned())
    else {
        return Outcome::Ignored;
    };
    let (Some(tp), Some(sp)) = (cx.prop(before.item_id), cx.prop(stone_item.item_id)) else {
        return Outcome::Ignored;
    };
    if tp.log >= 2 || sp.log >= 2 {
        return Outcome::Ignored;
    }
    if equipped(m, target) || equipped(m, stone) {
        fx.text(TID_GAME_EQUIPPUT);
        return Outcome::Refused;
    }
    let Some(class) = stone_target(tp) else {
        return Outcome::Ignored;
    };
    if before.element_stone_id != 0 || !is_system_kind(sp) {
        return Outcome::Ignored;
    }
    let removal = match commercial_stone(stone_item.item_id) {
        Some(StoneKind::Attack) if class == StoneTarget::Weapon => false,
        Some(StoneKind::Defence) if class == StoneTarget::Suit => false,
        Some(StoneKind::Remove) if before.element != Element::None => true,
        _ => return Outcome::Ignored,
    };
    fx.mark_urgent();
    let id32 = u32::from(target);
    if removal {
        if let Some(it) = m
            .player_mut()
            .and_then(|p| p.items.inventory.get_at_id_mut(target))
        {
            it.element = Element::None;
            it.element_level = 0;
        }
        fx.out.to_self(pb::CommercialElemNtf {
            item_obj_id: id32,
            resist_sm_item_id: ELEMENT_REMOVED,
        });
    } else if let Some(it) = m
        .player_mut()
        .and_then(|p| p.items.inventory.get_at_id_mut(target))
    {
        it.element_stone_id = stone_item.item_id;
    }
    take_one(m, cx, stone, fx);
    fx.out
        .near(create_sfx_msg(XI_SYS_EXPAN01, m.core.pos, false));
    let applied = item_at(m, target).map_or(0, |it| it.element_stone_id);
    fx.out.to_self(pb::CommercialElemNtf {
        item_obj_id: id32,
        resist_sm_item_id: i32::try_from(applied).unwrap_or(i32::MAX),
    });
    let after = item_at(m, target).cloned();
    audit(
        fx,
        "element_stone",
        if removal { "remove" } else { "apply" },
        "success",
        &before,
        after.as_ref(),
        vec![material(stone_item.item_id, 1)],
    );
    Outcome::Success
}
