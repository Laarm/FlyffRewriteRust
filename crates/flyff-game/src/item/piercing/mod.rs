//! Piercing (Plan/19 §12.5, WP-ITEM-14; ref/13 §18.6, ref/07b §8.2-8.4; src:WORLDSERVER/ItemUpgrade.cpp:119-380):
//! `PiercingSize` (one more socket), `PiercingReq` (a card into the first free socket), `PiercingRemove` (the
//! last card out), the card statistics ([`card_dsts`]) and, in [`transy`], the item transy.
//!
//! **`features.no_piercing`** (the repack default, both values are tested): gear is not pierceable, so
//! `PiercingSize` always answers `TID_PIERCING_POSSIBLE_ITEM` for a suit, a shield or a weapon, `PiercingReq`
//! answers it for the target, and `PiercingRemove` ignores such an item; with the switch off a suit takes up to 4
//! sockets, a shield or weapon up to 10 (`flyff_rules::item::piercing::is_pierce_able`). A vis pet stays
//! pierceable for cards, but its slots are the business of WP-ITEM-18 (`DoUseItem` of a vis item, the vis key):
//! `PiercingSize` refuses a vis pet (PL-070: the key expands it), `PiercingReq` ignores a vis card.
//!
//! **One commit block per request**: every refusal comes first, then the penya, the materials and the result,
//! all through one [`Fx`]. The order of the checks and of the messages is the original's (the penya is taken
//! *before* the size roll; a failed size attempt without `II_SYS_SYS_SCR_PIEPROT` destroys the item; the
//! moonstone and the protection scroll are always consumed).
//!
//! **Fixes of the original.** A protection scroll named by the request that is not a usable `PIEPROT` entry is
//! no protection; `PiercingSize` and `PiercingRemove` are refused while trading or vending (the original
//! refused the first and checked nothing in the second); `PiercingSize` is paced like the other upgrades
//! (PL-076, silent).

pub mod transy;

use super::container::{SerialSource, StorageId};
use super::env::ItemEnv;
use super::fx::{Fx, item_json};
use super::model::{Item, MAX_PIERCING_WEAPON};
use super::ops::{RemoveReason, UiParam, core as core_ops, make_long};
use super::ultimate::{feedback, pace_ok, with_actor};
use super::usage::UseFacts;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::mover::Mover;
use flyff_common::GameMs;
use flyff_common::defs::item::{
    II_GEN_MAT_MOONSTONE, II_GEN_MAT_MOONSTONE_1, II_SYS_SYS_SCR_PIEPROT,
};
use flyff_common::defs::itemkind::IK3_VIS;
use flyff_common::defs::text::{
    TID_GAME_EQUIPPUT, TID_GAME_LACKMONEY, TID_GAME_REMOVE_PIERCING_ERROR,
    TID_GAME_REMOVE_PIERCING_SUCCESS, TID_MMI_PIERCINGFAIL, TID_MMI_PIERCINGSUCCESS,
    TID_PIERCING_ERROR_NOPIERCING, TID_PIERCING_POSSIBLE_ITEM, TID_SBEVE_NOTUSEITEM,
    TID_UPGRADE_ERROR_WRONGUPLEVEL,
};
use flyff_common::rng::GameRng;
use flyff_data::GameData;
use flyff_data::props::item::ItemProp;
use flyff_db::audit::ev;
use flyff_proto::pb;
use flyff_rules::item::piercing::{
    PIERCING_REMOVE_COST, PIERCING_SIZE_COST, PIERCING_SIZE_DENOM, cards_apply, first_empty,
    is_pierce_able, last_filled, size_prob, size_roll_fails,
};
use serde_json::{Value, json};

/// What a piercing request did (for the callers and the tests).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Dropped without a message (an unknown, reserved or foreign entry, a trade or a stall, the pacing).
    Ignored,
    /// Refused with a text (or silently by a rule); nothing was consumed.
    Refused,
    /// The attempt ran and succeeded.
    Success,
    /// The size attempt failed; the item is kept (protected).
    Failed,
    /// The size attempt failed; the item was destroyed.
    Destroyed,
}

/// What the piercing functions need besides the mover and the [`Fx`].
pub struct PierceCx<'a> {
    /// The item environment (game data, switches, wall time).
    pub env: &'a ItemEnv<'a>,
    /// The serial source of created items (the transy).
    pub serials: &'a mut dyn SerialSource,
    /// The world's random stream.
    pub rng: &'a mut dyn GameRng,
    /// The game clock of the tick (the pacing).
    pub now: GameMs,
}

