//! The `IK2` rows of buff-like items (Plan/19 §5.3; ref/13 §10.2; `MoverSkill.cpp:1424-1727`):
//! `BUFF`, `BUFF_TOGIFT`, `BUFF2`, `AIRFUEL`, `GMTEXT`, `SKILL` and the generic timed buff of the `default` row.
//!
//! * **`BUFF` / `BUFF_TOGIFT`**: `IsDoUseBuff` ([`exclusive`]) first, then the item is applied
//!   as a buff (`DoApplySkill(this, prop, NULL)`, a [`Later`]); an angel buff item also starts a new angel
//!   (`angel::on_summon`).
//! * **`BUFF2`**: after `IsDoUseBuff` a `BUFF_ITEM2` entry with the absolute expiry `now + ability_min` minutes
//!   (collector batteries, party EXP/skill-up scrolls, `SPETGOOD`); it carries no effect of its own, other
//!   systems read it.
//! * **`AIRFUEL`**: only for a player who flies a ride. `IK3_ACCEL` sets the accelerator fuel to
//!   `a_fuel_re_max * 1000` ms; any other adds `fuel_re` to the fuel up to the ride's `f_fuel_re_max`, refused with
//!   `TID_GAME_COMBFUEL` (a board with the rocket fuel) or `TID_GAME_BOARDFUEL` (a stick with the board fuel).
//!   `SetFuel{fuel, acc_fuel}` goes to the owner.
//! * **`GMTEXT`**: a duration item (`skill_time` set) applies its buff unless one of its `IK3` runs
//!   (`TID_GAME_LIMITED_USE`); a disguise is refused while flying (`TID_PK_FLIGHT_NOUSE`) and in an instance
//!   dungeon (`TID_GAME_INSTANCE_DISGUISE01`). `II_SYS_SYS_GM_NODISGUISE` removes the disguise buffs (refused
//!   when there are none). Then the text command of the prop (`gm::item_text`: up to its second `/`; parentheses
//!   are quotes) is queued ([`Followup::ItemCommand`]) and runs through the GM registry with the issuer `Item`
//!   (PL-114: the whitelist bypasses the authority gate with `fidelity.item_gm_commands`, else the player's own
//!   authority decides). The guild-creation scroll has no name to give `/createguild`: it is refused and kept
//!   (23 §13 Q5).
//! * **`SKILL`**: the active skill of the prop fires with the chance `active_skill_rate` (always when unset) on
//!   the user (`WUI_NOW`) or on the user's selected target (`WUI_TARGETINGOBJ`).
//! * **default**: any other kind with a `skill_time` is a plain timed buff, with no exclusivity test.

use super::{Later, angel, exclusive};
use crate::buff::core::{BuffCx, RemoveFlags, add as add_buff, find_ik3, remove_buffs};
use crate::buff::set::BuffKind;
use crate::follow::Followup;
use crate::gm::item_text::{self, UsePlan};
use crate::item::env::ItemEnv;
use crate::item::fx::Fx;
use crate::item::usage::{RowCx, RowResult};
use crate::mover::Mover;
use flyff_common::defs::define::{WUI_NOW, WUI_TARGETINGOBJ};
use flyff_common::defs::item::II_SYS_SYS_GM_NODISGUISE;
use flyff_common::defs::itemkind::{
    IK2_AIRFUEL, IK2_BUFF, IK2_BUFF_TOGIFT, IK2_BUFF2, IK2_GMTEXT, IK2_SKILL, IK3_ACCEL,
    IK3_ANGEL_BUFF, IK3_BFLIGHT, IK3_BOARD, IK3_CFLIGHT, IK3_STICK, IK3_TEXT_DISGUISE,
};
use flyff_common::defs::text::{
    TID_GAME_BOARDFUEL, TID_GAME_COMBFUEL, TID_GAME_INSTANCE_DISGUISE01, TID_GAME_LIMITED_USE,
    TID_PK_FLIGHT_NOUSE,
};
use flyff_data::props::item::ItemProp;
use flyff_proto::pb;
use flyff_rules::consts::NULL_ID;

