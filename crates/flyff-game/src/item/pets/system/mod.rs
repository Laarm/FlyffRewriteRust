//! System pets: the pet inside an egg item (Plan/19 §13.1-§13.3, §5.9, WP-ITEM-16; ref/13 §19.1-§19.4;
//! src:_Common/MoverSkill.cpp:4011-4060, _Common/Mover.cpp:11112-11605, WORLDSERVER/DPSrvr.cpp:10168-10186).
//!
//! | module | content |
//! |---|---|
//! | [`state`] | `PetState` methods over the pet tables: max energy / EXP, item id, mover index, avail DST, wire form |
//! | [`summon`] | the egg toggle (`DoUseSystemPet`), [`activate`], [`release`], the avail bonus ([`apply_avail`], [`remove_bonus`]) |
//! | [`process`](mod@process) | `ProcessPet` every 16 ticks: avail, energy and life, the per-minute EXP |
//! | [`exp`] | `AddPetExpOnKill`, `PetLevelup` (hatch, avail roll), the DoDie penalty |
//! | [`scrolls`] | the life and hatch scrolls, pet naming and `ClearPetName` |
//! | [`hooks`] | the `GameCtx` face: the tick job, the death hooks, the join, the client handlers |
//!
//! The pet's visual mover exists only in the clients (`CAIEgg`, ref/12 §11.2): the server keeps the item's
//! `PetState`, `PlayerItems.pet_obj_id` (the summoned egg), the `BUFF_PET` entry with the stat bonus and the
//! `DstSource::Pet` lines of an awakening. Every mover-level function works on one `&mut Mover` and fills an
//! [`Fx`]; the persistence is the inventory document (the pet is saved atomically with its item) and the
//! summoned slot of the character row.

pub mod exp;
pub mod hooks;
pub mod process;
pub mod scrolls;
pub mod state;
pub mod summon;

pub use exp::{exp_on_kill, level_up, on_owner_killed};
pub use hooks::{
    after_join, on_clear_pet_name, on_do_use_item_input, on_pet_release_req, on_player_load,
    owner_pets_hook, pet_exp_hook, tick,
};
pub use process::process;
pub use scrolls::{clear_name, name_pet, use_hatch_scroll, use_life_scroll};
pub use state::MAX_PET_NAME_BYTES;
pub use summon::{activate, apply_avail, has_pet_buff, release, remove_bonus, use_egg};

use crate::item::container::StorageId;
use crate::item::fx::{Fx, item_json};
use crate::item::model::{Item, PetState};
use crate::mover::Mover;
use flyff_data::GameData;
use flyff_db::audit::ev;
use serde_json::json;

/// `PETLOGTYPE_*`: why a pet audit line was written (`pet.level_up` carries the cause).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PetCause {
    /// A kill filled the experience bar.
    Kill,
    /// The hatch scroll.
    HatchScroll,
    /// The tamer's mistake with `features.pet_auto_level_up`.
    Mistake,
    /// The tamer's miracle.
    Miracle,
    /// A quest reward (`m_bEndRewardPetLevelup`).
    Quest,
    /// A GM command.
    Gm,
}

impl PetCause {
    /// The audit tag.
    pub fn tag(self) -> &'static str {
        match self {
            PetCause::Kill => "kill",
            PetCause::HatchScroll => "hatch_scroll",
            PetCause::Mistake => "mistake",
            PetCause::Miracle => "miracle",
            PetCause::Quest => "quest",
            PetCause::Gm => "gm",
        }
    }
}

/// `GetPetItem()` and `GetPet()` in one: the summoned pet's storage id, its item and its state. `None` without a
/// summoned id, when the item is gone or is not an egg item (`IsPet`), or carries no `CPet`.
pub fn active_pet<'a>(
    m: &'a Mover,
    data: &GameData,
) -> Option<(StorageId, &'a Item, &'a PetState)> {
    let id = m.player()?.items.pet_obj_id?;
    let item = m.player()?.items.inventory.get_at_id(id)?;
    let prop = data.item(item.item_id)?;
    if !Item::is_pet(prop) {
        return None;
    }
    Some((id, item, item.pet.as_deref()?))
}

/// The summoned pet's storage id (`GetPetItem()->m_dwObjId`), see [`active_pet`].
pub fn active_id(m: &Mover, data: &GameData) -> Option<StorageId> {
    active_pet(m, data).map(|(id, _, _)| id)
}

/// The summoned pet's state, mutably (the item must be the pet item of [`active_pet`]).
pub(crate) fn pet_mut(m: &mut Mover, id: StorageId) -> Option<&mut PetState> {
    m.player_mut()?
        .items
        .inventory
        .get_at_id_mut(id)?
        .pet
        .as_deref_mut()
}

/// The item behind a pet slot, mutably.
pub(crate) fn item_mut(m: &mut Mover, id: StorageId) -> Option<&mut Item> {
    m.player_mut()?.items.inventory.get_at_id_mut(id)
}

/// Queues the audit line `event` of the pet in slot `id`: `{item, kind, level, exp, energy, life}` plus the cause
/// and the `extra` fields.
pub(crate) fn audit_pet(
    fx: &mut Fx,
    event: &'static str,
    m: &Mover,
    id: StorageId,
    cause: Option<PetCause>,
    extra: &[(&str, serde_json::Value)],
) {
    let Some(item) = m.player().and_then(|p| p.items.inventory.get_at_id(id)) else {
        return;
    };
    let Some(pet) = item.pet.as_deref() else {
        return;
    };
    let mut data = json!({
        "item": item_json(item),
        "kind": pet.kind,
        "level": pet.level_u8(),
        "exp": pet.exp,
        "energy": pet.energy,
        "life": pet.life,
    });
    if let Some(o) = data.as_object_mut() {
        if let Some(c) = cause {
            o.insert("cause".into(), json!(c.tag()));
        }
        for (k, v) in extra {
            o.insert((*k).to_owned(), v.clone());
        }
    }
    fx.audit(event, data);
}

/// The audit event of a hatch.
pub(crate) const EV_HATCH: &str = ev::PET_HATCH;
/// The audit event of a level-up.
pub(crate) const EV_LEVEL_UP: &str = ev::PET_LEVEL_UP;
/// The audit event of a feeding.
pub(crate) const EV_FEED: &str = ev::PET_FEED;
/// The audit event of a pet losing a life or dying.
pub(crate) const EV_DEATH: &str = ev::PET_DEATH;
