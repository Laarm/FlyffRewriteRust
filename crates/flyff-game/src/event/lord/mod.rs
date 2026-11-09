//! `event::lord`: the lord election, the lord events and the lord skills (Plan/22 §9; WP-EVENT-10, WP-EVENT-11).
//!
//! | file | what |
//! |---|---|
//! | `mod.rs` | [`LordService`] (the whole state), [`LordService::load`], P4 [`minute`], the queries other tasks ask |
//! | `election.rs` | the calendar ([`election::next_begin`], [`election::property_state`]), the phases, deposits, pledges, votes, refunds and the lord items |
//! | `levent.rs` | the lord events: creation, the factors, the minute tick |
//! | `skill.rs` | the lord skills, the cooldowns, [`skill::is_teleportable`] |
//! | `handlers.rs` | the five client messages |
//! | `join.rs` | [`join::LordStep`], the `Lord` message of every join (position 15) |
//! | `gm.rs` | the GM / admin calls (forward-only phase steps, the process switch, `state_line`, `to_json`) |
//!
//! **State.** Everything is memory owned by the game thread; the database holds the same facts in the rows of
//! `0006_realm.sql` (`flyff-db` "Content command API"). The service is built from `StartupState.lord` by
//! [`LordService::load`] in `Game::new` (no batch at boot: the row of a freshly prepared election rides at the FRONT of
//! the first lord write, [`LordService::row_ops`]).
//!
//! **Time.** P4 [`minute`] runs every 900 ticks in the legacy order of `CTLord::OnTimer`: the election step (only
//! while [`LordService::enabled`]), the lord-event ticks, the lord-skill ticks. All times are `now_secs(ctx)` (never the
//! system clock); `next_begin` goes through the scheduler's [`WallZone`].
//!
//! **Commit blocks.** Every deposit, pledge, vote, event and phase is validated first; then ONE block changes the
//! memory, builds the `AtomicBatch` with a fresh snapshot (VR-018) and sends the messages and the audit line. A batch
//! that answers `Failed` is a bug: its mails are removed again from the mail service, the rest of the memory is kept
//! and a CRITICAL line names the batch (21 §2.4).
//!
//! **Effects owned elsewhere** (read through their own seams, not duplicated here): the SHOUT colour
//! (`chat/route/shout.rs`), the RAGE mute (`chat/ingress/mode.rs`), the party skills x4 of a lord-led party
//! (`social/party/skills.rs`), CHEERING x1.1 and the vote thanks x1.05 (`stats/progression.rs`, `item/drop.rs`).

pub mod election;
pub mod gm;
pub mod handlers;
pub mod join;
pub mod levent;
pub mod skill;

use crate::ctx::{GameCtx, Services};
use crate::sched::wall::WallZone;
use crate::tickets::{Pending, ServiceTicket, TicketResult};
use flyff_common::{CharacterId, UnixMs, UnixTime};
use flyff_data::social::lord::{ElectionTable, LordTables};
use flyff_db::repo::meta::kv;
use flyff_db::{
    AtomicBatch, AtomicKind, DbOp, ElectionState, LordElectionRecord, LordEventRecord, StartupState,
};
use std::collections::{BTreeMap, BTreeSet};

/// `CCandidate::nMaxPledgeLen`: the longest pledge in bytes.
pub const MAX_PLEDGE_LEN: usize = 255;
/// `CCandidate::nMinDeposit`: the smallest total deposit in penya.
pub const MIN_DEPOSIT: i64 = 100_000_000;
/// The smallest raise of a deposit in penya (`slord.cpp:158`).
pub const MIN_DEPOSIT_STEP: i64 = 10_000_000;
/// `CCandidate::nSetPledgeCost`: what a second pledge costs in penya.
pub const SET_PLEDGE_COST: i64 = 1_000_000;
/// `IElection::nMaxCandidates`: the candidates that stay when the vote starts.
pub const MAX_CANDIDATES: usize = 10;
/// `IElection::nLevelRequirements`: the lowest level that may vote.
pub const LEVEL_REQUIREMENT: i32 = 60;
/// `ILordEvent::eTick`: the minutes a lord event runs.
pub const LEVENT_TICKS: i32 = 60;
/// How long the lord's items may stay in the mailbox: 14 days (`tlord.cpp:515`).
pub const LORD_ITEM_KEEP_S: i64 = 1_209_600;
/// `NULL_ID` on the wire: no lord.
pub const NO_LORD: u32 = u32::MAX;

