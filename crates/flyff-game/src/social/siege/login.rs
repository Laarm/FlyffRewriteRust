//! The login state of the siege (Plan/21 §7.8; `CDPDatabaseClient::OnJoin`, `src:WORLDSERVER/DPDatabaseClient.cpp:1140-1170`):
//! the five `GuildCombat` messages of EVERY join batch, in the order of the original, and of the re-attach replay
//! (the same registry). Two more messages follow while a siege runs (`GC_GUILDSTATUS`, `GC_WARPLAYERLIST`): a second
//! contributor of siege-b in the same [`JoinStep::GuildCombat`].
//!
//! 1. `GC_ISREQUEST{the player's guild is an active bidder}`,
//! 2. `GC_GCSTATE{state, war_state}`,
//! 3. `GC_ENTERTIME{ms}` when the row executed last was the one that led into the maintenance minute (its `param` is
//!    103), else `GC_NEXTTIMESTATE{ms, param}`, with `ms = step_due - now` saturated at 0 (FIX: the original wrapped
//!    the `u32` and read row -1 before the first row; with nothing executed the message is `{0, 0}`),
//! 4. `GC_WINGUILD{siege_id, win_guild, win_count}`,
//! 5. `GC_BESTPLAYER{id}`.

use crate::outbox::Outbox;
use crate::world::join::{JoinContributor, JoinCtx, JoinStep};
use flyff_proto::pb;
use pb::guild_combat::Sub;

/// The siege messages of the join batch (position 17 of 11 §2.5).
pub struct GuildCombatStep;

impl JoinContributor for GuildCombatStep {
    fn step(&self) -> JoinStep {
        JoinStep::GuildCombat
    }

    fn write(&self, cx: &JoinCtx<'_>, out: &mut Outbox) {
        let s = &cx.svc.social.siege;
        let guild = cx
            .worlds
            .player(cx.player)
            .and_then(|m| m.player())
            .and_then(|p| cx.svc.social.guild.guild_of(p.ident.character_id));
        let mut send = |sub: Sub| out.to_session(cx.session, 0, pb::GuildCombat { sub: Some(sub) });
        send(Sub::GcIsRequest(pb::guild_combat::GcIsRequest {
            request: guild.is_some_and(|g| s.is_bidder(g)),
        }));
        send(Sub::GcGcState(pb::guild_combat::GcGcState {
            state: s.state().code(),
            war_state: s.war_state().code(),
        }));
        let ms =
            u32::try_from(s.step_due().0.saturating_sub(cx.clock.game_ms.0)).unwrap_or(u32::MAX);
        let last_param = s
            .step()
            .checked_sub(1)
            .and_then(|i| s.script().row(i))
            .map_or(0, |r| r.param);
        if s.step() == 0 {
            send(Sub::GcNextTimeState(pb::guild_combat::GcNextTimeState {
                time_ms: 0,
                state: 0,
            }));
        } else if last_param == super::SiegeWarState::Maintenance.code() as u32 {
            send(Sub::GcEnterTime(pb::guild_combat::GcEnterTime {
                time_ms: ms,
            }));
        } else {
            send(Sub::GcNextTimeState(pb::guild_combat::GcNextTimeState {
                time_ms: ms,
                state: last_param,
            }));
        }
        send(Sub::GcWinGuild(pb::guild_combat::GcWinGuild {
            combat_index: i32::try_from(s.siege_id()).unwrap_or(i32::MAX),
            win_guild: s.win_guild().map_or(0, |g| g.0),
            win_count: s.win_count(),
        }));
        send(Sub::GcBestPlayer(pb::guild_combat::GcBestPlayer {
            id_player: s.best_player().map_or(0, |c| c.0),
        }));
    }
}
