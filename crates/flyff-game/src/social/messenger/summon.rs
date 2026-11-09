//! The friend summon scroll (Plan/21 §4.11, VR-027; ref/07b §18.1-§18.3; `MoverSkill.cpp:2696` and
//! `CDPSrvr::OnSummonFriend`, `OnSummonFriendConfirm`, `OnSummonFriendCancel`, `src:WORLDSERVER/DPSrvr.cpp:7954-8230`).
//!
//! The scroll `II_SYS_SYS_SCR_FRIENDSUMMON_A/B` is never consumed by its use: [`open`] shows the name dialog
//! (`Summon{Friend}`), the client answers with `SummonFriend{data, name}`, the named friend is asked
//! (`Summon{FriendConfirm}`) and his `SummonFriendConfirm` moves him to the summoner; one scroll of the SUMMONER is
//! consumed by that success.
//!
//! **The lock.** [`PendingSummon`] replaces the `m_bQuery` flag the original kept on the scroll: at most one per
//! summoner, no time limit (as the original lock), dropped by the confirm, the cancel, when either side leaves the
//! world, or when the scroll is gone at the confirm. **Fixes (VR-027, T17):** a confirm or a cancel works ONLY with a
//! pending summon whose target is the sender, whose summoner is the character of the named object and whose `data`
//! matches (the original trusted the object id: anybody could cancel for anybody and confirm anything). A refused
//! confirm keeps the pending summon: the friend may retry or cancel.
//!
//! **The word `data`.** It carries the inventory object id of the scroll in its LOW word, in both directions (`open`
//! writes it, `SummonFriend` and the friend's answers echo it, `party::summon::slot_of` reads it). The legacy server
//! packed it as `MAKELONG(ITYPE_ITEM, objid)`, the HIGH word, and the legacy client repacked it before replying; both
//! ends are ours here.

use super::model::PendingSummon;
use super::{actor, avatars_of};
use crate::chat::ingress::notice;
use crate::combat::pvp::summon_state;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::ops::{RemoveReason, remove_item};
use crate::social::party::handlers::gc1to1_participant;
use crate::social::party::summon::{
    format_s, in_rainbow_race, quoted, slot_of, state_tid, world_title,
};
use crate::social::party::wire::to_one;
use crate::world::ReplaceKind;
use flyff_common::defs::item::{II_SYS_SYS_SCR_FRIENDSUMMON_A, II_SYS_SYS_SCR_FRIENDSUMMON_B};
use flyff_common::defs::text::{
    TID_ERROR_SUMMONFRIEND_NOITEM, TID_ERROR_SUMMONFRIEND_NOUSER, TID_GAME_ATTACK_NOTUSE,
    TID_GAME_QUIZ_DO_NOT_USE, TID_GAME_RAINBOWRACE_NOTELEPORT, TID_GAME_STATE_NOTSUMMON,
    TID_GAME_STATE_NOTSUMMONOK, TID_GAME_STATE_NOTUSE, TID_GAME_SUMMON_FRIEND_CANCEL,
    TID_GAME_SUMMON_FRIEND_MY_NOUSE, TID_GAME_SUMMON_SUCCESS, TID_GAME_SUMMON_SUCCESS1,
    TID_GAME_SUMMONFRIEND_CONFIRM, TID_GAME_WORLD_NOTUSE,
};
use flyff_common::defs::world::WI_WORLD_QUIZ;
use flyff_common::{CharacterId, ObjId};
use flyff_proto::pb;

/// A name of this many bytes or more is ignored (`strlen( lpszPlayer ) >= MAX_NAME`, ref/07b §18.1).
pub const MAX_NAME_BYTES: usize = 64;

/// `true` for the two friend summon scrolls.
pub fn is_friend_scroll(item_id: u32) -> bool {
    item_id == II_SYS_SYS_SCR_FRIENDSUMMON_A || item_id == II_SYS_SYS_SCR_FRIENDSUMMON_B
}

/// The scroll at inventory object id `slot` of the avatar `obj` is usable and a friend summon scroll.
fn scroll_usable(ctx: &GameCtx<'_>, obj: ObjId, slot: u16) -> bool {
    ctx.worlds
        .player(obj)
        .and_then(|m| m.player())
        .and_then(|p| p.items.inventory.get_at_id(slot))
        .is_some_and(|it| it.is_usable() && is_friend_scroll(it.item_id))
}

