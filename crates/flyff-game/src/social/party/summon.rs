//! The party summon scroll (Plan/21 §4.11, VR-027; WP-SOCIAL-07; `src:WORLDSERVER/DPSrvr.cpp:8232-8455`,
//! ref/07b §18.4-18.5).
//!
//! `SummonParty{data}`: the leader of a party uses an `II_SYS_SYS_SCR_PARTYSUMMON` scroll. Every other member that is
//! online gets the scroll's buff (the acceptance window) and a `Summon{party_confirm}` message; the leader buffs
//! himself and the scroll is used up. `SummonPartyConfirm{leader_obj, data}`: a member that still holds the buff and
//! passes the state checks is replaced to the leader's place (`REPLACE_FORCE`) and loses the buff.
//!
//! **Fixes.** The scroll must really be the party-summon scroll (the original applied the buff of any usable item the
//! client named, VR-027). The window belongs to each member: a confirm needs the member's own buff, so a member
//! cannot be teleported twice by one scroll, nor one that joined after it was read. The leader is the one the party
//! record says, not the object id the client names alone.

use super::identity;
use crate::buff::{BuffKind, has_item};
use crate::chat::ingress::notice;
use crate::combat::pvp::summon_state;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::ops::{RemoveReason, remove_item};
use crate::outbox::Audience;
use crate::world::ReplaceKind;
use flyff_common::defs::item::II_SYS_SYS_SCR_PARTYSUMMON;
use flyff_common::defs::text::{
    TID_ERROR_SUMMONFRIEND_NOUSER, TID_ERROR_SUMMONPARTY_NOITEM, TID_ERROR_SUMMONPARTY_NOTTIME,
    TID_GAME_ATTACK_NOTUSE, TID_GAME_DIE_NOTUSE, TID_GAME_FLY_NOTUSE, TID_GAME_LIMITED_USE,
    TID_GAME_NOPARTY, TID_GAME_QUIZ_DO_NOT_USE, TID_GAME_STATE_NOTSUMMONOK, TID_GAME_STATE_NOTUSE,
    TID_GAME_SUMMON_SUCCESS, TID_GAME_SUMMON_SUCCESS1, TID_GAME_SUMMONFRIEND_CONFIRM,
    TID_GAME_TRADE_NOTUSE, TID_GAME_VENDOR_NOTUSE, TID_GAME_WORLD_NOTUSE,
    TID_GAME_WORLDLEADER_NOTUSE,
};
use flyff_common::defs::world::WI_WORLD_QUIZ;
use flyff_common::{ObjId, WorldId};
use flyff_proto::pb;
use flyff_rules::consts::NULL_ID;

/// The id of an inventory slot in a packed scroll word (`LOWORD( dwData )`).
pub(crate) fn slot_of(data: u32) -> u16 {
    (data & 0xFFFF) as u16
}

/// The `TID_GAME_*_NOTUSE` text of a summon state; `other` selects the "of the other one" variant (`+ 1`, the ids
/// are paired in the text table). A duel reads as combat.
pub(crate) fn state_tid(state: u8, other: bool) -> u32 {
    let base = match state {
        1 => TID_GAME_TRADE_NOTUSE,
        2 => TID_GAME_DIE_NOTUSE,
        3 => TID_GAME_VENDOR_NOTUSE,
        4 | 6 => TID_GAME_ATTACK_NOTUSE,
        5 => TID_GAME_FLY_NOTUSE,
        _ => return 0,
    };
    if other { base + 1 } else { base }
}

/// `"<text>"` as the argument of a `DefinedText`.
pub(crate) fn quoted(ctx: &GameCtx<'_>, tid: u32) -> String {
    format!("\"{}\"", ctx.data.texts.get(tid))
}

/// `CString::Format` with `%s` arguments, in order (the world names of `TID_GAME_WORLD_NOTUSE`).
pub(crate) fn format_s(template: &str, args: &[&str]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut args = args.iter();
    let mut rest = template;
    while let Some(i) = rest.find("%s") {
        out.push_str(&rest[..i]);
        out.push_str(args.next().copied().unwrap_or(""));
        rest = &rest[i + 2..];
    }
    out.push_str(rest);
    out
}

/// The title of a world (empty for an unknown one).
pub(crate) fn world_title(ctx: &GameCtx<'_>, id: WorldId) -> String {
    ctx.worlds
        .world(id)
        .map(|w| w.title.clone())
        .unwrap_or_default()
}

/// `g_UserMng.AddCreateSfxObj( mover, PARTYSUMMON.dwSfxObj3 )`: the scroll's effect on `target`, seen by everyone
/// near it.
fn sfx(ctx: &mut GameCtx<'_>, target: ObjId) {
    let Some(sfx_id) = ctx
        .data
        .item(II_SYS_SYS_SCR_PARTYSUMMON)
        .and_then(|p| p.sfx_obj.get(2).copied())
        .filter(|&s| s != NULL_ID)
    else {
        return;
    };
    let Some(pos) = ctx.worlds.core(target).map(|c| c.pos) else {
        return;
    };
    ctx.broadcast(
        None,
        target,
        Audience::NearInclSelf,
        pb::CreateSfxObj {
            sfx_id,
            x: pos.x,
            y: pos.y,
            z: pos.z,
            flag: false,
        },
    );
}

