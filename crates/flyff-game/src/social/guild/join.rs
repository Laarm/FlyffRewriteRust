//! The guild steps of the join batch (Plan/21 §3.5, 11 §2.5): step 11 `AllGuilds` (always, an empty list included)
//! and step 12 `Guild` (the joiner's own guild). Both are also part of the re-attach replay. Step 13 `War` is
//! guild-war's, step 7 `QueryPlayerDataNtf` messenger-core's.

use super::wire::{all_guilds, full};
use crate::outbox::Outbox;
use crate::world::join::{JoinContributor, JoinCtx, JoinStep};
use flyff_proto::pb;

/// Step 11 `AllGuilds{next_id, every guild}`: the client keeps the list for the guild names above heads and the war
/// and siege windows (KEEP: every guild, named or not, in id order).
pub struct AllGuildsStep;

impl JoinContributor for AllGuildsStep {
    fn step(&self) -> JoinStep {
        JoinStep::AllGuilds
    }

    fn write(&self, cx: &JoinCtx<'_>, out: &mut Outbox) {
        out.to_session(cx.session, 0, all_guilds(&cx.svc.social.guild));
    }
}

/// Step 12 `Guild{GuildFull}` for a member; nothing for a character without a guild.
pub struct GuildStep;

impl JoinContributor for GuildStep {
    fn step(&self) -> JoinStep {
        JoinStep::Guild
    }

    fn write(&self, cx: &JoinCtx<'_>, out: &mut Outbox) {
        let Some(character) = cx
            .worlds
            .player(cx.player)
            .and_then(|m| m.player())
            .map(|p| p.ident.character_id)
        else {
            return;
        };
        let guilds = &cx.svc.social.guild;
        let Some(g) = guilds.guild_of(character).and_then(|id| guilds.get(id)) else {
            return;
        };
        out.to_session(
            cx.session,
            0,
            pb::Guild {
                guild: Some(full(g)),
            },
        );
    }
}