/// `TID_ERROR_SUMMONFRIEND_NOITEM "<scroll name>" "<player>"` (the name of scroll A, as the original; nothing without
/// the item prop).
fn no_item(ctx: &mut GameCtx<'_>, to: PlayerRef, player: &str) {
    let Some(name) = ctx
        .data
        .item(II_SYS_SYS_SCR_FRIENDSUMMON_A)
        .map(|p| p.name.clone())
    else {
        return;
    };
    notice::defined(
        ctx,
        to,
        TID_ERROR_SUMMONFRIEND_NOITEM,
        vec![format!("\"{name}\""), format!("\"{player}\"")],
    );
}

/// `TID_GAME_STATE_NOTUSE "<text>"`.
fn not_use(ctx: &mut GameCtx<'_>, to: PlayerRef, text: &str) {
    notice::defined(ctx, to, TID_GAME_STATE_NOTUSE, vec![format!("\"{text}\"")]);
}

/// The player's summon state (`GetSummonState`), `None` for an avatar that is not a live player.
fn state_of(ctx: &GameCtx<'_>, obj: ObjId) -> Option<u8> {
    ctx.worlds.player(obj).map(summon_state)
}

/// The world and layer an avatar stands in.
fn place_of(
    ctx: &GameCtx<'_>,
    obj: ObjId,
) -> Option<(flyff_common::WorldId, flyff_common::LayerId)> {
    ctx.worlds.core(obj).map(|c| (c.world, c.layer))
}

// ---- the use of the scroll -----------------------------------------------------------------------------------------

/// The use of a friend summon scroll (`DoUseItemSystem`, `MoverSkill.cpp:2696`): never consumed. A busy player gets
/// `TID_GAME_STATE_NOTUSE "<state text>"`, a GC1:1 participant the same text for combat, everybody else the name
/// dialog `Summon{Friend{data, item_obj_id}}` with the scroll's inventory object id in the LOW word of `data`.
pub fn open(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, slot: u16) {
    let Some(state) = state_of(ctx, who.0) else {
        return;
    };
    if state != 0 {
        let text = ctx.data.texts.get(state_tid(state, false)).to_owned();
        not_use(ctx, who, &text);
    } else if gc1to1_participant(svc, ctx, who.0) {
        let text = ctx.data.texts.get(TID_GAME_ATTACK_NOTUSE).to_owned();
        not_use(ctx, who, &text);
    } else {
        to_one(
            ctx,
            who.0,
            pb::Summon {
                sub: Some(pb::summon::Sub::Friend(pb::summon::Friend {
                    data: u32::from(slot),
                    item_obj_id: u32::from(slot),
                })),
            },
        );
    }
}

// ---- SummonFriend --------------------------------------------------------------------------------------------------