impl<'a> PierceCx<'a> {
    fn data(&self) -> &'a GameData {
        self.env.data()
    }
}

// ---- helpers --------------------------------------------------------------------------------------------------

/// The entry named by a client object id: it exists and nothing reserves it (`IsUsableItem`).
pub(crate) fn usable(m: &Mover, obj_id: u32) -> Option<(StorageId, &Item)> {
    let id = StorageId::try_from(obj_id).ok()?;
    let it = m.player()?.items.inventory.get_at_id(id)?;
    it.is_usable().then_some((id, it))
}

pub(crate) fn equipped(m: &Mover, id: StorageId) -> bool {
    m.player().is_some_and(|p| p.items.inventory.is_equip(id))
}

fn busy(m: &Mover) -> bool {
    let f = UseFacts::of(m);
    f.trading || f.vending
}

/// `IsPierceAble` of an item under the switches.
fn pierce_able(
    env: &ItemEnv<'_>,
    prop: &ItemProp,
    it: &Item,
    card_kind3: Option<u32>,
    size: bool,
) -> bool {
    is_pierce_able(
        prop,
        it.piercing.cards.len(),
        it.is_vis_pet(prop),
        card_kind3,
        size,
        env.cfg().no_piercing,
    )
}

/// `AddGold( -cost )` after the check `gold >= cost` (`TID_GAME_LACKMONEY` and `false` otherwise).
fn pay(m: &mut Mover, env: &ItemEnv<'_>, cost: i32, fx: &mut Fx) -> bool {
    if crate::item::gold::core::gold(m) < i64::from(cost) {
        fx.text(TID_GAME_LACKMONEY);
        return false;
    }
    crate::item::gold::core::add_gold(m, env, -i64::from(cost), true, fx);
    true
}

fn material(item_id: u32, count: u32) -> Value {
    json!({ "item": item_id, "count": count })
}

fn audit(
    fx: &mut Fx,
    kind: &str,
    result: &str,
    before: &Item,
    after: Option<&Item>,
    materials: Vec<Value>,
) {
    fx.audit(
        ev::ITEM_UPGRADE,
        json!({
            "kind": kind,
            "result": result,
            "before": item_json(before),
            "after": after.map(item_json),
            "materials": materials,
        }),
    );
}

/// The statistics the cards of `item` add (`CItemElem::GetPiercingAvail` when `IsPierceAble()`, Plan/19 §12.5):
/// `(DST, value)` summed per DST in order of first appearance. Empty for gear under `features.no_piercing`.
/// Doc 16's equipment engine adds these to the worn item (`stats::equip::equip_dsts_with`).
pub fn card_dsts(
    item: &Item,
    prop: &ItemProp,
    data: &GameData,
    no_piercing: bool,
) -> Vec<(u32, i32)> {
    if !cards_apply(
        prop,
        item.piercing.cards.len(),
        item.is_vis_pet(prop),
        no_piercing,
    ) {
        return Vec::new();
    }
    flyff_rules::item::piercing::card_dsts(&data.item_etc.piercing, &item.piercing.cards)
}

// ---- PiercingSize ---------------------------------------------------------------------------------------------

