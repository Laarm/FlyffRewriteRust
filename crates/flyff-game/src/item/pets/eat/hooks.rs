//! The `GameCtx` face of the eat pets (Plan/19 §13.5, WP-ITEM-18): the summon with the services, the dismissal,
//! the per-tick job (the pet left behind, the re-summon, the 16-tick pass of the vis pet), the death hook, the
//! logout step and the client handlers of the vis sockets and the pet filter.

use super::{inactivate, on_summoned, summoned};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::container::StorageId;
use crate::item::fx::Fx;
use crate::item::model::Item;
use crate::item::pets::system::hooks::with_player;
use crate::world::logout::{LogoutHooks, LogoutStep};
use flyff_common::defs::text::TID_GAME_CANNOT_CALL_PET_ON_FLYING;
use flyff_common::math::Vec3;
use flyff_common::{ObjId, WorldId};
use flyff_proto::pb;

/// `IsValidArea(pet, 32)` of the owner: the same world and layer and strictly closer than
/// [`OWNER_AREA`](crate::ai::pet::OWNER_AREA).
fn in_area(ctx: &GameCtx<'_>, owner: ObjId, pet: ObjId) -> bool {
    let (Some(o), Some(p)) = (ctx.worlds.mover(owner), ctx.worlds.mover(pet)) else {
        return false;
    };
    if p.core.deleted || o.core.world != p.core.world || o.core.layer != p.core.layer {
        return false;
    }
    let d = o.sim.pos - p.sim.pos;
    d.x * d.x + d.y * d.y + d.z * d.z < crate::ai::pet::OWNER_AREA * crate::ai::pet::OWNER_AREA
}

/// `ActivateEatPet(item)` with the services: the pet mover is created at the owner's feet and the owner's state
/// finished ([`on_summoned`]). `None` when nothing happened (the owner is gone, flies, already has a pet, the
/// item is not a usable eat pet or its mover prop is unknown).
pub fn activate(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    item: StorageId,
) -> Option<ObjId> {
    let data = std::sync::Arc::clone(ctx.data);
    let (link_kind, sfx, world, flying) = {
        let m = ctx.worlds.player(who.0)?;
        let p = m.player()?;
        if p.items.summoned_eat_pet.is_some() {
            return None;
        }
        let it = p.items.inventory.get_at_id(item)?;
        let prop = data.item(it.item_id)?;
        if !Item::is_eat_pet(prop) || !it.is_usable() || it.is_expired() {
            return None;
        }
        let sfx = if it.is_vis_pet(prop) {
            super::super::vis::sfx_id(&data, &it.piercing.cards)
        } else {
            0
        };
        (prop.link_kind, sfx, m.core.world, m.sim.act.flags.fly())
    };
    if flying {
        with_player(svc, ctx, who, |_, _, _, fx| {
            fx.text(TID_GAME_CANNOT_CALL_PET_ON_FLYING);
        });
        return None;
    }
    let pet = crate::ai::pet::spawn(
        ctx.worlds,
        &data,
        ctx.rng.world(world),
        who.0,
        link_kind,
        sfx,
    )?;
    with_player(svc, ctx, who, |m, cx, _, fx| {
        on_summoned(m, cx, item, pet, fx);
    });
    Some(pet)
}

/// `InactivateEatPet` with the services: the owner's effects go and the pet mover is deleted. `true` when a pet
/// was summoned.
pub fn dismiss(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) -> bool {
    let pet = with_player(svc, ctx, who, |m, cx, _, fx| inactivate(m, cx, fx)).flatten();
    match pet {
        Some(pet) => {
            ctx.worlds.delete(pet);
            true
        }
        None => false,
    }
}

/// `ChangeVisPetSfx` ([`Op::Sfx`](super::Op::Sfx)): the effect of the pet mover becomes `sfx` and the viewers of the pet learn it.
pub fn change_sfx(svc: &mut Services, ctx: &mut GameCtx<'_>, player: ObjId, sfx: u32) {
    let Some((_, pet)) = ctx.worlds.player(player).and_then(summoned) else {
        return;
    };
    let changed = ctx
        .worlds
        .mover_mut(pet)
        .and_then(|m| m.monster_mut())
        .and_then(|e| e.ai.pet_mut())
        .is_some_and(|b| {
            let changed = b.sfx != sfx;
            b.sfx = sfx;
            changed
        });
    if changed {
        let mut fx = Fx::new();
        fx.out.near(pb::MoverChangeSfx { sfx_id: sfx });
        fx.flush_for(svc, ctx, pet);
    }
}

