//! The siege result (Plan/21 §7.6; WP-SOCIAL-18; `eveschool.cpp` `GuildCombatResult` 1064-1400, `GuildCombatResultRanking`
//! 1430-1560, `GetBestPlayer` 1890-2048, `GuildCombatCloseTeleport` 1630-1670; `DPDatabaseClient.cpp` result packet
//! 463-645, claims 1506-1660; `DPSrvr.cpp` `OnGCGetPenyaGuild` / `OnGCGetPenyaPlayer` 7796-7870): the points of a kill,
//! the winner, the best player, the red chips, the refunds of the bidders that were not admitted, the ONE result batch,
//! the claims, the cross-siege player ranking and the close.
//!
//! **One batch** (VR-018, 13 §18). The original granted the chips at the results row and sent its database result one
//! row later (`GuildCombatCloseTeleport`), a crash window that duplicated chips. Here [`results`] commits ONE
//! `AtomicBatch::siege_result` that holds everything: the ended siege, a row per bid and per selected player, the refund
//! and overflow mails, the next siege and the snapshots of the online chip recipients; the memory changes in the same
//! block. A cancelled siege (`StopWar::GmCancel`) never reaches this row and persists nothing: every bid carries over.
//!
//! **What the numbers are.** The guild score is Σ (player points + lives); the winner has the best score, then the
//! most lives, then the highest average level (integer average of the ONLINE players with lives), then the earlier rank
//! (`rules::pick_winner`). The chips follow the partial bubble order of the original (`rules::bubble_rank`: points,
//! lives, average level with `1.0` for nobody online) over ALL ranked guilds; ranks 1-3 give every ONLINE selected player
//! `max(1, siege_chips(JOINPENYA, n_bidders, rank))` chips with `n_bidders` = ALL active bidders (KQ-39 i). A bidder that
//! was not admitted gets `not_admitted_refund` (98 %, `f32`) by mail to the CURRENT master of its guild; the winner and the
//! best player win `MAXGUILDPERCENT` / `MAXPLAYERPERCENT` of the prize pool, which is 0 % in the shipped data (KQ-20).

use super::fight::{self, Frozen, character, enabled, live_avatar, send_world, set_gc_state};
use super::{SiegeState, SiegeWarState, set_state, set_war_state};
use crate::chat::ingress::notice;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::econ::mail::MailEntry;
use crate::econ::mail::commit::{can_snapshot, snapshot_for_batch};
use crate::item::model::Item;
use crate::item::ops::{CtxSerials, ItemError, ItemOrigin};
use crate::outbox::Audience;
use crate::tickets::{Pending, ServiceTicket, TicketResult};
use flyff_common::defs::item::II_CHP_RED;
use flyff_common::defs::obj::{MI_MADA_ACHABEN, XI_NAT_ROCKET02};
use flyff_common::defs::sound::BGM_IN_FITUP;
use flyff_common::defs::text::{
    TID_GAME_GC_NOTREQUEST, TID_GAME_GC_NOTREQUEST1, TID_GAME_GUILDCOMBAT_END,
    TID_GAME_GUILDCOMBAT1TO1_WARGUILDWINCHIP,
};
use flyff_common::defs::world::WI_WORLD_GUILDWAR;
use flyff_common::math::Vec3;
use flyff_common::{CharacterId, GuildId, MailId, WorldId};
use flyff_db::audit::social as audit;
use flyff_db::{
    AtomicBatch, BidStatus, GuildSiegeBidRecord, GuildSiegePlayerRecord, GuildSiegeRecord,
    MailRecord, PlayerRewardStatus, SaveReason, SiegeRankRow, SiegeStatus,
};
use flyff_proto::pb;
use flyff_proto::pb::guild_combat::Sub;
use flyff_rules::social::siege as rules;
use flyff_rules::social::siege::GuildScore;

/// The character key of the chip shop (`Mada_Guildcombatshop`).
pub const CHIP_SHOP_KEY: &str = "Mada_Guildcombatshop";
/// Where the two chip shops stand in the siege world (`SetNpc`).
pub const CHIP_SHOPS: [Vec3; 2] = [
    Vec3::new(1343.0, 94.0, 1215.0),
    Vec3::new(1288.0, 98.0, 1356.0),
];
/// The firework of the winner (`XI_NAT_ROCKET02` at (1291, 85, 1279)).
const FIREWORK_AT: Vec3 = Vec3::new(1291.0, 85.0, 1279.0);
/// The longest `GC_PLAYERPOINT` list (ours: the limit of the database procedure is unknown).
pub const PLAYER_POINT_LIMIT: usize = 100;

// ---- the points of a kill ------------------------------------------------------------------------------------------------------

pub use super::fight::kill_points;

// ---- the winner and the best player ---------------------------------------------------------------------------------------------

