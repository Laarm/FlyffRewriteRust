//! The tender phase (Plan/22 §11.2; `SecretRoom.cpp:412-617` the master actions, `773-915` tender, bubble sort and
//! cancel; `WORLDSERVER/DPSrvr.cpp:11649-11782` the handlers): the four requests a player makes at the side NPCs
//! while the manager is `Close`.
//!
//! * `SecretRoomTenderOpenWndReq` (menu 84) -> `SecretRoomTenderOpenWndNtf{min_penya, my_tender}`,
//! * `SecretRoomTender{penya}` (menu 84): the bid, ONE `AtomicBatch::secret_room_tender` with the master's snapshot,
//! * `SecretRoomTenderCancelReturn` (menu 103): the 80 % refund INTO the wallet, ONE `secret_room_cancel` batch,
//! * `SecretRoomTenderViewReq` (menu 85) -> `SecretRoomTenderViewNtf{my_penya, my_rank, remain_s, guilds}`.
//!
//! Every handler first looks for an NPC with its menu within `gameplay.npc_menu_range` of the player in the same
//! world and layer (VR-012; added: the legacy checked only the hunting-ground menu), then follows the legacy order of
//! checks. The side is the player's AREA (`Continents::area_of(continent_of(world, pos))`): Madrigal polygons, the
//! guild-war world counts as East, the arena as West, everything else is `NODATA` and silent.
//!
//! **The commit** (VR-018, the siege `bids::request` pattern): `can_snapshot` first, then in one block the penya
//! leaves the wallet, the tender is inserted or updated and re-ranked, the master is snapshotted AFTER the debit and
//! ONE batch goes out; the confirmation and the audit line follow. A cancel refunds
//! `(i32)(p as f32 - p as f32 * (CancelReturnRate as f32 / 100.0f32))` with every step rounded to `f32` (KEEP-QUIRK:
//! 123 456 789 -> 98 765 432, `i32::MAX` -> 1 717 986 944).

use super::{SecretRoomService, SrMngState, SrTender, cfg_of, tax_continent};
use crate::chat::ingress::notice;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::econ::mail::commit::{can_snapshot, snapshot_for_batch, submit};
use crate::gm::text::format_positional;
use crate::item::gold::{add_gold, can_add, core::gold};
use crate::tickets::TicketResult;
use crate::world::continent::{CONT_EAST, CONT_NODATA, CONT_WEST, Continents};
use flyff_common::defs::neuz::{MMI_SECRET_CANCEL, MMI_SECRET_OFFER, MMI_SECRET_OFFERSTATE};
use flyff_common::defs::text::{
    TID_GAME_CONT_EAST, TID_GAME_CONT_WEST, TID_GAME_LACKSPACE, TID_GAME_SECRETROOM_CANCELRETURN,
    TID_GAME_SECRETROOM_ISNOTGUILDMASTER, TID_GAME_SECRETROOM_ISOTHERCONT,
    TID_GAME_SECRETROOM_LOWGUILDLEVEL, TID_GAME_SECRETROOM_NOTENDER,
    TID_GAME_SECRETROOM_NOTENDERCANCEL, TID_GAME_SECRETROOM_NOTENDERGUILD,
    TID_GAME_SECRETROOM_NOTENDERVIEW, TID_GAME_SECRETROOM_NOTENOUGHPENYA,
    TID_GAME_SECRETROOM_TENDERCOMPLETE, TID_GAME_SECRETROOM_TRYMOREPENYA,
};
use flyff_common::{CharacterId, GuildId, UnixMs};
use flyff_db::audit::content as audit;
use flyff_db::{AtomicBatch, SecretRoomTenderState};
use flyff_proto::pb;

// ---- what the handlers share --------------------------------------------------------------------------------------------

/// VR-012: an NPC offering `menu` within `gameplay.npc_menu_range` of the player in his world and layer.
pub(super) fn npc_ok(ctx: &GameCtx<'_>, who: PlayerRef, menu: u32) -> bool {
    let Some(c) = ctx.worlds.core(who.0).map(|c| (c.world, c.layer, c.pos)) else {
        return false;
    };
    ctx.worlds.close_npc(menu, c.0, c.1, c.2).is_some()
}