/// The row of `prop`, or `None` when the kind is not one of this module's.
pub fn dispatch(
    m: &mut Mover,
    env: &ItemEnv<'_>,
    prop: &ItemProp,
    ux: &mut RowCx<'_>,
    fx: &mut Fx,
) -> Option<RowResult> {
    match prop.item_kind2 {
        IK2_BUFF | IK2_BUFF_TOGIFT => Some(buff(m, env, prop, ux, fx)),
        IK2_BUFF2 => Some(buff2(m, env, prop, ux, fx)),
        IK2_AIRFUEL => Some(airfuel(m, env, prop, fx)),
        IK2_GMTEXT => Some(gmtext(m, env, prop, ux, fx)),
        IK2_SKILL => Some(skill(m, env, prop, ux, fx)),
        _ => None,
    }
}

/// The `default` row: a plain timed buff when the prop has a duration, else nothing happens (and the item is
/// consumed, as the original).
pub fn default_row(m: &Mover, prop: &ItemProp, fx: &mut Fx) -> RowResult {
    if prop.skill_time != NULL_ID {
        apply(m, prop.id, fx);
    }
    RowResult::Consume
}

fn apply(m: &Mover, item: u32, fx: &mut Fx) {
    fx.later(Later::ApplyItem {
        target: m.id(),
        item,
    });
}

fn buff(
    m: &mut Mover,
    env: &ItemEnv<'_>,
    prop: &ItemProp,
    ux: &RowCx<'_>,
    fx: &mut Fx,
) -> RowResult {
    if exclusive::is_do_use_buff(m, env, prop, ux.now, ux.facts, fx) != exclusive::ALLOWED {
        return RowResult::Refused;
    }
    apply(m, prop.id, fx);
    if prop.item_kind3 == IK3_ANGEL_BUFF {
        angel::on_summon(m, fx);
    }
    RowResult::Consume
}

fn buff2(
    m: &mut Mover,
    env: &ItemEnv<'_>,
    prop: &ItemProp,
    ux: &RowCx<'_>,
    fx: &mut Fx,
) -> RowResult {
    if exclusive::is_do_use_buff(m, env, prop, ux.now, ux.facts, fx) != exclusive::ALLOWED {
        return RowResult::Refused;
    }
    let expiry = env
        .now
        .0
        .saturating_add(i64::from(prop.ability_min).saturating_mul(60));
    let cx = BuffCx::new(*env, ux.now);
    add_buff(
        m,
        &cx,
        BuffKind::Item2,
        prop.id,
        u32::try_from(expiry.max(0)).unwrap_or(u32::MAX),
        0,
        None,
        fx,
    );
    RowResult::Consume
}

fn airfuel(m: &mut Mover, env: &ItemEnv<'_>, prop: &ItemProp, fx: &mut Fx) -> RowResult {
    let data = env.data();
    let Some(ride) = m.sim.params.ride else {
        return RowResult::Refused;
    };
    let Some(ride_prop) = data.item(ride.item) else {
        return RowResult::Refused;
    };
    if !m.is_player() || !m.sim.act.flags.fly() {
        return RowResult::Refused;
    }
    if prop.item_kind3 == IK3_ACCEL {
        m.sim.tm_acc_fuel = prop.a_fuel_re_max.wrapping_mul(1000) as _;
    } else {
        if ride_prop.item_kind3 == IK3_BOARD && prop.item_kind3 == IK3_CFLIGHT {
            fx.text(TID_GAME_COMBFUEL);
            return RowResult::Refused;
        }
        if ride_prop.item_kind3 == IK3_STICK && prop.item_kind3 == IK3_BFLIGHT {
            fx.text(TID_GAME_BOARDFUEL);
            return RowResult::Refused;
        }
        let fuel = m.sim.fuel.wrapping_add(prop.fuel_re as i32);
        m.sim.fuel = if (fuel as u32) >= ride_prop.f_fuel_re_max {
            ride_prop.f_fuel_re_max as i32
        } else {
            fuel
        };
    }
    fx.out.to_self(pb::SetFuel {
        fuel: m.sim.fuel,
        acc_fuel: u32::try_from(m.sim.tm_acc_fuel).unwrap_or(0),
    });
    RowResult::Consume
}

