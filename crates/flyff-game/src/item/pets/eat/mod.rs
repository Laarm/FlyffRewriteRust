//! Eat pets (Plan/19 §13.5, WP-ITEM-18; src:_Common/MoverSkill.cpp:4062-4140, WORLDSERVER/User.cpp:566-587,
//! _Common/Mover.cpp:2280-2305, 5377-5395, 8741; ref/13 §19.8, ref/12 §11.1).
//!
//! An eat pet (`IK3_PET` item) is a real server mover (`prop.link_kind`, `AII_PET`, the brain of
//! [`crate::ai::pet`]) that follows its owner and fetches ground items for him. Using the item toggles it:
//!
//! * **summon** ([`use_eat_pet`], then [`activate`]): refused while flying
//!   (`TID_GAME_CANNOT_CALL_PET_ON_FLYING`); the mover is created at the owner's feet
//!   ([`ai::pet::spawn`](crate::ai::pet::spawn)), the item becomes "in use" (`PlayerItems.summoned_eat_pet`), the lines
//!   of its `EatPet` awakening are applied (`SetDestParamRandomOptExtension`) and so is its active skill
//!   (`DoActiveSkill(skill, 1, owner)`); a vis pet gets its first two sockets, the DSTs of its charms and
//!   `VisPetActivate{item, pet}` ([`super::vis`]);
//! * **dismissal** ([`inactivate`]): the vis DSTs, the awakening lines and the skill buff go, `VisPetActivate{NULL}`
//!   follows for a vis pet, the mover is deleted. The eat pet is dismissed when it is used again (a second eat pet
//!   too: the toggle looks only at the handle), the owner rides, teleports with the Tele16 window
//!   ([`dismiss`]), is killed by an NPC (`features.reactivate_eatpet`), logs out, or the item expires. The AI asks for
//!   it when the owner dies or the pet is farther than 32 m ([`world::EatPetWorld`]).
//!
//! `features.reactivate_eatpet` re-summons a pet that was left more than 32 m behind (the owner teleported, a
//! blink wing, a fall) at the owner's new position: the AI's request and [`tick`] both remember the item in
//! `PlayerItems.saved_eat_pet` and the next pass of [`tick`] uses it again (`SavedEatPetId` / `DoUseItem` of the
//! original; the explicit `before_teleport` / `after_teleport` calls of the plan are not needed because the pet's
//! distance is checked every tick - a teleport within 32 m leaves the pet where it is and it runs to its owner).
//!
//! | piece | content |
//! |---|---|
//! | [`mod@self`] | the mover-level rows and state: [`use_eat_pet`], [`inactivate`], [`on_summoned`], [`Op`] |
//! | [`hooks`] | the `GameCtx` face: [`activate`], [`dismiss`], [`tick`], the death and load hooks, [`EatPetLogout`] |
//! | [`world`] | [`EatPetWorld`]: the item system's side of the AI (`pet_can_loot`, `pet_arrived`, the dismissal) |

pub mod hooks;
pub mod world;

pub use hooks::{EatPetLogout, activate, dismiss, on_owner_killed, tick};
pub use world::EatPetWorld;

use crate::buff::core::{BuffCx, remove_fake};
use crate::buff::set::{BuffKey, BuffKind};
use crate::ctx::{GameCtx, Services};
use crate::follow::Followup;
use crate::item::buffitems::Later;
use crate::item::container::StorageId;
use crate::item::fx::Fx;
use crate::item::model::Item;
use crate::item::usage::RowResult;
use crate::mover::Mover;
use crate::stats::dst::DstSource;
use crate::stats::source::{apply_source, remove_source};
use flyff_common::ObjId;
use flyff_common::defs::text::TID_GAME_CANNOT_CALL_PET_ON_FLYING;
use flyff_data::GameData;
use flyff_proto::pb;
use flyff_rules::consts::{NULL_CHGPARAM, NULL_ID};

