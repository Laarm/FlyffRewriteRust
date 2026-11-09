//! Party skills, mode timers and the effect queries (Plan/21 §4.8; WP-SOCIAL-06;
//! `src:_Common/party.cpp:300-572`, `src:CORESERVER/DPCoreSrvr.cpp:1015-1084`).
//!
//! * [`on_party_skill_use`]: the leader of an advanced party uses a skill of `propTroupeSkill` (21 §4.8.1). The
//!   instant skills 1-4 (Call, Blitz, Retreat, Sphere Circle) pay their points when they actually fire and refresh
//!   the members' `PartyExp`; the mode skills 5-8 start a timer (`SetPartyMode`).
//! * [`tick_modes`]: the timers of the modes 0..=3 lose 1000 ms per `party::worker` run and end with one
//!   `SetPartyMode` off.
//! * PARSKILL (mode 4, the "party skill full" cash items): a flag that is on while any online member has
//!   `SM_PARTYSKILL1/15/30` ([`reevaluate_parskill`]).
//! * The effect queries of docs 16/17/19: [`stretching_factor`], [`link_attack_state_in`], [`drop_party_modes`].

use super::link::{PartyLink, Stretching};
use super::model::{
    MODE_FORTUNECIRCLE, MODE_GIFTBOX, MODE_LINKATTACK, MODE_PARSKILL, MODE_STRETCHING, Party,
};
use super::wire::{
    ERROR_NOTPARTYPOINT, ERROR_NOTPARTYSKILL, ERROR_NOTTARGET, error, to_list, to_members,
};
use super::{identity, sync_links};
use crate::buff::{BuffKind, has};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::mover::Mover;
use crate::world::ObjectStore;
use flyff_common::defs::item::{
    II_SYS_SYS_SCR_PARTYSKILLUP01, II_SYS_SYS_SCR_PARTYSKILLUP01_01, II_SYS_SYS_SCR_PARTYSKILLUP02,
};
use flyff_common::defs::job::{JOB_ASSIST, JOB_BILLPOSTER, JOB_RINGMASTER};
use flyff_common::math::Vec3;
use flyff_common::{CharacterId, ObjId, PartyId};
use flyff_proto::pb;
use flyff_rules::combat::LinkAttackState;
use flyff_rules::consts::{SM_PARTYSKILL1, SM_PARTYSKILL15, SM_PARTYSKILL30};
use flyff_sim::types::MVRF_CRITICAL;
use smallvec::SmallVec;

/// `ST_CALL`.
pub const ST_CALL: i32 = 1;
/// `ST_BLITZ`.
pub const ST_BLITZ: i32 = 2;
/// `ST_RETREAT`.
pub const ST_RETREAT: i32 = 3;
/// `ST_SPHERECIRCLE`.
pub const ST_SPHERECIRCLE: i32 = 4;
/// `ST_LINKATTACK`.
pub const ST_LINKATTACK: i32 = 5;
/// `ST_FORTUNECIRCLE`.
pub const ST_FORTUNECIRCLE: i32 = 6;
/// `ST_STRETCHING`.
pub const ST_STRETCHING: i32 = 7;
/// `ST_GIFTBOX`.
pub const ST_GIFTBOX: i32 = 8;

/// The leader's party-skill range for the drop and regeneration bonuses (`IsValidArea( .., 255.0f )`).
pub const LEADER_RANGE: f32 = 255.0;
/// The duration of the PARSKILL flag message (`SendUserPartySkill( .., PARTY_PARSKILL_MODE, 1000, 0, 1 )`).
pub const PARSKILL_MESSAGE_MS: u32 = 1000;

/// `CSLord::IsLord( leader )` (doc 22 §9): the one body is `social::hooks::is_lord` (the lord of the election,
/// `svc.event.lord`), which the honor title `HS_LORD` reads too.
fn is_lord(svc: &Services, c: CharacterId) -> bool {
    crate::social::hooks::is_lord(svc, c)
}

