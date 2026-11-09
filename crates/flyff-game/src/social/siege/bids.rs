//! Bidding (Plan/21 §7.3; `GuildCombatRequest` / `GuildCombatCancel` `src:_Common/eveschool.cpp:816-960`, `OnGCApp`
//! `WORLDSERVER/DPSrvr.cpp:7595-7635`, the database answers `DPDatabaseClient.cpp:1412-1500`): the ranking, the
//! bid ([`request`]), the cancel ([`cancel`]), the three C2S handlers and the questions guild-core asks
//! ([`invite_locked`], [`leave_locked`], [`dissolve_locked`]).
//!
//! Every handler acts for the session's player (VR-001) and ignores the request while `events.guild_combat` is off.
//! A bid and a cancel are ONE `AtomicBatch` each, validated first and committed in one block with the memory change,
//! the messages and the audit line (VR-018): the original deducted the penya in the world and asked the database
//! afterwards, so a crash in between lost it.
//!
//! No NPC proximity is required for bidding (Plan/21 OQ13, KEEP).

use super::schedule::SIEGE_OPEN;
use super::{SiegeBidState, SiegeService, SiegeState, SiegeWarState};
use crate::chat::ingress::notice;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::econ::mail::MailEntry;
use crate::econ::mail::commit::{can_snapshot, on_send_result, snapshot_for_batch, submit};
use crate::gm::text::format_positional;
use crate::tickets::TicketResult;
use flyff_common::config::EventId;
use flyff_common::defs::text::{
    TID_GAME_GC_CANCELREQUEST, TID_GAME_GC_CANCELREQUEST1, TID_GAME_GUILDCOMBAT_APP,
    TID_GAME_GUILDCOMBAT_APP_CONFIRM, TID_GAME_GUILDCOMBAT_CANCEL,
    TID_GAME_GUILDCOMBAT_EXPENSE_RETURN, TID_GAME_GUILDCOMBAT_NOT_GUILD_LEADER,
    TID_GAME_GUILDCOMBAT_NOT_USE, TID_GAME_GUILDCOMBAT_NOTAPP,
    TID_GAME_GUILDCOMBAT_REQUEST_BASEPENYA, TID_GAME_GUILDCOMBAT_REQUEST_GUILDCOMBAT,
    TID_GAME_GUILDCOMBAT_REQUEST_LEVEL, TID_GAME_GUILDCOMBAT_REQUEST_NOTPENYA,
    TID_GAME_GUILDCOMBAT_REQUEST_PENYA, TID_GAME_GUILDCOMBAT_REQUESTEND,
    TID_GAME_GUILDCOMBAT1TO1_ISGC1TO1TENDER, TID_GAME_LACKMONEY,
};
use flyff_common::{CharacterId, GuildId, UnixTime};
use flyff_db::audit::social as audit;
use flyff_db::{AtomicBatch, BidStatus, GuildSiegeBidRecord};
use flyff_proto::pb;
use flyff_rules::social::muldiv;

// ---- the questions of other documents ------------------------------------------------------------------------------

/// `true` when `guild` has an active bid (`IsRequestWarGuild(guild, TRUE)`).
pub fn is_bidder(svc: &Services, guild: GuildId) -> bool {
    svc.social.siege.is_bidder(guild)
}

/// A bidder may not invite while the siege is not closed (21 §5.5 row 1, §7.7;
/// `TID_GAME_GUILDCOMBAT_NOT_INVITATION_GUILD`, sent by guild-core). Installed as
/// `GuildSeams::siege_invite_locked`.
pub fn invite_locked(svc: &Services, guild: GuildId) -> bool {
    svc.social.siege.is_bidder(guild) && svc.social.siege.state != SiegeState::Close
}