/// The ledger source of the awakening lines of the summoned eat pet (`DstSource::Script` tag; the skill rows
/// use their skill ids, far below this).
pub const EAT_PET_TAG: u32 = 0xEA70_0001;
/// The ledger source of the awakening lines of an eat pet.
pub const EAT_PET_SOURCE: DstSource = DstSource::Script { tag: EAT_PET_TAG };

/// What the eat pet's follow-ups do at the `GameCtx` level (queued in an [`Fx`] as [`Followup::EatPet`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    /// `ActivateEatPet(item)`: create the pet mover and finish the summon ([`activate`]).
    Activate {
        /// The owner.
        player: ObjId,
        /// The pet item.
        item: StorageId,
    },
    /// The pet mover goes (`pEatPet->Delete()`): its owner has already given up the handle.
    Remove {
        /// The pet mover.
        pet: ObjId,
    },
    /// `ChangeVisPetSfx`: the effect of the vis pet mover follows the charms; `MoverChangeSfx` goes to the
    /// viewers of the pet when it changed.
    Sfx {
        /// The owner.
        player: ObjId,
        /// The new effect (0 = none).
        sfx: u32,
    },
}

impl Op {
    /// Replaces the mover id `from` by `to` (a join builds its follow-ups before the object store has given the
    /// player its id).
    pub fn rebind(&mut self, from: ObjId, to: ObjId) {
        match self {
            Op::Activate { player, .. } | Op::Sfx { player, .. } if *player == from => {
                *player = to;
            }
            Op::Remove { pet } if *pet == from => *pet = to,
            _ => {}
        }
    }

    /// Executes the action with the services.
    pub fn run(self, svc: &mut Services, ctx: &mut GameCtx<'_>) {
        match self {
            Op::Activate { player, item } => {
                let _ = activate(svc, ctx, crate::ctx::PlayerRef(player), item);
            }
            Op::Remove { pet } => ctx.worlds.delete(pet),
            Op::Sfx { player, sfx } => hooks::change_sfx(svc, ctx, player, sfx),
        }
    }
}

// ---- state ------------------------------------------------------------------------------------------------------

/// `m_oiEatPet` and the item it was summoned from (`m_idPetItem` of the AI).
pub fn summoned(m: &Mover) -> Option<(StorageId, ObjId)> {
    m.player()?.items.summoned_eat_pet
}

/// The inventory item of the summoned eat pet (`GetItemId( pAIPet->GetPetItemId() )`).
pub fn active_eat_item<'a>(m: &'a Mover, _data: &GameData) -> Option<(StorageId, &'a Item)> {
    let (id, _) = summoned(m)?;
    let it = m.player()?.items.inventory.get_at_id(id)?;
    Some((id, it))
}

/// `GetVisPetItem()`: the summoned eat pet when it is a vis pet (`HasActivatedVisPet()`).
pub fn vis_pet_of<'a>(m: &'a Mover, data: &GameData) -> Option<(StorageId, &'a Item)> {
    let (id, it) = active_eat_item(m, data)?;
    let prop = data.item(it.item_id)?;
    it.is_vis_pet(prop).then_some((id, it))
}

// ---- the rows ----------------------------------------------------------------------------------------------------

/// `DoUseEatPet` (the `IK3_PET` row of `DoUseItem`): `if( HasActivatedEatPet() ) InactivateEatPet(); else
/// ActivateEatPet( item )`. The item is permanent: the pipeline only shows its count again. The summon is
/// finished by [`activate`] (the mover is created with the services).
pub fn use_eat_pet(m: &mut Mover, cx: &BuffCx<'_>, id: StorageId, fx: &mut Fx) -> RowResult {
    if summoned(m).is_some() {
        inactivate_and_remove(m, cx, fx);
    } else {
        request_activation(m, cx, id, fx);
    }
    RowResult::Consume
}

