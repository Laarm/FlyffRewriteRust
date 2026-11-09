//! The value semantics of the SM premium timers (Plan/16 §14.1, §2.6; WP-MOVER-16; `Mover.cpp:690-975`).
//!
//! `m_dwSMTime[SM_*]` holds, per kind, an **absolute expiry** in unix seconds (`SM_BANK15`, `SM_SHOUT30`,
//! `SM_ESCAPE`...), a **counter** of remaining online seconds (`SM_MAX_HP`, `SM_MAINTAIN_MP`, the element
//! stones...) or a **flag** that is never decremented (`SM_REVIVAL`). The server stores and sends these numbers
//! unchanged; this module is the pure arithmetic of `SetSMMode` and `SubSMMode`.

use crate::consts::{SM_MAX, SM_REVIVAL, sm_is_absolute};

/// How a timer's number is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmKind {
    /// Unix seconds of the expiry; ends by the wall clock, also while the player is offline.
    Absolute,
    /// Remaining online seconds, decremented once per second of the per-player timer.
    Counter,
    /// A charge that exists (1) or not (0); never decremented (`SM_REVIVAL`).
    Flag,
}

/// The kind of timer `sm` (`g_AddSMMode.bSMModetime[sm]` and the `SM_REVIVAL` exception of `SubSMMode`).
pub fn sm_kind(sm: usize) -> SmKind {
    if sm == SM_REVIVAL {
        SmKind::Flag
    } else if sm_is_absolute(sm) {
        SmKind::Absolute
    } else {
        SmKind::Counter
    }
}

/// `SetSMMode(sm, seconds)`: the stored number. Absolute kinds become `now + seconds` (unix seconds, saturating
/// at `u32::MAX`), counters and flags keep `seconds`.
pub fn stored_value(sm: usize, seconds: u32, now_unix: i64) -> u32 {
    match sm_kind(sm) {
        SmKind::Absolute => {
            let t = now_unix.saturating_add(i64::from(seconds));
            u32::try_from(t.max(0)).unwrap_or(u32::MAX)
        }
        SmKind::Counter | SmKind::Flag => seconds,
    }
}

/// What one pass of the per-second timer did to one timer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SmTick {
    /// The number after the pass.
    pub value: u32,
    /// It was positive before and is 0 now: the expiry actions and `SmMode{sm, 0}` follow.
    pub expired: bool,
}

/// One pass of `SubSMMode` over timer `sm` holding `value`: an inactive timer (0) is untouched; an absolute
/// timer ends when `value < now_unix` (strict); a counter loses 1; a flag keeps its value.
pub fn tick(sm: usize, value: u32, now_unix: i64) -> SmTick {
    if value == 0 || sm >= SM_MAX {
        return SmTick {
            value,
            expired: false,
        };
    }
    let new = match sm_kind(sm) {
        SmKind::Absolute => {
            if i64::from(value) < now_unix {
                0
            } else {
                value
            }
        }
        SmKind::Counter => value - 1,
        SmKind::Flag => value,
    };
    SmTick {
        value: new,
        expired: new == 0,
    }
}
