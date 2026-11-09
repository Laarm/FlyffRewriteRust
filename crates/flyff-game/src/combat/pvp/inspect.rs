//! Region and duel inspection (Plan/17 §8.2, §8.6; F-177): `IsPKPVPInspectionBase` (`Mover.cpp:6959-6999`),
//! `IsPVPInspectionBase` (`7001-7030`), the solo half of `IsPVPInspection` (`6831-6890`), `GetSummonState`
//! (`MoverParam.cpp:313-345`) and the PK / PvP skill-region rule (`__PK_PVP_SKILL_REGION`).
//!
//! The party half of `IsPVPInspection` needs the party membership and lives with the party duels
//! ([`crate::combat::duel`]).

use super::PvpEnv;
use super::hit::{
    is_arena_target, is_guild_combat_target, is_pvp_target, is_vagrant, is_war_target, region_class,
};
use crate::combat::hooks::CombatHooks;
use crate::combat::state::is_die;
use crate::mover::Mover;
use crate::stats::tick::is_attack_mode;
use flyff_common::WorldId;
use flyff_common::defs::define::{RA_FIGHT, RA_PK, RA_SAFETY};
use flyff_common::defs::text::{
    TID_PK_CHANGEJOB_NO, TID_PK_LEVEL_GAP, TID_PK_SAFETY_NO, TID_PK_SAME_NO,
};
use flyff_common::defs::world::{WI_WORLD_GUILDWAR, WI_WORLD_QUIZ};
use flyff_rules::data::RulesData;

/// The level gap from which two players cannot duel (`abs(a - b) >= 30`).
pub const DUEL_LEVEL_GAP: i32 = 30;

/// Why a duel is refused: the level gap, or an inspection code (`IsPVPInspectionBase`'s non-zero result).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InspectError {
    /// A level gap of 30 or more (`TID_PK_LEVEL_GAP`).
    LevelGap,
    /// The code of the base inspection: 1 safety / flying / at war / quiz world, 2 other world, 3 vagrant,
    /// 9 already in a duel, 10 guild-war world.
    Code(u32),
}

impl InspectError {
    /// The text the requester gets, if any (codes 9 and 10 are silent).
    pub fn tid(self) -> Option<u32> {
        match self {
            InspectError::LevelGap => Some(TID_PK_LEVEL_GAP),
            InspectError::Code(1) => Some(TID_PK_SAFETY_NO),
            InspectError::Code(2) => Some(TID_PK_SAME_NO),
            InspectError::Code(3) => Some(TID_PK_CHANGEJOB_NO),
            InspectError::Code(_) => None,
        }
    }
}

/// `IsPKPVPInspectionBase(region, bPVP)` of `m` (Plan/17 §8.2): `0` = may take part in PK / PvP in the region
/// class `region` (the class of the *attacker*).
///
/// `region == RA_SAFETY` -> 1; `region` differs from the own class -> 2; PvP only: a vagrant in a fight or
/// penalty region -> 3; PK only: level at or below the limit -> 5; PvP only: already in a duel -> 9; flying or
/// at war -> 1.
pub fn pk_pvp_inspection_base(
    m: &Mover,
    region: u32,
    pvp: bool,
    env: &PvpEnv,
    jobs: &RulesData,
) -> u32 {
    let own = region_class(m);
    if region == RA_SAFETY {
        return 1;
    }
    if region != own {
        return 2;
    }
    if pvp && (own == RA_FIGHT || own != RA_PK) && is_vagrant(m, jobs) {
        return 3;
    }
    if !pvp && m.mv.level <= env.limit_level {
        return 5;
    }
    if pvp && m.combat.duel.state != 0 {
        return 9;
    }
    if m.sim.act.flags.fly() {
        return 1;
    }
    if m.player().is_some_and(|p| p.social.war > 0) {
        return 1;
    }
    0
}

