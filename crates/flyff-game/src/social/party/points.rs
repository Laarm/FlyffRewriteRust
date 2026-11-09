//! Party level, EXP and points (Plan/21 §4.5; WP-SOCIAL-04; `CParty::GetPoint` of `src:_Common/party.cpp:260-298`
//! and `CDPCoreSrvr::OnAddPartyExp` / `OnPartyLevel` of `src:CORESERVER/DPCoreSrvr.cpp`).
//!
//! [`party_points`] is the single entry doc 16 calls once per party share (16 §9.4, §9.5): the gate decides
//! whether the kill feeds the party at all, the formulas are `flyff_rules::social`, and the result goes to every
//! online member as `PartyExp`.

use super::skills::refresh_exp;
use crate::buff::{BuffKind, has, has_item};
use crate::ctx::{GameCtx, Services};
use flyff_common::PartyId;
use flyff_common::defs::item::{
    II_SYS_SYS_SCR_PARTYEXPUP01, II_SYS_SYS_SCR_PARTYEXPUP01_01, II_SYS_SYS_SCR_PARTYEXPUP02,
    II_SYS_SYS_SCR_SUPERLEADERPARTY,
};
use flyff_rules::social::{
    MAX_PARTY_LEVEL, PartyExpInput, PartyKind, advanced_party_next_exp, party_add_exp,
    party_points_gate,
};

/// `CParty::GetPoint( totalLevel, memberCount, monsterLevel )` followed by the core's `OnAddPartyExp`:
///
/// * the gate: `total_level / member_count - monster_level < 5` (integer division; the solo path
///   `features.party_exp_solo` calls it with `(0, 1, monster_level)`);
/// * a normal party at level 10 or more gains nothing and sends nothing;
/// * the leader's buffs: `II_SYS_SYS_SCR_SUPERLEADERPARTY` doubles, a `PARTYEXPUP` item (`BUFF_ITEM2`) multiplies by
///   1.5 (both need the leader to be online);
/// * the EXP, level and points of the party change (at most one level) and every online member gets `PartyExp`.
pub fn party_points(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    party: PartyId,
    total_level: i32,
    member_count: i32,
    monster_level: i32,
) {
    if !party_points_gate(total_level, member_count, monster_level) {
        return;
    }
    let Some(p) = svc.social.party.get(party) else {
        return;
    };
    let leader = p.leader().and_then(|l| l.obj);
    let (super_leader, leader_exp_up) = match leader {
        Some(l) => (
            has_item(ctx, l, II_SYS_SYS_SCR_SUPERLEADERPARTY),
            [
                II_SYS_SYS_SCR_PARTYEXPUP01,
                II_SYS_SYS_SCR_PARTYEXPUP02,
                II_SYS_SYS_SCR_PARTYEXPUP01_01,
            ]
            .into_iter()
            .any(|id| has(ctx, l, BuffKind::Item2, id as u16)),
        ),
        None => (false, false),
    };
    let input = PartyExpInput {
        kind: p.kind,
        level: p.level,
        exp: p.exp,
        point: p.point,
        monster_level,
        rate: ctx.live.rates.party_exp,
        super_leader,
        leader_exp_up,
    };
    let Some((level, exp, point)) = party_add_exp(&input, &ctx.data.exp.party) else {
        return;
    };
    if let Some(p) = svc.social.party.get_mut(party) {
        p.level = level;
        p.exp = exp;
        p.point = point;
    }
    refresh_exp(svc, ctx, party);
}

/// `/plv`: sets the level and the points of the party (the current EXP stays) and clamps the EXP below the next
/// level's requirement (`OnPartyLevel`): a level under 10 against `expParty[level]`, any other against the
/// advanced formula (also for a normal party, as the original). Every online member gets `PartyExp`.
pub fn set_party_level(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    party: PartyId,
    level: i32,
    point: i32,
) {
    let table = &ctx.data.exp.party;
    let Some(p) = svc.social.party.get_mut(party) else {
        return;
    };
    p.level = level;
    p.point = point;
    let need = if level < MAX_PARTY_LEVEL {
        usize::try_from(level)
            .ok()
            .and_then(|i| table.get(i))
            .map(|r| i32::try_from(r.exp).unwrap_or(i32::MAX))
    } else {
        Some(advanced_party_next_exp(level))
    };
    if let Some(need) = need
        && need <= p.exp
    {
        p.exp = need.saturating_sub(1).max(0);
    }
    refresh_exp(svc, ctx, party);
}

/// Whether a normal party can still gain levels (used by the client-facing queries).
pub fn gains_exp(kind: PartyKind, level: i32) -> bool {
    kind == PartyKind::Advanced || level < MAX_PARTY_LEVEL
}
