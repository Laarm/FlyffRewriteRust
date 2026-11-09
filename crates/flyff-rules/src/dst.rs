//! The DST ("destination parameter") aggregate table (Plan/16 §4.1-§4.4; ref/10 §7).
//!
//! [`DstTable`] stores the 95 stored DST slots: `adj` (additive sums, or bit masks for `CHRSTATE` /
//! `IMMUNITY`) and `chg` (an override, or [`NULL_CHGPARAM`]). The *primitives* [`DstTable::set_primitive`] and
//! [`DstTable::reset_primitive`] are the exact legacy semantics of `CMover::SetDestParam` /
//! `ResetDestParam` for an already expanded, stored dst (`dst < 95`, steps 3-5 of 16 §4.2 and §4.3); the three
//! `replica_*` functions are what the browser applies for the `SetDestParam` / `ResetDestParam` /
//! `ResetDestParamSync` messages (16 §4.7). The server's per-source ledger (`DstState`, WP-MOVER-05) is built
//! on the same primitives.
//!
//! Composite DSTs (`RESIST_ALL`, `LOCOMOTION`, the rate composites, ...) never reach the table:
//! [`expand_composite`] turns them into primitives first (16 §4.4).
//!
//! All arithmetic on the aggregate wraps like the C `int` it ports (16 §4.1).

use crate::consts::{NULL_CHGPARAM, NULL_ID, dst::*};
use smallvec::SmallVec;

/// The stored size of the aggregate (`MAX_ADJPARAMARY`).
pub const DST_STORED: usize = crate::consts::MAX_ADJPARAMARY;

/// The base values the rate composites are computed from at apply time: the maximum HP/MP/FP of the mover
/// from its **base** stats at its current level and job (`GetMaxOrigin*Point(TRUE)`, 16 §4.4). Doc 16 §4.5
/// records the computed integer in the ledger so that removal subtracts exactly what was added.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OriginMax {
    /// `GetMaxOriginHitPoint()`.
    pub hp: i32,
    /// `GetMaxOriginManaPoint()`.
    pub mp: i32,
    /// `GetMaxOriginFatiguePoint()`.
    pub fp: i32,
}

/// What [`DstTable::set_primitive`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetOutcome {
    /// Nothing was stored (`dst` out of range, a pool dst, `adj == chg == "none"`, a `NULL_ID` bit mask).
    Ignored,
    /// The aggregate changed.
    Stored {
        /// `DST_HEAL` with a non-zero `adj`: the caller must (re)start the heal counter
        /// (`status.heal_cnt = 45`, 16 §4.2 step 4).
        heal_started: bool,
    },
}

impl SetOutcome {
    /// `true` when the aggregate changed.
    pub fn stored(self) -> bool {
        matches!(self, SetOutcome::Stored { .. })
    }
}

/// The aggregate of one mover: `adj[95]` and `chg[95]` (16 §4.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DstTable {
    /// Additive sums; bit masks for `DST_CHRSTATE` and `DST_IMMUNITY`.
    pub adj: [i32; DST_STORED],
    /// Overrides (`NULL_CHGPARAM` = none); the `SPEED` slot is a root counter; `REFLECT_DAMAGE`, the
    /// `CHR_CHANCE*` and `AUTOHP` slots carry a side value.
    pub chg: [i32; DST_STORED],
}

impl Default for DstTable {
    fn default() -> Self {
        DstTable {
            adj: [0; DST_STORED],
            chg: [NULL_CHGPARAM; DST_STORED],
        }
    }
}

/// `true` for the dst ids that `SetDestParam` handles as immediate pool effects (never stored).
#[inline]
pub fn is_pool_dst(dst: u32) -> bool {
    matches!(dst, DST_HP | DST_MP | DST_FP)
}

/// `true` for the dsts whose `adj` is a bit mask (`or`-ed on set, cleared bitwise on reset).
#[inline]
pub fn is_mask_dst(dst: u32) -> bool {
    matches!(dst, DST_CHRSTATE | DST_IMMUNITY)
}

/// `true` for the dsts that store `chg` as a side value next to a non-zero `adj` (proc chance, duration,
/// reflect percentage; 16 §4.2 step 4).
#[inline]
pub fn has_side_value(dst: u32) -> bool {
    matches!(
        dst,
        DST_REFLECT_DAMAGE
            | DST_CHR_CHANCEBLEEDING
            | DST_CHR_CHANCESTEALHP
            | DST_CHR_CHANCEPOISON
            | DST_CHR_CHANCEDARK
            | DST_CHR_CHANCESTUN
            | DST_AUTOHP
    )
}

