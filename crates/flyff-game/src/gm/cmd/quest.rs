//! The quest commands (Plan/23 §4.5 rows 57-61, §4.6 row 86, §4.7 row 135; WP-GM-25): `/BeginQuest` (57),
//! `/EndQuest` (58), `/RemoveQuest` (59), `/RemoveAllQuest` (60), `/RemoveCompleteQuest` (61), `/QuestState` (86) and
//! `/SetGuildQuest` (135).
//!
//! Each handler parses and hands over to the quest engine ([`crate::quest::gm`], [`crate::quest::guild`]); the quest
//! messages (`SetQuest`, `RemoveQuestNtf`, `SetGuildQuest`), the timer rules and the `quest.remove` audit are the
//! engine's. Numbers are `args.number()` (`atoi`; a missing argument reads as 0); the quest id is the legacy `WORD`:
//! `q = n as u16` (KEEP: `/bq 65537` is quest 1). A quest without a property is accepted as the original did (the
//! record has no property; the quest UI is hidden on the real data, PL-148). The legacy rows are silent on bad input
//! and so are these; the `gm.command` record (with the line and the target name) replaces the legacy
//! `uspLoggingQuest` rows 11 and 40.

use crate::ctx::PlayerRef;
use crate::gm::target::{self, Target};
use crate::gm::{CmdArgs, CmdCtx, CmdDone, CmdError, CmdResult};
use crate::quest::gm::{
    gm_begin, gm_end, gm_remove, gm_remove_all, gm_remove_completed, gm_set_state,
};
use crate::quest::guild::{GuildQuestError, guild_quest_set};
use flyff_common::defs::text::TID_DIAG_0061;

/// The legacy `WORD` of a quest id typed as a number (`/bq 65537` is quest 1).
fn quest_word(n: i32) -> u32 {
    u32::from(n as u16)
}

fn me(cx: &CmdCtx<'_, '_>) -> Result<PlayerRef, CmdError> {
    cx.self_player.ok_or(CmdError::NeedsSelf)
}

/// Row 57 `/BeginQuest (bq) <questId>` (`TextCmd_BeginQuest`): the quest of the issuer is set to state 0
/// (`SetQuest` to the client). No text, no start effects, no audit record of the quest.
pub fn begin_quest(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let q = quest_word(args.number());
    gm_begin(cx.svc, cx.ctx, who, q);
    Ok(CmdDone::default())
}

/// Row 58 `/EndQuest (eq) <questId>` (`TextCmd_EndQuest`): the quest of the issuer is set to `QS_END` (14); a quest
/// with a property that is not repeatable moves to the completed list.
pub fn end_quest(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let q = quest_word(args.number());
    gm_end(cx.svc, cx.ctx, who, q);
    Ok(CmdDone::default())
}

/// The target of `/rq` and `/qs`: the ONE token after the numbers. A token names an online player (ASCII
/// case-insensitive, PL-099): an unknown or offline name answers `DefinedText(TID_DIAG_0061, [token])` (the legacy
/// answered it for both) and the command stops; no token is the issuer itself (`NeedsSelf` for a console line without
/// `as`). `Ok(Err(done))` is that stop.
fn player_argument(
    cx: &mut CmdCtx<'_, '_>,
    args: &mut CmdArgs<'_>,
) -> Result<Result<(PlayerRef, Option<String>), CmdDone>, CmdError> {
    let token = args.token().to_owned();
    if token.is_empty() {
        return Ok(Ok((me(cx)?, None)));
    }
    match target::find(cx.svc, &token) {
        Target::Online { player, name, .. } => Ok(Ok((player, Some(name)))),
        Target::Unknown | Target::Offline { .. } => {
            cx.out.defined(TID_DIAG_0061, vec![token]);
            Ok(Err(CmdDone::default()))
        }
    }
}

