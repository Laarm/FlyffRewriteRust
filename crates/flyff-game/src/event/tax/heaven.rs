//! The Heaven Tower (Plan/22 §10.5; legacy `CDPSrvr::OnTeleportToHeavenTower`, `DPSrvr.cpp:11943-12010`).
//!
//! NOT an instance: the NPC `MaFl_HEAVENMAN` (menu `MMI_HEAVEN_TOWER` 97, only in `WdMadrigal`, in Flaris = East) sells
//! a one-way teleport into the ordinary shared worlds `WI_WORLD_HEAVEN01..05` (ids 21..25) on layer 0 with
//! `REPLACE_NORMAL`. No layer, no party, no cooldown, no ticket. The monsters, portals and the revival inside the
//! tower are the existing world flow; "progression" is buying the next floor at the NPC. It touches the tax only through
//! the admission fee.
//!
//! In order: a dead avatar is ignored (FIX: the C++ has no check, the client cannot open the menu while dead); the
//! NPC must stand within `gameplay.npc_menu_range` in the same world and layer (VR-012, PL-107); a floor outside 1..=5 is
//! ignored (debug log); too little penya answers `TID_GAME_LACKMONEY` (perins do not count). The continent is taken from
//! the CURRENT position, before the teleport. A refused `replace` (a flying player, ...) charges nothing and sends
//! nothing; otherwise, in the same call: the fee leaves the purse (`AddGold(-cost)` sends it), the angle is set, the
//! admission is booked when the player's own continent has an owner (KEEP: the lord's share rides on it, so a West booking
//! reaches only the lord; the shipped NPC stands in Flaris), the counters are flushed, the avatar is saved at once (the
//! world-change snapshot of the same tick carries the new purse), and the audit line `shop.buy{service: "heaven_tower"}`
//! is written.

use super::flush_due;
use crate::combat::state::is_die;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::econ::tax::{Continent, TaxHooks, TaxKind};
use crate::item::fx::Fx;
use crate::item::gold::core::add_gold;
use crate::world::ReplaceKind;
use flyff_common::defs::neuz::MMI_HEAVEN_TOWER;
use flyff_common::defs::text::TID_GAME_LACKMONEY;
use flyff_common::math::Vec3;
use flyff_common::{LayerId, WorldId};
use flyff_db::audit::ev;
use flyff_proto::pb;
use serde_json::json;

/// One floor of the tower.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeavenFloor {
    /// The admission fee in penya.
    pub cost: i32,
    /// `WI_WORLD_HEAVEN0n`.
    pub world: u32,
    /// Where the avatar lands.
    pub pos: Vec3,
    /// The angle in degrees.
    pub angle: f32,
}

/// The five floors (`DPSrvr.cpp:11959-11977`).
pub const HEAVEN_FLOORS: [HeavenFloor; 5] = [
    HeavenFloor {
        cost: 10_000,
        world: 21,
        pos: Vec3::new(253.0, 102.0, 78.0),
        angle: 183.0,
    },
    HeavenFloor {
        cost: 30_000,
        world: 22,
        pos: Vec3::new(251.0, 102.0, 95.0),
        angle: 183.0,
    },
    HeavenFloor {
        cost: 50_000,
        world: 23,
        pos: Vec3::new(264.0, 102.0, 227.0),
        angle: 183.0,
    },
    HeavenFloor {
        cost: 70_000,
        world: 24,
        pos: Vec3::new(253.0, 102.0, 86.0),
        angle: 174.0,
    },
    HeavenFloor {
        cost: 100_000,
        world: 25,
        pos: Vec3::new(218.0, 102.0, 101.0),
        angle: 176.0,
    },
];

/// The floor `n` (1..=5).
pub fn floor_of(n: i32) -> Option<&'static HeavenFloor> {
    usize::try_from(n)
        .ok()
        .and_then(|n| n.checked_sub(1))
        .and_then(|i| HEAVEN_FLOORS.get(i))
}

/// `HeavenTowerTeleport{floor}` (see the module documentation).
pub fn on_heaven_tower_teleport(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::HeavenTowerTeleport,
) {
    let Some(mv) = ctx.player(who) else { return };
    if is_die(mv) {
        return;
    }
    let (world, layer, pos) = (mv.core.world, mv.core.layer, mv.core.pos);
    if ctx
        .worlds
        .close_npc(MMI_HEAVEN_TOWER, world, layer, pos)
        .is_none()
    {
        return;
    }
    let Some(floor) = floor_of(m.floor) else {
        tracing::debug!(floor = m.floor, "HeavenTowerTeleport: not a floor");
        return;
    };
    let gold = mv.player().map_or(0, |p| p.items.gold);
    let mut fx = Fx::new();
    if gold < floor.cost {
        fx.defined(TID_GAME_LACKMONEY, Vec::new());
        fx.flush(s, ctx, who);
        return;
    }
    // the continent of the CURRENT position, before the teleport
    let cont: Continent = s.event.tax.continent_of(pos, world);
    if ctx
        .worlds
        .replace(
            who.0,
            WorldId(floor.world),
            floor.pos,
            LayerId(0),
            ReplaceKind::Normal,
        )
        .is_err()
    {
        return;
    }
    let core = s.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    let Some(mv) = ctx.worlds.player_mut(who.0) else {
        return;
    };
    add_gold(mv, &env, -i64::from(floor.cost), true, &mut fx);
    let angle = flyff_sim::num::set_angle_quantized(floor.angle);
    mv.sim.angle = angle;
    mv.core.angle = angle;
    if s.event.tax.owner(cont).is_some() {
        s.event
            .tax
            .add_tax(cont, i64::from(floor.cost), TaxKind::Admission);
    }
    flush_due(&mut s.event.tax, ctx);
    fx.audit(
        ev::SHOP_BUY,
        json!({
            "service": "heaven_tower",
            "floor": m.floor,
            "gold": floor.cost,
            "continent": cont,
        }),
    );
    fx.mark_urgent();
    fx.flush(s, ctx, who);
}
