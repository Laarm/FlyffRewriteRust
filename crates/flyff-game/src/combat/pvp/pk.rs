//! Pink, chaotic and propensity (Plan/17 §8.3; `__S8_PK`, `PKSetting.inc`; `Mover.cpp:5679-5840`, `MoverAttack.cpp:
//! 1895-1915`, `User.cpp:557-565`, `MoverParam.cpp:1285-1310`).
//!
//! * **Pink**: a PK hit makes the attacker pink for `general_attack_s` (16 s), for `ready_attack_s` (8 s) when the
//!   victim is pink itself; helping a pink player (or a chaotic one with a beneficial skill) makes the helper pink
//!   for 8 s. The window only extends. The expiry is checked in the tick tail of the player ([`expire_pink`]) and
//!   announced with `PkRelation{PINK, 0}`.
//! * **Chaotic** is `propensity > 0`. A kill raises `pk_value` by one and the propensity by `60 * pk_value^2`
//!   ([`next_pk_propensity`]); the C++ setters ignore a result that overflows `int` ([`set_pk_value`],
//!   [`set_pk_propensity`]).
//! * **Restrictions** by propensity stage ([`chaotic_restriction`]): shop, bank, flying, guard attack, vendor.

use super::PvpEnv;
use crate::combat::env::ArbiterCx;
use crate::mover::Mover;
use crate::stats::out::Out;
use flyff_common::GameMs;
use flyff_common::ObjId;
use flyff_common::defs::world::WI_WORLD_ARENA;
use flyff_data::rules::pk_setting::{PkSetting, PropensityPenalty};
use flyff_proto::pb;

/// `IsPKPink()`: `m_dwPKTime > 0`.
pub fn is_pink(m: &Mover) -> bool {
    m.combat.pk.pink_until.is_some()
}

/// `SetPKPink(until)`: only ever extends the window.
pub fn set_pink(m: &mut Mover, until: GameMs) {
    let t = &mut m.combat.pk.pink_until;
    if t.is_none_or(|cur| cur < until) {
        *t = Some(until);
    }
}

/// The expiry of `CUser::Process` (`if IsPKPink() && now > m_dwPKTime { SetPKPink(0); AddPKPink(0) }`): `true` when
/// the pink window ended now (the caller broadcasts [`pink_relation`]`(false)`).
pub fn expire_pink(m: &mut Mover, now: GameMs) -> bool {
    match m.combat.pk.pink_until {
        Some(t) if now > t => {
            m.combat.pk.pink_until = None;
            true
        }
        _ => false,
    }
}

/// `PkRelation{PINK, on}` (audience: near, the subject included).
pub fn pink_relation(on: bool) -> pb::PkRelation {
    pb::PkRelation {
        sub: Some(pb::pk_relation::Sub::Pink(pb::pk_relation::PkPink {
            pink: u32::from(on),
        })),
    }
}

/// `PkRelation{PROPENSITY, value}` (audience: near, the subject included).
pub fn propensity_relation(propensity: u32) -> pb::PkRelation {
    pb::PkRelation {
        sub: Some(pb::pk_relation::Sub::Propensity(
            pb::pk_relation::PkPropensity { propensity },
        )),
    }
}

/// `PkRelation{PKVALUE, value}` (audience: the subject only).
pub fn pk_value_relation(pk_value: i32) -> pb::PkRelation {
    pb::PkRelation {
        sub: Some(pb::pk_relation::Sub::PkValue(pb::pk_relation::PkPkValue {
            pk_value,
        })),
    }
}

/// `OnAttacked` of a `HITTYPE_PK` hit (the caller has checked: not a reflected hit, neither side chaotic): the
/// attacker becomes pink, except in the arena. The window is `ready_attack_s` when the victim is pink,
/// `general_attack_s` otherwise.
pub fn on_pk_hit(cx: &mut ArbiterCx<'_>, a_id: ObjId, d_id: ObjId) {
    let env = cx.env.pvp;
    let now = cx.w.now;
    let Some(def) = cx.w.store.mover(d_id) else {
        return;
    };
    if def.core.world.0 == WI_WORLD_ARENA {
        return;
    }
    let secs = if is_pink(def) {
        env.ready_attack_s
    } else {
        env.general_attack_s
    };
    let until = GameMs(now.0 + u64::from(secs) * 1000);
    if let Some(att) = cx.w.store.mover_mut(a_id) {
        set_pink(att, until);
        cx.fx.near(a_id, pink_relation(true));
    }
}

