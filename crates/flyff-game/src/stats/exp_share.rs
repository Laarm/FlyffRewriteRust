//! Kill-EXP distribution for solo kills and the special bosses (Plan/16 §9, WP-MOVER-09; F-124, PL-081).
//!
//! [`on_monster_killed`] is the single entry doc 17's death pipeline calls (after the kill recovery, before the
//! loot; `AttackArbiter.cpp:938-944`). It contains both branches of the source:
//!
//! * the **special bosses** (`MI_CLOCKWORK1`, `MI_BIGMUSCLE`, `MI_KRRR` and, with
//!   `features.better_clockwork_alpha`, `MI_LOADCLOCKWORK`) split `exp_value` evenly among the players that see
//!   the corpse within 50 m ([`around_exp`], `SubAroundExp`);
//! * every other monster walks the **damage ledger** (`dead.combat.hate`, ascending `ObjId`): a player within 64 m
//!   of the corpse earns `exp_value * rates.exp * hits / total` ([`exp_solo`], `AddExperienceSolo`), with the
//!   level-gap penalty 0.7 / 0.4 / 0.1 and the per-level cap. The `MODE_EXPUP_STOP` flag of the **killer** zeroes
//!   the share of every attacker (KEEP-QUIRK, KQ-34).
//!
//! The party branch (WP-MOVER-20, `AddExperienceKillMember` / `AddExperienceParty`, 16 §9.3, §9.5): an attacker
//! in a party collects the hits of its party mates in range, the members within 64 m of it share the kill by
//! level or by contribution ([`exp_party`]), and the party earns its points once per share
//! (`social::party::party_points`).
//!
//! The flight EXP (`m_nFxp`) is kept as the original counter ([`super::progression::add_fxp`]); its messages
//! (`SetFlightLevel` near without self, `SetFxp` to self) precede the grant's, as in `AddExperienceSolo`.

use super::env::StatsCore;
use super::out::Out;
pub use super::progression::add_fxp;
use super::progression::{ExpGrant, emit_fxp, grant_experience};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::social::party::{ExpShareMode, PartyKind};
use flyff_common::math::Vec3;
use flyff_common::{LayerId, ObjId, PartyId, WorldId};
use flyff_proto::pb;
use flyff_rules::consts::EXPUP_STOP_MODE;
use flyff_rules::exp_share::{
    AROUND_EXP_RANGE, ContributionInput, KILL_EXP_RANGE, MAX_AROUND_USER, PARTY_MEMBER_RANGE,
    around_split, flight_level, is_around_exp_boss, party_contribution_mode, party_level_floor,
    party_level_mode, person_share, reduce_factor, solo_share,
};
use flyff_rules::progression::limit_exp;

/// Doc 21's `PartyService::party_of(player)` as the kill distribution needs it: the party the player is a member
/// of (`g_PartyMng.GetParty( m_idparty )` + `IsMember`).
fn party_of(svc: &Services, ctx: &GameCtx<'_>, who: ObjId) -> Option<PartyId> {
    let c = ctx.worlds.player(who)?.player()?.ident.character_id;
    svc.social.party.party_of(c)
}

/// `pParty->GetPoint(0, 1, monsterLevel)` (`__PARTY_EXP_SOLO`, 16 §9.4): the party points a member earns by a solo
/// kill.
fn solo_party_points(svc: &mut Services, ctx: &mut GameCtx<'_>, who: ObjId, monster_level: i32) {
    if let Some(pid) = party_of(svc, ctx, who) {
        crate::social::party::points::party_points(svc, ctx, pid, 0, 1, monster_level);
    }
}

/// What the distribution needs to know about the dead monster (copied out of the world first).
#[derive(Clone, Debug, PartialEq)]
pub struct Corpse {
    /// `MoverProp.id` (`m_dwIndex`).
    pub prop_id: u32,
    /// `nExpValue`.
    pub exp_value: i64,
    /// `nFxpValue`.
    pub fxp_value: i32,
    /// `dwLevel`.
    pub level: i32,
    /// `GetLevel()` of the dead mover (`m_nLevel`): what `GetPoint` receives in the party branch.
    pub mover_level: i32,
    /// `GetMaxHitPoint()` of the dead mover (the contribution mode divides by it).
    pub max_hp: i32,
    /// `m_fExp_Rate` (the per-prop GM multiplier, 1.0 unless an override exists).
    pub exp_rate: f32,
    /// Where it died.
    pub pos: Vec3,
    /// The world of the corpse.
    pub world: WorldId,
    /// The layer of the corpse.
    pub layer: LayerId,
    /// The damage ledger in ascending `ObjId` order: `(attacker, summed hits)`.
    pub ledger: Vec<(ObjId, i32)>,
    /// The players that currently see the corpse, ascending.
    pub viewers: Vec<ObjId>,
}

