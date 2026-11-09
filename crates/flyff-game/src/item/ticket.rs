//! Tickets and layer passes (Plan/19 §15, WP-ITEM-24; ref/13 §21; `User.cpp` 417-441, 3404-3497, 8594-8614;
//! `Mover.cpp` 11629-11653, 12036-12062; `Item.cpp` 792-800).
//!
//! A ticket (`IK3_TICKET`, `ticket.inc`: Kebaras and Cisland) is a permanent item with a keep time: the first use
//! starts the time (`dwAbilityMin` minutes), applies the ticket buff and teleports the user to the ticket world and
//! the layer he typed (`DoUseItemInput`, the layer key "0", "-1", ... of the client's layer window). Using the same
//! ticket again leaves (back to Madrigal). The ticket is never consumed.
//!
//! * [`use_ticket`]: `CUser::DoUseItemTicket`, reached only through `DoUseItemInput` (a plain `DoUseItem` of a
//!   ticket does nothing, `__AZRIA_1023`). Every branch answers `CONSUME`, like `DoUseItemInput`'s `return 0`.
//! * [`tick`]: the 16-tick pass of `CUser::Process`: inside a ticket world without a running ticket the buff goes and
//!   the player is forced to Madrigal; outside a ticket world the buff goes (the keep time keeps running); the
//!   "in use" mark ([`PlayerItems::active_ticket`](super::PlayerItems)) follows [`get_ticket`].
//! * [`get_ticket`] / [`find_active_ticket`]: `GetTicket` and `FindActiveTicket`.
//!
//! **Fixes** (against the original): every check of the use runs BEFORE the keep time starts (the original started it
//! and then dropped an invalid layer); `IsActiveTicket` without a table row is `false` (the original dereferenced
//! null). **Deviation**: the "in use" mark is set by the use and refreshed by the 16-tick pass, so it can lag a lost
//! buff by up to 15 ticks (the original's `IsUsing` evaluated `GetTicket()` at every check).

use super::container::StorageId;
use super::env::{ItemConfig, ItemEnv};
use super::fx::Fx;
use super::model::Item;
use super::ops::{UiParam, core as ops};
use super::pockets::MAX_POCKET;
use super::pockets::pocket::is_available;
use super::usage::RowCx;
use super::usage::system::{CONSUME, Sys};
use crate::buff::{BuffCx, BuffKey, BuffKind};
use crate::ctx::{GameCtx, Services};
use crate::follow::Followup;
use crate::item::buffitems::Later;
use crate::mover::Mover;
use crate::world::ReplaceKind;
use crate::world::revival::MADRIGAL_START;
use flyff_common::defs::itemkind::IK3_TICKET;
use flyff_common::defs::text::{
    TID_GAME_LIMITED_USE, TID_GAME_MUST_STOP_OTHER_TICKET, TID_GAME_RAINBOWRACE_NOTELEPORT,
};
use flyff_common::defs::world::WI_WORLD_MADRIGAL;
use flyff_common::math::Vec3;
use flyff_common::{LayerId, ObjId, WorldId};
use flyff_data::GameData;
use flyff_data::props::item::ItemProp;

/// The pass runs when `(tick + obj id) & PASS_MASK == 0` (`m_nCount & 15`, the pattern of the pet pass).
const PASS_MASK: u64 = 15;

/// `CMover::GetTicket`: the inventory item (ascending storage id; the pockets are not searched, KEEP) that belongs
/// to the running ticket buff: its item id is the buff's id, its keep time started, it is a ticket and not expired.
/// `None` without a ticket buff.
pub fn get_ticket(m: &Mover, data: &GameData) -> Option<StorageId> {
    let key = crate::buff::core::find_ik3(m, data, IK3_TICKET)?;
    let item_id = u32::from(key.id);
    m.player()?
        .items
        .inventory
        .iter()
        .find(|(_, it)| {
            it.keep_until > 0
                && it.item_id == item_id
                && !it.is_expired()
                && is_ticket_item(data, it)
        })
        .map(|(id, _)| id)
}

fn is_ticket_item(data: &GameData, it: &Item) -> bool {
    data.item(it.item_id)
        .is_some_and(|p| p.item_kind3 == IK3_TICKET)
}

/// `CItemElem::IsActiveTicket(item_id)`: a started, unexpired ticket that opens the same world as `item_id`. A missing
/// `ticket.inc` row is `false` (FIX).
fn is_active_ticket(data: &GameData, it: &Item, item_id: u32) -> bool {
    if it.is_expired() || it.keep_until <= 0 || !is_ticket_item(data, it) {
        return false;
    }
    match (
        data.tickets.ticket(it.item_id),
        data.tickets.ticket(item_id),
    ) {
        (Some(a), Some(b)) => a.world == b.world,
        _ => false,
    }
}

/// `CUser::FindActiveTicket(item_id)`: the first started ticket of the same world as `item_id`, first in the
/// inventory (ascending storage id), then in the available pockets `0..MAX_POCKET`.
pub fn find_active_ticket(
    m: &Mover,
    data: &GameData,
    cfg: &ItemConfig,
    item_id: u32,
) -> Option<StorageId> {
    let p = m.player()?;
    if let Some((id, _)) = p
        .items
        .inventory
        .iter()
        .find(|(_, it)| is_active_ticket(data, it, item_id))
    {
        return Some(id);
    }
    for (i, pocket) in p.items.pockets.iter().enumerate().take(MAX_POCKET) {
        if !is_available(cfg, i, pocket) {
            continue;
        }
        if let Some((id, _)) = pocket
            .items
            .iter()
            .find(|(_, it)| is_active_ticket(data, it, item_id))
        {
            return Some(id);
        }
    }
    None
}

