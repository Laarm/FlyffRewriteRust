//! The lord election (Plan/22 §9.1-§9.4; WP-EVENT-10): the calendar, the three phases, the deposit, pledge and vote of
//! a player, the deposit refunds and the lord's items.
//!
//! * **Calendar.** [`next_begin`] and [`property_state`] are pure; [`step`] (P4, only while the process is on) moves the
//!   election ONE phase per minute when its state differs from the calendar's (legacy `CTElection::OnTimer`), so after a
//!   long downtime every phase takes a minute and an empty catch-up election clears the lord, as the original.
//! * **Phases.** [`begin_candidacy`], [`begin_vote`] (ranks 11+ are left out and refunded 98 %), [`end_vote`] (refunds by
//!   rank, the 7 lord items by mail, the next election). Each is one batch in one commit block with the memory change, the
//!   broadcast and the audit line; the GM calls of `gm.rs` run the same functions.
//! * **Player actions.** [`add_deposit`], [`set_pledge`], [`inc_vote`] validate in the order of `slord.cpp:145-247` (the
//!   first failure sends its text and stops), then commit with a fresh snapshot (VR-018) in ONE `AtomicBatch`.
//!
//! The refund formula is the f32 quirk of `tlord.cpp:363` ([`refund`]); the money moves as mail ([`pay_out`]).

use super::{
    Candidate, LEVEL_REQUIREMENT, LORD_ITEM_KEEP_S, MAX_CANDIDATES, MAX_PLEDGE_LEN, MIN_DEPOSIT,
    MIN_DEPOSIT_STEP, NO_LORD, SET_PLEDGE_COST,
};
use crate::chat::ingress::notice;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::econ::mail::MailEntry;
use crate::econ::mail::commit::{can_snapshot, snapshot_for_batch};
use crate::event::payout::{fresh_serial, pay_out};
use crate::gm::text::format_positional;
use crate::item::model::Item;
use crate::outbox::Audience;
use crate::sched::wall::{WallSpec, WallZone};
use crate::social::party::now_secs;
use flyff_common::defs::item::II_SYS_SYS_VOTE_THANKS;
use flyff_common::defs::text::{
    TID_GAME_ELECTION_ADD_DEPOSIT_E001, TID_GAME_ELECTION_ADD_DEPOSIT_E002,
    TID_GAME_ELECTION_ADD_DEPOSIT_E003, TID_GAME_ELECTION_ADD_DEPOSIT_E004,
    TID_GAME_ELECTION_ADD_DEPOSIT_E005, TID_GAME_ELECTION_INC_VOTE_E001,
    TID_GAME_ELECTION_INC_VOTE_E002, TID_GAME_ELECTION_INC_VOTE_E003,
    TID_GAME_ELECTION_INC_VOTE_E004, TID_GAME_ELECTION_RETURN_DEPOSIT_MAIL_TEXT_01,
    TID_GAME_ELECTION_RETURN_DEPOSIT_MAIL_TEXT_02, TID_GAME_ELECTION_RETURN_DEPOSIT_MAIL_TITLE_01,
    TID_GAME_ELECTION_RETURN_DEPOSIT_MAIL_TITLE_02, TID_GAME_ELECTION_SET_LORD_MAIL_TEXT,
    TID_GAME_ELECTION_SET_LORD_MAIL_TITLE, TID_GAME_ELECTION_SET_PLEDGE_E001,
    TID_GAME_ELECTION_SET_PLEDGE_E002, TID_GAME_ELECTION_SET_PLEDGE_E003,
};
use flyff_common::{CharacterId, MailId, ObjId, UnixTime};
use flyff_data::social::lord::ElectionTable;
use flyff_db::audit::content as audit;
use flyff_db::{
    AtomicBatch, DbOp, ElectionState, LordCandidateRecord, LordElectionRecord, LordVoteRecord,
};
use flyff_proto::pb;
use flyff_rules::jobs::{is_hero, is_master};

// ---- the calendar (pure) --------------------------------------------------------------------------------------------

/// `true` when `election.inc` gives a usable calendar: a weekday in 1..=7 (1 = Sunday), an hour in 0..=23 and a
/// positive candidacy period, vote period and interval.
pub fn calendar_valid(t: &ElectionTable) -> bool {
    (1..=7).contains(&t.day_of_week)
        && (0..=23).contains(&t.hour)
        && t.candidacy_s > 0
        && t.vote_s > 0
        && t.days > 0
}