/// The facts about `dead` the distribution needs; `None` for a mover the world or the game data does not know.
pub fn corpse_of(ctx: &GameCtx<'_>, dead: ObjId) -> Option<Corpse> {
    let m = ctx.worlds.mover(dead)?;
    let prop = ctx.data.mover(m.prop_id())?;
    let mut viewers: Vec<ObjId> = m.core.viewers.iter().copied().collect();
    viewers.sort();
    Some(Corpse {
        prop_id: prop.id,
        exp_value: prop.exp_value,
        fxp_value: prop.fxp_value,
        level: i32::try_from(prop.level).unwrap_or(i32::MAX),
        mover_level: m.mv.level,
        max_hp: 0,
        exp_rate: m.mv.rates.exp,
        pos: m.core.pos,
        world: m.core.world,
        layer: m.core.layer,
        ledger: m.combat.hate.iter().map(|(id, e)| (id, e.hits)).collect(),
        viewers,
    })
}

/// `CMover::IsValidArea(other, range)` seen from the corpse: the same world and layer and a squared distance
/// strictly below `range * range` (f32, `D3DXVec3LengthSq`).
fn in_area(corpse: &Corpse, other: &crate::mover::Mover, range: f32) -> bool {
    other.core.world == corpse.world
        && other.core.layer == corpse.layer
        && (other.core.pos - corpse.pos).length_squared() < range * range
}

/// Doc 17's single entry (16 §9.1): `killer` has just killed `dead`. The special bosses split their EXP among the
/// players around the corpse, every other monster pays the players of its damage ledger. Does nothing when the
/// monster is unknown to the game data or is a player (no EXP for killing a player).
pub fn on_monster_killed(svc: &mut Services, ctx: &mut GameCtx<'_>, killer: ObjId, dead: ObjId) {
    let Some(mut corpse) = corpse_of(ctx, dead) else {
        return;
    };
    if ctx.worlds.mover(dead).is_some_and(|m| m.is_player()) {
        return;
    }
    let c = svc.stats.handle();
    corpse.max_hp = ctx
        .worlds
        .mover(dead)
        .map_or(0, |m| super::env::max_hp(m, &c));
    if is_around_exp_boss(corpse.prop_id, c.exp.better_clockwork_alpha) {
        around_exp(svc, ctx, &c, &corpse);
    } else {
        kill_members(svc, ctx, &c, killer, &corpse);
    }
}

/// `SubAroundExp(attacker, 50 m)` (`Mover.cpp:5985-6071`, 16 §9.2): every player that sees the corpse within 50 m
/// (3-D, `<=`) gets `exp_value / users` (i64 division), capped by the level of the user. No rates, no
/// `MODE_EXPUP_STOP` of the killer; each user's own flag still stops its grant inside `add_experience`.
///
/// With `fidelity.party_exp_loop_fix` (PL-081, default) each user is capped independently; without it the capped
/// value carries to the later users (the source's quirk).
pub fn around_exp(svc: &mut Services, ctx: &mut GameCtx<'_>, c: &StatsCore, corpse: &Corpse) {
    let mut users: Vec<ObjId> = corpse
        .viewers
        .iter()
        .copied()
        .filter(|id| {
            ctx.worlds.player(*id).is_some_and(|u| {
                (u.core.pos - corpse.pos).length_squared() <= AROUND_EXP_RANGE * AROUND_EXP_RANGE
            })
        })
        .collect();
    users.truncate(MAX_AROUND_USER);
    if users.is_empty() {
        tracing::warn!(
            prop = corpse.prop_id,
            "SubAroundExp: nobody around the corpse"
        );
        return;
    }
    let share = around_split(corpse.exp_value, users.len());
    let mut carried = share;
    for id in users {
        let Some(level) = ctx.worlds.player(id).map(|u| u.mv.level) else {
            continue;
        };
        let cap = limit_exp(&c.rules, level);
        let e = if c.exp.party_exp_loop_fix {
            share.min(cap)
        } else {
            carried = carried.min(cap);
            carried
        };
        grant_experience(svc, ctx, PlayerRef(id), ExpGrant::kill(e), Out::new());
    }
}

