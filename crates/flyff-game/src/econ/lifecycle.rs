//! The life cycle of the player-to-player economy (Plan/19 §8.4, §9.2): the per-tick pass that ends trades and
//! stalls when a trigger holds, and the logout cleanup.
//!
//! Death, teleport and world change are noticed by `EconState::end_shop_session` (they run where there is no
//! `Services`: `combat::death::do_die`, the `_replace` of the world phase) and **flagged**; disconnects and kicks
//! unbind the session; pickups and other inventory changes move `content_rev`. [`process`] runs right after the
//! worlds (tick phase 3.8, before the snapshot pass) and acts on all of it in the same tick:
//!
//! * [`trade::process`]: expired requests, the §8.4 triggers of every running trade;
//! * [`vendor::process`]: stalls whose owner died, teleported, lost the socket or vanished;
//!   visitors that left; vanished room members.
//!
//! [`cleanup_player`] is the same cleanup for one player at once; [`EconLogout`] runs it in the logout step
//! `LeaveChatRoom` of `world::logout::destroy_player` (the original's `OnTradeRemoveUser`, `ClosePVendor` and the
//! chat-room leave of `CUser::~CUser`, src:WORLDSERVER/User.cpp:4154-4169), which also covers an avatar that is
//! removed in the same tick its socket dropped.

use super::{trade, vendor};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::world::logout::{LogoutHooks, LogoutStep};
use flyff_common::WorldId;
use flyff_common::math::Vec3;

/// The economy pass of the tick: trades first (their reservations), then the stalls.
pub fn process(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    trade::process(svc, ctx);
    vendor::process(svc, ctx);
}

/// Ends everything the economy holds for `who`: its trade and trade requests, its stall (with the room), the
/// visit of somebody else's stall and the room membership. Idempotent.
pub fn cleanup_player(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    trade::cancel_for_player(svc, ctx, who.0, 0);
    svc.econ.trade.drop_requests_of(who.0);
    vendor::close_vendor(svc, ctx, who.0);
    vendor::leave_browse(svc, ctx, who.0, None);
    svc.econ.vendor.rooms.on_member_leave(ctx, who);
}

/// The logout hooks of the economy (chain it with [`EconLogout::with_inner`] behind the other documents').
#[derive(Default)]
pub struct EconLogout {
    inner: Option<Box<dyn LogoutHooks>>,
}

impl EconLogout {
    /// The economy's hooks alone.
    pub fn new() -> EconLogout {
        EconLogout { inner: None }
    }

    /// The same, forwarding every step and predicate to `inner` too.
    pub fn with_inner(inner: Box<dyn LogoutHooks>) -> EconLogout {
        EconLogout { inner: Some(inner) }
    }
}

impl std::fmt::Debug for EconLogout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EconLogout")
            .field("inner", &self.inner.is_some())
            .finish()
    }
}

impl LogoutHooks for EconLogout {
    fn step(
        &mut self,
        step: LogoutStep,
        svc: &mut Services,
        ctx: &mut GameCtx<'_>,
        player: PlayerRef,
    ) {
        if step == LogoutStep::LeaveChatRoom {
            cleanup_player(svc, ctx, player);
        }
        if let Some(i) = self.inner.as_mut() {
            i.step(step, svc, ctx, player);
        }
    }

    fn secret_room_position(&self, ctx: &GameCtx<'_>, player: PlayerRef) -> Option<Vec3> {
        self.inner
            .as_ref()
            .and_then(|i| i.secret_room_position(ctx, player))
    }

    fn is_instance_world(&self, world: WorldId) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|i| i.is_instance_world(world))
    }

    fn is_chaotic(&self, ctx: &GameCtx<'_>, player: PlayerRef) -> bool {
        match self.inner.as_ref() {
            Some(i) => i.is_chaotic(ctx, player),
            None => ctx
                .player(player)
                .and_then(|m| m.player())
                .is_some_and(|p| p.pk.propensity > 0),
        }
    }
}
