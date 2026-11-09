//! The client-message entry points of the stats area (Plan/16 §7.2, §15; WP-MOVER-07, WP-MOVER-12): thin
//! `(svc, ctx, who, message)` functions that `ClientCall` forwards to. Each resolves the player's mover, runs the
//! pure function of this module, then flushes the queued messages through the outbox.

use super::modes::{change_mode, set_tutorial_state};
use super::out::Out;
use super::points::{inc_stat_level, modify_status};
use super::progression::toggle_exp_stop;
use crate::ctx::{GameCtx, PlayerRef, Services};
use flyff_proto::pb;

/// `IncStatLevel` (VR-004).
pub fn on_inc_stat_level(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::IncStatLevel,
) {
    let mut out = Out::new();
    if let Some(mv) = ctx.worlds.mover_mut(who.0) {
        inc_stat_level(mv, svc.stats.core(), m.stat_id, &mut out);
    }
    out.flush(ctx, who.0);
}

/// `ModifyStatus` (VR-087); applied requests run the honor hooks of doc 21 (`CheckHonorStat`, `HonorListAck`).
pub fn on_modify_status(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::ModifyStatus,
) {
    let mut out = Out::new();
    let applied = ctx.worlds.mover_mut(who.0).is_some_and(|mv| {
        modify_status(mv, svc.stats.core(), [m.str, m.sta, m.dex, m.int], &mut out)
    });
    out.flush(ctx, who.0);
    if applied {
        crate::social::hooks::honor_recheck(svc, ctx, who, true);
    }
}

/// `TutorialState` (VR-004).
pub fn on_tutorial_state(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::TutorialState,
) {
    let mut out = Out::new();
    if let Some(mv) = ctx.worlds.mover_mut(who.0) {
        set_tutorial_state(mv, svc.stats.core(), m.state, &mut out);
    }
    out.flush(ctx, who.0);
}

/// `ExpStop` (16 §8.5, `features.exp_stop`): toggles `MODE_EXPUP_STOP`, answered with the literal `Text` to self and
/// `ModifyMode` to the near set; ignored when the feature is off.
pub fn on_exp_stop(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, _m: pb::ExpStop) {
    let mut out = Out::new();
    if let Some(mv) = ctx.worlds.player_mut(who.0) {
        toggle_exp_stop(mv, svc.stats.core(), &mut out);
    }
    out.flush(ctx, who.0);
}

/// `HideCoatReq` (VR-004, `features.hide_fashion`): see [`super::fashion::on_hide_coat`].
pub fn on_hide_coat(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, m: pb::HideCoatReq) {
    let mut out = Out::new();
    if let Some(mv) = ctx.worlds.player_mut(who.0) {
        super::fashion::on_hide_coat(mv, svc.stats.core(), m.position, m.hide, &mut out);
    }
    out.flush(ctx, who.0);
}

/// `OptionEnableRenderMask`: flips `MODE_OPTION_DONT_RENDER_MASK`, answers `ModifyMode` to the near set.
pub fn on_option_enable_render_mask(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::OptionEnableRenderMask,
) {
    let mut out = Out::new();
    if let Some(mv) = ctx.worlds.player_mut(who.0) {
        super::fashion::on_option_enable_render_mask(mv, svc.stats.core(), m.enable, &mut out);
    }
    out.flush(ctx, who.0);
}

/// `QueryEquipSetting`: flips `EQUIP_DENIAL_MODE`, answers `ModifyMode` to the near set.
pub fn on_query_equip_setting(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::QueryEquipSetting,
) {
    let mut out = Out::new();
    if let Some(mv) = ctx.worlds.player_mut(who.0) {
        super::fashion::on_query_equip_setting(mv, svc.stats.core(), m.allow, &mut out);
    }
    out.flush(ctx, who.0);
}

/// `ChangeMode` (16 §15, F-178): `flag` 0..3 set and clear `FREEPK` / `PVPCONFIRM`, other flags change nothing, and
/// `ModifyMode` goes to the near set in every case.
pub fn on_change_mode(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    m: pb::ChangeMode,
) {
    let mut out = Out::new();
    if let Some(mv) = ctx.worlds.player_mut(who.0) {
        change_mode(mv, svc.stats.core(), m.flag, &mut out);
    }
    out.flush(ctx, who.0);
}

/// `UpdateJob` (the fast job change, 16 §11.4): see [`super::jobs::on_update_job`].
pub fn on_update_job(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, m: pb::UpdateJob) {
    super::jobs::on_update_job(svc, ctx, who, m);
}
