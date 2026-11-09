//! What a joining player brings into the world besides its rows (Plan/16 §13.6, §14.3; Plan/17 §7.9): the
//! stats of what it wears, of the premium items that run and of the buffs it carries, applied **before** the
//! spawn so that the spawn snapshot (`MoverCommon.dst_params`) already holds them.
//!
//! The order is the legacy one: `RedoEquip(FALSE)` (equipment, sets), `OnApplySM`, then the first process of
//! the buffs. No message of this part reaches a client (the spawn carries the result); the follow-ups of the
//! buff pass (a `BUFF_TOGIFT` that ran out offline) are run after the join batch is written.

use super::env::refresh_sim_params;
use super::equip::redo_equip;
use super::out::Out;
use crate::buff::core::BuffCx;
use crate::buff::persist::apply_at_join as buffs_at_join;
use crate::ctx::{GameCtx, Services};
use crate::item::fx::Fx;
use crate::mover::Mover;
use flyff_common::config::EventId;

/// Applies the loaded state of `m` (see the module documentation) and returns the effects of the buff pass.
/// `m` is not in a world yet.
pub fn apply_loaded(svc: &Services, ctx: &GameCtx<'_>, m: &mut Mover) -> Fx {
    let items = svc.items.handle();
    let env = items.env(ctx.clock.unix_ms.to_unix_time());
    let mut out = Out::new();
    redo_equip(m, &env, false, &mut out);
    // 13 §17.4: a summoned slot that holds no pet item is cleared (with its stale `BUFF_PET`)
    crate::item::pets::system::on_player_load(m, env.data());
    super::sm::apply_at_join(m, env.stats());
    let cx = BuffCx::new(env, ctx.clock.game_ms);
    let fx = buffs_at_join(m, &cx, ctx.live.event_state(EventId::Pk) != 0);
    refresh_sim_params(m, env.stats());
    fx
}
