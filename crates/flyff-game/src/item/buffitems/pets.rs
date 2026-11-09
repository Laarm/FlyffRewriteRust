//! The pet items that are buffs (Plan/19 §5.4, §13.2, §13.4; ref/13 §10.3, §19.5; `MoverSkill.cpp:3955-4004,
//! 4309-4330`, `DPSrvr.cpp:10463`): the pet tonics, the pet feed pocket and `FeedPocketInactive`.
//!
//! * **`PET_TONIC_A` / `PET_TONIC_B`** (`DoUseItemPetTonic`): the two are exclusive (2 = `TID_GAME_LIMITED_USE`);
//!   a second use of the same tonic is refused (1, silent, not consumed) when the remaining time plus a new one
//!   would overflow the `int` of the original, else the buff is applied (the accumulation of the time is the
//!   buff engine's: `AddTotal` adds the remaining time of these ids). What the buff *does* is the pet processing
//!   (`item::pets::system`): no energy loss, +50 % pet EXP for B.
//! * **`PET_FEED_POCKET`** (`DoUseItemFeedPocket`): refused with `TID_GAME_DONOTUSE_PETPOCKET` while the
//!   `FEED_POCKET02` buff runs; used while its own buff runs it switches it off (a timed pocket, answer 4) or is
//!   refused with 2 (a pocket without a keep time); otherwise the first use starts the keep time of
//!   `ability_min` minutes (refused, with the same text, while another timed pocket of the same id is not
//!   expired in the bag) and applies the buff. The pocket is "in use" (`PlayerItems::active_feed_pocket`) while
//!   its buff runs; the field follows the buff within one pass ([`sync_active_pocket`]).
//! * **`FeedPocketInactive`** removes the buff of the pocket.

use super::Later;
use super::exclusive::{has_item, would_overflow};
use crate::buff::core::{BuffCx, remove_fake};
use crate::buff::set::{BuffKey, BuffKind};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::env::ItemEnv;
use crate::item::fx::Fx;
use crate::item::ops::{UiParam, core as ops};
use crate::item::usage::RowCx;
use crate::item::usage::system::{CONSUME, IN_USE, Sys};
use crate::mover::Mover;
use flyff_common::defs::item::{
    II_SYS_SYS_SCR_PET_FEED_POCKET, II_SYS_SYS_SCR_PET_FEED_POCKET02, II_SYS_SYS_SCR_PET_TONIC_A,
    II_SYS_SYS_SCR_PET_TONIC_B,
};
use flyff_common::defs::text::TID_GAME_DONOTUSE_PETPOCKET;
use flyff_data::props::item::ItemProp;
use flyff_proto::pb;

/// The answer "refused, silently" of `DoUseItemFeedPocket` (4) and of the tonic overflow (1 is the fall-through).
const SILENT: Sys = Sys::Ret(4);

/// `DoUseItemPetTonic`.
pub fn use_tonic(m: &Mover, prop: &ItemProp, ux: &RowCx<'_>, fx: &mut Fx) -> Sys {
    let other = if prop.id == II_SYS_SYS_SCR_PET_TONIC_A {
        II_SYS_SYS_SCR_PET_TONIC_B
    } else {
        II_SYS_SYS_SCR_PET_TONIC_A
    };
    if has_item(m, other) {
        return IN_USE;
    }
    if would_overflow(
        m,
        BuffKey::new(BuffKind::Item, prop.id),
        prop.skill_time,
        ux.now,
    ) {
        return Sys::Ret(1);
    }
    fx.later(Later::ApplyItem {
        target: m.id(),
        item: prop.id,
    });
    CONSUME
}

