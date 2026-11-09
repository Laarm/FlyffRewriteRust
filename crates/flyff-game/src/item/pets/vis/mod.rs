//! Vis pets (Plan/19 §13.5, WP-ITEM-18; src:_Common/MoverSkill.cpp:4062-4304, WORLDSERVER/ItemUpgrade.cpp:1546-1739,
//! WORLDSERVER/User.cpp:3015-3030, _Common/Item.cpp:840-875; ref/13 §19.8).
//!
//! A vis pet is an eat pet (`IK3_PET` with `refer_stat1 == PET_VIS`, or one turned into it with
//! `II_SYS_SYS_SCR_PET_MAGIC`) with up to nine sockets for vis charms (`IK3_VIS`). Every operation works on the
//! summoned vis pet of the user, one `&mut Mover` and an [`Fx`]:
//!
//! | operation | original | notes |
//! |---|---|---|
//! | [`use_vis_item`] | `SetPetVisItem` (use of an `IK3_VIS` item) | the same vis id twice -> `EQUIPVIS`, no free socket -> `LACKSLOT`, unmet prerequisites -> `REQVIS`; the charm is socketed with a keep time of `ability_min` minutes and consumed |
//! | [`expand_slots`] | `PetVisSize` (`II_SYS_SYS_VIS_KEY01`) | **PL-070** with `fidelity.fix_vis_slot_expansion` one socket up to nine, else the original's `TID_GAME_BUFFPET_EXPANSION` and nothing consumed |
//! | [`remove_vis`] | `RemovePetVisItem` (`VisPetRemoveVis`, expiry) | **VR-025** `0 <= pos < sockets` (the original read the array unchecked) |
//! | [`swap_vis`] | `SwapVis` (`VisPetSwapVis`) | both positions inside the sockets |
//! | [`transform`] | `TransFormVisPet` (`II_SYS_SYS_SCR_PET_MAGIC`) | the summoned eat pet becomes a vis pet and is summoned anew |
//! | [`process`] | `ProcessVisPet` | every 16 ticks the expired charms are removed |
//!
//! **The effect of the charms.** A charm applies its three `dest_param` slots only while its own prerequisites
//! (`refer_target1/2`: vis ids that must be socketed) hold ([`valid_table`], `GetValidVisTable`, recursive). The DSTs
//! are recorded under the source of the `BUFF_EQUIP` entry of the pet item (`AddBuff(BUFF_EQUIP, item id, 1,
//! 999999999)`), so a reset reverts exactly what was applied. The strongest `ability_max` (1-3) among the charms
//! selects the effect of the pet mover (`GetVisPetSfxId`, [`sfx_id`]); a change reaches the viewers as
//! `MoverChangeSfx` ([`Op::Sfx`](super::eat::Op)).

use super::eat::Op;
use super::eat::{active_eat_item, vis_pet_of};
use crate::buff::core::{BuffCx, add as buff_add, remove_real, source_of};
use crate::buff::set::{BuffKey, BuffKind};
use crate::follow::Followup;
use crate::item::container::StorageId;
use crate::item::fx::{Fx, item_json};
use crate::item::model::{Item, MAX_VIS, PETVIS_DEFAULT_VIS_SLOT_SIZE};
use crate::item::ops::{RemoveReason, UiParam, core as ops, make_long};
use crate::item::usage::UseOutcome;
use crate::mover::Mover;
use crate::stats::source::{apply_source, remove_source};
use flyff_common::defs::item::{II_SYS_SYS_SCR_PET_MAGIC, II_SYS_SYS_VIS_KEY01};
use flyff_common::defs::itemkind::IK3_VIS;
use flyff_common::defs::obj::{XI_BUFFPET_GRADE1, XI_BUFFPET_GRADE2, XI_BUFFPET_GRADE3};
use flyff_common::defs::text::{
    TID_GAME_BUFFPET_EQUIPVIS, TID_GAME_BUFFPET_EXPANSION, TID_GAME_BUFFPET_LACKSLOT,
    TID_GAME_BUFFPET_NOSUMMON01, TID_GAME_BUFFPET_NOSUMMON02, TID_GAME_BUFFPET_NOTREQVIS,
    TID_GAME_BUFFPET_REMOVEVIS, TID_GAME_BUFFPET_REQVIS, TID_GAME_BUFFPET_TIMEOUTVIS,
    TID_GAME_PET_TRAN_FAILURE, TID_GAME_PET_TRAN_SUCCESS,
};
use flyff_data::GameData;
use flyff_data::props::item::ItemProp;
use flyff_db::audit::ev;
use flyff_rules::consts::NULL_ID;
use flyff_rules::item::piercing::is_pierce_able;
use serde_json::json;