/// `CMover::DoDie` step 5 for the eat pet (`__REACTIVATE_EATPET`): a player killed by an NPC dismisses his pet. A
/// player killed by another player is covered by the pet's own AI (it sees the dead owner).
pub fn on_owner_killed(svc: &mut Services, ctx: &mut GameCtx<'_>, victim: ObjId) {
    if svc.items.handle().cfg.reactivate_eatpet {
        dismiss(svc, ctx, PlayerRef(victim));
    }
}

/// The per-tick job (`item::tick::process` calls it), in the order of the original's `CUser::Process`:
///
/// 1. the pet saved by a dismissal that wants it back (`SavedEatPetId`, [`PlayerItems::saved_eat_pet`](crate::item::PlayerItems))
///    is summoned again, if its item is still a usable eat pet and the owner lives;
/// 2. `features.reactivate_eatpet`: a pet farther than 32 m, in another world or layer, is dismissed and summoned
///    again at the owner; a pet whose mover is gone is dismissed (its effects would leak);
/// 3. every 16 ticks (`(tick + id) % 16 == 0`) the expired charms of a vis pet are removed (`ProcessVisPet`).
pub fn tick(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    let reactivate = svc.items.handle().cfg.reactivate_eatpet;
    let tick = ctx.clock.tick.0;
    let due: Vec<ObjId> = ctx
        .sessions
        .iter()
        .map(|e| e.player)
        .filter(|&id| {
            ctx.worlds
                .player(id)
                .and_then(|m| m.player())
                .is_some_and(|p| {
                    p.items.summoned_eat_pet.is_some() || p.items.saved_eat_pet.is_some()
                })
        })
        .collect();
    for id in due {
        let who = PlayerRef(id);
        // 1. the saved pet
        let saved = ctx
            .worlds
            .player_mut(id)
            .and_then(|m| m.player_mut())
            .and_then(|p| p.items.saved_eat_pet.take());
        if let Some(item) = saved {
            resummon(svc, ctx, who, item);
        }
        // 2. the pet left behind
        if let Some((item, pet)) = ctx.worlds.player(id).and_then(summoned) {
            let alive = ctx.worlds.mover(pet).is_some_and(|m| !m.core.deleted);
            if !alive {
                dismiss(svc, ctx, who);
            } else if reactivate && !in_area(ctx, id, pet) {
                dismiss(svc, ctx, who);
                resummon(svc, ctx, who, item);
            }
        }
        // 3. the vis pet
        if (tick.wrapping_add(u64::from(id.0))) % 16 == 0 {
            with_player(svc, ctx, who, |m, cx, _, fx| {
                super::super::vis::process(m, cx, fx);
            });
        }
    }
}

/// `DoUseItem(item)` after a teleport: the eat pet comes back if it can.
fn resummon(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, item: StorageId) {
    let alive = ctx
        .worlds
        .player(who.0)
        .is_some_and(|m| !(m.sim.act.state.is_die() || m.mv.hp == 0));
    if alive {
        let _ = activate(svc, ctx, who, item);
    }
}

// ---- the client handlers -----------------------------------------------------------------------------------------

/// `VisPetRemoveVis{pos}` (`CDPSrvr::OnRemoveVis`, VR-025).
pub fn on_vis_pet_remove_vis(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::VisPetRemoveVis,
) {
    with_player(svc, ctx, who, |mv, cx, _, fx| {
        super::super::vis::remove_vis(mv, cx, m.pos, false, fx);
    });
}

/// `VisPetSwapVis{pos1, pos2}` (`CDPSrvr::OnSwapVis`).
pub fn on_vis_pet_swap_vis(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::VisPetSwapVis,
) {
    with_player(svc, ctx, who, |mv, cx, _, fx| {
        super::super::vis::swap_vis(mv, cx, m.pos1, m.pos2, fx);
    });
}

// ---- logout ------------------------------------------------------------------------------------------------------

/// The logout hooks of the eat pets: the `DeactivateEatPet` step of `User.cpp:4045`; every other step and
/// predicate goes to the `inner` hooks.
#[derive(Default)]
pub struct EatPetLogout {
    inner: Option<Box<dyn LogoutHooks>>,
}

impl EatPetLogout {
    /// The hooks of this document alone.
    pub fn new() -> EatPetLogout {
        EatPetLogout { inner: None }
    }

    /// The same, forwarding everything else to `inner`.
    pub fn with_inner(inner: Box<dyn LogoutHooks>) -> EatPetLogout {
        EatPetLogout { inner: Some(inner) }
    }
}

impl std::fmt::Debug for EatPetLogout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EatPetLogout")
            .field("inner", &self.inner.is_some())
            .finish()
    }
}

impl LogoutHooks for EatPetLogout {
    fn step(&mut self, step: LogoutStep, svc: &mut Services, ctx: &mut GameCtx<'_>, p: PlayerRef) {
        if step == LogoutStep::DeactivateEatPet {
            dismiss(svc, ctx, p);
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
