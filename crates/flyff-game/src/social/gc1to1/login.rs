//! The login state and the logout rule of the 1:1 guild combat (Plan/21 §3.5, §8; `DPDatabaseClient.cpp:1180-1192`,
//! `User.cpp:4283-4295`).
//!
//! * [`Gc1to1LoginStep`] writes the manager's `Gc1to1NowState` of the joining player at [`JoinStep::Gc1to1NowState`] (also
//!   in the re-attach replay): with the flag on and only for members of a tendering guild (Open `{1, remaining}`, Entrance a
//!   line-up member `{2, remaining, members, enemy}` or a master `{3, 0}`, War `{3, 0}`, Close `{0, 0}`). With the default
//!   `events.guild_combat_1to1 = false` the join batch is exactly the one before this task.
//! * [`Gc1to1Logout`] answers [`LogoutStep::Gc1to1Check`] (already gated by the flag in `world::logout::remove_player`):
//!   see [`super::arena::on_logout`].

use super::manager_state_msg;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::outbox::Outbox;
use crate::world::join::{JoinContributor, JoinCtx, JoinStep, gc1to1_enabled};
use crate::world::logout::{LogoutHooks, LogoutStep};
use flyff_common::WorldId;

/// The `Gc1to1NowState` of the join batch.
pub struct Gc1to1LoginStep;

impl JoinContributor for Gc1to1LoginStep {
    fn step(&self) -> JoinStep {
        JoinStep::Gc1to1NowState
    }

    fn write(&self, cx: &JoinCtx<'_>, out: &mut Outbox) {
        if !gc1to1_enabled(cx.live) {
            return;
        }
        let Some(c) = cx
            .worlds
            .player(cx.player)
            .and_then(|m| m.player())
            .map(|p| p.ident.character_id)
        else {
            return;
        };
        if let Some(msg) = manager_state_msg(cx.svc, c, cx.clock.game_ms) {
            out.to_session(cx.session, 0, msg);
        }
    }
}

/// The logout hook of the 1:1 guild combat: a fighter who leaves during his round loses it. Every other step and
/// predicate is forwarded to `inner` (the same chain shape as `SiegeLogout`).
#[derive(Default)]
pub struct Gc1to1Logout {
    inner: Option<Box<dyn LogoutHooks>>,
}

impl Gc1to1Logout {
    /// The hook alone.
    pub fn new() -> Gc1to1Logout {
        Gc1to1Logout { inner: None }
    }

    /// The same, forwarding to `inner` too.
    pub fn with_inner(inner: Box<dyn LogoutHooks>) -> Gc1to1Logout {
        Gc1to1Logout { inner: Some(inner) }
    }
}

impl std::fmt::Debug for Gc1to1Logout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Gc1to1Logout")
            .field("inner", &self.inner.is_some())
            .finish()
    }
}

impl LogoutHooks for Gc1to1Logout {
    fn step(
        &mut self,
        step: LogoutStep,
        svc: &mut Services,
        ctx: &mut GameCtx<'_>,
        player: PlayerRef,
    ) {
        if step == LogoutStep::Gc1to1Check {
            super::arena::on_logout(svc, ctx, player);
        }
        if let Some(i) = self.inner.as_mut() {
            i.step(step, svc, ctx, player);
        }
    }

    fn secret_room_position(
        &self,
        ctx: &GameCtx<'_>,
        player: PlayerRef,
    ) -> Option<flyff_common::math::Vec3> {
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