/// `SummonFriend{data, name}` (`OnSummonFriend`, ref/07b §18.1): the checks in the order of the original, the first
/// failure wins. A name of 64 bytes or more is ignored; the sender's state (`TID_GAME_STATE_NOTUSE`); a usable friend
/// scroll at the slot of `data` and no summon of the sender in flight (`TID_ERROR_SUMMONFRIEND_NOITEM`); the target
/// online by name (`TID_ERROR_SUMMONFRIEND_NOUSER`); the target's state (the `NOTUSE1` texts); the target is the
/// sender; another world or layer; the rainbow race; the quiz world; and the target having blocked the sender -
/// `is_blocked` is TRUE for a non-friend, so a stranger cannot be summoned (the original gated it on the client).
/// Success records the pending summon, asks the friend (`Summon{FriendConfirm}`) and tells the sender
/// `TID_GAME_SUMMONFRIEND_CONFIRM`.
pub fn on_summon_friend(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::SummonFriend,
) {
    if m.name.len() >= MAX_NAME_BYTES {
        return;
    }
    let Some(a) = actor(ctx, who) else { return };
    let Some(state) = state_of(ctx, a.obj) else {
        return;
    };
    if state != 0 {
        let text = ctx.data.texts.get(state_tid(state, false)).to_owned();
        not_use(ctx, who, &text);
        return;
    }
    let slot = slot_of(m.data);
    if !scroll_usable(ctx, a.obj, slot) || svc.social.messenger.summon_of(a.character).is_some() {
        no_item(ctx, who, &m.name);
        return;
    }
    let target = svc
        .presence
        .by_name(&m.name)
        .map(|e| (e.id, e.name.clone()));
    let t_obj = target
        .as_ref()
        .and_then(|(c, _)| avatars_of(svc, ctx, [*c]).first().copied());
    let (Some((t_char, t_name)), Some(t_obj)) = (target, t_obj) else {
        notice::defined(
            ctx,
            who,
            TID_ERROR_SUMMONFRIEND_NOUSER,
            vec![format!("\"{}\"", m.name)],
        );
        return;
    };
    if let Some(ts) = state_of(ctx, t_obj).filter(|s| *s != 0) {
        let text = ctx.data.texts.get(state_tid(ts, true)).to_owned();
        not_use(ctx, who, &text);
        return;
    }
    if t_char == a.character {
        notice::defined(ctx, who, TID_GAME_SUMMON_FRIEND_MY_NOUSE, Vec::new());
        return;
    }
    // (no guild-quest region exists in the data)
    let (Some(mine), Some(theirs)) = (place_of(ctx, a.obj), place_of(ctx, t_obj)) else {
        return;
    };
    if mine != theirs {
        let text = format_s(
            ctx.data.texts.get(TID_GAME_WORLD_NOTUSE),
            &[&world_title(ctx, mine.0), &world_title(ctx, theirs.0)],
        );
        not_use(ctx, who, &text);
        return;
    }
    if in_rainbow_race(svc, ctx, a.obj) || in_rainbow_race(svc, ctx, t_obj) {
        notice::defined(ctx, who, TID_GAME_RAINBOWRACE_NOTELEPORT, Vec::new());
        return;
    }
    if mine.0.0 == WI_WORLD_QUIZ || theirs.0.0 == WI_WORLD_QUIZ {
        notice::defined(ctx, who, TID_GAME_QUIZ_DO_NOT_USE, Vec::new());
        return;
    }
    if svc.social.messenger.is_blocked(t_char, a.character) {
        notice::defined(
            ctx,
            who,
            TID_ERROR_SUMMONFRIEND_NOUSER,
            vec![format!("\"{}\"", m.name)],
        );
        return;
    }
    svc.social.messenger.begin_summon(PendingSummon {
        summoner: a.character,
        target: t_char,
        data: m.data,
        slot,
    });
    to_one(
        ctx,
        t_obj,
        pb::Summon {
            sub: Some(pb::summon::Sub::FriendConfirm(pb::summon::FriendConfirm {
                summoner_obj: a.obj.0,
                data: m.data,
                summoner_name: a.name,
                world_name: world_title(ctx, mine.0),
            })),
        },
    );
    notice::defined(
        ctx,
        who,
        TID_GAME_SUMMONFRIEND_CONFIRM,
        vec![format!("\"{t_name}\"")],
    );
}

// ---- the answer of the friend --------------------------------------------------------------------------------------

/// The pending summon that `(summoner_obj, data)` of a friend's answer names, when it targets `me` (VR-027): the
/// summoner must be the character of the named object and the word must match. Anything else is `None`.
fn matching(
    svc: &Services,
    ctx: &GameCtx<'_>,
    me: CharacterId,
    summoner_obj: u32,
    data: u32,
) -> Option<PendingSummon> {
    let summoner = ctx
        .worlds
        .player(ObjId(summoner_obj))?
        .player()?
        .ident
        .character_id;
    svc.social
        .messenger
        .summon_of(summoner)
        .copied()
        .filter(|p| p.target == me && p.data == data)
}