/// `BUFF_EQUIP` entry duration (`999999999`): the pet buff only ends with the summon.
const BUFF_FOREVER: u32 = 999_999_999;

/// `SUCCSESS_NEEDVIS` and the failures of `GetValidVisTable` (`UNDEFINED_NEEDVIS` is internal).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NeedVis {
    /// Not decided yet (internal; also the state of a socket whose check is running: a cycle in the
    /// prerequisites fails instead of recursing forever).
    Undefined,
    /// `SUCCSESS_NEEDVIS`: both prerequisites are met (or there are none).
    Success,
    /// `FAILED_1ST_NEEDVIS`: the first one is missing.
    Failed1st,
    /// `FAILED_2ND_NEEDVIS`: the second one is missing.
    Failed2nd,
    /// `FAILED_BOTH_NEEDVIS`: both are missing, the socket is empty or the charm has no prop.
    FailedBoth,
}

// ---- GetValidVisTable --------------------------------------------------------------------------------------------

/// `GetValidVisTable(item)`: for every socket whether the charm in it has its prerequisites (recursively).
pub fn valid_table(data: &GameData, cards: &[u32]) -> Vec<NeedVis> {
    let mut v = vec![NeedVis::Undefined; cards.len()];
    for (i, &c) in cards.iter().enumerate() {
        if c == 0 {
            v[i] = NeedVis::FailedBoth;
        }
    }
    for i in 0..cards.len() {
        if v[i] == NeedVis::Undefined {
            set_valid_need_vis(data, cards, i, &mut v);
        }
    }
    v
}

/// `SetValidNeedVis(item, pos, valid)`. A socket whose check is running reads `Undefined`; meeting it again
/// (a cycle) counts as a failure of that prerequisite.
fn set_valid_need_vis(data: &GameData, cards: &[u32], pos: usize, valid: &mut [NeedVis]) -> bool {
    match valid[pos] {
        NeedVis::Undefined => {}
        done => return done == NeedVis::Success,
    }
    let Some(prop) = data.item(cards[pos]) else {
        valid[pos] = NeedVis::FailedBoth;
        return false;
    };
    if prop.refer_target1 == NULL_ID && prop.refer_target2 == NULL_ID {
        valid[pos] = NeedVis::Success;
        return true;
    }
    // while this socket is being decided it must not satisfy itself
    valid[pos] = NeedVis::FailedBoth;
    let mut r1 = prop.refer_target1 == NULL_ID;
    let mut r2 = prop.refer_target2 == NULL_ID;
    for i in 0..cards.len() {
        if prop.refer_target1 == cards[i] {
            r1 = set_valid_need_vis(data, cards, i, valid);
        }
        if prop.refer_target2 == cards[i] {
            r2 = set_valid_need_vis(data, cards, i, valid);
        }
        if r1 && r2 {
            valid[pos] = NeedVis::Success;
            return true;
        }
    }
    valid[pos] = match (r1, r2) {
        (false, true) => NeedVis::Failed1st,
        (true, false) => NeedVis::Failed2nd,
        _ => NeedVis::FailedBoth,
    };
    false
}

/// `IsSatisfyNeedVis(pet, vis)`: would a charm with the prerequisites of `vis` be valid in this pet? A
/// prerequisite is met by a socketed, valid and unexpired charm (`now` in unix seconds).
pub fn is_satisfy_need_vis(
    data: &GameData,
    cards: &[u32],
    keep_until: &[i64],
    now: i64,
    vis: &ItemProp,
) -> NeedVis {
    let mut needs = [vis.refer_target1, vis.refer_target2];
    if needs[0] == NULL_ID && needs[1] == NULL_ID {
        return NeedVis::Success;
    }
    let table = valid_table(data, cards);
    for (i, &c) in cards.iter().enumerate() {
        let live = table[i] == NeedVis::Success && keep_until.get(i).is_some_and(|&t| now < t);
        for need in &mut needs {
            if *need == c && live {
                *need = NULL_ID;
            }
        }
        if needs[0] == NULL_ID && needs[1] == NULL_ID {
            return NeedVis::Success;
        }
    }
    if needs[0] != NULL_ID && needs[1] == NULL_ID {
        NeedVis::Failed1st
    } else if needs[0] == NULL_ID && needs[1] != NULL_ID {
        NeedVis::Failed2nd
    } else {
        NeedVis::FailedBoth
    }
}