/// FIX of a rule the original enforced only in the client (`WndGuildTabMember`): a bidder may not lose a member by
/// leave or kick while the siege is not closed, except in the war state `WarClose` (105,
/// `TID_GAME_GUILDCOMBAT_NOT_LEAVE_GUILD`). Installed as `GuildSeams::siege_leave_locked`.
pub fn leave_locked(svc: &Services, guild: GuildId) -> bool {
    svc.social.siege.is_bidder(guild)
        && svc.social.siege.state != SiegeState::Close
        && svc.social.siege.war_state != SiegeWarState::WarClose
}

/// FIX (`WndGuildDisMiss`, client only): a bidder may not dissolve its guild at all
/// (`TID_GAME_GUILDCOMBAT_NOT_DISSOLVE_GUILD`). Installed as `GuildSeams::siege_dissolve_locked`.
pub fn dissolve_locked(svc: &Services, guild: GuildId) -> bool {
    svc.social.siege.is_bidder(guild)
}

/// The guild is a tenderer of the 1:1 guild combat (`GetTenderGuildIndexByUser`): such a guild cannot bid. `false`
/// until WP-SOCIAL-25 replaces the body.
pub fn gc1to1_tenderer(_svc: &Services, _guild: GuildId) -> bool {
    false
}

// ---- texts ---------------------------------------------------------------------------------------------------------------

/// The text of `tid` (`prj.GetText`).
fn text_of(ctx: &GameCtx<'_>, tid: u32) -> String {
    ctx.data.texts.get(tid).to_owned()
}

/// The text of `tid` with the printf conversions replaced by `args` in order.
fn text_with(ctx: &GameCtx<'_>, tid: u32, args: &[String]) -> String {
    format_positional(ctx.data.texts.get(tid), args)
}

fn enabled(ctx: &GameCtx<'_>) -> bool {
    ctx.live.event_state(EventId::GuildCombat) != 0
}

// ---- the actor -------------------------------------------------------------------------------------------------------------

/// The character and guild of `who` when it is the master of a guild.
fn master_of(svc: &Services, ctx: &GameCtx<'_>, who: PlayerRef) -> Option<(CharacterId, GuildId)> {
    let c = crate::social::guild::character_of(ctx, who)?;
    let guild = svc.social.guild.guild_of(c)?;
    svc.social.guild.is_master(c).then_some((c, guild))
}

/// Sends `GuildCombat{GC_ISREQUEST{request}}` to the online members of `guild`.
fn send_is_request(svc: &Services, ctx: &mut GameCtx<'_>, guild: GuildId, request: bool) {
    super::send_guild(
        svc,
        ctx,
        guild,
        pb::guild_combat::Sub::GcIsRequest(pb::guild_combat::GcIsRequest { request }),
    );
}

/// A chat line (`AddText`) to every online member of `guild` (`CUserMng::AddGuildMsg`).
fn guild_line(svc: &Services, ctx: &mut GameCtx<'_>, guild: GuildId, line: &str) {
    for obj in svc.social.guild.online_objs(&svc.presence, guild) {
        notice::text(ctx, PlayerRef(obj), line);
    }
}

// ---- the bid list --------------------------------------------------------------------------------------------------------

impl SiegeService {
    /// Applies a bid of `fee` for `guild` in memory (`JoinGuildCombat(guild, fee, TRUE)`): a new guild is appended, an
    /// existing entry (active or not) is updated in place and keeps its `applied_at`. The status is 10 for a new
    /// entry and for a raise of a 10 entry, 12 when the entry was cancelled (11), applied again (12) or came back as
    /// 20 after a restart. Returns the entry as it was (`None` = new, for [`SiegeService::restore_bid`]) and the row
    /// for `UpsertGuildSiegeBid`.
    pub(super) fn apply_bid(
        &mut self,
        guild: GuildId,
        fee: u32,
        now: UnixTime,
    ) -> (Option<SiegeBidState>, GuildSiegeBidRecord) {
        let siege_id = self.siege_id;
        let previous = self.entry_of(guild).cloned();
        let status = match &previous {
            None => BidStatus::Applied,
            Some(e) if e.active && e.status == BidStatus::Applied => BidStatus::Applied,
            Some(_) => BidStatus::Reapplied,
        };
        let entry = SiegeBidState {
            guild,
            fee,
            active: true,
            status,
            applied_at: previous.as_ref().map_or(now, |e| e.applied_at),
        };
        let record = entry.record(siege_id, status);
        match self.bids.iter_mut().find(|b| b.guild == guild) {
            Some(slot) => *slot = entry,
            None => self.bids.push(entry),
        }
        (previous, record)
    }

