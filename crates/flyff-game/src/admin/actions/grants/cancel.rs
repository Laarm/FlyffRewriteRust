//! Cancelling pending grants and removals (Plan/25 §5.6; 19 §20.4) and the in-memory ledger of the grant commands.
//!
//! **A cancel racing the delivery is never both cancelled and delivered.** Three layers hold:
//! 1. the database: `CancelPendingGrant` is `UPDATE .. SET cancelled_at WHERE delivered_at IS NULL AND cancelled_at
//!    IS NULL`, `MarkGrantsDelivered` leaves a cancelled row open (`flyff-db` NOTES);
//! 2. the game thread refuses (`DeliveryInProgress`) while the id is in the guard's *in flight* phase (planned or
//!    sent, the mark has not committed yet; after the commit the database's `AlreadyDelivered` answers instead), and
//!    while the cancel is in flight the id is in [`GrantLedger`]: the poll's
//!    `deliver_once` refuses it and `on_open_grants` drops it from a result that was read before the cancel
//!    committed;
//! 3. a cancel that changed no row says `AlreadyDelivered` (the route reads the row to tell delivered from cancelled
//!    from unknown).
//!
//! The commands **defer their answer** to the commit of the update ([`super::super::run_deferred`]).

use super::{CancelPendingGrant, CancelPendingRemoval, GrantedReport};
use crate::admin::types::{AdminErrorCode, AdminFailure};
use crate::ctx::{GameCtx, Services};
use crate::input::{AdminActor, AdminOutput};
use crate::tickets::{Pending, ServiceTicket, TicketResult};
use flyff_common::rt::Reply;
use flyff_db::{AtomicBatch, AtomicKind, AtomicOutcome, BatchReply, DbOp};
use std::collections::{BTreeMap, BTreeSet};

/// How long the game remembers the `request_id` of a grant batch (25 §5.3).
pub const REQUEST_MEMORY_MS: u64 = 10 * 60 * 1000;
/// The most requests remembered at once (a flood cannot grow the ledger).
const REQUESTS_MAX: usize = 4_096;

/// How long the game remembers a grant it cancelled: a poll result read before the cancel committed can still
/// arrive a few seconds later (the poll runs every 30 s), so the id must stay refused for a while.
pub const CANCELLED_MEMORY_MS: u64 = 5 * 60 * 1000;

/// The in-memory state of the grant commands: grants whose cancel is in flight or was committed a moment ago and
/// the answers of the last ten minutes of `GrantBatch` requests. Part of
/// [`GrantGuard`](crate::persist::grants::GrantGuard).
#[derive(Clone, Debug, Default)]
pub struct GrantLedger {
    cancelling: BTreeSet<i64>,
    cancelled: BTreeMap<i64, u64>,
    requests: BTreeMap<u128, (u64, GrantedReport)>,
}

impl GrantLedger {
    /// Marks `id` as being cancelled; `false` when a cancel of it is in flight already.
    pub fn begin_cancel(&mut self, id: i64) -> bool {
        self.cancelling.insert(id)
    }

    /// The cancel of `id` finished: `committed` keeps the id refused for `CANCELLED_MEMORY_MS` (a stale poll
    /// result may still list it).
    pub fn end_cancel(&mut self, id: i64, now_ms: u64, committed: bool) {
        self.cancelling.remove(&id);
        if committed {
            self.cancelled.insert(id, now_ms);
        }
    }

    /// `true` while a cancel of `id` is in flight or was committed a moment ago: the delivery must not take it.
    pub fn is_cancelling(&self, id: i64) -> bool {
        self.cancelling.contains(&id) || self.cancelled.contains_key(&id)
    }

    /// Remembers the answer of a request (`now_ms` is the game clock).
    pub fn remember(&mut self, now_ms: u64, report: GrantedReport) {
        self.prune(now_ms);
        if self.requests.len() >= REQUESTS_MAX
            && let Some(oldest) = self
                .requests
                .iter()
                .min_by_key(|(_, (at, _))| *at)
                .map(|(k, _)| *k)
        {
            self.requests.remove(&oldest);
        }
        self.requests.insert(report.request_id, (now_ms, report));
    }

    /// The stored answer of a request carried out within the last ten minutes.
    pub fn recall(&self, request_id: u128, now_ms: u64) -> Option<&GrantedReport> {
        self.requests
            .get(&request_id)
            .filter(|(at, _)| now_ms.saturating_sub(*at) <= REQUEST_MEMORY_MS)
            .map(|(_, r)| r)
    }

    /// Forgets the requests older than ten minutes and the cancels older than five.
    pub fn prune(&mut self, now_ms: u64) {
        self.requests
            .retain(|_, (at, _)| now_ms.saturating_sub(*at) <= REQUEST_MEMORY_MS);
        self.cancelled
            .retain(|_, at| now_ms.saturating_sub(*at) <= CANCELLED_MEMORY_MS);
    }