/// `GetVisPetSfxId`: the effect of the strongest charm (`ability_max` 1 to 3), 0 = none.
pub fn sfx_id(data: &GameData, cards: &[u32]) -> u32 {
    let level = cards
        .iter()
        .filter_map(|&c| data.item(c))
        .map(|p| p.ability_max)
        .filter(|g| (1..=3).contains(g))
        .max()
        .unwrap_or(0);
    match level {
        1 => XI_BUFFPET_GRADE1,
        2 => XI_BUFFPET_GRADE2,
        3 => XI_BUFFPET_GRADE3,
        _ => 0,
    }
}

// ---- the effect of the charms ----------------------------------------------------------------------------------

fn name_arg(prop: &ItemProp) -> Vec<String> {
    vec![format!("\"{}\"", prop.name)]
}

/// The ledger source of the vis effects of the pet item `item_id`: its `BUFF_EQUIP` entry.
pub fn source_of_pet(item_id: u32) -> crate::stats::dst::DstSource {
    source_of(BuffKey::new(BuffKind::Equip, item_id))
}

/// `SetPetVisDST(item)`: the `BUFF_EQUIP` entry and the DSTs of every charm whose prerequisites hold; a charm
/// without them is announced (`TID_GAME_BUFFPET_NOTREQVIS`).
pub fn set_pet_vis_dst(m: &mut Mover, cx: &BuffCx<'_>, id: StorageId, fx: &mut Fx) {
    let data = cx.data();
    let Some(item) = m.player().and_then(|p| p.items.inventory.get_at_id(id)) else {
        return;
    };
    if !data.item(item.item_id).is_some_and(|p| item.is_vis_pet(p)) {
        return;
    }
    let (pet_item_id, cards) = (item.item_id, item.piercing.cards.clone());
    buff_add(
        m,
        cx,
        BuffKind::Equip,
        pet_item_id,
        1,
        BUFF_FOREVER,
        None,
        fx,
    );
    let valid = valid_table(data, &cards);
    let src = source_of_pet(pet_item_id);
    for (i, &c) in cards.iter().enumerate() {
        if c == 0 {
            continue;
        }
        let Some(vprop) = data.item(c) else {
            continue;
        };
        if valid[i] == NeedVis::Success {
            let slots: Vec<(u32, i32, i32)> = (0..3)
                .filter(|&k| vprop.dest_param[k] != NULL_ID)
                .map(|k| {
                    (
                        vprop.dest_param[k],
                        vprop.adj_param_val[k],
                        vprop.chg_param_val[k] as i32,
                    )
                })
                .collect();
            if !slots.is_empty() {
                apply_source(m, cx.stats(), src, &slots, true, &mut fx.out);
            }
        } else if m.is_player() {
            fx.defined(TID_GAME_BUFFPET_NOTREQVIS, name_arg(vprop));
        }
    }
}

/// `ResetPetVisDST(item)`: the `BUFF_EQUIP` entry goes, then the DSTs it applied revert exactly.
pub fn reset_pet_vis_dst(m: &mut Mover, cx: &BuffCx<'_>, item_id: u32, fx: &mut Fx) {
    remove_real(m, cx, BuffKey::new(BuffKind::Equip, item_id), fx);
    remove_source(m, cx.stats(), source_of_pet(item_id), true, &mut fx.out);
}

/// `ChangeVisPetSfx`: the effect of the pet mover follows the strongest charm. The comparison and the
/// `MoverChangeSfx` happen where the pet mover is reachable ([`Op::Sfx`]).
fn change_sfx(m: &Mover, cx: &BuffCx<'_>, id: StorageId, fx: &mut Fx) {
    let Some(item) = m.player().and_then(|p| p.items.inventory.get_at_id(id)) else {
        return;
    };
    fx.then(Followup::EatPet(Op::Sfx {
        player: m.id(),
        sfx: sfx_id(cx.data(), &item.piercing.cards),
    }));
}

// ---- audit -----------------------------------------------------------------------------------------------------

