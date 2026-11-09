//! The SM premium timers (Plan/16 §14, WP-MOVER-16; `Mover.cpp:690-975`, `ObjSerializeOpt.cpp:64-94`).
//!
//! `PlayerExt.sm` holds 26 numbers: an absolute expiry in unix seconds (shout, bank, party skill, escape), a
//! counter of online seconds (HP foods, MP / FP maintenance, action point mode, attack up, the element stones,
//! velocity jump) or the revival flag. The pure arithmetic is `flyff_rules::sm`; this module is the mover-level
//! face: [`set`], the once-a-second [`tick_second`] (step 4 of the per-player 1 s timer, 16 §6.4), [`clear_all`],
//! the join application [`apply_at_join`], the join message [`SmModeAllStep`] and the food row
//! [`use_sm_food`].
//!
//! The effect of an SM on the DST aggregate (the HP foods, the velocity scroll) is a ledger source
//! `DstSource::Sm { sm }`, so the expiry removes exactly what was added. Notifications to the social services
//! (the party skill modes) are queued in `SmState.events` - the tick tail has no services - and forwarded by
//! [`drain_events`].

use super::dst::DstSource;
use super::env::{StatsCore, origin_max};
use super::out::Out;
use super::pool::set_point_param;
use super::source::{apply_source, remove_source};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::env::ItemEnv;
use crate::item::fx::Fx;
use crate::mover::Mover;
use crate::outbox::Outbox;
use crate::player::SmEvent;
use crate::world::join::{JoinContributor, JoinCtx, JoinStep};
use flyff_common::defs::item::{
    II_CHR_FOO_COO_BULLHAMS, II_CHR_FOO_COO_GRILLEDEEL, II_SYS_SYS_SCR_VELOCIJUMP,
};
use flyff_common::defs::neuz::{PARTS_LWEAPON, PARTS_RWEAPON, PARTS_UPPER_BODY};
use flyff_common::defs::text::TID_GAME_LIMITED_USE;
use flyff_data::props::item::ItemProp;
use flyff_proto::pb;
use flyff_rules::consts::dst::*;
use flyff_rules::consts::{
    NULL_CHGPARAM, NULL_ID, SM_MAINTAIN_FP, SM_MAINTAIN_MP, SM_MAX, SM_MAX_HP, SM_MAX_HP50,
    SM_PARTYSKILL1, SM_PARTYSKILL15, SM_PARTYSKILL30, SM_RESIST_ATTACK_LEFT,
    SM_RESIST_ATTACK_RIGHT, SM_RESIST_DEFENSE, SM_VELOCIJUMP,
};
use flyff_rules::sm::{stored_value, tick};

pub use super::env::sm_active;

/// `IsShoutFull()`: a shout timer is running.
pub fn is_shout_full(m: &Mover) -> bool {
    use flyff_rules::consts::{SM_SHOUT001, SM_SHOUT15, SM_SHOUT30};
    [SM_SHOUT15, SM_SHOUT30, SM_SHOUT001]
        .into_iter()
        .any(|s| sm_active(m, s))
}

/// `IsCommBank()`: a remote-bank timer is running.
pub fn is_comm_bank(m: &Mover) -> bool {
    use flyff_rules::consts::{SM_BANK001, SM_BANK15, SM_BANK30};
    [SM_BANK15, SM_BANK30, SM_BANK001]
        .into_iter()
        .any(|s| sm_active(m, s))
}

/// A party-skill mode is active (`SM_PARTYSKILL1`, `15` or `30`).
pub fn is_party_skill(m: &Mover) -> bool {
    [SM_PARTYSKILL1, SM_PARTYSKILL15, SM_PARTYSKILL30]
        .into_iter()
        .any(|s| sm_active(m, s))
}

fn mode_msg(sm: usize, value: u32) -> pb::SmMode {
    pb::SmMode {
        sm_type: sm as i32,
        value: i64::from(value),
    }
}

/// `SetSMMode(sm, seconds)`: absolute kinds store `now + seconds`, counters the number itself; `SmMode` goes to
/// the player. `now_unix` is the tick's wall-clock sample.
pub fn set(m: &mut Mover, sm: usize, seconds: u32, now_unix: i64, out: &mut Out) {
    let Some(p) = m.player_mut() else { return };
    let Some(slot) = p.sm.timers.get_mut(sm) else {
        return;
    };
    *slot = stored_value(sm, seconds, now_unix);
    let value = *slot;
    p.sm.events.push(SmEvent {
        sm: sm as u8,
        active: value > 0,
    });
    out.to_self(mode_msg(sm, value));
}

