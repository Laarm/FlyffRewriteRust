//! `DstState`: the DST runtime of one mover (Plan/16 §4, WP-MOVER-05; F-119, F-113, PL-080).
//!
//! The aggregate [`DstTable`] (`adj[95]` / `chg[95]`, what `GetParam` reads) is kept exactly as the original
//! kept it, **plus** a per-source ledger so that removing a source reverts exactly what it applied: the legacy
//! `ResetDestParam` recomputed rate composites at reset time (drift), added instead of subtracted for
//! `KILL_ALL` / `ALL_DEC_RATE` and cleared `IMMUNITY` with `-=` (16 §1.4, ref/10 §7.3). Here
//!
//! * [`DstState::apply`] expands composites (16 §4.4), performs the primitive of §4.2 on the aggregate and
//!   records the operation actually applied under the [`DstSource`] (a rate composite records the integer it
//!   computed, not the percentage);
//! * [`DstState::remove`] reverts the recorded operations of the source in reverse order: additions are
//!   subtracted, `CHRSTATE` / `IMMUNITY` bits are reference counted per bit (a bit leaves the aggregate when its
//!   last holder goes), overrides and side values live in a per-dst stack (the aggregate is the value of the
//!   most recently applied remaining entry, or none), the `SPEED` root is a counter;
//! * every aggregate change appends the legacy replication message to [`DstCtx::emit`] (16 §4.7): `Set`
//!   (`SetDestParam`), `Reset` (`ResetDestParam`), `ResetSync` (`ResetDestParamSync`, `DST_SPEED` only). The
//!   browser applies them with `DstTable::replica_*` and always equals the server table (property test
//!   `dst_replica_matches_server`).
//!
//! The pool dsts `HP` / `MP` / `FP` are immediate effects, never stored and never recorded: [`DstState::apply`]
//! hands them to the caller as [`DstEmit::Pool`] (`stats::source` turns them into `set_point_param`).

use flyff_rules::consts::{NULL_CHGPARAM, NULL_ID, dst::*};
use flyff_rules::dst::{
    DST_STORED, DstTable, OriginMax, expand_composite, has_side_value, is_composite, is_mask_dst,
    is_pool_dst,
};
use std::collections::BTreeMap;

/// Who applied a set of DST modifiers (16 §4.5). Removing a source reverts exactly its own operations.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum DstSource {
    /// Prop DSTs, armour element/resist, accessory bonus, random option, (vis) piercing of the item in `part`.
    Equip {
        /// The equipment part (`PARTS_*`).
        part: u8,
    },
    /// Ultimate-weapon gem DSTs (the list is computed by doc 19).
    Ultimate {
        /// The equipment part.
        part: u8,
    },
    /// The armour +N bonus (16 §13.5).
    ArmorSet,
    /// A `propItemEtc` `SetItem` bonus (16 §13.5).
    NamedSet {
        /// The set id.
        set_id: u32,
    },
    /// A buff of doc 17: `kind` is `BUFF_ITEM 0`, `SKILL 1`, `PET 2`, `ITEM2 3`, `EQUIP 4`.
    Buff {
        /// `BUFF_*`.
        kind: u8,
        /// Skill or item id.
        id: u32,
    },
    /// An SM premium timer: `SM_MAX_HP`, `SM_MAX_HP50`, `SM_VELOCIJUMP` (16 §14).
    Sm {
        /// `SM_*`.
        sm: u8,
    },
    /// A counter-driven abnormal state of doc 17 (`CHS` bits, dark's `ADJ_HITRATE`).
    Abnormal {
        /// The `CHS_*` bit or state id.
        state: u32,
    },
    /// Item-script hooks (`ITEM_OP_EQUIP`) and GM / one-off sources (docs 19 and 23).
    Script {
        /// Caller-chosen tag.
        tag: u32,
    },
    /// The summoned system pet's avail bonus and awakening lines (19 §13, `ModSource::Pet`).
    Pet,
    /// An installed guild-house furniture entry (doc 22 §12.5, task guild-house): `seq` is its row's sequence number.
    GuildFurniture {
        /// The `guild_house_furniture.seq` of the entry.
        seq: u32,
    },
}