/// `IsValidArea` between two avatars: the same world and layer and a squared distance strictly below `range²`.
fn valid_area(store: &ObjectStore, a: ObjId, b: ObjId, range: f32) -> bool {
    match (store.core(a), store.core(b)) {
        (Some(x), Some(y)) => {
            x.world == y.world
                && x.layer == y.layer
                && (x.pos - y.pos).length_squared() < range * range
        }
        _ => false,
    }
}

/// `SM_PARTYSKILL1`, `15` or `30` is running on the avatar (`IsSMMode`).
fn has_party_skill_item(m: &Mover) -> bool {
    crate::stats::sm::is_party_skill(m)
}

// ---- the mode messages ------------------------------------------------------------------------------------------

/// `SetPartyMode{mode, time, on, point}` to every online member.
fn send_mode(
    svc: &Services,
    ctx: &mut GameCtx<'_>,
    party: PartyId,
    mode: usize,
    time_ms: u32,
    on: bool,
    point: i32,
) {
    to_members(
        svc,
        ctx,
        party,
        pb::SetPartyMode {
            mode: mode as i32,
            time_ms,
            on,
            point: if on { point } else { 0 },
        },
    );
}

/// Zeroes every running mode 0..=3 and tells the members (`SetPartyMode` off, in mode order). PARSKILL is not
/// touched: it follows the members' items. Used by a leader change and the removal of the leader (21 §4.3).
pub fn zero_modes_with_messages(svc: &mut Services, ctx: &mut GameCtx<'_>, party: PartyId) {
    let mut zeroed: SmallVec<[usize; 4]> = SmallVec::new();
    if let Some(p) = svc.social.party.get_mut(party) {
        for m in 0..MODE_PARSKILL {
            if p.mode_ms[m] != 0 {
                p.mode_ms[m] = 0;
                zeroed.push(m);
            }
        }
    }
    for m in zeroed {
        send_mode(svc, ctx, party, m, 0, false, 0);
    }
}

/// One second of the mode timers (`party::worker`, 21 §4.8.3): every running mode 0..=3 loses 1000 ms; at or
/// below 0 it stops with one `SetPartyMode{mode, 0, false}`. The links follow.
pub fn tick_modes(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    for id in svc.social.party.ids() {
        let mut ended: SmallVec<[usize; 4]> = SmallVec::new();
        if let Some(p) = svc.social.party.get_mut(id) {
            for m in 0..MODE_PARSKILL {
                if p.mode_ms[m] != 0 {
                    p.mode_ms[m] -= 1000;
                    if p.mode_ms[m] <= 0 {
                        p.mode_ms[m] = 0;
                        ended.push(m);
                    }
                }
            }
        }
        if !ended.is_empty() {
            for m in &ended {
                send_mode(svc, ctx, id, *m, 0, false, 0);
            }
            sync_links(svc, ctx, id);
        }
    }
}

// ---- PARSKILL ---------------------------------------------------------------------------------------------------

/// Whether any online member of `party` has a "party skill full" item running.
pub fn parskill_wanted(svc: &Services, ctx: &GameCtx<'_>, party: PartyId) -> bool {
    svc.social.party.get(party).is_some_and(|p| {
        p.online_objs()
            .any(|o| ctx.worlds.player(o).is_some_and(has_party_skill_item))
    })
}

/// Evaluates the PARSKILL flag again (a join, a logout, a reconnect, a removal, an item use or expiry): on a
/// change the members get `SetPartyMode{4, 1000 or 0, on, point}` (21 §4.8.4).
pub fn reevaluate_parskill(svc: &mut Services, ctx: &mut GameCtx<'_>, party: PartyId) {
    let want = parskill_wanted(svc, ctx, party);
    let Some(p) = svc.social.party.get_mut(party) else {
        return;
    };
    let on = p.mode_ms[MODE_PARSKILL] != 0;
    if on == want {
        return;
    }
    p.mode_ms[MODE_PARSKILL] = i32::from(want);
    let point = p.point;
    let time = if want { PARSKILL_MESSAGE_MS } else { 0 };
    send_mode(svc, ctx, party, MODE_PARSKILL, time, want, point);
    sync_links(svc, ctx, party);
}

