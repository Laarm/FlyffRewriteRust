//! The EXP scrolls (Plan/19 §5.4; ref/13 §10.3; `MoverSkill.cpp:2239-2525`): `AMPES` and `AMPEM` (the legacy ones,
//! exclusive with every other EXP scroll) and the level-banded `AMPESA..E` / `AMPESS` with their tiers.
//!
//! The scroll that is used is always the base id of a family; the buff that is applied is `id`, `id + 1` or
//! `id + 2` (the first free slot among the first `max` ones), and the three slots are shared by all families:
//!
//! * the band of the scroll: A 1-20, B 21-40, C 41-60, D 61-80, E 81-120 (`AMPESS` has none); outside it the
//!   answer is 99 with `TID_GAME_EXP_NOTUSELEVEL "lo - hi"`;
//! * `AMPES` / `AMPEM` active -> 2, `AMPI` active -> 3 (the band test runs after them and may replace the code by 99);
//! * `max` tiers: 3 below level 41; 2 at 41-60 with fewer than 2 special buffs; 1 at 61-120 with none; else 3;
//! * any `AMPESS` buff active makes every other scroll answer 2; a full set answers 99 with
//!   `TID_GAME_EXP_NOTUSE "band" "max"`;
//! * on success the text "bonus N% ..." (`TID_GAME_EXPITEM_TOOLTIP`, the percent, `..._TOOLTIP1`) tells the
//!   resulting factor: 1.5 for the first tier, then 2.0 / 2.5 (below level 61 / 41, or with a special) or 1.5.
//!
//! The EXP maths of the factor itself is doc 16's (`stats::progression`, which reads these buffs).

use super::exclusive::has_item;
use crate::item::fx::Fx;
use crate::item::usage::system::{CONSUME, IN_USE, REFUSED, Sys};
use crate::mover::Mover;
use flyff_common::defs::item::*;
use flyff_common::defs::text::{
    TID_GAME_EXP_NOTUSE, TID_GAME_EXP_NOTUSELEVEL, TID_GAME_EXPITEM_TOOLTIP,
    TID_GAME_EXPITEM_TOOLTIP1,
};
use flyff_data::GameData;
use flyff_data::props::item::ItemProp;
use flyff_proto::pb;
use flyff_rules::chat::TEXT_GENERAL;
use flyff_rules::item::buffitem::{ExpScrollPlan, exp_scroll_plan};

/// Every EXP scroll buff of the legacy `AMPES` / `AMPEM` exclusivity test.
const FAMILY: [u32; 18] = [
    II_SYS_SYS_SCR_AMPES,
    II_SYS_SYS_SCR_AMPEM,
    II_SYS_SYS_SCR_AMPESA,
    II_SYS_SYS_SCR_AMPESA1,
    II_SYS_SYS_SCR_AMPESA2,
    II_SYS_SYS_SCR_AMPESB,
    II_SYS_SYS_SCR_AMPESB1,
    II_SYS_SYS_SCR_AMPESB2,
    II_SYS_SYS_SCR_AMPESC,
    II_SYS_SYS_SCR_AMPESC1,
    II_SYS_SYS_SCR_AMPESC2,
    II_SYS_SYS_SCR_AMPESD,
    II_SYS_SYS_SCR_AMPESD1,
    II_SYS_SYS_SCR_AMPESD2,
    II_SYS_SYS_SCR_AMPESS,
    II_SYS_SYS_SCR_AMPESS1,
    II_SYS_SYS_SCR_AMPESS2,
    II_SYS_SYS_SCR_AMPESE,
];

/// The level-banded base ids.
pub fn is_tier_scroll(id: u32) -> bool {
    matches!(
        id,
        II_SYS_SYS_SCR_AMPESA
            | II_SYS_SYS_SCR_AMPESB
            | II_SYS_SYS_SCR_AMPESC
            | II_SYS_SYS_SCR_AMPESD
            | II_SYS_SYS_SCR_AMPESE
            | II_SYS_SYS_SCR_AMPESS
    )
}