/// `SummonFriendConfirm{summoner_obj, data}` (`OnSummonFriendConfirm`, ref/07b §18.2): only with a pending summon of
/// that summoner for the sender with that word. The states of both sides (the friend gets `TID_GAME_STATE_NOTSUMMONOK`,
/// the summoner `TID_GAME_STATE_NOTSUMMON` with the mirrored text), the scroll at the slot (gone: the summon is
/// dropped and the summoner gets `TID_ERROR_SUMMONFRIEND_NOITEM`), the world and layer, the rainbow race and the quiz
/// world refuse with texts to both and keep the summon. Success moves the friend to the summoner (`REPLACE_NORMAL`),
/// drops the summon, consumes ONE scroll of the SUMMONER and tells both (`TID_GAME_SUMMON_SUCCESS1` /
/// `TID_GAME_SUMMON_SUCCESS`).
pub fn on_summon_friend_confirm(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::SummonFriendConfirm,
) {
    let Some(a) = actor(ctx, who) else { return };
    let Some(p) = matching(svc, ctx, a.character, m.summoner_obj, m.data) else {
        return;
    };
    let summoner = PlayerRef(ObjId(m.summoner_obj));
    let (Some(my_state), Some(their_state)) = (state_of(ctx, a.obj), state_of(ctx, summoner.0))
    else {
        return;
    };
    // the friend's own state: his texts are the "self" variants, the summoner's the "other" ones
    if my_state != 0 {
        let (mine, theirs) = (
            quoted(ctx, state_tid(my_state, false)),
            quoted(ctx, state_tid(my_state, true)),
        );
        notice::defined(ctx, who, TID_GAME_STATE_NOTSUMMONOK, vec![mine]);
        notice::defined(ctx, summoner, TID_GAME_STATE_NOTSUMMON, vec![theirs]);
        return;
    }
    if their_state != 0 {
        let (mine, theirs) = (
            quoted(ctx, state_tid(their_state, true)),
            quoted(ctx, state_tid(their_state, false)),
        );
        notice::defined(ctx, who, TID_GAME_STATE_NOTSUMMONOK, vec![mine]);
        notice::defined(ctx, summoner, TID_GAME_STATE_NOTSUMMON, vec![theirs]);
        return;
    }
    if !scroll_usable(ctx, summoner.0, p.slot) {
        svc.social.messenger.end_summon(p.summoner);
        no_item(ctx, summoner, &a.name);
        return;
    }
    // (no guild-quest region exists in the data)
    let (Some(mine), Some(theirs)) = (place_of(ctx, a.obj), place_of(ctx, summoner.0)) else {
        return;
    };
    if mine != theirs {
        let to_friend = format_s(
            ctx.data.texts.get(TID_GAME_WORLD_NOTUSE),
            &[&world_title(ctx, mine.0), &world_title(ctx, theirs.0)],
        );
        let to_summoner = format_s(
            ctx.data.texts.get(TID_GAME_WORLD_NOTUSE),
            &[&world_title(ctx, theirs.0), &world_title(ctx, mine.0)],
        );
        notice::defined(
            ctx,
            who,
            TID_GAME_STATE_NOTSUMMONOK,
            vec![format!("\"{to_friend}\"")],
        );
        notice::defined(
            ctx,
            summoner,
            TID_GAME_STATE_NOTSUMMON,
            vec![format!("\"{to_summoner}\"")],
        );
        return;
    }
    if in_rainbow_race(svc, ctx, a.obj) || in_rainbow_race(svc, ctx, summoner.0) {
        notice::defined(ctx, who, TID_GAME_RAINBOWRACE_NOTELEPORT, Vec::new());
        notice::defined(ctx, summoner, TID_GAME_RAINBOWRACE_NOTELEPORT, Vec::new());
        return;
    }
    if mine.0.0 == WI_WORLD_QUIZ || theirs.0.0 == WI_WORLD_QUIZ {
        notice::defined(ctx, who, TID_GAME_QUIZ_DO_NOT_USE, Vec::new());
        notice::defined(ctx, summoner, TID_GAME_QUIZ_DO_NOT_USE, Vec::new());
        return;
    }
    let Some(pos) = ctx.worlds.core(summoner.0).map(|c| c.pos) else {
        return;
    };
    if let Err(e) = ctx
        .worlds
        .replace(a.obj, theirs.0, pos, theirs.1, ReplaceKind::Normal)
    {
        tracing::debug!(error = %e, "friend summon move refused");
        return;
    }
    svc.social.messenger.end_summon(p.summoner);
    remove_item(svc, ctx, summoner, p.slot, 1, RemoveReason::Consume);
    let summoner_name = ctx
        .worlds
        .player(summoner.0)
        .and_then(|mv| mv.player())
        .map(|pl| pl.ident.name.clone())
        .unwrap_or_default();
    notice::defined(
        ctx,
        who,
        TID_GAME_SUMMON_SUCCESS1,
        vec![format!("\"{summoner_name}\"")],
    );
    notice::defined(
        ctx,
        summoner,
        TID_GAME_SUMMON_SUCCESS,
        vec![format!("\"{}\"", a.name)],
    );
}

/// `SummonFriendCancel{summoner_obj, data}` (`OnSummonFriendCancel`, ref/07b §18.3): the friend declines. The same
/// matching rule as the confirm (a third player or a stale word is ignored); the summon is dropped and the summoner
/// gets `TID_GAME_SUMMON_FRIEND_CANCEL "<friend>"`.
pub fn on_summon_friend_cancel(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::SummonFriendCancel,
) {
    let Some(a) = actor(ctx, who) else { return };
    let Some(p) = matching(svc, ctx, a.character, m.summoner_obj, m.data) else {
        return;
    };
    svc.social.messenger.end_summon(p.summoner);
    notice::defined(
        ctx,
        PlayerRef(ObjId(m.summoner_obj)),
        TID_GAME_SUMMON_FRIEND_CANCEL,
        vec![format!("\"{}\"", a.name)],
    );
}