impl DstTable {
    /// `GetAdjParam(dst)`: `adj[dst]` when `dst < 95`, else 0.
    #[inline]
    pub fn adj(&self, dst: u32) -> i32 {
        self.adj.get(dst as usize).copied().unwrap_or(0)
    }

    /// `GetChgParam(dst)`: `chg[dst]` when `dst < 95`, else [`NULL_CHGPARAM`].
    #[inline]
    pub fn chg(&self, dst: u32) -> i32 {
        self.chg.get(dst as usize).copied().unwrap_or(NULL_CHGPARAM)
    }

    /// `GetParam(dst, base)` (`MoverParam.cpp:2775-2786`): an override replaces the value and ignores
    /// `base`, otherwise `base + adj`.
    #[inline]
    pub fn param(&self, dst: u32, base: i32) -> i32 {
        let c = self.chg(dst);
        if c != NULL_CHGPARAM {
            return c;
        }
        base.wrapping_add(self.adj(dst))
    }

    /// `true` when every slot is at its default (no source applied).
    pub fn is_default(&self) -> bool {
        *self == DstTable::default()
    }

    /// `SetDestParam` steps 3-5 for a stored primitive (`MoverParam.cpp:2262-2538`): `dst >= 95` and the pool
    /// dsts are ignored (the server handles pools as immediate effects, the legacy client adds to the pool);
    /// `adj != 0` stores additively (bit-or for the mask dsts, `chg` as side value for the proc dsts, a heal
    /// start for `HEAL`); otherwise a `chg` that is not [`NULL_CHGPARAM`] is stored (`SPEED`: a root counter).
    pub fn set_primitive(&mut self, dst: u32, adj: i32, chg: i32) -> SetOutcome {
        let i = dst as usize;
        if i >= DST_STORED || is_pool_dst(dst) {
            return SetOutcome::Ignored;
        }
        if adj != 0 {
            if is_mask_dst(dst) {
                // `adj == NULL_ID` (-1) is a no-op: the legacy code refuses an all-ones mask.
                if adj as u32 == NULL_ID {
                    return SetOutcome::Ignored;
                }
                if let Some(a) = self.adj.get_mut(i) {
                    *a |= adj;
                }
                return SetOutcome::Stored {
                    heal_started: false,
                };
            }
            if let Some(a) = self.adj.get_mut(i) {
                *a = a.wrapping_add(adj);
            }
            if has_side_value(dst)
                && let Some(c) = self.chg.get_mut(i)
            {
                *c = chg;
            }
            return SetOutcome::Stored {
                heal_started: dst == DST_HEAL,
            };
        }
        if chg != NULL_CHGPARAM {
            if let Some(c) = self.chg.get_mut(i) {
                if dst == DST_SPEED {
                    // root counter: unset -> 0, then +1; any count > 0 makes the mover stand still
                    if *c == NULL_CHGPARAM {
                        *c = 0;
                    }
                    *c = c.wrapping_add(1);
                } else {
                    *c = chg;
                }
            }
            return SetOutcome::Stored {
                heal_started: false,
            };
        }
        SetOutcome::Ignored
    }

    /// `ResetDestParam` for a stored dst (`MoverParam.cpp:2543-2730`, 16 §4.3). Returns `true` when the
    /// aggregate was addressed (`dst < 95`). `adj != 0`: the mask dsts clear the bits (`IMMUNITY` too: the
    /// replica follows the server's bit-reference-counted emission, 16 §4.5 rule 3, not the legacy `-=`),
    /// every other dst subtracts; `adj == 0`: a `SPEED` root counter above zero counts down (back to
    /// [`NULL_CHGPARAM`] at zero), every other dst drops its override.
    pub fn reset_primitive(&mut self, dst: u32, adj: i32) -> bool {
        let i = dst as usize;
        if i >= DST_STORED {
            return false;
        }
        if adj != 0 {
            if let Some(a) = self.adj.get_mut(i) {
                if is_mask_dst(dst) {
                    *a &= !adj;
                } else {
                    *a = a.wrapping_sub(adj);
                }
            }
        } else if let Some(c) = self.chg.get_mut(i) {
            if dst == DST_SPEED && *c != NULL_CHGPARAM && *c > 0 {
                *c -= 1;
                if *c == 0 {
                    *c = NULL_CHGPARAM;
                }
            } else {
                *c = NULL_CHGPARAM;
            }
        }
        true
    }