    /// Number of remembered requests.
    pub fn remembered(&self) -> usize {
        self.requests.len()
    }
}

fn refuse(reply: Reply<AdminOutput>, code: AdminErrorCode, message: impl Into<String>) {
    let _ = reply.send(AdminOutput::Failed(AdminFailure::new(code, message)));
}

/// Sends the conditional update and answers `reply` when it committed. `on_done` runs first on the game thread
/// (the grant guard is released there).
fn send_cancel(
    ctx: &mut GameCtx<'_>,
    op: DbOp,
    what: &'static str,
    id: i64,
    reply: Reply<AdminOutput>,
    on_done: impl FnOnce(&mut Services, u64, bool) + Send + 'static,
) {
    let ticket = ctx.tickets.issue(Pending::Service(ServiceTicket::new(
        move |svc: &mut Services, ctx: &mut GameCtx<'_>, result: TicketResult| {
            let committed = matches!(&result, TicketResult::Committed(AtomicOutcome::Changed(n)) if *n > 0);
            on_done(svc, ctx.clock.game_ms.0, committed);
            let out = match result {
                TicketResult::Committed(AtomicOutcome::Changed(n)) if n > 0 => {
                    AdminOutput::Done { affected: n }
                }
                TicketResult::Committed(_) => AdminOutput::Failed(AdminFailure::new(
                    AdminErrorCode::AlreadyDelivered,
                    format!("no open {what} {id}: it was done, cancelled before or does not exist"),
                )),
                TicketResult::Failed(e) => {
                    tracing::error!(%id, error = %e, "cancel of a pending {what} failed");
                    AdminOutput::Failed(AdminFailure::new(
                        AdminErrorCode::Internal,
                        "the database refused the cancel",
                    ))
                }
                other => {
                    tracing::error!(%id, ?other, "cancel of a pending {what} got an unexpected answer");
                    AdminOutput::Failed(AdminFailure::new(
                        AdminErrorCode::Internal,
                        "the database answered unexpectedly",
                    ))
                }
            };
            let _ = reply.send(out);
        },
    )));
    ctx.persist.atomic(AtomicBatch {
        reply: BatchReply::Game(ticket),
        ..AtomicBatch::of_ops(AtomicKind::Other("admin_cancel"), vec![op])
    });
}

/// `CancelPendingGrant`: see the module documentation. Answers [`AdminOutput::Done`]`{affected: 1}` when the row
/// was cancelled, `Failed(DeliveryInProgress)` while the game delivers it, `Failed(AlreadyDelivered)` when no open
/// row changed.
pub(crate) fn cancel_pending_grant(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    actor: AdminActor,
    cmd: CancelPendingGrant,
    reply: Reply<AdminOutput>,
) {
    let id = cmd.id;
    if id <= 0 {
        return refuse(reply, AdminErrorCode::Invalid, "a grant id is positive");
    }
    if svc.grants.in_flight(id) {
        return refuse(
            reply,
            AdminErrorCode::DeliveryInProgress,
            format!("grant {id} is being delivered; its mark has not committed yet"),
        );
    }
    if !svc.grants.admin.begin_cancel(id) {
        return refuse(
            reply,
            AdminErrorCode::DeliveryInProgress,
            format!("grant {id} is being cancelled already"),
        );
    }
    send_cancel(
        ctx,
        DbOp::CancelPendingGrant {
            id,
            by: actor.label(),
        },
        "grant",
        id,
        reply,
        move |svc, now_ms, committed| svc.grants.admin.end_cancel(id, now_ms, committed),
    );
}

/// `CancelPendingRemoval`: the same rules for a removal. The enter-world pass applies a removal in memory and marks
/// it with its `RemovalApply` batch; while that batch is unresolved the cancel is refused (`DeliveryInProgress`), after
/// it the conditional update says `AlreadyDelivered`.
pub(crate) fn cancel_pending_removal(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    actor: AdminActor,
    cmd: CancelPendingRemoval,
    reply: Reply<AdminOutput>,
) {
    let id = cmd.id;
    if id <= 0 {
        return refuse(reply, AdminErrorCode::Invalid, "a removal id is positive");
    }
    if svc.grants.removal_in_flight(id) {
        return refuse(
            reply,
            AdminErrorCode::DeliveryInProgress,
            format!("removal {id} is being applied; its mark has not committed yet"),
        );
    }
    send_cancel(
        ctx,
        DbOp::CancelPendingRemoval {
            id,
            by: actor.label(),
        },
        "removal",
        id,
        reply,
        |_, _, _| {},
    );
}
