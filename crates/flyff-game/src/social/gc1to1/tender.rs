//! The tenders (Plan/21 §8.3, §8.4; WP-SOCIAL-25; `GuildCombat1to1.cpp:1031-1124`, `1177-1482`): the tender window and
//! list, the tender itself, the cancel, the refund of a failed tender and the opening.
//!
//! Every handler returns at once while `events.guild_combat_1to1` is off, acts for the session's player (VR-001) and is
//! for the master of a guild. A tender, a cancel and a refund are ONE `AtomicBatch` each (the master's snapshot and the
//! tender row, VR-018): validated first, then committed in one block with the memory change, the messages and the audit
//! line. The original deducted the penya in the world and asked the database afterwards, so a crash in between lost it.
//!
//! The refunds are single precision in the order of the C++ expression ([`flyff_rules::social::gc1to1::refund`]): 80 %
//! of a cancelled tender into the master's purse, 98 % of a failed one on request (not by mail). No NPC proximity is
//! required (Plan/21 OQ13, KEEP, as the siege).

use super::{
    AlreadyOpen, Gc1to1State, Tender, big_caption_all, character_of, defined, enabled,
    live_members, send_manager_state, send_one, submit_logged, text_of, text_with,
};
use crate::chat::ingress::notice;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::econ::mail::commit::{can_snapshot, snapshot_for_batch, submit};
use crate::item::gold::{GOLD_MAX, add_gold, core::gold};
use crate::social::siege::fight::live_avatar;
use flyff_common::defs::text::{
    TID_GAME_GUILDCOMBAT1TO1_CANCELTENDER, TID_GAME_GUILDCOMBAT1TO1_FAILRETURN,
    TID_GAME_GUILDCOMBAT1TO1_FAILTENDER, TID_GAME_GUILDCOMBAT1TO1_ISGCTENDER,
    TID_GAME_GUILDCOMBAT1TO1_LIMIT_MIN, TID_GAME_GUILDCOMBAT1TO1_LINEUP,
    TID_GAME_GUILDCOMBAT1TO1_LOWGUILDLEVEL, TID_GAME_GUILDCOMBAT1TO1_MORE_CURRENT_REQUEST,
    TID_GAME_GUILDCOMBAT1TO1_MORETENDER, TID_GAME_GUILDCOMBAT1TO1_NOGAME,
    TID_GAME_GUILDCOMBAT1TO1_NOTENDER, TID_GAME_GUILDCOMBAT1TO1_NOTMASTER,
    TID_GAME_GUILDCOMBAT1TO1_NOTRETURNGUILD, TID_GAME_GUILDCOMBAT1TO1_OPEN,
    TID_GAME_GUILDCOMBAT1TO1_TENDERPENYA, TID_GAME_LACKMONEY, TID_GAME_LACKSPACE,
};
use flyff_common::{CharacterId, GuildId, UnixTime};
use flyff_db::{AtomicBatch, AtomicKind, DbOp, Gc1to1TenderRecord, TenderState};
use flyff_proto::pb;
use flyff_rules::social::gc1to1::refund;
use serde_json::json;

// ---- the state table -----------------------------------------------------------------------------------------------------

/// `true` when the stored row may move from `from` (`None` = no row yet) to `to` (21 §8.3 plus `C -> T`): `T` from
/// new, `T` (a raise) and `C`; `F`, `C` and `E` only from `T`; `G` and `N` only from `F`.
pub fn transition_allowed(from: Option<TenderState>, to: TenderState) -> bool {
    use TenderState::{Cancelled, Ended, Failed, Given, NotClaimed, Tendered};
    matches!(
        (from, to),
        (None | Some(Tendered | Cancelled), Tendered)
            | (Some(Tendered), Failed | Cancelled | Ended)
            | (Some(Failed), Given | NotClaimed)
    )
}