/// `SubExperience` + `AddExperienceKillMember` (`Mover.cpp:6125-6304`, 16 §9.3): the raw EXP is
/// `exp_value * rates.exp` (`f64`, zero under the killer's EXP stop); each player of the ledger within 64 m of the
/// corpse earns its share of the hits. An attacker in a party first collects the hits of its party mates that are
/// also within 64 m of the corpse (they are then skipped); the share goes to the party members within 64 m of the
/// attacker ([`exp_party`]), or to the attacker alone when it is the only one in range.
fn kill_members(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    c: &StatsCore,
    killer: ObjId,
    corpse: &Corpse,
) {
    let Some(killer_mode) = ctx.worlds.player(killer).map(|k| k.mv.mode) else {
        return; // `IsPlayer() == FALSE`: only a player's kill pays
    };
    let mut f_exp = corpse.exp_value as f64 * f64::from(corpse.exp_rate);
    let f_fxp = corpse.fxp_value as f32 * corpse.exp_rate;
    if c.exp.exp_stop && killer_mode & EXPUP_STOP_MODE != 0 {
        f_exp = 0.0;
    }
    // `dwMaxEnemyHit`: the DWORD sum of the ledger
    let total = corpse
        .ledger
        .iter()
        .fold(0_u32, |t, (_, hits)| t.wrapping_add(*hits as u32));
    if total == 0 {
        return;
    }
    let valid = |ctx: &GameCtx<'_>, id: ObjId| {
        ctx.worlds
            .player(id)
            .is_some_and(|e| in_area(corpse, e, KILL_EXP_RANGE))
    };
    let n = corpse.ledger.len();
    let mut consumed = vec![false; n];
    for j in 0..n {
        let Some(&(id, hits_j)) = corpse.ledger.get(j) else {
            break;
        };
        if consumed[j] || !valid(ctx, id) {
            continue;
        }
        let mut hits = hits_j;
        let mut party_hits: u32 = 0;
        let party = party_of(svc, ctx, id);
        if let Some(pid) = party {
            party_hits = hits_j as u32;
            for (k, &(id2, hits_k)) in corpse.ledger.iter().enumerate().skip(j + 1) {
                if consumed[k] {
                    continue;
                }
                if valid(ctx, id2) {
                    if party_of(svc, ctx, id2) == Some(pid) {
                        party_hits = party_hits.wrapping_add(hits_k as u32);
                        consumed[k] = true;
                    }
                } else {
                    consumed[k] = true; // out of range or not a player: nothing to collect
                }
            }
            if party_hits > 0 {
                hits = party_hits as i32;
            }
        }
        let person = person_share(f_exp, hits as u32, total);
        match party {
            Some(pid) if party_hits != 0 => {
                // `GetPartyMemberFind`: the members within 64 m of the credited attacker
                let members = in_range_members(svc, ctx, pid, id);
                let total_level: i32 = members.iter().map(|m| m.level).sum();
                if members.is_empty() || total_level == 0 {
                    if c.exp.party_exp_loop_fix {
                        continue; // PL-081: the next attacker is still served
                    }
                    break; // legacy: the whole loop ends
                }
                if members.len() > 1 {
                    exp_party(svc, ctx, c, corpse, pid, &members, f64::from(person));
                } else {
                    exp_solo(
                        svc,
                        ctx,
                        c,
                        PlayerRef(id),
                        f64::from(person),
                        f_fxp,
                        corpse,
                        true,
                    );
                }
            }
            _ => exp_solo(
                svc,
                ctx,
                c,
                PlayerRef(id),
                f64::from(person),
                f_fxp,
                corpse,
                false,
            ),
        }
    }
}

/// One party member in range of the credited attacker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RangeMember {
    /// The avatar.
    pub obj: ObjId,
    /// `GetLevel()`.
    pub level: i32,
}