/// The pink rule of helping (`DoUseSkill` step 22, `MoverSkill.cpp:1061-1068`; Plan/17 §6.3): with `EVE_PK`, a
/// caster that is not chaotic and whose target is pink - or chaotic, for a beneficial skill (`evildoing > 0`) -
/// becomes pink for `ready_attack_s`. Returns the announcement when the caster became pink.
pub fn help_pink(
    env: &PvpEnv,
    caster: &mut Mover,
    target: &Mover,
    evildoing: i32,
    now: GameMs,
) -> Option<pb::PkRelation> {
    if !env.pk {
        return None;
    }
    let caster_chaotic = crate::combat::state::is_chaotic(caster);
    if caster_chaotic {
        return None;
    }
    let target_chaotic = crate::combat::state::is_chaotic(target);
    if is_pink(target) || (target_chaotic && evildoing > 0) {
        set_pink(caster, GameMs(now.0 + u64::from(env.ready_attack_s) * 1000));
        return Some(pink_relation(true));
    }
    None
}

/// `NextPKPropensity(pk_value)` = `60 * v * v` as a C `int`; an overflow (negative result) gives `0x7fffffff`.
pub fn next_pk_propensity(pk_value: i32) -> u32 {
    let n = 60_i32.wrapping_mul(pk_value).wrapping_mul(pk_value);
    if n < 0 { 0x7fff_ffff } else { n as u32 }
}

/// `SetPKValue(v)`: ignored when `v` is negative (an `int` overflow).
pub fn set_pk_value(m: &mut Mover, v: i32) {
    if v >= 0
        && let Some(p) = m.player_mut()
    {
        p.pk.value = v;
    }
}

/// `SetPKPropensity(v)`: ignored when `v` does not fit a non-negative `int` (an overflow).
pub fn set_pk_propensity(m: &mut Mover, v: u32) {
    if (v as i32) >= 0
        && let Some(p) = m.player_mut()
    {
        p.pk.propensity = v;
    }
}

/// `GetPropensityPenalty(propensity)`: the first stage whose range holds the value, else the last stage (8).
pub fn propensity_penalty(pk: &PkSetting, propensity: u32) -> Option<&PropensityPenalty> {
    let p = i64::from(propensity);
    pk.penalties
        .iter()
        .find(|r| i64::from(r.min) <= p && p <= i64::from(r.max))
        .or_else(|| pk.penalties.iter().find(|r| r.step == 8))
}

/// What a chaotic player may not do (`PKSetting.inc` columns `shop bank fly guardAttack vendor`; a 0 forbids).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Restriction {
    /// Opening an NPC shop (`DPSrvr.cpp:2795`).
    Shop,
    /// Opening the bank (`DPSrvr.cpp:3247`).
    Bank,
    /// Flying (`MoverEquip.cpp:1548`; `TID_GAME_CHAOTIC_NOT_FLY`).
    Fly,
    /// Attacking guards (unused by the C++).
    GuardAttack,
    /// Opening a personal shop (`DPSrvr.cpp:8997`; `TID_GAME_CHAOTIC_NOT_VENDOR`).
    Vendor,
}

/// `IsChaotic() && !GetPropensityPenalty(propensity).<column>`: `true` when `m` is chaotic and its stage forbids
/// `what`. A stage table without rows forbids everything (the C++ default struct is all zero).
pub fn chaotic_restriction(m: &Mover, pk: &PkSetting, what: Restriction) -> bool {
    let Some(p) = m.player() else {
        return false;
    };
    if p.pk.propensity == 0 {
        return false;
    }
    let Some(row) = propensity_penalty(pk, p.pk.propensity) else {
        return true;
    };
    let allowed = match what {
        Restriction::Shop => row.shop,
        Restriction::Bank => row.bank,
        Restriction::Fly => row.fly,
        Restriction::GuardAttack => row.guard_attack,
        Restriction::Vendor => row.vendor,
    };
    allowed == 0
}

/// Queues the three `PkRelation` kinds of a changed PK state for `Out` consumers.
pub fn announce_propensity(out: &mut Out, propensity: u32) {
    out.near(propensity_relation(propensity));
}
