//! Cast time of item use (Plan/19 §5.2; src:_Common/Mover.cpp:8732-8792, WORLDSERVER/User.cpp:384-415).
//!
//! An item with `skill_ready_type` (ms) does not act at once: the first use starts a cast (the base-motion
//! state mode goes on, `StateModeNtf`), the item is **not** consumed, and when the cast time has passed the
//! per-tick item job re-issues the use, which finds the state on with no cast pending, switches it off and
//! proceeds. A second such item while casting gives `TID_PK_BLINK_LIMIT`.
//!
//! The virtual uses (`DoUseItemVirtual`: the escape wing of `DoEscape`) cast the same way without an inventory
//! item: the pending record carries the prop id instead (`m_bItemFind == FALSE`).

use crate::item::container::StorageId;
use crate::item::env::ItemEnv;
use crate::item::fx::Fx;
use crate::item::inventory::CastPending;
use crate::mover::Mover;
use crate::stats::fashion;
use flyff_common::GameMs;
use flyff_common::defs::itemkind::{IK2_BLINKWING, IK2_RIDING};
use flyff_common::defs::text::{TID_GAME_NOTSIT_BLINK, TID_PK_BLINK_LIMIT};
use flyff_data::props::item::ItemProp;
use flyff_rules::consts::{NULL_ID, STATE_BASEMOTION_MODE};

/// `STATEMODE_BASEMOTION_ON`.
pub const STATEMODE_ON: u32 = 0;
/// `STATEMODE_BASEMOTION_OFF`.
pub const STATEMODE_OFF: u32 = 1;
/// `STATEMODE_BASEMOTION_CANCEL`.
pub const STATEMODE_CANCEL: u32 = 2;

/// The answer of [`ready_check`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CastStep {
    /// No cast time (or the cast just finished): carry on.
    Proceed,
    /// A cast started; the use fires later and nothing is consumed now.
    Deferred,
    /// Refused (a text was queued).
    Refused,
}

/// `true` for an item that has a cast time (`skill_ready_type` neither 0 nor `NULL_ID`).
pub fn has_cast_time(prop: &ItemProp) -> bool {
    prop.skill_ready_type != 0 && prop.skill_ready_type != NULL_ID
}

/// `SetStateMode(BASEMOTION, flag)` / `SetStateNotMode(BASEMOTION, flag)` with the `StateModeNtf` to the near
/// set (`stats::fashion`).
fn set_state_mode(m: &mut Mover, on: bool, flag: u32, item_prop_id: u32, fx: &mut Fx) {
    if on {
        fashion::set_state_mode(m, STATE_BASEMOTION_MODE, flag, item_prop_id, &mut fx.out);
    } else {
        fashion::set_state_not_mode(m, STATE_BASEMOTION_MODE, flag, &mut fx.out);
    }
}

/// What a cast is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    /// The inventory item with this storage id.
    Item(StorageId),
    /// The item prop of a virtual use.
    Virtual(u32),
}

/// `IsItemRedyTime(prop, obj_id, find)` (src:_Common/Mover.cpp:8732): see the module docs. A ride first
/// dismisses (`features.improved_pets`) or refuses (`TID_GAME_CANNOT_FLY_WITH_PET`) pets; a blink wing
/// cannot start while sitting.
pub fn ready_check(
    m: &mut Mover,
    env: &ItemEnv<'_>,
    id: StorageId,
    prop: &ItemProp,
    now: GameMs,
    fx: &mut Fx,
) -> CastStep {
    ready_check_for(m, env, Target::Item(id), prop, now, fx)
}

/// [`ready_check`] of a virtual use (`bItemFind == FALSE`): the pending record names the prop.
pub fn ready_check_virtual(
    m: &mut Mover,
    env: &ItemEnv<'_>,
    prop: &ItemProp,
    now: GameMs,
    fx: &mut Fx,
) -> CastStep {
    ready_check_for(m, env, Target::Virtual(prop.id), prop, now, fx)
}

