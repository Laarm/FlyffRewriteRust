//! The tax control command (Plan/23 §4.5 row 188; WP-GM-26): `/TaxApplyNow (tan)` (`FuncTextCmd.cpp:4682-4689`,
//! `Tax.cpp:199-229, 380-389`), over `event::tax::gm` (tax-tower).
//!
//! Silent, as the legacy. While the lord election votes nothing happens (KEEP: the legacy tested the vote before the GM
//! flag); otherwise tax-tower's forced weekly change runs at once: the counters are paid first, the lord becomes the next
//! owner of the whole world, the next owners and rates apply, the round counts up.

use crate::event::tax::gm::{self, TaxGmError};
use crate::gm::{CmdArgs, CmdCtx, CmdDone, CmdResult};

/// The action of `/tan` and `/content tax next`.
pub(super) fn apply_now(cx: &mut CmdCtx<'_, '_>) -> Result<(), TaxGmError> {
    gm::apply_now(cx.svc, cx.ctx)
}

/// The action of `/content tax pay`: the daily payout of the current counters now, without its date guard.
pub(super) fn pay_now(cx: &mut CmdCtx<'_, '_>) {
    gm::pay_now(cx.svc, cx.ctx);
}

/// Row 188 `/TaxApplyNow`: the weekly tax change now. In the vote it is a debug log and no output.
pub fn tax_apply_now(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    if let Err(e) = apply_now(cx) {
        tracing::debug!(error = %e, "/tan was refused");
    }
    Ok(CmdDone::default())
}
