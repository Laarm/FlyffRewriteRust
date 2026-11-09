//! The pending-grant poll (Plan/13 §18 `GrantDelivery`, PL-055, WP-DB-07 / WP-DB-10).
//!
//! Scheduler job **P12** `grants_poll` (period `gameplay.grant_poll_s`, default 30 s, registered only when the
//! key is positive: `sched::registry`; `0` = the original login-only delivery, no job) asks the database for
//! the open grants of the characters that are in the world (`GameReadQuery::OpenGrants`, answered by
//! `DbReply::ReadDone`) and hands the ones the game has not delivered yet to the delivery that WP-ECON-09
//! installs in [`GrantGuard::deliver`] (`econ::grants::deliver`: gold is added, an item needs room, one
//! `GrantDelivery` batch per receiver and pass). The in-memory [`GrantGuard`] keeps one grant from being delivered
//! twice between the delivery and the commit of its `MarkGrantsDelivered` and for a while after it (a poll result
//! that was read before the commit can reach the game thread after it).
//!
//! The enter-world pass (`econ::grants::on_joined`) uses the same guard: [`GrantGuard::stash_join`] keeps the
//! grants and removals of the `CharacterLoad` from `WorldRules::join` (where the session entry does not exist yet)
//! to `WorldRules::joined`.

use crate::ctx::{GameCtx, Services};
use crate::tickets::{Pending, ServiceTicket, TicketResult};
use flyff_common::{CharacterId, GameMs};
use flyff_db::{GameReadQuery, GameReadResult, PendingGrantRecord, PendingRemovalRecord};
use std::collections::{BTreeMap, BTreeSet};

