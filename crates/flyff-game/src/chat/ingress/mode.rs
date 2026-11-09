//! Mode bits and mute at the chat checkpoints (Plan/23 §2.2, §2.6, §6.4): `is_mode`, `mute_text`,
//! `is_shout_full` as free functions over a mover (WP-GM-06).

use crate::ctx::{GameCtx, PlayerRef};
use crate::mover::Mover;
use flyff_common::defs::attribute::{CHS_INVISIBILITY, DST_CHRSTATE};
use flyff_common::defs::text::{TID_GAME_LORD_SKILL_RAGE_E001, TID_GAME_STATE_MUTE};
use flyff_common::gm::{Authority, ModeFlags};

/// `CMover::IsMode` (`Mover.h:1756`, 23 §2.2). `bits` may be a combo: all of them must be set, with two
/// exceptions:
///
/// * KEEP-QUIRK (`__HACK_0516`): exactly `MATCHLESS`, `ONEKILL` or `MATCHLESS2` asked of an `AUTH_GENERAL`
///   (`'F'`) mover is `false` (`'G'`, `'H'`, `'J'` are not affected);
/// * a `TRANSPARENT` question is `true` for a mover with `CHS_INVISIBILITY` in `DST_CHRSTATE`.
pub fn is_mode(m: &Mover, bits: ModeFlags) -> bool {
    // monsters and NPCs have authority GENERAL too, but nothing sets these bits on them
    let general = m
        .player()
        .is_none_or(|p| p.ident.authority == Authority::GENERAL);
    if general
        && (bits == ModeFlags::MATCHLESS
            || bits == ModeFlags::ONEKILL
            || bits == ModeFlags::MATCHLESS2)
    {
        return false;
    }
    if bits.contains(ModeFlags::TRANSPARENT)
        && (m.mv.dst.table.adj(DST_CHRSTATE) as u32) & CHS_INVISIBILITY != 0
    {
        return true;
    }
    ModeFlags::from_bits(m.mv.mode).contains(bits)
}

/// `is_mode` of a live player by reference (`false` when `who` is not a live player).
pub fn is_mode_of(ctx: &GameCtx<'_>, who: PlayerRef, bits: ModeFlags) -> bool {
    ctx.player(who).is_some_and(|m| is_mode(m, bits))
}

/// `CUser::GetMuteText` (`User.cpp:8243`, 23 §2.6): `TID_GAME_STATE_MUTE` while `mute_remaining_s > 0`, else
/// `TID_GAME_LORD_SKILL_RAGE_E001` with the lord-skill buff `II_SYS_SYS_LS_RAGE`, else `None`.
pub fn mute_text(m: &Mover) -> Option<u32> {
    let p = m.player()?;
    if p.misc.mute_remaining_s > 0 {
        return Some(TID_GAME_STATE_MUTE);
    }
    if super::markup::has_item_buff(m, flyff_common::defs::item::II_SYS_SYS_LS_RAGE) {
        return Some(TID_GAME_LORD_SKILL_RAGE_E001);
    }
    None
}

/// `IsShoutFull` (23 §6.4): an active `SM_SHOUT15`, `SM_SHOUT30` or `SM_SHOUT001` timer. One definition: 16's
/// `sm::is_shout_full` over `sm::sm_active` (WP-GM-14: the §6.0 seam is no longer a direct `SmState` read).
pub fn is_shout_full(m: &Mover) -> bool {
    crate::stats::sm::is_shout_full(m)
}