/// `on_sm_mode_changed` for the party skill items: the owner's party evaluates PARSKILL again.
pub fn on_sm_changed(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, sm: u8) {
    let sm = usize::from(sm);
    if sm != SM_PARTYSKILL1 && sm != SM_PARTYSKILL15 && sm != SM_PARTYSKILL30 {
        return;
    }
    let Some(c) = identity(ctx, who) else { return };
    if let Some(pid) = svc.social.party.party_of(c) {
        reevaluate_parskill(svc, ctx, pid);
    }
}

// ---- PartySkillUse ----------------------------------------------------------------------------------------------

/// `PartySkillUse{skill}` (`CParty::DoUsePartySkill`, 21 §4.8.2): ignored unless the sender leads an advanced
/// party and the skill exists. `ErrorParty` 206 when the party level is below the skill's, 205 when the points are
/// short. Instant skills 1-4 pay when they fire (Blitz and Sphere Circle need a monster target:
/// `ErrorParty{207, skill}` otherwise) and refresh the members' `PartyExp`; mode skills 5-8 start the timer.
pub fn on_party_skill_use(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::PartySkillUse,
) {
    let Some(character) = identity(ctx, who) else {
        return;
    };
    let Some(pid) = svc.social.party.party_of(character) else {
        return;
    };
    let Some(p) = svc.social.party.get(pid) else {
        return;
    };
    if p.leader_id() != Some(character) || p.kind != flyff_rules::social::PartyKind::Advanced {
        return;
    }
    let Some(prop) = u32::try_from(m.skill)
        .ok()
        .and_then(|id| ctx.data.party_skills.get(id))
    else {
        return;
    };
    if p.level < i32::try_from(prop.req_dis_lv).unwrap_or(i32::MAX) {
        error(ctx, who.0, ERROR_NOTPARTYSKILL, 0);
        return;
    }
    let cost = i32::try_from(prop.exp).unwrap_or(i32::MAX);
    let mut time = prop.skill_time;
    if is_lord(svc, character) {
        time = time.saturating_mul(4);
    }
    let cash = (has(
        ctx,
        who.0,
        BuffKind::Item2,
        II_SYS_SYS_SCR_PARTYSKILLUP01 as u16,
    ) || has(
        ctx,
        who.0,
        BuffKind::Item2,
        II_SYS_SYS_SCR_PARTYSKILLUP02 as u16,
    ) || has(
        ctx,
        who.0,
        BuffKind::Item2,
        II_SYS_SYS_SCR_PARTYSKILLUP01_01 as u16,
    )) && matches!(
        m.skill,
        ST_LINKATTACK | ST_FORTUNECIRCLE | ST_STRETCHING | ST_GIFTBOX
    );
    if p.point.wrapping_sub(cost) < 0 {
        error(ctx, who.0, ERROR_NOTPARTYPOINT, 0);
        return;
    }
    match m.skill {
        ST_CALL => {
            // every online member, the leader included, gets the leader's position; the cost (0) is paid after
            let pos = ctx.worlds.core(who.0).map_or(Vec3::ZERO, |c| c.pos);
            let objs: Vec<ObjId> = svc
                .social
                .party
                .get(pid)
                .map(|p| p.online_objs().collect())
                .unwrap_or_default();
            to_list(
                svc,
                ctx,
                &objs,
                pb::PartySkillCall {
                    leader_pos: Some(crate::movement::intents::pb_vec(pos)),
                },
            );
            deduct_cost(svc, ctx, pid, cost);
        }
        ST_BLITZ | ST_SPHERECIRCLE => {
            let target = ctx
                .player(who)
                .and_then(|l| l.combat.id_set_target)
                .filter(|t| ctx.worlds.mover(*t).is_some_and(|t| !t.is_player()));
            let Some(target) = target else {
                error(ctx, who.0, ERROR_NOTTARGET, m.skill as u32);
                return;
            };
            let audience = skill_audience(svc, ctx, pid, who.0);
            if m.skill == ST_BLITZ {
                to_list(
                    svc,
                    ctx,
                    &audience,
                    pb::PartySkillBlitz { target: target.0 },
                );
            } else {
                // the Sphere Circle message carries the target in its header and arms the one-shot critical
                for o in &audience {
                    if let Some(sid) = ctx.session_of(PlayerRef(*o)) {
                        ctx.outbox
                            .to_session(sid, target.0, pb::PartySkillSphereCircle {});
                    }
                    if let Some(mv) = ctx.worlds.player_mut(*o) {
                        mv.sim.mvrf.add(MVRF_CRITICAL);
                    }
                }
            }
            deduct_cost(svc, ctx, pid, cost);
        }
        ST_RETREAT => {
            let audience = skill_audience(svc, ctx, pid, who.0);
            for o in &audience {
                // header = the member itself (`AddHdr( pMember->GetId(), ... )`)
                if let Some(sid) = ctx.session_of(PlayerRef(*o)) {
                    ctx.outbox.to_session(sid, o.0, pb::PartySkillRetreat {});
                }
            }
            deduct_cost(svc, ctx, pid, cost);
        }
        ST_LINKATTACK | ST_FORTUNECIRCLE | ST_STRETCHING | ST_GIFTBOX => {
            let mode = match m.skill {
                ST_LINKATTACK => MODE_LINKATTACK,
                ST_FORTUNECIRCLE => MODE_FORTUNECIRCLE,
                ST_STRETCHING => MODE_STRETCHING,
                _ => MODE_GIFTBOX,
            };
            start_mode(svc, ctx, pid, mode, time, cost, cash);
        }
        _ => {}
    }
}

