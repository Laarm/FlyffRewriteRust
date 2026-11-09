//! The return scroll (Plan/19 §5.4, §5.7.3; ref/13 §10.3, ref/07b §15.2; `MoverSkill.cpp:2634-2681`, `DPSrvr.cpp:
//! 7176-7199`, `MoverSkill.cpp:1191-1235`).
//!
//! **The scroll** (`II_SYS_SYS_SCR_RETURN`, cast 4 s like every item with a cast time): a running buff of it
//! answers 2; in the guild-war world, in Kebaras, in a guild-quest region or in a 1:1 guild combat the use is
//! refused with `TID_GAME_LIMITZONE_USE` - **FIX:** the original answered `return 0` there, i.e. "consume the
//! scroll" after telling the player it cannot be used; here nothing is consumed (99). Else the current spot
//! becomes the return point (`PlayerMisc.return_world / return_pos`), the buff is applied and the player is
//! teleported to the revival point of the village chosen with `ReturnScrollReq{0..2}` (a [`Later`]).
//!
//! **The trip back** (`ReturnScrollReq{-1}`): with the buff running, a virtual use of the scroll (the 4 s cast
//! first, nothing consumed): the buff is removed and the player is replaced to the stored return point. A stored
//! point of exactly (0, 0, 0) is the "no return point" of a new character (KEEP-QUIRK of the `return_pos`
//! default, 13 OQ15): the buff is removed and nothing else happens.

use super::Later;
use super::exclusive::has_item;
use crate::buff::core::{BuffCx, remove_fake};
use crate::buff::set::{BuffKey, BuffKind};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::follow::Followup;
use crate::item::env::ItemEnv;
use crate::item::fx::Fx;
use crate::item::usage::RowCx;
use crate::item::usage::system::{CONSUME, IN_USE, REFUSED, Sys};
use crate::mover::Mover;
use crate::world::ReplaceKind;
use flyff_common::GameMs;
use flyff_common::LayerId;
use flyff_common::defs::item::II_SYS_SYS_SCR_RETURN;
use flyff_common::defs::text::TID_GAME_LIMITZONE_USE;
use flyff_common::defs::world::{WI_WORLD_GUILDWAR, WI_WORLD_KEBARAS};
use flyff_common::math::Vec3;
use flyff_data::props::item::ItemProp;

/// The scroll row of `DoUseItemSystem`.
pub fn use_row(m: &mut Mover, prop: &ItemProp, ux: &RowCx<'_>, fx: &mut Fx) -> Sys {
    if !m.is_player() {
        return CONSUME;
    }
    if has_item(m, II_SYS_SYS_SCR_RETURN) {
        return IN_USE;
    }
    let world = m.core.world.0;
    if world == WI_WORLD_GUILDWAR
        || world == WI_WORLD_KEBARAS
        || ux.facts.guild_quest_region
        || ux.facts.gc1to1
    {
        fx.text(TID_GAME_LIMITZONE_USE);
        return REFUSED;
    }
    let (here_world, here_pos) = (m.core.world, m.core.pos);
    if let Some(p) = m.player_mut() {
        p.misc.return_world = here_world;
        p.misc.return_pos = here_pos;
    }
    fx.later(Later::ApplyItem {
        target: m.id(),
        item: prop.id,
    });
    fx.later(Later::ReturnVillage { player: m.id() });
    CONSUME
}

/// The virtual use after the cast: the buff goes, the player returns to the stored point. Always `true` (the
/// effect happened).
pub fn trip(m: &mut Mover, env: &ItemEnv<'_>, now: GameMs, fx: &mut Fx) -> bool {
    let cx = BuffCx::new(*env, now);
    if has_item(m, II_SYS_SYS_SCR_RETURN) {
        remove_fake(
            m,
            &cx,
            BuffKey::new(BuffKind::Item, II_SYS_SYS_SCR_RETURN),
            fx,
        );
    }
    let Some(p) = m.player() else { return true };
    let (world, pos) = (p.misc.return_world, p.misc.return_pos);
    if pos == Vec3::ZERO {
        return true;
    }
    fx.then(Followup::Teleport {
        mover: m.id(),
        world,
        layer: LayerId(0),
        pos,
        kind: ReplaceKind::Normal,
    });
    true
}

/// `ReturnScrollReq{-1}`: only with the buff running, a virtual use of the scroll (its cast first).
pub fn on_trip_request(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let has = ctx
        .worlds
        .player(who.0)
        .is_some_and(|m| has_item(m, II_SYS_SYS_SCR_RETURN));
    if has {
        crate::item::usage::blink::run_virtual(svc, ctx, who, II_SYS_SYS_SCR_RETURN);
    }
}
