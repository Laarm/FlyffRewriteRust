//! The GM and admin calls of the lord system (Plan/22 §9.8, Plan/23 rows 178-181 and 184).
//!
//! These are pure calls: `gm/cmd/content/` parses the line, checks the authority and prints; nothing here prints or
//! checks authority. The rows `/er /ead /esp /eiv /estate /lecreate /lskill` are DROP: the GM client sends the normal
//! requests, so there is nothing to write for them.
//!
//! * [`set_process`] (`/ElectionProcess 0|1`): the switch of the election step (the legacy `m_bRun`), kept in
//!   `server_kv lord.election_enabled`. It stops only the election; lord events and lord skills keep ticking.
//! * [`begin_candidacy`], [`begin_vote`], [`end_vote`] (`/ElectionBeginCandidacy`, `/ElectionBeginVote`,
//!   `/ElectionEndVote`): FORWARD ONLY (candidacy from Ready; the vote from Ready or Candidacy; the end from Ready,
//!   Candidacy or Vote: the database guard refuses a backward step), running the same phase functions as the calendar
//!   without moving `begin_at`. While the process is ON, the next minute compares the state with the calendar again and
//!   may step on (legacy), so a GM flow calls `set_process(false)` first, as the original's internal servers ran with
//!   `m_bRun` false.
//! * [`initialize_events`] (`/LEventInitialize`): clears every lord event.
//! * [`state_line`] and [`to_json`]: the one status line (`/content lord`, `/ElectionState`) and the detail.

use super::{election, levent};
use crate::ctx::{GameCtx, Services};
use crate::social::party::now_secs;
use flyff_common::UnixTime;
use flyff_db::repo::meta::kv;
use flyff_db::{DbOp, ElectionState};
use serde_json::{Value, json};

/// Why a forward-only step was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LordGmError {
    /// The election is in a state the step cannot leave (it only moves forward).
    #[error("the election is in state {0:?}")]
    WrongState(ElectionState),
}

/// The plan's name of a state (`Ready`, `Candidacy`, `Vote`, `Expired`).
pub fn state_name(state: ElectionState) -> &'static str {
    match state {
        ElectionState::Ready => "Ready",
        ElectionState::Candidacy => "Candidacy",
        ElectionState::Vote => "Vote",
        ElectionState::Expired => "Expired",
    }
}

/// `/ElectionProcess <0|1>`: switches the election step of P4 and records it in `server_kv lord.election_enabled`
/// (a JSON bool), so it survives a restart.
pub fn set_process(svc: &mut Services, ctx: &mut GameCtx<'_>, on: bool) {
    svc.event.lord.enabled = on;
    ctx.persist.op(DbOp::SetServerKv {
        key: kv::LORD_ELECTION_ENABLED.to_owned(),
        value: Value::from(on),
    });
}

/// `/ElectionBeginCandidacy`: the candidacy starts now (from Ready only).
pub fn begin_candidacy(svc: &mut Services, ctx: &mut GameCtx<'_>) -> Result<(), LordGmError> {
    match svc.event.lord.election.state {
        ElectionState::Ready => {
            election::begin_candidacy(svc, ctx);
            Ok(())
        }
        other => Err(LordGmError::WrongState(other)),
    }
}

/// `/ElectionBeginVote`: the vote starts now (from Ready or Candidacy).
pub fn begin_vote(svc: &mut Services, ctx: &mut GameCtx<'_>) -> Result<(), LordGmError> {
    match svc.event.lord.election.state {
        ElectionState::Ready | ElectionState::Candidacy => {
            election::begin_vote(svc, ctx);
            Ok(())
        }
        other => Err(LordGmError::WrongState(other)),
    }
}

/// `/ElectionEndVote`: the election ends now (from Ready, Candidacy or Vote); the winner is decided exactly as at the end
/// of the calendar.
pub fn end_vote(svc: &mut Services, ctx: &mut GameCtx<'_>) -> Result<(), LordGmError> {
    match svc.event.lord.election.state {
        ElectionState::Ready | ElectionState::Candidacy | ElectionState::Vote => {
            election::end_vote(svc, ctx);
            Ok(())
        }
        other => Err(LordGmError::WrongState(other)),
    }
}

/// `/LEventInitialize`: every lord event is cleared, the factors are 1.0 and everybody hears `LEventInitialize`.
pub fn initialize_events(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    levent::initialize(svc, ctx);
}