/// The row of a state write: every move of a tender goes through here, which `debug_assert!`s the "From" column of 21
/// §8.3 (the legacy procedure silently ignored a wrong source state; the guard of the table refuses it too).
pub fn set_state(
    from: Option<TenderState>,
    to: TenderState,
    round: u32,
    guild: GuildId,
    penya: i32,
    tendered_at: UnixTime,
) -> Gc1to1TenderRecord {
    debug_assert!(
        transition_allowed(from, to),
        "a GC 1:1 tender cannot move from {from:?} to {to:?}"
    );
    Gc1to1TenderRecord {
        round,
        guild_id: guild,
        penya: i64::from(penya),
        state: to,
        tendered_at,
    }
}

// ---- the actor ---------------------------------------------------------------------------------------------------------------

/// The character and guild of `who` when it is the master of a guild.
fn master_of(svc: &Services, ctx: &GameCtx<'_>, who: PlayerRef) -> Option<(CharacterId, GuildId)> {
    let c = character_of(ctx, who)?;
    let guild = svc.social.guild.guild_of(c)?;
    svc.social.guild.is_master(c).then_some((c, guild))
}

/// `AddDiagText( prj.GetText( tid ) )`.
fn diag_text(ctx: &mut GameCtx<'_>, who: PlayerRef, tid: u32) {
    let line = text_of(ctx, tid);
    notice::diag(ctx, who, &line);
}

/// `true` when the guild of `c` bid for the siege (`IsGCTenderGuild`).
fn is_siege_bidder(svc: &Services, c: CharacterId) -> bool {
    svc.social
        .guild
        .guild_of(c)
        .is_some_and(|g| crate::social::siege::bids::is_bidder(svc, g))
}

/// The level of the guild.
fn guild_level(svc: &Services, guild: GuildId) -> i32 {
    svc.social.guild.get(guild).map_or(0, |g| g.level)
}

// ---- the windows ---------------------------------------------------------------------------------------------------------------

/// `Gc1to1TenderOpenWndReq` (`SendTenderGuildOpenWnd`, `1177-1214`), in this order: state not Close
/// (`NOTENDER`, box); not the master (`NOTMASTER`); an active siege bidder (`ISGCTENDER`); guild level below
/// `MinGuildLevel` (`LOWGUILDLEVEL` with the level); else `Gc1to1TenderOpenWndNtf{my_penya (0 = none), join_penya}`.
pub fn on_open_wnd_req(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    if !enabled(ctx) {
        return;
    }
    if svc.social.gc1to1.state() != Gc1to1State::Close {
        diag_text(ctx, who, TID_GAME_GUILDCOMBAT1TO1_NOTENDER);
        return;
    }
    let Some((c, guild)) = master_of(svc, ctx, who) else {
        defined(ctx, who.0, TID_GAME_GUILDCOMBAT1TO1_NOTMASTER, Vec::new());
        return;
    };
    if is_siege_bidder(svc, c) {
        defined(ctx, who.0, TID_GAME_GUILDCOMBAT1TO1_ISGCTENDER, Vec::new());
        return;
    }
    let min_level = svc.social.gc1to1.config.min_guild_level;
    if guild_level(svc, guild) < min_level {
        defined(
            ctx,
            who.0,
            TID_GAME_GUILDCOMBAT1TO1_LOWGUILDLEVEL,
            vec![min_level.to_string()],
        );
        return;
    }
    let msg = pb::Gc1to1TenderOpenWndNtf {
        my_penya: svc.social.gc1to1.tender_of(guild).map_or(0, |t| t.penya),
        join_penya: svc.social.gc1to1.config.min_join_penya,
    };
    send_one(ctx, who.0, msg);
}

