//! `event::misc`: the small systems of Plan/22 §17 (WP-EVENT-21): the arena entry and exit, the balloon rule, the
//! Wormon switch and the `/ctd` service.
//!
//! **Arena (§17.1).** `ArenaEnter` is sent by the client at the menu `MMI_ARENA_ENTER` (80) of an NPC, `ArenaExit` at
//! `MMI_ARENA_EXIT` (81) (`WndWorld.cpp:6118-6123`). The original handlers (`DPSrvr.cpp:11577-11596`) checked nothing
//! about the NPC; the rewrite adds VR-012 (an NPC with the menu within `gameplay.npc_menu_range`, same world and
//! layer) for the entry and VR-062 (only inside the arena world) for the exit. Arena PvP is doc 17's
//! (`HitType::Arena`).
//!
//! * [`on_arena_enter`]: no NPC 80 close by, or a Vagrant (`IsBaseJob`): ignored silently. Otherwise the player's
//!   place is stored as the marking position (`SetMarkingPos`, before the move; KEEP: these are the Lodelight mark
//!   fields, so entering the arena overwrites the persisted Lodelight mark exactly as in the original) and the
//!   player is placed at `(540, 140, 485)` of world 208, layer 0. A refused replace (flying) is logged and leaves the
//!   mark set (legacy order).
//! * [`on_arena_exit`]: only in world 208. The player goes back to the mark; no mark, or a mark the replace refuses,
//!   sends him to the Flaris centre of Madrigal. FIX: the original left a player without a mark in the arena.
//!
//! **Balloon (§17.2).** Nothing to build here: doc 19's `IK3_BALLOON` rule (a second balloon item while a balloon
//! buff runs answers result 2 = `TID_GAME_LIMITED_USE` and keeps the item, `item/buffitems/system.rs`) is the rule;
//! `event::on_item_use` has no balloon branch. The test `res_balloon_second_item_is_refused_with_result_2` proves it
//! on two real balloon items.
//!
//! **Wormon and `/ctd` (§17.3).** Flag 9 (`events.wormon`, default on, PD-14) feeds the NPC helper
//! `IsWormonServer()`. `/ctd` (`FuncTextCmd.cpp:3085-3127`) creates a duel zone region and acts only on a server that
//! is not a Wormon server; the region itself needs `AddRegion` (F-101 DEFER, PL-147, WP-GM-30 of M8):
//! [`create_duel_zone`] is the service half and reports what the command would do.

use crate::combat::pvp::hit::is_vagrant;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::world::ReplaceKind;
use crate::world::revival::MADRIGAL_FLARIS_CENTER;
use flyff_common::config::EventId;
use flyff_common::defs::neuz::MMI_ARENA_ENTER;
use flyff_common::defs::world::{WI_WORLD_ARENA, WI_WORLD_MADRIGAL};
use flyff_common::math::Vec3;
use flyff_common::{LayerId, WorldId};
use flyff_proto::pb;

/// Where `ArenaEnter` puts the player: `(540, 140, 485)` of world 208, layer 0.
pub const ARENA_ENTRY: Vec3 = Vec3::new(540.0, 140.0, 485.0);

/// `ArenaEnter` (22 §17.1, VR-012): see the module documentation.
pub fn on_arena_enter(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    _msg: pb::ArenaEnter,
) {
    let Some((world, layer, pos)) = ctx.worlds.core(who.0).map(|c| (c.world, c.layer, c.pos))
    else {
        return;
    };
    if ctx
        .worlds
        .close_npc(MMI_ARENA_ENTER, world, layer, pos)
        .is_none()
    {
        return;
    }
    let core = svc.stats.handle();
    let vagrant = ctx
        .worlds
        .mover(who.0)
        .is_some_and(|m| is_vagrant(m, &core.rules));
    if vagrant {
        return;
    }
    let Some(p) = ctx.worlds.player_mut(who.0).and_then(|m| m.player_mut()) else {
        return;
    };
    // `SetMarkingPos()` before the move
    p.misc.marking_world = world;
    p.misc.marking_pos = pos;
    svc.saves.mark_urgent(who.0);
    if let Err(e) = ctx.worlds.replace(
        who.0,
        WorldId(WI_WORLD_ARENA),
        ARENA_ENTRY,
        LayerId(0),
        ReplaceKind::Normal,
    ) {
        tracing::info!(
            ?e,
            "ArenaEnter: the player could not be placed in the arena"
        );
    }
}

/// `ArenaExit` (22 §17.1, VR-062): see the module documentation.
pub fn on_arena_exit(
    _svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    _msg: pb::ArenaExit,
) {
    if ctx.worlds.core(who.0).map(|c| c.world) != Some(WorldId(WI_WORLD_ARENA)) {
        return;
    }
    let mark = ctx
        .worlds
        .mover(who.0)
        .and_then(|m| m.player())
        .map(|p| (p.misc.marking_world, p.misc.marking_pos));
    if let Some((world, pos)) = mark
        && world.0 != 0
        && ctx
            .worlds
            .replace(who.0, world, pos, LayerId(0), ReplaceKind::Normal)
            .is_ok()
    {
        return;
    }
    if let Err(e) = ctx.worlds.replace(
        who.0,
        WorldId(WI_WORLD_MADRIGAL),
        MADRIGAL_FLARIS_CENTER,
        LayerId(0),
        ReplaceKind::Normal,
    ) {
        tracing::warn!(?e, "ArenaExit: the player could not be placed in Flaris");
    }
}

/// What `/ctd` would do (22 §17.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CtdOutcome {
    /// The server is a Wormon server (flag 9 on): the command does nothing. The original said nothing; the text is
    /// the plan's ([`CtdOutcome::text`]).
    NotAvailable,
    /// The command would create the duel-zone region, which needs `AddRegion` (PL-147, WP-GM-30): no region is
    /// created and nothing is announced.
    Deferred,
}

impl CtdOutcome {
    /// The line the GM handler answers.
    pub fn text(self) -> &'static str {
        match self {
            CtdOutcome::NotAvailable => "not available",
            CtdOutcome::Deferred => "needs AddRegion, PL-147",
        }
    }
}

/// `/ctd` service half (22 §17.3): `events.wormon` (flag 9) on means "not available"; otherwise the region is
/// deferred (F-101).
pub fn create_duel_zone(svc: &Services, _ctx: &GameCtx<'_>) -> CtdOutcome {
    if svc.event.flags.is_on(EventId::Wormon) {
        CtdOutcome::NotAvailable
    } else {
        CtdOutcome::Deferred
    }
}
