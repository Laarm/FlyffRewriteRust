//! The five client messages of the lord system (Plan/22 §9.4, §9.6, §19.1): thin glue from the generated handlers to
//! `election.rs`, `levent.rs` and `skill.rs`.
//!
//! Every handler acts for the session's player (VR-001: the message carries no actor). The NPC rules of 22 §2.6 are
//! checked HERE, first: the deposit needs `MMI_LORD_TENDER` (92) in range, the vote `MMI_LORD_VOTE` (93) and the lord
//! event `MMI_LORD_EVENT` (94); without the NPC (or in another layer, or farther than `gameplay.npc_menu_range`) the
//! request is ignored silently, as the original's call sites did. The pledge and the skills are not NPC-gated.

use super::{election, levent, skill};
use crate::ctx::{GameCtx, PlayerRef, Services};
use flyff_common::defs::neuz::{MMI_LORD_EVENT, MMI_LORD_TENDER, MMI_LORD_VOTE};
use flyff_proto::pb;

/// `ElectionAddDepositReq` (`OnElectionAddDeposit`).
pub fn on_election_add_deposit(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::ElectionAddDepositReq,
) {
    if election::near_npc(ctx, who, MMI_LORD_TENDER) {
        election::add_deposit(svc, ctx, who, m.total);
    }
}

/// `ElectionSetPledgeReq` (`OnElectionSetPledge`).
pub fn on_election_set_pledge(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::ElectionSetPledgeReq,
) {
    election::set_pledge(svc, ctx, who, &m.pledge);
}

/// `ElectionIncVoteReq` (`OnElectionIncVote`).
pub fn on_election_inc_vote(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::ElectionIncVoteReq,
) {
    if election::near_npc(ctx, who, MMI_LORD_VOTE) {
        election::inc_vote(svc, ctx, who, m.candidate);
    }
}

/// `LEventCreateReq` (`OnLEventCreate`).
pub fn on_l_event_create(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::LEventCreateReq,
) {
    if election::near_npc(ctx, who, MMI_LORD_EVENT) {
        levent::create_event(svc, ctx, who, m.e_index, m.i_index);
    }
}

/// `LordSkillUseReq` (`OnLordSkillUse`).
pub fn on_lord_skill_use(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::LordSkillUseReq,
) {
    skill::use_skill(svc, ctx, who, m.skill, &m.target_name);
}