/// `GetPartyMemberFind( pParty, .., 64.0f )` (`Mover.cpp:6184-6218`): the online members of `party`, in party
/// order, that are valid players in the world and layer of `from` with a squared distance **strictly** below
/// `64²` of it (`IsValidArea`); `from` itself counts when it is a member.
pub fn in_range_members(
    svc: &Services,
    ctx: &GameCtx<'_>,
    party: PartyId,
    from: ObjId,
) -> Vec<RangeMember> {
    let Some(p) = svc.social.party.get(party) else {
        return Vec::new();
    };
    let Some(origin) = ctx.worlds.core(from) else {
        return Vec::new();
    };
    p.members
        .iter()
        .filter_map(|slot| slot.obj)
        .filter_map(|o| {
            let m = ctx.worlds.player(o)?;
            let valid = !m.core.deleted
                && m.core.world == origin.world
                && m.core.layer == origin.layer
                && (m.core.pos - origin.pos).length_squared()
                    < PARTY_MEMBER_RANGE * PARTY_MEMBER_RANGE;
            valid.then_some(RangeMember {
                obj: o,
                level: m.mv.level,
            })
        })
        .collect()
}

/// `AddExperienceParty` (`Mover.cpp:6372-6405`, 16 §9.5): the share `person` of one party (the hits of all its
/// members in range) goes to `members`. The reduce factor of the highest level in range scales it, the party earns
/// its points once, then an advanced party by contribution shares by damage, every other party by level
/// (`AddExperiencePartyLevel`). Each member's EXP is capped by its level's limit and granted like a kill EXP
/// (`AddPartyMemberExperience`: no flight EXP in party mode).
pub fn exp_party(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    c: &StatsCore,
    corpse: &Corpse,
    party: PartyId,
    members: &[RangeMember],
    person: f64,
) {
    let max_level = members.iter().map(|m| m.level).max().unwrap_or(0);
    let factor = reduce_factor(corpse.level, max_level);
    let f_exp = person * f64::from(factor);
    let total_level: i32 = members.iter().map(|m| m.level).sum();
    crate::social::party::points::party_points(
        svc,
        ctx,
        party,
        total_level,
        i32::try_from(members.len()).unwrap_or(i32::MAX),
        corpse.mover_level,
    );
    let Some(p) = svc.social.party.get(party) else {
        return;
    };
    let by_contribution =
        p.kind == PartyKind::Advanced && p.exp_share == ExpShareMode::Contribution;
    let party_size = p.size();
    let floor = party_level_floor(max_level);
    let levels: Vec<i32> = members.iter().map(|m| m.level).collect();
    let shares = if by_contribution {
        let hits: Vec<i32> = members
            .iter()
            .map(|m| {
                corpse
                    .ledger
                    .iter()
                    .find(|(o, _)| *o == m.obj)
                    .map_or(0, |(_, h)| *h)
            })
            .collect();
        party_contribution_mode(&ContributionInput {
            f_exp,
            levels: &levels,
            hits: &hits,
            party_size,
            max_level10: floor,
            monster_max_hp: corpse.max_hp,
        })
    } else {
        party_level_mode(f_exp, &levels, floor)
    };
    for (m, share) in members.iter().zip(shares) {
        let Some(e) = share else { continue };
        let cap = limit_exp(&c.rules, m.level);
        grant_experience(
            svc,
            ctx,
            PlayerRef(m.obj),
            ExpGrant::kill(e.min(cap)),
            Out::new(),
        );
    }
}

/// `AddExperienceSolo(fExpValue, prop, fFxpValue, bParty)` (`Mover.cpp:6306-6370`, 16 §9.4): the level-gap factor,
/// the cap of the player's level, the flight EXP (`SetFlightLevel` to the near set without self when it wrapped,
/// `SetFxp` to self), then the grant with the EXP factor and the level-up sequence.
#[allow(clippy::too_many_arguments)]
pub fn exp_solo(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    c: &StatsCore,
    who: PlayerRef,
    f_exp: f64,
    f_fxp: f32,
    corpse: &Corpse,
    in_party: bool,
) {
    let Some(level) = ctx.worlds.player(who.0).map(|m| m.mv.level) else {
        return;
    };
    if c.exp.party_exp_solo && in_party {
        solo_party_points(svc, ctx, who.0, corpse.level);
    }
    let (exp, fxp) = solo_share(&c.rules, f_exp, f_fxp, level, corpse.level);
    let mut pre = Out::new();
    if fxp != 0.0
        && let Some(m) = ctx.worlds.player_mut(who.0)
    {
        if add_fxp(m, c, fxp as i32) {
            pre.near_excl(pb::SetFlightLevel {
                flight_level: flight_level(m.mv.level) as u32,
            });
        }
        emit_fxp(m, &mut pre);
    }
    grant_experience(svc, ctx, who, ExpGrant::kill(exp as i64), pre);
}
