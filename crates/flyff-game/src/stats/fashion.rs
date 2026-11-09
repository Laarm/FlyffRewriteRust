//! Fashion and appearance switches, the state-mode word, scale and disguise messages (Plan/16 §13.7, §15,
//! WP-MOVER-17; ref/07b §7.2-7.4, `CDPSrvr::OnHideCoat` / `OnOptionEnableRenderMask` / `OnQueryEquipSetting` /
//! `OnStateMode`, `CMover::SetStateMode`).
//!
//! * `HideCoatReq{position, hide}` (`features.hide_fashion`): VR-004 `position` in `0..=4` (else ignored), an
//!   unchanged value is ignored, a change is stored in `appearance.hide_coat` (persisted as `hide_coat_mask`) and
//!   broadcast once as `HideCoatNtf` to the near set including the player.
//! * `OptionEnableRenderMask{enable}` and `QueryEquipSetting{allow}` flip only their own mode bit
//!   (`OPTION_DONT_RENDER_MASK_MODE`, `EQUIP_DENIAL_MODE`) and answer `ModifyMode` to the near set.
//! * [`set_state_mode`] / [`set_state_not_mode`] change the state-mode word (`m_dwStateMode`) and queue
//!   `StateModeNtf{state_mode, flag, item_prop_id}` to the near set; the sim's copy of the base-motion bit
//!   follows. The item cast of doc 19 (`item::usage::cast`) and the death of a casting player use them.
//! * [`emit_set_scale`], [`set_disguise`] / [`clear_disguise`]: the messages of the model scale and of the
//!   disguise buff.

use super::env::StatsCore;
use super::modes::{clear_mode_bits, set_mode_bits};
use super::out::Out;
use crate::mover::Mover;
use flyff_data::types::MoverId;
use flyff_proto::pb;
use flyff_rules::consts::{EQUIP_DENIAL_MODE, STATE_BASEMOTION_MODE};

/// `MODE_OPTION_DONT_RENDER_MASK` (`0x01000000`).
pub const OPTION_DONT_RENDER_MASK_MODE: u32 = 0x0100_0000;
/// The number of hideable fashion parts (`m_abHideCoat[5]`).
pub const HIDE_COAT_PARTS: i32 = 5;

/// `HideCoatReq` for `m`: returns `true` when the value changed (and `HideCoatNtf` was queued).
pub fn on_hide_coat(
    m: &mut Mover,
    c: &StatsCore,
    position: i32,
    hide: bool,
    out: &mut Out,
) -> bool {
    if !c.hide_fashion || !(0..HIDE_COAT_PARTS).contains(&position) {
        return false;
    }
    let Some(p) = m.player_mut() else {
        return false;
    };
    let slot = &mut p.appearance.hide_coat[position as usize];
    if *slot == hide {
        return false;
    }
    *slot = hide;
    out.near(pb::HideCoatNtf { position, hide });
    true
}

/// `OptionEnableRenderMask{enable}`: `true` clears `MODE_OPTION_DONT_RENDER_MASK`, `false` sets it; one
/// `ModifyMode` follows.
pub fn on_option_enable_render_mask(m: &mut Mover, c: &StatsCore, enable: bool, out: &mut Out) {
    if enable {
        clear_mode_bits(m, c, OPTION_DONT_RENDER_MASK_MODE, out);
    } else {
        set_mode_bits(m, c, OPTION_DONT_RENDER_MASK_MODE, out);
    }
}

/// `QueryEquipSetting{allow}`: `true` clears `EQUIP_DENIAL_MODE`, `false` sets it; one `ModifyMode` follows.
pub fn on_query_equip_setting(m: &mut Mover, c: &StatsCore, allow: bool, out: &mut Out) {
    if allow {
        clear_mode_bits(m, c, EQUIP_DENIAL_MODE, out);
    } else {
        set_mode_bits(m, c, EQUIP_DENIAL_MODE, out);
    }
}

/// Mirrors the base-motion bit into the shared sim (`IsStateMode(STATE_BASEMOTION_MODE)` blocks attacks).
fn sync_sim(m: &mut Mover) {
    m.sim.params.base_motion_mode = m.mv.state_mode & STATE_BASEMOTION_MODE != 0;
}

/// `CMover::SetStateMode(mode, flag)`: `m_dwStateMode |= bits`, then `StateModeNtf` to the near set.
/// `item_prop_id` is the item being cast (meaningful with `flag == STATEMODE_BASEMOTION_ON`, else pass 0).
pub fn set_state_mode(m: &mut Mover, bits: u32, flag: u32, item_prop_id: u32, out: &mut Out) {
    m.mv.state_mode |= bits;
    sync_sim(m);
    out.near(pb::StateModeNtf {
        state_mode: bits,
        flag,
        item_prop_id,
    });
}

/// `CMover::SetStateNotMode(mode, flag)`: `m_dwStateMode &= !bits`, then `StateModeNtf` (no item).
pub fn set_state_not_mode(m: &mut Mover, bits: u32, flag: u32, out: &mut Out) {
    m.mv.state_mode &= !bits;
    sync_sim(m);
    out.near(pb::StateModeNtf {
        state_mode: bits,
        flag,
        item_prop_id: 0,
    });
}

/// `StateModeReq{state_mode, flag}` (`OnStateMode`): when the mover is in a state mode and the flag is
/// `STATEMODE_BASEMOTION_CANCEL`, the base-motion bit goes off and `StateModeNtf{.., CANCEL}` follows. A cancel
/// without the bit set does nothing. Returns `true` when a cancel happened (doc 19 clears the cast then).
pub fn on_state_mode_req(m: &mut Mover, state_mode: u32, flag: u32, out: &mut Out) -> bool {
    if m.mv.state_mode & state_mode == 0 || flag != STATEMODE_CANCEL {
        return false;
    }
    set_state_not_mode(m, STATE_BASEMOTION_MODE, STATEMODE_CANCEL, out);
    true
}

/// `STATEMODE_BASEMOTION_CANCEL`.
pub const STATEMODE_CANCEL: u32 = 2;

/// `SetScale{factor}` to the near set: the model scale of a mover (collecting shrink, giant buffs). The
/// factor is the multiplier the client animates toward.
pub fn emit_set_scale(factor: f32, out: &mut Out) {
    out.near(pb::SetScale { factor });
}

/// The disguise of a mover: stores the model (`m_dwDisguise`) and queues `Disguise{mover_index}` to the near
/// set.
pub fn set_disguise(m: &mut Mover, mover_index: MoverId, out: &mut Out) {
    m.mv.disguise = Some(mover_index);
    out.near(pb::Disguise { mover_index });
}

/// The end of a disguise: clears `m_dwDisguise` and queues `NoDisguise` to the near set.
pub fn clear_disguise(m: &mut Mover, out: &mut Out) {
    m.mv.disguise = None;
    out.near(pb::NoDisguise {});
}
