//! `CreateMonster{data, pos}` (Plan/19 §5.10, WP-ITEM-22; ref/07b §8.16; src:WORLDSERVER/DPSrvr.cpp:8523-8545):
//! the inventory half of a monster-summon scroll.
//!
//! Layering: this module decodes the message, checks the item and consumes it; **every summon rule** (limit,
//! listing, roll, distance, area, spawn, registry, texts, audit) is `ai::create_monster::try_create` (doc 18,
//! WP-AI-16), which this module calls only after the item check and whose refusals it does not repeat.
//!
//! 1. `obj_id = data >> 16` (the `HIWORD` of the legacy `MAKELONG(ITYPE_ITEM, objid)`); the item must be an
//!    entry of the sender's inventory that is not reserved (`IsUsableItem`), else the message is ignored and
//!    `try_create` is **not** called. A dead sender and a missing or malformed position are ignored too.
//! 2. `CUser::IsUsableState`: a trade partner answers `TID_GAME_TRADELIMITUSING`, an open stall is silent.
//! 3. `try_create(user, item id, pos)`: `Ok` consumes exactly one item (`remove_item(obj_id, 1)`) and marks the
//!    player for an urgent save, `Err` consumes nothing.

use crate::ai::create_monster::{CreateMonsterError, try_create};
use crate::combat::state::is_die;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::container::StorageId;
use crate::item::ops::{self, RemoveReason};
use crate::item::usage::UseFacts;
use flyff_common::ObjId;
use flyff_common::defs::text::TID_GAME_TRADELIMITUSING;
use flyff_common::math::Vec3;
use flyff_proto::pb;

/// What a `CreateMonster` message did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestOutcome {
    /// Dropped before `try_create` (a foreign or reserved item, a busy or dead sender, a bad position).
    Ignored,
    /// `try_create` refused; nothing was consumed.
    Refused(CreateMonsterError),
    /// The monster stands and one item was consumed.
    Created(ObjId),
}

/// `CreateMonster`: [`on_request_with`] over the real [`try_create`].
pub fn on_request(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::CreateMonster,
) -> RequestOutcome {
    on_request_with(svc, ctx, who, msg, try_create)
}

/// The handler with the summon rules injected (the tests record the calls and answer chosen results).
pub fn on_request_with(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::CreateMonster,
    create: impl FnOnce(
        &mut Services,
        &mut GameCtx<'_>,
        PlayerRef,
        u32,
        Vec3,
    ) -> Result<ObjId, CreateMonsterError>,
) -> RequestOutcome {
    let Some(pos) = msg.pos else {
        return RequestOutcome::Ignored;
    };
    let pos = Vec3::new(pos.x, pos.y, pos.z);
    let Ok(storage) = StorageId::try_from(msg.data >> 16) else {
        return RequestOutcome::Ignored;
    };
    // the item: an entry of the sender's own inventory that nothing reserves
    let (item_id, facts) = {
        let Some(m) = ctx.player(who) else {
            return RequestOutcome::Ignored;
        };
        let Some(p) = m.player() else {
            return RequestOutcome::Ignored;
        };
        let Some(item) = p.items.inventory.get_at_id(storage) else {
            return RequestOutcome::Ignored;
        };
        if !item.is_usable() || is_die(m) {
            return RequestOutcome::Ignored;
        }
        (item.item_id, UseFacts::of(m))
    };
    // `CUser::IsUsableState`
    if facts.trading {
        crate::chat::ingress::notice::defined(ctx, who, TID_GAME_TRADELIMITUSING, Vec::new());
        return RequestOutcome::Ignored;
    }
    if facts.vending {
        return RequestOutcome::Ignored;
    }
    match create(svc, ctx, who, item_id, pos) {
        Ok(id) => {
            ops::remove_item(
                svc,
                ctx,
                who,
                storage,
                1,
                RemoveReason::Other("create_monster"),
            );
            svc.saves.mark_urgent(who.0);
            RequestOutcome::Created(id)
        }
        Err(e) => RequestOutcome::Refused(e),
    }
}