/// One candidate (`CCandidate`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    /// The character.
    pub id: CharacterId,
    /// The deposit in penya; only grows.
    pub deposit: i64,
    /// The votes.
    pub votes: i32,
    /// The pledge (empty = none yet).
    pub pledge: String,
    /// When the candidacy was created (`tCreate`): the tie-break of equal deposits and votes.
    pub created_at: UnixTime,
}

/// The election (`IElection`): its phase, its calendar position and its candidates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Election {
    /// `idElection`.
    pub id: u32,
    /// The phase.
    pub state: ElectionState,
    /// When the candidacy starts; `UnixTime(0)` = not scheduled.
    pub begin_at: UnixTime,
    /// The votes needed to win (always 0: KQ-25, the external statistics job never ran).
    pub requirement: i32,
    /// The candidates: by deposit (descending, ties the earlier `created_at`) until the vote starts, then in the
    /// order of the vote start (votes only change a count, the list is sorted again when the vote ends).
    pub candidates: Vec<Candidate>,
    /// The electors who voted in this election.
    pub voters: BTreeSet<CharacterId>,
}

impl Election {
    fn blank(id: u32, begin_at: UnixTime) -> Election {
        Election {
            id,
            state: ElectionState::Ready,
            begin_at,
            requirement: 0,
            candidates: Vec::new(),
            voters: BTreeSet::new(),
        }
    }

    /// The candidate record of `c`.
    pub fn candidate(&self, c: CharacterId) -> Option<&Candidate> {
        self.candidates.iter().find(|x| x.id == c)
    }

    /// The row of the election for the database (no winner yet).
    pub fn record(&self) -> LordElectionRecord {
        LordElectionRecord {
            id: self.id,
            state: self.state,
            begin_at: self.begin_at,
            requirement: self.requirement,
            lord_id: None,
            ended_at: None,
        }
    }
}

/// One running lord event (`CLEComponent`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LordEvent {
    /// The creator.
    pub creator: CharacterId,
    /// Minutes left.
    pub ticks: i32,
    /// The EXP factor added to 1.0.
    pub exp_factor: f32,
    /// The drop factor added to 1.0.
    pub item_factor: f32,
    /// When it was created.
    pub created_at: UnixTime,
}

/// The lord service (`Services.event.lord`, 22 §9): the lord, the election, the lord events and the skill cooldowns.
#[derive(Debug)]
pub struct LordService {
    /// `server_kv lord.election_enabled`: the election step runs (the legacy `m_bRun`); events and skills work
    /// regardless.
    pub(crate) enabled: bool,
    /// The current lord.
    pub(crate) lord: Option<CharacterId>,
    /// The election.
    pub(crate) election: Election,
    /// The running lord events in creation order (the order of the f32 sums).
    pub(crate) events: Vec<LordEvent>,
    /// The cooldown minutes of every skill of the table (0 = ready).
    pub(crate) skill_ticks: BTreeMap<u32, i32>,
    /// The ids of the skills in table order (the order of `LordSkillTick` and `Lord.skill_ticks`).
    pub(crate) skill_order: Vec<u32>,
    /// The row of the prepared election is not known to exist in the database ([`LordService::row_ops`]).
    pub(crate) row_pending: bool,
    /// `election.inc`.
    pub(crate) table: ElectionTable,
    /// The calendar of `table` is usable ([`election::calendar_valid`]).
    pub(crate) scheduled: bool,
}

impl Default for LordService {
    /// An empty service: the process on, no lord, an unscheduled election, no calendar.
    fn default() -> Self {
        LordService {
            enabled: true,
            lord: None,
            election: Election::blank(1, UnixTime(0)),
            events: Vec::new(),
            skill_ticks: BTreeMap::new(),
            skill_order: Vec::new(),
            row_pending: true,
            table: ElectionTable::default(),
            scheduled: false,
        }
    }
}