/// `IElection::GetNextBegin` (22 §9.2). `begin == 0` (the first election): the next local `day_of_week` / `hour`:00
/// strictly after `now` in the server zone (DST by the zone's rules, 10 §5.5); otherwise `begin + days x 86 400 s`
/// (`CTimeSpan`, fixed seconds). An invalid calendar gives 0 (never scheduled).
pub fn next_begin(t: &ElectionTable, begin: UnixTime, now: UnixTime, zone: &WallZone) -> UnixTime {
    if !calendar_valid(t) {
        return UnixTime(0);
    }
    if begin.0 != 0 {
        return UnixTime(
            begin
                .0
                .saturating_add(i64::from(t.days).saturating_mul(86_400)),
        );
    }
    let (Ok(weekday), Ok(hour)) = (u8::try_from(t.day_of_week - 1), u8::try_from(t.hour)) else {
        return UnixTime(0);
    };
    zone.next_occurrence(
        &WallSpec::Weekly {
            weekday,
            hour,
            minute: 0,
        },
        now.to_unix_ms(),
    )
    .map_or(UnixTime(0), |at| at.to_unix_time())
}

/// `IElection::GetPropertyState` (22 §9.2): what the calendar says the state is at `now`. `end_c = begin + candidacy_s`,
/// `end_v = begin + candidacy_s + vote_s`: Ready before the begin (and for `begin == 0`), Candidacy in
/// `[begin, end_c)`, Vote in `[end_c, end_v)`, Expired after `end_v`; exactly `now == end_v` is Ready (the harmless
/// edge of the original, KEEP).
pub fn property_state(t: &ElectionTable, begin: UnixTime, now: UnixTime) -> ElectionState {
    if begin.0 == 0 {
        return ElectionState::Ready;
    }
    let end_c = begin.0.saturating_add(t.candidacy_s);
    let end_v = end_c.saturating_add(t.vote_s);
    if begin.0 <= now.0 && now.0 < end_c {
        ElectionState::Candidacy
    } else if end_c <= now.0 && now.0 < end_v {
        ElectionState::Vote
    } else if now.0 > end_v {
        ElectionState::Expired
    } else {
        ElectionState::Ready
    }
}

/// `DepositCompare`: the higher deposit first, equal deposits the earlier `created_at`. Stable.
pub fn sort_by_deposit(v: &mut [Candidate]) {
    v.sort_by(|a, b| {
        b.deposit
            .cmp(&a.deposit)
            .then(a.created_at.cmp(&b.created_at))
    });
}

/// `VoteCompare`: more votes first, equal votes the earlier `created_at` (KEEP: with no vote at all the EARLIEST
/// candidacy wins). A stable sort (FIX: `std::sort` was unstable on complete ties), so complete ties keep the order of
/// the list.
pub fn sort_by_votes(v: &mut [Candidate]) {
    v.sort_by(|a, b| b.votes.cmp(&a.votes).then(a.created_at.cmp(&b.created_at)));
}

/// The deposit refund of `tlord.cpp:363` (KEEP-QUIRK f32): `(deposit / 100) as f32 * (rate * 100.0f32)`, truncated.
/// 123 456 789 at 0.98 gives 120 987 568; 1 000 000 000 at 0.6 gives 600 000 064.
pub fn refund(deposit: i64, rate: f32) -> i64 {
    ((deposit / 100) as f32 * (rate * 100.0_f32)) as i64
}

/// The rate printed in the mails (`%3.0f` of `rate * 100`).
pub fn percent_text(rate: f32) -> String {
    format!("{:3.0}", rate * 100.0_f32)
}

/// The refund rate of the candidate at 0-based `order` of the final order: `rates[min(order, 10)]` (`GetRetRate`), 0
/// for a missing entry.
pub fn rate_at(rates: &[f32], order: usize) -> f32 {
    rates.get(order.min(MAX_CANDIDATES)).copied().unwrap_or(0.0)
}

/// `pledge` cut to [`MAX_PLEDGE_LEN`] bytes at a char boundary.
pub fn clip_pledge(pledge: &str) -> String {
    if pledge.len() <= MAX_PLEDGE_LEN {
        return pledge.to_owned();
    }
    let mut end = MAX_PLEDGE_LEN;
    while end > 0 && !pledge.is_char_boundary(end) {
        end -= 1;
    }
    pledge[..end].to_owned()
}

