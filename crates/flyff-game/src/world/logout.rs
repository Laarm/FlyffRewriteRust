//! The world part of the logout (Plan/15 §18, WP-WORLD-13; ref/06 §16; src:WORLDSERVER/User.cpp:4017-4298; F-038).
//!
//! The leave rule (immediate or linger) is doc 12's `leave_mode`; the session layer's `finish_leave` is the
//! only caller of the two functions here, in this order (12 §12.5, 15 §18.2):
//!
//! 1. [`destroy_player`] (steps 1-4): the cleanup hooks of docs 21/19/22 ([`LogoutStep`]), the location to save
//!    and its overrides, the chat-room leave;
//! 2. the final snapshot with that location (doc 12/13, not here);
//! 3. [`remove_player`] (steps 6-8): the guild-house exit, the removal from the world (`DelObj` to the observers at
//!    that tick's `_delete`; a pending-add player is dropped silently), the siege hooks.
//!
//! The hooks of the other docs are one trait, [`LogoutHooks`]: [`LogoutHooks::step`] is called for every cleanup
//! of §18.2 in the order of the document, so a recording stub can assert the order; two predicates answer the
//! questions that need doc 22's state.

use super::revival::{MADRIGAL_FLARIS_CENTER, RevivalPoint};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::persist::snapshot::Location;
use flyff_common::defs::world::{
    WI_DUNGEON_SECRET_0, WI_WORLD_GUILDWAR, WI_WORLD_MADRIGAL, WI_WORLD_MINIROOM,
};
use flyff_common::math::Vec3;
use flyff_common::{LayerId, WorldId};

/// One cleanup of §18.2, in call order. The doc that owns the step is in the comment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogoutStep {
    /// Step 1: guild buffs removed (doc 21).
    GuildBuffs,
    /// Step 1: collecting stopped (doc 19).
    StopCollecting,
    /// Step 1: the five-system mini-game window closed (doc 22).
    CloseMiniGame,
    /// Step 1: the eat-pet deactivated (doc 19).
    DeactivateEatPet,
    /// Step 2, guild-war world and alive: `remove_all_enemies` (doc 17).
    RemoveAllEnemiesGuildWar,
    /// Step 2, dead: death EXP penalty (KQ-09), HP = max * rate, MP/FP raised to max * rate (doc 16/17; the exp
    /// box is DROP).
    DeadLogout,
    /// Step 2, alive and flying: `stop` (doc 18).
    StopFlying,
    /// Step 2, alive: `remove_all_enemies` (doc 17).
    RemoveAllEnemies,
    /// Step 3: chat-room membership removed (doc 23).
    LeaveChatRoom,
    /// Step 4: the secret-room override (doc 22) applied.
    SecretRoomOverride,
    /// Step 4: housing destroyed after the miniroom override (doc 22).
    DestroyHousing,
    /// Step 4: the rainbow race marks the player dropped out (doc 22).
    RainbowDropOut,
    /// Step 4: `instance_go_out(player)` (doc 22).
    InstanceGoOut,
    /// Step 6: guild-house exit (doc 22).
    GuildHouseGoOut,
    /// Step 8: the guild siege `out_war` (doc 21, `events.guild_combat`, maintenance or war state).
    SiegeOutWar,
    /// Step 8: the GC1to1 result check (doc 21, `events.guild_combat_1to1`).
    Gc1to1Check,
}

/// The hooks of the docs that own a cleanup (15 §18.2). Every method has the "feature absent" default.
pub trait LogoutHooks: Send {
    /// Runs one cleanup. Called in the order of [`LogoutStep`] for the steps that apply.
    fn step(
        &mut self,
        _step: LogoutStep,
        _svc: &mut Services,
        _ctx: &mut GameCtx<'_>,
        _player: PlayerRef,
    ) {
    }

    /// Doc 22: the revival position of a secret-room participant (`Some` = the player is in a secret room).
    fn secret_room_position(&self, _ctx: &GameCtx<'_>, _player: PlayerRef) -> Option<Vec3> {
        None
    }

    /// Doc 22's `IsInstanceDungeon(world)`.
    fn is_instance_world(&self, _world: WorldId) -> bool {
        false
    }

    /// Doc 17's `is_chaotic` (`pk_propensity > 0`, 16 §16).
    fn is_chaotic(&self, ctx: &GameCtx<'_>, player: PlayerRef) -> bool {
        ctx.player(player)
            .and_then(|m| m.player())
            .is_some_and(|p| p.pk.propensity > 0)
    }
}

/// The hooks of a game without docs 16-23: nothing to clean up.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoLogoutHooks;

impl LogoutHooks for NoLogoutHooks {}

fn madrigal() -> WorldId {
    WorldId(WI_WORLD_MADRIGAL)
}

/// The Madrigal mark of the player, else the Flaris centre (`marking_world == 1 ? marking_pos : (6968, 100,
/// 3328)`).
fn mark_or_flaris(ctx: &GameCtx<'_>, player: PlayerRef) -> Vec3 {
    ctx.player(player)
        .and_then(|m| m.player())
        .filter(|p| p.misc.marking_world == madrigal())
        .map_or(MADRIGAL_FLARIS_CENTER, |p| p.misc.marking_pos)
}