    /// Undoes [`SiegeService::apply_bid`].
    pub(super) fn restore_bid(&mut self, guild: GuildId, previous: Option<SiegeBidState>) {
        match previous {
            Some(p) => {
                if let Some(slot) = self.bids.iter_mut().find(|b| b.guild == guild) {
                    *slot = p;
                }
            }
            None => self.bids.retain(|b| b.guild != guild),
        }
    }

    /// Cancels the active bid of `guild` in memory (`OutGuildCombat`: `bRequest = FALSE`, `dwPenya = 0`); returns the
    /// row for `UpsertGuildSiegeBid` (status 11 with the old bid, the refund and `cancelled_at`).
    pub(super) fn cancel_bid(
        &mut self,
        guild: GuildId,
        refund: i64,
        now: UnixTime,
    ) -> Option<GuildSiegeBidRecord> {
        let siege_id = self.siege_id;
        let e = self
            .bids
            .iter_mut()
            .find(|b| b.guild == guild && b.active)?;
        let mut record = e.record(siege_id, BidStatus::Cancelled);
        record.refund = refund;
        record.cancelled_at = Some(now);
        e.fee = 0;
        e.active = false;
        e.status = BidStatus::Cancelled;
        Some(record)
    }
}

// ---- InGuildCombat -----------------------------------------------------------------------------------------------------

/// `InGuildCombat` (`OnGCApp`): the window or a bid. Both are for the master only.
pub fn on_in_guild_combat(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::InGuildCombat,
) {
    if !enabled(ctx) {
        return;
    }
    match m.sub {
        Some(pb::in_guild_combat::Sub::GcInWindow(_)) => in_window(svc, ctx, who),
        Some(pb::in_guild_combat::Sub::GcInApp(app)) => request(svc, ctx, who, app.penya),
        None => {}
    }
}

/// `GC_IN_WINDOW`: not the master -> `TID_GAME_GUILDCOMBAT_NOT_GUILD_LEADER` (message box); a GC 1:1 tenderer ->
/// `TID_GAME_GUILDCOMBAT1TO1_ISGC1TO1TENDER`; else `GC_IN_WINDOW{prize, my_bid, min_bid}` in ANY state (the window
/// shows `REQUESTEND` itself): the prize is the sum of the first `MAXJOINGUILDSIZE` ranked bids.
fn in_window(svc: &Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let Some((_, guild)) = master_of(svc, ctx, who) else {
        let line = text_of(ctx, TID_GAME_GUILDCOMBAT_NOT_GUILD_LEADER);
        notice::diag(ctx, who, &line);
        return;
    };
    if (svc.social.siege.seams.gc1to1_tenderer)(svc, guild) {
        notice::defined(
            ctx,
            who,
            TID_GAME_GUILDCOMBAT1TO1_ISGC1TO1TENDER,
            Vec::new(),
        );
        return;
    }
    let s = &svc.social.siege;
    super::send_one(
        ctx,
        who.0,
        pb::guild_combat::Sub::GcInWindow(pb::guild_combat::GcInWindow {
            prize: s.prize_pool(),
            my_bid: s.fee_of(guild),
            min_bid: u32::try_from(s.config.join_penya).unwrap_or(0),
        }),
    );
}