/// One operation that was applied to the aggregate for a source (the ledger entry, 16 §4.5).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DstOp {
    /// `adj[dst] += value`.
    Add {
        /// The stored dst.
        dst: u8,
        /// The amount that was added.
        value: i32,
    },
    /// `adj[dst] |= bits` for `CHRSTATE` / `IMMUNITY` (reference counted per bit).
    Bits {
        /// `DST_CHRSTATE` or `DST_IMMUNITY`.
        dst: u8,
        /// The mask the source set.
        bits: u32,
    },
    /// An override or side value pushed onto the stack of `dst`.
    Chg {
        /// The stored dst.
        dst: u8,
        /// The value.
        value: i32,
    },
    /// One `DST_SPEED` root (the counter in `chg[SPEED]` went up).
    SpeedRoot,
}

/// What an apply or remove asks the caller to do (16 §4.7). The first three are the legacy messages for the
/// mover's `NearInclSelf` audience; `Pool` is an immediate pool effect.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DstEmit {
    /// `SetDestParam{dst, adj, chg}`.
    Set {
        /// DST id.
        dst: u32,
        /// Adjustment.
        adj: i32,
        /// Change value, [`NULL_CHGPARAM`] when unset.
        chg: i32,
    },
    /// `ResetDestParam{dst, adj}` (`adj == 0` clears the override).
    Reset {
        /// DST id.
        dst: u32,
        /// Adjustment taken back.
        adj: i32,
    },
    /// `ResetDestParamSync{dst = SPEED, adj, total}`.
    ResetSync {
        /// Always `DST_SPEED`.
        dst: u32,
        /// Adjustment taken back (0 = a root was lifted).
        adj: i32,
        /// The resulting `adj[SPEED]`.
        total: i32,
    },
    /// An immediate `HP` / `MP` / `FP` effect (`SetDestParam` on a pool, `adj == -1` = fill): the caller
    /// calls `set_point_param`.
    Pool {
        /// `DST_HP`, `DST_MP` or `DST_FP`.
        dst: u32,
        /// The adjustment (`-1` = to the maximum).
        adj: i32,
    },
}

/// What `apply` / `remove` need besides the state (16 §4.5).
pub struct DstCtx<'a> {
    /// `GetMaxOrigin*Point(TRUE)` of the mover **now**: the base the rate composites are computed from.
    pub origin: OriginMax,
    /// `status.heal_cnt`, set to [`DstCtx::heal_ticks`] when a `DST_HEAL` adjustment is stored.
    pub heal_cnt: &'a mut i16,
    /// The `DST_HEAL` interval (`HEAL_TICKS` 45, or 90 without `features.heal_over_time`).
    pub heal_ticks: i16,
    /// Receives the emissions in order.
    pub emit: &'a mut Vec<DstEmit>,
    /// `false` suppresses `Set` / `Reset` / `ResetSync` (the join batch and `redo_equip` at join: the spawn
    /// snapshot already contains the result, 16 §4.7 rule 1). Pool effects are always emitted.
    pub send: bool,
}

/// The DST state of a mover: the aggregate `GetParam` reads, the per-source ledger and the bookkeeping that
/// makes a removal exact (16 §4.5). `table` stays public: the spawn snapshot and the formulas read it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DstState {
    /// `m_adjParamAry` / `m_chgParamAry`.
    pub table: DstTable,
    /// The operations actually applied per source, in order.
    ledger: BTreeMap<DstSource, Vec<DstOp>>,
    /// Per-bit holder counts for `CHRSTATE` (row 0) and `IMMUNITY` (row 1).
    bit_refs: [[u16; 32]; 2],
    /// Overrides and side values per stored dst (`SPEED` excluded), oldest first.
    chg_stack: BTreeMap<u8, Vec<(DstSource, i32)>>,
}

fn mask_row(dst: u32) -> usize {
    usize::from(dst == DST_IMMUNITY)
}

impl DstState {
    /// `true` while `src` has applied operations.
    pub fn has(&self, src: DstSource) -> bool {
        self.ledger.contains_key(&src)
    }

    /// The sources that currently hold operations, in `DstSource` order.
    pub fn sources(&self) -> impl Iterator<Item = &DstSource> {
        self.ledger.keys()
    }

    /// The operations recorded for `src` (tests and debugging).
    pub fn ops_of(&self, src: DstSource) -> &[DstOp] {
        self.ledger.get(&src).map_or(&[], Vec::as_slice)
    }