fn audit(
    fx: &mut Fx,
    kind: &str,
    before: &Item,
    after: Option<&Item>,
    materials: serde_json::Value,
) {
    fx.audit(
        ev::ITEM_UPGRADE,
        json!({
            "kind": kind,
            "result": "success",
            "before": item_json(before),
            "after": after.map(item_json),
            "materials": materials,
        }),
    );
}

// ---- the operations --------------------------------------------------------------------------------------------

/// How an operation ended, for the use pipeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisResult {
    /// It happened.
    Done,
    /// A text was queued, nothing changed.
    Refused,
    /// Nothing happened and nothing was said (the original's silent `return`).
    Ignored,
}

impl From<VisResult> for UseOutcome {
    fn from(r: VisResult) -> UseOutcome {
        match r {
            VisResult::Done => UseOutcome::Used,
            VisResult::Refused => UseOutcome::Refused,
            VisResult::Ignored => UseOutcome::Ignored,
        }
    }
}

/// The three item rows of `CUser::DoUseItem` that run before the pack items (`User.cpp:3015-3030`): a vis charm,
/// the vis key and the transform scroll. `None` = not one of them, the pipeline goes on.
pub fn try_use(
    m: &mut Mover,
    cx: &BuffCx<'_>,
    id: StorageId,
    prop: &ItemProp,
    fx: &mut Fx,
) -> Option<UseOutcome> {
    let r = if prop.item_kind3 == IK3_VIS {
        use_vis_item(m, cx, id, fx)
    } else if prop.id == II_SYS_SYS_VIS_KEY01 {
        expand_slots(m, cx, id, fx)
    } else if prop.id == II_SYS_SYS_SCR_PET_MAGIC {
        transform(m, cx, id, fx)
    } else {
        return None;
    };
    Some(r.into())
}

/// `SetPetVisItem(vis)`: socket a vis charm into the summoned vis pet.
pub fn use_vis_item(m: &mut Mover, cx: &BuffCx<'_>, vis: StorageId, fx: &mut Fx) -> VisResult {
    let data = cx.data();
    let Some((pet_id, pet)) = vis_pet_of(m, data).filter(|(_, it)| it.is_usable()) else {
        fx.text(TID_GAME_BUFFPET_NOSUMMON01);
        return VisResult::Refused;
    };
    let Some(visitem) = m
        .player()
        .and_then(|p| p.items.inventory.get_at_id(vis))
        .filter(|it| it.is_usable())
    else {
        return VisResult::Ignored;
    };
    let Some(vprop) = data.item(visitem.item_id) else {
        return VisResult::Ignored;
    };
    let size = pet.piercing.cards.len();
    // `IsPierceAble(vis.ik3)`: a vis pet takes IK3_VIS cards up to nine sockets
    let Some(pprop) = data.item(pet.item_id) else {
        return VisResult::Ignored;
    };
    if !is_pierce_able(
        pprop,
        size,
        pet.is_vis_pet(pprop),
        Some(vprop.item_kind3),
        false,
        cx.cfg().no_piercing,
    ) {
        return VisResult::Ignored;
    }
    let mut first_empty = None;
    for (i, &c) in pet.piercing.cards.iter().enumerate() {
        if c == vprop.id {
            fx.text(TID_GAME_BUFFPET_EQUIPVIS);
            return VisResult::Refused;
        }
        if first_empty.is_none() && c == 0 {
            first_empty = Some(i);
        }
    }
    let Some(slot) = first_empty else {
        fx.text(TID_GAME_BUFFPET_LACKSLOT);
        return VisResult::Refused;
    };
    let now = cx.unix();
    if is_satisfy_need_vis(
        data,
        &pet.piercing.cards,
        &pet.piercing.vis_keep_until,
        now,
        vprop,
    ) != NeedVis::Success
    {
        fx.text(TID_GAME_BUFFPET_REQVIS);
        return VisResult::Refused;
    }
    let before = pet.clone();
    let (pet_item_id, vis_item_id, minutes) = (pet.item_id, vprop.id, vprop.ability_min);
    reset_pet_vis_dst(m, cx, pet_item_id, fx);
    ops::update_item(
        m,
        &cx.env,
        pet_id,
        UiParam::PetVisItem,
        make_long(slot as u32, vis_item_id),
        minutes,
        fx,
    );
    ops::remove_item(m, &cx.env, vis, 1, RemoveReason::Consume, fx);
    if let Some(after) = m.player().and_then(|p| p.items.inventory.get_at_id(pet_id)) {
        audit(
            fx,
            "vis_insert",
            &before,
            Some(after),
            json!([{ "item": vis_item_id, "count": 1 }]),
        );
    }
    set_pet_vis_dst(m, cx, pet_id, fx);
    change_sfx(m, cx, pet_id, fx);
    VisResult::Done
}