/// `DoUseItemFeedPocket` for the pocket `ux.id`.
pub fn use_feed_pocket(
    m: &mut Mover,
    env: &ItemEnv<'_>,
    prop: &ItemProp,
    ux: &RowCx<'_>,
    fx: &mut Fx,
) -> Sys {
    if has_item(m, II_SYS_SYS_SCR_PET_FEED_POCKET02) {
        fx.text(TID_GAME_DONOTUSE_PETPOCKET);
        return SILENT;
    }
    let keep = m
        .player()
        .and_then(|p| p.items.inventory.get_at_id(ux.id))
        .map_or(0, |it| it.keep_until);
    if has_item(m, prop.id) {
        // active: a timed pocket is switched off, a permanent one cannot be used twice
        if keep != 0 {
            let cx = BuffCx::new(*env, ux.now);
            remove_fake(m, &cx, BuffKey::new(BuffKind::Item, prop.id), fx);
            set_active(m, None);
            return SILENT;
        }
        return IN_USE;
    }
    if keep == 0 {
        // no keep time yet: the first use starts it, unless another timed pocket of the id is running
        let other_timed = m.player().is_some_and(|p| {
            p.items.inventory.iter().any(|(_, it)| {
                it.is_usable() && it.item_id == prop.id && it.keep_until > 0 && !it.is_expired()
            })
        });
        if other_timed {
            fx.text(TID_GAME_DONOTUSE_PETPOCKET);
            return SILENT;
        }
        ops::update_item(m, env, ux.id, UiParam::KeepTime, prop.ability_min, 0, fx);
    }
    set_active(m, Some(ux.id));
    fx.later(Later::ApplyItem {
        target: m.id(),
        item: prop.id,
    });
    CONSUME
}

fn set_active(m: &mut Mover, id: Option<crate::item::container::StorageId>) {
    if let Some(p) = m.player_mut() {
        p.items.active_feed_pocket = id;
    }
}

/// The timed pocket of the bag that the running buff belongs to: usable, not expired, with a keep time.
fn running_pocket(m: &Mover) -> Option<crate::item::container::StorageId> {
    // a fake-removed entry (switched off a moment ago) is not running
    let key = BuffKey::new(BuffKind::Item, II_SYS_SYS_SCR_PET_FEED_POCKET);
    if !m.buffs.get(key).is_some_and(|e| !e.removed) {
        return None;
    }
    m.player()?
        .items
        .inventory
        .iter()
        .find(|(_, it)| {
            it.item_id == II_SYS_SYS_SCR_PET_FEED_POCKET && it.keep_until > 0 && !it.is_expired()
        })
        .map(|(id, _)| id)
}

/// Makes `PlayerItems::active_feed_pocket` agree with the buff (the original computed "in use" from it).
pub fn sync_active_pocket(m: &mut Mover) {
    let want = running_pocket(m);
    if m.player()
        .is_some_and(|p| p.items.active_feed_pocket != want)
    {
        set_active(m, want);
    }
}

/// `OnFeedPocketInactive` over one mover: the buff goes. Returns `true` when there was one.
pub fn feed_pocket_inactive(
    m: &mut Mover,
    env: &ItemEnv<'_>,
    now: flyff_common::GameMs,
    fx: &mut Fx,
) -> bool {
    if !has_item(m, II_SYS_SYS_SCR_PET_FEED_POCKET) {
        return false;
    }
    let cx = BuffCx::new(*env, now);
    remove_fake(
        m,
        &cx,
        BuffKey::new(BuffKind::Item, II_SYS_SYS_SCR_PET_FEED_POCKET),
        fx,
    );
    set_active(m, None);
    true
}

/// `FeedPocketInactive` (C2S, no payload).
pub fn on_feed_pocket_inactive(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    _: pb::FeedPocketInactive,
) {
    let core = svc.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    let now = ctx.clock.game_ms;
    let mut fx = Fx::new();
    if let Some(m) = ctx.worlds.player_mut(who.0) {
        feed_pocket_inactive(m, &env, now, &mut fx);
    }
    fx.flush(svc, ctx, who);
}

/// The pass over the players whose phase it is (once per 16 ticks): [`sync_active_pocket`].
pub fn tick(ctx: &mut GameCtx<'_>) {
    let tick = ctx.clock.tick.0;
    let due: Vec<_> = ctx
        .sessions
        .iter()
        .map(|e| e.player)
        .filter(|id| (tick + u64::from(id.0)).is_multiple_of(16))
        .collect();
    for id in due {
        if let Some(m) = ctx.worlds.player_mut(id) {
            sync_active_pocket(m);
        }
    }
}
