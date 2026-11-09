//! `IsDoUseBuff` (Plan/19 §5.5; ref/13 §10.3; `MoverSkill.cpp:2863-3035`): when an item buff may not be used.
//!
//! The result is the original's code, `0` = allowed:
//!
//! | code | meaning | text |
//! |---|---|---|
//! | 1 | an identical buff exists (the potion kinds, the angel buff and `BUFF_TOGIFT`: one of the same `IK3`) | `TID_GAME_LIMITED_USE` |
//! | 2 | angel buff: the PK value is 0 or less | `TID_GAME_PKVALUE_NOT_ANGEL` |
//! | 3 | angel buff: the PK-minus quest is not active | `TID_GAME_NOT_QUEST_ANGEL` |
//! | 4 | angel buff: a finished angel item is in the bag | `TID_GAME_NOT_COMPLETE_ANGEL` |
//! | 5 | angel buff: `events.pk` is off (KQ-17, the repack's `__PKSERVER_USE_ANGEL`) | none |
//!
//! The assignments overwrite each other in the original's order (a later code wins). The branches are an
//! `else if` chain: the potion/angel/gift group, `DEFENSE20/50` (exclusive), `PET_FEED_POCKET02` (blocked while
//! an unexpired timed feed pocket is in the bag or when the remaining time plus the new one would overflow the
//! `int` of the original), the plain "the same item buff exists", the collector batteries (exclusive among
//! themselves, `BUFF_ITEM2`) and the party EXP-up / skill-up scrolls and `SPETGOOD` (`BUFF_ITEM2` groups).

use crate::buff::core::find_ik3;
use crate::buff::set::{BuffKey, BuffKind};
use crate::item::env::ItemEnv;
use crate::item::fx::Fx;
use crate::item::usage::UseFacts;
use crate::mover::Mover;
use flyff_common::GameMs;
use flyff_common::defs::item::*;
use flyff_common::defs::itemkind::{
    IK2_BUFF_TOGIFT, IK3_ANGEL_BUFF, IK3_POTION_BUFF_DEFENSE, IK3_POTION_BUFF_DEX,
    IK3_POTION_BUFF_INT, IK3_POTION_BUFF_STA, IK3_POTION_BUFF_STR,
};
use flyff_common::defs::quest::QUEST_PKMINUS;
use flyff_common::defs::text::{
    TID_GAME_LIMITED_USE, TID_GAME_NOT_COMPLETE_ANGEL, TID_GAME_NOT_QUEST_ANGEL,
    TID_GAME_PKVALUE_NOT_ANGEL,
};
use flyff_data::props::item::ItemProp;

/// The result code of an allowed use.
pub const ALLOWED: u8 = 0;

/// `HasBuff(BUFF_ITEM, id)`.
pub fn has_item(m: &Mover, id: u32) -> bool {
    m.buffs.contains(BuffKey::new(BuffKind::Item, id))
}

/// `HasBuff(BUFF_ITEM2, id)`.
pub fn has_item2(m: &Mover, id: u32) -> bool {
    m.buffs.contains(BuffKey::new(BuffKind::Item2, id))
}

/// `true` when the buff `key` plus `skill_time` more would overflow the `int` of the original's test
/// `(int)skill_time > (int)(skill_time + remaining)` (`PET_FEED_POCKET02`, the pet tonics). `false` without the
/// buff.
pub fn would_overflow(m: &Mover, key: BuffKey, skill_time: u32, now: GameMs) -> bool {
    let Some(e) = m.buffs.get(key) else {
        return false;
    };
    let sum = skill_time.wrapping_add(e.remain_ms(now));
    (skill_time as i32) > (sum as i32)
}