/// Core `OnPartySkillUse` for a mode: refuses silently when the points are short or the mode runs without a
/// "skill up" item; otherwise pays, sets or extends the timer and tells the members.
fn start_mode(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    pid: PartyId,
    mode: usize,
    time_ms: u32,
    cost: i32,
    cash: bool,
) {
    let Some(p) = svc.social.party.get_mut(pid) else {
        return;
    };
    if p.point < cost || (!cash && p.mode_ms[mode] != 0) {
        return;
    }
    p.point -= cost;
    let add = i32::try_from(time_ms).unwrap_or(i32::MAX);
    p.mode_ms[mode] = if cash {
        p.mode_ms[mode].saturating_add(add)
    } else {
        add
    };
    let (remaining, point) = (u32::try_from(p.mode_ms[mode]).unwrap_or(0), p.point);
    send_mode(svc, ctx, pid, mode, remaining, true, point);
    sync_links(svc, ctx, pid);
}

/// The members a Blitz, Retreat or Sphere Circle reaches: everybody online while Gift Box or Fortune Circle runs,
/// else the online members in the leader's view set (`IsNearPC`).
fn skill_audience(
    svc: &Services,
    ctx: &GameCtx<'_>,
    pid: PartyId,
    leader: ObjId,
) -> SmallVec<[ObjId; 8]> {
    let Some(p) = svc.social.party.get(pid) else {
        return SmallVec::new();
    };
    if p.mode_running(MODE_GIFTBOX) || p.mode_running(MODE_FORTUNECIRCLE) {
        return p.online_objs().collect();
    }
    let viewers = ctx.worlds.core(leader).map(|c| &c.viewers);
    p.online_objs()
        .filter(|o| viewers.is_some_and(|v| v.contains(o)))
        .collect()
}

/// `SendRemovePartyPoint( party, cost )`: the points are paid and every member gets `PartyExp` (the Core's
/// reply in the original; the skill's own messages went out first).
fn deduct_cost(svc: &mut Services, ctx: &mut GameCtx<'_>, pid: PartyId, cost: i32) {
    if let Some(p) = svc.social.party.get_mut(pid) {
        p.point -= cost;
    }
    refresh_exp(svc, ctx, pid);
}

