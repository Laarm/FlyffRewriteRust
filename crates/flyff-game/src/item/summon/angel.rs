//! `CreateAngel` (Plan/19 §5.12, WP-ITEM-22; ref/07c §6.1; src:WORLDSERVER/DPSrvr.cpp:9328-9494).
//!
//! The legacy payload was one string built by the client and parsed with an off-by-one buffer overflow; here it is
//! a typed list (VR-025). The request is refused as a whole, nothing consumed, when it has no entry or more than
//! 20, names a stack twice or one that does not exist, is reserved by a trade or a stall, is no Orichalcum or
//! moonstone (the original accepted such items and *consumed* them), or asks for 0 or more pieces than the stack
//! holds. A request without any entry is refused too: the original rolled a free red angel for it.
//!
//! The order is the original's: roll, then the empty-slot check (`TID_GAME_LACKSPACE` as a message box), then the
//! materials are taken and the angel is created. The colour shares are `flyff_rules::item::angel`.

use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::container::{SerialSource, StorageId};
use crate::item::env::ItemEnv;
use crate::item::fx::Fx;
use crate::item::model::Item;
use crate::item::ops::{ItemOrigin, RemoveReason, core as ops};
use crate::item::ultimate::with_actor;
use crate::mover::Mover;
use flyff_common::defs::item::{
    II_GEN_MAT_MOONSTONE, II_GEN_MAT_MOONSTONE_1, II_GEN_MAT_ORICHALCUM01,
    II_GEN_MAT_ORICHALCUM01_1,
};
use flyff_common::defs::text::TID_GAME_LACKSPACE;
use flyff_common::rng::GameRng;
use flyff_proto::pb;
use flyff_rules::chat::TEXT_DIAG;
use flyff_rules::item::angel::{MAX_ANGEL_MATERIAL_ENTRIES, angel_item};

/// What a `CreateAngel` request did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AngelOutcome {
    /// The materials are gone and this angel item was created.
    Created(u32),
    /// Refused without a message, nothing consumed (a bad list).
    Refused,
    /// No empty slot (`TID_GAME_LACKSPACE` sent), nothing consumed.
    LackSpace,
}

/// `true` for the four items an angel is made of.
fn is_material(item_id: u32) -> bool {
    matches!(
        item_id,
        II_GEN_MAT_ORICHALCUM01
            | II_GEN_MAT_ORICHALCUM01_1
            | II_GEN_MAT_MOONSTONE
            | II_GEN_MAT_MOONSTONE_1
    )
}

/// One checked material: the storage entry and how many pieces of it.
struct Piece {
    id: StorageId,
    count: u16,
}

/// Checks the list against the inventory; the pieces in request order and their total.
fn check_list(m: &Mover, materials: &[(u32, u32)]) -> Option<(Vec<Piece>, u32)> {
    if materials.is_empty() || materials.len() > MAX_ANGEL_MATERIAL_ENTRIES {
        return None;
    }
    let inv = &m.player()?.items.inventory;
    let mut pieces: Vec<Piece> = Vec::with_capacity(materials.len());
    let mut total = 0_u32;
    for &(obj_id, count) in materials {
        let id = StorageId::try_from(obj_id).ok()?;
        if pieces.iter().any(|p| p.id == id) {
            return None;
        }
        let item = inv.get_at_id(id)?;
        if !item.is_usable() || inv.is_equip(id) || !is_material(item.item_id) {
            return None;
        }
        let count = u16::try_from(count).ok()?;
        if count == 0 || count > item.count {
            return None;
        }
        total += u32::from(count);
        pieces.push(Piece { id, count });
    }
    Some((pieces, total))
}

/// `CreateAngel` on one mover (the core of [`on_create_angel`]).
pub fn create_angel(
    m: &mut Mover,
    env: &ItemEnv<'_>,
    serials: &mut dyn SerialSource,
    rng: &mut dyn GameRng,
    materials: &[(u32, u32)],
    fx: &mut Fx,
) -> AngelOutcome {
    let Some((pieces, total)) = check_list(m, materials) else {
        return AngelOutcome::Refused;
    };
    let Some(angel) = angel_item(total, rng.random_below(1000)) else {
        return AngelOutcome::Refused;
    };
    let item = Item::new(angel, 1);
    let Some(p) = m.player() else {
        return AngelOutcome::Refused;
    };
    // the original asks for an empty slot before it takes the materials
    if p.items.inventory.empty_count() < 1 {
        let text = env.data().texts.get(TID_GAME_LACKSPACE).to_owned();
        fx.out.to_self(pb::Text {
            kind: TEXT_DIAG,
            text,
        });
        return AngelOutcome::LackSpace;
    }
    if !ops::can_create(m, env, &item) {
        // no prop for the angel: nothing is taken
        return AngelOutcome::Refused;
    }
    for piece in &pieces {
        ops::remove_item(
            m,
            env,
            piece.id,
            piece.count,
            RemoveReason::Other("angel_material"),
            fx,
        );
    }
    match ops::create_item(m, env, serials, item, ItemOrigin::Angel, fx) {
        Ok(_) => {
            fx.mark_urgent();
            AngelOutcome::Created(angel)
        }
        Err(_) => AngelOutcome::Refused,
    }
}

/// `CreateAngel{materials}`.
pub fn on_create_angel(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::CreateAngel,
) -> Option<AngelOutcome> {
    let materials: Vec<(u32, u32)> = msg.materials.iter().map(|a| (a.obj_id, a.count)).collect();
    with_actor(svc, ctx, who, |m, env, serials, rng, _now, fx| {
        create_angel(m, env, serials, rng, &materials, fx)
    })
}