/// The rainbow-race entry list (doc 22 §14, task rainbow-race): `CRainbowRaceMng::IsEntry` of the avatar.
pub(crate) fn in_rainbow_race(svc: &Services, ctx: &GameCtx<'_>, who: ObjId) -> bool {
    crate::event::rainbow::is_entry_obj(svc, ctx, who)
}

/// `SummonParty{data}` (`OnSummonParty`): see the module documentation.
pub fn on_summon_party(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::SummonParty,
) {
    let Some(character) = identity(ctx, who) else {
        return;
    };
    let Some(state) = ctx.worlds.player(who.0).map(summon_state) else {
        return;
    };
    if state != 0 {
        let reason = quoted(ctx, state_tid(state, false));
        notice::defined(ctx, who, TID_GAME_STATE_NOTUSE, vec![reason]);
        return;
    }
    // (no guild-quest region exists in the data)
    if in_rainbow_race(svc, ctx, who.0) {
        notice::defined(
            ctx,
            who,
            flyff_common::defs::text::TID_GAME_RAINBOWRACE_NOTELEPORT,
            Vec::new(),
        );
        return;
    }
    let slot = slot_of(m.data);
    let scroll = ctx
        .worlds
        .player(who.0)
        .and_then(|mv| mv.player())
        .and_then(|p| p.items.inventory.get_at_id(slot))
        .filter(|it| it.is_usable() && it.item_id == II_SYS_SYS_SCR_PARTYSUMMON)
        .map(|it| it.item_id);
    let Some(item) = scroll else {
        let name = ctx
            .data
            .item(II_SYS_SYS_SCR_PARTYSUMMON)
            .map(|p| p.name.clone());
        if let Some(name) = name {
            notice::defined(
                ctx,
                who,
                TID_ERROR_SUMMONPARTY_NOITEM,
                vec![format!("\"{name}\"")],
            );
        }
        return;
    };
    let Some(pid) = svc.social.party.party_of(character) else {
        return;
    };
    let Some(party) = svc.social.party.get(pid) else {
        return;
    };
    if party.leader_id() != Some(character) {
        return; // only the leader may; silent
    }
    if has_item(ctx, who.0, item) {
        notice::defined(ctx, who, TID_GAME_LIMITED_USE, Vec::new());
        return;
    }
    let Some((world, world_name)) = ctx
        .worlds
        .core(who.0)
        .map(|c| (c.world, world_title(ctx, c.world)))
    else {
        return;
    };
    // the members after the leader, with their avatars (None = offline)
    let others: Vec<(flyff_common::CharacterId, Option<ObjId>)> = party
        .members
        .iter()
        .skip(1)
        .map(|s| (s.id, s.obj.filter(|_| s.is_online())))
        .collect();
    if let Some(p) = svc.social.party.get_mut(pid) {
        p.summon_world = Some(world);
    }
    for (c, obj) in others {
        match obj.filter(|o| ctx.worlds.player(*o).is_some()) {
            Some(member) => {
                let _ = crate::buff::apply_item(svc, ctx, member, item, Some(who.0));
                super::wire::to_one(
                    ctx,
                    member,
                    pb::Summon {
                        sub: Some(pb::summon::Sub::PartyConfirm(pb::summon::PartyConfirm {
                            leader_obj: who.0.0,
                            data: m.data,
                            world_name: world_name.clone(),
                        })),
                    },
                );
                sfx(ctx, member);
                let name = ctx
                    .worlds
                    .player(member)
                    .and_then(|mv| mv.player())
                    .map(|p| p.ident.name.clone())
                    .unwrap_or_default();
                notice::defined(
                    ctx,
                    who,
                    TID_GAME_SUMMONFRIEND_CONFIRM,
                    vec![format!("\"{name}\"")],
                );
            }
            None => {
                let name = super::wire::name_of(svc, c);
                notice::defined(
                    ctx,
                    who,
                    TID_ERROR_SUMMONFRIEND_NOUSER,
                    vec![format!("\"{name}\"")],
                );
            }
        }
    }
    let _ = crate::buff::apply_item(svc, ctx, who.0, item, None);
    remove_item(svc, ctx, who, slot, 1, RemoveReason::Consume);
    sfx(ctx, who.0);
}