/// `PartyExp{exp, level, point}` to every online member (`SETPARTYEXP`, `REMOVEPARTYPOINT`).
pub fn refresh_exp(svc: &Services, ctx: &mut GameCtx<'_>, pid: PartyId) {
    if let Some(p) = svc.social.party.get(pid) {
        to_members(
            svc,
            ctx,
            pid,
            pb::PartyExp {
                exp: p.exp,
                level: p.level,
                point: p.point,
            },
        );
    }
}

// ---- the effect queries -----------------------------------------------------------------------------------------

/// The sitting-regeneration factor `member` earns from the stretching mode of its party (`ProcessRecovery`,
/// `Mover.cpp:8446-8480`): an advanced party with mode 2 running, and PARSKILL running or the leader within 255 m
/// (the same world and layer); 1.8 when the leader is an Assist, Ringmaster or Billposter, else 1.5.
pub fn stretching_factor_in(
    svc: &Services,
    ctx: &GameCtx<'_>,
    party: PartyId,
    member: ObjId,
) -> Option<Stretching> {
    let p = svc.social.party.get(party)?;
    stretching_of(p, ctx.worlds.store(), member)
}

fn stretching_of(p: &Party, store: &ObjectStore, member: ObjId) -> Option<Stretching> {
    if p.kind != flyff_rules::social::PartyKind::Advanced || !p.mode_running(MODE_STRETCHING) {
        return None;
    }
    let leader = p.leader().and_then(|l| l.obj);
    let near = p.mode_running(MODE_PARSKILL)
        || leader.is_some_and(|l| valid_area(store, member, l, LEADER_RANGE));
    if !near {
        return None;
    }
    let buffer_job = leader
        .and_then(|l| store.mover(l))
        .is_some_and(|l| is_stretching_job(l.mv.job));
    Some(if buffer_job {
        Stretching::X18
    } else {
        Stretching::X15
    })
}

fn is_stretching_job(job: i32) -> bool {
    u32::try_from(job).is_ok_and(|j| matches!(j, JOB_ASSIST | JOB_RINGMASTER | JOB_BILLPOSTER))
}

/// `stretching_factor(g, sitter)` of 21 §4.8.5: 1.0, 1.5 or 1.8.
pub fn stretching_factor(svc: &Services, ctx: &GameCtx<'_>, sitter: ObjId) -> f32 {
    let Some(c) = ctx
        .worlds
        .player(sitter)
        .and_then(|m| m.player())
        .map(|p| p.ident.character_id)
    else {
        return 1.0;
    };
    svc.social
        .party
        .party_of(c)
        .and_then(|pid| stretching_factor_in(svc, ctx, pid, sitter))
        .map_or(1.0, Stretching::factor)
}

/// Refreshes the stretching factor on the links once per second (the leader walks; the sitting regeneration reads
/// the link because the tick tail has only the mover).
pub fn refresh_stretching(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    for id in svc.social.party.ids() {
        let Some(p) = svc.social.party.get(id) else {
            continue;
        };
        let objs: SmallVec<[ObjId; 8]> = p.online_objs().collect();
        for o in objs {
            let new = stretching_factor_in(svc, ctx, id, o);
            if let Some(l) = ctx
                .worlds
                .player_mut(o)
                .and_then(|m| m.player_mut())
                .and_then(|pl| pl.social.party_link.as_mut())
                && l.stretching != new
            {
                l.stretching = new;
            }
        }
    }
}

/// The party facts `CalcLinkAttackDamage` reads (17 §5.13): built from the attacker's link and, for the vanilla
/// rule only, the members' targets and positions in `store`. `LinkAttackState::default()` for a player outside a
/// party.
pub fn link_attack_state_in(
    store: &ObjectStore,
    att: &Mover,
    better_link_attack: bool,
) -> LinkAttackState {
    let Some(p) = att.player() else {
        return LinkAttackState::default();
    };
    let Some(link) = p.social.party_link.as_ref() else {
        return LinkAttackState::default();
    };
    let size = i32::try_from(link.size()).unwrap_or(i32::MAX);
    let mut s = LinkAttackState {
        in_party: true,
        advanced: link.kind == flyff_rules::social::PartyKind::Advanced,
        link_mode: link.mode_running(MODE_LINKATTACK),
        is_leader: link.is_leader(p.ident.character_id),
        parskill_mode: link.mode_running(MODE_PARSKILL),
        party_size: size,
        joined_members: 0,
    };
    // the vanilla count is only read without `features.better_link_attack`, by the leader, when the flag runs
    if !better_link_attack && s.advanced && s.link_mode && s.is_leader && !s.parskill_mode {
        s.joined_members = joined_members(store, att, link);
    }
    s
}