/// The delivery of doc 19 (`econ::grants::deliver`): receives the open grants of online characters that were
/// not delivered in this run. It marks each one it delivers with [`GrantGuard::deliver_once`] and sends the
/// `GrantDelivery` batch (`AtomicBatch::grant_delivery`).
pub type DeliverFn = fn(&mut Services, &mut GameCtx<'_>, Vec<PendingGrantRecord>);

/// How long a resolved id stays refused: the poll runs every 30 s and its answer is read on another thread, so a
/// result that was read before the commit can arrive a few seconds after it; ten minutes is generous.
pub const RECENT_MEMORY_MS: u64 = 10 * 60 * 1000;

/// The two phases of an id the game acted on: `in_flight` (planned or sent, the batch is unresolved) and `recent`
/// (resolved, kept for [`RECENT_MEMORY_MS`]).
#[derive(Clone, Debug, Default)]
struct Phases {
    in_flight: BTreeSet<i64>,
    recent: BTreeMap<i64, GameMs>,
}

impl Phases {
    fn blocks(&self, id: i64) -> bool {
        self.in_flight.contains(&id) || self.recent.contains_key(&id)
    }

    fn begin(&mut self, id: i64) -> bool {
        !self.blocks(id) && self.in_flight.insert(id)
    }

    fn resolve(&mut self, ids: &[i64], now: GameMs) {
        for id in ids {
            if self.in_flight.remove(id) {
                self.recent.insert(*id, now);
            }
        }
    }

    fn prune(&mut self, now: GameMs) {
        self.recent
            .retain(|_, at| now.0.saturating_sub(at.0) <= RECENT_MEMORY_MS);
    }

    fn len(&self) -> usize {
        self.in_flight.len() + self.recent.len()
    }
}

/// The grants and removals of one `CharacterLoad`, kept between `join` and `joined`.
#[derive(Clone, Debug, Default)]
pub struct JoinLoad {
    /// The open grants of the load, oldest first.
    pub grants: Vec<PendingGrantRecord>,
    /// The open character removals of the load, oldest first.
    pub removals: Vec<PendingRemovalRecord>,
}

/// The ids of the grants and removals the game acted on and the stash of the enter-world pass (13 §18, 19 §20.4).
#[derive(Clone, Debug, Default)]
pub struct GrantGuard {
    grants: Phases,
    removals: Phases,
    joining: BTreeMap<CharacterId, JoinLoad>,
    /// The delivery the poll feeds; `None` until WP-ECON-09 installs it (the poll then reads nothing).
    pub deliver: Option<DeliverFn>,
    /// The admin console's side: grants whose cancel is in flight (never delivered meanwhile) and the answers of
    /// the last `GrantBatch` requests (Plan/25 §5.3, §5.6; WP-ADMIN-20).
    pub admin: crate::admin::actions::grants::GrantLedger,
}

impl GrantGuard {
    /// Marks `id` as in flight (the game plans to deliver it or sent the batch); `true` when it was new (deliver
    /// it), `false` when it is cancelling (the admin console), in flight or was resolved a moment ago.
    pub fn deliver_once(&mut self, id: i64) -> bool {
        // a grant whose admin cancel is in flight is not delivered (it would be both, 25 §5.6)
        !self.admin.is_cancelling(id) && self.grants.begin(id)
    }

    /// `true` when [`GrantGuard::deliver_once`] would refuse `id` (the planning of a pass asks this first, so that
    /// only the ids that go into the batch enter the guard).
    pub fn refuses(&self, id: i64) -> bool {
        self.admin.is_cancelling(id) || self.grants.blocks(id)
    }

    /// `true` when `id` is in flight or was resolved in the last ten minutes (the poll drops such ids).
    pub fn was_delivered(&self, id: i64) -> bool {
        self.grants.blocks(id)
    }

    /// `true` while the batch of `id` is unresolved: the admin console's cancel answers `delivery_in_progress`.
    /// After the commit the database's `AlreadyDelivered` is the better answer.
    pub fn in_flight(&self, id: i64) -> bool {
        self.grants.in_flight.contains(&id)
    }

    /// The `GrantDelivery` batch of `ids` committed: they stay refused for [`RECENT_MEMORY_MS`].
    pub fn resolve(&mut self, ids: &[i64], now: GameMs) {
        self.grants.resolve(ids, now);
    }

    /// Forgets `id` (the delivery was abandoned before anything happened and is retried).
    pub fn forget(&mut self, id: i64) {
        self.grants.in_flight.remove(&id);
    }

    /// Forgets the resolved ids older than [`RECENT_MEMORY_MS`] (the poll does it).
    pub fn prune(&mut self, now: GameMs) {
        self.grants.prune(now);
        self.removals.prune(now);
    }

    /// Number of remembered grant ids (both phases).
    pub fn len(&self) -> usize {
        self.grants.len()
    }

    /// `true` when no grant id is remembered.
    pub fn is_empty(&self) -> bool {
        self.grants.len() == 0
    }

    /// `true` when the removal `id` was applied in this run and may not have been committed yet (or a moment ago).
    pub fn removal_refuses(&self, id: i64) -> bool {
        self.removals.blocks(id)
    }

    /// `true` while the `RemovalApply` batch of `id` is unresolved: the admin console's cancel of it is refused.
    pub fn removal_in_flight(&self, id: i64) -> bool {
        self.removals.in_flight.contains(&id)
    }

    /// Marks the removal `id` as applied; `true` when it was new.
    pub fn removal_once(&mut self, id: i64) -> bool {
        self.removals.begin(id)
    }

    /// The `RemovalApply` batch of `ids` committed.
    pub fn removal_resolved(&mut self, ids: &[i64], now: GameMs) {
        self.removals.resolve(ids, now);
    }

    /// Keeps the grants and removals of a load for `joined` (`WorldRules::join` calls it last; a load without any
    /// stores nothing).
    pub fn stash_join(&mut self, character: CharacterId, load: JoinLoad) {
        if load.grants.is_empty() && load.removals.is_empty() {
            self.joining.remove(&character);
        } else {
            self.joining.insert(character, load);
        }
    }

    /// Takes the stash of `character` (`WorldRules::joined`).
    pub fn take_join(&mut self, character: CharacterId) -> Option<JoinLoad> {
        self.joining.remove(&character)
    }
}

/// P12 `item::grants::poll`: one `OpenGrants` read for the characters in the world (a no-op without a delivery
/// or without anybody online). The answer is handled by `on_open_grants` when the completion arrives.
pub fn poll(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    if svc.grants.deliver.is_none() {
        return;
    }
    svc.grants.prune(ctx.clock.game_ms);
    let online: Vec<CharacterId> = svc
        .presence
        .iter()
        .filter(|e| e.online.is_some())
        .map(|e| e.id)
        .collect();
    if online.is_empty() {
        return;
    }
    let ticket = ctx
        .tickets
        .issue(Pending::Service(ServiceTicket::new(on_open_grants)));
    ctx.persist.read_for_game(
        ticket,
        GameReadQuery::OpenGrants {
            character_ids: online,
            after_id: 0,
        },
    );
}

/// The continuation of the poll: the open grants the game has not delivered yet, of characters that are still in
/// the world, go to the installed delivery. A failed read is logged; the next poll asks again.
fn on_open_grants(svc: &mut Services, ctx: &mut GameCtx<'_>, result: TicketResult) {
    let grants = match result {
        TicketResult::ReadDone(Ok(GameReadResult::Grants(grants))) => grants,
        TicketResult::ReadDone(Err(e)) => {
            tracing::warn!(error = %e, "the grant poll failed; it asks again");
            return;
        }
        other => {
            tracing::error!(?other, "the grant poll got an unexpected answer");
            return;
        }
    };
    let Some(deliver) = svc.grants.deliver else {
        return;
    };
    let due: Vec<PendingGrantRecord> = grants
        .into_iter()
        .filter(|g| !svc.grants.was_delivered(g.id) && !svc.grants.admin.is_cancelling(g.id))
        .filter(|g| {
            svc.presence
                .get(g.character_id)
                .is_some_and(|e| e.online.is_some())
        })
        .collect();
    if !due.is_empty() {
        deliver(svc, ctx, due);
    }
}