/// `CContinent::GetArea(GetContinent(mover))`: `CONT_EAST`, `CONT_WEST` or `CONT_NODATA`.
pub(super) fn area_of(ctx: &GameCtx<'_>, who: PlayerRef) -> u8 {
    ctx.worlds.core(who.0).map_or(CONT_NODATA, |c| {
        Continents::area_of(ctx.worlds.continents.continent_of(c.world, c.pos))
    })
}

/// The character and guild of `who` when he is the master of a guild (`IsGuildMaster`: no guild counts as not master).
pub(super) fn master_of(
    s: &Services,
    ctx: &GameCtx<'_>,
    who: PlayerRef,
) -> Option<(CharacterId, GuildId)> {
    let c = crate::social::guild::character_of(ctx, who)?;
    let guild = s.social.guild.guild_of(c)?;
    s.social.guild.is_master(c).then_some((c, guild))
}

/// `AddDefinedText(tid)`.
pub(super) fn say(ctx: &mut GameCtx<'_>, who: PlayerRef, tid: u32) {
    notice::defined(ctx, who, tid, Vec::new());
}

/// The name of a side (`CContinent::GetContinentName`: `TID_GAME_CONT_EAST` / `_WEST`).
pub(super) fn side_name(ctx: &GameCtx<'_>, side: u8) -> String {
    let tid = if side == CONT_WEST {
        TID_GAME_CONT_WEST
    } else {
        TID_GAME_CONT_EAST
    };
    ctx.data.texts.get(tid).to_owned()
}

/// `AddDefinedText(tid, "\"%s\"", side name)`.
pub(super) fn say_side(ctx: &mut GameCtx<'_>, who: PlayerRef, tid: u32, side: u8) {
    let arg = format!("\"{}\"", side_name(ctx, side));
    notice::defined(ctx, who, tid, vec![arg]);
}

/// The text of `tid` with the printf conversions replaced by `args` in order (captions).
pub(super) fn text_with(ctx: &GameCtx<'_>, tid: u32, args: &[String]) -> String {
    format_positional(ctx.data.texts.get(tid), args)
}

/// The level of the guild.
pub(super) fn guild_level(s: &Services, guild: GuildId) -> i32 {
    s.social.guild.get(guild).map_or(0, |g| g.level)
}

/// `(int)(p - p * (rate / 100.0f))` with every step rounded to `f32`: the refund of a cancelled tender
/// (`CancelReturnRate`) and of a drop-out (`DropoutReturnRate`).
pub fn refund_after_fee(penya: i32, rate_percent: i32) -> i32 {
    let rate = rate_percent as f32 / 100.0_f32;
    let p = penya as f32;
    let fee = p * rate;
    let left = p - fee;
    if left.is_nan() { 0 } else { left as i32 }
}

/// Seconds from `now` to the next opening time (`GetNextOpenTime`; FIX PL-158: strictly after `now`, the legacy
/// answered the current second inside the opening minute). 0 without an occurrence.
pub fn seconds_to_next_open(s: &Services, ctx: &GameCtx<'_>) -> i32 {
    next_open_secs(s, ctx.sched.zone(), ctx.clock.unix_ms)
}

/// [`seconds_to_next_open`] over a zone and a time.
pub fn next_open_secs(s: &Services, zone: &crate::sched::wall::WallZone, now: UnixMs) -> i32 {
    let spec = s.event.configs.schedule.secret_room_open;
    zone.next_occurrence(&spec, now).map_or(0, |next| {
        let secs = next.0.div_euclid(1000) - now.0.div_euclid(1000);
        i32::try_from(secs.clamp(0, i64::from(i32::MAX))).unwrap_or(i32::MAX)
    })
}

// ---- SecretRoomTenderOpenWndReq ------------------------------------------------------------------------------------------