/// `PetVisSize(key)`: one more socket (PL-070).
pub fn expand_slots(m: &mut Mover, cx: &BuffCx<'_>, key: StorageId, fx: &mut Fx) -> VisResult {
    let data = cx.data();
    let Some((pet_id, pet)) = vis_pet_of(m, data).filter(|(_, it)| it.is_usable()) else {
        fx.text(TID_GAME_BUFFPET_NOSUMMON02);
        return VisResult::Refused;
    };
    let usable_key = m
        .player()
        .and_then(|p| p.items.inventory.get_at_id(key))
        .is_some_and(|it| it.is_usable() && it.item_id == II_SYS_SYS_VIS_KEY01);
    if !usable_key {
        return VisResult::Ignored;
    }
    let size = pet.piercing.cards.len();
    // PL-070: `IsPierceAble(NULL_ID, TRUE)` is false for every vis pet in the original build
    let ok = cx.cfg().fix_vis_slot_expansion && size < MAX_VIS;
    if !ok {
        fx.text(TID_GAME_BUFFPET_EXPANSION);
        return VisResult::Refused;
    }
    let before = pet.clone();
    ops::update_item(
        m,
        &cx.env,
        pet_id,
        UiParam::PetVisSize,
        (size + 1) as u32,
        0,
        fx,
    );
    ops::remove_item(m, &cx.env, key, 1, RemoveReason::Consume, fx);
    if let Some(after) = m.player().and_then(|p| p.items.inventory.get_at_id(pet_id)) {
        audit(
            fx,
            "vis_expand",
            &before,
            Some(after),
            json!([{ "item": II_SYS_SYS_VIS_KEY01, "count": 1 }]),
        );
    }
    VisResult::Done
}

/// `RemovePetVisItem(pos, expired)`: the charm in socket `pos` is destroyed. VR-025: a negative position or one
/// beyond the sockets is ignored, so is an empty socket.
pub fn remove_vis(
    m: &mut Mover,
    cx: &BuffCx<'_>,
    pos: i32,
    expired: bool,
    fx: &mut Fx,
) -> VisResult {
    let data = cx.data();
    let Some((pet_id, pet)) = vis_pet_of(m, data).filter(|(_, it)| it.is_usable()) else {
        return VisResult::Ignored;
    };
    let Some(idx) = usize::try_from(pos)
        .ok()
        .filter(|&i| i < pet.piercing.cards.len())
    else {
        return VisResult::Ignored;
    };
    let vis_id = pet.piercing.cards[idx];
    if vis_id == 0 {
        return VisResult::Ignored;
    }
    let before = pet.clone();
    let pet_item_id = pet.item_id;
    reset_pet_vis_dst(m, cx, pet_item_id, fx);
    ops::update_item(
        m,
        &cx.env,
        pet_id,
        UiParam::PetVisItem,
        make_long(idx as u32, 0),
        0,
        fx,
    );
    if let Some(after) = m.player().and_then(|p| p.items.inventory.get_at_id(pet_id)) {
        let kind = if expired { "vis_expire" } else { "vis_remove" };
        audit(
            fx,
            kind,
            &before,
            Some(after),
            json!([{ "item": vis_id, "count": 1 }]),
        );
    }
    if let Some(p) = data.item(vis_id) {
        fx.defined(TID_GAME_BUFFPET_REMOVEVIS, name_arg(p));
    }
    set_pet_vis_dst(m, cx, pet_id, fx);
    change_sfx(m, cx, pet_id, fx);
    VisResult::Done
}

/// `SwapVis(a, b)` (`VisPetSwapVis`): both positions must be sockets.
pub fn swap_vis(m: &mut Mover, cx: &BuffCx<'_>, a: i32, b: i32, fx: &mut Fx) -> VisResult {
    let data = cx.data();
    let Some((pet_id, pet)) = vis_pet_of(m, data).filter(|(_, it)| it.is_usable()) else {
        return VisResult::Ignored;
    };
    let size = pet.piercing.cards.len();
    let (Some(a), Some(b)) = (
        usize::try_from(a).ok().filter(|&i| i < size),
        usize::try_from(b).ok().filter(|&i| i < size),
    ) else {
        return VisResult::Ignored;
    };
    ops::update_item(
        m,
        &cx.env,
        pet_id,
        UiParam::PetVisItemSwap,
        make_long(a as u32, b as u32),
        0,
        fx,
    );
    VisResult::Done
}

