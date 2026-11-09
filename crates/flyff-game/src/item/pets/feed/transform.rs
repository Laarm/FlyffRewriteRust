//! The egg transform (Plan/19 §13.4; ref/13 §19.7; src:WORLDSERVER/DPSrvr.cpp:12186-12198, _Common/pet.cpp:680-985,
//! PL-069, VR-025): a fixed number of distinct eggs become one random result of `transformitem.inc`.
//!
//! **FIX (PL-069 / VR-025).** The original removed the eggs first and only then created the result: an entry
//! whose item does not exist (the pet kinds DRAGON, GRIFFIN and UNICORN have no `AvailDestParam` row, so their
//! item id is 0) or a roll that lands in the gap of a table whose weights sum below 1 000 000 left the player with
//! nothing, and a transformation number other than 0 dereferenced a null transformer. Here the request is
//! validated, the result is rolled **and resolved** first, and only a resolvable result consumes the eggs;
//! everything else answers `TID_GAME_TRANSFORM_S01` and changes nothing.

use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::container::StorageId;
use crate::item::fx::Fx;
use crate::item::model::{Item, PetLevel, PetState};
use crate::item::ops::{self, ItemOrigin, RemoveReason};
use flyff_common::defs::text::{TID_GAME_TRANSFORM_S00, TID_GAME_TRANSFORM_S01};
use flyff_common::rng::GameRng;
use flyff_data::GameData;
use flyff_data::events::transform_item::{TransformComponent, TransformResult};
use flyff_proto::pb;
use flyff_rules::item::pet::PET_AVAIL_SLOTS;
use std::collections::BTreeSet;

/// `eMaxProb`: the probabilities of `transformitem.inc` are out of 1 000 000.
pub const MAX_PROB: u32 = 1_000_000;

/// Why a request does nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The request is malformed or a stuff item is not usable: silent (the original returned silently).
    Invalid,
    /// `TID_GAME_TRANSFORM_S01`: no such transformation, an empty table, or a roll that gives nothing.
    NoResult,
}

/// What a transformation yields, resolved to an item ready to be created.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved {
    /// The item (a plain stack or an egg item with its pet).
    pub item: Item,
}

/// `IsValidStuff` (both the base and the egg transformer): exactly `stuff` distinct inventory items, each an egg
/// that is not summoned or otherwise in use, not reserved, with `num == 1`. Returns the storage ids.
pub fn validate_stuff(
    p: &crate::player::PlayerExt,
    data: &GameData,
    comp: &TransformComponent,
    stuff: &pb::TransformStuff,
) -> Result<Vec<StorageId>, Refusal> {
    if usize::try_from(comp.stuff).ok() != Some(stuff.items.len()) {
        return Err(Refusal::Invalid);
    }
    let mut seen = BTreeSet::new();
    let mut ids = Vec::with_capacity(stuff.items.len());
    for s in &stuff.items {
        let id = StorageId::try_from(s.item_obj_id).map_err(|_| Refusal::Invalid)?;
        let item = p.items.inventory.get_at_id(id).ok_or(Refusal::Invalid)?;
        if p.items.is_using(id) || !item.is_usable() || p.items.inventory.is_equip(id) {
            return Err(Refusal::Invalid);
        }
        if !seen.insert(id) {
            return Err(Refusal::Invalid);
        }
        let prop = data.item(item.item_id).ok_or(Refusal::Invalid)?;
        if !item.is_egg(prop) || s.num != 1 {
            return Err(Refusal::Invalid);
        }
        ids.push(id);
    }
    Ok(ids)
}

