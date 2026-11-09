//! Pockets (Plan/19 §3.5; ref/13 §6.5; ref/07c §16; `_Common/pocket.cpp`, `DPSrvr.cpp:11034-11117`).
//!
//! * **Availability.** With `features.perm_extrabags` (the repack default) all three pockets are available for
//!   ever: `ProcessExpiring` is a no-op and `AvailPocket` is accepted but has no effect (nothing consumed).
//!   Without it pocket 0 is always available, pockets 1 and 2 for `prop.skill_time` seconds after a pocket item
//!   (`IK3_POCKET`) was consumed (an already available pocket is not extended: the original acts only on a
//!   pocket that is not available); the once-per-second pass marks the elapsed ones expired. An expired pocket
//!   keeps its items but cannot be read or written until it is activated again.
//! * **`MoveItemPocket{from, obj_id, num, to}`** (`-1` = the bag): `from == to`, `num <= 0` (VR-025 FIX; the
//!   original had no check), `num` above the stack, a worn, reserved or in-use source (the in-use rule is the
//!   bag's only) and an unavailable pocket are ignored; a full destination answers `TID_GAME_LACKSPACE` and
//!   changes nothing. A whole stack keeps its serial, a part gets fresh serials for the entries it creates
//!   (PL-050: a part merged into an existing stack retires its serial). Messages: the bag side
//!   `CreateItem` / `UpdateItem`, the pocket side `PocketAddItem{pocket, item}` / `PocketRemoveItem{pocket,
//!   obj_id, num}`; audit `pocket.move`; urgent save.
//! * The pocket contents are not usable directly (`GetItemId` only looks at the bag), except tickets (ITEM-24).

use super::with_player;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::container::{AddError, ItemContainer, SerialSource, StorageId};
use crate::item::env::{ItemConfig, ItemEnv};
use crate::item::fx::{Fx, item_json};
use crate::item::inventory::{ACCOUNT_SLOTS, Pocket};
use crate::item::ops::{ItemError, ItemOrigin, RemoveReason, UiParam, core as ops};
use crate::mover::Mover;
use flyff_common::ItemSerial;
use flyff_common::defs::itemkind::IK3_POCKET;
use flyff_common::defs::text::TID_GAME_LACKSPACE;
use flyff_db::audit::ev;
use flyff_proto::pb;
use flyff_rules::consts::NULL_ID;
use flyff_rules::item::buffitem::pocket_available;
use serde_json::json;

/// `MAX_POCKET`: the pockets of a character.
pub const MAX_POCKET: usize = ACCOUNT_SLOTS;

/// `CPocketController::expired` of `PocketAttribute.attribute`.
pub const ATTR_EXPIRED: i32 = 0;
/// `CPocketController::avail` of `PocketAttribute.attribute`.
pub const ATTR_AVAIL: i32 = 1;

/// `CPocketController::IsAvailable(index)`: always with `perm_extrabags`, else pocket 0, else a pocket that
/// exists and has not expired.
pub fn is_available(cfg: &ItemConfig, index: usize, pk: &Pocket) -> bool {
    pocket_available(cfg.perm_extrabags, index, pk.available, pk.expired)
}

/// A container the move can read or write: the bag or one pocket.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    Bag,
    Pocket(usize),
}

impl Place {
    /// `-1` is the bag, `0..MAX_POCKET` a pocket; anything else is not a container.
    fn of(v: i32) -> Option<Place> {
        match v {
            -1 => Some(Place::Bag),
            0..=2 => usize::try_from(v).ok().map(Place::Pocket),
            _ => None,
        }
    }

    /// The pocket index sent on the wire (`-1` = the bag).
    fn wire(self) -> i32 {
        match self {
            Place::Bag => -1,
            Place::Pocket(i) => i as i32,
        }
    }
}

/// The fields of a `MoveItemPocket` message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MoveReq {
    /// The source container, `-1` = the bag.
    pub from: i32,
    /// The storage id in the source.
    pub obj_id: i32,
    /// How many pieces move.
    pub num: i32,
    /// The destination container, `-1` = the bag.
    pub to: i32,
}

/// How a [`move_item`] ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveOutcome {
    /// The pieces moved.
    Moved,
    /// Invalid or forbidden: nothing changed, nothing was said.
    Ignored,
    /// The destination is full: `TID_GAME_LACKSPACE`, nothing changed.
    LackSpace,
}

fn container(m: &Mover, place: Place) -> Option<&ItemContainer> {
    let items = &m.player()?.items;
    match place {
        Place::Bag => Some(&items.inventory),
        Place::Pocket(i) => items.pockets.get(i).map(|p| &p.items),
    }
}

fn available(m: &Mover, cfg: &ItemConfig, place: Place) -> bool {
    match place {
        Place::Bag => m.player().is_some(),
        Place::Pocket(i) => m
            .player()
            .and_then(|p| p.items.pockets.get(i))
            .is_some_and(|pk| is_available(cfg, i, pk)),
    }
}

