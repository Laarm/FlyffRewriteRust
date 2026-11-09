//! The `GameCtx` wrappers of the upgrade handlers (Plan/19 §19; registered in `flyff_game::handlers`): resolve
//! the actor, the clock, the item environment and the world's random stream, run the mover-level function of
//! this module tree and flush its messages, audit lines and urgent-save request once.
//!
//! All handlers use the player of the session (VR-001); a message names only inventory object ids, which the
//! functions re-validate. The NPC services check `NpcChecker::close_npc` first (VR-012: 32 m in 3-D, same
//! world and layer).

use super::safe::UpgradeHooks;
use super::txn::{Outcome, UpgradeCx};
use super::{element, random_option, safe, stones, target};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::fx::Fx;
use crate::item::usage::UseFacts;
use crate::mover::Mover;
use flyff_common::defs::neuz::{MMI_BLESSING_CANCEL, MMI_ITEM_AWAKENING, MMI_PET_AWAK_CANCEL};
use flyff_common::defs::world::WI_WORLD_QUIZ;
use flyff_common::rng::GameRng;
use flyff_proto::pb;
use flyff_rules::item::refine::UpgradeTables;

/// The facts of the player's other states, with the quiz world filled in.
fn facts_of(m: &Mover) -> UseFacts {
    UseFacts {
        quiz_world: m.core.world.0 == WI_WORLD_QUIZ,
        ..UseFacts::of(m)
    }
}

/// `true` when a menu NPC of `menu` is within the NPC-menu range of the player (VR-012).
fn npc_near(ctx: &GameCtx<'_>, who: PlayerRef, menu: u32) -> bool {
    ctx.worlds.core(who.0).is_some_and(|c| {
        ctx.worlds
            .close_npc(menu, c.world, c.layer, c.pos)
            .is_some()
    })
}