/// The row of a candidate.
pub(crate) fn candidate_record(election: u32, c: &Candidate) -> LordCandidateRecord {
    LordCandidateRecord {
        election_id: election,
        character_id: c.id,
        deposit: c.deposit,
        votes: c.votes,
        pledge: c.pledge.clone(),
        left_out: false,
        created_at: c.created_at,
    }
}

fn text_of(ctx: &GameCtx<'_>, tid: u32) -> String {
    ctx.data.texts.get(tid).to_owned()
}

fn text_with(ctx: &GameCtx<'_>, tid: u32, args: &[String]) -> String {
    format_positional(ctx.data.texts.get(tid), args)
}

// ---- the minute step ----------------------------------------------------------------------------------------------------

/// The election step of P4 (`CTElection::OnTimer`): when the state differs from the calendar's, ONE phase function
/// runs (Ready: begin the candidacy, Candidacy: begin the vote, Vote: end the vote). Nothing runs for an election that
/// is never scheduled.
pub fn step(s: &mut Services, ctx: &mut GameCtx<'_>) {
    let now = now_secs(ctx);
    let lord = &s.event.lord;
    if !lord.scheduled {
        return;
    }
    let want = property_state(&lord.table, lord.election.begin_at, now);
    if want == lord.election.state {
        return;
    }
    match lord.election.state {
        ElectionState::Ready => begin_candidacy(s, ctx),
        ElectionState::Candidacy => begin_vote(s, ctx),
        ElectionState::Vote => end_vote(s, ctx),
        ElectionState::Expired => {}
    }
}

// ---- the phases --------------------------------------------------------------------------------------------------------

/// `begin_candidacy` (22 §9.3 step 1): the state is Candidacy (`UpsertLordElection` behind the pending row), and
/// `ElectionBeginCandidacy` goes to everybody.
pub fn begin_candidacy(s: &mut Services, ctx: &mut GameCtx<'_>) {
    s.event.lord.election.state = ElectionState::Candidacy;
    let rec = s.event.lord.election.record();
    super::send_ops(s, ctx, "lord_phase", vec![DbOp::UpsertLordElection(rec)]);
    ctx.broadcast(
        Some(&*s),
        ObjId::NULL,
        Audience::AllOnline,
        pb::ElectionBeginCandidacy {},
    );
}

/// One refund mail of the candidate at 0-based `order`: nothing for a rate of 0. Orders below 10 get
/// `..._MAIL_TITLE_02` (deposit, rank, rate %, refund), the others `..._01` (deposit, rate %, refund).
fn refund_mails(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    c: &Candidate,
    order: usize,
    rates: &[f32],
) -> Vec<MailId> {
    let rate = rate_at(rates, order);
    if rate == 0.0 {
        return Vec::new();
    }
    let amount = refund(c.deposit, rate);
    let percent = percent_text(rate);
    let (title, body) = if order < MAX_CANDIDATES {
        (
            text_of(ctx, TID_GAME_ELECTION_RETURN_DEPOSIT_MAIL_TITLE_02),
            text_with(
                ctx,
                TID_GAME_ELECTION_RETURN_DEPOSIT_MAIL_TEXT_02,
                &[
                    c.deposit.to_string(),
                    (order + 1).to_string(),
                    percent,
                    amount.to_string(),
                ],
            ),
        )
    } else {
        (
            text_of(ctx, TID_GAME_ELECTION_RETURN_DEPOSIT_MAIL_TITLE_01),
            text_with(
                ctx,
                TID_GAME_ELECTION_RETURN_DEPOSIT_MAIL_TEXT_01,
                &[c.deposit.to_string(), percent, amount.to_string()],
            ),
        )
    };
    pay_out(s, ctx, c.id, &title, &body, amount)
}

/// The records of the mails `ids` as the batch constructors want them.
fn mail_records(s: &Services, ids: &[MailId]) -> Vec<flyff_db::MailRecord> {
    ids.iter()
        .filter_map(|id| s.econ.mail.get(*id).map(MailEntry::to_record))
        .collect()
}

