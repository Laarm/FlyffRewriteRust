//! The weekly opening (Plan/21 §2.2, §7.2; `CGuildCombat::Process`, `src:_Common/eveschool.cpp:2751-2797`,
//! `GuildCombatOpen` `1686-1734`): the wall job `siege_open` with one `Weekly` spec per `AUTO_OPEN_IDC` line, its body
//! and [`open`], which the GM command `/gcopen` calls as well.
//!
//! The schedule is doc 10's wall machinery (boundary-crossed, in `server.timezone`, PL-100): nothing here compares a
//! weekday, an hour or a minute with the clock, and no lock is kept (the original's 60 s mutex only kept a second
//! firing out of the matched minute; the scheduler fires once per crossed boundary).

use super::{SiegeState, SiegeWarState, StopWar};
use crate::chat::ingress::notice;
use crate::ctx::{GameCtx, Services};
use crate::event::calendar::NEVER;
use crate::sched::wall::{CatchUp, WallJob, WallSpec};
use crate::sched::{DocId, Scheduler};
use crate::social::guild::GuildRank;
use crate::tick::LocalStamp;
use flyff_common::GameMs;
use flyff_common::config::EventId;
use flyff_common::defs::text::{
    TID_GAME_GUILDCOMBAT_ENJOY, TID_GAME_GUILDCOMBAT_JOIN_CANCLE, TID_GAME_GUILDCOMBAT_JOIN_OK,
    TID_GAME_GUILDCOMBAT_NEXT_COMBAT,
};
use flyff_data::social::guild::GuildCombatSettings;
use flyff_db::{AtomicBatch, BidStatus, GuildSiegeRecord, SiegeStatus};
use flyff_proto::pb;
use smallvec::SmallVec;

/// The `WallJob` name (the `schedule.last_fired.<job>` key suffix, 13 §15.2).
pub const SIEGE_OPEN: &str = "siege_open";

/// The owning document.
const DOC: DocId = DocId(21);

/// The specs of `AUTO_OPEN_IDC`: one `Weekly{weekday: day - 1, hour, minute}` per line in file order (`7 18 00` is
/// Saturday 18:00 in `server.timezone`). A line with hour 24 never matched in the original (`GetHour() == 24`) and is
/// skipped with a warning, so is any other line the scheduler could not hold; no valid line gives the single spec
/// [`NEVER`] (a job needs a spec).
pub fn specs(combat: &GuildCombatSettings) -> SmallVec<[WallSpec; 2]> {
    let mut out: SmallVec<[WallSpec; 2]> = SmallVec::new();
    for line in &combat.auto_open {
        let ok = (1..=7).contains(&line.day)
            && (0..=23).contains(&line.hour)
            && (0..=59).contains(&line.minute);
        if !ok {
            tracing::warn!(
                day = line.day,
                hour = line.hour,
                minute = line.minute,
                "GuildCombat.txt: an AUTO_OPEN_IDC line can never match (hour 24 never did in the original): skipped"
            );
            continue;
        }
        out.push(WallSpec::Weekly {
            weekday: (line.day - 1) as u8,
            hour: line.hour as u8,
            minute: line.minute as u8,
        });
    }
    if out.is_empty() {
        out.push(NEVER);
    }
    out
}

/// Registers the job `siege_open` (`CatchUp::Skip`: an opening missed while the server was down is not made up).
/// The specs are validated here, so `register_wall` cannot panic.
pub fn register(sched: &mut Scheduler, combat: &GuildCombatSettings) {
    sched.register_wall(WallJob {
        name: SIEGE_OPEN,
        owner: DOC,
        specs: specs(combat),
        catch_up: CatchUp::Skip,
        run: on_open_time,
    });
}

/// Why [`open`] did nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum OpenError {
    /// The siege is not `Close` (`GuildCombatOpen` returns at once).
    #[error("the siege is not closed")]
    NotClose,
}

/// The body of the job (gate `events.guild_combat`, checked here): only in state `Close`; with at least
/// `MINJOINGUILDSIZE` ACTIVE bidders (FIX: the original counted cancelled entries too) the siege opens, else two
/// system notices (`TID_GAME_GUILDCOMBAT_NEXT_COMBAT`, `..._ENJOY`) say that there is none this week.
pub fn on_open_time(svc: &mut Services, ctx: &mut GameCtx<'_>, _stamp: &LocalStamp) {
    if ctx.live.event_state(EventId::GuildCombat) == 0 {
        return;
    }
    if svc.social.siege.state != SiegeState::Close {
        return;
    }
    let min = usize::try_from(svc.social.siege.config.min_join_guild_size).unwrap_or(0);
    if svc.social.siege.n_bidders() >= min {
        let _ = open(svc, ctx);
        return;
    }
    for tid in [TID_GAME_GUILDCOMBAT_NEXT_COMBAT, TID_GAME_GUILDCOMBAT_ENJOY] {
        let text = ctx.data.texts.get(tid).to_owned();
        if !text.is_empty() && text != "error" {
            notice::system(svc, ctx, &text);
        }
    }
}

