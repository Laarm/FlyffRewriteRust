//! The ten client messages of the NPC mini-games (Plan/22 §16; src:WORLDSERVER/DPSrvr.cpp:9569-9716), routed from
//! the `ClientCall` overrides of `crate::handlers`.
//!
//! Each handler first requires the NPC menu of its game within reach (VR-012, PL-107: the original checked none of
//! them) and ignores the request silently otherwise; `FiveSystemDestroyWnd` is exempt, so a player who walked away
//! from the NPC still gets the stakes back. The shape checks (VR-064) and the game logic are in the game modules.

use super::{alphabet, at_npc, five_system, kawibawibo, reassemble};
use crate::ctx::{GameCtx, PlayerRef, Services};
use flyff_common::defs::neuz::{MMI_FINDWORD, MMI_FIVESYSTEM, MMI_KAWIBAWIBO, MMI_REASSEMBLE};
use flyff_proto::pb;

/// `KAWIBAWIBO_START`.
pub fn on_kawibawibo_start(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    _m: pb::KawibawiboStart,
) {
    if at_npc(ctx, who, MMI_KAWIBAWIBO) {
        kawibawibo::start(svc, ctx, who);
    }
}

/// `KAWIBAWIBO_GETITEM`.
pub fn on_kawibawibo_get_item(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    _m: pb::KawibawiboGetItem,
) {
    if at_npc(ctx, who, MMI_KAWIBAWIBO) {
        kawibawibo::get_item(svc, ctx, who);
    }
}

/// `REASSEMBLE_OPENWND`.
pub fn on_reassemble_open_wnd(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    _m: pb::ReassembleOpenWnd,
) {
    if at_npc(ctx, who, MMI_REASSEMBLE) {
        reassemble::open_wnd(svc, ctx, who);
    }
}

/// `REASSEMBLE_START`: nine inventory object ids.
pub fn on_reassemble_start(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::ReassembleStart,
) {
    if at_npc(ctx, who, MMI_REASSEMBLE) {
        reassemble::start(svc, ctx, who, &m.item_obj_ids);
    }
}

/// `ALPHABET_OPENWND`.
pub fn on_alphabet_open_wnd(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    _m: pb::AlphabetOpenWnd,
) {
    if at_npc(ctx, who, MMI_FINDWORD) {
        alphabet::open_wnd(svc, ctx, who);
    }
}

/// `ALPHABET_START`: the offered question and five inventory object ids.
pub fn on_alphabet_start(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::AlphabetStart,
) {
    if at_npc(ctx, who, MMI_FINDWORD) {
        alphabet::start(svc, ctx, who, m.question_id, &m.item_obj_ids);
    }
}

/// `FIVESYSTEM_OPENWND`.
pub fn on_five_system_open_wnd(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    _m: pb::FiveSystemOpenWnd,
) {
    if at_npc(ctx, who, MMI_FIVESYSTEM) {
        five_system::open_wnd(svc, ctx, who);
    }
}

/// `FIVESYSTEM_DESTROYWND`: refunds the stakes; no NPC needed.
pub fn on_five_system_destroy_wnd(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    _m: pb::FiveSystemDestroyWnd,
) {
    five_system::close_five_system(svc, ctx, who);
}

/// `FIVESYSTEM_BET`.
pub fn on_five_system_bet(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::FiveSystemBet,
) {
    if at_npc(ctx, who, MMI_FIVESYSTEM) {
        five_system::bet(svc, ctx, who, m.bet_num, m.bet_penya);
    }
}

/// `FIVESYSTEM_START`.
pub fn on_five_system_start(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    _m: pb::FiveSystemStart,
) {
    if at_npc(ctx, who, MMI_FIVESYSTEM) {
        five_system::start(svc, ctx, who);
    }
}