/// The score of an admitted guild for the winner and the chip ranking.
#[derive(Clone, Debug, PartialEq)]
pub struct GuildResult {
    /// The guild.
    pub guild: GuildId,
    /// Σ (player points + lives).
    pub points: i32,
    /// Σ lives.
    pub lives: i32,
    /// The levels of the ONLINE selected players with lives left.
    pub levels: Vec<i32>,
}

impl GuildResult {
    /// The key of the winner.
    pub fn score(&self) -> GuildScore {
        GuildScore {
            points: self.points,
            lives: self.lives,
            avg_level: rules::avg_level(&self.levels),
        }
    }

    /// The key of the chip ranking (`1.0` as the average of nobody).
    pub fn ranking_score(&self) -> GuildScore {
        GuildScore {
            points: self.points,
            lives: self.lives,
            avg_level: rules::avg_level_for_ranking(&self.levels),
        }
    }
}

/// The score of `guild` over its line-up.
fn guild_result(svc: &Services, ctx: &GameCtx<'_>, guild: GuildId) -> GuildResult {
    let lineup = svc.social.siege.fight.lineup(guild);
    let (points, lives) = lineup.map_or((0, 0), |l| {
        (
            l.select.iter().map(|s| s.points + s.lives).sum(),
            l.total_lives(),
        )
    });
    let levels = lineup
        .map(|l| {
            l.select
                .iter()
                .filter(|s| s.lives > 0)
                .filter_map(|s| live_avatar(&svc.presence, &*ctx.worlds, s.character))
                .filter_map(|o| ctx.worlds.player(o).map(|m| m.mv.level))
                .collect()
        })
        .unwrap_or_default();
    GuildResult {
        guild,
        points,
        lives,
        levels,
    }
}

/// `GuildCombatResult`'s winner among `results` in rank order: `None` for an empty list.
pub fn pick_winner(results: &[GuildResult]) -> Option<GuildId> {
    let scores: Vec<GuildScore> = results.iter().map(GuildResult::score).collect();
    rules::pick_winner(&scores)
        .and_then(|i| results.get(i))
        .map(|r| r.guild)
}

/// `GetBestPlayer` over the admitted guilds in rank order: the player with the most points; equal points are decided
/// by the lowest level, then the lowest EXP, among the tied players who are ONLINE (an offline tied player cannot win
/// the tie-break; without an online one the first player who reached the maximum stays). All-zero points use the same
/// rule. `None` without a selected player.
pub fn best_player(svc: &Services, ctx: &GameCtx<'_>, admitted: &[GuildId]) -> Option<CharacterId> {
    let mut best = None;
    let mut max_point = -1;
    let mut unique = false;
    let mut tied: Vec<CharacterId> = Vec::new();
    for g in admitted {
        let Some(l) = svc.social.siege.fight.lineup(*g) else {
            continue;
        };
        for s in &l.select {
            if max_point < s.points {
                max_point = s.points;
                best = Some(s.character);
                unique = true;
                tied.clear();
                tied.push(s.character);
            } else if max_point == s.points {
                unique = false;
                if !tied.contains(&s.character) {
                    tied.push(s.character);
                }
            }
        }
    }
    if unique {
        return best;
    }
    let mut min_level = i32::MAX;
    let mut min_exp = i64::MAX;
    for g in admitted {
        let Some(l) = svc.social.siege.fight.lineup(*g) else {
            continue;
        };
        for s in &l.select {
            if !tied.contains(&s.character) {
                continue;
            }
            let Some(m) = live_avatar(&svc.presence, &*ctx.worlds, s.character)
                .and_then(|o| ctx.worlds.player(o))
            else {
                continue;
            };
            let level = m.mv.level;
            let exp = m.player().map_or(0, |p| p.progression.exp);
            if min_level > level || (min_level == level && min_exp > exp) {
                min_level = level;
                min_exp = exp;
                best = Some(s.character);
            }
        }
    }
    best
}

/// `GuildCombatResultRanking`'s order: every ranked guild (admitted or not, best first) by the partial bubble sort of
/// the original. `results` are the admitted guilds' scores; a guild without one counts 0 points, 0 lives and `1.0`.
pub fn rank_guilds(ranked: &[GuildId], results: &[GuildResult], max_pass: usize) -> Vec<GuildId> {
    let scores: Vec<GuildScore> = ranked
        .iter()
        .map(|g| {
            results.iter().find(|r| r.guild == *g).map_or(
                GuildScore {
                    points: 0,
                    lives: 0,
                    avg_level: 1.0,
                },
                GuildResult::ranking_score,
            )
        })
        .collect();
    rules::bubble_rank(&scores, max_pass)
        .into_iter()
        .filter_map(|i| ranked.get(i).copied())
        .collect()
}

