//! `event::lord::join`: the `Lord` message of every join and re-attach (Plan/22 §9.7, §19.2; join position 15).
//!
//! The legacy `CUser::AddLord` ran on every login, the idle character included (`CLord::Serialize`): the lord, the
//! election with its property and candidates, the running lord events and the skill cooldowns. Changes travel as the
//! specific messages afterwards.

use super::NO_LORD;
use crate::ctx::Services;
use crate::outbox::Outbox;
use crate::world::join::{JoinContributor, JoinCtx, JoinStep};
use flyff_proto::pb;

/// `Lord` (`JoinStep::Lord`, position 15; `obj_id` 0, the legacy `NULL_ID`).
pub struct LordStep;

impl JoinContributor for LordStep {
    fn step(&self) -> JoinStep {
        JoinStep::Lord
    }

    fn write(&self, cx: &JoinCtx<'_>, out: &mut Outbox) {
        out.to_session(cx.session, 0, snapshot(cx.svc));
    }
}

/// The `Lord` message of the current state: `id_lord` is `NULL_ID` without a lord, the property comes from `election.inc`,
/// the candidates are in memory order and the skill ticks are all skills in table order.
pub fn snapshot(svc: &Services) -> pb::Lord {
    let lord = &svc.event.lord;
    let t = &lord.table;
    let clip = |v: i64| i32::try_from(v).unwrap_or(i32::MAX);
    let items = t
        .items
        .iter()
        .map(|list| pb::LordElectionItems {
            item_ids: list
                .iter()
                .map(|id| i32::try_from(*id).unwrap_or(i32::MAX))
                .collect(),
        })
        .collect();
    let election = &lord.election;
    pb::Lord {
        id_lord: lord.lord.map_or(NO_LORD, |c| c.0),
        election: Some(pb::LordElection {
            election_id: election.id,
            state: i32::from(election.state.code()),
            begin_at: election.begin_at.0,
            requirement: election.requirement,
            property: Some(pb::LordElectionProperty {
                day_of_week: t.day_of_week,
                hour: t.hour,
                candidacy_s: clip(t.candidacy_s),
                vote_s: clip(t.vote_s),
                requirement_factor: t.requirement_factor,
                days: t.days,
                deposit_return_rates: t.rates.clone(),
                items,
            }),
            candidates: election
                .candidates
                .iter()
                .map(|c| pb::LordCandidate {
                    id_player: c.id.0,
                    deposit: c.deposit,
                    votes: c.votes,
                    created_at: c.created_at.0,
                    pledge: c.pledge.clone(),
                })
                .collect(),
        }),
        events: lord
            .events
            .iter()
            .map(|e| pb::LordEventComponent {
                tick: e.ticks,
                id_player: e.creator.0,
                exp_factor: e.exp_factor,
                item_factor: e.item_factor,
            })
            .collect(),
        skill_ticks: super::skill::skill_entries(svc),
    }
}