/// `begin_vote` (22 §9.3 step 2): the requirement is `(0 x requirement_factor) = 0` (KQ-25: the external statistics job
/// never ran); the candidates are sorted by deposit; ranks 11 and up are left out, refunded `rates[10]` (98 %) by mail and
/// removed. ONE `lord_begin_vote` batch; `ElectionBeginVote{requirement}` goes to everybody.
pub fn begin_vote(s: &mut Services, ctx: &mut GameCtx<'_>) {
    let rates = s.event.lord.table.rates.clone();
    let (left, election) = {
        let el = &mut s.event.lord.election;
        sort_by_deposit(&mut el.candidates);
        el.requirement = 0;
        el.state = ElectionState::Vote;
        let left = if el.candidates.len() > MAX_CANDIDATES {
            el.candidates.split_off(MAX_CANDIDATES)
        } else {
            Vec::new()
        };
        (left, el.record())
    };
    let mut mails: Vec<MailId> = Vec::new();
    let mut rows = Vec::with_capacity(left.len());
    for (i, c) in left.iter().enumerate() {
        mails.extend(refund_mails(s, ctx, c, MAX_CANDIDATES + i, &rates));
        let mut row = candidate_record(election.id, c);
        row.left_out = true;
        rows.push(row);
    }
    let batch = AtomicBatch::lord_begin_vote(election.clone(), rows, mail_records(s, &mails));
    super::send(s, ctx, "lord_begin_vote", batch, mails);
    ctx.broadcast(
        Some(&*s),
        ObjId::NULL,
        Audience::AllOnline,
        pb::ElectionBeginVote {
            requirement: election.requirement,
        },
    );
}

/// The 7 items of the winner's sex by mail, each with the 14-day keep time (`tlord.cpp:492-531`). An item the data
/// does not know is skipped with a warning.
fn lord_item_mails(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    winner: CharacterId,
    now: UnixTime,
) -> Vec<MailId> {
    // legacy: the sex of the player data, male when unknown
    let sex = s
        .presence
        .get(winner)
        .map_or(0_usize, |e| usize::from(e.sex.min(1)));
    let items: Vec<u32> = s
        .event
        .lord
        .table
        .items
        .get(sex)
        .cloned()
        .unwrap_or_default();
    let title = text_of(ctx, TID_GAME_ELECTION_SET_LORD_MAIL_TITLE);
    let body = text_of(ctx, TID_GAME_ELECTION_SET_LORD_MAIL_TEXT);
    let mut mails = Vec::with_capacity(items.len());
    for id in items {
        if ctx.data.items.get(id).is_none() {
            tracing::warn!(
                item = id,
                "a lord item of election.inc is not in the item data: skipped"
            );
            continue;
        }
        let Some(serial) = fresh_serial(s, ctx) else {
            continue;
        };
        let mut item = Item::new(id, 1);
        item.serial = serial;
        item.keep_until = now.0.saturating_add(LORD_ITEM_KEEP_S);
        let (mail, _) = s
            .econ
            .mail
            .send_system_with_id(winner, &title, &body, Some(item), 0);
        mails.push(mail);
    }
    mails
}