/// `SecretRoomTenderOpenWndReq` (menu 84, `SetTenderOpenWnd`): the manager must be `Close` (else `NOTENDER`), the
/// player the master of his guild (`ISNOTGUILDMASTER`) of at least `MinGuildLevel` (`LOWGUILDLEVEL`); then
/// `SecretRoomTenderOpenWndNtf{min_penya, my_tender}` (0 = no tender on this side).
pub fn on_tender_open_wnd(s: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    if !npc_ok(ctx, who, MMI_SECRET_OFFER) {
        return;
    }
    if s.event.secret_room.state != SrMngState::Close {
        say(ctx, who, TID_GAME_SECRETROOM_NOTENDER);
        return;
    }
    let Some((_, guild)) = master_of(s, ctx, who) else {
        say(ctx, who, TID_GAME_SECRETROOM_ISNOTGUILDMASTER);
        return;
    };
    let cfg = cfg_of(s);
    if guild_level(s, guild) < cfg.min_guild_level {
        say(ctx, who, TID_GAME_SECRETROOM_LOWGUILDLEVEL);
        return;
    }
    let min_penya = cfg.min_penya;
    let side = area_of(ctx, who);
    if side == CONT_NODATA {
        return;
    }
    let my_tender = s
        .event
        .secret_room
        .side(side)
        .and_then(|sd| sd.tender(guild))
        .map_or(0, |t| t.penya);
    crate::chat::send_to(
        ctx,
        who,
        pb::SecretRoomTenderOpenWndNtf {
            min_penya,
            my_tender,
        },
    );
}

// ---- SecretRoomTender ----------------------------------------------------------------------------------------------------

/// `SecretRoomTender{penya}` (menu 84, `SetTender` + `SetContTender`). `penya <= 0` is ignored before anything. The
/// refusals, in this order, change nothing: the manager is not `Close` (`NOTENDER`), not the master
/// (`ISNOTGUILDMASTER`), guild level below `MinGuildLevel` (`LOWGUILDLEVEL`), no side at the player's position
/// (silent), a tender of the guild on the OTHER side (`ISOTHERCONT`), `penya` below `MinPenya` (`TRYMOREPENYA`), a
/// first bid the wallet cannot pay (`NOTENOUGHPENYA`), a raise not above the old tender (`TRYMOREPENYA`) or whose
/// difference the wallet cannot pay (`NOTENOUGHPENYA`). Then the commit block (see the module documentation) and
/// `TENDERCOMPLETE`.
pub fn on_tender(s: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, penya: i32) {
    if penya <= 0 {
        return;
    }
    if !npc_ok(ctx, who, MMI_SECRET_OFFER) {
        return;
    }
    if s.event.secret_room.state != SrMngState::Close {
        say(ctx, who, TID_GAME_SECRETROOM_NOTENDER);
        return;
    }
    let Some((master, guild)) = master_of(s, ctx, who) else {
        say(ctx, who, TID_GAME_SECRETROOM_ISNOTGUILDMASTER);
        return;
    };
    let cfg = cfg_of(s);
    let (min_level, min_penya) = (cfg.min_guild_level, cfg.min_penya);
    if guild_level(s, guild) < min_level {
        say(ctx, who, TID_GAME_SECRETROOM_LOWGUILDLEVEL);
        return;
    }
    let side = area_of(ctx, who);
    if side == CONT_NODATA {
        return;
    }
    let sr = &s.event.secret_room;
    let other = if side == CONT_EAST {
        CONT_WEST
    } else {
        CONT_EAST
    };
    if sr
        .side(other)
        .is_some_and(|sd| sd.index_of(guild).is_some())
    {
        say(ctx, who, TID_GAME_SECRETROOM_ISOTHERCONT);
        return;
    }
    if penya < min_penya {
        say(ctx, who, TID_GAME_SECRETROOM_TRYMOREPENYA);
        return;
    }
    let previous = sr
        .side(side)
        .and_then(|sd| sd.tender(guild))
        .map(|t| t.penya);
    let wallet = ctx.player(who).map_or(0, gold);
    let paid = match previous {
        None => {
            if wallet < i64::from(penya) {
                say(ctx, who, TID_GAME_SECRETROOM_NOTENOUGHPENYA);
                return;
            }
            penya
        }
        Some(old) => {
            if penya <= old {
                say(ctx, who, TID_GAME_SECRETROOM_TRYMOREPENYA);
                return;
            }
            if wallet < i64::from(penya - old) {
                say(ctx, who, TID_GAME_SECRETROOM_NOTENOUGHPENYA);
                return;
            }
            penya - old
        }
    };
    if !can_snapshot(ctx, who) {
        tracing::debug!("a secret-room tender of a player without a savable session was refused");
        return;
    }

    // ---- the commit block: nothing below can fail ----
    let now = crate::social::party::now_secs(ctx);
    add_gold(s, ctx, who, -i64::from(paid), true);
    let sr = &mut s.event.secret_room;
    let round = sr.round;
    let before = sr.apply_tender(side, guild, penya, round, now);
    let Some((snap, generation)) = snapshot_for_batch(s, ctx, who) else {
        // unreachable after `can_snapshot`; undo rather than lose the penya
        s.event.secret_room.undo_tender(side, guild, before);
        add_gold(s, ctx, who, i64::from(paid), true);
        tracing::error!("a secret-room tender lost its snapshot after the check: undone");
        return;
    };
    let Some(row) = s
        .event
        .secret_room
        .side(side)
        .and_then(|sd| sd.tender(guild))
        .map(|t| t.record(side, SecretRoomTenderState::Tendered, t.round))
    else {
        return;
    };
    let tender_round = row.round;
    submit(
        s,
        ctx,
        who,
        Some(generation),
        AtomicBatch::secret_room_tender(snap, row),
        |_svc, _ctx, result| log_result("SecretRoomTender", &result),
    );
    say(ctx, who, TID_GAME_SECRETROOM_TENDERCOMPLETE);
    let mut ev = audit::secret_room_tender(
        tender_round,
        tax_continent(side),
        guild,
        master,
        i64::from(penya),
        i64::from(paid),
    );
    if let Some(actor) = crate::audit::actor_of(ctx, who) {
        ev = ev.by(actor);
    }
    ctx.audit.emit(ev);
}