/// `TransFormVisPet(material)`: the summoned eat pet becomes a vis pet. The pet is dismissed and summoned
/// anew with the vis state (`UpdateItem(UI_TRANSFORM_VISPET)` does that on the server).
pub fn transform(m: &mut Mover, cx: &BuffCx<'_>, material: StorageId, fx: &mut Fx) -> VisResult {
    let data = cx.data();
    // no summoned eat pet: the original's "the mover is not valid" branch
    if m.player().and_then(|p| p.items.summoned_eat_pet).is_none() {
        fx.text(TID_GAME_PET_TRAN_FAILURE);
        return VisResult::Refused;
    }
    let Some((pet_id, pet)) = active_eat_item(m, data) else {
        return VisResult::Ignored;
    };
    let usable_material = m
        .player()
        .and_then(|p| p.items.inventory.get_at_id(material))
        .is_some_and(|it| it.is_usable());
    if !pet.is_usable() || !usable_material {
        return VisResult::Ignored;
    }
    let Some(pprop) = data.item(pet.item_id) else {
        return VisResult::Ignored;
    };
    if pet.is_vis_pet(pprop) {
        fx.text(TID_GAME_PET_TRAN_FAILURE);
        return VisResult::Refused;
    }
    let before = pet.clone();
    let name = name_arg(pprop);
    ops::remove_item(m, &cx.env, material, 1, RemoveReason::Consume, fx);
    // the pet comes down first (the item is still a plain eat pet), then the item changes, then it is summoned
    if let Some(pet_mover) = super::eat::inactivate(m, cx, fx) {
        fx.then(Followup::EatPet(Op::Remove { pet: pet_mover }));
    }
    ops::update_item(m, &cx.env, pet_id, UiParam::TransformVisPet, 1, 0, fx);
    if let Some(after) = m.player().and_then(|p| p.items.inventory.get_at_id(pet_id)) {
        audit(
            fx,
            "vis_transform",
            &before,
            Some(after),
            json!([{ "item": II_SYS_SYS_SCR_PET_MAGIC, "count": 1 }]),
        );
    }
    fx.defined(TID_GAME_PET_TRAN_SUCCESS, name);
    fx.then(Followup::EatPet(Op::Activate {
        player: m.id(),
        item: pet_id,
    }));
    VisResult::Done
}

/// The summoned vis pet's first summon: `UpdateItem(PETVIS_SIZE, 2)` when it has no sockets yet.
pub fn first_summon(m: &mut Mover, cx: &BuffCx<'_>, id: StorageId, fx: &mut Fx) {
    let empty = m
        .player()
        .and_then(|p| p.items.inventory.get_at_id(id))
        .is_some_and(|it| it.piercing.cards.is_empty());
    if empty {
        ops::update_item(
            m,
            &cx.env,
            id,
            UiParam::PetVisSize,
            PETVIS_DEFAULT_VIS_SLOT_SIZE as u32,
            0,
            fx,
        );
    }
}

/// `ProcessVisPet` (every 16 ticks): charms whose keep time has passed are announced
/// (`TID_GAME_BUFFPET_TIMEOUTVIS`) and removed. Returns the number removed.
pub fn process(m: &mut Mover, cx: &BuffCx<'_>, fx: &mut Fx) -> usize {
    let data = cx.data();
    let Some((_, pet)) = vis_pet_of(m, data) else {
        return 0;
    };
    let now = cx.unix();
    let due: Vec<(usize, u32)> = pet
        .piercing
        .cards
        .iter()
        .enumerate()
        .filter(|&(i, &c)| {
            c != 0
                && pet
                    .piercing
                    .vis_keep_until
                    .get(i)
                    .is_some_and(|&t| now >= t)
        })
        .map(|(i, &c)| (i, c))
        .collect();
    let mut n = 0;
    for (i, c) in due {
        if let Some(p) = data.item(c) {
            fx.defined(TID_GAME_BUFFPET_TIMEOUTVIS, name_arg(p));
        }
        if remove_vis(m, cx, i as i32, true, fx) == VisResult::Done {
            n += 1;
        }
    }
    n
}