// ---- the chips ----------------------------------------------------------------------------------------------------------------------

/// One recipient of red chips.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChipAward {
    /// The guild he fought for.
    pub guild: GuildId,
    /// The rank of the guild, 1..=3.
    pub rank: u8,
    /// The player.
    pub character: CharacterId,
    /// How many chips.
    pub chips: u16,
    /// They went by system mail (a full inventory, or a session that cannot be snapshotted).
    pub mailed: bool,
}

/// What [`award_chips`] left behind for the batch.
#[derive(Debug, Default)]
pub struct Chips {
    /// The awards in order.
    pub awards: Vec<ChipAward>,
    /// The mails of the awards that could not be delivered in the inventory.
    pub mails: Vec<MailId>,
    /// The players whose inventory changed (they need a snapshot in the batch).
    pub changed: Vec<PlayerRef>,
}

/// The chip item (`II_CHP_RED`, `m_bCharged` from the prop).
fn chip_item(ctx: &GameCtx<'_>, n: u16) -> Item {
    let mut item = Item::new(II_CHP_RED, n);
    item.charged = ctx.data.item(II_CHP_RED).is_some_and(|p| p.charged != 0);
    item
}

/// The chips of the three best guilds to their ONLINE selected players (lives irrelevant): `n = max(1, siege_chips(..))`
/// of `II_CHP_RED` in the inventory, `DefinedText(TID_GAME_GUILDCOMBAT1TO1_WARGUILDWINCHIP, n)` to the recipient; a
/// full inventory (or a session that cannot be snapshotted) turns the award into a system mail whose title and text are
/// the item's name. Memory only: the batch of [`results`] carries the snapshots and the mails.
pub fn award_chips(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    top: &[GuildId],
    n_bidders: usize,
) -> Chips {
    let mut out = Chips::default();
    let join_penya = svc.social.siege.config.join_penya;
    let name = ctx
        .data
        .item(II_CHP_RED)
        .map(|p| p.name.clone())
        .unwrap_or_default();
    for (rank, guild) in top.iter().enumerate().take(rules::CHIP_RANKS) {
        let n = rules::siege_chips(join_penya, n_bidders, rank).max(1);
        let n = u16::try_from(n).unwrap_or(u16::MAX);
        let recipients: Vec<CharacterId> = svc
            .social
            .siege
            .fight
            .lineup(*guild)
            .map(|l| l.select.iter().map(|s| s.character).collect())
            .unwrap_or_default();
        for c in recipients {
            let Some(obj) = live_avatar(&svc.presence, &*ctx.worlds, c) else {
                continue;
            };
            let who = PlayerRef(obj);
            let mut award = ChipAward {
                guild: *guild,
                rank: (rank + 1) as u8,
                character: c,
                chips: n,
                mailed: false,
            };
            let item = chip_item(ctx, n);
            let delivered = if can_snapshot(ctx, who) {
                match crate::item::ops::create_item(
                    svc,
                    ctx,
                    who,
                    item.clone(),
                    ItemOrigin::Other("siege_chip"),
                ) {
                    Ok(_) => {
                        notice::defined(
                            ctx,
                            who,
                            TID_GAME_GUILDCOMBAT1TO1_WARGUILDWINCHIP,
                            vec![n.to_string()],
                        );
                        out.changed.push(who);
                        true
                    }
                    Err(ItemError::LackSpace) => false,
                    Err(e) => {
                        tracing::error!(error = %e, character = c.0, "siege chips: the item could not be created");
                        continue;
                    }
                }
            } else {
                false
            };
            if !delivered {
                let mut item = item;
                if item.serial.0 == 0 {
                    let Services { serials, .. } = &mut *svc;
                    let mut src = CtxSerials::new(serials, ctx.persist, ctx.tickets);
                    match crate::item::container::SerialSource::next_serial(&mut src) {
                        Ok(s) => item.serial = flyff_common::ItemSerial(s),
                        Err(e) => {
                            tracing::error!(error = %e, character = c.0, "siege chips: no serial for the mail");
                            continue;
                        }
                    }
                }
                let (id, _) = svc
                    .econ
                    .mail
                    .send_system_with_id(c, &name, &name, Some(item), 0);
                out.mails.push(id);
                award.mailed = true;
            }
            out.awards.push(award);
        }
    }
    out
}

// ---- the result ----------------------------------------------------------------------------------------------------------------------------

/// The refund mails of the bidders that were not admitted: `(guild, bid, refund)` for the ones that got one.
struct Refunds {
    mails: Vec<MailId>,
    /// `(guild, refund)` of every non-admitted active bid (0: nobody to mail).
    amounts: Vec<(GuildId, i32)>,
}

