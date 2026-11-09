//! Join step 13 `War` (Plan/21 §3.5, 11 §2.5): the running war of the joiner's guild. Also part of the re-attach
//! replay (15 §9.3).

use crate::outbox::Outbox;
use crate::world::join::{JoinContributor, JoinCtx, JoinStep};
use flyff_proto::pb;

/// Step 13 `War{GuildWarInfo}` for a member of a guild at war (a member who surrendered gets it too, his own flag is
/// 0 in `SpawnSelf.id_war`); nothing for everybody else and after the end of the war.
pub struct WarStep;

impl JoinContributor for WarStep {
    fn step(&self) -> JoinStep {
        JoinStep::War
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
        let social = &cx.svc.social;
        let Some(war) = social
            .guild
            .guild_record_of(character)
            .and_then(|g| g.war)
            .and_then(|id| social.guild_war.get(id))
        else {
            return;
        };
        out.to_session(
            cx.session,
            0,
            pb::War {
                war: Some(war.wire()),
            },
        );
    }
}
