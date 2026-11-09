//! The `IK2_SYSTEM` rows that apply buffs or depend on them (Plan/19 §5.4; ref/13 §10.3;
//! `MoverSkill.cpp:2036-2046, 2192-2226, 2239-2680, 2830-2858`). `DoUseItemSystem` answers with the result codes of
//! [`Sys`]: 0 consume, 1 not consumed (the gift box table follows), 2 "already running" (`TID_GAME_LIMITED_USE`), 3
//! "cannot overlap", 99 not consumed with a text already sent.
//!
//! | row | rule |
//! |---|---|
//! | `SMELPROT/2/3/4`, `SMELTING`, `SMELTING2`, `SUPERSMELTING`, `GLVCHEER`, `SUPERLEADERPARTY`, `FONTEDIT`, `VENDOR`, `SKLINSTRUCT`, `AWAKESAFE`, `GET01/02` | the buff of the item, 2 while the same one (`GET`: either of the two) runs. The protection and smelting buffs are consumed later by the legacy upgrade path (`item::upgrade`), `AWAKESAFE` by the awakening scrolls (12.6.4), `GET01/02` read by the drop passes (6.2) |
//! | `AMPES`, `AMPEM`, `AMPESA..E`, `AMPESS` | [`ampes`] |
//! | any `IK3_BALLOON` | the buff, 2 while a balloon buff runs |
//! | `PET_TONIC_A/B`, `PET_FEED_POCKET` | [`pets`] |
//! | `RETURN` | [`return_scroll`] |
//! | `HOLY` | the hostile buffs go (`RemoveDebufBuffs`) |
//! | `SELPAR` | a party below the maximum level becomes level 10 with 180 points; no party or a full party answers `TID_GAME_NTROUPEO10` and keeps the scroll |
//! | `II_GEN_TOO_COL_NORMALBATTERY` | refills the worn collector ([`collect::use_battery`](crate::item::collect)) |

use super::{Later, ampes, exclusive::has_item, pets, return_scroll};
use crate::buff::core::{BuffCx, RemoveFlags, find_ik3, remove_buffs};
use crate::item::collect;
use crate::item::env::ItemEnv;
use crate::item::fx::Fx;
use crate::item::usage::RowCx;
use crate::item::usage::system::{CONSUME, FALLTHROUGH, IN_USE, Sys};
use crate::mover::Mover;
use flyff_common::defs::item::*;
use flyff_common::defs::itemkind::IK3_BALLOON;
use flyff_common::defs::text::TID_GAME_NTROUPEO10;
use flyff_data::props::item::ItemProp;
use flyff_rules::social::MAX_PARTY_LEVEL;

/// The scrolls that are just a buff (2 while the same buff runs).
pub fn is_plain_buff_scroll(id: u32) -> bool {
    matches!(
        id,
        II_SYS_SYS_SCR_GLVCHEER
            | II_SYS_SYS_SCR_SMELPROT
            | II_SYS_SYS_SCR_SMELPROT2
            | II_SYS_SYS_SCR_SMELPROT3
            | II_SYS_SYS_SCR_SMELPROT4
            | II_SYS_SYS_SCR_SMELTING
            | II_SYS_SYS_SCR_SUPERLEADERPARTY
            | II_SYS_SYS_SCR_SUPERSMELTING
            | II_SYS_SYS_SCR_FONTEDIT
            | II_SYS_SYS_SCR_VENDOR
            | II_SYS_SYS_SCR_SKLINSTRUCT
            | II_SYS_SYS_SCR_SMELTING2
            | II_SYS_SYS_SCR_AWAKESAFE
    )
}

fn apply(m: &Mover, item: u32, fx: &mut Fx) {
    fx.later(Later::ApplyItem {
        target: m.id(),
        item,
    });
}

/// The row of `prop`, or `None` when it is not one of this module's.
pub fn dispatch(
    m: &mut Mover,
    env: &ItemEnv<'_>,
    prop: &ItemProp,
    ux: &mut RowCx<'_>,
    fx: &mut Fx,
) -> Option<Sys> {
    // `IK3_BALLOON` is tested after the id switch in the original, but no balloon has an id of the switch
    if prop.item_kind3 == IK3_BALLOON {
        return Some(if find_ik3(m, env.data(), IK3_BALLOON).is_some() {
            IN_USE
        } else {
            apply(m, prop.id, fx);
            CONSUME
        });
    }
    let id = prop.id;
    Some(match id {
        II_GEN_TOO_COL_NORMALBATTERY => {
            if collect::use_battery(m, env, fx) {
                CONSUME
            } else {
                FALLTHROUGH
            }
        }
        II_SYS_SYS_SCR_SELPAR => match ux.facts.party_level {
            Some(level) if level < MAX_PARTY_LEVEL => {
                fx.later(Later::PartyLevel { player: m.id() });
                CONSUME
            }
            _ => {
                fx.text(TID_GAME_NTROUPEO10);
                FALLTHROUGH
            }
        },
        II_SYS_SYS_SCR_HOLY => {
            let cx = BuffCx::new(*env, ux.now);
            remove_buffs(m, &cx, RemoveFlags::DEBUF, 0, fx);
            CONSUME
        }
        II_SYS_SYS_SCR_AMPES | II_SYS_SYS_SCR_AMPEM => ampes::use_legacy(m, prop, fx),
        _ if ampes::is_tier_scroll(id) => ampes::use_tier(m, env.data(), prop, fx),
        _ if is_plain_buff_scroll(id) => {
            if has_item(m, id) {
                IN_USE
            } else {
                apply(m, id, fx);
                CONSUME
            }
        }
        II_SYS_SYS_SCR_GET01 | II_SYS_SYS_SCR_GET02 => {
            if has_item(m, II_SYS_SYS_SCR_GET01) || has_item(m, II_SYS_SYS_SCR_GET02) {
                IN_USE
            } else {
                apply(m, id, fx);
                CONSUME
            }
        }
        II_SYS_SYS_SCR_RETURN => return_scroll::use_row(m, prop, ux, fx),
        II_SYS_SYS_SCR_PET_TONIC_A | II_SYS_SYS_SCR_PET_TONIC_B => pets::use_tonic(m, prop, ux, fx),
        II_SYS_SYS_SCR_PET_FEED_POCKET => pets::use_feed_pocket(m, env, prop, ux, fx),
        _ => return None,
    })
}