impl SecretRoomService {
    /// Applies a tender of `penya` for `guild` in memory: a new guild is appended behind its equals, an existing tender
    /// is raised (its `tendered_at` becomes `now`), then the list is re-ranked. Returns the tender as it was (`None` =
    /// new) for [`SecretRoomService::undo_tender`].
    pub(super) fn apply_tender(
        &mut self,
        side: u8,
        guild: GuildId,
        penya: i32,
        round: u32,
        now: flyff_common::UnixTime,
    ) -> Option<SrTender> {
        let sd = self.sides.entry(side).or_default();
        let before = sd.tender(guild).cloned();
        match sd.tenders.iter_mut().find(|t| t.guild == guild) {
            Some(t) => {
                t.penya = penya;
                t.tendered_at = now;
            }
            None => sd.tenders.push(SrTender::new(guild, round, penya, now)),
        }
        sd.sort();
        before
    }

    /// Undoes [`SecretRoomService::apply_tender`].
    pub(super) fn undo_tender(&mut self, side: u8, guild: GuildId, before: Option<SrTender>) {
        let Some(sd) = self.sides.get_mut(&side) else {
            return;
        };
        match before {
            Some(old) => {
                if let Some(t) = sd.tenders.iter_mut().find(|t| t.guild == guild) {
                    *t = old;
                }
            }
            None => sd.tenders.retain(|t| t.guild != guild),
        }
        sd.sort();
    }
}

/// Logs the outcome of a tender or cancel batch: a failure is a bug (the database rolled everything back, the memory
/// is kept, 21 §2.4).
pub(super) fn log_result(name: &str, result: &TicketResult) {
    match result {
        TicketResult::Committed(_) => {}
        TicketResult::Failed(e) => {
            tracing::error!(error = %e, batch = name, "CRITICAL: a secret-room batch failed: the memory is kept (a bug)");
        }
        other => tracing::error!(
            ?other,
            batch = name,
            "a secret-room batch completed unexpectedly"
        ),
    }
}

// ---- SecretRoomTenderCancelReturn ----------------------------------------------------------------------------------------