/// `Gc1to1TenderViewReq` (`SendTenderGuildView`, `1217-1248`, any player): state not Close -> `NOTENDER`; else
/// `Gc1to1TenderViewNtf{penya, ranking (index + 1; 0 for a non-tenderer), remain_s, guilds (rank order)}`. FIX:
/// `remain_s` is read from the next firing of the wall job `gc1to1_open` (0 before the scheduler evaluated it); the Lua
/// `GetRemainNextTime` of the original has quirks.
pub fn on_view_req(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    if !enabled(ctx) {
        return;
    }
    if svc.social.gc1to1.state() != Gc1to1State::Close {
        diag_text(ctx, who, TID_GAME_GUILDCOMBAT1TO1_NOTENDER);
        return;
    }
    let g = &svc.social.gc1to1;
    let mine = character_of(ctx, who)
        .and_then(|c| svc.social.guild.guild_of(c))
        .and_then(|guild| g.tenders().iter().position(|t| t.guild == guild));
    let remain_s = ctx
        .sched
        .next_fire(super::GC1TO1_OPEN)
        .map_or(0, |next| (next.0 - ctx.clock.unix_ms.0).div_euclid(1000))
        .clamp(0, i64::from(i32::MAX)) as i32;
    let msg = pb::Gc1to1TenderViewNtf {
        penya: mine.and_then(|i| g.tenders().get(i)).map_or(0, |t| t.penya),
        ranking: mine.map_or(0, |i| i32::try_from(i + 1).unwrap_or(i32::MAX)),
        remain_s,
        guilds: g.tenders().iter().map(|t| t.guild.0).collect(),
    };
    send_one(ctx, who.0, msg);
}

// ---- the tender ----------------------------------------------------------------------------------------------------------------

/// `Gc1to1Tender{penya}` (`SetTenderGuild`, `1251-1367`). Refusals, in this order, change nothing: state not Close
/// (`NOTENDER`, box); an active siege bidder (`ISGCTENDER`, BEFORE the master check, as the original); not the master
/// (`NOTMASTER`); guild level below `MinGuildLevel` (`LOWGUILDLEVEL`); `penya` below `MinJoinPenya`, negative values too
/// (`LIMIT_MIN`); an existing tender and `penya` not above it (`MORE_CURRENT_REQUEST`); the purse short of the
/// difference (`TID_GAME_LACKMONEY`); a player that cannot be snapshotted (silent, debug log).
///
/// The commit (VR-018): the difference leaves the master (a new tender pays all), the entry is appended or raised in
/// place with `tendered_at = now`, the list is re-ranked, the master is snapshotted AFTER the debit and ONE
/// `AtomicBatch::gc1to1_tender` goes out; then, in the same block, the confirmation box (`MORETENDER [difference,
/// penya]` or `TENDERPENYA [penya]`) and the audit line `gc1to1.tender`.
pub fn on_tender(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, penya: i32) {
    if !enabled(ctx) {
        return;
    }
    if svc.social.gc1to1.state() != Gc1to1State::Close {
        diag_text(ctx, who, TID_GAME_GUILDCOMBAT1TO1_NOTENDER);
        return;
    }
    let Some(c) = character_of(ctx, who) else {
        return;
    };
    if is_siege_bidder(svc, c) {
        defined(ctx, who.0, TID_GAME_GUILDCOMBAT1TO1_ISGCTENDER, Vec::new());
        return;
    }
    let Some((master, guild)) = master_of(svc, ctx, who) else {
        defined(ctx, who.0, TID_GAME_GUILDCOMBAT1TO1_NOTMASTER, Vec::new());
        return;
    };
    let (min_level, min_penya) = {
        let c = &svc.social.gc1to1.config;
        (c.min_guild_level, c.min_join_penya)
    };
    if guild_level(svc, guild) < min_level {
        defined(
            ctx,
            who.0,
            TID_GAME_GUILDCOMBAT1TO1_LOWGUILDLEVEL,
            vec![min_level.to_string()],
        );
        return;
    }
    if penya < min_penya {
        defined(ctx, who.0, TID_GAME_GUILDCOMBAT1TO1_LIMIT_MIN, Vec::new());
        return;
    }
    let old = svc.social.gc1to1.tender_of(guild).map(|t| t.penya);
    if old.is_some_and(|o| penya <= o) {
        defined(
            ctx,
            who.0,
            TID_GAME_GUILDCOMBAT1TO1_MORE_CURRENT_REQUEST,
            Vec::new(),
        );
        return;
    }
    let pay = i64::from(penya) - i64::from(old.unwrap_or(0));
    if ctx.player(who).map_or(0, gold) < pay {
        defined(ctx, who.0, TID_GAME_LACKMONEY, Vec::new());
        return;
    }
    if !can_snapshot(ctx, who) {
        tracing::debug!("a GC 1:1 tender of a player without a savable session was refused");
        return;
    }

    // ---- the commit block: nothing below can fail ----
    let now = crate::social::party::now_secs(ctx);
    add_gold(svc, ctx, who, -pay, true);
    // the memory change, with what undoes it
    let (round, from, undo) = {
        let g = &mut svc.social.gc1to1;
        match g.tenders.iter_mut().find(|t| t.guild == guild) {
            Some(t) => {
                let undo = Undo::Raise {
                    penya: t.penya,
                    tendered_at: t.tendered_at,
                };
                t.penya = penya;
                t.tendered_at = now;
                let round = t.round;
                g.resort();
                (round, Some(TenderState::Tendered), undo)
            }
            None => {
                let round = g.round_for_new_tender(guild);
                g.insert_ranked(Tender {
                    guild,
                    penya,
                    round,
                    tendered_at: now,
                    lineup: Vec::new(),
                    stage: None,
                });
                (round, None, Undo::New)
            }
        }
    };
    let Some((snap, generation)) = snapshot_for_batch(svc, ctx, who) else {
        // unreachable after `can_snapshot`; undo rather than lose the penya
        undo.apply(&mut svc.social.gc1to1, guild);
        add_gold(svc, ctx, who, pay, true);
        tracing::error!("a GC 1:1 tender lost its snapshot after the check: undone");
        return;
    };
    let record = set_state(from, TenderState::Tendered, round, guild, penya, now);
    submit(
        svc,
        ctx,
        who,
        Some(generation),
        AtomicBatch::gc1to1_tender(snap, record),
        |_svc, _ctx, result| super::log_batch_result("Gc1to1Tender", &result),
    );
    let line = match old {
        Some(o) => text_with(
            ctx,
            TID_GAME_GUILDCOMBAT1TO1_MORETENDER,
            &[(penya - o).to_string(), penya.to_string()],
        ),
        None => text_with(
            ctx,
            TID_GAME_GUILDCOMBAT1TO1_TENDERPENYA,
            &[penya.to_string()],
        ),
    };
    notice::diag(ctx, who, &line);
    crate::audit::emit_for(
        ctx,
        who,
        "gc1to1.tender",
        json!({
            "guild": guild.0,
            "master": master.0,
            "penya": penya,
            "paid": pay,
            "round": round,
            "kind": if old.is_some() { "raise" } else { "new" },
        }),
    );
}

