//! GM mode flags and mute (Plan/23 §2; WP-GM-12; F-300, F-301, VR-085, KQ-22).
//!
//! * [`set_mode`] / [`change_mode`] are the one setter of the mode word for GM commands, items and the join: they
//!   apply the change, keep the sim's copy in step and fan out **one** `ModifyMode` to `NearInclSelf` (the viewers of
//!   the target and the target itself); nothing is sent when the word did not change (23 §2.4). The mode word is
//!   persisted by the character snapshot (`characters.mode_flags`, `FRESH` stripped), so the periodic save needs no
//!   dirty marker.
//! * [`mute_tick`] is the mute countdown (KQ-22): `mute_remaining_s` falls by one on the mover's "second tick"
//!   (`m_nCount & CF_SEC == 0`, every 16 ticks, about 1.067 s) and only while the avatar is in the world. It is
//!   called by `stats::tick::tick_tail_at`.
//! * The predicates at the end are the §2.3 enforcement contract that docs 17/18/19 call: each states the exact form
//!   (raw bit or `is_mode`) the legacy check used.

use crate::chat::ingress::mode::is_mode;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::mover::Mover;
use crate::stats::Out;
use crate::stats::env::refresh_sim_params;
use crate::stats::modes::{clear_mode_bits, set_mode_bits};
use flyff_common::CharacterId;
use flyff_common::gm::{Authority, ModeFlags};
use flyff_proto::pb;

/// The mover counter mask of the second tick (`CF_SEC`, `Mover.h`, 16 ticks per pass).
pub const CF_SEC: u32 = 15;

/// What changed a mode word (23 §2.4); only used for the trace line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModeSource {
    /// A GM command typed by (or run as) `issuer`.
    Command {
        /// The character whose authority ran the command.
        issuer: CharacterId,
    },
    /// An item with a GM text (23 §5).
    Item {
        /// The item id.
        item: u32,
    },
    /// The join normalisation.
    Join,
    /// The server itself.
    System,
}

/// `m.mode = (m.mode & !clear) | set`, the sim's copy updated, and **one** `ModifyMode` to `NearInclSelf` when the
/// word changed. Returns whether it changed. `false` as well when `target` is not a live player.
pub fn change_mode(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    target: PlayerRef,
    set: ModeFlags,
    clear: ModeFlags,
    by: ModeSource,
) -> bool {
    let core = svc.stats.handle();
    let Some(m) = ctx.worlds.player_mut(target.0) else {
        return false;
    };
    let old = m.mv.mode;
    // the stat functions keep the sim parameters in step; their own messages are dropped, one message goes out
    let mut scratch = Out::new();
    if clear.bits() != 0 {
        clear_mode_bits(m, &core, clear.bits(), &mut scratch);
    }
    if set.bits() != 0 {
        set_mode_bits(m, &core, set.bits(), &mut scratch);
    }
    let new = m.mv.mode;
    if new == old {
        return false;
    }
    refresh_sim_params(m, &core);
    tracing::debug!(
        target = target.0.0,
        old = format_args!("{old:#x}"),
        new = format_args!("{new:#x}"),
        by = ?by,
        "mode word changed"
    );
    let mut out = Out::new();
    out.near(pb::ModifyMode { mode: new });
    out.flush(ctx, target.0);
    true
}

/// Sets (`on`) or clears the bits of `bits` on `target` (23 §2.4).
pub fn set_mode(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    target: PlayerRef,
    bits: ModeFlags,
    on: bool,
    by: ModeSource,
) -> bool {
    if on {
        change_mode(svc, ctx, target, bits, ModeFlags::NONE, by)
    } else {
        change_mode(svc, ctx, target, ModeFlags::NONE, bits, by)
    }
}

/// The login normalisation of VR-085 (23 §2.5): below `GAMEMASTER` the `GM_ONLY` bits (`MATCHLESS`, `TRANSPARENT`,
/// `ONEKILL`, `MATCHLESS2`) are removed. `persist::load` applies the same rule on the saved word; this is the form
/// for a live change of authority (a demotion).
pub fn strip_on_join(mode: u32, authority: Authority) -> u32 {
    ModeFlags::from_bits(mode)
        .strip_gm_only_for(authority)
        .bits()
}

/// The mute countdown (KQ-22, 23 §2.6): on the mover's second tick (`count & CF_SEC == 0`) `mute_remaining_s`
/// falls by one. Only players in the world are ticked, so a logged-out character keeps its value.
pub fn mute_tick(m: &mut Mover) {
    if m.mv.count & CF_SEC != 0 {
        return;
    }
    if let Some(p) = m.player_mut()
        && p.misc.mute_remaining_s > 0
    {
        p.misc.mute_remaining_s -= 1;
    }
}

/// `/mute <name> <seconds>`: sets the remaining mute (0 unmutes). `false` when `m` is not a player.
pub fn set_mute(m: &mut Mover, seconds: u32) -> bool {
    match m.player_mut() {
        Some(p) => {
            p.misc.mute_remaining_s = seconds;
            true
        }
        None => false,
    }
}

// ---- the enforcement contract of 23 §2.3 --------------------------------------------------------------------

/// `MATCHLESS`: the raw bit; no damage, no knock-down (`PreProcessDamage`, `OBJMSG_FALL`; doc 17).
pub fn is_matchless(m: &Mover) -> bool {
    m.mv.mode & ModeFlags::MATCHLESS.bits() != 0
}

/// `MATCHLESS2`: the raw bit; HP never drops below 1 (doc 17).
pub fn is_matchless2(m: &Mover) -> bool {
    m.mv.mode & ModeFlags::MATCHLESS2.bits() != 0
}

/// `ONEKILL`: the raw bit; every hit hits and deals the target's current HP (doc 17).
pub fn is_onekill(m: &Mover) -> bool {
    m.mv.mode & ModeFlags::ONEKILL.bits() != 0
}

/// `NO_ATTACK` through `is_mode`: melee, magic and range messages and skills are refused (doc 17).
pub fn is_no_attack(m: &Mover) -> bool {
    is_mode(m, ModeFlags::NO_ATTACK)
}

/// `ITEM` through `is_mode`: no pick-up, `IsDropable` false (doc 19).
pub fn is_item_locked(m: &Mover) -> bool {
    is_mode(m, ModeFlags::ITEM)
}

/// `TRANSPARENT` through `is_mode` (so `CHS_INVISIBILITY` counts): monsters never acquire or keep the mover as a
/// target, the upgrade SFX is not broadcast (docs 18, 19).
pub fn is_transparent(m: &Mover) -> bool {
    is_mode(m, ModeFlags::TRANSPARENT)
}

/// `EXPUP_STOP`: the raw bit; the monster-kill EXP is 0 (doc 16).
pub fn is_exp_stopped(m: &Mover) -> bool {
    m.mv.mode & ModeFlags::EXPUP_STOP.bits() != 0
}

/// `TALK` (normal chat): silent drop (the say pipeline).
pub fn is_talk_blocked(m: &Mover) -> bool {
    m.mv.mode & ModeFlags::TALK.bits() != 0
}

/// `SHOUTTALK` (`/s`): silent drop (the shout pipeline).
pub fn is_shout_blocked(m: &Mover) -> bool {
    m.mv.mode & ModeFlags::SHOUTTALK.bits() != 0
}

/// `SAYTALK` (`/w`, `/say`): silent drop (doc 21's whisper and IM handlers).
pub fn is_whisper_blocked(m: &Mover) -> bool {
    m.mv.mode & ModeFlags::SAYTALK.bits() != 0
}