/// `IsDoUseBuff(prop)` without the texts: the code, see the module documentation.
pub fn code(m: &Mover, env: &ItemEnv<'_>, prop: &ItemProp, now: GameMs, facts: &UseFacts) -> u8 {
    let data = env.data();
    let mut result = ALLOWED;
    let ik3 = prop.item_kind3;
    let id = prop.id;
    if matches!(
        ik3,
        IK3_POTION_BUFF_STR
            | IK3_POTION_BUFF_DEX
            | IK3_POTION_BUFF_INT
            | IK3_POTION_BUFF_STA
            | IK3_POTION_BUFF_DEFENSE
            | IK3_ANGEL_BUFF
    ) || prop.item_kind2 == IK2_BUFF_TOGIFT
    {
        if find_ik3(m, data, ik3).is_some() {
            result = 1;
        }
        if ik3 == IK3_ANGEL_BUFF {
            if facts.pk_server {
                if m.player().is_none_or(|p| p.pk.value <= 0) {
                    result = 2;
                }
                if m.player()
                    .is_none_or(|p| p.quests.find(QUEST_PKMINUS).is_none())
                {
                    result = 3;
                }
                if has_finished_angel(m) {
                    result = 4;
                }
            } else {
                result = 5;
            }
        }
    } else if id == II_SYS_SYS_SCR_DEFENSE20 || id == II_SYS_SYS_SCR_DEFENSE50 {
        if has_item(m, II_SYS_SYS_SCR_DEFENSE20) || has_item(m, II_SYS_SYS_SCR_DEFENSE50) {
            result = 1;
        }
    } else if id == II_SYS_SYS_SCR_PET_FEED_POCKET02 {
        let timed_pocket = m.player().is_some_and(|p| {
            p.items.inventory.iter().any(|(_, it)| {
                it.is_usable()
                    && it.item_id == II_SYS_SYS_SCR_PET_FEED_POCKET
                    && it.keep_until > 0
                    && !it.is_expired()
            })
        });
        if timed_pocket || would_overflow(m, BuffKey::new(BuffKind::Item, id), prop.skill_time, now)
        {
            result = 1;
        }
    } else if has_item(m, id) {
        result = 1;
    } else if matches!(
        id,
        II_GEN_TOO_COL_SILVERBATTERY | II_GEN_TOO_COL_GOLDBATTERY | II_GEN_TOO_COL_BATTERY001
    ) {
        if [
            II_GEN_TOO_COL_SILVERBATTERY,
            II_GEN_TOO_COL_GOLDBATTERY,
            II_GEN_TOO_COL_BATTERY001,
        ]
        .iter()
        .any(|&b| has_item2(m, b))
        {
            result = 1;
        }
    } else if matches!(
        id,
        II_SYS_SYS_SCR_PARTYEXPUP01 | II_SYS_SYS_SCR_PARTYEXPUP02 | II_SYS_SYS_SCR_PARTYEXPUP01_01
    ) {
        if [
            II_SYS_SYS_SCR_PARTYEXPUP01,
            II_SYS_SYS_SCR_PARTYEXPUP02,
            II_SYS_SYS_SCR_PARTYEXPUP01_01,
        ]
        .iter()
        .any(|&b| has_item2(m, b))
        {
            result = 1;
        }
    } else if id == II_SYS_SYS_SCR_SPETGOOD {
        if has_item2(m, II_SYS_SYS_SCR_SPETGOOD) {
            result = 1;
        }
    } else if matches!(
        id,
        II_SYS_SYS_SCR_PARTYSKILLUP01
            | II_SYS_SYS_SCR_PARTYSKILLUP02
            | II_SYS_SYS_SCR_PARTYSKILLUP01_01
    ) && [
        II_SYS_SYS_SCR_PARTYSKILLUP01,
        II_SYS_SYS_SCR_PARTYSKILLUP02,
        II_SYS_SYS_SCR_PARTYSKILLUP01_01,
    ]
    .iter()
    .any(|&b| has_item2(m, b))
    {
        result = 1;
    }
    result
}

/// A finished angel item (the 100 % one of any colour) is in the bag.
fn has_finished_angel(m: &Mover) -> bool {
    m.player().is_some_and(|p| {
        [
            II_SYS_SYS_QUE_ANGEL_RED100,
            II_SYS_SYS_QUE_ANGEL_BLUE100,
            II_SYS_SYS_QUE_ANGEL_GREEN100,
            II_SYS_SYS_QUE_ANGEL_WHITE100,
        ]
        .iter()
        .any(|&i| p.items.inventory.count_of(i) > 0)
    })
}

/// The text of a refusal code (`AddDefinedText` of the switch at the end of `IsDoUseBuff`; 5 says nothing).
pub fn tell(code: u8, fx: &mut Fx) {
    match code {
        1 => fx.text(TID_GAME_LIMITED_USE),
        2 => fx.text(TID_GAME_PKVALUE_NOT_ANGEL),
        3 => fx.text(TID_GAME_NOT_QUEST_ANGEL),
        4 => fx.text(TID_GAME_NOT_COMPLETE_ANGEL),
        _ => {}
    }
}

/// `IsDoUseBuff(prop)`: the code, after queuing its text.
pub fn is_do_use_buff(
    m: &Mover,
    env: &ItemEnv<'_>,
    prop: &ItemProp,
    now: GameMs,
    facts: &UseFacts,
    fx: &mut Fx,
) -> u8 {
    let c = code(m, env, prop, now, facts);
    tell(c, fx);
    c
}
