//! The line-up (Plan/22 §11.3 step 3; `SecretRoom.cpp:540-585` `SetLineUpOpenWnd` / `SetLineUp`, `917-954`
//! `SetContLineUp`, `DPSrvr.cpp:11691-11717` the handler): while the manager is in `LineUp` the master of a tendering
//! guild picks the members who may fight.
//!
//! * `SecretRoomLineUpOpenWndReq` (menu 86) -> `SecretRoomLineUpOpenWndNtf{min_members, max_members, ids}`,
//! * `SecretRoomLineUpMember{ids}` (menu 86): the whole list replaces the stored one (`DbOp::ReplaceSecretRoomLineup`,
//!   a single op: no money moves).
//!
//! The shape of the list is checked first and a bad list is dropped as a whole without any answer (VR-001/004, the
//! legacy handler): more ids than `MaxGuildMemberNum`, an id of no known character, a duplicate. Then the legacy order:
//! `NOLINEUP`, `ISNOTGUILDMASTER`, the side (silent without one), `NOTOPENCONT`, no tender of the guild (silent), then
//! VR-065 (new, silent): an id that is not a CURRENT member of the master's guild drops the submission; then
//! `LINEUPMINMAX`, `LOWMEMBERLEVEL("<level>")` for an id that is offline or below `MinGuildMemberLevel`, and
//! `NOGUILDMASTER` unless the master of the GUILD is in the list (FIX: the legacy accepted the master of any guild).

use super::tender::{area_of, master_of, npc_ok, say, say_side};
use super::wire::online_obj;
use super::{SrMngState, SrSideState, cfg_of, tax_continent};
use crate::chat::ingress::notice;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::world::continent::CONT_NODATA;
use flyff_common::CharacterId;
use flyff_common::defs::neuz::MMI_SECRET_SELECTION;
use flyff_common::defs::text::{
    TID_GAME_SECRETROOM_ISNOTGUILDMASTER, TID_GAME_SECRETROOM_LINEUPCOMPLETE,
    TID_GAME_SECRETROOM_LINEUPMINMAX, TID_GAME_SECRETROOM_LOWMEMBERLEVEL,
    TID_GAME_SECRETROOM_NOGUILDMASTER, TID_GAME_SECRETROOM_NOLINEUP,
    TID_GAME_SECRETROOM_NOTENDERGUILD, TID_GAME_SECRETROOM_NOTOPENCONT,
};
use flyff_db::DbOp;
use flyff_proto::pb;

/// `SecretRoomLineUpOpenWndReq` (menu 86, `SetLineUpOpenWnd`): the manager must be in `LineUp` (`NOLINEUP`), the player
/// the master (`ISNOTGUILDMASTER`), the side at his position open (`NOTOPENCONT("\"side\"")`) and his guild a tender of
/// it (`NOTENDERGUILD("\"side\"")`); then `SecretRoomLineUpOpenWndNtf{min_members, max_members, stored line-up}`.
pub fn on_line_up_open_wnd(s: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    if !npc_ok(ctx, who, MMI_SECRET_SELECTION) {
        return;
    }
    if s.event.secret_room.state != SrMngState::LineUp {
        say(ctx, who, TID_GAME_SECRETROOM_NOLINEUP);
        return;
    }
    let Some((_, guild)) = master_of(s, ctx, who) else {
        say(ctx, who, TID_GAME_SECRETROOM_ISNOTGUILDMASTER);
        return;
    };
    let side = area_of(ctx, who);
    if side == CONT_NODATA {
        return;
    }
    let cfg = cfg_of(s);
    let (min_members, max_members) = (cfg.min_guild_member_num, cfg.max_guild_member_num);
    let sd = s.event.secret_room.side(side);
    if !sd.is_some_and(|sd| sd.state == SrSideState::Open) {
        say_side(ctx, who, TID_GAME_SECRETROOM_NOTOPENCONT, side);
        return;
    }
    let Some(tender) = sd.and_then(|sd| sd.tender(guild)) else {
        say_side(ctx, who, TID_GAME_SECRETROOM_NOTENDERGUILD, side);
        return;
    };
    let ids: Vec<u32> = tender.lineup.iter().map(|c| c.0).collect();
    crate::chat::send_to(
        ctx,
        who,
        pb::SecretRoomLineUpOpenWndNtf {
            min_members,
            max_members,
            ids,
        },
    );
}