impl LordService {
    /// Builds the service from the start-up state (22 §9.1; `CTLord::Restore*`).
    ///
    /// * `enabled` = `server_kv[lord.election_enabled]` as a JSON bool (missing or anything else = true),
    /// * the lord, the events (all rows, a row at 0 minutes is deleted by the first tick) and the cooldowns of the
    ///   skills of `tables` come from `startup.lord`,
    /// * the newest election is reused while it is Ready, Candidacy or Vote with a begin time (the candidates without
    ///   the left-out rows, sorted by votes in Vote and else by deposit; the voters), otherwise the next one is
    ///   prepared: id `max(counter, newest + 1)`, Ready, `begin_at = next_begin(newest.begin_at or 0)`, and its row
    ///   rides in the first lord write ([`LordService::row_ops`]).
    ///
    /// An invalid calendar (no `election.inc`, a weekday outside 1..=7, an hour outside 0..=23, a period or an interval
    /// that is not positive) leaves `begin_at` at 0: the election is never scheduled. A table that was loaded but is
    /// invalid warns once; the empty default table (no `election.inc` at all, the data of almost every test) is silent.
    pub fn load(
        startup: &StartupState,
        tables: &LordTables,
        now: UnixMs,
        zone: &WallZone,
    ) -> LordService {
        let table = tables.election.clone();
        let scheduled = election::calendar_valid(&table);
        if !scheduled && table != ElectionTable::default() {
            tracing::warn!(
                day_of_week = table.day_of_week,
                hour = table.hour,
                candidacy_s = table.candidacy_s,
                vote_s = table.vote_s,
                days = table.days,
                "election.inc is not a usable calendar: the lord election is never scheduled"
            );
        }
        let enabled = startup
            .server_kv
            .get(kv::LORD_ELECTION_ENABLED)
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true);
        let load = &startup.lord;
        let now_s = now.to_unix_time();

        let skill_order: Vec<u32> = tables.skills.iter().map(|s| s.id).collect();
        let mut skill_ticks: BTreeMap<u32, i32> = skill_order.iter().map(|id| (*id, 0)).collect();
        for (skill, minutes) in &load.skill_cooldowns {
            if let Some(t) = skill_ticks.get_mut(skill) {
                *t = (*minutes).max(0);
            }
        }
        let events = load
            .events
            .iter()
            .map(|e: &LordEventRecord| LordEvent {
                creator: e.character_id,
                ticks: e.ticks_left.max(0),
                exp_factor: e.exp_factor,
                item_factor: e.item_factor,
                created_at: e.created_at,
            })
            .collect();

        let reusable = load.election.as_ref().filter(|e| {
            matches!(
                e.state,
                ElectionState::Ready | ElectionState::Candidacy | ElectionState::Vote
            ) && (e.begin_at.0 > 0 || !scheduled)
        });
        let (election, row_pending) = match reusable {
            Some(rec) => {
                let mut candidates: Vec<Candidate> = load
                    .candidates
                    .iter()
                    .filter(|c| !c.left_out && c.election_id == rec.id)
                    .map(|c| Candidate {
                        id: c.character_id,
                        deposit: c.deposit,
                        votes: c.votes,
                        pledge: c.pledge.clone(),
                        created_at: c.created_at,
                    })
                    .collect();
                if rec.state == ElectionState::Vote {
                    election::sort_by_votes(&mut candidates);
                } else {
                    election::sort_by_deposit(&mut candidates);
                }
                let election = Election {
                    id: rec.id,
                    state: rec.state,
                    begin_at: rec.begin_at,
                    requirement: rec.requirement,
                    candidates,
                    voters: load.voters.iter().copied().collect(),
                };
                (election, false)
            }
            None => {
                let newest = load.election.as_ref();
                let id = startup
                    .counters
                    .next_lord_election_id
                    .max(newest.map_or(1, |e| e.id.saturating_add(1)));
                let begin = election::next_begin(
                    &table,
                    newest.map_or(UnixTime(0), |e| e.begin_at),
                    now_s,
                    zone,
                );
                (Election::blank(id, begin), true)
            }
        };
        LordService {
            enabled,
            lord: load.lord,
            election,
            events,
            skill_ticks,
            skill_order,
            row_pending,
            table,
            scheduled,
        }
    }

    // ---- queries ------------------------------------------------------------------------------------------------

    /// `true` while the election step of P4 runs (`server_kv lord.election_enabled`).
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// The current lord (`CSLord::Get`).
    pub fn lord(&self) -> Option<CharacterId> {
        self.lord
    }

    /// `CSLord::IsLord( idPlayer )`: the honor title `HS_LORD`, the lord's party skills and the lord events read it.
    pub fn is_lord(&self, c: CharacterId) -> bool {
        self.lord == Some(c)
    }

    /// The phase of the election.
    pub fn state(&self) -> ElectionState {
        self.election.state
    }

    /// `true` while the voters decide (tax-tower skips its weekly change then).
    pub fn in_vote(&self) -> bool {
        self.election.state == ElectionState::Vote
    }

    /// When the candidacy of the prepared election starts (0 = not scheduled).
    pub fn begin_at(&self) -> UnixTime {
        self.election.begin_at
    }

    /// The election (read access).
    pub fn election(&self) -> &Election {
        &self.election
    }

    /// The running lord events in creation order.
    pub fn events(&self) -> &[LordEvent] {
        &self.events
    }

    /// The cooldown minutes left of `skill` (0 = ready or unknown).
    pub fn skill_tick(&self, skill: u32) -> i32 {
        self.skill_ticks.get(&skill).copied().unwrap_or(0)
    }

    /// The factors of the running lord events: `(lord_exp, lord_item)` = `(1 + sum fE, 1 + sum fI)`, summed in list order
    /// as f32 (`ILordEvent::GetEFactor()` / `GetIFactor()`).
    pub fn factors(&self) -> (f32, f32) {
        levent::factors_of(&self.events)
    }

    // ---- the row of the prepared election -------------------------------------------------------------------------

    /// The ops that create the row of the prepared election (`UpsertLordElection` Ready and
    /// `RaiseCounter(next_lord_election_id)`); empty once they were taken. The first lord write takes them at the front
    /// of its own batch: a fresh database gets the row with the first write instead of a batch of its own at boot (the
    /// siege lesson: a boot batch shows up in every database test). They are idempotent; a `Failed` batch that carried
    /// them gives them back.
    pub fn row_ops(&mut self) -> Vec<DbOp> {
        if !self.row_pending {
            return Vec::new();
        }
        self.row_pending = false;
        let ready = LordElectionRecord::ready(self.election.id, self.election.begin_at);
        DbOp::lord_election_create_ops(ready).into_iter().collect()
    }

    /// `true` while the row of the prepared election is not known to exist in the database.
    pub fn row_pending(&self) -> bool {
        self.row_pending
    }
}