/// The command part that runs on use: the text up to its second `/` (`CString::Find("/", 1)`), parentheses
/// turned into quotes, cut to the legacy `char[64]` (`DoUseItem`, `IK2_GMTEXT`). `None` for an empty text. The
/// parser is `gm::item_text::split` (WP-GM-19).
pub fn use_command(text_file: &str) -> Option<String> {
    item_text::split(text_file).on_use
}

fn gmtext(
    m: &mut Mover,
    env: &ItemEnv<'_>,
    prop: &ItemProp,
    ux: &RowCx<'_>,
    fx: &mut Fx,
) -> RowResult {
    let data = env.data();
    // 23 §5.2 step 3 and §13 Q5: the command of the item (`gm::item_text`); a scroll whose command needs an
    // argument it cannot supply (the guild-creation scroll) is refused before anything is applied
    let plan = if m.is_player() {
        item_text::use_plan(prop)
    } else {
        UsePlan::Nothing
    };
    if let UsePlan::Refuse(tid) = plan {
        fx.text(tid);
        return RowResult::Refused;
    }
    let mut ok = true;
    if prop.skill_time != NULL_ID {
        if prop.item_kind3 == IK3_TEXT_DISGUISE {
            if m.sim.act.flags.fly() {
                fx.text(TID_PK_FLIGHT_NOUSE);
                ok = false;
            } else if ux.facts.instance_dungeon {
                fx.text(TID_GAME_INSTANCE_DISGUISE01);
                ok = false;
            }
        }
        if ok {
            if find_ik3(m, data, prop.item_kind3).is_none() {
                apply(m, prop.id, fx);
            } else {
                fx.text(TID_GAME_LIMITED_USE);
                ok = false;
            }
        }
    }
    if prop.id == II_SYS_SYS_GM_NODISGUISE {
        if find_ik3(m, data, IK3_TEXT_DISGUISE).is_some() {
            let cx = BuffCx::new(*env, ux.now);
            remove_buffs(m, &cx, RemoveFlags::IK3, IK3_TEXT_DISGUISE, fx);
        } else {
            ok = false;
        }
    }
    if !ok {
        return RowResult::Refused;
    }
    // the registry decides whether the command runs: the whitelist bypasses the authority gate with
    // `fidelity.item_gm_commands`, else the player's own authority applies (the item is consumed either way)
    if let UsePlan::Run(text) = plan {
        fx.then(Followup::ItemCommand {
            item_id: prop.id,
            player: m.id(),
            text,
        });
    }
    RowResult::Consume
}

fn skill(
    m: &mut Mover,
    env: &ItemEnv<'_>,
    prop: &ItemProp,
    ux: &mut RowCx<'_>,
    fx: &mut Fx,
) -> RowResult {
    let skill = prop.active_skill;
    if skill != NULL_ID
        && (prop.active_skill_rate == NULL_ID || ux.rng.random_below(100) < prop.active_skill_rate)
    {
        match env.data().skill(skill) {
            Some(sp) if sp.use_chance == WUI_TARGETINGOBJ => {
                // the selected target, if it is still there
                if let Some(t) = m.combat.id_set_target {
                    fx.later(Later::ItemSkill {
                        player: m.id(),
                        skill,
                        level: prop.active_skill_lv,
                        target: t,
                    });
                }
            }
            Some(sp) if sp.use_chance == WUI_NOW => {
                fx.later(Later::ItemSkill {
                    player: m.id(),
                    skill,
                    level: prop.active_skill_lv,
                    target: m.id(),
                });
            }
            Some(_) => {}
            None => {
                tracing::error!(
                    item = prop.id,
                    skill,
                    "IK2_SKILL: the active skill has no prop"
                );
            }
        }
    }
    RowResult::Consume
}