/// `OnMoveItemOnPocket` over one mover: see the module documentation.
pub fn move_item(
    m: &mut Mover,
    env: &ItemEnv<'_>,
    serials: &mut dyn SerialSource,
    req: MoveReq,
    fx: &mut Fx,
) -> MoveOutcome {
    let MoveReq {
        from,
        obj_id,
        num,
        to,
    } = req;
    if from == to || num <= 0 {
        return MoveOutcome::Ignored;
    }
    let (Some(src), Some(dst)) = (Place::of(from), Place::of(to)) else {
        return MoveOutcome::Ignored;
    };
    let Ok(id) = StorageId::try_from(obj_id) else {
        return MoveOutcome::Ignored;
    };
    let cfg = env.cfg();
    if !available(m, cfg, src) || !available(m, cfg, dst) {
        return MoveOutcome::Ignored;
    }
    // the source: an existing, usable item that is not worn or in use, and enough of it
    let Some(from_c) = container(m, src) else {
        return MoveOutcome::Ignored;
    };
    if src == Place::Bag && from_c.is_equip(id) {
        return MoveOutcome::Ignored;
    }
    let Some(item) = from_c.get_at_id(id) else {
        return MoveOutcome::Ignored;
    };
    if !item.is_usable() {
        return MoveOutcome::Ignored;
    }
    if src == Place::Bag && m.player().is_some_and(|p| p.items.is_using(id)) {
        return MoveOutcome::Ignored;
    }
    if i64::from(num) > i64::from(item.count) {
        return MoveOutcome::Ignored;
    }
    let count = item.count;
    let whole = i64::from(num) == i64::from(count);
    let mut moved = item.clone();
    moved.reserved = 0;
    moved.vendor_price = 0;
    // a validated `num <= count <= u16::MAX`
    moved.count = num as u16;
    if !whole {
        // the part is a new item: `add` numbers the entries it creates (and none when it merges)
        moved.serial = ItemSerial(0);
    }
    // the destination
    match dst {
        Place::Bag => {
            match ops::create_item(m, env, serials, moved.clone(), ItemOrigin::PocketMove, fx) {
                Ok(plan) => moved.serial = plan.template_serial,
                Err(ItemError::LackSpace) => {
                    fx.text(TID_GAME_LACKSPACE);
                    return MoveOutcome::LackSpace;
                }
                Err(_) => return MoveOutcome::Ignored,
            }
        }
        Place::Pocket(i) => {
            let Some(pk) = m.player_mut().and_then(|p| p.items.pockets.get_mut(i)) else {
                return MoveOutcome::Ignored;
            };
            match pk.items.add(&moved, env.data(), serials) {
                Ok(plan) => moved.serial = plan.template_serial,
                Err(AddError::Full) => {
                    fx.text(TID_GAME_LACKSPACE);
                    return MoveOutcome::LackSpace;
                }
                Err(_) => return MoveOutcome::Ignored,
            }
            fx.out.to_self(pb::PocketAddItem {
                pocket: i as i32,
                item: Some(moved.to_pb(0)),
            });
        }
    }
    // the source
    match src {
        Place::Bag => ops::remove_item(m, env, id, moved.count, RemoveReason::Moved, fx),
        Place::Pocket(i) => {
            if let Some(pk) = m.player_mut().and_then(|p| p.items.pockets.get_mut(i)) {
                if whole {
                    pk.items.remove_at_id(id);
                } else if let Some(it) = pk.items.get_at_id_mut(id) {
                    it.count = count - moved.count;
                }
            }
            fx.out.to_self(pb::PocketRemoveItem {
                pocket: i as i32,
                obj_id: i32::from(id),
                num: i32::from(moved.count),
            });
        }
    }
    fx.audit(
        ev::POCKET_MOVE,
        json!({
            "from": src.wire(),
            "to": dst.wire(),
            "item": item_json(&moved),
            "count": moved.count,
        }),
    );
    fx.mark_urgent();
    MoveOutcome::Moved
}

/// `MoveItemPocket` (C2S).
pub fn on_move_item_pocket(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::MoveItemPocket,
) {
    let req = MoveReq {
        from: msg.pocket_from,
        obj_id: msg.obj_id,
        num: msg.num,
        to: msg.pocket_to,
    };
    with_player(svc, ctx, who, |m, env, serials, fx| {
        move_item(m, env, serials, req, fx)
    });
}

/// `CPocketController::Avail(index, t)`: the pocket becomes available for `t` seconds (an expired one counts
/// from now, a still running one is extended; `t == 0` never expires).
fn avail(pk: &mut Pocket, now: i64, t: i64) {
    if pk.available {
        if pk.expired {
            pk.expires_at = now.saturating_add(t);
        } else {
            pk.expires_at = pk.expires_at.saturating_add(t);
        }
    } else {
        pk.available = true;
        pk.expires_at = now.saturating_add(t);
    }
    pk.expired = false;
    if t == 0 {
        pk.expires_at = 0;
    }
}