/// P4 `event::lord::minute` (22 §2.3, §9.2; legacy `CTLord::OnTimer`, src:databaseserver/tlord.cpp:150-158): the election
/// step ONLY while the process is on, then the lord-event ticks, then the lord-skill ticks.
pub fn minute(s: &mut Services, ctx: &mut GameCtx<'_>) {
    if s.event.lord.enabled {
        election::step(s, ctx);
    }
    levent::minute(s, ctx);
    skill::minute(s, ctx);
}

/// Sends `batch` for the lord (an actor-less write: a phase, the end of the election) with a ticket. The pending
/// election row rides at the front. `mails` are removed from the mail service again when the batch fails.
pub(crate) fn send(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    name: &'static str,
    mut batch: AtomicBatch,
    mails: Vec<flyff_common::MailId>,
) {
    let rows = s.event.lord.row_ops();
    let carried = !rows.is_empty();
    batch.ops.splice(0..0, rows);
    let ticket = ctx.tickets.issue(Pending::Service(ServiceTicket::new(
        move |svc, _ctx, result| {
            on_result(svc, name, carried, &mails, result);
        },
    )));
    ctx.persist.atomic(batch.with_ticket(ticket));
}

/// Sends the batch of a player's action (deposit, pledge, vote, event) with the pending election row at its front and
/// the ticketed continuation of every lord batch (`generation` is the snapshot's: the save scheduler is told).
pub(crate) fn send_player(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: crate::ctx::PlayerRef,
    generation: Option<flyff_common::SaveGen>,
    name: &'static str,
    mut batch: AtomicBatch,
) {
    let rows = s.event.lord.row_ops();
    let carried = !rows.is_empty();
    batch.ops.splice(0..0, rows);
    crate::econ::mail::commit::submit(s, ctx, who, generation, batch, move |svc, _ctx, result| {
        on_result(svc, name, carried, &[], result);
    });
}

/// A single lord op as its own batch (the phase row of `begin_candidacy`).
pub(crate) fn send_ops(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    name: &'static str,
    ops: Vec<DbOp>,
) {
    send(
        s,
        ctx,
        name,
        AtomicBatch::of_ops(AtomicKind::Other(name), ops),
        Vec::new(),
    );
}

/// The continuation of every lord batch: a refusal is a bug (the memory is kept, 21 §2.4).
pub(crate) fn on_result(
    svc: &mut Services,
    name: &'static str,
    carried_row: bool,
    mails: &[flyff_common::MailId],
    result: TicketResult,
) {
    match result {
        TicketResult::Committed(_) => {}
        TicketResult::Failed(e) => {
            for id in mails {
                svc.econ.mail.remove(*id);
            }
            if carried_row {
                svc.event.lord.row_pending = true;
            }
            tracing::error!(
                error = %e,
                batch = name,
                "CRITICAL: a lord batch failed: the memory is kept (a bug, 21 §2.4)"
            );
        }
        other => tracing::error!(?other, batch = name, "a lord batch completed unexpectedly"),
    }
}
