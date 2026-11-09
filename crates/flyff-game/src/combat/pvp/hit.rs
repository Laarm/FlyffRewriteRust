//! The player-versus-player hit types (Plan/17 §8.1; `GetHitType` / `GetHitType2`, `MoverAttack.cpp:1950-2126`,
//! `IsPVPTarget` and friends `2131-2209`).

use super::PvpEnv;
use crate::combat::hooks::CombatHooks;
use crate::combat::state::{is_die, is_invulnerable};
use crate::mover::Mover;
use crate::world::{pkpvp_region_attr, region::region_attr_of};
use flyff_common::defs::define::RA_SAFETY;
use flyff_common::defs::world::WI_WORLD_ARENA;
use flyff_rules::combat::HitType;
use flyff_rules::data::RulesData;
use flyff_rules::jobs::is_base;

/// `GetPKPVPRegionAttr()`: `RA_FIGHT` > `RA_PENALTY_PK` > `RA_PK`, else `RA_SAFETY`.
pub fn region_class(m: &Mover) -> u32 {
    pkpvp_region_attr(region_attr_of(m))
}

/// `IsBaseJob()`: a vagrant.
pub fn is_vagrant(m: &Mover, jobs: &RulesData) -> bool {
    is_base(jobs, m.mv.job)
}

/// The id of the party `m` is in (`m_idparty`, 0 = none).
pub(super) fn party_id(m: &Mover) -> u32 {
    m.player()
        .and_then(|p| p.social.party)
        .map_or(0, |party| party.0)
}

/// `m_idDuelParty` (0 = none).
pub(super) fn duel_party_id(m: &Mover) -> u32 {
    m.player().map_or(0, |p| p.social.duel_party)
}

/// `IsPVPTarget(def)` (this = `att`): `def` is the solo duel partner of `att`, or a member of the party `att`
/// duels, and the fight is on (`state == 1`) on either side.
pub fn is_pvp_target(att: &Mover, def: &Mover) -> bool {
    let mut partner = att.combat.duel.other == Some(def.id());
    let duel_party = duel_party_id(att);
    if duel_party > 0 && duel_party == party_id(def) {
        partner = true;
    }
    partner && (att.combat.duel.state == 1 || def.combat.duel.state == 1)
}

/// `IsWarTarget(def)`: `EVE_GUILDWAR == 1`, the same war (> 0), different guilds.
pub fn is_war_target(env: &PvpEnv, att: &Mover, def: &Mover) -> bool {
    let (Some(a), Some(d)) = (att.player(), def.player()) else {
        return false;
    };
    env.guild_war
        && a.social.war > 0
        && a.social.war == d.social.war
        && a.social.guild != d.social.guild
}

/// `IsSchoolTarget(def)`: the school battle is in its second phase (a dead event: always `false`).
pub fn is_school_target(env: &PvpEnv) -> bool {
    env.school_battle
}

/// The arena pit (`IsArenaTarget`): the squared radius `d <= 1246` (about 35.3 m) around `(490, 507)` in the XZ
/// plane, compared on the offsets cut to `int` like the C++.
pub const ARENA_RADIUS_SQ: i32 = 1246;

/// The centre of the arena pit.
pub const ARENA_CENTRE: (i32, i32) = (490, 507);

/// `true` when the XZ position lies in the arena pit.
pub fn in_arena_pit(x: f32, z: f32) -> bool {
    let dx = (x - ARENA_CENTRE.0 as f32) as i32;
    let dz = (z - ARENA_CENTRE.1 as f32) as i32;
    dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz)) <= ARENA_RADIUS_SQ
}

/// `IsArenaTarget(def)`: in the arena world (`WI_WORLD_ARENA`) both players stand in the pit.
pub fn is_arena_target(att: &Mover, def: &Mover) -> bool {
    att.core.world.0 == WI_WORLD_ARENA
        && in_arena_pit(def.core.pos.x, def.core.pos.z)
        && in_arena_pit(att.core.pos.x, att.core.pos.z)
}

/// `IsGuildCombatTarget(def)`: both are combatants (`guild_combat_state == 1`) of different guilds while a
/// guild combat runs (the manager's state is doc 21's, [`CombatHooks::guild_combat_running`]).
pub fn is_guild_combat_target(att: &Mover, def: &Mover, hooks: &dyn CombatHooks) -> bool {
    hooks.is_guild_combat_target(att, def)
}

