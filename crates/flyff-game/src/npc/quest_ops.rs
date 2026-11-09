//! The quest helpers of the helper API (Plan/20 §6.5, #38-#51): the dispatch of the helper names to the quest
//! primitives of [`crate::quest::ops`] (§9.6) and the start effects and rewards of [`crate::quest::effects`]
//! (§9.6-§9.7). The record, the frames, the texts and the audit lines live in `ops`.

use super::host::{DialogHost, cre_guild_shim, int};
use crate::quest::flow::{self, FlowHost};
use crate::quest::ops::{self, RemoveReason};
use crate::quest::{QS_END, effects};
use flyff_common::ObjId;
use flyff_lua::{HVal, Helper, HostError};
use flyff_proto::pb;

pub use crate::quest::msg::quest_msg;

impl DialogHost<'_, '_> {
    /// The helpers of §6.5. `LaunchQuest` and the flows can fail with a host error; the others cannot.
    pub(super) fn quest_helper(&mut self, h: Helper, args: &[HVal]) -> Result<i64, HostError> {
        let Some(who) = self.who else { return Ok(0) };
        let q = int(args, 0);
        let quest = u32::try_from(q).unwrap_or(u32::MAX);
        use Helper as H;
        Ok(match h {
            H::LaunchQuest => {
                flow::quest_end(self, 0, false)?;
                0
            }
            H::GetQuestState => {
                if cre_guild_shim(&self.svc.npc, q) {
                    // PL-149: the guild creation quest reads as completed
                    i64::from(QS_END)
                } else {
                    self.player()
                        .and_then(|m| m.player())
                        .and_then(|p| p.quests.state_of(quest))
                        .map_or(-1, i64::from)
                }
            }
            H::IsSetQuest => {
                if cre_guild_shim(&self.svc.npc, q) {
                    1
                } else {
                    self.player().and_then(|m| m.player()).map_or(0, |p| {
                        i64::from(p.quests.is_active(quest) || p.quests.is_complete(quest))
                    })
                }
            }
            H::SetQuest => {
                ops::set_quest_started(self.svc, self.ctx, who, quest);
                0
            }
            H::SetQuestState => {
                // `__SetQuestState`: the text, audit and urgent save test the untruncated int; the record stores
                // the state as a C `BYTE` (KEEP): a state outside 0..=255 only sets the truncated byte
                let state = int(args, 1);
                if (0..=255).contains(&state) {
                    ops::set_quest_state(self.svc, self.ctx, who, quest, state as u8);
                } else {
                    ops::apply_set(self.svc, self.ctx, who, quest, state as u8);
                }
                0
            }
            H::BeginQuest => {
                effects::run_quest(self.svc, self.ctx, who, self.npc, quest);
                0
            }
            // the rewards without the end condition (KEEP); a full bag closes the dialog
            H::EndQuest => {
                FlowHost::end_quest(self, quest, false);
                0
            }
            H::RemoveQuest => {
                ops::remove_quest(self.svc, self.ctx, who, quest, RemoveReason::Script);
                0
            }
            H::IsGuildQuest => i64::from(self.guild_quest(who.0, quest).is_some()),
            H::GetGuildQuestState => self.guild_quest(who.0, quest).map_or(-1, i64::from),
            H::MonHuntStart | H::MonHuntStartParty => {
                // F-253 DEFER: the Wormon processors are not implemented, the slot is "busy"
                if self.svc.npc.stub_logged.insert(h) {
                    tracing::info!(
                        target: "lua.npc",
                        helper = h.name(),
                        "guild/party boss quests are deferred (F-253): the helper reports a busy slot"
                    );
                }
                0
            }
            H::DropQuestItem => self.drop_quest_item(who, q, int(args, 1)),
            H::ExpQuestEffect => {
                self.exp_quest_effect(int(args, 0), int(args, 1) != 0);
                0
            }
            _ => 0,
        })
    }

    /// The state of guild quest `q` of the player's guild (WP-NPC-16): the real guild service first
    /// ([`crate::quest::guild::player_guild_quest`]), the [`SocialView`](super::SocialView) seam only for a player
    /// without a real guild (the pattern of `npc/social.rs`).
    fn guild_quest(&self, player: ObjId, q: u32) -> Option<i32> {
        match crate::quest::guild::player_guild_quest(&*self.svc, &*self.ctx, player, q) {
            Some(state) => state,
            None => self.social().guild_quest_state(player, q),
        }
    }

    /// `CreateSfxObj{sfx}` on the NPC (`on_npc`) or the player, to everyone who sees it.
    fn exp_quest_effect(&mut self, sfx: i64, on_npc: bool) {
        let Ok(sfx_id) = u32::try_from(sfx) else {
            return;
        };
        let Some(who) = self.who else { return };
        let subject = if on_npc { self.npc } else { who.0 };
        self.ctx.broadcast(
            Some(&*self.svc),
            subject,
            crate::outbox::Audience::NearInclSelf,
            pb::CreateSfxObj {
                sfx_id,
                x: 0.0,
                y: 0.0,
                z: 0.0,
                flag: false,
            },
        );
    }
}
