//! HP / MP / FP: `SetPointParam`, the increment helpers, the silent setters and the absolute resend
//! (Plan/16 §4.8, PL-075, PL-083; `MoverParam.cpp:555-590, 892-959`).
//!
//! The pools are server-authoritative absolute values. A change is sent as `SetPointParam{dst, value}` to the
//! mover's `NearInclSelf` audience when the stored value changed or the send is forced; damage never goes
//! through here (each client subtracts the `Damage` amount), heals, regeneration, potions, revival and the
//! forced resync after level-up, job change and restat do.

use super::env::{StatsCore, max_of, sm_active};
use super::out::Out;
use crate::mover::Mover;
use flyff_proto::pb;
use flyff_rules::consts::{SM_MAINTAIN_FP, SM_MAINTAIN_MP, dst::*};

/// The sentinel of `SetPointParam` that means "fill to the maximum" (`nMax == 999999999`, server only).
pub const FILL_SENTINEL: i32 = 999_999_999;

/// `CMover::SetPointParam(dst, value, bForceSend)` for the three pools (`DST_GOLD` is never set here: the gold
/// API is doc 19's `add_gold` and calls [`emit_gold`]; any other dst is ignored).
///
/// `value < 0` becomes 0; a value above the maximum, or the [`FILL_SENTINEL`], becomes the maximum; the stored
/// value is replaced and `SetPointParam{dst, value}` goes to `NearInclSelf` iff it changed or `force`.
pub fn set_point_param(
    m: &mut Mover,
    c: &StatsCore,
    dst: u32,
    value: i32,
    force: bool,
    out: &mut Out,
) {
    if !matches!(dst, DST_HP | DST_MP | DST_FP) {
        return;
    }
    let fill = value == FILL_SENTINEL;
    let mut v = value.max(0);
    let max = max_of(m, c, dst);
    if v > max || fill {
        v = max;
    }
    let slot = match dst {
        DST_HP => &mut m.mv.hp,
        DST_MP => &mut m.mv.mp,
        _ => &mut m.mv.fp,
    };
    let changed = *slot != v;
    *slot = v;
    if changed || force {
        out.near(pb::SetPointParam {
            dst: dst as i32,
            value: v,
        });
    }
}

/// `IncHitPoint(n)`: `SetPointParam(DST_HP, stored + n)` on the **raw stored** value.
pub fn inc_hp(m: &mut Mover, c: &StatsCore, n: i32, out: &mut Out) {
    let v = m.mv.hp.wrapping_add(n);
    set_point_param(m, c, DST_HP, v, false, out);
}

/// `IncManaPoint(n)`: a negative amount is a no-op while `SM_MAINTAIN_MP` is active (`MoverParam.cpp:563`).
pub fn inc_mp(m: &mut Mover, c: &StatsCore, n: i32, out: &mut Out) {
    if n < 0 && sm_active(m, SM_MAINTAIN_MP) {
        return;
    }
    let v = m.mv.mp.wrapping_add(n);
    set_point_param(m, c, DST_MP, v, false, out);
}

/// `IncFatiguePoint(n)`: a negative amount is a no-op while `SM_MAINTAIN_FP` is active.
pub fn inc_fp(m: &mut Mover, c: &StatsCore, n: i32, out: &mut Out) {
    if n < 0 && sm_active(m, SM_MAINTAIN_FP) {
        return;
    }
    let v = m.mv.fp.wrapping_add(n);
    set_point_param(m, c, DST_FP, v, false, out);
}

/// `SetHitPoint(v)`: stores with the same clamp and never sends. Every caller that uses it for a level-up, job
/// change, restat or revival MUST then call [`send_pools_absolute`] (PL-083).
pub fn set_hp_silent(m: &mut Mover, c: &StatsCore, v: i32) {
    m.mv.hp = v.clamp(0, max_of(m, c, DST_HP).max(0));
}

/// `SetManaPoint(v)`: see [`set_hp_silent`].
pub fn set_mp_silent(m: &mut Mover, c: &StatsCore, v: i32) {
    m.mv.mp = v.clamp(0, max_of(m, c, DST_MP).max(0));
}

/// `SetFatiguePoint(v)`: see [`set_hp_silent`].
pub fn set_fp_silent(m: &mut Mover, c: &StatsCore, v: i32) {
    m.mv.fp = v.clamp(0, max_of(m, c, DST_FP).max(0));
}

/// `SetHitPoint(GetMaxHitPoint())`, `SetManaPoint(GetMaxManaPoint())`, `SetFatiguePoint(GetMaxFatiguePoint())`
/// without a message: what level-up, job change and the restat functions do before [`send_pools_absolute`].
pub fn fill_pools_silent(m: &mut Mover, c: &StatsCore) {
    let (hp, mp, fp) = (
        max_of(m, c, DST_HP),
        max_of(m, c, DST_MP),
        max_of(m, c, DST_FP),
    );
    m.mv.hp = hp;
    m.mv.mp = mp;
    m.mv.fp = fp;
}

/// PL-083: three forced `SetPointParam` (HP, MP, FP) with the stored values to `NearInclSelf`, so that no
/// client has to guess the pools after a level-up, job change, restat or revival.
pub fn send_pools_absolute(m: &mut Mover, c: &StatsCore, out: &mut Out) {
    let (hp, mp, fp) = (m.mv.hp, m.mv.mp, m.mv.fp);
    for (dst, v) in [(DST_HP, hp), (DST_MP, mp), (DST_FP, fp)] {
        set_point_param(m, c, dst, v, true, out);
    }
}

/// `SetPointParam{DST_GOLD, gold}` after a gold change (doc 19 calls it from `add_gold`, VR-011): to the owner
/// **only** when `fidelity.gold_update_self_only` (default, PL-075), otherwise to the near set as the legacy did.
pub fn emit_gold(m: &Mover, c: &StatsCore, out: &mut Out) {
    let Some(p) = m.player() else { return };
    let msg = pb::SetPointParam {
        dst: DST_GOLD as i32,
        value: p.items.gold,
    };
    if c.gold_self_only {
        out.to_self_subject(msg);
    } else {
        out.near(msg);
    }
}
