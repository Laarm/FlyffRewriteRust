//! The snapshots of an economy batch (Plan/13 §17.3 rule 4, §18; Plan/19 §20.3): `snap(x)` is a fresh
//! `CharacterSnapshot` of a player with a **new** save generation, built in the same panic-free commit block as
//! the in-memory change it belongs to and sent inside one `AtomicBatch` (`Trade`, `VendorPurchase`).
//!
//! Items reserved by a trade or a stall are ordinary inventory items in the snapshot, and the wallet holds the
//! reserved penya (the snapshot builder writes the inventory as it is; `reserved` is runtime only).

use crate::ctx::{GameCtx, Services};
use crate::persist::snapshot::{self, SnapshotCtx};
use flyff_common::ObjId;
use flyff_db::{CharacterSnapshot, SaveReason};

/// `true` when a snapshot of `who` can be built: a live player whose session is not tainted (13 §20: never a
/// snapshot after a panic). The validate phase of a batch checks this for every party **before** it mutates.
pub fn can_snapshot(ctx: &GameCtx<'_>, who: ObjId) -> bool {
    ctx.sessions.get(who).is_some_and(|e| !e.tainted) && ctx.worlds.player(who).is_some()
}

/// A snapshot of `who` for an atomic batch: a fresh generation (`SaveReason::Atomic`), the scheduler's
/// bookkeeping updated (the periodic timer restarts, like after any snapshot). `None` when [`can_snapshot`] is
/// false.
pub fn atomic_snapshot(
    svc: &mut Services,
    ctx: &GameCtx<'_>,
    who: ObjId,
) -> Option<CharacterSnapshot> {
    let entry = ctx.sessions.get(who).filter(|e| !e.tainted)?;
    let mover = ctx.worlds.player(who)?;
    let generation = svc.saves.next_gen();
    let snap = snapshot::build(
        mover,
        &SnapshotCtx {
            entry,
            generation,
            reason: SaveReason::Atomic,
            now: ctx.clock.game_ms,
            unix_ms: ctx.clock.unix_ms,
            max_pack: u16::try_from(ctx.cfg.rules.gameplay.max_pack_item).unwrap_or(u16::MAX),
            location: None,
        },
    )?;
    svc.saves
        .record(who, generation, ctx.clock.game_ms, SaveReason::Atomic);
    Some(snap)
}
