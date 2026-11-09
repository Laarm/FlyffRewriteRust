//! The party command (Plan/23 §4, Plan/21 §4.5; WP-GM-20): `/PartyLevel (plv)` (row 65).

use crate::gm::{CmdArgs, CmdCtx, CmdDone, CmdError, CmdResult};
use crate::social::party::points::set_party_level;

/// The largest party level the command accepts (the original took any `DWORD`; a level this high is already beyond
/// every table and keeps the integer arithmetic of the advanced formula well inside `i32`).
pub const MAX_GM_PARTY_LEVEL: i32 = 1000;

/// Row 65 `/PartyLevel (plv) <level> [points=100]`: sets the level and the skill points of the issuer's own party
/// and keeps its EXP (the EXP is cut below the next level's requirement); a points value of 0 (or none) means 100.
/// Without a party nothing happens. Every online member gets `PartyExp`.
pub fn party_level(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let me = cx.self_player.ok_or(CmdError::NeedsSelf)?;
    let level = args.number().clamp(0, MAX_GM_PARTY_LEVEL);
    let mut point = args.number();
    if point == 0 {
        point = 100;
    }
    let point = point.max(0);
    let character = cx
        .ctx
        .worlds
        .player(me.0)
        .and_then(|m| m.player())
        .map(|p| p.ident.character_id);
    if let Some(pid) = character.and_then(|c| cx.svc.social.party.party_of(c)) {
        set_party_level(cx.svc, cx.ctx, pid, level, point);
    }
    Ok(CmdDone::default())
}