fn refund_mails(svc: &mut Services, ctx: &GameCtx<'_>, admitted: &[GuildId]) -> Refunds {
    let percent = svc.social.siege.config.not_request_percent;
    let title = text_of(ctx, TID_GAME_GC_NOTREQUEST);
    let body = text_of(ctx, TID_GAME_GC_NOTREQUEST1);
    let bids: Vec<(GuildId, u32)> = svc
        .social
        .siege
        .bid_entries()
        .iter()
        .filter(|b| b.active && !admitted.contains(&b.guild))
        .map(|b| (b.guild, b.fee))
        .collect();
    let mut out = Refunds {
        mails: Vec::new(),
        amounts: Vec::new(),
    };
    for (guild, fee) in bids {
        let refund = rules::not_admitted_refund(fee, percent).max(0);
        let master = svc.social.guild.get(guild).and_then(|g| g.master());
        match master {
            Some(m) => {
                let (id, _) =
                    svc.econ
                        .mail
                        .send_system_with_id(m, &title, &body, None, i64::from(refund));
                out.mails.push(id);
                out.amounts.push((guild, refund));
            }
            None => {
                // KEEP: a vanished guild is not refunded (21 §7.6; the row is closed with nothing paid)
                tracing::error!(
                    guild = guild.0,
                    bid = fee,
                    refund,
                    "siege result: the guild of a bid that was not admitted is gone: the refund is lost"
                );
                out.amounts.push((guild, 0));
            }
        }
    }
    out
}

fn text_of(ctx: &GameCtx<'_>, tid: u32) -> String {
    fight::text_of(ctx, tid)
}

