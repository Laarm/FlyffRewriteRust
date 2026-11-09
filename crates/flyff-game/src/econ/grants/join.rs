//! The enter-world pass of the grants and removals (Plan/13 §17.4 step 8, 19 §20.4; WP-ECON-09).
//!
//! The dispatcher inserts the `SessionEntry` AFTER `WorldRules::join` returns, and the batches of a pass need it
//! (`snapshot_for_batch`), so the work is split: [`stash`] keeps the grants and removals of the `CharacterLoad` at the
//! very end of `join`, and [`on_joined`] (called from `WorldRules::joined`, when the session exists) applies the
//! removals FIRST (so that "remove all X" cannot eat the item a grant brings) and then delivers the grants. Both
//! arrive as ordinary item messages after the join batch (a deviation from the plan's "in the load step").
//!
//! `CharacterLoad.grants` and `CharacterLoad.removals` exclude delivered, done and cancelled rows (migration 0008),
//! and the guard turns away an id the game acted on in this run whose batch may not have committed yet. The pass
//! works whatever `gameplay.grant_poll_s` is. It only ever produces `AtomicKind::{GrantDelivery, RemovalApply}`:
//! `AdminOfflineEdit` is the admin console's, under `OnlineRegistry::admin_hold` (the account's login waits with
//! 503 meanwhile, so a join pass never races an offline edit).

use super::{apply_removals, deliver};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::persist::grants::JoinLoad;
use crate::social::relation::character_of;
use flyff_db::CharacterLoad;

/// Keeps the grants and removals of `load` for [`on_joined`].
pub fn stash(svc: &mut Services, load: &CharacterLoad) {
    svc.grants.stash_join(
        load.identity.id,
        JoinLoad {
            grants: load.grants.clone(),
            removals: load.removals.clone(),
        },
    );
}

/// The pass of a character that just joined: removals, then grants.
pub fn on_joined(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let Some(character) = character_of(ctx, who) else {
        return;
    };
    let Some(load) = svc.grants.take_join(character) else {
        return;
    };
    if !load.removals.is_empty() {
        apply_removals(svc, ctx, who, load.removals);
    }
    if !load.grants.is_empty() {
        deliver(svc, ctx, load.grants);
    }
}