/// `AMPES` and `AMPEM`: 2 with any scroll of the family active, 3 with `AMPI`, else the buff.
pub fn use_legacy(m: &Mover, prop: &ItemProp, fx: &mut Fx) -> Sys {
    if FAMILY.iter().any(|&id| has_item(m, id)) {
        IN_USE
    } else if has_item(m, II_SYS_SYS_SCR_AMPI) {
        Sys::Ret(3)
    } else {
        fx.later(super::Later::ApplyItem {
            target: m.id(),
            item: prop.id,
        });
        CONSUME
    }
}

/// The tiers `AMPESx`, `AMPESx1`, `AMPESx2` of all families: which of the three slots hold a buff, how many
/// buffs there are and how many of them are special (`AMPESS*`).
struct Slots {
    taken: [bool; 3],
    count: i32,
    special: i32,
}

fn slots(m: &Mover) -> Slots {
    let mut s = Slots {
        taken: [false; 3],
        count: 0,
        special: 0,
    };
    let mut mark = |slot: usize, id: u32, special: bool| {
        if has_item(m, id) {
            s.taken[slot] = true;
            s.count += 1;
            if special {
                s.special += 1;
            }
        }
    };
    for (a, b, c) in [
        (
            II_SYS_SYS_SCR_AMPESA,
            II_SYS_SYS_SCR_AMPESA1,
            II_SYS_SYS_SCR_AMPESA2,
        ),
        (
            II_SYS_SYS_SCR_AMPESB,
            II_SYS_SYS_SCR_AMPESB1,
            II_SYS_SYS_SCR_AMPESB2,
        ),
        (
            II_SYS_SYS_SCR_AMPESC,
            II_SYS_SYS_SCR_AMPESC1,
            II_SYS_SYS_SCR_AMPESC2,
        ),
        (
            II_SYS_SYS_SCR_AMPESD,
            II_SYS_SYS_SCR_AMPESD1,
            II_SYS_SYS_SCR_AMPESD2,
        ),
    ] {
        mark(0, a, false);
        mark(1, b, false);
        mark(2, c, false);
    }
    mark(0, II_SYS_SYS_SCR_AMPESE, false);
    mark(0, II_SYS_SYS_SCR_AMPESS, true);
    mark(1, II_SYS_SYS_SCR_AMPESS1, true);
    mark(2, II_SYS_SYS_SCR_AMPESS2, true);
    s
}

/// `AMPESA..E` and `AMPESS`: see the module documentation.
pub fn use_tier(m: &Mover, data: &GameData, prop: &ItemProp, fx: &mut Fx) -> Sys {
    let level = m.mv.level;
    // the exclusivity with the legacy scrolls (2) and the "I" scroll (3); the band test may replace it by 99
    let busy: u8 = if has_item(m, II_SYS_SYS_SCR_AMPES) || has_item(m, II_SYS_SYS_SCR_AMPEM) {
        2
    } else if has_item(m, II_SYS_SYS_SCR_AMPI) {
        3
    } else {
        0
    };
    let s = slots(m);
    match exp_scroll_plan(prop.id, level, s.taken, s.count, s.special) {
        ExpScrollPlan::WrongBand { band: (lo, hi) } => {
            fx.defined(TID_GAME_EXP_NOTUSELEVEL, vec![format!("\"{lo} - {hi}\"")]);
            REFUSED
        }
        _ if busy != 0 => Sys::Ret(busy),
        ExpScrollPlan::Limited => IN_USE,
        ExpScrollPlan::Full {
            band: (lo, hi),
            max_tiers,
        } => {
            fx.defined(
                TID_GAME_EXP_NOTUSE,
                vec![format!("\"{lo} - {hi}\" \"{max_tiers}\"")],
            );
            REFUSED
        }
        ExpScrollPlan::Apply { slot, percent, .. } => {
            fx.later(super::Later::ApplyItem {
                target: m.id(),
                item: prop.id.saturating_add(slot as u32),
            });
            let text = format!(
                "{} {}% {}",
                data.texts.get(TID_GAME_EXPITEM_TOOLTIP),
                percent,
                data.texts.get(TID_GAME_EXPITEM_TOOLTIP1)
            );
            fx.out.to_self(pb::Text {
                kind: TEXT_GENERAL,
                text,
            });
            CONSUME
        }
    }
}
