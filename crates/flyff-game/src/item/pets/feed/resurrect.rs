//! Pet resurrection (Plan/19 §12.6.5, §13.4; ref/13 §19; src:WORLDSERVER/DPSrvr.cpp:11601-11645, 5517-5560):
//! `QuePetResurrection` turns a dead-able B..S pet into a quest fragment; the fragment (`..._02` scroll) used on
//! an expired pet of the same grade brings it back (the target branch of `DoUseItemTarget`, shared with
//! ITEM-11).

use crate::buff::core::BuffCx;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::container::StorageId;
use crate::item::fx::Fx;
use crate::item::model::{Item, PetLevel};
use crate::item::ops::{self, ItemOrigin, RemoveReason, core as core_ops};
use crate::item::pets::system::process::send_state;
use crate::mover::Mover;
use flyff_common::defs::item::{
    II_SYS_SYS_QUE_PETRESURRECTION01_A, II_SYS_SYS_QUE_PETRESURRECTION01_B,
    II_SYS_SYS_QUE_PETRESURRECTION01_S, II_SYS_SYS_QUE_PETRESURRECTION02_A,
    II_SYS_SYS_QUE_PETRESURRECTION02_B, II_SYS_SYS_QUE_PETRESURRECTION02_S,
};
use flyff_common::defs::text::{
    TID_GAME_PETRESURRECTION_SUCCESS, TID_GAME_PETRESURRECTION_WRONG_TARGET_01,
    TID_GAME_PETRESURRECTION_WRONG_TARGET_02,
};
use flyff_proto::pb;

/// `OnQuePetResurrection( item )`: the item must be usable (else silent); it must carry a pet that is not the
/// summoned one, not expired and of grade B, A or S - otherwise `QuePetResurrectionResult{ok: false}`. The pet
/// item is removed, one fragment `II_SYS_SYS_QUE_PETRESURRECTION01_B/_A/_S` is created and
/// `QuePetResurrectionResult{ok: true}` answers.
pub fn on_que_pet_resurrection(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::QuePetResurrection,
) {
    let Ok(id) = StorageId::try_from(msg.obj_id) else {
        return;
    };
    let Some(p) = ctx.player(who).and_then(|m| m.player()) else {
        return;
    };
    let Some(item) = p.items.inventory.get_at_id(id).filter(|it| it.is_usable()) else {
        return;
    };
    let summoned = p.items.pet_obj_id == Some(id);
    let fragment = match item.pet.as_deref() {
        Some(pet) if !summoned && !item.is_expired() => match pet.level {
            PetLevel::B => Some(II_SYS_SYS_QUE_PETRESURRECTION01_B),
            PetLevel::A => Some(II_SYS_SYS_QUE_PETRESURRECTION01_A),
            PetLevel::S => Some(II_SYS_SYS_QUE_PETRESURRECTION01_S),
            _ => None,
        },
        _ => None,
    };
    let mut ok = false;
    if let Some(fragment) = fragment {
        ops::remove_item(
            svc,
            ctx,
            who,
            id,
            1,
            RemoveReason::Other("pet_resurrection"),
        );
        ok = ops::create_item(
            svc,
            ctx,
            who,
            Item::new(fragment, 1),
            ItemOrigin::Other("pet_resurrection"),
        )
        .is_ok();
        svc.saves.mark_urgent(who.0);
    }
    let mut fx = Fx::new();
    fx.out.to_self(pb::QuePetResurrectionResult { ok });
    fx.flush(svc, ctx, who);
}

/// The target branch of `DoUseItemTarget` for the scrolls `II_SYS_SYS_QUE_PETRESURRECTION02_S/_A/_B`
/// (ITEM-11's handler calls it after its own checks - both items usable, the target not worn): the target must
/// be an expired pet item (`TID_GAME_PETRESURRECTION_WRONG_TARGET_01`) of the grade of the scroll, B, A or S
/// (`..._WRONG_TARGET_02`). Success clears `EXPIRED` (`UpdateItem( UI_FLAG )`), sets life 0 and energy to half the
/// maximum, sends `PetState` and `TID_GAME_PETRESURRECTION_SUCCESS`. `true` when the pet was revived (the caller
/// then consumes the scroll with the success sound and effect).
pub fn use_target_scroll(
    m: &mut Mover,
    cx: &BuffCx<'_>,
    scroll_item: u32,
    target: StorageId,
    fx: &mut Fx,
) -> bool {
    let tables = &cx.data().pet;
    let Some(it) = m.player().and_then(|p| p.items.inventory.get_at_id(target)) else {
        return false;
    };
    let Some(pet) = it.pet.as_deref().filter(|_| it.is_expired()) else {
        fx.text(TID_GAME_PETRESURRECTION_WRONG_TARGET_01);
        return false;
    };
    let fits = match pet.level {
        PetLevel::B => scroll_item == II_SYS_SYS_QUE_PETRESURRECTION02_B,
        PetLevel::A => scroll_item == II_SYS_SYS_QUE_PETRESURRECTION02_A,
        PetLevel::S => scroll_item == II_SYS_SYS_QUE_PETRESURRECTION02_S,
        _ => false,
    };
    if !fits {
        fx.text(TID_GAME_PETRESURRECTION_WRONG_TARGET_02);
        return false;
    }
    let flags = it.flags.0;
    let half = u32::from(pet.max_energy(tables)) / 2;
    core_ops::set_item_flags(
        m,
        &cx.env,
        target,
        flags & !flyff_rules::item::ITEM_FLAG_EXPIRED,
        fx,
    );
    if let Some(p) = crate::item::pets::system::pet_mut(m, target) {
        p.life = 0;
        p.set_energy(tables, half);
    }
    send_state(m, cx, target, fx);
    fx.text(TID_GAME_PETRESURRECTION_SUCCESS);
    true
}
