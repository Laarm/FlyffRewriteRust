//! Guild quests (Plan/20 §9.11, WP-NPC-16): the per-guild quest states of the original's `CGuild::SetQuest` /
//! `RemoveQuest` (`_Common/guild.cpp` 912-982), the messages `SetGuildQuest` / `RemoveGuildQuest`, and the two
//! script helpers `IsGuildQuest` / `GetGuildQuestState` that read them.
//!
//! **State.** [`Guild::quests`](crate::social::guild::Guild) is the live list, sorted by quest id (the database load
//! order, `guild_quests` has one row per guild **and** quest). The setter upserts one entry and persists ONE plain
//! `DbOp::UpsertGuildQuest` (a single-row change, as guild-core's `persist` does; no money or item moves, so no atomic
//! batch), then tells every online member. The remover deletes the entry, the row and tells the members. A restart
//! restores the list through `Guild::from_load`; a disbanded guild takes its rows with it (`ON DELETE CASCADE`).
//! `GuildFull.quests` ([`crate::social::guild::wire::full`]) carries the live entries, so a member who logs in or who
//! joins later sees them. No audit event: the GM path writes `gm.command`.
//!
//! **Original and changes.** `SetQuest` sent `SETGUILDQUEST` to every online member also when nothing changed (KEEP).
//! `RemoveQuest` returned `TRUE` BEFORE its member loop, so `REMOVEGUILDQUEST` was never sent (FIX: the work package
//! requires the broadcast). The original counted every slot ever allocated against `MAX_GUILD_QUEST` (freed `nId = -1`
//! slots included, because the size test ran before the free-slot search) and serialised the freed slots too: only
//! live entries count and travel here (invisible FIX).
//!
//! **The `/sgq` contract** (`TextCmd_SetGuildQuest`, `_Interface/FuncTextCmd.cpp` 1262-1290; the handler is the code
//! of the GM task, this module is its API), in the original order: (1) the property check on
//! `ctx.data.quests.guild_quests` (a missing property answers the usage line); (2) the guild by name
//! (`svc.social.guild.by_name`, ASCII case-folded; unknown is silent, the handler never reaches
//! [`guild_quest_set`]); (3) the state (`GuildQuestError::BadState` is silent); (4) [`guild_quest_set`].

use super::{QS_BEGIN, QS_END};
use crate::ctx::{GameCtx, Services};
use crate::social::guild::{GuildService, wire};
use flyff_common::{GuildId, ObjId};
use flyff_db::{DbOp, GuildQuestRecord};
use flyff_proto::pb;

/// `MAX_GUILD_QUEST` (`_Common/guildquest.h`): live quests per guild.
pub const MAX_GUILD_QUEST: usize = 256;

/// Why a guild quest was not set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuildQuestError {
    /// The guild id is unknown.
    NoGuild,
    /// The quest id does not fit the wire and the database (above `u16::MAX`).
    BadQuest,
    /// The state is outside `QS_BEGIN..=QS_END` (0..=14); the GM command maps it to the legacy silence.
    BadState,
    /// A NEW entry while [`MAX_GUILD_QUEST`] live entries exist (`OVER MAX_GUILD_QUEST` is logged).
    Full,
}

/// `CGuild::SetQuest(q, state)` (`guild.cpp` 912-947): upserts the entry of `guild`, persists it, and sends
/// `SetGuildQuest{quest_id, state}` to every online member (also when the state did not change). A new entry gets
/// `started_at = now`; an existing one keeps it; `ended_at` is `Some(now)` exactly when `state` is `QS_END`. `now` is
/// the wall time (the columns are `UnixTime`, not game time).
pub fn guild_quest_set(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    guild: GuildId,
    q: u32,
    state: i32,
) -> Result<(), GuildQuestError> {
    let Some(g) = svc.social.guild.get(guild) else {
        return Err(GuildQuestError::NoGuild);
    };
    let Ok(quest) = u16::try_from(q) else {
        return Err(GuildQuestError::BadQuest);
    };
    if !(i32::from(QS_BEGIN)..=i32::from(QS_END)).contains(&state) {
        return Err(GuildQuestError::BadState);
    }
    if g.quests.len() >= MAX_GUILD_QUEST && !g.quests.iter().any(|r| r.quest_id == quest) {
        tracing::error!(
            guild = guild.0,
            quest = q,
            "OVER MAX_GUILD_QUEST: the guild quest is not set"
        );
        return Err(GuildQuestError::Full);
    }
    let now = ctx.clock.unix_ms.to_unix_time();
    let ended_at = (state == i32::from(QS_END)).then_some(now);
    let Some(g) = svc.social.guild.get_mut(guild) else {
        return Err(GuildQuestError::NoGuild);
    };
    let rec = match g.quests.binary_search_by_key(&quest, |r| r.quest_id) {
        Ok(i) => {
            let r = &mut g.quests[i];
            r.state = state;
            r.ended_at = ended_at;
            r.clone()
        }
        Err(i) => {
            let r = GuildQuestRecord {
                guild_id: guild,
                quest_id: quest,
                state,
                started_at: now,
                ended_at,
            };
            g.quests.insert(i, r.clone());
            r
        }
    };
    ctx.persist.op(DbOp::UpsertGuildQuest(rec));
    wire::to_members(
        svc,
        ctx,
        guild,
        pb::SetGuildQuest {
            quest_id: i32::from(quest),
            state,
        },
    );
    Ok(())
}

/// `CGuild::RemoveQuest(q)` (`guild.cpp` 949-967) with the broadcast the original never reached (FIX): deletes the
/// entry, the row, and sends `RemoveGuildQuest{quest_id}` to the online members. `false` (nothing sent or written)
/// when the guild or the entry does not exist.
pub fn guild_quest_remove(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    guild: GuildId,
    q: u32,
) -> bool {
    let Ok(quest) = u16::try_from(q) else {
        return false;
    };
    let Some(g) = svc.social.guild.get_mut(guild) else {
        return false;
    };
    let Ok(i) = g.quests.binary_search_by_key(&quest, |r| r.quest_id) else {
        return false;
    };
    g.quests.remove(i);
    ctx.persist.op(DbOp::DeleteGuildQuest {
        guild_id: guild,
        quest_id: quest,
    });
    wire::to_members(
        svc,
        ctx,
        guild,
        pb::RemoveGuildQuest {
            quest_id: i32::from(quest),
        },
    );
    true
}

/// The state of guild quest `q` of `guild` (`None` = no such entry, or no such guild).
pub fn state_of(svc: &GuildService, guild: GuildId, q: u32) -> Option<i32> {
    let quest = u16::try_from(q).ok()?;
    let quests = &svc.get(guild)?.quests;
    let i = quests.binary_search_by_key(&quest, |r| r.quest_id).ok()?;
    quests.get(i).map(|r| r.state)
}

/// The state of guild quest `q` for the guild of the avatar `who`, for the script helpers `IsGuildQuest` and
/// `GetGuildQuestState`. The outer `None` means "no real guild" (the avatar has none, or is unknown): the helper then
/// falls back to the `SocialView` seam of the NPC runtime. `Some(None)` is a real guild without the quest.
pub fn player_guild_quest(
    svc: &Services,
    ctx: &GameCtx<'_>,
    who: ObjId,
    q: u32,
) -> Option<Option<i32>> {
    let c = ctx.worlds.player(who)?.player()?.ident.character_id;
    let guild = svc.social.guild.guild_of(c)?;
    Some(state_of(&svc.social.guild, guild, q))
}
