//! The per-player pass and the join of the relation systems (Plan/21 §2.2, §3.4-§3.5, §11-§13; WP-SOCIAL-21..23):
//! couples, campus and honor titles share the per-player cadence, the join and the helpers below.
//!
//! * [`process`] (tick phase 5, right after the item pass): walks the in-world avatars in `ObjId` order once per tick,
//!   advances the cadence counter and drains the honor check flag of the 90 s pass; on a cadence step (every 16th
//!   tick, 1.0667 s) it runs the couple step, the campus buffs and the campus point recovery, in that order
//!   (`CUser::Process`, `User.cpp:417-447`);
//! * [`on_join`] (before the join batch, `WorldRules::join`): the campus id and points mirrors that ride in
//!   `SpawnSelf`, the pending proposal and the couple counter reset, and the first honor recheck - as the original
//!   `OnJoin` did, so that the nameplate title and `honor_progress` are right from the first message;
//! * [`purge`]: the memory side of the database purge;
//! * small helpers shared by the three systems: `avatar_of`, `commit`, `fill_d` (crate-private).

use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::tickets::{Pending, ServiceTicket, TicketResult};
use flyff_common::{CharacterId, ObjId};
use flyff_db::{AtomicBatch, AtomicKind, BatchReply, DbOp};

/// The live avatar of a character the directory lists as online.
pub(crate) fn avatar_of(svc: &Services, ctx: &GameCtx<'_>, c: CharacterId) -> Option<PlayerRef> {
    let o = svc.presence.get(c)?.online?.obj;
    ctx.worlds
        .player(o)
        .is_some_and(|m| !m.core.deleted)
        .then_some(PlayerRef(o))
}

/// The character behind a live avatar.
pub(crate) fn character_of(ctx: &GameCtx<'_>, who: PlayerRef) -> Option<CharacterId> {
    ctx.worlds
        .player(who.0)
        .and_then(|m| m.player())
        .map(|p| p.ident.character_id)
}

/// The name the avatar carries.
pub(crate) fn avatar_name(ctx: &GameCtx<'_>, who: PlayerRef) -> String {
    ctx.worlds
        .player(who.0)
        .and_then(|m| m.player())
        .map(|p| p.ident.name.clone())
        .unwrap_or_default()
}

/// Queues one atomic batch of `ops` (no snapshots) in the commit block of the memory change it belongs to. A failed
/// batch is a bug of the game (21 §2.4: the rules were checked first), so the memory is kept and the divergence is
/// logged for the operator.
pub(crate) fn commit(ctx: &mut GameCtx<'_>, label: &'static str, ops: Vec<DbOp>) {
    if ops.is_empty() {
        return;
    }
    let ticket = ctx.tickets.issue(Pending::Service(ServiceTicket::new(
        move |_svc, _ctx, result| match result {
            TicketResult::Committed(_) => {}
            TicketResult::Failed(e) => tracing::error!(
                batch = label,
                error = %e,
                "a social batch failed: the memory and the database diverge until the next restart"
            ),
            other => tracing::error!(
                batch = label,
                ?other,
                "unexpected completion of a social batch"
            ),
        },
    )));
    ctx.persist.atomic(AtomicBatch {
        reply: BatchReply::Game(ticket),
        ..AtomicBatch::of_ops(AtomicKind::Other(label), ops)
    });
}

/// `sprintf( text, a, b, ... )` for the `%d` conversions of a client text (`%%` is a percent sign); a missing
/// argument prints nothing. The client texts the server formats itself are few (`TID_GAME_COUPLE_M00`).
pub(crate) fn fill_d(fmt: &str, args: &[i64]) -> String {
    let mut out = String::with_capacity(fmt.len() + 8);
    let mut next = args.iter();
    let mut chars = fmt.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some('d') => {
                chars.next();
                if let Some(v) = next.next() {
                    out.push_str(&v.to_string());
                }
            }
            Some('%') => {
                chars.next();
                out.push('%');
            }
            _ => out.push('%'),
        }
    }
    out
}

/// The in-world avatars in `ObjId` order.
fn avatars(ctx: &GameCtx<'_>) -> Vec<ObjId> {
    ctx.sessions
        .iter()
        .map(|e| e.player)
        .filter(|o| ctx.worlds.player(*o).is_some())
        .collect()
}

/// Tick phase 5 (21 §2.2): the per-player pass of the relation systems. Every avatar in the world is visited once per
/// tick, a lingering one included (the original processed every `CUser` of the world); the cadence counter advances
/// for each, a cadence step runs the couple step, the campus buffs and the campus point recovery in that order.
pub fn process(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    let repairs = svc.social.campus.take_repairs();
    commit(ctx, "CampusRepair", repairs);
    if ctx.data.lua.campus.is_none() && svc.social.campus.note_inert() {
        tracing::warn!(
            "Campus.lua is not loaded: the campus system is inert (no invitations, no buffs, no points)"
        );
    }
    for o in avatars(ctx) {
        let who = PlayerRef(o);
        let (step, due) = match ctx.worlds.player_mut(o).and_then(|m| m.player_mut()) {
            Some(p) => {
                let step = p.social.advance();
                (step, std::mem::take(&mut p.social.honor_check_due))
            }
            None => continue,
        };
        if due {
            crate::social::honor::check_time(svc, ctx, who);
        }
        if step {
            crate::social::couple::exp::step(svc, ctx, who);
            crate::social::campus::process(svc, ctx, who);
            crate::social::campus::points::recover(svc, ctx, who);
        }
    }
}

/// `CDPDatabaseClient::OnJoin` for the relation systems, before the join batch: the campus mirrors (the campus id and
/// the points ride in `SpawnSelf`), no pending proposal, the couple counter at 0 and the honor recheck without
/// notification (`CheckHonorStat`; the list reaches the client in `SpawnSelf.honor_progress`).
pub fn on_join(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let Some(character) = character_of(ctx, who) else {
        return;
    };
    let campus = svc.social.campus.campus_id_of(character).unwrap_or(0);
    let points = svc.social.campus.points(character);
    if let Some(p) = ctx.worlds.player_mut(who.0).and_then(|m| m.player_mut()) {
        p.social.campus = campus;
        p.social.campus_point = points;
        p.social.proposer = None;
        p.social.couple_counter = 0;
        p.social.campus_tick = None;
        p.social.honor_check_due = false;
    }
    crate::social::honor::recheck(svc, ctx, who, false);
}

/// The memory side of the database purge (13 §14.2): couples and campuses that contain a purged character vanish
/// and their online members are told (`DecoupleResult`, `CampusRemove`).
pub fn purge(svc: &mut Services, ctx: &mut GameCtx<'_>, ids: &[CharacterId]) {
    let report = svc.social.purge(ids);
    for partner in report.decoupled {
        if let Some(o) = avatar_of(svc, ctx, partner) {
            crate::social::party::wire::to_one(ctx, o.0, flyff_proto::pb::DecoupleResult {});
        }
    }
    crate::social::campus::announce_dissolved(svc, ctx, report.dissolved);
    for id in report.updated {
        crate::social::campus::send_update(svc, ctx, id);
    }
}