/// `GuildCombatRequest`: the master bids `penya` for its guild (also `/gcrquest`, WP-GM-24). Refusals, in this
/// order, change nothing: not the master (`NOT_GUILD_LEADER`, message box); a GC 1:1 tenderer
/// (`ISGC1TO1TENDER`, the seam is false until WP-SOCIAL-25); state not `Close` (`REQUESTEND`, box); guild level
/// below `GUILDLEVEL` (`REQUEST_LEVEL`, box, the level as argument); `penya > i32::MAX` (silent); `penya` below
/// `JOINPENYA` (`REQUEST_BASEPENYA`, box, plus `TID_GAME_LACKMONEY`); `penya` not above the existing bid
/// (`REQUEST_PENYA`, box); gold short of the difference (`REQUEST_NOTPENYA`, box).
///
/// The commit (VR-018, validate-then-commit): `can_snapshot` first, the difference leaves the master, the entry is
/// appended or updated and re-ranked, the master is snapshotted AFTER the debit and ONE `AtomicBatch::siege_bid`
/// (the snapshot and the bid row, status 10 or 12) goes out; then, in the same block, the confirmation box
/// (`APP_CONFIRM` with `0, penya` for a first bid, else `existing, penya - existing`), `GC_IN_COMPLETE` to the
/// master, `GC_ISREQUEST{true}` and the two guild lines to the online members, and the audit line `siege.bid`.
pub fn request(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, penya: u32) {
    let Some((master, guild)) = master_of(svc, ctx, who) else {
        let line = text_of(ctx, TID_GAME_GUILDCOMBAT_NOT_GUILD_LEADER);
        notice::diag(ctx, who, &line);
        return;
    };
    if (svc.social.siege.seams.gc1to1_tenderer)(svc, guild) {
        // the rule of the window, applied to the bid itself (the original left the direct packet open; crate NOTES)
        notice::defined(
            ctx,
            who,
            TID_GAME_GUILDCOMBAT1TO1_ISGC1TO1TENDER,
            Vec::new(),
        );
        return;
    }
    if svc.social.siege.state != SiegeState::Close {
        let line = text_of(ctx, TID_GAME_GUILDCOMBAT_REQUESTEND);
        notice::diag(ctx, who, &line);
        return;
    }
    let (join_penya, min_level) = {
        let c = &svc.social.siege.config;
        (c.join_penya, c.guild_level)
    };
    let level = svc.social.guild.get(guild).map_or(0, |g| g.level);
    if level < min_level {
        let line = text_with(
            ctx,
            TID_GAME_GUILDCOMBAT_REQUEST_LEVEL,
            &[min_level.to_string()],
        );
        notice::diag(ctx, who, &line);
        return;
    }
    let existing = svc.social.siege.fee_of(guild);
    let Ok(penya_i) = i32::try_from(penya) else {
        return; // above INT_MAX: silent
    };
    if penya_i < join_penya {
        let line = text_with(
            ctx,
            TID_GAME_GUILDCOMBAT_REQUEST_BASEPENYA,
            &[join_penya.to_string()],
        );
        notice::diag(ctx, who, &line);
        let lack = text_of(ctx, TID_GAME_LACKMONEY);
        notice::text(ctx, who, &lack);
        return;
    }
    if penya <= existing {
        let line = text_with(
            ctx,
            TID_GAME_GUILDCOMBAT_REQUEST_PENYA,
            &[penya.to_string()],
        );
        notice::diag(ctx, who, &line);
        return;
    }
    let diff = penya - existing;
    let gold = ctx.player(who).map_or(0, crate::item::gold::core::gold);
    if gold < i64::from(diff) {
        let line = text_of(ctx, TID_GAME_GUILDCOMBAT_REQUEST_NOTPENYA);
        notice::diag(ctx, who, &line);
        return;
    }
    if !can_snapshot(ctx, who) {
        tracing::debug!("a siege bid of a player without a savable session was refused");
        return;
    }

    // ---- the commit block: nothing below can fail ----
    let now = crate::social::party::now_secs(ctx);
    crate::item::gold::add_gold(svc, ctx, who, -i64::from(diff), true);
    let (previous, record) = svc.social.siege.apply_bid(guild, penya, now);
    let Some((snap, generation)) = snapshot_for_batch(svc, ctx, who) else {
        // unreachable after `can_snapshot`; undo rather than lose the penya
        svc.social.siege.restore_bid(guild, previous);
        crate::item::gold::add_gold(svc, ctx, who, i64::from(diff), true);
        tracing::error!("a siege bid lost its snapshot after the check: undone");
        return;
    };
    let status = record.status.code();
    let siege_id = record.siege_id;
    // the first bid of a fresh database creates the row of the siege in the same batch (the foreign key)
    let row_ops = svc.social.siege.row_ops(now);
    let creates_row = !row_ops.is_empty();
    let mut batch = AtomicBatch::siege_bid(snap, record);
    batch.ops.splice(0..0, row_ops);
    submit(
        svc,
        ctx,
        who,
        Some(generation),
        batch,
        move |svc, _ctx, result| {
            if creates_row && matches!(result, TicketResult::Committed(_)) {
                svc.social.siege.row_created();
            }
            super::log_batch_result("SiegeBid", &result);
        },
    );
    let args = if existing == 0 {
        [0.to_string(), penya.to_string()]
    } else {
        [existing.to_string(), diff.to_string()]
    };
    let confirm = text_with(ctx, TID_GAME_GUILDCOMBAT_APP_CONFIRM, &args);
    notice::diag(ctx, who, &confirm);
    super::send_one(
        ctx,
        who.0,
        pb::guild_combat::Sub::GcInComplete(pb::guild_combat::GcInComplete {}),
    );
    send_is_request(svc, ctx, guild, true);
    let first = text_of(ctx, TID_GAME_GUILDCOMBAT_REQUEST_GUILDCOMBAT);
    let second = text_of(ctx, TID_GAME_GUILDCOMBAT_APP);
    guild_line(svc, ctx, guild, &first);
    guild_line(svc, ctx, guild, &second);
    let mut ev = audit::siege_bid(siege_id, guild, master, i64::from(penya), status);
    if let Some(actor) = crate::audit::actor_of(ctx, who) {
        ev = ev.by(actor);
    }
    ctx.audit.emit(ev);
}