    /// The browser's load of one `MoverCommon.dst_params` entry of a spawn message (16 §4.7 rule 1): the slot is
    /// **assigned** `(adj, chg)`. Unlike [`DstTable::replica_set`] it also restores a `SPEED` root counter above 1
    /// and a side value next to a zero `adj` exactly; `dst >= 95` is ignored.
    pub fn replica_load(&mut self, dst: u32, adj: i32, chg: i32) {
        let i = dst as usize;
        if let (Some(a), Some(c)) = (self.adj.get_mut(i), self.chg.get_mut(i)) {
            *a = adj;
            *c = chg;
        }
    }

    /// The browser's `SetDestParam` message (16 §4.7): [`DstTable::set_primitive`] without the outcome.
    pub fn replica_set(&mut self, dst: u32, adj: i32, chg: i32) {
        let _ = self.set_primitive(dst, adj, chg);
    }

    /// The browser's `ResetDestParam` message (16 §4.7).
    pub fn replica_reset(&mut self, dst: u32, adj: i32) {
        let _ = self.reset_primitive(dst, adj);
    }

    /// The browser's `ResetDestParamSync` message (`__SPEED_SYNC_0108`, `ResetDestParamSync`): for `SPEED`
    /// the sum is replaced by the server's total and an `adj == 0` reset counts the root counter down; the
    /// other dsts are ignored.
    pub fn replica_reset_sync(&mut self, dst: u32, adj: i32, total: i32) {
        if dst != DST_SPEED {
            return;
        }
        let i = dst as usize;
        if let Some(a) = self.adj.get_mut(i) {
            *a = total;
        }
        if adj == 0
            && let Some(c) = self.chg.get_mut(i)
            && *c != NULL_CHGPARAM
            && *c > 0
        {
            *c -= 1;
            if *c == 0 {
                *c = NULL_CHGPARAM;
            }
        }
    }
}

/// The composite DSTs and the primitives each expands into (16 §4.4). `LOCOMOTION` multiplies the `JUMPING`
/// adjustment by 3, the rate composites compute `(int)(max_origin * (adj / 100.0f))`: see
/// [`expand_composite`] for the values.
pub const COMPOSITES: &[(u32, &[u32])] = &[
    (
        DST_RESIST_ALL,
        &[
            DST_RESIST_ELECTRICITY,
            DST_RESIST_FIRE,
            DST_RESIST_WIND,
            DST_RESIST_WATER,
            DST_RESIST_EARTH,
        ],
    ),
    (DST_STAT_ALLUP, &[DST_STR, DST_DEX, DST_INT, DST_STA]),
    (DST_HPDMG_UP, &[DST_HP_MAX, DST_CHR_DMG]),
    (DST_DEFHITRATE_DOWN, &[DST_ADJDEF, DST_ADJ_HITRATE]),
    (DST_LOCOMOTION, &[DST_SPEED, DST_JUMPING]),
    (DST_HP_RECOVERY_RATE, &[DST_HP_RECOVERY]),
    (DST_MP_RECOVERY_RATE, &[DST_MP_RECOVERY]),
    (DST_FP_RECOVERY_RATE, &[DST_FP_RECOVERY]),
    (
        DST_ALL_RECOVERY,
        &[DST_HP_RECOVERY, DST_MP_RECOVERY, DST_FP_RECOVERY],
    ),
    (
        DST_ALL_RECOVERY_RATE,
        &[DST_HP_RECOVERY, DST_MP_RECOVERY, DST_FP_RECOVERY],
    ),
    (DST_KILL_ALL, &[DST_KILL_HP, DST_KILL_MP, DST_KILL_FP]),
    (DST_KILL_HP_RATE, &[DST_KILL_HP]),
    (DST_KILL_MP_RATE, &[DST_KILL_MP]),
    (DST_KILL_FP_RATE, &[DST_KILL_FP]),
    (DST_KILL_ALL_RATE, &[DST_KILL_HP, DST_KILL_MP, DST_KILL_FP]),
    (DST_ALL_DEC_RATE, &[DST_MP_DEC_RATE, DST_FP_DEC_RATE]),
    // dropped or no-op composites: listed with no primitive so `is_composite` knows them
    (DST_MASTRY_ALL, &[]),
    (DST_CURECHR, &[]),
    (DST_GOLD, &[]),
    (DST_PXP, &[]),
];

/// `true` for a composite or action dst (never stored; `>= 10000`).
pub fn is_composite(dst: u32) -> bool {
    COMPOSITES.iter().any(|(c, _)| *c == dst)
}