/// `CTransformItemComponent::GetItem` with the resolution of the element: the roll `0..1 000 000` picks the
/// element; `None` when the roll lands past the weights, the item does not exist or the pet kind has no row.
pub fn resolve(data: &GameData, comp: &TransformComponent, roll: u32) -> Option<Resolved> {
    let element = comp.pick(roll)?;
    match &element.result {
        TransformResult::Item { item, num } => {
            data.item(*item)?;
            let num = u16::try_from((*num).max(1)).unwrap_or(u16::MAX);
            Some(Resolved {
                item: Item::new(*item, num),
            })
        }
        TransformResult::Pet {
            kind,
            avail,
            life,
            exp_percent,
        } => {
            let kind = u8::try_from(*kind).ok()?;
            let level = u8::try_from(avail.len())
                .ok()
                .filter(|l| (1..=5).contains(l))?;
            let mut avail_levels = [0_u8; PET_AVAIL_SLOTS];
            for (slot, v) in avail_levels.iter_mut().zip(avail) {
                *slot = *v;
            }
            let mut pet = PetState {
                kind,
                level: PetLevel::from_u8(level),
                exp: 0,
                energy: 0,
                life: u16::try_from((*life).max(0)).unwrap_or(u16::MAX),
                avail_levels,
                name: String::new(),
            };
            let item_id = pet.item_id(&data.pet);
            if item_id == 0 || data.item(item_id).is_none() {
                return None;
            }
            // `(DWORD)( GetMaxExp() * fExp / 100 )` in float
            pet.set_exp(((pet.max_exp() as f32 * *exp_percent) / 100.0_f32) as u32);
            let max_energy = u32::from(pet.max_energy(&data.pet));
            pet.set_energy(&data.pet, max_energy);
            let mut item = Item::new(item_id, 1);
            item.pet = Some(Box::new(pet));
            Some(Resolved { item })
        }
    }
}

/// `TransformItem{transform, items}`. Only transformation 0 has a transformer: another number answers
/// `TID_GAME_TRANSFORM_S01`, as does a table without that transformation. A request that does not match the
/// stuff of the transformation (its size, distinct usable eggs, `num == 1`) is dropped silently. Then the result
/// is rolled (`xRandom( 0, 1e6 )`) and resolved; an unresolvable result answers S01 and consumes nothing.
/// Otherwise the eggs are removed, the result is created and `TID_GAME_TRANSFORM_S00` answers.
pub fn on_transform_item(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    msg: pb::TransformItem,
) {
    let core = svc.items.handle();
    let data = &*core.data;
    let Some(stuff) = msg.stuff else { return };
    let mut fx = Fx::new();
    let Some(comp) = (stuff.transform_id == 0)
        .then(|| data.transform_items.component(0))
        .flatten()
    else {
        fx.text(TID_GAME_TRANSFORM_S01);
        fx.flush(svc, ctx, who);
        return;
    };
    let Some(m) = ctx.player(who) else { return };
    let Some(p) = m.player() else { return };
    let world = m.core.world;
    let Ok(ids) = validate_stuff(p, data, comp, &stuff) else {
        return;
    };
    let eggs: Vec<(StorageId, Item)> = ids
        .iter()
        .filter_map(|&id| p.items.inventory.get_at_id(id).map(|it| (id, it.clone())))
        .collect();
    let roll = ctx.rng.world(world).random_range(0, MAX_PROB);
    let Some(result) = resolve(data, comp, roll) else {
        fx.text(TID_GAME_TRANSFORM_S01);
        fx.flush(svc, ctx, who);
        return;
    };
    // all or nothing: the eggs go, the result comes; the freed slots always hold it
    for (id, _) in &eggs {
        ops::remove_item(svc, ctx, who, *id, 1, RemoveReason::Other("transform"));
    }
    match ops::create_item(svc, ctx, who, result.item, ItemOrigin::PetTransform) {
        Ok(_) => fx.text(TID_GAME_TRANSFORM_S00),
        Err(e) => {
            tracing::warn!(error = %e, "egg transform: the result did not fit, the eggs are given back");
            for (_, egg) in eggs {
                let _ = ops::create_item(svc, ctx, who, egg, ItemOrigin::PetTransform);
            }
            fx.text(TID_GAME_TRANSFORM_S01);
        }
    }
    svc.saves.mark_urgent(who.0);
    fx.flush(svc, ctx, who);
}