// ---- OutGuildCombat ----------------------------------------------------------------------------------------------------

/// `OutGuildCombat` (the handler): the master withdraws the bid.
pub fn on_out_guild_combat(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    _m: pb::OutGuildCombat,
) {
    if enabled(ctx) {
        cancel(svc, ctx, who);
    }
}

/// `GuildCombatCancel` (also `/gccancel`): not the master -> `NOT_GUILD_LEADER` (a chat line here); no active bid ->
/// `TID_GAME_GUILDCOMBAT_NOTAPP`; state not `Close` -> `TID_GAME_GUILDCOMBAT_NOT_USE`. The refund is
/// `muldiv(bid, REQUESTCANCLEPERCENT, 100)` penya (100 000 001 -> 80 000 001), a system mail to the CURRENT master
/// (`TID_GAME_GC_CANCELREQUEST` / `..1`, no item), in ONE `AtomicBatch::siege_cancel` with the bid row in status 11
/// and no snapshot (a second cancel is a `Conflict` in the database: it can never mail a second refund). The entry
/// becomes inactive with fee 0; the master gets `EXPENSE_RETURN`, the online members `TID_GAME_GUILDCOMBAT_CANCEL`
/// and `GC_ISREQUEST{false}`; audit `siege.cancel`.
pub fn cancel(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let Some((master, guild)) = master_of(svc, ctx, who) else {
        let line = text_of(ctx, TID_GAME_GUILDCOMBAT_NOT_GUILD_LEADER);
        notice::text(ctx, who, &line);
        return;
    };
    if !svc.social.siege.is_bidder(guild) {
        let line = text_of(ctx, TID_GAME_GUILDCOMBAT_NOTAPP);
        notice::text(ctx, who, &line);
        return;
    }
    if svc.social.siege.state != SiegeState::Close {
        let line = text_of(ctx, TID_GAME_GUILDCOMBAT_NOT_USE);
        notice::text(ctx, who, &line);
        return;
    }

    // ---- the commit block ----
    let now = crate::social::party::now_secs(ctx);
    let fee = svc.social.siege.fee_of(guild);
    let percent = svc.social.siege.config.request_cancel_percent;
    // MulDiv takes an int: the fee is at most INT_MAX; -1 (overflow) or a negative percentage refunds nothing
    let refund = i64::from(muldiv(i32::try_from(fee).unwrap_or(i32::MAX), percent, 100).max(0));
    let title = text_of(ctx, TID_GAME_GC_CANCELREQUEST);
    let body = text_of(ctx, TID_GAME_GC_CANCELREQUEST1);
    let (mail_id, _ops) = svc
        .econ
        .mail
        .send_system_with_id(master, &title, &body, None, refund);
    let Some(mail) = svc.econ.mail.get(mail_id).map(MailEntry::to_record) else {
        tracing::error!("a siege refund mail vanished right after it was written");
        return;
    };
    let Some(record) = svc.social.siege.cancel_bid(guild, refund, now) else {
        return; // unreachable: `is_bidder` held above
    };
    let siege_id = record.siege_id;
    let ticket = ctx.tickets.issue(crate::tickets::Pending::Service(
        crate::tickets::ServiceTicket::new(move |svc, _ctx, result| {
            super::log_batch_result("SiegeCancel", &result);
            on_send_result(svc, mail_id, result);
        }),
    ));
    ctx.persist
        .atomic(AtomicBatch::siege_cancel(record, mail).with_ticket(ticket));
    let returned = text_of(ctx, TID_GAME_GUILDCOMBAT_EXPENSE_RETURN);
    notice::text(ctx, who, &returned);
    let cancelled = text_of(ctx, TID_GAME_GUILDCOMBAT_CANCEL);
    guild_line(svc, ctx, guild, &cancelled);
    send_is_request(svc, ctx, guild, false);
    let mut ev = audit::siege_cancel(siege_id, guild, master, i64::from(fee), refund);
    if let Some(actor) = crate::audit::actor_of(ctx, who) {
        ev = ev.by(actor);
    }
    ctx.audit.emit(ev);
}