/// `(int)(max * (adj / 100.0f))` with the C usual arithmetic conversions: `int * float` is a float product.
#[inline]
fn rate(max: i32, adj: i32) -> i32 {
    (max as f32 * (adj as f32 / 100.0_f32)) as i32
}

/// Expands a dst into the primitives `SetDestParam` applies (16 §4.4). A non-composite dst is returned
/// unchanged (pool dsts included: the caller decides what an immediate effect is). `MASTRY_ALL`, `CURECHR`
/// (**KEEP-QUIRK**: the legacy `case` is unreachable because 10006 >= 95 returns first), `GOLD` and `PXP`
/// expand to nothing. The rate composites use `origin` (16 §4.4: `GetMaxOrigin*Point(TRUE)` at apply time) and
/// give `chg = NULL_CHGPARAM`.
pub fn expand_composite(
    dst: u32,
    adj: i32,
    chg: i32,
    origin: &OriginMax,
) -> SmallVec<[(u32, i32, i32); 5]> {
    let mut out: SmallVec<[(u32, i32, i32); 5]> = SmallVec::new();
    match dst {
        DST_RESIST_ALL => {
            for d in [
                DST_RESIST_ELECTRICITY,
                DST_RESIST_FIRE,
                DST_RESIST_WIND,
                DST_RESIST_WATER,
                DST_RESIST_EARTH,
            ] {
                out.push((d, adj, chg));
            }
        }
        DST_STAT_ALLUP => {
            for d in [DST_STR, DST_DEX, DST_INT, DST_STA] {
                out.push((d, adj, chg));
            }
        }
        DST_HPDMG_UP => {
            out.push((DST_HP_MAX, adj, chg));
            out.push((DST_CHR_DMG, adj, chg));
        }
        DST_DEFHITRATE_DOWN => {
            out.push((DST_ADJDEF, adj, chg));
            out.push((DST_ADJ_HITRATE, adj, chg));
        }
        DST_LOCOMOTION => {
            out.push((DST_SPEED, adj, chg));
            out.push((DST_JUMPING, adj.wrapping_mul(3), chg));
        }
        DST_HP_RECOVERY_RATE => out.push((DST_HP_RECOVERY, rate(origin.hp, adj), NULL_CHGPARAM)),
        DST_MP_RECOVERY_RATE => out.push((DST_MP_RECOVERY, rate(origin.mp, adj), NULL_CHGPARAM)),
        DST_FP_RECOVERY_RATE => out.push((DST_FP_RECOVERY, rate(origin.fp, adj), NULL_CHGPARAM)),
        DST_ALL_RECOVERY => {
            for d in [DST_HP_RECOVERY, DST_MP_RECOVERY, DST_FP_RECOVERY] {
                out.push((d, adj, NULL_CHGPARAM));
            }
        }
        DST_ALL_RECOVERY_RATE => {
            out.push((DST_HP_RECOVERY, rate(origin.hp, adj), NULL_CHGPARAM));
            out.push((DST_MP_RECOVERY, rate(origin.mp, adj), NULL_CHGPARAM));
            out.push((DST_FP_RECOVERY, rate(origin.fp, adj), NULL_CHGPARAM));
        }
        DST_KILL_ALL => {
            for d in [DST_KILL_HP, DST_KILL_MP, DST_KILL_FP] {
                out.push((d, adj, NULL_CHGPARAM));
            }
        }
        DST_KILL_HP_RATE => out.push((DST_KILL_HP, rate(origin.hp, adj), NULL_CHGPARAM)),
        DST_KILL_MP_RATE => out.push((DST_KILL_MP, rate(origin.mp, adj), NULL_CHGPARAM)),
        DST_KILL_FP_RATE => out.push((DST_KILL_FP, rate(origin.fp, adj), NULL_CHGPARAM)),
        DST_KILL_ALL_RATE => {
            out.push((DST_KILL_HP, rate(origin.hp, adj), NULL_CHGPARAM));
            out.push((DST_KILL_MP, rate(origin.mp, adj), NULL_CHGPARAM));
            out.push((DST_KILL_FP, rate(origin.fp, adj), NULL_CHGPARAM));
        }
        DST_ALL_DEC_RATE => {
            out.push((DST_MP_DEC_RATE, adj, NULL_CHGPARAM));
            out.push((DST_FP_DEC_RATE, adj, NULL_CHGPARAM));
        }
        DST_MASTRY_ALL | DST_CURECHR | DST_GOLD | DST_PXP => {}
        other => out.push((other, adj, chg)),
    }
    out
}
