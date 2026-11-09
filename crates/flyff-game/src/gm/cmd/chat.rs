//! Notice commands (Plan/23 §4.4, §6.13): `/system (sys, announce)` (row 43).

use crate::chat::ingress::notice;
use crate::gm::{CmdArgs, CmdCtx, CmdDone, CmdResult};

/// Row 43 `/system`: `last_full`; a text of 512 bytes or more is dropped silently; trimmed on the right; a
/// `SystemNotice{text}` to every in-world session (the client prints it as `TID_ADMIN_ANNOUNCE` with the big
/// caption). A player's line is already cut to 120 characters (23 §6.1); console and admin lines are not.
pub fn system(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let text = args.last_full();
    if text.len() >= 512 {
        return Ok(CmdDone::default());
    }
    let text = text.trim_end();
    if text.is_empty() {
        return Ok(CmdDone::default());
    }
    notice::system(cx.svc, cx.ctx, text);
    Ok(CmdDone::default())
}