/// `WARSTATE 106` (`SetGuildCombatCloseWait( FALSE )`: `GuildCombatResult` + `GuildCombatResultRanking` + the database
/// result in one block): see the module documentation. A cancelled siege and a second call do nothing.
pub fn results(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    if !enabled(ctx) || svc.social.siege.gm_cancelled() || svc.social.siege.fight.is_frozen() {
        return;
    }
    let now = crate::social::party::now_secs(ctx);
    let cfg = svc.social.siege.config.clone();
    let siege_id = svc.social.siege.siege_id();
    let admitted = fight::admitted(svc);
    let ranking = svc.social.siege.ranking();
    let ranked: Vec<GuildId> = ranking.iter().map(|r| r.guild).collect();
    let n_bidders = ranking.len();
    let prize = svc.social.siege.prize_pool();
    svc.social.siege.fight.freeze(Frozen {
        admitted: admitted.clone(),
        order: fight::bid_order(svc),
    });

    // ---- GuildCombatResult
    let guild_results: Vec<GuildResult> = admitted
        .iter()
        .map(|g| guild_result(svc, ctx, *g))
        .collect();
    for r in &guild_results {
        if let Some(l) = svc.social.siege.fight.guilds.get_mut(&r.guild) {
            l.points = r.points;
        }
    }
    let winner = pick_winner(&guild_results).filter(|g| svc.social.guild.get(*g).is_some());
    let (prev_winner, prev_streak, prev_best) = {
        let s = &svc.social.siege;
        (s.win_guild(), s.win_count(), s.best_player())
    };
    let (win_guild, streak, best) = match winner {
        Some(w) => {
            let streak = if prev_winner == Some(w) {
                prev_streak.saturating_add(1)
            } else {
                1
            };
            (Some(w), streak, best_player(svc, ctx, &admitted))
        }
        // nobody won: the memory keeps the last winner (the original sent it to the database again)
        None => (prev_winner, prev_streak, prev_best),
    };

    // ---- GuildCombatResultRanking: the chips
    let max_pass = usize::try_from(cfg.max_join_guild_size).unwrap_or(0);
    let order = rank_guilds(&ranked, &guild_results, max_pass);
    let top: Vec<GuildId> = order.iter().copied().take(rules::CHIP_RANKS).collect();
    let chips = award_chips(svc, ctx, &top, n_bidders);

    // ---- the refunds of the bidders that were not admitted
    let refunds = refund_mails(svc, ctx, &admitted);
    let refunded: i64 = refunds.amounts.iter().map(|(_, r)| i64::from(*r)).sum();

    // ---- the rows
    let guild_prize = rules::prize_share(prize, cfg.max_guild_percent);
    let player_prize = rules::prize_share(prize, cfg.max_player_percent);
    let ended = GuildSiegeRecord {
        id: siege_id,
        status: SiegeStatus::Ended,
        opened_at: if svc.social.siege.opened_at().0 == 0 {
            now
        } else {
            svc.social.siege.opened_at()
        },
        started_at: svc.social.siege.fight.started_at().or(Some(now)),
        ended_at: Some(now),
        winner_guild_id: win_guild,
        winner_streak: streak,
        best_player_id: best,
    };
    let mut bid_rows: Vec<GuildSiegeBidRecord> = Vec::new();
    let mut unpaid_bids: Vec<GuildSiegeBidRecord> = Vec::new();
    for e in svc.social.siege.bid_entries().iter().filter(|e| e.active) {
        let mut row = e.record(siege_id, BidStatus::Graded);
        if admitted.contains(&e.guild) {
            let won = winner == Some(e.guild);
            row.points = svc
                .social
                .siege
                .fight
                .lineup(e.guild)
                .map_or(0, |l| l.points);
            row.straight_wins = if won { streak } else { 0 };
            row.reward = if won { guild_prize } else { 0 };
        } else {
            row.refund = refunds
                .amounts
                .iter()
                .find(|(g, _)| *g == e.guild)
                .map_or(0, |(_, r)| i64::from(*r));
        }
        if row.refund + row.reward > 0 && admitted.contains(&e.guild) {
            unpaid_bids.push(row.clone());
        } else {
            row.status = BidStatus::Paid;
            row.rewarded_at = Some(now);
        }
        bid_rows.push(row);
    }
    let mut player_rows: Vec<GuildSiegePlayerRecord> = Vec::new();
    let mut unpaid_players: Vec<GuildSiegePlayerRecord> = Vec::new();
    for g in &admitted {
        let Some(l) = svc.social.siege.fight.lineup(*g) else {
            continue;
        };
        for s in &l.select {
            let reward = if best == Some(s.character) {
                player_prize
            } else {
                0
            };
            let mut row = GuildSiegePlayerRecord {
                siege_id,
                guild_id: *g,
                character_id: s.character,
                status: PlayerRewardStatus::Pending,
                points: s.points,
                reward,
                rewarded_at: None,
            };
            if reward > 0 {
                unpaid_players.push(row.clone());
            } else {
                row.status = PlayerRewardStatus::Paid;
                row.rewarded_at = Some(now);
            }
            player_rows.push(row);
        }
    }
    let mail_records: Vec<MailRecord> = refunds
        .mails
        .iter()
        .chain(chips.mails.iter())
        .filter_map(|id| svc.econ.mail.get(*id).map(MailEntry::to_record))
        .collect();
    let next_id = siege_id.saturating_add(1);
    let next = GuildSiegeRecord::open(next_id, now);
    let mut snapshots = Vec::new();
    let mut generations = Vec::new();
    for who in &chips.changed {
        match snapshot_for_batch(svc, ctx, *who) {
            Some((snap, generation)) => {
                snapshots.push(snap);
                generations.push((*who, generation));
            }
            None => tracing::error!(
                player = who.0.0,
                "siege result: a chip recipient could not be snapshotted: the next save keeps his chips"
            ),
        }
    }

    // ---- the batch, with the memory changes in the same block
    let all_mails: Vec<MailId> = refunds
        .mails
        .iter()
        .chain(chips.mails.iter())
        .copied()
        .collect();
    let batch = AtomicBatch::siege_result(
        snapshots,
        ended.clone(),
        bid_rows,
        player_rows,
        mail_records,
        next,
    );
    let ticket = ctx.tickets.issue(Pending::Service(ServiceTicket::new(
        move |svc, _ctx, result| match result {
            TicketResult::Committed(_) => {}
            TicketResult::Failed(e) => {
                for id in &all_mails {
                    svc.econ.mail.remove(*id);
                }
                tracing::error!(
                    error = %e,
                    siege = siege_id,
                    "CRITICAL: the siege result batch failed: the memory is kept (a bug, 21 §2.4)"
                );
            }
            other => tracing::error!(?other, "the siege result batch completed unexpectedly"),
        },
    )));
    ctx.persist.atomic(batch.with_ticket(ticket));
    for (who, generation) in generations {
        svc.saves
            .record(who.0, generation, ctx.clock.game_ms, SaveReason::Atomic);
    }
    svc.social.siege.fight.set_last_ended(ended);
    // the carried lists and the cross-siege ranking
    let mut bids_left = svc.social.siege.unpaid_bids().to_vec();
    bids_left.extend(unpaid_bids);
    let mut players_left = svc.social.siege.unpaid_players().to_vec();
    players_left.extend(unpaid_players);
    svc.social.siege.set_unpaid(bids_left, players_left);
    update_player_ranking(svc, &admitted);
    svc.social.siege.set_result(win_guild, streak, best);
    svc.social.siege.begin_next_siege(next_id);

    // ---- audit
    ctx.audit.emit(audit::siege_result(
        siege_id,
        win_guild,
        streak,
        best,
        u32::try_from(n_bidders).unwrap_or(u32::MAX),
        refunded,
    ));
    for a in &chips.awards {
        ctx.audit.emit(audit::siege_chips(
            siege_id,
            a.guild,
            a.character,
            u32::from(a.chips),
            a.rank,
            a.mailed,
        ));
    }
    if !svc.social.siege.fight.send_items_logged() && !ctx.data.guild.combat.send_items.is_empty() {
        tracing::info!(
            "GuildCombat.txt lists GCSENDITEM win-streak items: not implemented (the guild bank gets none)"
        );
    }

    // ---- the announcements
    if winner.is_some() {
        announce(svc, ctx, win_guild, streak, best);
    }
}