/// The element stone of an SM slot ends: the resist byte is reset, the stone is taken off the item and the
/// client is told (`CommercialElemNtf{obj_id, 0}`).
fn end_stone(m: &mut Mover, sm: usize, out: &mut Out) {
    let (part, byte): (u32, fn(&mut crate::player::SmState) -> &mut u8) = match sm {
        SM_RESIST_ATTACK_LEFT => (PARTS_LWEAPON, |s| &mut s.attack_resist_left),
        SM_RESIST_ATTACK_RIGHT => (PARTS_RWEAPON, |s| &mut s.attack_resist_right),
        _ => (PARTS_UPPER_BODY, |s| &mut s.defense_resist),
    };
    let Some(p) = m.player_mut() else { return };
    *byte(&mut p.sm) = 0xFF;
    let Some(id) = p.items.inventory.equip_id(part) else {
        return;
    };
    if let Some(it) = p.items.inventory.get_at_id_mut(id) {
        it.element_stone_id = 0;
        out.to_self(pb::CommercialElemNtf {
            item_obj_id: u32::from(id),
            resist_sm_item_id: 0,
        });
    }
}

/// The expiry actions of one timer (16 §14.2), then `SmMode{sm, 0}`. The value is already 0.
fn expire(m: &mut Mover, c: &StatsCore, sm: usize, out: &mut Out) {
    match sm {
        SM_RESIST_ATTACK_LEFT | SM_RESIST_ATTACK_RIGHT | SM_RESIST_DEFENSE => {
            end_stone(m, sm, out);
        }
        SM_MAX_HP | SM_MAX_HP50 | SM_VELOCIJUMP => {
            remove_source(m, c, DstSource::Sm { sm: sm as u8 }, true, out);
        }
        _ => {}
    }
    if let Some(p) = m.player_mut() {
        p.sm.events.push(SmEvent {
            sm: sm as u8,
            active: false,
        });
    }
    out.to_self(mode_msg(sm, 0));
}

/// `SubSMMode()` (16 §14.2): one pass over the 26 timers in index order. `now_unix` is the wall-clock sample of
/// the tick; the messages of a pass that expires a timer come in the order of the indices.
pub fn tick_second(m: &mut Mover, c: &StatsCore, now_unix: i64, out: &mut Out) {
    for sm in 0..SM_MAX {
        let Some(value) = m.player().and_then(|p| p.sm.timers.get(sm).copied()) else {
            return;
        };
        if value == 0 {
            continue;
        }
        let t = tick(sm, value, now_unix);
        if let Some(p) = m.player_mut()
            && let Some(slot) = p.sm.timers.get_mut(sm)
        {
            *slot = t.value;
        }
        if t.expired {
            expire(m, c, sm, out);
        }
    }
}

/// `ClearAllSMMode()` (GM, 23): every active timer ends with its expiry actions.
pub fn clear_all(m: &mut Mover, c: &StatsCore, out: &mut Out) {
    for sm in 0..SM_MAX {
        let active = m
            .player()
            .and_then(|p| p.sm.timers.get(sm).copied())
            .is_some_and(|v| v > 0);
        if !active {
            continue;
        }
        if let Some(p) = m.player_mut()
            && let Some(slot) = p.sm.timers.get_mut(sm)
        {
            *slot = 0;
        }
        expire(m, c, sm, out);
    }
}

/// `OnApplySM()` (16 §14.3): the DSTs of the active HP foods and the velocity scroll after a load, silently
/// (the spawn snapshot carries the result).
pub fn apply_at_join(m: &mut Mover, c: &StatsCore) {
    let mut out = Out::new();
    let (hp, hp50, jump, plus) = match m.player() {
        Some(p) => (
            p.sm.timers[SM_MAX_HP] > 0,
            p.sm.timers[SM_MAX_HP50] > 0,
            p.sm.timers[SM_VELOCIJUMP] > 0,
            p.sm.plus_max_hp,
        ),
        None => return,
    };
    if hp && let Some(it) = c.data.item(II_CHR_FOO_COO_BULLHAMS) {
        apply_source(
            m,
            c,
            DstSource::Sm {
                sm: SM_MAX_HP as u8,
            },
            &[(it.dest_param[0], it.adj_param_val[1], NULL_CHGPARAM)],
            false,
            &mut out,
        );
    }
    if hp50 && let Some(it) = c.data.item(II_CHR_FOO_COO_GRILLEDEEL) {
        apply_source(
            m,
            c,
            DstSource::Sm {
                sm: SM_MAX_HP50 as u8,
            },
            &[(it.dest_param[0], plus, NULL_CHGPARAM)],
            false,
            &mut out,
        );
    }
    if jump && let Some(it) = c.data.item(II_SYS_SYS_SCR_VELOCIJUMP) {
        let slots = velocity_slots(it, NULL_CHGPARAM);
        apply_source(
            m,
            c,
            DstSource::Sm {
                sm: SM_VELOCIJUMP as u8,
            },
            &slots,
            false,
            &mut out,
        );
    }
}

