//! The housing control command (Plan/23 §4.5 row 209; WP-GM-26): `/HousingGMRemoveAll (hgmra)`
//! (`FuncTextCmd.cpp:4912-4920`, `HousingDBCtrl.cpp:67-75`), over `event::housing::gm_remove_all` (housing).
//!
//! The plan's wording "send the full furniture list to the GM for removal" is the request; the database server removes
//! EVERY listed item of the GM's own room (`ProcessExpiredFurniture(now, bGMRemove = TRUE)`: uninstall, delete the row,
//! `HousingFurnitureList{info, add: false}`). A GM whose room is not loaded gets nothing. From the console the command is
//! `as <owner> /hgmra`.

use crate::gm::{CmdArgs, CmdCtx, CmdDone, CmdError, CmdResult};

/// Row 209 `/HousingGMRemoveAll` (needs a character): the own room is cleared; silent.
pub fn housing_gm_remove_all(cx: &mut CmdCtx<'_, '_>, _args: &mut CmdArgs<'_>) -> CmdResult {
    let who = cx.self_player.ok_or(CmdError::NeedsSelf)?;
    let owner = cx
        .ctx
        .entry(who)
        .map(|e| e.character)
        .ok_or(CmdError::NeedsSelf)?;
    let n = crate::event::housing::gm_remove_all(cx.svc, cx.ctx, owner);
    tracing::debug!(owner = owner.0, removed = n, "/hgmra");
    Ok(CmdDone::default())
}
