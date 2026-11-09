//! `QuestSubject`: the read-only view of a player that the quest predicates ask (Plan/20 §9.4), and
//! [`PlayerSubject`], its implementation over the real game state (WP-NPC-13).

use super::QuestBook;
use crate::ctx::{GameCtx, Services};
use crate::mover::Mover;
use crate::player::PlayerExt;
use crate::stats::StatsCore;
use flyff_common::ObjId;
use flyff_data::GameData;
use flyff_data::types::{ItemId, MoverId, SkillId};
use flyff_rules::jobs::{is_hero, is_master};
use std::sync::Arc;

/// The summoned pet as the quest conditions see it (`GetPet()`).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PetView {
    /// `CPet::GetLevel()` (the `PL_*` grade, 0 for the egg).
    pub level: i32,
    /// `CPet::GetExpPercent()`: `exp * 100 / max_exp`.
    pub exp_percent: i32,
}

/// The party of the subject (`CParty`).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PartyView {
    /// `GetSizeofMember()`.
    pub size: i32,
    /// `m_aMember[0]` is the subject (the leader is member 0).
    pub is_leader: bool,
}

/// The guild of the subject (`CGuild`).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct GuildView {
    /// `GetSize()`: the member count.
    pub size: i32,
    /// `m_idMaster == idPlayer`.
    pub is_master: bool,
}

/// What the begin, next-level and end predicates read of a player (see the module documentation).
pub trait QuestSubject {
    /// `GetLevel()`.
    fn level(&self) -> i32;
    /// `GetJob()`.
    fn job(&self) -> i32;
    /// `GetSex()`.
    fn sex(&self) -> i32;
    /// `GetGold()`.
    fn gold(&self) -> i64;
    /// `GetExpPercent()` in units of 1/100 percent (0..=9999).
    fn exp_percent(&self) -> i32;
    /// `GetPKValue()`.
    fn pk_value(&self) -> i32;
    /// `GetSkill(s)`: the level of the job skill `s` the player has learned (any level), `None` without it.
    fn skill_level(&self, s: SkillId) -> Option<u32>;
    /// `CMover::CheckSkill(s)`.
    fn check_skill(&self, s: SkillId) -> bool;
    /// `GetItemNum(i)` with the KQ-27 rule: 0 while any inventory item is reserved.
    fn item_num(&self, i: ItemId) -> i32;
    /// `m_Inventory.GetEmptyCount()`.
    fn empty_slots(&self) -> i32;
    /// `IsDisguise()`.
    fn is_disguised(&self) -> bool;
    /// `GetIndex()` (the disguise when there is one).
    fn mover_index(&self) -> MoverId;
    /// `GetPet()`: `None` without a summoned pet.
    fn pet(&self) -> Option<PetView>;
    /// `GetTutorialState()`.
    fn tutorial_state(&self) -> i32;
    /// `GetCampusPoint()`.
    fn campus_points(&self) -> i32;
    /// The party, `None` without one.
    fn party(&self) -> Option<PartyView>;
    /// The guild, `None` without one.
    fn guild(&self) -> Option<GuildView>;
    /// The quest book.
    fn quests(&self) -> &QuestBook;
}

/// [`QuestSubject`] over a live player: borrows the services and the context immutably for the length of one
/// predicate call.
pub struct PlayerSubject<'a> {
    svc: &'a Services,
    data: &'a GameData,
    core: Arc<StatsCore>,
    m: &'a Mover,
    p: &'a PlayerExt,
}

impl<'a> PlayerSubject<'a> {
    /// The subject for player `who`; `None` when the avatar is not a live player.
    pub fn new(svc: &'a Services, ctx: &'a GameCtx<'_>, who: ObjId) -> Option<PlayerSubject<'a>> {
        let m = ctx.worlds.player(who)?;
        let p = m.player()?;
        Some(PlayerSubject {
            svc,
            data: ctx.data.as_ref(),
            core: svc.stats.handle(),
            m,
            p,
        })
    }
}

fn clamp_i32(v: u32) -> i32 {
    i32::try_from(v).unwrap_or(i32::MAX)
}

impl QuestSubject for PlayerSubject<'_> {
    fn level(&self) -> i32 {
        self.m.mv.level
    }