/// What undoes the memory change of [`on_tender`].
enum Undo {
    /// The guild had no tender.
    New,
    /// The guild raised its tender.
    Raise { penya: i32, tendered_at: UnixTime },
}

impl Undo {
    fn apply(self, g: &mut super::Gc1to1Service, guild: GuildId) {
        match self {
            Undo::New => g.tenders.retain(|t| t.guild != guild),
            Undo::Raise { penya, tendered_at } => {
                if let Some(t) = g.tenders.iter_mut().find(|t| t.guild == guild) {
                    t.penya = penya;
                    t.tendered_at = tendered_at;
                }
                g.resort();
            }
        }
    }
}

// ---- the cancel ------------------------------------------------------------------------------------------------------------------

/// `Gc1to1TenderCancel` (`SetCancelTenderGuild`, `1370-1426`): state not Close (`NOTENDER`, box); not the master
/// (`NOTMASTER`); no tender (silent). The refund is `(penya as f32 * (CancelReturnRate as f32 / 100.0)) as i32`
/// (110 000 001 -> 88 000 000) INTO the purse; a purse that would overflow (`TID_GAME_LACKSPACE`) or a player that cannot
/// be snapshotted (silent) leaves everything as it is. ONE `AtomicBatch::gc1to1_cancel` (the master's snapshot and the
/// row in state `C`; a line-up kept from an interrupted event is cleared in the same batch); `CANCELTENDER [refund]`;
/// audit `gc1to1.tender{kind: "cancel"}`. A cancelled guild may tender again in the same round (`C -> T`).
pub fn on_cancel(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    if !enabled(ctx) {
        return;
    }
    if svc.social.gc1to1.state() != Gc1to1State::Close {
        diag_text(ctx, who, TID_GAME_GUILDCOMBAT1TO1_NOTENDER);
        return;
    }
    let Some((master, guild)) = master_of(svc, ctx, who) else {
        defined(ctx, who.0, TID_GAME_GUILDCOMBAT1TO1_NOTMASTER, Vec::new());
        return;
    };
    let Some(t) = svc.social.gc1to1.tender_of(guild).cloned() else {
        return;
    };
    let back = refund(t.penya, svc.social.gc1to1.config.cancel_return_rate);
    let purse = ctx.player(who).map_or(0, gold);
    if back > 0 && purse + i64::from(back) > GOLD_MAX {
        defined(ctx, who.0, TID_GAME_LACKSPACE, Vec::new());
        return;
    }
    if !can_snapshot(ctx, who) {
        tracing::debug!("a GC 1:1 cancel of a player without a savable session was refused");
        return;
    }

    // ---- the commit block ----
    add_gold(svc, ctx, who, i64::from(back), true);
    svc.social.gc1to1.tenders.retain(|x| x.guild != guild);
    let Some((snap, generation)) = snapshot_for_batch(svc, ctx, who) else {
        svc.social.gc1to1.insert_ranked(t);
        add_gold(svc, ctx, who, -i64::from(back), true);
        tracing::error!("a GC 1:1 cancel lost its snapshot after the check: undone");
        return;
    };
    let record = set_state(
        Some(TenderState::Tendered),
        TenderState::Cancelled,
        t.round,
        guild,
        t.penya,
        t.tendered_at,
    );
    let mut batch = AtomicBatch::gc1to1_cancel(snap, record);
    if !t.lineup.is_empty() {
        // the stored line-up of the cancelled row must not come back with a later tender of the same round
        batch.ops.push(DbOp::ReplaceGc1to1Lineup {
            round: t.round,
            guild_id: guild,
            members: Vec::new(),
        });
    }
    submit(
        svc,
        ctx,
        who,
        Some(generation),
        batch,
        |_svc, _ctx, result| super::log_batch_result("Gc1to1Cancel", &result),
    );
    let line = text_with(
        ctx,
        TID_GAME_GUILDCOMBAT1TO1_CANCELTENDER,
        &[back.to_string()],
    );
    notice::diag(ctx, who, &line);
    crate::audit::emit_for(
        ctx,
        who,
        "gc1to1.tender",
        json!({
            "guild": guild.0,
            "master": master.0,
            "penya": t.penya,
            "refund": back,
            "round": t.round,
            "kind": "cancel",
        }),
    );
}