/// `end_vote` (22 §9.3 step 3): the candidates are sorted by votes (ties the earlier `created_at`); the winner is the first
/// one when the list is not empty and the votes reach the requirement (always, with 0), else nobody. Every candidate of
/// rank r gets `rates[min(r - 1, 10)]` of his deposit by mail (no mail for a rate of 0), the winner his 7 items. ONE
/// `LordElectionEnd` batch (the election Expired with the winner, the next election Ready, every skill cooldown 0); then
/// the memory (`lord = winner`, an election without a winner CLEARS the lord, as `Set(NULL_ID)`), the player data of the
/// winner and `ElectionEndVote` to everybody, the honor title recheck and the audit line.
pub fn end_vote(s: &mut Services, ctx: &mut GameCtx<'_>) {
    let now = now_secs(ctx);
    let rates = s.event.lord.table.rates.clone();
    let (candidates, winner, votes, ended, previous_begin) = {
        let el = &mut s.event.lord.election;
        sort_by_votes(&mut el.candidates);
        let total: i64 = el.candidates.iter().map(|c| i64::from(c.votes)).sum();
        let winner = el
            .candidates
            .first()
            .filter(|_| total >= i64::from(el.requirement))
            .map(|c| (c.id, c.votes));
        let mut ended = el.record();
        ended.lord_id = winner.map(|w| w.0);
        ended.ended_at = Some(now);
        (
            el.candidates.clone(),
            winner.map(|w| w.0),
            winner.map_or(0, |w| w.1),
            ended,
            el.begin_at,
        )
    };
    let mut mails: Vec<MailId> = Vec::new();
    for (order, c) in candidates.iter().enumerate() {
        mails.extend(refund_mails(s, ctx, c, order, &rates));
    }
    if let Some(w) = winner {
        mails.extend(lord_item_mails(s, ctx, w, now));
    }
    let next_id = ended.id.saturating_add(1);
    let next_at = next_begin(&s.event.lord.table, previous_begin, now, ctx.sched.zone());
    let next = LordElectionRecord::ready(next_id, next_at);
    let skills = s.event.lord.skill_order.clone();
    let batch = AtomicBatch::lord_election_end(ended, mail_records(s, &mails), skills, next);
    super::send(s, ctx, "LordElectionEnd", batch, mails);

    // ---- the memory, in the same block
    {
        let lord = &mut s.event.lord;
        lord.lord = winner;
        for t in lord.skill_ticks.values_mut() {
            *t = 0;
        }
        let el = &mut lord.election;
        el.candidates.clear();
        el.voters.clear();
        el.id = next_id;
        el.begin_at = next_at;
        el.state = ElectionState::Ready;
        el.requirement = 0;
    }
    let ended_id = next_id.saturating_sub(1);

    // ---- the messages: the player data of the winner first (`AddQueryPlayerData`), then the end
    if let Some(w) = winner
        && let Some(ntf) = crate::social::player_data::ntf_of(s, w)
    {
        ctx.broadcast(Some(&*s), ObjId::NULL, Audience::AllOnline, ntf);
    }
    ctx.broadcast(
        Some(&*s),
        ObjId::NULL,
        Audience::AllOnline,
        pb::ElectionEndVote {
            id_lord: winner.map_or(NO_LORD, |c| c.0),
        },
    );
    // HS_LORD of the old and the new lord
    crate::social::hooks::on_lord_changed(s, ctx);
    ctx.audit.emit(audit::lord_elected(
        ended_id,
        winner,
        votes,
        u32::try_from(candidates.len()).unwrap_or(u32::MAX),
    ));
    tracing::info!(
        election = ended_id,
        lord = winner.map(|c| c.0),
        candidates = candidates.len(),
        "the lord election ended"
    );
}

// ---- the player actions -------------------------------------------------------------------------------------------------

/// `true` when a lord NPC offering `menu` stands within the NPC range of `who` (VR-012).
pub fn near_npc(ctx: &GameCtx<'_>, who: PlayerRef, menu: u32) -> bool {
    ctx.player(who).is_some_and(|m| {
        ctx.worlds
            .close_npc(menu, m.core.world, m.core.layer, m.core.pos)
            .is_some()
    })
}

fn defined(ctx: &mut GameCtx<'_>, who: PlayerRef, tid: u32) {
    notice::defined(ctx, who, tid, Vec::new());
}

fn emit_by(ctx: &GameCtx<'_>, who: PlayerRef, ev: flyff_db::AuditEvent) {
    let ev = match crate::audit::actor_of(ctx, who) {
        Some(actor) => ev.by(actor),
        None => ev,
    };
    ctx.audit.emit(ev);
}