/// The two DST slots of the velocity scroll; `chg` is the change value to pass (`NULL_CHGPARAM` at join, the
/// adjustment itself when the scroll is read).
fn velocity_slots(it: &ItemProp, chg: i32) -> Vec<(u32, i32, i32)> {
    (0..2)
        .filter(|&i| it.dest_param[i] != NULL_ID)
        .map(|i| {
            let adj = it.adj_param_val[i];
            (
                it.dest_param[i],
                adj,
                if chg == NULL_CHGPARAM { chg } else { adj },
            )
        })
        .collect()
}

/// What an SM food or scroll did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmUse {
    /// The effect took hold: consume the item.
    Used,
    /// The mode is already running (`TID_GAME_LIMITED_USE` was queued): the item is not consumed.
    Refused,
    /// The item is none of the SM foods.
    NotMine,
}

/// `DoUseItemFood_SM` (`MoverSkill.cpp:3039-3070`; 16 §14.4): refused while `SM_MAX_HP` or `SM_MAX_HP50` runs.
/// The bull hams add `adj_param_val[1]` (300) of max HP, the grilled eel a share of the origin max HP,
/// `origin / (100 / adj_param_val[0])` (integers), kept in `plus_max_hp`; the effect is a ledger source `Sm`,
/// the timer is `circle_time` seconds.
pub fn use_sm_food(m: &mut Mover, env: &ItemEnv<'_>, prop: &ItemProp, fx: &mut Fx) -> SmUse {
    let c = env.stats();
    let (sm, adj) = match prop.id {
        II_CHR_FOO_COO_BULLHAMS => (SM_MAX_HP, prop.adj_param_val[1]),
        II_CHR_FOO_COO_GRILLEDEEL => {
            let div = if prop.adj_param_val[0] == 0 {
                1
            } else {
                100 / prop.adj_param_val[0]
            };
            let div = if div == 0 { 1 } else { div };
            (SM_MAX_HP50, origin_max(m, c).hp / div)
        }
        _ => return SmUse::NotMine,
    };
    if sm_active(m, SM_MAX_HP) || sm_active(m, SM_MAX_HP50) {
        fx.text(TID_GAME_LIMITED_USE);
        return SmUse::Refused;
    }
    if sm == SM_MAX_HP50
        && let Some(p) = m.player_mut()
    {
        p.sm.plus_max_hp = adj;
    }
    apply_source(
        m,
        c,
        DstSource::Sm { sm: sm as u8 },
        &[(prop.dest_param[0], adj, adj)],
        true,
        &mut fx.out,
    );
    set(m, sm, prop.circle_time, env.now.0, &mut fx.out);
    SmUse::Used
}

/// The velocity scroll (`II_SYS_SYS_SCR_VELOCIJUMP`): the two DST slots (with `chg` = `adj`) and the timer;
/// refused while it runs (result 2).
pub fn use_velocity_jump(m: &mut Mover, env: &ItemEnv<'_>, prop: &ItemProp, fx: &mut Fx) -> SmUse {
    if sm_active(m, SM_VELOCIJUMP) {
        return SmUse::Refused;
    }
    let slots = velocity_slots(prop, 0);
    apply_source(
        m,
        env.stats(),
        DstSource::Sm {
            sm: SM_VELOCIJUMP as u8,
        },
        &slots,
        true,
        &mut fx.out,
    );
    set(m, SM_VELOCIJUMP, prop.circle_time, env.now.0, &mut fx.out);
    SmUse::Used
}