/// `OnAvailPocket` over one mover. Returns `true` when a pocket item was consumed.
///
/// With `perm_extrabags` the request is accepted but has no effect. The pocket index must be `1..=2`
/// (VR-025 FIX of the always false `&&` test of the original, which let any index through), the item a usable
/// `IK3_POCKET` item of the bag with a positive duration, and the pocket must not be available yet.
pub fn avail_pocket(
    m: &mut Mover,
    env: &ItemEnv<'_>,
    pocket: i32,
    item_obj_id: i32,
    fx: &mut Fx,
) -> bool {
    let cfg = env.cfg();
    let Some(index) = usize::try_from(pocket)
        .ok()
        .filter(|i| (1..MAX_POCKET).contains(i))
    else {
        return false;
    };
    if cfg.perm_extrabags {
        return false;
    }
    let Ok(id) = StorageId::try_from(item_obj_id) else {
        return false;
    };
    let Some(p) = m.player() else { return false };
    let Some(pk) = p.items.pockets.get(index) else {
        return false;
    };
    if is_available(cfg, index, pk) {
        return false;
    }
    let Some(item) = p.items.inventory.get_at_id(id) else {
        return false;
    };
    if !item.is_usable() || p.items.inventory.is_equip(id) {
        return false;
    }
    let Some(prop) = env.data().item(item.item_id) else {
        return false;
    };
    if prop.item_kind3 != IK3_POCKET || prop.skill_time == NULL_ID || prop.skill_time == 0 {
        return false;
    }
    let t = i64::from(prop.skill_time);
    let count = item.count;
    let now = env.now.0;
    if let Some(pk) = m.player_mut().and_then(|p| p.items.pockets.get_mut(index)) {
        avail(pk, now, t);
    }
    fx.out.to_self(pb::PocketAttribute {
        attribute: ATTR_AVAIL,
        pocket,
        data: i32::try_from(t).unwrap_or(i32::MAX),
    });
    ops::update_item(
        m,
        env,
        id,
        UiParam::Num,
        u32::from(count).saturating_sub(1),
        0,
        fx,
    );
    fx.mark_urgent();
    true
}

/// `AvailPocket` (C2S).
pub fn on_avail_pocket(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::AvailPocket,
) {
    with_player(svc, ctx, who, |m, env, _, fx| {
        avail_pocket(m, env, msg.pocket, msg.item_obj_id, fx)
    });
}

/// `CPocketController::ProcessExpiring` for one mover: every available pocket 1 and 2 whose time is up is
/// marked expired and `PocketAttribute{expired, pocket, 1}` goes to the owner. A no-op with
/// `perm_extrabags`; a pocket without an expiry time (`expires_at == 0`, `Avail(p, 0)`) never expires (the
/// original expired it at once, against the evident intent of `t == 0`). Returns the number expired.
pub fn process_expiring(m: &mut Mover, env: &ItemEnv<'_>, fx: &mut Fx) -> usize {
    let cfg = env.cfg();
    if cfg.perm_extrabags {
        return 0;
    }
    let now = env.now.0;
    let Some(p) = m.player_mut() else { return 0 };
    let mut n = 0;
    for (i, pk) in p.items.pockets.iter_mut().enumerate().skip(1) {
        if is_available(cfg, i, pk) && pk.expires_at != 0 && now > pk.expires_at {
            pk.expired = true;
            fx.out.to_self(pb::PocketAttribute {
                attribute: ATTR_EXPIRED,
                pocket: i as i32,
                data: 1,
            });
            n += 1;
        }
    }
    n
}

/// The pass of one tick for every player whose phase it is (once per 16 ticks, like the player process of the
/// original); called by [`crate::item::tick::process`].
pub fn process_expiring_all(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    if svc.items.core().cfg.perm_extrabags {
        return;
    }
    let tick = ctx.clock.tick.0;
    let due: Vec<_> = ctx
        .sessions
        .iter()
        .map(|e| e.player)
        .filter(|id| (tick + u64::from(id.0)).is_multiple_of(PERIOD_TICKS))
        .collect();
    for id in due {
        let who = PlayerRef(id);
        let n = with_player(svc, ctx, who, |m, env, _, fx| {
            let n = process_expiring(m, env, fx);
            if n > 0 {
                fx.mark_urgent();
            }
            n
        });
        let _ = n;
    }
}

/// `CF_SEC`: the player process of the original runs the pocket and collecting passes when
/// `(m_nCount & 15) == 0`, once per 16 ticks (about 1.07 s).
pub const PERIOD_TICKS: u64 = 16;
