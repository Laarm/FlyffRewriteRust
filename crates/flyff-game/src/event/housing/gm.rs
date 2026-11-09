//! The seam of gm-content-control (WP-GM-26): `/hgmra` (`as <owner> /hgmra`, row 209) calls [`gm_remove_all`], and
//! `/content housing [<name>]` prints [`room_line`] or [`state_line`] as chat lines (the BFIX1 list rule). The two line
//! builders only read.
//!
//! The `<key>: <State>[, <counts>]` shape of the state line is the one tests-m7 polls. Housing has no event flag (it is
//! always on), so the state is always `On`.

use super::room::players_in_room;
use super::{ROOM_WORLD, remove_entry, room_layer};
use crate::ctx::{GameCtx, Services};
use crate::world::LayerState;
use flyff_common::CharacterId;

/// `ProcessExpiredFurniture(now, bGMRemove = TRUE)` for `owner`'s loaded room (`/HousingGMRemoveAll`, own list): every
/// non-pending entry is removed through the same path as the expiry (an installed piece comes down without a
/// `HousingSetupFurnitureNtf`). Returns how many were removed; no loaded room gives 0 and sends nothing.
pub fn gm_remove_all(svc: &mut Services, ctx: &mut GameCtx<'_>, owner: CharacterId) -> usize {
    let items: Vec<u32> = svc.event.housing.room(owner).map_or_else(Vec::new, |r| {
        r.items
            .iter()
            .filter(|i| !i.pending)
            .map(|i| i.item)
            .collect()
    });
    items
        .into_iter()
        .filter(|&item| remove_entry(svc, ctx, owner, item))
        .count()
}

/// The name of a character for the lines (the directory's, else the id).
fn name_of(svc: &Services, c: CharacterId) -> String {
    svc.presence
        .get(c)
        .map_or_else(|| format!("#{}", c.0), |e| e.name.clone())
}

/// `housing <name>: <n> items (<i> installed, <p> pending), visitors <v>, layer live|draining|none, <k> inside` for a
/// loaded room, `housing <name>: not loaded (offline)` otherwise.
pub fn room_line(svc: &Services, ctx: &GameCtx<'_>, owner: CharacterId) -> String {
    let name = name_of(svc, owner);
    let Some(room) = svc.event.housing.room(owner) else {
        return format!("housing {name}: not loaded (offline)");
    };
    let layer = room_layer(owner);
    let state = match layer.and_then(|l| ctx.worlds.layer(ROOM_WORLD, l)) {
        Some(l) if l.state == LayerState::Live => "live",
        Some(_) => "draining",
        None => "none",
    };
    let inside = layer.map_or(0, |l| players_in_room(ctx, l).len());
    format!(
        "housing {name}: {} items ({} installed, {} pending), visitors {}, layer {state}, {inside} inside",
        room.items.len(),
        room.installed_count(),
        room.pending_count(),
        room.visitors.len()
    )
}

/// `housing: On, <r> rooms, <n> items (<i> installed), <k> inside`: the totals over the loaded rooms (`inside` counts
/// the players standing in any of their layers).
pub fn state_line(svc: &Services, ctx: &GameCtx<'_>) -> String {
    let (mut items, mut installed, mut inside) = (0_usize, 0_usize, 0_usize);
    for r in svc.event.housing.rooms() {
        items += r.items.len();
        installed += r.installed_count();
        if let Some(l) = room_layer(r.owner) {
            inside += players_in_room(ctx, l).len();
        }
    }
    format!(
        "housing: On, {} rooms, {items} items ({installed} installed), {inside} inside",
        svc.event.housing.len()
    )
}