/// Row 59 `/RemoveQuest (rq) <questId> [player]` (`TextCmd_RemoveQuest`): the quest leaves the active, completed and
/// checked lists of the issuer or of an online player; `RemoveQuestNtf{0, q}` and the `quest.remove` audit line
/// (`reason: "gm"`) are ALWAYS written, also for an absent quest. An unknown or offline name answers
/// `TID_DIAG_0061`.
pub fn remove_quest(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let q = quest_word(args.number());
    let (who, name) = match player_argument(cx, args)? {
        Ok(found) => found,
        Err(done) => return Ok(done),
    };
    gm_remove(cx.svc, cx.ctx, who, q);
    Ok(name.map_or_else(CmdDone::default, CmdDone::on))
}

/// Row 60 `/RemoveAllQuest (raq)` (`TextCmd_RemoveAllQuest`; the number the legacy read is ignored): the active,
/// completed and checked lists of the issuer are emptied; `RemoveQuestNtf{1, 0}`.
pub fn remove_all_quest(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let _ = args.number();
    gm_remove_all(cx.svc, cx.ctx, who);
    Ok(CmdDone::default())
}

/// Row 61 `/RemoveCompleteQuest (rcq)` (`TextCmd_RemoveCompleteQuest`; the number is ignored): the completed list of
/// the issuer is emptied; `RemoveQuestNtf{2, 0}`.
pub fn remove_complete_quest(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let who = me(cx)?;
    let _ = args.number();
    gm_remove_completed(cx.svc, cx.ctx, who);
    Ok(CmdDone::default())
}

/// Row 86 `/QuestState (qs) <questId> <state> [player]` (`TextCmd_QuestState`): the quest of the issuer or of an
/// online player is set to `state`, only for `0 <= state < 14` (anything else is silent). The name is resolved
/// BEFORE the state is checked, so an unknown or offline name answers `TID_DIAG_0061` also for a bad state (KEEP).
/// A GM state change zeroes the timer (PL-108).
pub fn quest_state(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let q = quest_word(args.number());
    let state = args.number();
    let (who, name) = match player_argument(cx, args)? {
        Ok(found) => found,
        Err(done) => return Ok(done),
    };
    gm_set_state(cx.svc, cx.ctx, who, q, state);
    Ok(name.map_or_else(CmdDone::default, CmdDone::on))
}

/// Row 135 `/SetGuildQuest (sgq) <guildName> <questId> <state>` (`TextCmd_SetGuildQuest`; no character needed), in the
/// order of the original: the guild quest must have a property (`ctx.data.quests.guild_quests`; a negative id or a
/// missing property answers the usage line, the original returned FALSE without a reply); an unknown guild (ASCII
/// case-insensitive, PL-058) and a state outside `0..=14` are silent; then [`guild_quest_set`] (the members get
/// `SetGuildQuest` even when nothing changed, ONE `UpsertGuildQuest`; a guild at 256 quests logs and stays silent).
pub fn set_guild_quest(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let name = args.token().to_owned();
    let q = args.number();
    let state = args.number();
    let Ok(quest) = u32::try_from(q) else {
        return Err(CmdError::Usage);
    };
    if !cx.ctx.data.quests.guild_quests.contains(quest) {
        return Err(CmdError::Usage);
    }
    let Some(guild) = cx.svc.social.guild.by_name(&name) else {
        return Ok(CmdDone::default());
    };
    let stored = cx.svc.social.guild.get(guild).map(|g| g.name.clone());
    match guild_quest_set(cx.svc, cx.ctx, guild, quest, state) {
        Ok(()) => {}
        Err(GuildQuestError::BadState | GuildQuestError::Full) => {}
        Err(e @ (GuildQuestError::NoGuild | GuildQuestError::BadQuest)) => {
            tracing::warn!(error = ?e, quest, "/sgq: the guild quest was not set");
        }
    }
    Ok(stored.map_or_else(CmdDone::default, CmdDone::on))
}