    /// Moves everything `from` applied to `to` without touching the aggregate (the left weapon becoming the
    /// right one, 16 §13.3): the ledger entry and the override-stack tags follow. A no-op when `from` holds
    /// nothing; operations already held by `to` stay in front.
    pub fn rekey(&mut self, from: DstSource, to: DstSource) {
        if from == to {
            return;
        }
        if let Some(ops) = self.ledger.remove(&from) {
            self.ledger.entry(to).or_default().extend(ops);
        }
        for stack in self.chg_stack.values_mut() {
            for entry in stack.iter_mut() {
                if entry.0 == from {
                    entry.0 = to;
                }
            }
        }
    }

    /// `true` when nothing is applied: no source, a default aggregate, no bit holders and no stack entry.
    pub fn is_clean(&self) -> bool {
        self.ledger.is_empty()
            && self.table.is_default()
            && self.chg_stack.is_empty()
            && self.bit_refs.iter().flatten().all(|n| *n == 0)
    }

    /// `SetDestParam(dst, adj, chg)` for one prop slot, on behalf of `src` (16 §4.2, §4.4).
    pub fn apply(&mut self, src: DstSource, dst: u32, adj: i32, chg: i32, ctx: &mut DstCtx<'_>) {
        // the legacy refuses a negative dst (`nDstParameter < 0`); the ids are `u32` here
        if (dst as i32) < 0 {
            return;
        }
        if is_pool_dst(dst) {
            ctx.emit.push(DstEmit::Pool { dst, adj });
            return;
        }
        if dst >= DST_STORED as u32 {
            if is_composite(dst) {
                for (d, a, c) in expand_composite(dst, adj, chg, &ctx.origin) {
                    self.apply_primitive(src, d, a, c, ctx);
                }
            }
            return;
        }
        self.apply_primitive(src, dst, adj, chg, ctx);
    }

    /// Rewrites what `src` recorded as additions on `dst` so that they total `value`: the first matching entry
    /// takes the value, further ones go (Master's Blessing shield: the aggregate shrinks behind the ledger
    /// as the shield absorbs damage, and the release must give back what is left). `value == 0` drops the
    /// entries.
    pub fn set_recorded_total(&mut self, src: DstSource, dst: u32, value: i32) {
        let Some(ops) = self.ledger.get_mut(&src) else {
            return;
        };
        let mut first = true;
        ops.retain_mut(|op| match op {
            DstOp::Add { dst: d, value: v } if u32::from(*d) == dst => {
                if first && value != 0 {
                    first = false;
                    *v = value;
                    true
                } else {
                    false
                }
            }
            _ => true,
        });
    }

    /// Applies every slot of a prop in order (`SetDestParam(itemId)`).
    pub fn apply_all(&mut self, src: DstSource, slots: &[(u32, i32, i32)], ctx: &mut DstCtx<'_>) {
        for &(dst, adj, chg) in slots {
            self.apply(src, dst, adj, chg, ctx);
        }
    }

    fn record(&mut self, src: DstSource, op: DstOp) {
        self.ledger.entry(src).or_default().push(op);
    }

    fn push_chg(&mut self, src: DstSource, dst: u32, value: i32) {
        if let Ok(key) = u8::try_from(dst) {
            self.chg_stack.entry(key).or_default().push((src, value));
        }
        self.record(
            src,
            DstOp::Chg {
                dst: dst as u8,
                value,
            },
        );
    }

    /// `SetDestParam` steps 3-5 for a stored primitive (`dst < 95`, not a pool, not a composite).
    fn apply_primitive(
        &mut self,
        src: DstSource,
        dst: u32,
        adj: i32,
        chg: i32,
        ctx: &mut DstCtx<'_>,
    ) {
        let i = dst as usize;
        if i >= DST_STORED || is_pool_dst(dst) {
            return;
        }
        let key = dst as u8;
        if adj != 0 {
            if is_mask_dst(dst) {
                // `adj == NULL_ID` (-1, all bits) is refused by the legacy code
                if adj as u32 == NULL_ID {
                    return;
                }
                let bits = adj as u32;
                self.table.replica_set(dst, adj, chg);
                let row = mask_row(dst);
                for b in 0..32 {
                    if bits & (1_u32 << b) != 0
                        && let Some(n) = self.bit_refs.get_mut(row).and_then(|r| r.get_mut(b))
                    {
                        *n = n.saturating_add(1);
                    }
                }
                self.record(src, DstOp::Bits { dst: key, bits });
                self.send_set(ctx, dst, adj, chg);
                return;
            }
            let outcome = self.table.set_primitive(dst, adj, chg);
            self.record(
                src,
                DstOp::Add {
                    dst: key,
                    value: adj,
                },
            );
            if has_side_value(dst) {
                // the side value is the aggregate `chg` now; remember whose it is
                self.push_chg(src, dst, chg);
            }
            if matches!(
                outcome,
                flyff_rules::dst::SetOutcome::Stored { heal_started: true }
            ) {
                *ctx.heal_cnt = ctx.heal_ticks;
            }
            self.send_set(ctx, dst, adj, chg);
            return;
        }
        if chg != NULL_CHGPARAM {
            self.table.replica_set(dst, 0, chg);
            if dst == DST_SPEED {
                self.record(src, DstOp::SpeedRoot);
            } else {
                self.push_chg(src, dst, chg);
            }
            self.send_set(ctx, dst, 0, chg);
        }
    }

