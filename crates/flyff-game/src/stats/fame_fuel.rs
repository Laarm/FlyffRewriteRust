//! Fame, flight fuel and the flight level messages (Plan/16 §15, WP-MOVER-21; F-175, F-178).
//!
//! * [`set_fame`] / [`emit_fame`]: `SetFame{fame}` to the near set (doc 17 changes the fame with duel and PvP
//!   results, the GM command `/PvpParam` sets it);
//! * [`report_fuel`]: `SetFuel{fuel, acc_fuel}` to the owner every 16 ticks while the player flies, accelerates
//!   and has fuel left (`CUser::Process`, `User.cpp:478-490`); the fuel itself is consumed by the flying
//!   simulation (docs 18/31) and refilled by doc 19;
//! * [`emit_flight_level`]: `SetFlightLevel{level}` to the observers when the flight EXP counter wrapped
//!   (`AddFxp`, 16 §9.4); the owner learns it from `SetFxp`.

use super::out::Out;
use crate::mover::Mover;
use flyff_proto::pb;
use flyff_rules::exp_share::flight_level;
use flyff_sim::types::OBJSTAF_ACC;

/// The tick mask of the fuel report: `(m_nCount & 15) == 0`, one report per 16 ticks.
pub const FUEL_REPORT_MASK: u32 = 15;

/// `SetFame{fame}` to the near set with the stored fame of `m` (`g_UserMng.AddSetFame`).
pub fn emit_fame(m: &Mover, out: &mut Out) {
    if let Some(p) = m.player() {
        out.near(pb::SetFame {
            fame: p.progression.fame,
        });
    }
}

/// `m_nFame = fame` and `SetFame` to the near set. Returns `false` for a non-player.
pub fn set_fame(m: &mut Mover, fame: i32, out: &mut Out) -> bool {
    let Some(p) = m.player_mut() else {
        return false;
    };
    p.progression.fame = fame;
    emit_fame(m, out);
    true
}

/// The fuel report of one tick (`CUser::Process`, 16 §15): `SetFuel{fuel, acc_fuel}` to the owner when the player
/// has fuel (`sim.fuel > 0`), the mover counter `mv.count` is a multiple of 16, and the player flies and
/// accelerates (`OBJSTAF_FLY` and `OBJSTAF_ACC`). Returns `true` when a message was queued.
pub fn report_fuel(m: &Mover, out: &mut Out) -> bool {
    if !m.is_player()
        || m.sim.fuel <= 0
        || m.mv.count & FUEL_REPORT_MASK != 0
        || !m.sim.act.flags.fly()
        || !m.sim.act.flags.has(OBJSTAF_ACC)
    {
        return false;
    }
    out.to_self(pb::SetFuel {
        fuel: m.sim.fuel,
        acc_fuel: u32::try_from(m.sim.tm_acc_fuel).unwrap_or(0),
    });
    true
}

/// `SetFlightLevel{level}` to the observers (`NearExclSelf`): the flight level of `m` (derived from the character
/// level, 16 §0.13) after the flight EXP counter wrapped.
pub fn emit_flight_level(m: &Mover, out: &mut Out) {
    out.near_excl(pb::SetFlightLevel {
        flight_level: u32::try_from(flight_level(m.mv.level)).unwrap_or(0),
    });
}
