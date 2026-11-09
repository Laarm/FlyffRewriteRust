//! The logout link of the mini-games (`LogoutStep::CloseMiniGame`, src:WORLDSERVER/User.cpp:4040-4042; Plan/22 §16.4).
//!
//! A leaving player gets the stakes of an open five-system window back before the final snapshot (so the refund is in
//! the saved wallet), and the player's mini-game state is dropped. The link follows the `EatPetLogout` pattern: it
//! answers its own step and forwards every step and predicate to its `inner` hooks. The doc-22 hook object of the
//! other event cleanups is instances' `event::hooks::EventLogout` (README ruling 8); this link is the innermost one.

use super::on_logout;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::world::logout::{LogoutHooks, LogoutStep};
use flyff_common::WorldId;
use flyff_common::math::Vec3;

/// The mini-game part of the logout hooks.
#[derive(Default)]
pub struct MiniGameLogout {
    inner: Option<Box<dyn LogoutHooks>>,
}

impl MiniGameLogout {
    /// The hooks of this document alone.
    pub fn new() -> MiniGameLogout {
        MiniGameLogout { inner: None }
    }

    /// The same, forwarding everything else to `inner`.
    pub fn with_inner(inner: Box<dyn LogoutHooks>) -> MiniGameLogout {
        MiniGameLogout { inner: Some(inner) }
    }
}

impl std::fmt::Debug for MiniGameLogout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MiniGameLogout")
            .field("inner", &self.inner.is_some())
            .finish()
    }
}

impl LogoutHooks for MiniGameLogout {
    fn step(&mut self, step: LogoutStep, svc: &mut Services, ctx: &mut GameCtx<'_>, p: PlayerRef) {
        if step == LogoutStep::CloseMiniGame {
            on_logout(svc, ctx, p);
        }
        if let Some(inner) = self.inner.as_mut() {
            inner.step(step, svc, ctx, p);
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
        match &self.inner {
            Some(i) => i.is_chaotic(ctx, player),
            None => ctx
                .player(player)
                .and_then(|m| m.player())
                .is_some_and(|p| p.pk.propensity > 0),
        }
    }
}