/// Runs `f` for the player `who` and flushes its effects. `None` when `who` is not a live player.
fn run<R>(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    f: impl FnOnce(&mut Mover, &UpgradeCx<'_>, &UseFacts, &UpgradeHooks, &mut dyn GameRng, &mut Fx) -> R,
) -> Option<R> {
    let core = svc.items.handle();
    let hooks = svc.items.upgrade_hooks;
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    let data = &*core.data;
    let tables = UpgradeTables::from_data(
        data.lua.item_upgrade.as_ref(),
        &data.accessory,
        &data.collecting,
    );
    let cx = UpgradeCx::new(&env, ctx.clock.game_ms, &tables);
    let world = ctx.worlds.core(who.0)?.world;
    let mut fx = Fx::new();
    let r = {
        let m = ctx.worlds.player_mut(who.0)?;
        let facts = facts_of(m);
        f(m, &cx, &facts, &hooks, ctx.rng.world(world), &mut fx)
    };
    fx.flush(svc, ctx, who);
    Some(r)
}

/// `Enchant{item, material}` (C2S `ENCHANT`).
pub fn on_enchant(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::Enchant,
) -> Outcome {
    run(svc, ctx, who, |m, cx, facts, _, rng, fx| {
        super::enchant(m, cx, &msg, facts, rng, fx)
    })
    .unwrap_or(Outcome::Ignored)
}

/// `SmeltSafetyReq` (C2S `SMELT_SAFETY`): answers `SmeltSafetyNtf`. Returns the result byte.
pub fn on_smelt_safety(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::SmeltSafetyReq,
) -> u8 {
    run(svc, ctx, who, |m, cx, facts, hooks, rng, fx| {
        safe::smelt_safety(m, cx, &msg, facts, hooks, rng, fx)
    })
    .unwrap_or(0)
}

/// `RemoveAttributeReq` (C2S `REMVOE_ATTRIBUTE`): answers `RemoveAttributeNtf`.
pub fn on_remove_attribute(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::RemoveAttributeReq,
) -> Outcome {
    run(svc, ctx, who, |m, cx, facts, _, _, fx| {
        element::on_remove(m, cx, &msg, facts, fx)
    })
    .unwrap_or(Outcome::Ignored)
}

/// `ChangeAttribute` (C2S `CHANGE_ATTRIBUTE`).
pub fn on_change_attribute(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::ChangeAttribute,
) -> Outcome {
    run(svc, ctx, who, |m, cx, facts, _, _, fx| {
        element::on_change(m, cx, &msg, facts, fx)
    })
    .unwrap_or(Outcome::Ignored)
}

/// `CommercialElemReq` (C2S `COMMERCIALELEM`): answers `CommercialElemNtf`.
pub fn on_commercial_elem(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::CommercialElemReq,
) -> Outcome {
    run(svc, ctx, who, |m, cx, _, _, _, fx| {
        stones::on_commercial_elem(m, cx, &msg, fx)
    })
    .unwrap_or(Outcome::Ignored)
}

/// `RandomScroll` (C2S `RANDOMSCROLL`).
pub fn on_random_scroll(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::RandomScroll,
) -> Outcome {
    run(svc, ctx, who, |m, cx, _, _, rng, fx| {
        random_option::on_stat_scroll(m, cx, &msg, rng, fx)
    })
    .unwrap_or(Outcome::Ignored)
}

/// `DoUseItemTarget` (C2S `DO_USE_ITEM_TARGET`).
pub fn on_use_target(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::DoUseItemTarget,
) -> Outcome {
    run(svc, ctx, who, |m, cx, _, hooks, rng, fx| {
        target::on_use_target(m, cx, &msg, hooks, rng, fx)
    })
    .unwrap_or(Outcome::Ignored)
}

/// `Awakening` (C2S `AWAKENING`) at the awakening NPC.
pub fn on_awakening(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::Awakening,
) -> Outcome {
    let near = npc_near(ctx, who, MMI_ITEM_AWAKENING);
    run(svc, ctx, who, |m, cx, facts, _, rng, fx| {
        random_option::on_npc_awakening(m, cx, &msg, near, facts, rng, fx)
    })
    .unwrap_or(Outcome::Ignored)
}

/// `BlessednessCancel` (C2S `BLESSEDNESS_CANCEL`) at the blessing-cancel NPC.
pub fn on_blessedness_cancel(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::BlessednessCancel,
) -> Outcome {
    let near = npc_near(ctx, who, MMI_BLESSING_CANCEL);
    run(svc, ctx, who, |m, cx, _, _, _, fx| {
        random_option::on_blessedness_cancel(m, cx, &msg, near, fx)
    })
    .unwrap_or(Outcome::Ignored)
}

/// `PickupPetAwakeningCancel` (C2S) at the pet-awakening-cancel NPC.
pub fn on_eat_pet_cancel(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::PickupPetAwakeningCancel,
) -> Outcome {
    let near = npc_near(ctx, who, MMI_PET_AWAK_CANCEL);
    run(svc, ctx, who, |m, cx, _, hooks, _, fx| {
        random_option::on_eat_pet_cancel(m, cx, &msg, near, hooks, fx)
    })
    .unwrap_or(Outcome::Ignored)
}

/// `ItemSelectAwakeningValueReq` (C2S `ITEM_SELECT_AWAKENING_VALUE`).
pub fn on_select_value(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::ItemSelectAwakeningValueReq,
) -> Outcome {
    run(svc, ctx, who, |m, cx, _, _, _, fx| {
        random_option::on_select_value(m, cx, &msg, fx)
    })
    .unwrap_or(Outcome::Ignored)
}

/// `RandomOptionRetry` (C2S `RANDOMOPTIONRETRY`).
pub fn on_retry(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::RandomOptionRetry,
) -> Outcome {
    run(svc, ctx, who, |m, cx, _, _, rng, fx| {
        random_option::on_retry(m, cx, &msg, rng, fx)
    })
    .unwrap_or(Outcome::Ignored)
}

/// `RemoveItemLevelDown` (C2S `REMOVE_ITEM_LEVEL_DOWN`).
pub fn on_remove_level_down(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::RemoveItemLevelDown,
) -> Outcome {
    run(svc, ctx, who, |m, cx, _, _, _, fx| {
        random_option::on_remove_level_down(m, cx, &msg, fx)
    })
    .unwrap_or(Outcome::Ignored)
}

/// After the join batch and after a re-attach (PL-079): offers every pending protected awakening again to
/// `session` (the join runs before the avatar is bound to its session, so the message goes to the session
/// directly).
pub fn resend_pending(ctx: &mut GameCtx<'_>, who: PlayerRef, session: flyff_common::SessionId) {
    let msgs = ctx
        .worlds
        .player(who.0)
        .map(random_option::pending_messages)
        .unwrap_or_default();
    for msg in msgs {
        ctx.outbox.to_session(session, 0, msg);
    }
}