// ---- the refund of a failed tender ---------------------------------------------------------------------------------------------

/// `Gc1to1TenderFailed` (`SetFailedTenderGuild`, `1429-1482`, any state): not the master (`NOTMASTER`); the guild has no
/// failed tender (`NOTRETURNGUILD`); the refund (`FailReturnRate` 98 %, 110 000 001 -> 107 800 000) would overflow the
/// purse (`TID_GAME_LACKSPACE`); a player that cannot be snapshotted (silent). ONE `AtomicBatch::gc1to1_refund` (the
/// master's snapshot and the row in state `G`); the entry leaves the failed list; `FAILRETURN [refund]`; audit
/// `gc1to1.refund`. A second claim finds nothing.
pub fn on_failed(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    if !enabled(ctx) {
        return;
    }
    let Some((master, guild)) = master_of(svc, ctx, who) else {
        defined(ctx, who.0, TID_GAME_GUILDCOMBAT1TO1_NOTMASTER, Vec::new());
        return;
    };
    let Some(f) = svc.social.gc1to1.failed_of(guild).cloned() else {
        defined(
            ctx,
            who.0,
            TID_GAME_GUILDCOMBAT1TO1_NOTRETURNGUILD,
            Vec::new(),
        );
        return;
    };
    let back = refund(f.penya, svc.social.gc1to1.config.fail_return_rate);
    let purse = ctx.player(who).map_or(0, gold);
    if back > 0 && purse + i64::from(back) > GOLD_MAX {
        defined(ctx, who.0, TID_GAME_LACKSPACE, Vec::new());
        return;
    }
    if !can_snapshot(ctx, who) {
        tracing::debug!("a GC 1:1 refund of a player without a savable session was refused");
        return;
    }

    // ---- the commit block ----
    add_gold(svc, ctx, who, i64::from(back), true);
    svc.social.gc1to1.failed.retain(|x| x.guild != guild);
    let Some((snap, generation)) = snapshot_for_batch(svc, ctx, who) else {
        svc.social.gc1to1.failed.push(f);
        add_gold(svc, ctx, who, -i64::from(back), true);
        tracing::error!("a GC 1:1 refund lost its snapshot after the check: undone");
        return;
    };
    if f.round >= svc.social.gc1to1.round {
        svc.social.gc1to1.spent.insert(guild);
    }
    let record = set_state(
        Some(TenderState::Failed),
        TenderState::Given,
        f.round,
        guild,
        f.penya,
        f.tendered_at,
    );
    submit(
        svc,
        ctx,
        who,
        Some(generation),
        AtomicBatch::gc1to1_refund(snap, record),
        |_svc, _ctx, result| super::log_batch_result("Gc1to1Refund", &result),
    );
    let line = text_with(
        ctx,
        TID_GAME_GUILDCOMBAT1TO1_FAILRETURN,
        &[back.to_string()],
    );
    notice::diag(ctx, who, &line);
    crate::audit::emit_for(
        ctx,
        who,
        "gc1to1.refund",
        json!({
            "guild": guild.0,
            "master": master.0,
            "penya": f.penya,
            "refund": back,
            "round": f.round,
        }),
    );
}