/// `ElectionAddDepositReq{total}` (22 §9.4; `election::AddDepositRequirements`, `slord.cpp:145-170`): the NPC was checked
/// by the handler. In this order, the first failure sends its text: Master or Hero (`E001`), `total >= 100 000 000`
/// (`E002`), the candidacy phase (`E004`), `total` above the old deposit (`E003`), the raise fits an `i32` and the
/// wallet (`E002`), the raise is at least 10 000 000 (`E005`). Then the commit: the raise is taken from the wallet, the
/// candidate is created or raised, the list is sorted again, and `lord_deposit` (the snapshot AFTER the debit) goes out;
/// `ElectionAddDepositNtf{id, raise, created_at}` goes to everybody.
pub fn add_deposit(s: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, total: i64) {
    let Some(m) = ctx.player(who) else {
        return;
    };
    let Some(me) = m.player().map(|p| p.ident.character_id) else {
        return;
    };
    let rules = &s.stats.core().rules;
    if !is_master(rules, m.mv.job) && !is_hero(rules, m.mv.job) {
        return defined(ctx, who, TID_GAME_ELECTION_ADD_DEPOSIT_E001);
    }
    if total < MIN_DEPOSIT {
        return defined(ctx, who, TID_GAME_ELECTION_ADD_DEPOSIT_E002);
    }
    if s.event.lord.election.state != ElectionState::Candidacy {
        return defined(ctx, who, TID_GAME_ELECTION_ADD_DEPOSIT_E004);
    }
    let old = s.event.lord.election.candidate(me).map_or(0, |c| c.deposit);
    if total <= old {
        return defined(ctx, who, TID_GAME_ELECTION_ADD_DEPOSIT_E003);
    }
    let delta = total - old;
    let gold = crate::item::gold::core::gold(m);
    if delta > i64::from(i32::MAX) || gold < delta {
        return defined(ctx, who, TID_GAME_ELECTION_ADD_DEPOSIT_E002);
    }
    if delta < MIN_DEPOSIT_STEP {
        return defined(ctx, who, TID_GAME_ELECTION_ADD_DEPOSIT_E005);
    }
    if !can_snapshot(ctx, who) {
        tracing::debug!("a lord deposit of a player without a savable session was refused");
        return;
    }

    // ---- the commit block: nothing below can fail ----
    let now = now_secs(ctx);
    crate::item::gold::add_gold(s, ctx, who, -delta, true);
    let Some((snap, generation)) = snapshot_for_batch(s, ctx, who) else {
        // unreachable after `can_snapshot`; give the penya back rather than lose it
        crate::item::gold::add_gold(s, ctx, who, delta, true);
        tracing::error!("a lord deposit lost its snapshot after the check: undone");
        return;
    };
    let election = s.event.lord.election.id;
    let el = &mut s.event.lord.election;
    let created_at = match el.candidates.iter_mut().find(|c| c.id == me) {
        Some(c) => {
            c.deposit = total;
            c.created_at
        }
        None => {
            el.candidates.push(Candidate {
                id: me,
                deposit: total,
                votes: 0,
                pledge: String::new(),
                created_at: now,
            });
            now
        }
    };
    sort_by_deposit(&mut el.candidates);
    let row = el
        .candidate(me)
        .map(|c| candidate_record(election, c))
        .unwrap_or_else(|| LordCandidateRecord::new(election, me, total, created_at));
    super::send_player(
        s,
        ctx,
        who,
        Some(generation),
        "LordDeposit",
        AtomicBatch::lord_deposit(snap, row),
    );
    ctx.broadcast(
        Some(&*s),
        ObjId::NULL,
        Audience::AllOnline,
        pb::ElectionAddDepositNtf {
            id_player: me.0,
            deposit: delta,
            created_at: created_at.0,
        },
    );
    emit_by(ctx, who, audit::lord_deposit(election, me, delta, total));
}

/// `ElectionSetPledgeReq{pledge}` (22 §9.4; `SetPledgeRequirements`, `slord.cpp:172-196`): an empty pledge is ignored
/// before anything (legacy), one over 255 bytes is cut at a char boundary. A candidate of the list (`E001`), the vote phase
/// (`E003`); a candidate with a pledge already pays 1 000 000 (`E002` when the wallet is short; FIX of nothing: the first
/// pledge is free). `lord_pledge` goes out (with the snapshot after the debit for a re-pledge) and
/// `ElectionSetPledgeNtf` to everybody. No NPC (22 §2.6).
pub fn set_pledge(s: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, pledge: &str) {
    if pledge.is_empty() {
        return;
    }
    let pledge = clip_pledge(pledge);
    let Some(m) = ctx.player(who) else {
        return;
    };
    let Some(me) = m.player().map(|p| p.ident.character_id) else {
        return;
    };
    let Some(existing) = s.event.lord.election.candidate(me) else {
        return defined(ctx, who, TID_GAME_ELECTION_SET_PLEDGE_E001);
    };
    if s.event.lord.election.state != ElectionState::Vote {
        return defined(ctx, who, TID_GAME_ELECTION_SET_PLEDGE_E003);
    }
    let paid = !existing.pledge.is_empty();
    if paid && crate::item::gold::core::gold(m) < SET_PLEDGE_COST {
        return defined(ctx, who, TID_GAME_ELECTION_SET_PLEDGE_E002);
    }
    if paid && !can_snapshot(ctx, who) {
        tracing::debug!("a lord re-pledge of a player without a savable session was refused");
        return;
    }

    // ---- the commit block ----
    let snapshot = if paid {
        crate::item::gold::add_gold(s, ctx, who, -SET_PLEDGE_COST, true);
        match snapshot_for_batch(s, ctx, who) {
            Some(x) => Some(x),
            None => {
                crate::item::gold::add_gold(s, ctx, who, SET_PLEDGE_COST, true);
                tracing::error!("a lord re-pledge lost its snapshot after the check: undone");
                return;
            }
        }
    } else {
        None
    };
    let election = s.event.lord.election.id;
    let Some(c) = s
        .event
        .lord
        .election
        .candidates
        .iter_mut()
        .find(|c| c.id == me)
    else {
        return; // unreachable: the candidate was found above
    };
    c.pledge.clone_from(&pledge);
    let row = candidate_record(election, c);
    let (snap, generation) = match snapshot {
        Some((snap, generation)) => (Some(snap), Some(generation)),
        None => (None, None),
    };
    super::send_player(
        s,
        ctx,
        who,
        generation,
        "LordPledge",
        AtomicBatch::lord_pledge(snap, row),
    );
    ctx.broadcast(
        Some(&*s),
        ObjId::NULL,
        Audience::AllOnline,
        pb::ElectionSetPledgeNtf {
            id_player: me.0,
            pledge,
        },
    );
    emit_by(
        ctx,
        who,
        audit::lord_pledge(election, me, if paid { SET_PLEDGE_COST } else { 0 }),
    );
}

