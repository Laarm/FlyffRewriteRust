//! Casting time and casting animation speed (Plan/17 §6.4; `MoverAttack.cpp:289-310`).

use crate::dst::DstTable;
use crate::stats;

/// `GetCastingTime(t)`: `max(0, t - t * param(SPELL_RATE, 0) / 100)` in integers.
pub fn casting_time(dst: &DstTable, t: i32) -> i32 {
    stats::casting_time(dst, t)
}

/// `GetCastingAniSpeed()`: `clamp(1 + param(SPELL_RATE, 0) / 100.0f, 0.1, 2.0)`.
pub fn casting_ani_speed(dst: &DstTable) -> f32 {
    stats::casting_ani_speed(dst)
}

/// The cast time of a `KT_MAGIC` skill in **server ticks**: `casting_time(((add.casting_time / 1000.0f) * 15.0f)
/// as i32)` (`do_use_skill`, Plan/17 §6.3). `cast_ms` is the `AddSkillProp.casting_time` column (milliseconds).
pub fn casting_ticks(dst: &DstTable, cast_ms: u32) -> i32 {
    let ticks = ((cast_ms as f32 / 1000.0_f32) * 15.0_f32) as i32;
    casting_time(dst, ticks)
}