/// `world::destroy_player` (15 §18.2 steps 1-4): the cleanups and the location to save, in this order:
///
/// 1. guild buffs, collecting, the mini-game window, the eat-pet;
/// 2. the location: **guild-war world 202**: the Madrigal `flaris` revival point (layer 0) when it exists, else
///    where the player stands (alive: `remove_all_enemies`); **dead**: the death-logout hook, then
///    `select_revival(world, pos, chaotic)` (layer 0), `None` keeps the current location; **alive**: where it
///    stands (flying: `stop`; `remove_all_enemies`);
/// 3. the chat room;
/// 4. the overrides, in this order: a secret-room participant -> `(1, secret_room_position, 0)`; the secret
///    dungeon 151 -> layer 0 and the Madrigal mark or the Flaris centre; the miniroom 209 -> `(1, (6968, 100,
///    3328), 0)` and doc 22 destroys the housing (also for everybody else, the original called it
///    unconditionally); the rainbow-race drop-out; an instance-dungeon world -> `instance_go_out`, then the
///    Madrigal mark or the Flaris centre, layer 0.
pub fn destroy_player(
    hooks: &mut dyn LogoutHooks,
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    player: PlayerRef,
) -> Location {
    use LogoutStep as S;
    hooks.step(S::GuildBuffs, svc, ctx, player);
    hooks.step(S::StopCollecting, svc, ctx, player);
    hooks.step(S::CloseMiniGame, svc, ctx, player);
    hooks.step(S::DeactivateEatPet, svc, ctx, player);

    let (world, pos, layer, dead, flying) = match ctx.player(player) {
        Some(m) => (
            m.core.world,
            m.core.pos,
            m.core.layer,
            m.mv.hp <= 0,
            m.sim.act.flags.fly(),
        ),
        None => (madrigal(), MADRIGAL_FLARIS_CENTER, LayerId(0), false, false),
    };
    let current = Location { world, pos, layer };
    let mut loc = current;
    let flaris =
        || -> Option<RevivalPoint> { ctx.worlds.revival.revival_pos(madrigal(), "flaris") };

    if world.0 == WI_WORLD_GUILDWAR {
        if let Some(p) = flaris() {
            loc = Location {
                world: p.world,
                pos: p.pos,
                layer: LayerId(0),
            };
        }
        if !dead {
            hooks.step(S::RemoveAllEnemiesGuildWar, svc, ctx, player);
        }
    } else if dead {
        hooks.step(S::DeadLogout, svc, ctx, player);
        let chaotic = hooks.is_chaotic(ctx, player);
        let settings = ctx.worlds.world(world).map(|w| w.statics.settings.clone());
        if let Some(s) = settings
            && let Some(p) = ctx.worlds.revival.select(world, &s, pos, chaotic)
        {
            loc = Location {
                world: p.world,
                pos: p.pos,
                layer: LayerId(0),
            };
        }
    } else {
        if flying {
            hooks.step(S::StopFlying, svc, ctx, player);
        }
        hooks.step(S::RemoveAllEnemies, svc, ctx, player);
    }

    hooks.step(S::LeaveChatRoom, svc, ctx, player);

    // 4: the overrides, in order
    if let Some(p) = hooks.secret_room_position(ctx, player) {
        hooks.step(S::SecretRoomOverride, svc, ctx, player);
        loc = Location {
            world: madrigal(),
            pos: p,
            layer: LayerId(0),
        };
    }
    if world.0 == WI_DUNGEON_SECRET_0 {
        loc = Location {
            world: madrigal(),
            pos: mark_or_flaris(ctx, player),
            layer: LayerId(0),
        };
    }
    if world.0 == WI_WORLD_MINIROOM {
        loc = Location {
            world: madrigal(),
            pos: MADRIGAL_FLARIS_CENTER,
            layer: LayerId(0),
        };
    }
    hooks.step(S::DestroyHousing, svc, ctx, player);
    hooks.step(S::RainbowDropOut, svc, ctx, player);
    if hooks.is_instance_world(world) {
        hooks.step(S::InstanceGoOut, svc, ctx, player);
        loc = Location {
            world: madrigal(),
            pos: mark_or_flaris(ctx, player),
            layer: LayerId(0),
        };
    }
    loc
}

/// `world::remove_player` (15 §18.2 steps 6-8): the guild-house exit (doc 22), the removal from the world (a
/// player that is still in an add queue is dropped silently, otherwise it is deleted and `DelObj` goes to its
/// observers at that tick's `_delete`), and the siege hooks (doc 21; `events.guild_combat` /
/// `events.guild_combat_1to1`). A second call is a no-op.
pub fn remove_player(
    hooks: &mut dyn LogoutHooks,
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    player: PlayerRef,
) {
    use LogoutStep as S;
    if ctx.worlds.core(player.0).is_none_or(|c| c.deleted) {
        return;
    }
    hooks.step(S::GuildHouseGoOut, svc, ctx, player);
    ctx.worlds.delete(player.0);
    use flyff_common::config::EventId;
    if ctx.live.event_state(EventId::GuildCombat) != 0 {
        hooks.step(S::SiegeOutWar, svc, ctx, player);
    }
    if ctx.live.event_state(EventId::GuildCombat1to1) != 0 {
        hooks.step(S::Gc1to1Check, svc, ctx, player);
    }
}
