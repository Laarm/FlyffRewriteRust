//! Skills (Plan/17 §6): the application of a skill or item effect, the skill start, the skill states and hit
//! handlers, the action slot, skill points, the Blinkpool teleport and the Resurrection offers.
//!
//! | module | content | WP |
//! |---|---|---|
//! | [`apply`] | `apply_param`, `apply_skill` (the `CCtrl::ApplySkill` steps), `do_active_skill`, the override views | SKILL-02 |
//! | [`area`] | `do_apply_skill[_ex]`, the AoE shapes (around, region, line, troupe), `target_selecter`, `IsRangeObj` over the store | SKILL-06 |
//! | [`ctrl`] | the ground DoT (`CContDamageCtrl`) and the Psychic Wall, one pass per tick | SKILL-06 |
//! | [`hardcoded`] | the catalogue of the hard-coded skills (`ApplySkillHardCoding` and the steps around it) | SKILL-08 |
//! | [`monster`] | the effect of a monster skill cast, `monster_skill_level` | SKILL-13 |
//! | [`use_skill`] | `UseSkillReq`, the 22 steps of `DoUseSkill`, costs, cooldowns, `UseSkillNtf`, the effective level | SKILL-04 |
//! | [`cast`] | the skill hit handlers (`OnMeleeSkill` / `OnMagicSkill`), shot skills, bursts, the skill end | SKILL-05 |
//! | [`queue`] | the action slot (skill queue), the server-driven approach, action points | SKILL-09 |
//! | [`points`] | `DoUseSkillPointReq` (VR-086), the legend skill-up, the reset and the master growth, the skill messages | SKILL-10 |
//! | [`tele`] | `TeleSkill` (Blinkpool, VR-082) | SKILL-10 |
//! | [`resurrect`] | the Resurrection skill: offer, accept, cancel; the buffs after a revival (PL-082) | SKILL-10 |
//!
//! [`SkillRuntime`] is the per-player state (`PlayerExt.skills_rt`): the re-use timers, the action points and the
//! queue cursor. None of it is persisted except the action points (`TaskbarDoc.action_points`); a cool-down is
//! lost on logout (KEEP-QUIRK, `Mover.cpp:483,1519`).
//!
//! **Where the code runs.** The skill start and the queue run on the [`ActionCx`](crate::combat::ActionCx) the
//! action layer hands out, so they work both in a client handler and at the barrier of a frame (the queue's
//! next skill starts when the previous skill state ends, inside the world phase). The hit handlers of the
//! barrier have no services: a skill hit whose effect needs the buff engine is queued
//! ([`cast::SkillJob`]) and runs in the service context right after the world phase
//! ([`cast::process_pending`], called by `combat::tick::finish`); the costs, the cool-down and the projectile
//! spawn of a shot skill need no services and run in the barrier.

pub mod apply;
pub mod area;
pub mod cast;
pub mod ctrl;
pub mod hardcoded;
pub mod monster;
pub mod points;
pub mod procs;
pub mod queue;
pub mod resurrect;
pub mod tele;
pub mod use_skill;

pub use cast::SkillHits;
pub use queue::{AP_MAX, queue_ap_gain};
pub use use_skill::{
    Refusal, SkillUseType, Started, effective_level, on_use_skill_req, queue_casting_ticks,
    use_skill_direct,
};

use flyff_common::GameMs;

/// `MAX_SKILL_JOB`: the slots of `m_aJobSkill`.
pub const SLOTS: usize = flyff_rules::consts::MAX_SKILL_JOB;

pub use flyff_rules::combat::flags::REQ_ALL;

/// The skill engine's service data (`Services.skill`): what the rules need and `GameData` does not hold.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SkillService {
    /// The guild-quest regions of `propGuildQuest.inc` (`m_aPropGuildQuest`): `[x1, y1, x2, y2]` of each. The
    /// file of this distribution is empty, so the list is empty; Blinkpool is then never restricted.
    pub guild_quest_regions: Vec<[i32; 4]>,
    /// The ground DoTs alive (`CContDamageCtrl`, virtual objects: no id, no link map, never sent to a client),
    /// in the order they were created ([`ctrl::process`] walks them once per tick).
    pub dots: Vec<ctrl::GroundDot>,
}

impl SkillService {
    /// `CProject::IsGuildQuestRegion(pos)` (`Project.cpp:4683`): the point `(int)x, (int)z` lies in a region's
    /// `CRect(x1, y2, x2, y1)` (`PtInRect`: left and top inclusive, right and bottom exclusive).
    pub fn is_guild_quest_region(&self, pos: flyff_common::math::Vec3) -> bool {
        let (x, z) = (pos.x as i32, pos.z as i32);
        self.guild_quest_regions.iter().any(|r| {
            let [x1, y1, x2, y2] = *r;
            let (left, top, right, bottom) = (x1, y2, x2, y1);
            left <= x && x < right && top <= z && z < bottom
        })
    }
}

/// Per-player skill runtime: re-use timers, action points and the queue cursor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillRuntime {
    /// `m_tmReUseDelay[45]`: when each skill slot is ready again (`now < ready_at` is a cool-down).
    pub ready_at: [GameMs; SLOTS],
    /// `m_nActionPoint`: restored from the taskbar document (0..=100).
    pub action_points: i32,
    /// `m_tmActionPoint`: the last time the passive action-point gain ran; `None` until the first look (the gain
    /// is counted from the moment the player is first processed, as the legacy constructor stamps the time).
    pub ap_last: Option<GameMs>,
    /// `m_nUsedSkillQueue`: the slot of the action-slot skill being executed; `None` is the legacy `-1`.
    pub queue_cursor: Option<u8>,
    /// The queued skill that waits for the player to arrive (`m_oaCmd` of `OBJACT_USESKILL` with its parameters).
    pub pending: Option<queue::PendingSkill>,
    /// `m_pWall[2]`: the Psychic Walls of this player (at most two; a slot whose ctrl is gone is free).
    pub walls: [Option<flyff_common::ObjId>; 2],
}

impl Default for SkillRuntime {
    fn default() -> Self {
        SkillRuntime {
            ready_at: [GameMs(0); SLOTS],
            action_points: 0,
            ap_last: None,
            queue_cursor: None,
            pending: None,
            walls: [None; 2],
        }
    }
}

impl SkillRuntime {
    /// `GetReuseDelay(slot)`: the milliseconds left of the cool-down of `slot` at `now` (0 = ready).
    pub fn reuse_delay(&self, slot: usize, now: GameMs) -> u64 {
        self.ready_at
            .get(slot)
            .map_or(0, |end| end.0.saturating_sub(now.0))
    }

    /// `SetCoolTime`: the slot is ready again `cooldown_ms` after `now`.
    pub fn set_cool_time(&mut self, slot: usize, now: GameMs, cooldown_ms: u32) {
        if let Some(end) = self.ready_at.get_mut(slot) {
            *end = GameMs(now.0.saturating_add(u64::from(cooldown_ms)));
        }
    }
}