// ---- RequestStatus ---------------------------------------------------------------------------------------------------------

/// `RequestStatus` (`OnGCRequestStatus`): anyone gets `GC_REQUEST_STATUS{seconds_to_next, prize, guild_names, my_bid}`
/// with ALL ranked bidders in rank order (`"NotGuild"` for a vanished guild) and the own guild's bid. FIX:
/// `seconds_to_next` is read from the next firing of the wall job `siege_open` (0 before the scheduler evaluated it
/// for the first time; the original looked only at the last listed weekday).
pub fn on_request_status(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    _m: pb::RequestStatus,
) {
    if !enabled(ctx) {
        return;
    }
    let s = &svc.social.siege;
    let guild_names = s
        .ranking()
        .iter()
        .map(|r| {
            svc.social
                .guild
                .get(r.guild)
                .map_or_else(|| "NotGuild".to_owned(), |g| g.name.clone())
        })
        .collect();
    let my_bid = crate::social::guild::character_of(ctx, who)
        .and_then(|c| svc.social.guild.guild_of(c))
        .map_or(0, |g| s.fee_of(g));
    let seconds_to_next = ctx
        .sched
        .next_fire(SIEGE_OPEN)
        .map_or(0, |next| (next.0 - ctx.clock.unix_ms.0).div_euclid(1000))
        .clamp(0, i64::from(i32::MAX)) as i32;
    super::send_one(
        ctx,
        who.0,
        pb::guild_combat::Sub::GcRequestStatus(pb::guild_combat::GcRequestStatus {
            seconds_to_next,
            prize: s.prize_pool(),
            guild_names,
            my_bid,
        }),
    );
}