/// `PiercingSize{item, moonstone, protection}` (`CItemUpgrade::OnPiercingSize`): one more socket for 100 000
/// penya and a moonstone. `protection` is `NULL_ID` for none.
pub fn piercing_size(
    m: &mut Mover,
    cx: &mut PierceCx<'_>,
    item: u32,
    moonstone: u32,
    protection: u32,
    fx: &mut Fx,
) -> Outcome {
    let env = cx.env;
    // PL-076: the pacing of the upgrade attempts comes first, like in the other upgrade handlers
    if !pace_ok(m, env, cx.now) {
        return Outcome::Ignored;
    }
    let (Some((tid, target)), Some((mid, stone))) = (usable(m, item), usable(m, moonstone)) else {
        return Outcome::Ignored;
    };
    if busy(m) {
        return Outcome::Ignored;
    }
    if equipped(m, tid) {
        fx.text(TID_GAME_EQUIPPUT);
        return Outcome::Refused;
    }
    let data = cx.data();
    let Some(prop) = data.item(target.item_id) else {
        return Outcome::Ignored;
    };
    // a vis pet grows through the vis key (WP-ITEM-18, PL-070), never through a moonstone
    if target.is_vis_pet(prop) || !pierce_able(env, prop, target, None, true) {
        fx.text(TID_PIERCING_POSSIBLE_ITEM);
        return Outcome::Refused;
    }
    if stone.item_id != II_GEN_MAT_MOONSTONE && stone.item_id != II_GEN_MAT_MOONSTONE_1 {
        fx.text(TID_SBEVE_NOTUSEITEM);
        return Outcome::Refused;
    }
    // the protection scroll: a usable entry must be the PIEPROT; anything else is no protection at all
    let prot = usable(m, protection);
    if let Some((_, p)) = prot
        && p.item_id != II_SYS_SYS_SCR_PIEPROT
    {
        fx.text(TID_SBEVE_NOTUSEITEM);
        return Outcome::Refused;
    }
    let prot_id = prot.map(|(id, _)| id);
    let size = target.piercing.cards.len();
    let before = target.clone();
    let stone_item = stone.item_id;
    let suit_prob = data
        .lua
        .item_upgrade
        .as_ref()
        .map_or(&[][..], |c| c.suit_prob.as_slice());
    let weapon_prob = data
        .lua
        .item_upgrade
        .as_ref()
        .map_or(&[][..], |c| c.weapon_prob.as_slice());
    let prob = size_prob(prop, size, suit_prob, weapon_prob);
    if crate::item::gold::core::gold(m) < i64::from(PIERCING_SIZE_COST) {
        fx.text(TID_GAME_LACKMONEY);
        return Outcome::Refused;
    }
    // the point of no return: the penya first, then the roll (the original takes it before it rolls)
    let _ = pay(m, env, PIERCING_SIZE_COST, fx);
    fx.mark_urgent();
    let mut materials = vec![material(stone_item, 1)];
    let fails = size_roll_fails(prob, cx.rng.random_below(PIERCING_SIZE_DENOM));
    let outcome = if fails {
        feedback(m, fx, false);
        fx.text(TID_MMI_PIERCINGFAIL);
        if prot_id.is_none() {
            core_ops::remove_item(m, env, tid, 1, RemoveReason::UpgradeFail, fx);
            audit(
                fx,
                "piercing_size",
                "destroyed",
                &before,
                None,
                materials.clone(),
            );
            Outcome::Destroyed
        } else {
            audit(
                fx,
                "piercing_size",
                "protected",
                &before,
                Some(&before),
                materials.clone(),
            );
            Outcome::Failed
        }
    } else {
        feedback(m, fx, true);
        core_ops::update_item(m, env, tid, UiParam::PiercingSize, (size + 1) as u32, 0, fx);
        fx.text(TID_MMI_PIERCINGSUCCESS);
        let after = m
            .player()
            .and_then(|p| p.items.inventory.get_at_id(tid))
            .cloned();
        audit(
            fx,
            "piercing_size",
            "success",
            &before,
            after.as_ref(),
            materials.clone(),
        );
        Outcome::Success
    };
    // the moonstone is always consumed, then the protection scroll when one was given
    core_ops::remove_item(m, env, mid, 1, RemoveReason::Other("piercing_size"), fx);
    if let Some(pid) = prot_id {
        if let Some(p) = m.player().and_then(|p| p.items.inventory.get_at_id(pid)) {
            materials.push(material(p.item_id, 1));
        }
        core_ops::remove_item(m, env, pid, 1, RemoveReason::Other("piercing_size"), fx);
    }
    outcome
}

// ---- PiercingReq ----------------------------------------------------------------------------------------------