/// A MP / FP maintenance item (`IK2_REFRESHER` / `IK2_POTION` with a `circle_time`): refused with
/// `TID_GAME_LIMITED_USE` while the mode runs; else the mode starts and the pool is filled. `sm` is
/// `SM_MAINTAIN_MP` (refresher) or `SM_MAINTAIN_FP` (potion).
pub fn use_maintain(
    m: &mut Mover,
    env: &ItemEnv<'_>,
    prop: &ItemProp,
    sm: usize,
    fx: &mut Fx,
) -> SmUse {
    debug_assert!(sm == SM_MAINTAIN_MP || sm == SM_MAINTAIN_FP);
    if sm_active(m, sm) {
        fx.text(TID_GAME_LIMITED_USE);
        return SmUse::Refused;
    }
    let c = env.stats();
    set(m, sm, prop.circle_time, env.now.0, &mut fx.out);
    let dst = if sm == SM_MAINTAIN_MP { DST_MP } else { DST_FP };
    let max = super::env::max_of(m, c, dst);
    set_point_param(m, c, dst, max, false, &mut fx.out);
    SmUse::Used
}

// ---- the element stones of equipment ----------------------------------------------------------------------------------

/// The element (`SAI79::ePropType`: 1 fire, 2 water, 3 electricity, 4 wind, 5 earth) of a commercial element
/// stone, `0` for any other id (`DoSMItemEquip`'s `nResist` default).
fn stone_element(stone: u32) -> u8 {
    use flyff_common::defs::item::*;
    match stone {
        II_CHR_SYS_SCR_FIREASTONE | II_CHR_SYS_SCR_DEFIREASTONE => 1,
        II_CHR_SYS_SCR_WATEILSTONE | II_CHR_SYS_SCR_DEWATEILSTONE => 2,
        II_CHR_SYS_SCR_LIGHTINESTONE | II_CHR_SYS_SCR_DELIGHTINESTONE => 3,
        II_CHR_SYS_SCR_WINDYOSTONE | II_CHR_SYS_SCR_DEWINDYOSTONE => 4,
        II_CHR_SYS_SCR_EARTHYSTONE | II_CHR_SYS_SCR_DEEARTHYSTONE => 5,
        _ => 0,
    }
}

/// `true` for the five attack stones.
fn is_attack_stone(stone: u32) -> bool {
    use flyff_common::defs::item::*;
    matches!(
        stone,
        II_CHR_SYS_SCR_FIREASTONE
            | II_CHR_SYS_SCR_WATEILSTONE
            | II_CHR_SYS_SCR_WINDYOSTONE
            | II_CHR_SYS_SCR_LIGHTINESTONE
            | II_CHR_SYS_SCR_EARTHYSTONE
    )
}

/// `true` for the five defense stones.
fn is_defense_stone(stone: u32) -> bool {
    use flyff_common::defs::item::*;
    matches!(
        stone,
        II_CHR_SYS_SCR_DEFIREASTONE
            | II_CHR_SYS_SCR_DEWATEILSTONE
            | II_CHR_SYS_SCR_DEWINDYOSTONE
            | II_CHR_SYS_SCR_DELIGHTINESTONE
            | II_CHR_SYS_SCR_DEEARTHYSTONE
    )
}

/// `CUser::DoSMItemEquip` (`User.cpp:7398`): a worn item carries a commercial element stone, so the matching
/// resist byte is set for the part (left weapon, right weapon, anything else is the defense byte), the
/// `ResistSmMode` goes to the player and the timer of the slot starts with the `circle_time` of the stone. A
/// stone without a prop does nothing.
pub fn stone_equip(
    m: &mut Mover,
    env: &ItemEnv<'_>,
    id: crate::item::container::StorageId,
    part: u32,
    out: &mut Out,
) {
    let Some(stone) = m
        .player()
        .and_then(|p| p.items.inventory.get_at_id(id))
        .map(|it| it.element_stone_id)
        .filter(|&s| s != 0)
    else {
        return;
    };
    let Some(prop) = env.data().item(stone) else {
        return;
    };
    let element = stone_element(stone);
    let slot = match part {
        PARTS_LWEAPON => SM_RESIST_ATTACK_LEFT,
        PARTS_RWEAPON => SM_RESIST_ATTACK_RIGHT,
        _ => SM_RESIST_DEFENSE,
    };
    if let Some(p) = m.player_mut() {
        match slot {
            SM_RESIST_ATTACK_LEFT => p.sm.attack_resist_left = element,
            SM_RESIST_ATTACK_RIGHT => p.sm.attack_resist_right = element,
            _ => p.sm.defense_resist = element,
        }
    }
    if let Some(msg) = resist_sm_mode(m) {
        out.to_self(msg);
    }
    set(m, slot, prop.circle_time, env.now.0, out);
}