/// `GuildCombatOpen`: state `Open`, war state `NotEnter`, the script at row 0 with the timer cleared and no stop
/// request. ONE `AtomicBatch::siege_open` (the siege row in status 20, the active bids in status 20 and the expiry of
/// the unclaimed rewards of older sieges: `uspStartCombat` and `uspExpireCombatReward`), `GC_GCSTATE` to everybody and
/// to every online master and kingpin of every ranked bidder a `GC_DIAGMESSAGE` ("admitted" for a rank below
/// `MAXJOINGUILDSIZE`, else "not admitted"). Works with an empty ranking (`/gcopen` skips the bidder count).
pub fn open(svc: &mut Services, ctx: &mut GameCtx<'_>) -> Result<(), OpenError> {
    if svc.social.siege.state != SiegeState::Close {
        return Err(OpenError::NotClose);
    }
    let now = crate::social::party::now_secs(ctx);
    let id = {
        let s = &mut svc.social.siege;
        s.war_state = SiegeWarState::NotEnter;
        s.state = SiegeState::Open;
        s.step = 0;
        s.step_due = GameMs(0);
        s.stop_war = StopWar::None;
        s.gm_cancelled = false;
        s.publish();
        s.siege_id
    };
    // the persistence in the same block as the memory
    let (batch, creates_row) = {
        let s = &mut svc.social.siege;
        // the database keeps the `opened_at` of the stored row; the value matters only for a row that does not exist
        if s.siege_opened_at.0 == 0 {
            s.siege_opened_at = now;
        }
        let mut siege = GuildSiegeRecord::open(id, s.siege_opened_at);
        siege.status = SiegeStatus::Started;
        siege.started_at = Some(now);
        s.fight.mark_started(now); // siege-b's result rewrites the row and needs the time
        let mut fighting = Vec::new();
        for b in s.bids.iter_mut().filter(|b| b.active) {
            b.status = BidStatus::Fighting;
            fighting.push(b.record(id, BidStatus::Fighting));
        }
        // `uspExpireCombatReward`: what older sieges left unclaimed is gone from the carried lists
        s.unpaid_bids.retain(|r| r.siege_id >= id);
        s.unpaid_players.retain(|r| r.siege_id >= id);
        let mut batch = AtomicBatch::siege_open(siege, fighting, id, now);
        // `UpsertGuildSiege` creates a missing row; only the counter is left to add
        let creates_row = s.needs_row;
        if creates_row {
            batch.ops.push(s.counter_op());
        }
        (batch, creates_row)
    };
    super::submit_logged(ctx, "SiegeOpen", batch, creates_row);
    super::broadcast_state(svc, ctx);
    announce(svc, ctx);
    Ok(())
}

/// The admitted / not-admitted dialog to the masters and kingpins of the ranked bidders.
fn announce(svc: &Services, ctx: &mut GameCtx<'_>) {
    let max = usize::try_from(svc.social.siege.config.max_join_guild_size).unwrap_or(0);
    let ok = ctx.data.texts.get(TID_GAME_GUILDCOMBAT_JOIN_OK).to_owned();
    let cancel = ctx
        .data
        .texts
        .get(TID_GAME_GUILDCOMBAT_JOIN_CANCLE)
        .to_owned();
    for (index, ranked) in svc.social.siege.ranking().into_iter().enumerate() {
        let Some(guild) = svc.social.guild.get(ranked.guild) else {
            continue;
        };
        let text = if index < max { &ok } else { &cancel };
        for (c, m) in &guild.members {
            if !matches!(m.rank, GuildRank::Master | GuildRank::Kingpin) {
                continue;
            }
            let Some(obj) = svc.presence.get(*c).and_then(|e| e.online).map(|o| o.obj) else {
                continue;
            };
            super::send_one(
                ctx,
                obj,
                pb::guild_combat::Sub::GcDiagMessage(pb::guild_combat::GcDiagMessage {
                    text: text.clone(),
                }),
            );
        }
    }
}
