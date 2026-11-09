//! Pet feeding, the food mill, the tamer's scrolls, the resurrection fragment and the egg transform (Plan/19 §13.4,
//! WP-ITEM-17; ref/13 §19.5-§19.7; src:WORLDSERVER/DPSrvr.cpp:10186-10475, 11601-11645, 12186-12198).
//!
//! | module | content |
//! |---|---|
//! | [`feeding`] | `UsePetFeed`: the stack, capped by the need of the pet |
//! | [`mill`] | `MakePetFeed`: gems into feed, with a feed maker or at the mill NPC |
//! | [`tamer`] | `PetTamerMistake` (+ the auto level-up) and `PetTamerMiracle` |
//! | [`resurrect`] | `QuePetResurrection` and the target-scroll branch for the `..._02` fragments |
//! | [`transform`] | `TransformItem` with the PL-069 / VR-025 fixes |
//!
//! The feed pocket (`FeedPocketInactive`, the auto-feed buff) is WP-ITEM-20's; the pocket's per-pass use of the
//! feed is in `system::process`.

pub mod feeding;
pub mod mill;
pub mod resurrect;
pub mod tamer;
pub mod transform;

pub use feeding::{EGG_FEED_REFUSAL, use_pet_feed};
pub use mill::on_make_pet_feed;
pub use resurrect::{on_que_pet_resurrection, use_target_scroll};
pub use tamer::{tamer_miracle, tamer_mistake};
pub use transform::on_transform_item;

use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::container::StorageId;
use crate::item::pets::system::hooks::with_player;
use flyff_proto::pb;

/// `UsePetFeed{feed_obj_id}`: see [`use_pet_feed`].
pub fn on_use_pet_feed(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::UsePetFeed,
) {
    let Ok(feed) = StorageId::try_from(msg.feed_obj_id) else {
        return;
    };
    with_player(svc, ctx, who, |m, cx, _, fx| use_pet_feed(m, cx, feed, fx));
}

/// `PetTamerMistake{obj_id}`: see [`tamer_mistake`].
pub fn on_pet_tamer_mistake(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::PetTamerMistake,
) {
    let Ok(scroll) = StorageId::try_from(msg.obj_id) else {
        return;
    };
    with_player(svc, ctx, who, |m, cx, rng, fx| {
        tamer_mistake(m, cx, rng, scroll, fx);
    });
}

/// `PetTamerMiracle{obj_id}`: see [`tamer_miracle`].
pub fn on_pet_tamer_miracle(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::PetTamerMiracle,
) {
    let Ok(scroll) = StorageId::try_from(msg.obj_id) else {
        return;
    };
    with_player(svc, ctx, who, |m, cx, rng, fx| {
        tamer_miracle(m, cx, rng, scroll, fx);
    });
}