/// The messages of a result with a winner: `TID_GAME_GUILDCOMBAT_END` (system notice and caption), the firework and the
/// music in the siege world, `GC_WINGUILD`, `GC_BESTPLAYER` to everybody and `GC_LOG` to the siege world.
fn announce(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    winner: Option<GuildId>,
    streak: i32,
    best: Option<CharacterId>,
) {
    let line = text_of(ctx, TID_GAME_GUILDCOMBAT_END);
    notice::system(svc, ctx, &line);
    notice::caption(svc, ctx, Audience::AllOnline, false, WorldId(0), &line);
    crate::world::fx::sfx_world(
        ctx,
        WorldId(WI_WORLD_GUILDWAR),
        XI_NAT_ROCKET02,
        FIREWORK_AT,
        false,
    );
    crate::world::fx::music_world(ctx, WorldId(WI_WORLD_GUILDWAR), BGM_IN_FITUP);
    super::send_all(
        svc,
        ctx,
        Sub::GcWinGuild(pb::guild_combat::GcWinGuild {
            combat_index: i32::try_from(svc.social.siege.siege_id()).unwrap_or(i32::MAX),
            win_guild: winner.map_or(0, |g| g.0),
            win_count: streak,
        }),
    );
    super::send_all(
        svc,
        ctx,
        Sub::GcBestPlayer(pb::guild_combat::GcBestPlayer {
            id_player: best.map_or(0, |c| c.0),
        }),
    );
    let entries = svc
        .social
        .siege
        .fight
        .kills
        .iter()
        .map(fight::KillRecord::to_entry)
        .collect();
    send_world(svc, ctx, Sub::GcLog(pb::guild_combat::GcLog { entries }));
}

/// The sums per player over all sieges (`uspRankGuildCombatPlayer`) with the points of this siege added.
fn update_player_ranking(svc: &mut Services, admitted: &[GuildId]) {
    let mut rows: Vec<SiegeRankRow> = svc.social.siege.player_ranking().to_vec();
    for g in admitted {
        let Some(l) = svc.social.siege.fight.lineup(*g) else {
            continue;
        };
        for s in &l.select {
            let job = svc.presence.get(s.character).map_or(0, |e| e.job);
            match rows.iter_mut().find(|r| r.character_id == s.character) {
                Some(r) => {
                    r.points += i64::from(s.points);
                    r.job = job;
                }
                None => rows.push(SiegeRankRow {
                    character_id: s.character,
                    job,
                    points: i64::from(s.points),
                }),
            }
        }
    }
    svc.social.siege.set_player_ranking(rows);
}

// ---- claims ----------------------------------------------------------------------------------------------------------------------------

/// The gold ceiling of a purse and of a claim (`INT_MAX`).
const PURSE_MAX: i64 = i32::MAX as i64;

/// `OnGCGetPenyaGuild` (the master of a guild collects the oldest unpaid row of its guild): not the master -> silent;
/// no row -> `GC_GETPENYAGUILD{2, 0}`; `get = refund + reward` above `INT_MAX` or over the purse -> `{3, get}` and
/// nothing changes; else the penya is paid and the row goes to 31 in ONE `SiegeClaim` batch, the answer is
/// `{1 if refund > 0 else 0, get}`.
pub fn claim_guild(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    if !enabled(ctx) {
        return;
    }
    let Some(c) = character(ctx, who.0) else {
        return;
    };
    let Some(guild) = svc.social.guild.guild_of(c) else {
        return;
    };
    if !svc.social.guild.is_master(c) {
        return;
    }
    let row = svc
        .social
        .siege
        .unpaid_bids()
        .iter()
        .filter(|r| r.guild_id == guild && r.status == BidStatus::Graded)
        .min_by_key(|r| r.siege_id)
        .cloned();
    let reply = |ctx: &mut GameCtx<'_>, result: i32, penya: i64| {
        super::send_one(
            ctx,
            who.0,
            Sub::GcGetPenyaGuild(pb::guild_combat::GcGetPenyaGuild { result, penya }),
        );
    };
    let Some(row) = row else {
        reply(ctx, 2, 0);
        return;
    };
    let get = row.refund + row.reward;
    let gold = ctx.player(who).map_or(0, crate::item::gold::core::gold);
    if get > PURSE_MAX || gold + get > PURSE_MAX {
        reply(ctx, 3, get);
        return;
    }
    if !can_snapshot(ctx, who) {
        tracing::debug!("a siege claim of a player without a savable session was refused");
        return;
    }
    crate::item::gold::add_gold(svc, ctx, who, get, true);
    let mut paid = row.clone();
    paid.status = BidStatus::Paid;
    paid.rewarded_at = Some(crate::social::party::now_secs(ctx));
    let Some((snap, generation)) = snapshot_for_batch(svc, ctx, who) else {
        crate::item::gold::add_gold(svc, ctx, who, -get, true);
        tracing::error!("a siege claim lost its snapshot after the check: undone");
        return;
    };
    let siege = row.siege_id;
    submit_claim(
        svc,
        ctx,
        who,
        generation,
        AtomicBatch::siege_claim_guild(snap, paid),
        "SiegeClaim",
    );
    remove_unpaid_bid(svc, siege, guild);
    reply(ctx, i32::from(row.refund > 0), get);
    let mut ev = audit::siege_claim(siege, guild, c, get, "guild");
    if let Some(actor) = crate::audit::actor_of(ctx, who) {
        ev = ev.by(actor);
    }
    ctx.audit.emit(ev);
}