/// `SecretRoomTenderCancelReturn` (menu 103, `SetTenderCancelReturn` + `SetContTenderCancelReturn`): the manager must be
/// `Close` (`NOTENDERCANCEL`), the player the master (`ISNOTGUILDMASTER`, no level check), the guild must have a tender
/// on his side (`NOTENDERGUILD("\"<side>\"")`). The refund is [`refund_after_fee`]; a wallet that cannot take it
/// (`!can_add`) answers `TID_GAME_LACKSPACE` and changes nothing. The commit: the tender is removed, the refund goes INTO
/// the wallet, ONE `secret_room_cancel` batch (row `C`), `CANCELRETURN("<refund>")`, the audit line.
pub fn on_cancel_return(s: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    if !npc_ok(ctx, who, MMI_SECRET_CANCEL) {
        return;
    }
    if s.event.secret_room.state != SrMngState::Close {
        say(ctx, who, TID_GAME_SECRETROOM_NOTENDERCANCEL);
        return;
    }
    let Some((master, guild)) = master_of(s, ctx, who) else {
        say(ctx, who, TID_GAME_SECRETROOM_ISNOTGUILDMASTER);
        return;
    };
    let side = area_of(ctx, who);
    if side == CONT_NODATA {
        return;
    }
    let Some(tender) = s
        .event
        .secret_room
        .side(side)
        .and_then(|sd| sd.tender(guild))
        .cloned()
    else {
        say_side(ctx, who, TID_GAME_SECRETROOM_NOTENDERGUILD, side);
        return;
    };
    let rate = cfg_of(s).cancel_return_rate;
    let refund = refund_after_fee(tender.penya, rate);
    let wallet = ctx.player(who).map_or(0, gold);
    if !can_add(wallet, i64::from(refund)) {
        say(ctx, who, TID_GAME_LACKSPACE);
        return;
    }
    if !can_snapshot(ctx, who) {
        tracing::debug!("a secret-room cancel of a player without a savable session was refused");
        return;
    }

    // ---- the commit block ----
    let row = tender.record(side, SecretRoomTenderState::Cancelled, tender.round);
    if let Some(sd) = s.event.secret_room.sides.get_mut(&side) {
        sd.tenders.retain(|t| t.guild != guild);
    }
    add_gold(s, ctx, who, i64::from(refund), true);
    let Some((snap, generation)) = snapshot_for_batch(s, ctx, who) else {
        // unreachable after `can_snapshot`; undo
        add_gold(s, ctx, who, -i64::from(refund), true);
        if let Some(sd) = s.event.secret_room.sides.get_mut(&side) {
            sd.tenders.push(tender);
            sd.sort();
        }
        tracing::error!("a secret-room cancel lost its snapshot after the check: undone");
        return;
    };
    submit(
        s,
        ctx,
        who,
        Some(generation),
        AtomicBatch::secret_room_cancel(snap, row),
        |_svc, _ctx, result| log_result("SecretRoomCancel", &result),
    );
    notice::defined(
        ctx,
        who,
        TID_GAME_SECRETROOM_CANCELRETURN,
        vec![refund.to_string()],
    );
    let mut ev = audit::secret_room_cancel(
        tender.round,
        tax_continent(side),
        guild,
        master,
        i64::from(tender.penya),
        i64::from(refund),
    );
    if let Some(actor) = crate::audit::actor_of(ctx, who) {
        ev = ev.by(actor);
    }
    ctx.audit.emit(ev);
}

// ---- SecretRoomTenderViewReq ----------------------------------------------------------------------------------------------

/// `SecretRoomTenderViewReq` (menu 85, `GetTenderView`): the manager must be `Close` (`NOTENDERVIEW`; no master and no
/// level check); then `SecretRoomTenderViewNtf{my_penya, my_rank (1-based, 0 = none), remain_s, guilds in rank order}`
/// of the player's side. `remain_s` is the time to the next opening ([`seconds_to_next_open`]).
pub fn on_tender_view(s: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    if !npc_ok(ctx, who, MMI_SECRET_OFFERSTATE) {
        return;
    }
    if s.event.secret_room.state != SrMngState::Close {
        say(ctx, who, TID_GAME_SECRETROOM_NOTENDERVIEW);
        return;
    }
    let side = area_of(ctx, who);
    if side == CONT_NODATA {
        return;
    }
    let Some(sd) = s.event.secret_room.side(side) else {
        return;
    };
    let guild =
        crate::social::guild::character_of(ctx, who).and_then(|c| s.social.guild.guild_of(c));
    let index = guild.and_then(|g| sd.index_of(g));
    let (my_penya, my_rank) = index.map_or((0, 0), |i| {
        (
            sd.tenders[i].penya,
            i32::try_from(i + 1).unwrap_or(i32::MAX),
        )
    });
    let guilds: Vec<u32> = sd.tenders.iter().map(|t| t.guild.0).collect();
    let remain_s = seconds_to_next_open(s, ctx);
    crate::chat::send_to(
        ctx,
        who,
        pb::SecretRoomTenderViewNtf {
            my_penya,
            my_rank,
            remain_s,
            guilds,
        },
    );
}
