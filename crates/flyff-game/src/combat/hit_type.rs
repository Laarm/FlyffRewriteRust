//! `GetHitType` and `IsAttackAbleNPC` (Plan/17 §8.1; `MoverAttack.cpp:2013-2126`, `Mover.cpp:6680-6734`;
//! WP-COMBAT-04): whether damage is allowed at all.
//!
//! The player-versus-player branch (duels, wars, guild combat, arena, PK) is [`super::pvp::hit_type_pvp`]
//! (WP-COMBAT-12); [`super::hooks::CombatHooks::hit_type_pvp`] can override it (tests). With the repack defaults
//! (`events.pk = false`) two players can hurt each other only as duel partners, war or guild-combat targets or in
//! the arena.

use super::env::DamageEnv;
use super::hooks::CombatHooks;
use super::pvp;
use super::state::{is_chaotic, is_die, is_invulnerable, is_peaceful};
use crate::mover::Mover;
use flyff_data::GameData;
use flyff_rules::combat::HitType;
use flyff_rules::consts::OBJTYPE_FLYING;
use flyff_rules::consts::dst::RANK_GUARD;

/// The answer of [`hit_type_ex`] with the side effect of the original `IsAttackAbleNPC`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HitCheck {
    /// The hit type.
    pub ty: HitType,
    /// A player attacker whose NPC target turned out dead: the original sends it `SetTarget(NULL_ID)`.
    pub clear_attacker_target: bool,
}

/// `HasBuffByIk3(IK3_TEXT_DISGUISE)`: a disguised player cannot attack NPCs. The disguise buff belongs to the
/// buff engine (`MoverCore.disguise` holds the disguise mover, 16 §3.2).
fn is_disguised(m: &Mover) -> bool {
    m.mv.disguise.is_some()
}

/// `IsFlyingNPC()`: `m_dwTypeFlag & OBJTYPE_FLYING`.
fn is_flying_npc(m: &Mover) -> bool {
    !m.is_player() && m.mv.type_flags & OBJTYPE_FLYING != 0
}

/// `bAttackerFly` / `bDefenderFly` of the NPC branch: a flying NPC, or a flying player.
fn flying(m: &Mover) -> bool {
    if m.is_player() {
        m.sim.act.flags.fly()
    } else {
        is_flying_npc(m)
    }
}

/// `CMover::IsAttackAbleNPC(npc)` (this = `att`): `(allowed, att_must_clear_its_target)`.
///
/// A disguised attacker, a killable-0 prop, a peaceful NPC fail; a dead NPC fails and tells a player to drop its
/// target; a `RANK_GUARD` compares the propensity (`chaotic < 0` guards spare chaotic attackers, `chaotic > 0`
/// guards spare non-chaotic ones); a flying attacker needs a flying target and a ground attacker a ground one.
pub fn is_attackable_npc(att: &Mover, npc: &Mover, data: &GameData) -> (bool, bool) {
    if is_disguised(att) {
        return (false, false);
    }
    if is_die(npc) {
        return (false, att.is_player());
    }
    let Some(prop) = data.mover(npc.prop_id()) else {
        return (true, false);
    };
    if prop.killable == 0 {
        return (false, false);
    }
    let mut able = true;
    if is_peaceful(npc) {
        able = false;
    } else {
        if prop.class == RANK_GUARD
            && ((prop.chaotic < 0 && is_chaotic(att)) || (prop.chaotic > 0 && !is_chaotic(att)))
        {
            able = false;
        }
        let att_fly = att.sim.act.flags.fly();
        if (att_fly && prop.flying == 0) || (!att_fly && prop.flying == 1) {
            able = false;
        }
    }
    (able, false)
}

/// `GetHitType(defender, is_target, reflect)` (this = `att`) with the `IsAttackAbleNPC` side effect reported.
///
/// ```text
/// either invalid, dead, or invulnerable (dead_ticks > 0)       -> Fail
/// att == def                                                     -> Generic
/// both players                                                   -> pvp::hit_type_pvp (17 §8.1)
/// otherwise:  attacker RANK_GUARD                                -> Generic
///             defender NPC and !is_attackable_npc                -> Fail
///             one side flying, the other not                     -> Fail
///                                                                -> Generic
/// ```
pub fn hit_type_ex(
    att: &Mover,
    def: &Mover,
    env: &DamageEnv<'_>,
    hooks: &dyn CombatHooks,
    is_target: bool,
    reflect: i32,
) -> HitCheck {
    let data = &env.core.data;
    let ok = |ty| HitCheck {
        ty,
        clear_attacker_target: false,
    };
    if att.core.deleted || def.core.deleted {
        return ok(HitType::Fail);
    }
    if is_die(att) || is_die(def) {
        return ok(HitType::Fail);
    }
    if is_invulnerable(att) || is_invulnerable(def) {
        return ok(HitType::Fail);
    }
    if att.id() == def.id() {
        return ok(HitType::Generic);
    }
    if att.is_player() && def.is_player() {
        let ty = hooks
            .hit_type_pvp(att, def, is_target, reflect)
            .unwrap_or_else(|| pvp::hit_type_pvp(att, def, &env.pvp, &env.core.rules, hooks));
        return ok(ty);
    }
    if !att.is_player()
        && data
            .mover(att.prop_id())
            .is_some_and(|p| p.class == RANK_GUARD)
    {
        return ok(HitType::Generic);
    }
    if !def.is_player() {
        let (able, clear) = is_attackable_npc(att, def, data);
        if !able {
            return HitCheck {
                ty: HitType::Fail,
                clear_attacker_target: clear,
            };
        }
    }
    if flying(att) != flying(def) {
        return ok(HitType::Fail);
    }
    ok(HitType::Generic)
}

/// [`hit_type_ex`] without the side effect.
pub fn hit_type(
    att: &Mover,
    def: &Mover,
    env: &DamageEnv<'_>,
    hooks: &dyn CombatHooks,
    is_target: bool,
    reflect: i32,
) -> HitType {
    hit_type_ex(att, def, env, hooks, is_target, reflect).ty
}
