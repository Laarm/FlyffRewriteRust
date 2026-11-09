//! The party and guild helpers (Plan/20 §6.7, D20-16): thin bindings over doc 21.
//!
//! This document implements no party or guild rule. The helpers ask the REAL services first (WP-NPC-11,
//! WP-SOCIAL-03/10): `social::party::npc_facts` for the party, `social::guild::{npc_facts, is_party_guild,
//! create_for_npc}` for the guild, and fall back to the [`SocialView`] seam when the real service has no answer (a
//! player without a real party or guild; [`NoSocial`], the default, answers "no party, no guild" and `CreateGuild`
//! returns 0; the tests bind a fake view). Helena's creation is `create_for_npc` called synchronously inside the helper:
//! the script ran `RemoveGold(3 000 000)` just before in the same handler, so the leader snapshot of the
//! `GuildCreate` batch holds the debit and the penya and the guild persist together (PL-091); a refusal after the
//! debit loses the penya (KEEP, 20 §10.3 step 4).

use super::host::DialogHost;
use crate::chat::ingress::notice;
use flyff_common::ObjId;
use flyff_common::defs::text::{TID_GAME_COMDELNOTKINGPIN, TID_GAME_COMNOHAVECOM};
use flyff_lua::Helper;
use flyff_proto::pb;
use flyff_proto::pb::dialog_op::Op;
use std::sync::Arc;

/// What the helpers ask about a player's party.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartyFacts {
    /// `GetPartyNum`: the member count.
    pub size: u32,
    /// `IsPartyMaster`: the player leads the party.
    pub leader: bool,
    /// `GetPartyLevel`.
    pub level: u32,
}

/// What the helpers ask about a player's guild.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuildFacts {
    /// `IsGuildMaster`.
    pub master: bool,
}

/// The party and guild queries of the helper API (doc 21's answers, passed through unchanged).
pub trait SocialView: std::fmt::Debug + Send + Sync {
    /// The party of the player, `None` without one.
    fn party(&self, _who: ObjId) -> Option<PartyFacts> {
        None
    }

    /// The guild of the player, `None` without one.
    fn guild(&self, _who: ObjId) -> Option<GuildFacts> {
        None
    }

    /// `IsPartyGuild` (src:_Common/ScriptLib.cpp:737-780): 1 without a party or when a member is offline or in a
    /// guild, 2 for a running rejoin cooldown (only with `features.remove_guild_cooldown = false`), else 0.
    fn is_party_guild(&self, _who: ObjId) -> i64 {
        1
    }

    /// The state of guild quest `q` of the player's guild.
    fn guild_quest_state(&self, _who: ObjId, _q: u32) -> Option<i32> {
        None
    }

    /// `CreateGuild()`: doc 21's no-charge creation with the player as actor. `true` when a guild was created.
    fn create_guild(&self, _who: ObjId) -> bool {
        false
    }
}

/// The default binding: no party, no guild, no creation.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoSocial;

impl SocialView for NoSocial {}

impl DialogHost<'_, '_> {
    /// The social binding in force.
    pub(super) fn social(&self) -> Arc<dyn SocialView> {
        Arc::clone(&self.svc.npc.social)
    }

    /// The party of the player: the real party service first (WP-SOCIAL-03), then the binding's answer.
    fn party_facts(&self, player: ObjId) -> Option<PartyFacts> {
        crate::social::party::npc_facts(&*self.svc, &*self.ctx, player)
            .or_else(|| self.social().party(player))
    }

    /// The guild of the player: the real guild service first (WP-NPC-11), then the binding's answer.
    fn guild_facts(&self, player: ObjId) -> Option<GuildFacts> {
        crate::social::guild::npc_facts(&*self.svc, &*self.ctx, player)
            .or_else(|| self.social().guild(player))
    }

    /// `true` when the real party service knows a party of the player (the real guild queries apply to it).
    fn has_real_party(&self, player: ObjId) -> bool {
        crate::social::party::npc_facts(&*self.svc, &*self.ctx, player).is_some()
    }

    /// The helpers of §6.7.
    pub(super) fn social_helper(&mut self, h: Helper) -> i64 {
        let Some(who) = self.who else { return 0 };
        let social = self.social();
        let player = who.0;
        use Helper as H;
        match h {
            H::IsParty => i64::from(self.party_facts(player).is_some()),
            H::IsPartyMaster => i64::from(self.party_facts(player).is_some_and(|p| p.leader)),
            H::GetPartyNum => self.party_facts(player).map_or(0, |p| i64::from(p.size)),
            H::GetPartyLevel => self.party_facts(player).map_or(0, |p| i64::from(p.level)),
            H::IsPartyGuild => {
                if self.party_facts(player).is_none() {
                    1
                } else if self.has_real_party(player) {
                    crate::social::guild::is_party_guild(&*self.svc, &*self.ctx, player)
                } else {
                    social.is_party_guild(player)
                }
            }
            H::IsGuild => i64::from(self.guild_facts(player).is_some()),
            H::IsGuildMaster => i64::from(self.guild_facts(player).is_some_and(|g| g.master)),
            H::CreateGuild => {
                let created = if self.has_real_party(player) {
                    crate::social::guild::create_for_npc(self.svc, self.ctx, player)
                } else {
                    social.create_guild(player)
                };
                if !created {
                    // no refund: the script already took the 3 000 000 penya (KEEP, 20 §10.3 step 4)
                    tracing::error!(
                        player = player.0,
                        npc = self.npc.0,
                        "the guild creation of the NPC dialog was refused after the penya were taken"
                    );
                }
                i64::from(created)
            }
            H::DestroyGuild => {
                match self.guild_facts(player) {
                    Some(g) if g.master => {
                        // a no-op on the client (KEEP): disbanding goes through the guild window
                        self.push(Op::DestroyGuild(pb::DialogEmpty {}));
                    }
                    Some(_) => {
                        notice::defined(self.ctx, who, TID_GAME_COMDELNOTKINGPIN, Vec::new())
                    }
                    None => notice::defined(self.ctx, who, TID_GAME_COMNOHAVECOM, Vec::new()),
                }
                1
            }
            _ => 0,
        }
    }
}

impl SocialView for Arc<dyn SocialView> {
    fn party(&self, who: ObjId) -> Option<PartyFacts> {
        (**self).party(who)
    }
    fn guild(&self, who: ObjId) -> Option<GuildFacts> {
        (**self).guild(who)
    }
    fn is_party_guild(&self, who: ObjId) -> i64 {
        (**self).is_party_guild(who)
    }
    fn guild_quest_state(&self, who: ObjId, q: u32) -> Option<i32> {
        (**self).guild_quest_state(who, q)
    }
    fn create_guild(&self, who: ObjId) -> bool {
        (**self).create_guild(who)
    }
}