/// `ElectionIncVoteReq{candidate}` (22 §9.4; `IncVoteRequirements`, `slord.cpp:198-216`): the candidate is in the list
/// (`E001`), the vote phase (`E002`), the voter's level is at least 60 (`E003`, the level only, as the code), he did not
/// vote in this election (`E004`). Then the commit: the candidate's votes + 1 (the list is not sorted again, as the
/// legacy mirror), the voter is recorded, he gets the `II_SYS_SYS_VOTE_THANKS` buff, and `lord_vote` (the snapshot after
/// the buff) goes out; `ElectionIncVoteNtf{candidate, elector}` to everybody. A self-vote is allowed (KEEP).
pub fn inc_vote(s: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, candidate: u32) {
    let Some(m) = ctx.player(who) else {
        return;
    };
    let Some(me) = m.player().map(|p| p.ident.character_id) else {
        return;
    };
    let cand = CharacterId(candidate);
    if s.event.lord.election.candidate(cand).is_none() {
        return defined(ctx, who, TID_GAME_ELECTION_INC_VOTE_E001);
    }
    if s.event.lord.election.state != ElectionState::Vote {
        return defined(ctx, who, TID_GAME_ELECTION_INC_VOTE_E002);
    }
    if m.mv.level < LEVEL_REQUIREMENT {
        return defined(ctx, who, TID_GAME_ELECTION_INC_VOTE_E003);
    }
    if s.event.lord.election.voters.contains(&me) {
        return defined(ctx, who, TID_GAME_ELECTION_INC_VOTE_E004);
    }
    if !can_snapshot(ctx, who) {
        tracing::debug!("a lord vote of a player without a savable session was refused");
        return;
    }

    // ---- the commit block ----
    let now = now_secs(ctx);
    if let Err(e) = crate::buff::apply_item(s, ctx, who.0, II_SYS_SYS_VOTE_THANKS, None) {
        tracing::warn!(error = ?e, "the vote thanks buff was refused");
    }
    let Some((snap, generation)) = snapshot_for_batch(s, ctx, who) else {
        tracing::error!("a lord vote lost its snapshot after the check: not counted");
        return;
    };
    let election = s.event.lord.election.id;
    let el = &mut s.event.lord.election;
    el.voters.insert(me);
    let Some(c) = el.candidates.iter_mut().find(|c| c.id == cand) else {
        return; // unreachable: the candidate was found above
    };
    c.votes = c.votes.saturating_add(1);
    let votes = c.votes;
    let row = candidate_record(election, c);
    let vote = LordVoteRecord {
        election_id: election,
        elector_id: me,
        voted_at: now,
    };
    super::send_player(
        s,
        ctx,
        who,
        Some(generation),
        "LordVote",
        AtomicBatch::lord_vote(snap, vote, row),
    );
    ctx.broadcast(
        Some(&*s),
        ObjId::NULL,
        Audience::AllOnline,
        pb::ElectionIncVoteNtf {
            id_candidate: cand.0,
            id_elector: me.0,
        },
    );
    emit_by(ctx, who, audit::lord_vote(election, cand, me, votes));
}