/// The unix second of the next step of the calendar for the current state: the begin of the candidacy (Ready), the end
/// of the candidacy (Candidacy) or the end of the vote (Vote); `None` while the election is not scheduled.
fn next_step_at(svc: &Services) -> Option<UnixTime> {
    let lord = &svc.event.lord;
    let el = &lord.election;
    if el.begin_at.0 == 0 || !lord.scheduled {
        return None;
    }
    let end_c = el.begin_at.0.saturating_add(lord.table.candidacy_s);
    let end_v = end_c.saturating_add(lord.table.vote_s);
    Some(UnixTime(match el.state {
        ElectionState::Ready | ElectionState::Expired => el.begin_at.0,
        ElectionState::Candidacy => end_c,
        ElectionState::Vote => end_v,
    }))
}

/// ONE status line in the uniform format of the content commands:
/// `lord: <Ready|Candidacy|Vote|Expired>[, <n> s left][, <c> candidates, <v> votes, <e> events][, process off]`.
/// `<n>` is the whole number of seconds to the next calendar step (0 when it is due); it is omitted while the election is
/// not scheduled (`begin_at == 0`) or the process is off. Never prints and never checks authority.
pub fn state_line(svc: &Services, ctx: &GameCtx<'_>) -> String {
    let lord = &svc.event.lord;
    let mut line = format!("lord: {}", state_name(lord.election.state));
    if lord.enabled
        && let Some(at) = next_step_at(svc)
    {
        let left = (at.0 - now_secs(ctx).0).max(0);
        line.push_str(&format!(", {left} s left"));
    }
    let votes: i64 = lord
        .election
        .candidates
        .iter()
        .map(|c| i64::from(c.votes))
        .sum();
    line.push_str(&format!(
        ", {} candidates, {votes} votes, {} events",
        lord.election.candidates.len(),
        lord.events.len()
    ));
    if !lord.enabled {
        line.push_str(", process off");
    }
    line
}

/// A unix second as `YYYY-MM-DD HH:MM:SS` in UTC.
fn utc_text(t: UnixTime) -> String {
    chrono::DateTime::from_timestamp(t.0, 0).map_or_else(
        || "-".to_owned(),
        |d| d.format("%Y-%m-%d %H:%M:%S").to_string(),
    )
}

/// A unix second as `YYYY-MM-DD HH:MM:SS` in the server zone.
fn local_text(ctx: &GameCtx<'_>, t: UnixTime) -> String {
    let l = ctx.sched.zone().local(t.to_unix_ms());
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        l.year, l.month, l.day, l.hour, l.minute, l.second
    )
}

/// The detail of the lord system as JSON (`/content lord`, the later admin view): the process flag, the lord, the election
/// (id, state, begin in the server zone and UTC, requirement, candidates with deposit / votes / pledge, the number of
/// voters), the running events and the skill cooldowns.
pub fn to_json(svc: &Services, ctx: &GameCtx<'_>) -> Value {
    let lord = &svc.event.lord;
    let el = &lord.election;
    let begin = (el.begin_at.0 != 0).then_some(el.begin_at);
    json!({
        "process": lord.enabled,
        "scheduled": lord.scheduled,
        "lord": lord.lord.map(|c| c.0),
        "election": {
            "id": el.id,
            "state": state_name(el.state),
            "state_code": el.state.code(),
            "begin_at": el.begin_at.0,
            "begin_local": begin.map(|t| local_text(ctx, t)),
            "begin_utc": begin.map(utc_text),
            "zone": ctx.sched.zone().name(),
            "requirement": el.requirement,
            "candidates": el.candidates.iter().map(|c| json!({
                "id": c.id.0,
                "deposit": c.deposit,
                "votes": c.votes,
                "pledge": c.pledge,
                "created_at": c.created_at.0,
            })).collect::<Vec<_>>(),
            "voters": el.voters.len(),
        },
        "events": lord.events.iter().map(|e| json!({
            "creator": e.creator.0,
            "ticks": e.ticks,
            "exp_factor": e.exp_factor,
            "item_factor": e.item_factor,
        })).collect::<Vec<_>>(),
        "skill_ticks": lord.skill_order.iter().map(|id| json!({
            "skill": id,
            "ticks": lord.skill_tick(*id),
        })).collect::<Vec<_>>(),
    })
}