/// The members that share the leader's target within 255 m of the leader (the leader counts itself).
fn joined_members(store: &ObjectStore, leader: &Mover, link: &PartyLink) -> i32 {
    let Some(target) = leader.combat.id_set_target else {
        return 0;
    };
    let mut n = 0;
    for o in link.online_objs() {
        let Some(m) = store.mover(o) else { continue };
        if m.core.deleted || m.combat.id_set_target != Some(target) {
            continue;
        }
        if (m.core.pos - leader.core.pos).length_squared() < LEADER_RANGE * LEADER_RANGE {
            n += 1;
        }
    }
    n
}

/// The Sphere Circle critical bonus (17 §5.11): `party_size / 2` for a player in a party whose critical flag is
/// armed (the caller checked the flag).
pub fn party_critical(att: &Mover) -> Option<i32> {
    let link = att.player()?.social.party_link.as_ref()?;
    Some(i32::try_from(link.size()).unwrap_or(i32::MAX) / 2)
}

/// `drop_party_modes(g, killer)` (21 §4.8.5, 19 §6.2 step 4) over the object store: `(giftbox, fortune)` - the
/// killer belongs to an advanced party with Gift Box (two passes) or Fortune Circle (unique drops) running, and
/// PARSKILL runs or the leader is valid and within 255 m of the killer. The drop pass reads it through the
/// killer's [`PartyLink`] (`DropCx` has the store, not the services).
pub fn drop_party_modes_in(store: &ObjectStore, killer: ObjId) -> (bool, bool) {
    let Some(link) = store
        .mover(killer)
        .and_then(|m| m.player())
        .and_then(|p| p.social.party_link.as_ref())
    else {
        return (false, false);
    };
    if link.kind != flyff_rules::social::PartyKind::Advanced {
        return (false, false);
    }
    let (g, f) = (
        link.mode_running(MODE_GIFTBOX),
        link.mode_running(MODE_FORTUNECIRCLE),
    );
    if !g && !f {
        return (false, false);
    }
    let in_reach = link.mode_running(MODE_PARSKILL)
        || link
            .members
            .first()
            .and_then(|l| l.obj)
            .is_some_and(|l| valid_area(store, l, killer, LEADER_RANGE));
    if in_reach { (g, f) } else { (false, false) }
}

/// `drop_party_modes(g, killer)` of 21 §4.8.5 over the services.
pub fn drop_party_modes(_svc: &Services, ctx: &GameCtx<'_>, killer: ObjId) -> (bool, bool) {
    drop_party_modes_in(ctx.worlds.store(), killer)
}

/// `link_attack_bonus(g, attacker, damage)` of 21 §4.8.5: the Link Attack damage added to a hit of `attacker`
/// (the formula of `flyff_rules::combat::link_attack_bonus` over the party facts).
pub fn link_attack_bonus(
    svc: &Services,
    ctx: &GameCtx<'_>,
    attacker: ObjId,
    damage: i32,
    better_link_attack: bool,
) -> i32 {
    let _ = svc;
    let Some(m) = ctx.worlds.player(attacker) else {
        return 0;
    };
    let state = link_attack_state_in(ctx.worlds.store(), m, better_link_attack);
    let cfg = flyff_rules::combat::CombatConfig {
        better_link_attack,
        ..flyff_rules::combat::CombatConfig::default()
    };
    flyff_rules::combat::link_attack_bonus(damage, &state, &cfg)
}