// ---- the opening -------------------------------------------------------------------------------------------------------------------

/// `GuildCombat1to1Open` (`1031-1124`; also `/gc1to1open [1]` through [`super::gm::open`]; the weekly wall job calls it
/// through [`super::config::on_open_time`]). A state other than Close is `Err(AlreadyOpen)` (FIX: the GM command tested
/// `!= OPEN` and re-opened a running Entrance or War). The flag is the callers' test.
///
/// 1. Unless `server_down` (the GM argument 1): every failed tender not claimed becomes `N` and the list is cleared.
/// 2. Fewer than `MinJoinGuild` tenders: the big caption `NOGAME [MinJoinGuild]` to everybody, the state stays Close,
///    the tenders stay `T` for the next opening (the `N` writes of (1) still commit).
/// 3. Else the big captions `OPEN` and `LINEUP`, state Open for `MemberLineUpTime`; the tenders beyond rank
///    `MaxJoinGuild` become `F`, then an odd remainder makes the LAST one `F`; each gets `FAILTENDER` at its online master
///    and moves to the failed list; `Gc1to1NowState{1, remaining}` goes to every online member of every remaining
///    guild. ONE `AtomicBatch::of_ops(Other("Gc1to1Open"), ..)` carries all `N` and `F` writes (none when empty); audit
///    `gc1to1.open`.
pub fn open(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    server_down: bool,
) -> Result<(), AlreadyOpen> {
    if svc.social.gc1to1.state() != Gc1to1State::Close {
        return Err(AlreadyOpen);
    }
    let now = ctx.clock.game_ms;
    let cfg = svc.social.gc1to1.config.clone();
    let mut ops: Vec<DbOp> = Vec::new();

    // (1) the forfeited refunds
    if !server_down {
        let g = &mut svc.social.gc1to1;
        for f in std::mem::take(&mut g.failed) {
            if f.round >= g.round {
                g.spent.insert(f.guild);
            }
            ops.push(DbOp::UpsertGc1to1Tender(set_state(
                Some(TenderState::Failed),
                TenderState::NotClaimed,
                f.round,
                f.guild,
                f.penya,
                f.tendered_at,
            )));
        }
    }

    // (2) too few tenders
    let tenders_now = svc.social.gc1to1.tenders.len();
    if i32::try_from(tenders_now).unwrap_or(i32::MAX) < cfg.min_join_guild {
        let line = text_with(
            ctx,
            TID_GAME_GUILDCOMBAT1TO1_NOGAME,
            &[cfg.min_join_guild.to_string()],
        );
        big_caption_all(svc, ctx, &line);
        svc.social.gc1to1.due = flyff_common::GameMs(0);
        if !ops.is_empty() {
            submit_logged(
                ctx,
                "Gc1to1Open",
                AtomicBatch::of_ops(AtomicKind::Other("Gc1to1Open"), ops),
            );
        }
        return Ok(());
    }

    // (3) the opening
    for tid in [
        TID_GAME_GUILDCOMBAT1TO1_OPEN,
        TID_GAME_GUILDCOMBAT1TO1_LINEUP,
    ] {
        let line = text_of(ctx, tid);
        big_caption_all(svc, ctx, &line);
    }
    {
        let g = &mut svc.social.gc1to1;
        g.set_state(Gc1to1State::Open);
        g.due = flyff_common::GameMs(now.0.saturating_add(cfg.member_line_up_time_ms));
        for t in &mut g.tenders {
            t.stage = None;
        }
    }
    // the tenders beyond the rank limit, then an odd remainder: the last one
    let max = usize::try_from(cfg.max_join_guild).unwrap_or(0);
    let mut out: Vec<Tender> = Vec::new();
    {
        let g = &mut svc.social.gc1to1;
        if g.tenders.len() > max {
            out.extend(g.tenders.drain(max..));
        }
        if !g.tenders.len().is_multiple_of(2) {
            out.extend(g.tenders.pop());
        }
    }
    for t in out {
        let master = svc.social.guild.get(t.guild).and_then(|g| g.master());
        if let Some(obj) = master.and_then(|m| live_avatar(&svc.presence, &*ctx.worlds, m)) {
            defined(ctx, obj, TID_GAME_GUILDCOMBAT1TO1_FAILTENDER, Vec::new());
        }
        ops.push(DbOp::UpsertGc1to1Tender(set_state(
            Some(TenderState::Tendered),
            TenderState::Failed,
            t.round,
            t.guild,
            t.penya,
            t.tendered_at,
        )));
        svc.social.gc1to1.failed.push(super::FailedTender {
            guild: t.guild,
            penya: t.penya,
            round: t.round,
            tendered_at: t.tendered_at,
        });
    }
    // every online member of every remaining guild hears that the line-up time began
    let guilds: Vec<GuildId> = svc.social.gc1to1.tenders.iter().map(|t| t.guild).collect();
    for guild in &guilds {
        let members: Vec<CharacterId> = svc
            .social
            .guild
            .get(*guild)
            .map(|g| g.members.keys().copied().collect())
            .unwrap_or_default();
        for c in members {
            if let Some(obj) = live_avatar(&svc.presence, &*ctx.worlds, c) {
                send_manager_state(svc, ctx, obj, c);
            }
        }
    }
    let failed_now = svc.social.gc1to1.failed.len();
    if !ops.is_empty() {
        submit_logged(
            ctx,
            "Gc1to1Open",
            AtomicBatch::of_ops(AtomicKind::Other("Gc1to1Open"), ops),
        );
    }
    ctx.audit.emit(flyff_db::AuditEvent::new(
        "gc1to1.open",
        json!({
            "round": svc.social.gc1to1.round(),
            "tenders": guilds.len(),
            "failed": failed_now,
            "server_down": server_down,
        }),
    ));
    Ok(())
}

/// The online avatars of the members of the tendering guilds (every state-change message of the manager goes to them).
pub(crate) fn tender_member_avatars(
    svc: &Services,
    ctx: &GameCtx<'_>,
) -> Vec<(CharacterId, flyff_common::ObjId)> {
    let mut out = Vec::new();
    for t in svc.social.gc1to1.tenders() {
        for obj in live_members(svc, ctx, t.guild) {
            if let Some(c) = ctx
                .worlds
                .player(obj)
                .and_then(|m| m.player())
                .map(|p| p.ident.character_id)
            {
                out.push((c, obj));
            }
        }
    }
    out
}