/// `SecretRoomLineUpMember{ids}` (menu 86, `SetLineUp` + `SetContLineUp`; see the module documentation).
pub fn on_line_up_member(s: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, ids: Vec<u32>) {
    if !npc_ok(ctx, who, MMI_SECRET_SELECTION) {
        return;
    }
    let cfg = cfg_of(s);
    let (min_members, max_members, min_level) = (
        cfg.min_guild_member_num,
        cfg.max_guild_member_num,
        cfg.min_guild_member_level,
    );
    // the shape: dropped as a whole, without an answer
    if i64::try_from(ids.len()).unwrap_or(i64::MAX) > i64::from(max_members) {
        return;
    }
    let mut members: Vec<CharacterId> = Vec::with_capacity(ids.len());
    for id in &ids {
        let c = CharacterId(*id);
        if s.presence.get(c).is_none() || members.contains(&c) {
            return;
        }
        members.push(c);
    }
    if s.event.secret_room.state != SrMngState::LineUp {
        say(ctx, who, TID_GAME_SECRETROOM_NOLINEUP);
        return;
    }
    let Some((_, guild)) = master_of(s, ctx, who) else {
        say(ctx, who, TID_GAME_SECRETROOM_ISNOTGUILDMASTER);
        return;
    };
    let side = area_of(ctx, who);
    if side == CONT_NODATA {
        return;
    }
    let open = s
        .event
        .secret_room
        .side(side)
        .is_some_and(|sd| sd.state == SrSideState::Open);
    if !open {
        say_side(ctx, who, TID_GAME_SECRETROOM_NOTOPENCONT, side);
        return;
    }
    let Some(round) = s
        .event
        .secret_room
        .side(side)
        .and_then(|sd| sd.tender(guild))
        .map(|t| t.round)
    else {
        return; // no tender: silent (legacy)
    };
    // VR-065: every id must be a current member of the master's guild
    if members
        .iter()
        .any(|c| s.social.guild.guild_of(*c) != Some(guild))
    {
        tracing::debug!(
            guild = guild.0,
            "a secret-room line-up with a non-member was dropped (VR-065)"
        );
        return;
    }
    let n = i64::try_from(members.len()).unwrap_or(i64::MAX);
    if n < i64::from(min_members) || n > i64::from(max_members) {
        say(ctx, who, TID_GAME_SECRETROOM_LINEUPMINMAX);
        return;
    }
    for c in &members {
        let level = online_obj(&s.presence, ctx, *c)
            .and_then(|o| ctx.worlds.player(o))
            .map(|m| m.mv.level);
        if level.is_none_or(|l| l < min_level) {
            notice::defined(
                ctx,
                who,
                TID_GAME_SECRETROOM_LOWMEMBERLEVEL,
                vec![min_level.to_string()],
            );
            return;
        }
    }
    let master_listed = s
        .social
        .guild
        .get(guild)
        .and_then(|g| g.master())
        .is_some_and(|m| members.contains(&m));
    if !master_listed {
        say(ctx, who, TID_GAME_SECRETROOM_NOGUILDMASTER);
        return;
    }
    if let Some(t) = s
        .event
        .secret_room
        .sides
        .get_mut(&side)
        .and_then(|sd| sd.tenders.iter_mut().find(|t| t.guild == guild))
    {
        t.lineup.clone_from(&members);
    }
    say(ctx, who, TID_GAME_SECRETROOM_LINEUPCOMPLETE);
    ctx.persist.op(DbOp::ReplaceSecretRoomLineup {
        round,
        continent: tax_continent(side).code(),
        guild_id: guild,
        members,
    });
}
