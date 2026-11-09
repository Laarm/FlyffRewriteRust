//! The item system's side of the eat-pet AI (Plan/19 §6.8, §13.5; WP-ITEM-18): [`EatPetWorld`] implements
//! [`crate::ai::pet::PetWorld`] over the world phase's `FrameCtx` - the loot rights with the owner's pet filter, the
//! pickup on behalf of the owner with the multiple picker around the pet, and the dismissal of a pet whose owner
//! died or is out of range.

use super::{inactivate, summoned};
use crate::ai::pet::{Dismiss, PetWorld};
use crate::buff::core::BuffCx;
use crate::item::env::ItemCore;
use crate::item::fx::Fx;
use crate::item::loot::{LootResult, pet_arrived, pet_can_loot};
use crate::world::phases::FrameCtx;
use flyff_common::{ObjId, UnixTime};
use flyff_db::AuditLog;
use std::sync::Arc;

/// The [`PetWorld`] of the game: `MovementHooks::with_pets(Box::new(EatPetWorld::new(core, audit)))`.
#[derive(Debug)]
pub struct EatPetWorld {
    core: Arc<ItemCore>,
    audit: AuditLog,
}

impl EatPetWorld {
    /// The eat-pet side of the item system over the shared item environment and the audit writer.
    pub fn new(core: Arc<ItemCore>, audit: AuditLog) -> EatPetWorld {
        EatPetWorld { core, audit }
    }
}

impl PetWorld for EatPetWorld {
    fn can_loot(&mut self, ctx: &FrameCtx<'_>, owner: ObjId, item: ObjId) -> bool {
        pet_can_loot(&self.core, ctx, owner, item)
    }

    fn arrived(
        &mut self,
        ctx: &mut FrameCtx<'_>,
        owner: ObjId,
        pet: ObjId,
        item: ObjId,
    ) -> LootResult {
        pet_arrived(&self.core, &self.audit, ctx, owner, pet, item)
    }

    /// `owner.InactivateEatPet()` in the world phase: the effects of the pet are taken off the owner (messages
    /// through the sink) and the pet mover is deleted. A pet that is merely out of range (and not the owner's
    /// death) is remembered for the re-summon of `features.reactivate_eatpet` (`SavedEatPetId`): the next pass of
    /// [`tick`](super::tick) summons it again at the owner's position.
    fn inactivate(&mut self, ctx: &mut FrameCtx<'_>, owner: ObjId, pet: ObjId, why: Dismiss) {
        // the world phase has no wall clock; the dismissal reads none
        let cx = BuffCx::new(self.core.env(UnixTime(0)), ctx.now);
        let reactivate = self.core.cfg.reactivate_eatpet && why == Dismiss::OutOfArea;
        let mut fx = Fx::new();
        if let Some(m) = ctx.store.mover_mut(owner) {
            // only this pet: a stale request must not tear down a newer summon
            if summoned(m).is_some_and(|(_, p)| p == pet) {
                let item = summoned(m).map(|(i, _)| i);
                inactivate(m, &cx, &mut fx);
                if reactivate && let Some(p) = m.player_mut() {
                    p.items.saved_eat_pet = item;
                }
            }
        }
        if let Some(m) = ctx.store.mover(owner) {
            fx.flush_sink(&mut *ctx.sink, m, &self.audit);
        }
        ctx.delete(pet);
    }
}