/// `OnGCGetPenyaPlayer` (a selected player collects the reward of his own row): no row -> `GC_GETPENYAPLAYER{1, 0}`;
/// `reward` above `INT_MAX` or over the purse -> `{2, reward}`; else paid in ONE `SiegeClaim` batch, answer `{0, reward}`.
pub fn claim_player(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    if !enabled(ctx) {
        return;
    }
    let Some(c) = character(ctx, who.0) else {
        return;
    };
    let row = svc
        .social
        .siege
        .unpaid_players()
        .iter()
        .filter(|r| r.character_id == c && r.status == PlayerRewardStatus::Pending)
        .min_by_key(|r| r.siege_id)
        .cloned();
    let reply = |ctx: &mut GameCtx<'_>, result: i32, penya: i64| {
        super::send_one(
            ctx,
            who.0,
            Sub::GcGetPenyaPlayer(pb::guild_combat::GcGetPenyaPlayer { result, penya }),
        );
    };
    let Some(row) = row else {
        reply(ctx, 1, 0);
        return;
    };
    let gold = ctx.player(who).map_or(0, crate::item::gold::core::gold);
    if row.reward > PURSE_MAX || gold + row.reward > PURSE_MAX {
        reply(ctx, 2, row.reward);
        return;
    }
    if !can_snapshot(ctx, who) {
        tracing::debug!("a siege claim of a player without a savable session was refused");
        return;
    }
    crate::item::gold::add_gold(svc, ctx, who, row.reward, true);
    let mut paid = row.clone();
    paid.status = PlayerRewardStatus::Paid;
    paid.rewarded_at = Some(crate::social::party::now_secs(ctx));
    let Some((snap, generation)) = snapshot_for_batch(svc, ctx, who) else {
        crate::item::gold::add_gold(svc, ctx, who, -row.reward, true);
        tracing::error!("a siege claim lost its snapshot after the check: undone");
        return;
    };
    submit_claim(
        svc,
        ctx,
        who,
        generation,
        AtomicBatch::siege_claim_player(snap, paid),
        "SiegeClaim",
    );
    let (siege, guild) = (row.siege_id, row.guild_id);
    let mut players = svc.social.siege.unpaid_players().to_vec();
    players.retain(|r| !(r.siege_id == siege && r.character_id == c));
    let bids = svc.social.siege.unpaid_bids().to_vec();
    svc.social.siege.set_unpaid(bids, players);
    reply(ctx, 0, row.reward);
    let mut ev = audit::siege_claim(siege, guild, c, row.reward, "player");
    if let Some(actor) = crate::audit::actor_of(ctx, who) {
        ev = ev.by(actor);
    }
    ctx.audit.emit(ev);
}

fn remove_unpaid_bid(svc: &mut Services, siege: u32, guild: GuildId) {
    let mut bids = svc.social.siege.unpaid_bids().to_vec();
    bids.retain(|r| !(r.siege_id == siege && r.guild_id == guild));
    let players = svc.social.siege.unpaid_players().to_vec();
    svc.social.siege.set_unpaid(bids, players);
}

fn submit_claim(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    generation: flyff_common::SaveGen,
    batch: AtomicBatch,
    label: &'static str,
) {
    crate::econ::mail::commit::submit(
        svc,
        ctx,
        who,
        Some(generation),
        batch,
        move |_svc, _ctx, result| super::log_batch_result(label, &result),
    );
}

// ---- the cross-siege ranking ------------------------------------------------------------------------------------------------------