/// The checks of `ActivateEatPet` that need only the owner; the rest is [`activate`].
fn request_activation(m: &mut Mover, cx: &BuffCx<'_>, id: StorageId, fx: &mut Fx) {
    let Some(item) = m.player().and_then(|p| p.items.inventory.get_at_id(id)) else {
        return;
    };
    let Some(prop) = cx.data().item(item.item_id) else {
        return;
    };
    // the quiz world is refused by the pipeline
    if m.sim.act.flags.fly() {
        fx.text(TID_GAME_CANNOT_CALL_PET_ON_FLYING);
        return;
    }
    if cx.data().mover(prop.link_kind).is_none() {
        return;
    }
    fx.then(Followup::EatPet(Op::Activate {
        player: m.id(),
        item: id,
    }));
}

/// `InactivateEatPet` for a caller that works with an [`Fx`] (the rows, the expiry): the effects go and the
/// pet mover is queued for deletion.
pub fn inactivate_and_remove(m: &mut Mover, cx: &BuffCx<'_>, fx: &mut Fx) {
    if let Some(pet) = inactivate(m, cx, fx) {
        fx.then(Followup::EatPet(Op::Remove { pet }));
    }
}

/// `InactivateEatPet` on the owner (everything but the deletion of the mover, which the caller does with the
/// returned id): a vis pet loses its DSTs and `VisPetActivate{NULL_ID, NULL_ID}` goes to the owner, the
/// awakening lines and the active skill of the item are reverted, the handle is cleared. The original did
/// nothing but clear the handle when the mover was already gone; here the effects are always reverted.
pub fn inactivate(m: &mut Mover, cx: &BuffCx<'_>, fx: &mut Fx) -> Option<ObjId> {
    let data = cx.data();
    let (item_id, pet) = m.player_mut()?.items.summoned_eat_pet.take()?;
    let item = m
        .player()
        .and_then(|p| p.items.inventory.get_at_id(item_id))
        .and_then(|it| {
            data.item(it.item_id)
                .map(|p| (it.item_id, it.is_vis_pet(p), p))
        });
    if let Some((prop_id, is_vis, prop)) = item {
        if is_vis {
            super::vis::reset_pet_vis_dst(m, cx, prop_id, fx);
            fx.out.to_self(pb::VisPetActivate {
                vis_pet_item_obj_id: NULL_ID,
                eat_pet_obj_id: NULL_ID,
            });
        }
        if prop.active_skill != NULL_ID {
            remove_fake(m, cx, BuffKey::new(BuffKind::Skill, prop.active_skill), fx);
        }
    }
    remove_source(m, cx.stats(), EAT_PET_SOURCE, true, &mut fx.out);
    Some(pet)
}

/// What the summon does to the owner once the mover exists (`ActivateEatPet` after `CreateMover`): the handle,
/// `SetItem` (the awakening lines, the active skill) and, for a vis pet, the sockets, the charm DSTs and
/// `VisPetActivate`.
pub fn on_summoned(m: &mut Mover, cx: &BuffCx<'_>, item: StorageId, pet: ObjId, fx: &mut Fx) {
    let data = cx.data();
    let Some(it) = m.player().and_then(|p| p.items.inventory.get_at_id(item)) else {
        return;
    };
    let Some(prop) = data.item(it.item_id) else {
        return;
    };
    let is_vis = it.is_vis_pet(prop);
    let skill = prop.active_skill;
    let lines: Vec<(u32, i32, i32)> = it
        .random_option
        .lines()
        .map(|l| (u32::from(l.dst), i32::from(l.adj), NULL_CHGPARAM))
        .collect();
    if let Some(p) = m.player_mut() {
        p.items.summoned_eat_pet = Some((item, pet));
    }
    // SetItem: the EatPet awakening and the active skill of the item
    if !lines.is_empty() {
        apply_source(m, cx.stats(), EAT_PET_SOURCE, &lines, true, &mut fx.out);
    }
    if skill != NULL_ID {
        fx.later(Later::ItemSkill {
            player: m.id(),
            skill,
            level: 1,
            target: m.id(),
        });
    }
    if is_vis {
        super::vis::first_summon(m, cx, item, fx);
        super::vis::set_pet_vis_dst(m, cx, item, fx);
        fx.out.to_self(pb::VisPetActivate {
            vis_pet_item_obj_id: u32::from(item),
            eat_pet_obj_id: pet.0,
        });
    }
}