/// `IsPVPInspectionBase(region, world, bPVP = TRUE)` (`Mover.cpp:7001-7030`): why `m` cannot duel with somebody
/// in world `other_world`; `0` = may. A vagrant -> 3; already in a duel -> 9; in the guild-war world -> 10;
/// another world -> 2; flying or at war -> 1; in the quiz world -> 1.
pub fn pvp_inspection_base(m: &Mover, other_world: WorldId, jobs: &RulesData) -> u32 {
    if is_vagrant(m, jobs) {
        return 3;
    }
    if m.combat.duel.state != 0 {
        return 9;
    }
    let world = m.core.world;
    if world.0 == WI_WORLD_GUILDWAR {
        return 10;
    }
    if world != other_world {
        return 2;
    }
    if m.sim.act.flags.fly() {
        return 1;
    }
    if m.player().is_some_and(|p| p.social.war > 0) {
        return 1;
    }
    if world.0 == WI_WORLD_QUIZ {
        return 1;
    }
    0
}

/// `IsPVPInspection(target, 1)` (this = `a`): may the two players duel? The level gap first, then the base
/// inspection of `a` against the world of `b` and, if that passes, of `b` against the world of `a`.
pub fn pvp_inspection_solo(a: &Mover, b: &Mover, jobs: &RulesData) -> Result<(), InspectError> {
    if (a.mv.level - b.mv.level).abs() >= DUEL_LEVEL_GAP {
        return Err(InspectError::LevelGap);
    }
    let mut code = pvp_inspection_base(a, b.core.world, jobs);
    if code == 0 {
        code = pvp_inspection_base(b, a.core.world, jobs);
    }
    if code != 0 {
        return Err(InspectError::Code(code));
    }
    Ok(())
}

/// `GetSummonState()` (`MoverParam.cpp:313-345`): what keeps a player from summoning, trading or starting a duel.
/// `0` free, `1` trading / bank / guild bank, `2` dead, `3` vending, `4` in combat, `5` flying, `6` dueling.
pub fn summon_state(m: &Mover) -> u8 {
    if let Some(p) = m.player()
        && (p.econ.has_partner() || p.econ.bank_open || p.social.guild_bank_open)
    {
        return 1;
    }
    if is_die(m) {
        return 2;
    }
    if m.player().is_some_and(|p| p.econ.vending) {
        return 3;
    }
    if is_attack_mode(m) {
        return 4;
    }
    if m.sim.act.flags.fly() {
        return 5;
    }
    if m.combat.duel.duel != 0 {
        return 6;
    }
    0
}

/// The PK / PvP skill-region rule (F-177, `__PK_PVP_SKILL_REGION`; `MoverSkill.cpp:354`, `Ctrl.cpp:1048`, KQ-08):
/// a **hostile** skill (`nEvildoing < 0`) between two different players is refused when either of them is, for
/// the class of `att`'s region, in a "PK-safe" position (inspection result 1: the class is a safe zone, or one is
/// flying or at war) and they are not duel, war, guild-combat or arena partners.
///
/// The caller decides that the skill is hostile and a skill (not an item), and tells the caster
/// `TID_GMAE_PK_NOT_AREA` when `use_skill` refuses (the buff application skips silently).
pub fn skill_region_refused(
    att: &Mover,
    def: &Mover,
    env: &PvpEnv,
    jobs: &RulesData,
    hooks: &dyn CombatHooks,
) -> bool {
    if !(att.is_player() && def.is_player()) || att.id() == def.id() {
        return false;
    }
    let region = region_class(att);
    let a = pk_pvp_inspection_base(att, region, false, env, jobs);
    let d = pk_pvp_inspection_base(def, region, false, env, jobs);
    (a == 1 || d == 1)
        && !(is_pvp_target(att, def)
            || is_war_target(env, att, def)
            || is_guild_combat_target(att, def, hooks)
            || is_arena_target(att, def))
}
