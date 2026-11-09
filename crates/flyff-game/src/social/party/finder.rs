//! The party finder, the recruiting flag, the navigator marks and the party map info (Plan/21 §4.9, §4.10;
//! WP-SOCIAL-07; `src:WORLDSERVER/DPSrvr.cpp:12992-13084`, `6634-6668`, `src:_Common/party.cpp:1260-1303`).

use super::model::{MODE_FORTUNECIRCLE, MODE_GIFTBOX, MODE_LINKATTACK, Party};
use super::wire::{name_of, to_list, to_members, to_one};
use super::{MAP_INFO_EVERY, MAP_INFO_FAR_SQ, identity};
use crate::buff::{BuffKind, has};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::movement::intents::pb_vec;
use flyff_common::defs::item::{
    II_SYS_SYS_SCR_PARTYEXPUP01, II_SYS_SYS_SCR_PARTYEXPUP01_01, II_SYS_SYS_SCR_PARTYEXPUP02,
    II_SYS_SYS_SCR_PARTYSKILLUP01, II_SYS_SYS_SCR_PARTYSKILLUP01_01, II_SYS_SYS_SCR_PARTYSKILLUP02,
};
use flyff_common::defs::text::TID_APP_PARTY;
use flyff_common::{ObjId, PartyId};
use flyff_proto::pb;
use smallvec::SmallVec;

/// `PARTY_FINDER_LIST.name` holds the party name only when it is longer than this (else the text of
/// `TID_APP_PARTY`).
const FINDER_NAME_MIN_LEN: usize = 3;

/// `JoinPartyReq` (`OnJoinPartyReq`, needs `features.party_find`): the parties that recruit - the leader's
/// allow-join flag is set and the party has room - in party id order, with their six flags. `PartyFinderRefresh`
/// carries no count: the list length is the count (FIX, 11a).
pub fn on_join_party_req(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    _m: pb::JoinPartyReq,
) {
    if !ctx.cfg.rules.features.party_find {
        return;
    }
    let entries: Vec<pb::party_finder_refresh::PartyFinderEntry> = svc
        .social
        .party
        .iter()
        .filter(|p| !p.is_full() && leader_recruits(ctx, p))
        .map(|p| finder_entry(svc, ctx, p))
        .collect();
    to_one(ctx, who.0, pb::PartyFinderRefresh { entries });
}

/// The `__PARTY_FIND` flag of the party's leader (`CMover::m_bPartyJoin`): an online leader with the flag set.
fn leader_recruits(ctx: &GameCtx<'_>, p: &Party) -> bool {
    p.leader()
        .and_then(|l| l.obj)
        .and_then(|o| ctx.worlds.player(o))
        .and_then(|m| m.player())
        .is_some_and(|pl| pl.social.party_allow_join)
}

/// One finder row (`PARTYFINDER_LIST`).
fn finder_entry(
    svc: &Services,
    ctx: &GameCtx<'_>,
    p: &Party,
) -> pb::party_finder_refresh::PartyFinderEntry {
    let leader_id = p.leader_id().unwrap_or_default();
    let leader_obj = p.leader().and_then(|l| l.obj);
    let names: String = p
        .members
        .iter()
        .map(|m| format!("{}, ", name_of(svc, m.id)))
        .collect();
    let item = |ids: &[u32], kind: BuffKind| {
        leader_obj.is_some_and(|l| ids.iter().any(|id| has(ctx, l, kind, *id as u16)))
    };
    pb::party_finder_refresh::PartyFinderEntry {
        party_id: p.id.0,
        name: if p.name.len() > FINDER_NAME_MIN_LEN {
            p.name.clone()
        } else {
            ctx.data.texts.get(TID_APP_PARTY).to_owned()
        },
        member_count: i32::try_from(p.size()).unwrap_or(i32::MAX),
        level: p.level,
        point: p.point,
        leader_id: leader_id.0,
        leader_name: name_of(svc, leader_id),
        member_list: names,
        link_attack: p.mode_running(MODE_LINKATTACK),
        fortune_circle: p.mode_running(MODE_FORTUNECIRCLE),
        gift_box: p.mode_running(MODE_GIFTBOX),
        skill_full: leader_obj
            .and_then(|l| ctx.worlds.player(l))
            .is_some_and(crate::stats::sm::is_party_skill),
        exp_up: item(
            &[
                II_SYS_SYS_SCR_PARTYEXPUP01,
                II_SYS_SYS_SCR_PARTYEXPUP02,
                II_SYS_SYS_SCR_PARTYEXPUP01_01,
            ],
            BuffKind::Item2,
        ),
        skill_up: item(
            &[
                II_SYS_SYS_SCR_PARTYSKILLUP01,
                II_SYS_SYS_SCR_PARTYSKILLUP02,
                II_SYS_SYS_SCR_PARTYSKILLUP01_01,
            ],
            BuffKind::Item2,
        ),
    }
}