/// `PiercingReq{item, card}` (`CItemUpgrade::OnPiercing`): the card goes into the first free socket.
pub fn insert_card(
    m: &mut Mover,
    cx: &mut PierceCx<'_>,
    item: u32,
    card: u32,
    fx: &mut Fx,
) -> Outcome {
    let (Some((tid, target)), Some((cid, card_it))) = (usable(m, item), usable(m, card)) else {
        return Outcome::Ignored;
    };
    if equipped(m, tid) {
        fx.text(TID_GAME_EQUIPPUT);
        return Outcome::Refused;
    }
    let env = cx.env;
    let data = cx.data();
    let (Some(prop), Some(card_prop)) = (data.item(target.item_id), data.item(card_it.item_id))
    else {
        return Outcome::Ignored;
    };
    if !pierce_able(env, prop, target, None, false) {
        fx.text(TID_PIERCING_POSSIBLE_ITEM);
        return Outcome::Refused;
    }
    // a vis item goes in through its own use (keep time, prerequisites: WP-ITEM-18)
    if card_prop.item_kind3 == IK3_VIS {
        return Outcome::Ignored;
    }
    if !pierce_able(env, prop, target, Some(card_prop.item_kind3), false) {
        fx.text(TID_UPGRADE_ERROR_WRONGUPLEVEL);
        return Outcome::Refused;
    }
    let Some(slot) = first_empty(&target.piercing.cards) else {
        fx.text(TID_PIERCING_ERROR_NOPIERCING);
        return Outcome::Refused;
    };
    if busy(m) {
        return Outcome::Ignored;
    }
    let before = target.clone();
    let materials = vec![material(card_it.item_id, 1)];
    let card_item = card_it.item_id;
    core_ops::update_item(
        m,
        env,
        tid,
        UiParam::Piercing,
        make_long(slot as u32, card_item),
        0,
        fx,
    );
    feedback(m, fx, true);
    core_ops::remove_item(m, env, cid, 1, RemoveReason::Other("piercing"), fx);
    fx.mark_urgent();
    let after = m
        .player()
        .and_then(|p| p.items.inventory.get_at_id(tid))
        .cloned();
    audit(
        fx,
        "piercing",
        "success",
        &before,
        after.as_ref(),
        materials,
    );
    Outcome::Success
}

// ---- PiercingRemove -------------------------------------------------------------------------------------------

/// `PiercingRemove{item}` (`CItemUpgrade::OnPiercingRemove`): 1 000 000 penya take the last card out.
pub fn remove_card(m: &mut Mover, cx: &mut PierceCx<'_>, item: u32, fx: &mut Fx) -> Outcome {
    let Some((tid, target)) = usable(m, item) else {
        return Outcome::Ignored;
    };
    let env = cx.env;
    let Some(prop) = cx.data().item(target.item_id) else {
        return Outcome::Ignored;
    };
    if !pierce_able(env, prop, target, None, false) || equipped(m, tid) || busy(m) {
        return Outcome::Ignored;
    }
    if target.piercing.cards.first().is_none_or(|&c| c == 0) {
        fx.text(TID_GAME_REMOVE_PIERCING_ERROR);
        return Outcome::Refused;
    }
    let Some(last) = last_filled(&target.piercing.cards) else {
        return Outcome::Refused;
    };
    let before = target.clone();
    if !pay(m, env, PIERCING_REMOVE_COST, fx) {
        return Outcome::Refused;
    }
    fx.text(TID_GAME_REMOVE_PIERCING_SUCCESS);
    core_ops::update_item(
        m,
        env,
        tid,
        UiParam::Piercing,
        make_long(last as u32, 0),
        0,
        fx,
    );
    fx.mark_urgent();
    let after = m
        .player()
        .and_then(|p| p.items.inventory.get_at_id(tid))
        .cloned();
    audit(
        fx,
        "piercing_remove",
        "success",
        &before,
        after.as_ref(),
        Vec::new(),
    );
    Outcome::Success
}

// ---- the handlers ---------------------------------------------------------------------------------------------

fn run(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    op: impl FnOnce(&mut Mover, &mut PierceCx<'_>, &mut Fx) -> Outcome,
) -> Outcome {
    with_actor(svc, ctx, who, |m, env, serials, rng, now, fx| {
        let mut cx = PierceCx {
            env,
            serials,
            rng,
            now,
        };
        op(m, &mut cx, fx)
    })
    .unwrap_or(Outcome::Ignored)
}

/// `PiercingSize`.
pub fn on_piercing_size(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::PiercingSize,
) -> Outcome {
    run(svc, ctx, who, |m, cx, fx| {
        piercing_size(
            m,
            cx,
            msg.item_obj_id,
            msg.moonstone_obj_id,
            msg.protection_obj_id,
            fx,
        )
    })
}

/// `PiercingReq`.
pub fn on_piercing(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::PiercingReq,
) -> Outcome {
    run(svc, ctx, who, |m, cx, fx| {
        insert_card(m, cx, msg.item_obj_id, msg.card_obj_id, fx)
    })
}

/// `PiercingRemove`.
pub fn on_piercing_remove(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::PiercingRemove,
) -> Outcome {
    run(svc, ctx, who, |m, cx, fx| {
        remove_card(m, cx, msg.item_obj_id, fx)
    })
}

/// The largest socket count of any item (a `MAX_PIERCING_WEAPON` weapon).
pub const MAX_SOCKETS: usize = MAX_PIERCING_WEAPON;
