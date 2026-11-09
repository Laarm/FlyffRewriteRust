//! The pet food mill (Plan/19 §13.4; ref/13 §19.5; src:WORLDSERVER/DPSrvr.cpp:10249-10313): gems are ground into
//! `II_SYS_SYS_FEED_01`, either with a feed maker tool (table 1, one tool piece per grinding) or at a pet food
//! mill NPC (table 0, within the NPC-menu range).

use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::container::StorageId;
use crate::item::fx::Fx;
use crate::item::model::Item;
use crate::item::ops::{self, ItemOrigin, RemoveReason};
use flyff_common::defs::item::{II_SYS_SYS_FEED_01, II_SYS_SYS_FEED_MAKER};
use flyff_common::defs::itemkind::IK3_GEM;
use flyff_common::defs::neuz::MMI_PET_FOODMILL;
use flyff_common::defs::text::{
    TID_GAME_PET_FEED_CHARGED, TID_GAME_PET_FEED_EQUIPED, TID_GAME_PET_IS_NOT_FEED,
};
use flyff_common::rng::GameRng;
use flyff_proto::pb;
use flyff_rules::consts::NULL_ID;
use flyff_rules::item::pet;

/// `MakePetFeed{material, tool, num}`. The tool, when named, must be a usable `II_SYS_SYS_FEED_MAKER` (else the
/// request is dropped); without a tool a pet food mill NPC must stand within the menu range of the player, in
/// the same world and layer (VR-012). The material is a usable, uncharged `IK3_GEM` that is not worn; `num` is
/// clamped to `1..=stack`. Each piece gives `GetFeedEnergy( cost, table )`; the grinding stops before the feed
/// would exceed the pack size of the feed (9 999) and only the pieces ground until then are used up. The feed
/// is created, the materials (and one tool piece) consumed, and `PetMill{result, count}` answers
/// (`result` 1 = created, 0 = no room).
pub fn on_make_pet_feed(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::MakePetFeed,
) {
    let core = svc.items.handle();
    let data = &*core.data;
    let Some(m) = ctx.player(who) else { return };
    let Some(p) = m.player() else { return };
    let world = m.core.world;
    let inv = &p.items.inventory;
    // the tool
    let tool: Option<StorageId> = if msg.tool_obj_id == NULL_ID {
        None
    } else {
        let Ok(id) = StorageId::try_from(msg.tool_obj_id) else {
            return;
        };
        match inv.get_at_id(id) {
            Some(it) if it.is_usable() && it.item_id == II_SYS_SYS_FEED_MAKER => Some(id),
            _ => return,
        }
    };
    if tool.is_none()
        && ctx
            .worlds
            .close_npc(MMI_PET_FOODMILL, m.core.world, m.core.layer, m.core.pos)
            .is_none()
    {
        return;
    }
    let Ok(mat_id) = StorageId::try_from(msg.material_obj_id) else {
        return;
    };
    let Some(mat) = inv.get_at_id(mat_id).filter(|it| it.is_usable()) else {
        return;
    };
    let Some(prop) = data.item(mat.item_id) else {
        return;
    };
    let mut fx = Fx::new();
    if mat.is_charged() {
        fx.text(TID_GAME_PET_FEED_CHARGED);
        fx.flush(svc, ctx, who);
        return;
    }
    if prop.item_kind3 != IK3_GEM {
        fx.text(TID_GAME_PET_IS_NOT_FEED);
        fx.flush(svc, ctx, who);
        return;
    }
    let count = u32::from(mat.count);
    let wanted = u32::try_from(msg.num).ok().filter(|&n| n > 0).unwrap_or(1);
    let wanted = wanted.min(count);
    if inv.is_equip(mat_id) {
        fx.text(TID_GAME_PET_FEED_EQUIPED);
        fx.flush(svc, ctx, who);
        return;
    }
    let Some(feed_prop) = data.item(II_SYS_SYS_FEED_01) else {
        return;
    };
    let table = usize::from(tool.is_some());
    let cost = prop.cost;
    let rng = ctx.rng.world(world);
    let (total, used) = pet::mill(wanted, feed_prop.pack_max, || {
        pet::feed_roll(data.pet.feed_energy_range(cost, table), |lo, hi| {
            rng.random_range(lo, hi)
        })
    });
    let created = total > 0
        && u16::try_from(total).is_ok_and(|n| {
            ops::create_item(
                svc,
                ctx,
                who,
                Item::new(II_SYS_SYS_FEED_01, n),
                ItemOrigin::PetFeed,
            )
            .is_ok()
        });
    if created {
        ops::remove_item(
            svc,
            ctx,
            who,
            mat_id,
            used as u16,
            RemoveReason::Other("pet_food_material"),
        );
        if let Some(tool) = tool {
            ops::remove_item(svc, ctx, who, tool, 1, RemoveReason::Other("pet_food_tool"));
        }
        svc.saves.mark_urgent(who.0);
    }
    let mut fx = Fx::new();
    fx.out.to_self(pb::PetMill {
        result: i32::from(created),
        count: i32::try_from(total).unwrap_or(i32::MAX),
    });
    fx.flush(svc, ctx, who);
}