fn ready_check_for(
    m: &mut Mover,
    env: &ItemEnv<'_>,
    target: Target,
    prop: &ItemProp,
    now: GameMs,
    fx: &mut Fx,
) -> CastStep {
    if !has_cast_time(prop) {
        return CastStep::Proceed;
    }
    if prop.item_kind2 == IK2_RIDING {
        let has_pets = m
            .player()
            .is_some_and(|p| p.items.pet_obj_id.is_some() || p.items.summoned_eat_pet.is_some());
        if env.cfg().improved_pets {
            // `PetRelease()` (Mover.cpp:8743): the system pet is released with its messages (ITEM-16), the eat pet
            // is dismissed (`InactivateEatPet()`, ITEM-18)
            let cx = crate::buff::BuffCx::new(*env, now);
            crate::item::pets::system::release(m, &cx, fx);
            crate::item::pets::eat::inactivate_and_remove(m, &cx, fx);
        } else if has_pets {
            fx.text(flyff_common::defs::text::TID_GAME_CANNOT_FLY_WITH_PET);
            return CastStep::Refused;
        }
    }
    let in_cast_state = m.mv.state_mode & STATE_BASEMOTION_MODE != 0;
    if in_cast_state {
        let casting = m.player().is_some_and(|p| p.items.cast.is_some());
        if casting {
            fx.text(TID_PK_BLINK_LIMIT);
            return CastStep::Refused;
        }
        // the cast finished: the state goes off and the use goes on
        set_state_mode(m, false, STATEMODE_OFF, 0, fx);
        return CastStep::Proceed;
    }
    if prop.item_kind2 == IK2_BLINKWING && m.sim.act.flags.sit() {
        fx.text(TID_GAME_NOTSIT_BLINK);
        return CastStep::Refused;
    }
    if let Some(p) = m.player_mut() {
        let (obj_id, virtual_item) = match target {
            Target::Item(id) => (id, None),
            Target::Virtual(prop_id) => (0, Some(prop_id)),
        };
        p.items.cast = Some(CastPending {
            ready_at: GameMs(now.0.saturating_add(u64::from(prop.skill_ready_type))),
            obj_id,
            virtual_item,
            timer: None,
        });
    }
    set_state_mode(m, true, STATEMODE_ON, prop.id, fx);
    CastStep::Deferred
}

/// `StateModeReq{CANCEL}` (doc 16 handler): drops a pending cast and switches the state off. Returns `true`
/// when a cast was pending.
pub fn cancel_cast(m: &mut Mover, fx: &mut Fx) -> bool {
    let had = m
        .player_mut()
        .is_some_and(|p| p.items.cast.take().is_some());
    if m.mv.state_mode & STATE_BASEMOTION_MODE != 0 {
        set_state_mode(m, false, STATEMODE_CANCEL, 0, fx);
    }
    had
}

/// What is to be re-issued when a cast is over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Due {
    /// The use of the inventory item.
    Item(StorageId),
    /// The virtual use of this item prop.
    Virtual(u32),
}

/// The use whose cast is due at `now`: its pending record is taken (`m_nReadyTime = 0`) and the state mode
/// stays on until the re-issued use finds it.
pub fn take_due(m: &mut Mover, now: GameMs) -> Option<Due> {
    let in_cast_state = m.mv.state_mode & STATE_BASEMOTION_MODE != 0;
    let p = m.player_mut()?;
    let c = p.items.cast?;
    if !in_cast_state || now < c.ready_at {
        return None;
    }
    p.items.cast = None;
    Some(match c.virtual_item {
        Some(prop_id) => Due::Virtual(prop_id),
        None => Due::Item(c.obj_id),
    })
}

/// The cast is due but the item is gone: `TID_PK_BLINK_LIMIT` and the cast is cancelled.
pub fn fail_due(m: &mut Mover, fx: &mut Fx) {
    fx.text(TID_PK_BLINK_LIMIT);
    set_state_mode(m, false, STATEMODE_CANCEL, 0, fx);
}
