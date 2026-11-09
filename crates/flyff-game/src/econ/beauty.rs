//! The beauty shop (Plan/19 §7.8, WP-ECON-02; ref/07b §6; src:WORLDSERVER/DPSrvr.cpp:4558, 4952-5008,
//! _Common/Mover.cpp:651-690): `SetHairReq` and `ChangeFaceReq`.
//!
//! * **VR-026.** A hair mesh `>= 10` (`MAX_HAIR`) and a face `>= 11` (`MAX_HEAD`) are ignored; with a coupon
//!   the coupon item is looked up and checked *before* anything changes (the original set the hair mesh
//!   first and then bailed out of a missing coupon); without a coupon the gold is checked first.
//! * **VR-001.** The request changes the session's own character, whatever ids the payload carries (the
//!   legacy client sent its player id; the message has none any more).
//! * **PL-154.** `ChangeFaceNtf` goes to the viewers of the changer including itself (the original broadcast
//!   it to every user of the server); `SetHairNtf` likewise.
//! * The hair cost is `flyff_rules::item::hair_cost` against the *current* appearance: the stored colour goes
//!   through the original's float path before it is compared with the requested one (golden for 0..=255), so
//!   an unchanged colour costs nothing. The stored colour is what `CMover::SetHairColor` makes of the
//!   requested one ([`flyff_rules::appearance::set_hair_color`]: at least 0.3 per channel, white means the
//!   default of the mesh).
//! * Paying: the gold, or one coupon (`II_SYS_SYS_SCR_HAIRCHANGE` / `II_SYS_SYS_SCR_FACEOFFFREE`, audited as
//!   `item.destroy{reason: consume}`). Every payment also writes one `shop.buy` audit line with
//!   `service: "hair" | "face"`.
//!
//! The NPC dialog that opens the beauty-shop window is doc 20's; the original checked no NPC distance for
//! either request and none is added here.

use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::container::StorageId;
use crate::item::fx::Fx;
use crate::item::gold::core::add_gold;
use crate::item::ops::{RemoveReason, core as ops};
use crate::mover::Mover;
use flyff_common::defs::item::{II_SYS_SYS_SCR_FACEOFFFREE, II_SYS_SYS_SCR_HAIRCHANGE};
use flyff_common::defs::neuz::{CHANGE_FACE_COST, MAX_HAIR, MAX_HEAD};
use flyff_common::defs::text::{TID_GAME_LACKMONEY, TID_GAME_WARNNING_COUPON};
use flyff_db::audit::ev;
use flyff_proto::pb;
use flyff_rules::appearance::set_hair_color;
use flyff_rules::item::hair_cost;
use serde_json::json;

/// What the payment of one beauty request is.
enum Payment {
    /// `cost` penya (0 = free: nothing is deducted).
    Gold(i32),
    /// One coupon in storage entry `id`.
    Coupon(StorageId),
}

/// Looks up the payment: with a coupon the first item of that id must be usable (`TID_GAME_WARNNING_COUPON`
/// otherwise), without one the wallet must hold `cost` (`TID_GAME_LACKMONEY`).
fn payment(m: &Mover, use_coupon: bool, coupon_item: u32, cost: i32) -> Result<Payment, u32> {
    let Some(p) = m.player() else {
        return Err(0);
    };
    if use_coupon {
        match p.items.inventory.find(coupon_item) {
            Some((id, it)) if it.is_usable() => Ok(Payment::Coupon(id)),
            _ => Err(TID_GAME_WARNNING_COUPON),
        }
    } else if p.items.gold < cost {
        Err(TID_GAME_LACKMONEY)
    } else {
        Ok(Payment::Gold(cost))
    }
}

/// Pays: deducts the gold or consumes one coupon.
fn pay(m: &mut Mover, env: &crate::item::env::ItemEnv<'_>, pay: &Payment, fx: &mut Fx) {
    match *pay {
        Payment::Gold(cost) => add_gold(m, env, -i64::from(cost), true, fx),
        Payment::Coupon(id) => ops::remove_item(m, env, id, 1, RemoveReason::Consume, fx),
    }
}

/// `SetHairReq{hair, r, g, b, use_coupon}` (see the module documentation).
pub fn on_set_hair(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, msg: pb::SetHairReq) {
    let (Ok(hair), Ok(r), Ok(g), Ok(b)) = (
        u8::try_from(msg.hair),
        u8::try_from(msg.r),
        u8::try_from(msg.g),
        u8::try_from(msg.b),
    ) else {
        return;
    };
    if u32::from(hair) >= MAX_HAIR {
        return;
    }
    let core = svc.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    let mut fx = Fx::new();
    let Some(m) = ctx.worlds.player_mut(who.0) else {
        return;
    };
    if crate::combat::state::is_die(m) {
        return;
    }
    let Some(p) = m.player() else { return };
    let cur = p.appearance;
    let cur_rgb = [
        (cur.hair_color >> 16) as u8,
        (cur.hair_color >> 8) as u8,
        cur.hair_color as u8,
    ];
    let cost = hair_cost(u32::from(cur.hair_mesh), cur_rgb, [r, g, b], hair);
    let paid = match payment(m, msg.use_coupon, II_SYS_SYS_SCR_HAIRCHANGE, cost) {
        Ok(p) => p,
        Err(0) => return,
        Err(tid) => {
            fx.text(tid);
            fx.flush(svc, ctx, who);
            return;
        }
    };
    pay(m, &env, &paid, &mut fx);
    if let Some(p) = m.player_mut() {
        p.appearance.hair_mesh = hair;
        p.appearance.hair_color = set_hair_color(p.appearance.sex, u32::from(hair), [r, g, b]);
    }
    fx.out.near(pb::SetHairNtf {
        hair: u32::from(hair),
        r: u32::from(r),
        g: u32::from(g),
        b: u32::from(b),
    });
    fx.audit(
        ev::SHOP_BUY,
        json!({
            "service": "hair",
            "gold": if matches!(paid, Payment::Gold(_)) { cost } else { 0 },
            "coupon": matches!(paid, Payment::Coupon(_)),
            "hair": hair,
            "color": [r, g, b],
        }),
    );
    fx.flush(svc, ctx, who);
}

/// `ChangeFaceReq{face, cost (ignored), use_coupon}` (see the module documentation).
pub fn on_change_face(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::ChangeFaceReq,
) {
    let Ok(face) = u8::try_from(msg.face) else {
        return;
    };
    if u32::from(face) >= MAX_HEAD {
        return;
    }
    let core = svc.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    let mut fx = Fx::new();
    let Some(m) = ctx.worlds.player_mut(who.0) else {
        return;
    };
    if crate::combat::state::is_die(m) {
        return;
    }
    let cost = CHANGE_FACE_COST as i32;
    let paid = match payment(m, msg.use_coupon, II_SYS_SYS_SCR_FACEOFFFREE, cost) {
        Ok(p) => p,
        Err(0) => return,
        Err(tid) => {
            fx.text(tid);
            fx.flush(svc, ctx, who);
            return;
        }
    };
    pay(m, &env, &paid, &mut fx);
    let Some(p) = m.player_mut() else { return };
    p.appearance.head_mesh = face;
    let character = p.ident.character_id.0;
    fx.out.near(pb::ChangeFaceNtf {
        id_player: character,
        face: u32::from(face),
    });
    fx.audit(
        ev::SHOP_BUY,
        json!({
            "service": "face",
            "gold": if matches!(paid, Payment::Gold(_)) { cost } else { 0 },
            "coupon": matches!(paid, Payment::Coupon(_)),
            "face": face,
        }),
    );
    fx.flush(svc, ctx, who);
}