/// `GC_PLAYERPOINT`: the sums per player over all sieges, highest first then by character id, only players with a sum
/// above 0, at most [`PLAYER_POINT_LIMIT`]; `job_of` answers the job (the directory's).
pub fn player_point_msg(ranking: &[SiegeRankRow], job_of: impl Fn(CharacterId) -> u8) -> Sub {
    let mut rows: Vec<&SiegeRankRow> = ranking.iter().filter(|r| r.points > 0).collect();
    rows.sort_by(|a, b| {
        b.points
            .cmp(&a.points)
            .then(a.character_id.cmp(&b.character_id))
    });
    let entries = rows
        .into_iter()
        .take(PLAYER_POINT_LIMIT)
        .map(|r| pb::guild_combat::GcPlayerPointEntry {
            id_player: r.character_id.0,
            job: i32::from(job_of(r.character_id)),
            point: i32::try_from(r.points).unwrap_or(i32::MAX),
        })
        .collect();
    Sub::GcPlayerPoint(pb::guild_combat::GcPlayerPoint { entries })
}

/// `OnGCPlayerPoint`.
pub fn player_point(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    if !enabled(ctx) {
        return;
    }
    let sub = player_point_msg(svc.social.siege.player_ranking(), |c| {
        svc.presence.get(c).map_or(0, |e| e.job)
    });
    super::send_one(ctx, who.0, sub);
}

// ---- the close ----------------------------------------------------------------------------------------------------------------------------

/// `WARSTATE 107` (`GuildCombatCloseTeleport`): everybody in the siege world goes to Madrigal, every selected player is
/// in state 0 again, the line-up tables are cleared (a fresh line-up per siege, FIX), the war state becomes 105 and
/// the siege state `Close` (each with its broadcast; the script then writes 107 and announces `(1, 107)`), the stop
/// request and the cancel mark are reset and the chip shops are made sure of.
pub fn close_teleport(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    if !enabled(ctx) {
        return;
    }
    super::kick_to_madrigal(ctx);
    let selected: Vec<CharacterId> = svc
        .social
        .siege
        .fight
        .guilds
        .values()
        .flat_map(|l| l.select.iter().map(|s| s.character))
        .collect();
    for c in selected {
        if let Some(obj) = live_avatar(&svc.presence, &*ctx.worlds, c) {
            set_gc_state(ctx, obj, 0);
        }
    }
    svc.social.siege.fight.clear_siege();
    set_war_state(svc, ctx, SiegeWarState::WarClose);
    set_state(svc, ctx, SiegeState::Close);
    svc.social.siege.reset_stop();
    ensure_chip_shop_npcs(svc, ctx);
}

/// `SetNpc`: two `Mada_Guildcombat shop` NPCs stand in the siege world at (1343, 94, 1215) and (1288, 98, 1356), so
/// the red chips can be spent. Idempotent: only the missing ones are spawned (the original created a new pair at every
/// start and every close). A game without the NPC key or the mover prop logs once and skips.
pub fn ensure_chip_shop_npcs(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    svc.social.siege.fight.set_chip_shop_done(true);
    let world = WorldId(WI_WORLD_GUILDWAR);
    if ctx.worlds.world(world).is_none() {
        return;
    }
    let (Some(def), Some(prop)) = (ctx.data.npc(CHIP_SHOP_KEY), ctx.data.mover(MI_MADA_ACHABEN))
    else {
        tracing::warn!(
            "the chip shop NPC (Mada_Guildcombatshop) is not in the game data: no chip shop in the siege world"
        );
        return;
    };
    let standing: Vec<Vec3> = ctx
        .worlds
        .world(world)
        .map(|w| {
            w.layers
                .values()
                .flat_map(|l| l.members.iter().copied())
                .filter_map(|o| ctx.worlds.mover(o))
                .filter(|m| {
                    m.npc()
                        .is_some_and(|n| n.character_key.eq_ignore_ascii_case(CHIP_SHOP_KEY))
                })
                .map(|m| m.core.pos)
                .collect()
        })
        .unwrap_or_default();
    for pos in CHIP_SHOPS {
        let there = standing
            .iter()
            .any(|p| (p.x - pos.x).abs() < 2.0 && (p.z - pos.z).abs() < 2.0);
        if there {
            continue;
        }
        let (core, sim) = crate::mover::npc_parts(ctx.data, prop, pos, 0.0);
        let mover = crate::mover::Mover::new_npc(
            core,
            sim,
            prop,
            Some(def),
            ctx.data,
            ctx.rng.world(world),
        );
        if let Err(e) = ctx
            .worlds
            .spawn_mover(world, flyff_common::LayerId(0), mover)
        {
            tracing::warn!(error = %e, "a chip shop NPC could not be placed");
        }
    }
}
