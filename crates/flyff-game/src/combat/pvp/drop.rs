//! `CMover::DoDropItemRandom` (`Mover.cpp:2494-2570`): the item loss of a chaotic player killed with
//! `events.pk_cost` (Plan/17 §8.3; doc 19's `drop_random_items`).

use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::container::StorageId;
use crate::item::fx::Fx;
use crate::item::handlers::{equip_toggle, is_dropable};
use crate::item::loot::GroundItemMeta;
use crate::item::ops::{RemoveReason, core as ops};
use crate::world::ItemDrop;
use flyff_common::ObjId;
use flyff_common::defs::itemkind::IK3_QUEST;
use flyff_common::rng::GameRng;
use flyff_db::audit::ev;
use flyff_rules::combat::x_random;
use serde_json::json;

/// Drops one random item of `victim` where it stands and returns `true` (`false` when nothing was droppable).
///
/// The candidates are the bag items that are not quest items, charged, eat pets, in use, bound or (with
/// `only_equip`) not worn; `exclude_equip` drops the worn ones from the candidates. The pick is
/// `x_random(candidates)`; a worn pick is taken off first (a weapon with its own part), then the whole stack
/// falls to the ground (`DropItem(id, 0, pos, bPK = TRUE)`: the flight check is skipped, a trade or a stall
/// refuses). A pick that cannot fall leaves the candidate list and the next one is tried.
pub fn drop_random_item(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    victim: ObjId,
    exclude_equip: bool,
    only_equip: bool,
) -> bool {
    let who = PlayerRef(victim);
    let core = s.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    // the candidate list, in ascending object id (`for i in 0..GetMax(): GetAtId(i)`)
    let mut buf: Vec<StorageId> = {
        let Some(p) = ctx.worlds.player(victim).and_then(|m| m.player()) else {
            return false;
        };
        if p.econ.vending {
            return false;
        }
        let mut ids: Vec<(StorageId, bool)> = p
            .items
            .inventory
            .iter()
            .filter_map(|(id, item)| {
                let prop = env.data().item(item.item_id)?;
                if prop.item_kind3 == IK3_QUEST
                    || item.is_charged()
                    || crate::item::model::Item::is_eat_pet(prop)
                    || p.items.is_using(id)
                    || item.is_binds(prop, env.cfg().only_armor_awakes)
                {
                    return None;
                }
                let worn = p.items.inventory.is_equip(id);
                if only_equip {
                    worn.then_some((id, worn))
                } else if exclude_equip && worn {
                    None
                } else {
                    Some((id, worn))
                }
            })
            .collect();
        ids.sort_by_key(|(id, _)| *id);
        ids.into_iter().map(|(id, _)| id).collect()
    };
    let Some(world) = ctx.worlds.core(victim).map(|c| c.world) else {
        return false;
    };
    while !buf.is_empty() {
        let idx = (x_random(ctx.rng.world(world), buf.len() as u32) as usize).min(buf.len() - 1);
        let id = buf[idx];
        // a worn item comes off first (a weapon with the part it sits in)
        let worn = ctx
            .worlds
            .player(victim)
            .and_then(|m| m.player())
            .is_some_and(|p| p.items.inventory.is_equip(id));
        if worn {
            let part = ctx
                .worlds
                .player(victim)
                .and_then(|m| m.player())
                .and_then(|p| {
                    let inv = &p.items.inventory;
                    inv.slot_of(id)
                        .map(|slot| u32::from(slot) - u32::from(inv.index_num()))
                })
                .map_or(0, |part| part as i32);
            equip_toggle(s, ctx, who, id, part);
        }
        if drop_item(s, ctx, who, id) {
            return true;
        }
        buf.remove(idx);
    }
    false
}

/// `DropItem(id, 0, GetPos(), TRUE)`: the whole stack lands on the ground at the player's feet.
fn drop_item(s: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, id: StorageId) -> bool {
    let core = s.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    let snap = {
        let Some(m) = ctx.worlds.player(who.0) else {
            return false;
        };
        let Some(p) = m.player() else { return false };
        let Some(item) = p.items.inventory.get_at_id(id) else {
            return false;
        };
        let Some(prop) = env.data().item(item.item_id) else {
            return false;
        };
        // bPK: the flight check is skipped; a trade partner still refuses
        if is_dropable(m, &env, id, prop, false, p.econ.has_partner()).is_err() {
            return false;
        }
        (
            item.clone(),
            m.core.pos,
            m.core.world,
            m.core.layer,
            p.ident.character_id,
            prop.item_kind3,
        )
    };
    let (item, pos, world, layer, character, kind3) = snap;
    let angle = ctx.rng.world(world).random_below(360) as f32;
    let meta = GroundItemMeta {
        owner: None,
        holder: (kind3 == IK3_QUEST).then_some(character),
        drop_time: ctx.clock.game_ms,
        from_monster: false,
        event_monster: None,
        gold: None,
        respawned: false,
    };
    let mut dropped = item.clone();
    dropped.reserved = 0;
    let ground = ItemDrop::new(dropped.to_record(0), pos, angle, meta);
    if ctx.worlds.spawn_item(world, layer, ground).is_err() {
        return false;
    }
    let mut fx = Fx::new();
    if let Some(m) = ctx.worlds.player_mut(who.0) {
        ops::remove_item(m, &env, id, item.count, RemoveReason::Moved, &mut fx);
    }
    fx.audit(
        ev::ITEM_DROP,
        json!({ "item": crate::item::fx::item_json(&item), "why": "pk" }),
    );
    fx.flush(s, ctx, who);
    true
}