    fn send_set(&self, ctx: &mut DstCtx<'_>, dst: u32, adj: i32, chg: i32) {
        if ctx.send {
            ctx.emit.push(DstEmit::Set { dst, adj, chg });
        }
    }

    /// Reverts every operation `src` applied, newest first (16 §4.5 rule 2). Pool effects are not undone
    /// (KEEP). A source without operations is a no-op.
    pub fn remove(&mut self, src: DstSource, ctx: &mut DstCtx<'_>) {
        let Some(ops) = self.ledger.remove(&src) else {
            return;
        };
        for op in ops.into_iter().rev() {
            match op {
                DstOp::Add { dst, value } => {
                    let d = u32::from(dst);
                    self.table.replica_reset(d, value);
                    if ctx.send {
                        if d == DST_SPEED {
                            ctx.emit.push(DstEmit::ResetSync {
                                dst: d,
                                adj: value,
                                total: self.table.adj(d),
                            });
                        } else {
                            ctx.emit.push(DstEmit::Reset { dst: d, adj: value });
                        }
                    }
                }
                DstOp::Bits { dst, bits } => {
                    let d = u32::from(dst);
                    let row = mask_row(d);
                    let mut cleared = 0_u32;
                    for b in 0..32 {
                        if bits & (1_u32 << b) == 0 {
                            continue;
                        }
                        if let Some(n) = self.bit_refs.get_mut(row).and_then(|r| r.get_mut(b)) {
                            *n = n.saturating_sub(1);
                            if *n == 0 {
                                cleared |= 1_u32 << b;
                            }
                        }
                    }
                    if cleared != 0 {
                        self.table.replica_reset(d, cleared as i32);
                        if ctx.send {
                            ctx.emit.push(DstEmit::Reset {
                                dst: d,
                                adj: cleared as i32,
                            });
                        }
                    }
                }
                DstOp::Chg { dst, .. } => self.pop_chg(src, dst, ctx),
                DstOp::SpeedRoot => {
                    let d = DST_SPEED;
                    self.table.replica_reset(d, 0);
                    if ctx.send {
                        ctx.emit.push(DstEmit::ResetSync {
                            dst: d,
                            adj: 0,
                            total: self.table.adj(d),
                        });
                    }
                }
            }
        }
    }

    /// Takes the newest stack entry of `src` out of `chg_stack[dst]`; when the aggregate `chg` changes the
    /// replica gets `Reset{dst, 0}` (clears it) and, if an older entry is now on top, `Set{dst, 0, prev}`.
    fn pop_chg(&mut self, src: DstSource, dst: u8, ctx: &mut DstCtx<'_>) {
        let d = u32::from(dst);
        let mut top = NULL_CHGPARAM;
        if let Some(stack) = self.chg_stack.get_mut(&dst) {
            if let Some(i) = stack.iter().rposition(|(s, _)| *s == src) {
                stack.remove(i);
            }
            top = stack.last().map_or(NULL_CHGPARAM, |(_, v)| *v);
            if stack.is_empty() {
                self.chg_stack.remove(&dst);
            }
        }
        if self.table.chg(d) == top {
            return;
        }
        // the aggregate follows the stack: clear, then set the older entry
        self.table.replica_reset(d, 0);
        if ctx.send {
            ctx.emit.push(DstEmit::Reset { dst: d, adj: 0 });
        }
        if top != NULL_CHGPARAM {
            self.table.replica_set(d, 0, top);
            if ctx.send {
                ctx.emit.push(DstEmit::Set {
                    dst: d,
                    adj: 0,
                    chg: top,
                });
            }
        }
    }
}