/// `SummonPartyConfirm{leader_obj, data}` (`OnSummonPartyConfirm`): see the module documentation. Every refusal is a
/// text to the member; the order of the checks is the original's.
pub fn on_summon_party_confirm(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::SummonPartyConfirm,
) {
    let Some(character) = identity(ctx, who) else {
        return;
    };
    let Some(pid) = svc.social.party.party_of(character) else {
        notice::defined(ctx, who, TID_GAME_NOPARTY, Vec::new());
        return;
    };
    let leader = ObjId(m.leader_obj);
    let Some(party) = svc.social.party.get(pid) else {
        return;
    };
    // the named object must be the leader of the sender's party and hold the scroll's buff, and the member its own
    let is_leader =
        party.leader().and_then(|l| l.obj) == Some(leader) && ctx.worlds.player(leader).is_some();
    if !is_leader
        || !has_item(ctx, leader, II_SYS_SYS_SCR_PARTYSUMMON)
        || !has_item(ctx, who.0, II_SYS_SYS_SCR_PARTYSUMMON)
    {
        notice::defined(ctx, who, TID_ERROR_SUMMONPARTY_NOTTIME, Vec::new());
        return;
    }
    if leader == who.0 {
        return;
    }
    let party_world = party.summon_world;
    let (Some(member_state), Some(leader_state)) = (
        ctx.worlds.player(who.0).map(summon_state),
        ctx.worlds.player(leader).map(summon_state),
    ) else {
        return;
    };
    if member_state != 0 && member_state != 5 {
        let reason = quoted(ctx, state_tid(member_state, false));
        notice::defined(ctx, who, TID_GAME_STATE_NOTSUMMONOK, vec![reason]);
        return;
    }
    if leader_state != 0 {
        let reason = quoted(ctx, state_tid(leader_state, true));
        notice::defined(ctx, who, TID_GAME_STATE_NOTSUMMONOK, vec![reason]);
        return;
    }
    // (no guild-quest region exists in the data)
    let (Some(mine), Some(theirs)) = (
        ctx.worlds.core(who.0).map(|c| (c.world, c.layer)),
        ctx.worlds.core(leader).map(|c| (c.world, c.layer, c.pos)),
    ) else {
        return;
    };
    let refuse = |ctx: &mut GameCtx<'_>, text: String| {
        notice::defined(
            ctx,
            who,
            TID_GAME_STATE_NOTSUMMONOK,
            vec![format!("\"{text}\"")],
        );
    };
    if Some(mine.0) != party_world {
        // the original answers only when the party's world still exists
        if let Some(pw) = party_world.filter(|w| ctx.worlds.world(*w).is_some()) {
            let t = format_s(
                ctx.data.texts.get(TID_GAME_WORLD_NOTUSE),
                &[&world_title(ctx, mine.0), &world_title(ctx, pw)],
            );
            refuse(ctx, t);
        }
        return;
    }
    if party_world != Some(theirs.0) {
        if party_world.is_some_and(|w| ctx.worlds.world(w).is_some()) {
            let t = ctx.data.texts.get(TID_GAME_WORLDLEADER_NOTUSE).to_owned();
            refuse(ctx, t);
        }
        return;
    }
    if theirs.1 != mine.1 {
        let name = world_title(ctx, mine.0);
        let t = format_s(ctx.data.texts.get(TID_GAME_WORLD_NOTUSE), &[&name, &name]);
        refuse(ctx, t);
        return;
    }
    if in_rainbow_race(svc, ctx, who.0) {
        notice::defined(
            ctx,
            who,
            flyff_common::defs::text::TID_GAME_RAINBOWRACE_NOTELEPORT,
            Vec::new(),
        );
        return;
    }
    if mine.0.0 == WI_WORLD_QUIZ || theirs.0.0 == WI_WORLD_QUIZ {
        notice::defined(ctx, who, TID_GAME_QUIZ_DO_NOT_USE, Vec::new());
        return;
    }
    // success: the move first (a refused move changes nothing), then the buff goes
    if let Err(e) = ctx
        .worlds
        .replace(who.0, theirs.0, theirs.2, theirs.1, ReplaceKind::Force)
    {
        tracing::debug!(error = %e, "party summon move refused");
        return;
    }
    crate::buff::remove(
        svc,
        ctx,
        who.0,
        BuffKind::Item,
        II_SYS_SYS_SCR_PARTYSUMMON as u16,
    );
    let leader_name = ctx
        .worlds
        .player(leader)
        .and_then(|mv| mv.player())
        .map(|p| p.ident.name.clone())
        .unwrap_or_default();
    let member_name = ctx
        .worlds
        .player(who.0)
        .and_then(|mv| mv.player())
        .map(|p| p.ident.name.clone())
        .unwrap_or_default();
    notice::defined(
        ctx,
        who,
        TID_GAME_SUMMON_SUCCESS1,
        vec![format!("\"{leader_name}\"")],
    );
    notice::defined(
        ctx,
        PlayerRef(leader),
        TID_GAME_SUMMON_SUCCESS,
        vec![format!("\"{member_name}\"")],
    );
    sfx(ctx, who.0);
}