    fn job(&self) -> i32 {
        self.m.mv.job
    }

    fn sex(&self) -> i32 {
        i32::from(self.p.appearance.sex)
    }

    fn gold(&self) -> i64 {
        i64::from(self.p.items.gold)
    }

    fn exp_percent(&self) -> i32 {
        flyff_rules::progression::exp_percent_x100(
            &self.core.rules,
            self.m.mv.level,
            self.p.progression.exp,
        )
    }

    fn pk_value(&self) -> i32 {
        self.p.pk.value
    }

    fn skill_level(&self, s: SkillId) -> Option<u32> {
        if s == flyff_data::NULL_ID {
            return None;
        }
        self.p
            .progression
            .job_skills
            .iter()
            .find(|j| j.skill_id == s)
            .map(|j| j.level)
    }

    /// `CMover::CheckSkill` (`MoverParam.cpp:117-157`): the skill property exists and is learnable (`nLog != 1`),
    /// the character level reaches the required level (masters and heroes are exempt; `NULL_ID` = none), and each
    /// prerequisite skill the player HAS is at its required level.
    fn check_skill(&self, s: SkillId) -> bool {
        let Some(prop) = self.data.skill(s) else {
            return false;
        };
        if prop.log == 1 {
            return false;
        }
        let master_or_hero =
            is_master(&self.core.rules, self.m.mv.job) || is_hero(&self.core.rules, self.m.mv.job);
        if !master_or_hero && self.m.mv.level < prop.req_dis_lv as i32 {
            return false;
        }
        for (skill, level) in [
            (prop.re_skill1, prop.re_skill_level1),
            (prop.re_skill2, prop.re_skill_level2),
        ] {
            if skill != flyff_data::NULL_ID
                && let Some(have) = self.skill_level(skill)
                && have < level
            {
                return false;
            }
        }
        true
    }

    fn item_num(&self, i: ItemId) -> i32 {
        clamp_i32(self.p.items.quest_item_count(i))
    }

    fn empty_slots(&self) -> i32 {
        i32::try_from(self.p.items.inventory.empty_count()).unwrap_or(i32::MAX)
    }

    fn is_disguised(&self) -> bool {
        self.m.mv.disguise.is_some()
    }

    fn mover_index(&self) -> MoverId {
        self.m.mv.disguise.unwrap_or(self.m.core.index)
    }

    fn pet(&self) -> Option<PetView> {
        let (_, _, pet) = crate::item::pets::system::active_pet(self.m, self.data)?;
        let max = u64::from(pet.max_exp()).max(1);
        Some(PetView {
            level: i32::from(pet.level_u8()),
            exp_percent: i32::try_from(u64::from(pet.exp) * 100 / max).unwrap_or(i32::MAX),
        })
    }

    fn tutorial_state(&self) -> i32 {
        self.p.progression.tutorial_state
    }

    fn campus_points(&self) -> i32 {
        self.p.social.campus_point
    }

    fn party(&self) -> Option<PartyView> {
        let c = self.p.ident.character_id;
        let party = self.svc.social.party.party_record_of(c)?;
        Some(PartyView {
            size: i32::try_from(party.size()).unwrap_or(i32::MAX),
            is_leader: party.leader_id() == Some(c),
        })
    }

    fn guild(&self) -> Option<GuildView> {
        let c = self.p.ident.character_id;
        let g = self.svc.social.guild.guild_record_of(c)?;
        Some(GuildView {
            size: i32::try_from(g.members.len()).unwrap_or(i32::MAX),
            is_master: g.master() == Some(c),
        })
    }

    fn quests(&self) -> &QuestBook {
        &self.p.quests
    }
}
