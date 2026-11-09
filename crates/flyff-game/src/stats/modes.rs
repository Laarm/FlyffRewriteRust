//! The mode word and the tutorial state (Plan/16 §2.5, §15, WP-MOVER-12; F-255, VR-002, VR-004, AC-13).
//!
//! `ModifyMode{mode}` goes to the near set (a player subject included) after **every** change of `mv.mode` made
//! by this module, by the GM commands of doc 23 and by doc 17; it is never accepted from a client (the legacy
//! C2S `MODIFYMODE` is an internal server-to-server packet, VR-002: the proto has no C2S message for it). The
//! tutorial step is a plain stored integer with an echo to the owner.

use super::env::{StatsCore, refresh_sim_params};
use super::out::Out;
use crate::mover::Mover;
use flyff_proto::pb;
use flyff_rules::consts::{FREEPK_MODE, PVPCONFIRM_MODE};

/// `ModifyMode{mode}` to `NearInclSelf` with the current word (`g_UserMng.AddModifyMode`).
pub fn emit_modify_mode(m: &Mover, out: &mut Out) {
    out.near(pb::ModifyMode { mode: m.mv.mode });
}

/// `m_dwMode |= bits`, then exactly one `ModifyMode` (also when no bit changed: the legacy sends on every
/// call, KEEP). Returns `true` when the word changed. The sim's copy follows.
pub fn set_mode_bits(m: &mut Mover, c: &StatsCore, bits: u32, out: &mut Out) -> bool {
    let old = m.mv.mode;
    m.mv.mode |= bits;
    refresh_sim_params(m, c);
    emit_modify_mode(m, out);
    m.mv.mode != old
}

/// `m_dwMode &= !bits`, then exactly one `ModifyMode`. Returns `true` when the word changed.
pub fn clear_mode_bits(m: &mut Mover, c: &StatsCore, bits: u32, out: &mut Out) -> bool {
    let old = m.mv.mode;
    m.mv.mode &= !bits;
    refresh_sim_params(m, c);
    emit_modify_mode(m, out);
    m.mv.mode != old
}

/// `ChangeMode{mode, flag}` (`OnChangeMode`, 16 §15, F-178): the `mode` field is ignored; `flag` 0 sets
/// `FREEPK_MODE`, 1 clears it, 2 sets `PVPCONFIRM_MODE` (refuse PvP), 3 clears it, anything else changes nothing;
/// `ModifyMode` goes to the near set in every case (KEEP). Returns `true` when the word changed.
pub fn change_mode(m: &mut Mover, c: &StatsCore, flag: i32, out: &mut Out) -> bool {
    match flag {
        0 => set_mode_bits(m, c, FREEPK_MODE, out),
        1 => clear_mode_bits(m, c, FREEPK_MODE, out),
        2 => set_mode_bits(m, c, PVPCONFIRM_MODE, out),
        3 => clear_mode_bits(m, c, PVPCONFIRM_MODE, out),
        _ => {
            emit_modify_mode(m, out);
            false
        }
    }
}

/// `SetTutorialState{state}` to self with the stored step (`AddSetTutorialState`): the answer of
/// [`set_tutorial_state`], the join batch (15 §9) and the GM command `sts` (23).
pub fn emit_tutorial_state(m: &Mover, out: &mut Out) {
    if let Some(p) = m.player() {
        out.to_self(pb::SetTutorialState {
            state: p.progression.tutorial_state,
        });
    }
}

/// `TutorialState{state}` (VR-004, `OnTutorialState`; the legacy md5 token is dropped, AC-13): a state in
/// `0..=c.guide_max_seq` is stored in `progression.tutorial_state` (persisted by the next snapshot) and echoed
/// as `SetTutorialState` to **self only** (wire `obj_id` 0); anything else is ignored. Returns `true` when
/// stored.
pub fn set_tutorial_state(m: &mut Mover, c: &StatsCore, state: i32, out: &mut Out) -> bool {
    if state < 0 || state > c.guide_max_seq {
        return false;
    }
    let Some(p) = m.player_mut() else {
        return false;
    };
    p.progression.tutorial_state = state;
    emit_tutorial_state(m, out);
    true
}