/// `PartyAllowJoinReq{mode}` (`OnPartyAllowJoin`): the leader sets whether the party recruits (`mode != 0`); the
/// leader and the members get `PartyAllowJoinNtf{allow}`. Only a leader of a party may.
pub fn on_party_allow_join(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::PartyAllowJoinReq,
) {
    if !ctx.cfg.rules.features.party_find {
        return;
    }
    let Some(character) = identity(ctx, who) else {
        return;
    };
    let Some(pid) = svc.social.party.party_of(character) else {
        return;
    };
    if svc.social.party.get(pid).and_then(Party::leader_id) != Some(character) {
        return;
    }
    let allow = m.mode != 0;
    if let Some(p) = ctx.player_mut(who).and_then(|m| m.player_mut()) {
        p.social.party_allow_join = allow;
    }
    to_members(svc, ctx, pid, pb::PartyAllowJoinNtf { allow });
}

/// `SetNaviPointReq{pos, target}` (`OnSetNaviPoint`): a minimap marker. `target` 0 (or the legacy `NULL_ID`) shows
/// it to every online party member, the sender included; a target must be a player in the sender's view set
/// (FIX VR-051) and then sees it together with the sender. The wire `obj_id` is the sender.
pub fn on_set_navi_point(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::SetNaviPointReq,
) {
    let Some(character) = identity(ctx, who) else {
        return;
    };
    let name = name_of(svc, character);
    let msg = pb::SetNaviPointNtf {
        pos: m.pos,
        sender_name: name,
    };
    let send = |ctx: &mut GameCtx<'_>, to: ObjId| {
        if let Some(sid) = ctx.session_of(PlayerRef(to)) {
            ctx.outbox.to_session(sid, who.0.0, msg.clone());
        }
    };
    if m.target == 0 || m.target == ObjId::NONE.0 {
        let Some(pid) = svc.social.party.party_of(character) else {
            return;
        };
        let objs: SmallVec<[ObjId; 8]> = svc
            .social
            .party
            .get(pid)
            .map(|p| p.online_objs().collect())
            .unwrap_or_default();
        for o in objs {
            send(ctx, o);
        }
        return;
    }
    let target = ObjId(m.target);
    let visible = ctx
        .worlds
        .core(who.0)
        .is_some_and(|c| c.viewers.contains(&target));
    if !visible || ctx.worlds.player(target).is_none() {
        return; // VR-051: nobody outside the view set
    }
    send(ctx, who.0);
    if target != who.0 {
        send(ctx, target);
    }
}

/// Slot item 12 (21 §4.9, `CPartyMng::PartyMapInfo`): every [`MAP_INFO_EVERY`]th call, for each member whose
/// position changed since the last send, `PartyMapInfo{index, pos}` goes to every other online member farther than
/// 32 m (squared 1024; the worlds are not compared, KEEP).
pub fn map_info(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    svc.social.party.map_info_calls += 1;
    if svc.social.party.map_info_calls < MAP_INFO_EVERY {
        return;
    }
    svc.social.party.map_info_calls = 0;
    for pid in svc.social.party.ids() {
        send_map_info(svc, ctx, pid);
    }
}

fn send_map_info(svc: &mut Services, ctx: &mut GameCtx<'_>, pid: PartyId) {
    let Some(p) = svc.social.party.get(pid) else {
        return;
    };
    // (index, avatar, position now) of the online members
    let members: Vec<(usize, ObjId, flyff_common::math::Vec3)> = p
        .members
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            let o = s.obj?;
            ctx.worlds.player(o).map(|m| (i, o, m.core.pos))
        })
        .collect();
    let mut moved: Vec<(usize, flyff_common::math::Vec3)> = Vec::new();
    for &(i, _, pos) in &members {
        let last = svc
            .social
            .party
            .get(pid)
            .and_then(|p| p.members.get(i))
            .map_or(flyff_common::math::Vec3::ZERO, |s| s.map_pos);
        if (pos - last).length_squared() > 0.0 {
            moved.push((i, pos));
        }
    }
    for (i, pos) in moved {
        if let Some(s) = svc
            .social
            .party
            .get_mut(pid)
            .and_then(|p| p.members.get_mut(i))
        {
            s.map_pos = pos;
        }
        let targets: Vec<ObjId> = members
            .iter()
            .filter(|(k, _, kpos)| *k != i && (*kpos - pos).length_squared() > MAP_INFO_FAR_SQ)
            .map(|(_, o, _)| *o)
            .collect();
        to_list(
            svc,
            ctx,
            &targets,
            pb::PartyMapInfo {
                member_index: i32::try_from(i).unwrap_or(i32::MAX),
                pos: Some(pb_vec(pos)),
            },
        );
    }
}
