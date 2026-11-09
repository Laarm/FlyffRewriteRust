//! Pockets, the inventory sort and the equipment inspection (Plan/19 §3.4, §3.5, §5.13.5; WP-ITEM-15; ref/13
//! §6.5, §6.6; ref/07c §16, §30.1; ref/07b §9.2).
//!
//! | module | content |
//! |---|---|
//! | [`pocket`] | `MoveItemPocket`, `AvailPocket`, the availability rule of both `features.perm_extrabags` values, the once-per-second expiry pass of the non-permanent mode, the `Pocket*` messages |
//! | [`sort`] | `SortInventory`: a stable sort by `item_kind3` with one `MoveItemNtf` per swap (PL-153) |
//! | [`inspect`] | `QueryEquipReq`: VR-051 (the target is in the actor's view set) and `EQUIP_DENIAL_MODE` |
//!
//! Every handler resolves the actor from the session (VR-001) and validates what the message names against the
//! actor's own containers; an unusable request is ignored and changes nothing (VR-010, VR-025).

pub mod inspect;
pub mod pocket;
pub mod sort;

pub use inspect::on_query_equip;
pub use pocket::{
    MAX_POCKET, MoveOutcome, MoveReq, on_avail_pocket, on_move_item_pocket, process_expiring_all,
};
pub use sort::on_sort_inventory;

use super::env::ItemEnv;
use super::fx::Fx;
use super::ops::CtxSerials;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::mover::Mover;

/// Runs `f` with the player's mover, the item environment, the game's serial source and a fresh [`Fx`], then
/// flushes the effects. `None` when `who` is not a live player.
pub(crate) fn with_player<R>(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    f: impl FnOnce(&mut Mover, &ItemEnv<'_>, &mut CtxSerials<'_>, &mut Fx) -> R,
) -> Option<R> {
    let core = svc.items.handle();
    let env = core.env(ctx.clock.unix_ms.to_unix_time());
    let mut fx = Fx::new();
    let r = {
        let mut serials = CtxSerials::new(&mut svc.serials, ctx.persist, ctx.tickets);
        let m = ctx.worlds.player_mut(who.0)?;
        f(m, &env, &mut serials, &mut fx)
    };
    fx.flush(svc, ctx, who);
    Some(r)
}