/// The player part of `GetHitType` (this = `att`, `def`; both live players, not invulnerable, different):
///
/// ```text
/// guild-combat targets                                  -> GuildCombat
/// either flying                                          -> Fail
/// duel partner / duel party member, duel on              -> Pvp
/// war target (EVE_GUILDWAR)                              -> War
/// school battle                                          -> School
/// arena (both in the pit)                                -> Arena
/// attacker's region class is SAFETY or differs           -> Fail
/// either a vagrant                                       -> Fail
/// EVE_GUILDWAR and either at war                         -> Fail
/// EVE_PK: either level <= LIMITE_LEVEL                   -> Fail, else Pk
/// otherwise                                              -> Fail
/// ```
pub fn hit_type_pvp(
    att: &Mover,
    def: &Mover,
    env: &PvpEnv,
    jobs: &RulesData,
    hooks: &dyn CombatHooks,
) -> HitType {
    if is_guild_combat_target(att, def, hooks) {
        return HitType::GuildCombat;
    }
    if att.sim.act.flags.fly() || def.sim.act.flags.fly() {
        return HitType::Fail;
    }
    if is_pvp_target(att, def) {
        return HitType::Pvp;
    }
    if is_war_target(env, att, def) {
        return HitType::War;
    }
    if is_school_target(env) {
        return HitType::School;
    }
    if is_arena_target(att, def) {
        return HitType::Arena;
    }
    let class = region_class(att);
    if class == RA_SAFETY || class != region_class(def) {
        return HitType::Fail;
    }
    if is_vagrant(att, jobs) || is_vagrant(def, jobs) {
        return HitType::Fail;
    }
    if env.guild_war {
        let at_war = |m: &Mover| m.player().is_some_and(|p| p.social.war > 0);
        if at_war(att) || at_war(def) {
            return HitType::Fail;
        }
    }
    if env.pk {
        if att.mv.level <= env.limit_level || def.mv.level <= env.limit_level {
            return HitType::Fail;
        }
        return HitType::Pk;
    }
    HitType::Fail
}

/// `GetHitType2(def, is_target, good)` (this = `att`; Plan/17 §8.1): "is `def` an enemy for skills?".
///
/// | case | answer |
/// |---|---|
/// | either invalid | `Generic` |
/// | either dead | `Fail` |
/// | either invulnerable (`m_nDead`, a just revived or just finished duelist) | `Generic` |
/// | two players, either flying | `Fail` |
/// | two players, duel / war / school / guild combat / arena target | `Pvp` / `War` / `School` / `GuildCombat` / `Arena` |
/// | two players otherwise, `EVE_PK` | beneficial skill (`good`) `Fail`; level limit `Fail`; else `Pk` |
/// | two players otherwise | `Fail` |
/// | an NPC involved | `Generic` (a beneficial skill on an NPC is refused by the caller) |
///
/// Unlike `GetHitType` there is no region test and no vagrant test here (the skill region rule is
/// [`super::inspect::skill_region_refused`]).
pub fn hit_type2(
    att: &Mover,
    def: &Mover,
    env: &PvpEnv,
    hooks: &dyn CombatHooks,
    good: bool,
) -> HitType {
    if att.core.deleted || def.core.deleted {
        return HitType::Generic;
    }
    if is_die(att) || is_die(def) {
        return HitType::Fail;
    }
    if is_invulnerable(att) || is_invulnerable(def) {
        return HitType::Generic;
    }
    if !(att.is_player() && def.is_player()) {
        return HitType::Generic;
    }
    if att.sim.act.flags.fly() || def.sim.act.flags.fly() {
        return HitType::Fail;
    }
    if is_pvp_target(att, def) {
        return HitType::Pvp;
    }
    if is_war_target(env, att, def) {
        return HitType::War;
    }
    if is_school_target(env) {
        return HitType::School;
    }
    if is_guild_combat_target(att, def, hooks) {
        return HitType::GuildCombat;
    }
    if is_arena_target(att, def) {
        return HitType::Arena;
    }
    if env.pk {
        if good {
            return HitType::Fail;
        }
        if att.mv.level <= env.limit_level || def.mv.level <= env.limit_level {
            return HitType::Fail;
        }
        return HitType::Pk;
    }
    HitType::Fail
}