/// `CUser::DoSMItemUnEquip` (`User.cpp:7484`): the stone of a worn item that comes off. An attack stone
/// shortens the timer of its hand to one second (the left weapon: `SM_RESIST_ATTACK_LEFT`, any other part
/// `SM_RESIST_ATTACK_RIGHT`), a defense stone that of `SM_RESIST_DEFENSE`; the item loses the stone and the
/// client is told (`CommercialElemNtf{id, 0}`). Any other stone id is left alone.
pub fn stone_unequip(
    m: &mut Mover,
    id: crate::item::container::StorageId,
    part: u32,
    out: &mut Out,
) {
    let Some(stone) = m
        .player()
        .and_then(|p| p.items.inventory.get_at_id(id))
        .map(|it| it.element_stone_id)
        .filter(|&s| s != 0)
    else {
        return;
    };
    let slot = if is_attack_stone(stone) {
        if part == PARTS_LWEAPON {
            SM_RESIST_ATTACK_LEFT
        } else {
            SM_RESIST_ATTACK_RIGHT
        }
    } else if is_defense_stone(stone) {
        SM_RESIST_DEFENSE
    } else {
        return;
    };
    // `SetSMMode(slot, 1)` on a counter: one more online second
    set(m, slot, 1, 0, out);
    if let Some(it) = m
        .player_mut()
        .and_then(|p| p.items.inventory.get_at_id_mut(id))
    {
        it.element_stone_id = 0;
    }
    out.to_self(pb::CommercialElemNtf {
        item_obj_id: u32::from(id),
        resist_sm_item_id: 0,
    });
}

// ---- messages -------------------------------------------------------------------------------------------------------

/// `SmModeAll{entries}`: the active timers in index order, `None` when there is none (the join sends
/// nothing then, 11 §2.5 last paragraph).
pub fn sm_mode_all(m: &Mover) -> Option<pb::SmModeAll> {
    let p = m.player()?;
    let entries: Vec<pb::SmModeEntry> =
        p.sm.timers
            .iter()
            .enumerate()
            .filter(|(_, v)| **v > 0)
            .map(|(sm, &v)| pb::SmModeEntry {
                sm_type: sm as i32,
                value: i64::from(v),
            })
            .collect();
    (!entries.is_empty()).then_some(pb::SmModeAll { entries })
}

/// `ResistSmMode{atk_left, atk_right, def}` (self): emitted whenever a stone is applied (doc 19 builds it
/// from here).
pub fn resist_sm_mode(m: &Mover) -> Option<pb::ResistSmMode> {
    let p = m.player()?;
    Some(pb::ResistSmMode {
        attack_resist_left: u32::from(p.sm.attack_resist_left),
        attack_resist_right: u32::from(p.sm.attack_resist_right),
        defense_resist: u32::from(p.sm.defense_resist),
    })
}

/// The join step `SmModeAll` (16 §14.3): after `PartyMember`, only when a timer runs.
pub struct SmModeAllStep;

impl JoinContributor for SmModeAllStep {
    fn step(&self) -> JoinStep {
        JoinStep::SmModeAll
    }

    fn write(&self, cx: &JoinCtx<'_>, out: &mut Outbox) {
        if let Some(m) = cx.worlds.mover(cx.player)
            && let Some(msg) = sm_mode_all(m)
        {
            out.to_session(cx.session, 0, msg);
        }
    }
}

/// Forwards the queued SM changes of the players to the social services (the party skill modes). Called every
/// tick by the buff pass.
pub fn drain_events(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    let pending: Vec<(flyff_common::ObjId, Vec<SmEvent>)> = ctx
        .worlds
        .store_mut()
        .movers
        .iter_mut()
        .filter_map(|(_, m)| {
            let id = m.id();
            let p = m.player_mut()?;
            (!p.sm.events.is_empty()).then(|| (id, std::mem::take(&mut p.sm.events)))
        })
        .collect();
    for (id, events) in pending {
        for e in events {
            if matches!(
                e.sm as usize,
                SM_PARTYSKILL1 | SM_PARTYSKILL15 | SM_PARTYSKILL30
            ) {
                crate::social::hooks::on_sm_mode_changed(svc, ctx, PlayerRef(id), e.sm, e.active);
            }
        }
    }
}