/// Madrigal's start: where leaving a ticket world and an expired ticket put the player (layer 0, forced).
fn to_madrigal(m: &Mover) -> Followup {
    Followup::Teleport {
        mover: m.id(),
        world: WorldId(WI_WORLD_MADRIGAL),
        layer: LayerId(0),
        pos: MADRIGAL_START,
        kind: ReplaceKind::Force,
    }
}

/// `CUser::DoUseItemTicket` for the inventory item `ux.id` (reached through `DoUseItemInput`; `input` is the text:
/// the layer key of the client's layer window). Always `CONSUME`: a permanent item loses nothing and the after-use
/// step runs (KEEP).
pub fn use_ticket(
    m: &mut Mover,
    env: &ItemEnv<'_>,
    prop: &ItemProp,
    input: &str,
    ux: &RowCx<'_>,
    fx: &mut Fx,
) -> Sys {
    // `g_GuildCombat1to1Mng.IsPossibleUser`: silent
    if ux.facts.gc1to1 {
        return CONSUME;
    }
    let data = env.data();
    let item_id = prop.id;
    let keep_until = m
        .player()
        .and_then(|p| p.items.inventory.get_at_id(ux.id))
        .map_or(0, |it| it.keep_until);
    if crate::buff::core::find_ik3(m, data, IK3_TICKET).is_some() {
        // a ticket buff is running: this ticket leaves, any other must be stopped first
        let key = BuffKey::new(BuffKind::Item, item_id);
        if m.buffs.get(key).is_none() {
            fx.text(TID_GAME_MUST_STOP_OTHER_TICKET);
            return CONSUME;
        }
        if keep_until == 0 {
            fx.text(TID_GAME_LIMITED_USE);
            return CONSUME;
        }
        // `RemoveBuff(BUFF_ITEM, id)`: the fake removal
        crate::buff::core::remove_fake(m, &BuffCx::new(*env, ux.now), key, fx);
        if let Some(p) = m.player_mut() {
            p.items.active_ticket = None;
        }
        fx.then(to_madrigal(m));
        return CONSUME;
    }
    // `CRainbowRaceMng::IsEntry`
    if ux.facts.rainbow_entrant {
        fx.text(TID_GAME_RAINBOWRACE_NOTELEPORT);
        return CONSUME;
    }
    let Some(ticket) = data.tickets.ticket(item_id).copied() else {
        return CONSUME;
    };
    // `::atoi( GetInput() )`: leading spaces, sign, digits, anything else ends it; the layer must be one of the
    // ticket layers `-expand ..= 0` (FIX: checked before the keep time starts)
    let layer = flyff_script::cnum::c_atoi(input.as_bytes());
    let expand = data.tickets.expanded_layer(ticket.world);
    if layer > 0 || i64::from(layer) < -i64::from(expand) {
        return CONSUME;
    }
    if keep_until == 0 {
        if find_active_ticket(m, data, env.cfg(), item_id).is_some() {
            fx.text(TID_GAME_LIMITED_USE);
            return CONSUME;
        }
        // `UpdateItem(.., UI_KEEPTIME, dwAbilityMin)`: minutes
        ops::update_item(m, env, ux.id, UiParam::KeepTime, prop.ability_min, 0, fx);
    }
    // `DoApplySkill( this, pItemProp, NULL )`
    fx.later(Later::ApplyItem {
        target: m.id(),
        item: item_id,
    });
    if let Some(p) = m.player_mut() {
        p.items.active_ticket = Some(ux.id);
    }
    // a flying user keeps the buff and stays (the refused normal replace; the next pass strips the buff)
    fx.then(Followup::Teleport {
        mover: m.id(),
        world: WorldId(ticket.world),
        layer: LayerId(layer),
        pos: Vec3::new(ticket.pos[0], ticket.pos[1], ticket.pos[2]),
        kind: ReplaceKind::Normal,
    });
    CONSUME
}

/// The 16-tick ticket pass of `CUser::Process`, called by `item::tick::process` right after the keep-time pass (an
/// item flagged expired this tick is seen by the same tick's pass).
pub fn tick(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    let tick = ctx.clock.tick.0;
    let due: Vec<ObjId> = ctx
        .sessions
        .iter()
        .map(|e| e.player)
        .filter(|id| tick.wrapping_add(u64::from(id.0)) & PASS_MASK == 0)
        .collect();
    let data = std::sync::Arc::clone(ctx.data);
    for id in due {
        let Some(m) = ctx.worlds.player(id) else {
            continue;
        };
        let in_ticket_world = data.tickets.is_target(m.core.world.0);
        let ticket = get_ticket(m, &data);
        let has_buff = crate::buff::core::find_ik3(m, &data, IK3_TICKET).is_some();
        let mut running = ticket;
        if in_ticket_world {
            if ticket.is_none() {
                // no running ticket: the buff goes and the player is forced to Madrigal
                if has_buff {
                    crate::buff::api::remove_by_ik3(svc, ctx, id, IK3_TICKET);
                }
                if let Err(e) = ctx.worlds.replace(
                    id,
                    WorldId(WI_WORLD_MADRIGAL),
                    MADRIGAL_START,
                    LayerId(0),
                    ReplaceKind::Force,
                ) {
                    tracing::debug!(player = id.0, "ticket world exit refused: {e}");
                }
                running = None;
            }
        } else if has_buff {
            // outside a ticket world there is no ticket buff (the keep time keeps running)
            crate::buff::api::remove_by_ik3(svc, ctx, id, IK3_TICKET);
            running = None;
        }
        if let Some(p) = ctx.worlds.player_mut(id).and_then(|m| m.player_mut()) {
            p.items.active_ticket = running;
        }
    }
}
