//! The lord skills (Plan/22 §9.6; WP-EVENT-11): the eight skills of `lordskill.txt`, their cooldowns in minutes and the
//! teleport rules.
//!
//! * [`use_skill`] (`LordSkillUseReq{skill, target_name}`, no NPC): the checks of `CLordSkillComponentExecutable::
//!   IsExecutable` and `IsTeleportable` in their order (`lordskill.cpp:184-221`), then the commit: the cooldown starts
//!   (`SetLordSkillCooldown`), the skill executes (`lordskillexecutable.cpp`) and `LordSkillUseNtf` goes to the lord's
//!   surroundings. The effects themselves are the item buffs `II_SYS_SYS_LS_*` and two teleports; their readers (shout
//!   colour, rage mute, party skills x4, CHEERING x1.1) live in their own modules and are not repeated here.
//! * [`minute`] (P4, after the lord-event ticks): every running cooldown loses a minute (`SetLordSkillCooldown`), then the
//!   lord, when he is in the world, gets `LordSkillTick` with ALL skills in table order (every minute, as the original).
//! * [`is_teleportable`] (`CUser::IsTeleportable`, `User.cpp:8262-8290`): the world table of the places a lord may not
//!   summon from or to.
//!
//! FIX: the range of a skill is measured only between players of the same world and layer (the original compared
//! positions across worlds); the cooldown is spent although the buff may fail (KEEP).

use crate::chat::ingress::notice;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::outbox::Audience;
use crate::world::ReplaceKind;
use flyff_common::defs::lordskill::{
    LI_RAGE, LT_ITEMBUF, LT_SUMMON, LT_TELEPORT, LTT_OTHER, LTT_SELF,
};
use flyff_common::defs::text::{
    TID_GAME_LORD_SKILL_RAGE_S001, TID_GAME_LORD_SKILL_RAGE_S002, TID_GAME_LORD_SKILL_SUMMON_S001,
    TID_GAME_LORD_SKILL_USE_E001, TID_GAME_LORD_SKILL_USE_E002, TID_GAME_LORD_SKILL_USE_E003,
    TID_GAME_LORD_SKILL_USE_E004, TID_GAME_LORD_SKILL_USE_E005, TID_GAME_LORD_SKILL_USE_E008,
    TID_GAME_LORD_SKILL_USE_E009, TID_GAME_LORD_SKILL_USE_E010,
};
use flyff_common::defs::world::{
    WI_DUNGEON_BEAR, WI_DUNGEON_KRRR, WI_DUNGEON_MUSCLE, WI_DUNGEON_SECRET_0, WI_DUNGEON_SECRET_L,
    WI_WORLD_CISLAND, WI_WORLD_GUILDWAR, WI_WORLD_GUILDWAR1TO1_0, WI_WORLD_GUILDWAR1TO1_L,
    WI_WORLD_KEBARAS, WI_WORLD_MADRIGAL, WI_WORLD_QUIZ,
};
use flyff_common::gm::Authority;
use flyff_common::math::Vec3;
use flyff_common::{CharacterId, LayerId, ObjId, WorldId};
use flyff_db::DbOp;
use flyff_db::audit::content as audit;
use flyff_proto::pb;
use std::sync::Arc;

fn defined(ctx: &mut GameCtx<'_>, who: PlayerRef, tid: u32) {
    notice::defined(ctx, who, tid, Vec::new());
}

/// `CUser::IsTeleportable` (`User.cpp:8262-8290`): `false` in the guild-war world (202), the GC 1:1 worlds (241..=244),
/// the dungeons 210 (`WI_DUNGEON_MUSCLE`), 220 (`KRRR`) and 230 (`BEAR`), Kebaras (2), Madrigal (1) inside a guild-quest
/// region, the secret-room worlds 151..=167, Coral Island (3), every instance-dungeon world (the `svc.gm.instances`
/// seam that `/teleport` and the disguise rule read) and the quiz world (211); everywhere else `true` when the player
/// has no summon state (not trading, dead, vending, fighting, flying or dueling). `false` for an object that is not a
/// mover.
pub fn is_teleportable(svc: &Services, ctx: &GameCtx<'_>, obj: ObjId) -> bool {
    let Some(m) = ctx.worlds.mover(obj) else {
        return false;
    };
    let world = m.core.world.0;
    if world == WI_WORLD_GUILDWAR
        || (WI_WORLD_GUILDWAR1TO1_0..=WI_WORLD_GUILDWAR1TO1_L).contains(&world)
        || world == WI_DUNGEON_MUSCLE
        || world == WI_DUNGEON_KRRR
        || world == WI_DUNGEON_BEAR
        || world == WI_WORLD_KEBARAS
        || (world == WI_WORLD_MADRIGAL && svc.skill.is_guild_quest_region(m.core.pos))
        || (WI_DUNGEON_SECRET_0..=WI_DUNGEON_SECRET_L).contains(&world)
        || world == WI_WORLD_CISLAND
        || svc.gm.instances.is_instance_world(m.core.world)
        || world == WI_WORLD_QUIZ
    {
        return false;
    }
    crate::combat::pvp::summon_state(m) == 0
}

/// Where and who a skill aims at.
struct Target {
    obj: ObjId,
    character: CharacterId,
    name: String,
    world: WorldId,
    layer: LayerId,
    pos: Vec3,
    game_master: bool,
}

fn target_of(ctx: &GameCtx<'_>, obj: ObjId) -> Option<Target> {
    let m = ctx.worlds.player(obj)?;
    let p = m.player()?;
    Some(Target {
        obj,
        character: p.ident.character_id,
        name: p.ident.name.clone(),
        world: m.core.world,
        layer: m.core.layer,
        pos: m.core.pos,
        game_master: p.ident.authority.at_least(Authority::GAMEMASTER),
    })
}

fn distance_sq(a: Vec3, b: Vec3) -> f32 {
    let (dx, dy, dz) = (a.x - b.x, a.y - b.y, a.z - b.z);
    dx * dx + dy * dy + dz * dz
}

/// `LordSkillUseReq{skill, target_name}` (22 §9.6). In this order, the first failure sends its text:
/// unknown skill (`E002`, a negative id included), not the lord (`E001`), a passive skill (`E003`); the target (`LTT_SELF`:
/// the lord; else the online player of that name, `LTT_OTHER` not the lord himself: `E004`); only for a target type other
/// than SELF: a range (`E008`: not the same world and layer, or `|d|^2 > r^2`, 3-D) and RAGE on a game master (`E004`);
/// SUMMON and TELEPORT: a game master as target (`E004`), the lord not teleportable (`E009`), the target not
/// teleportable (`E010`); the cooldown (`E005`).
pub fn use_skill(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    skill: i32,
    target_name: &str,
) {
    let data = Arc::clone(ctx.data);
    let Some(def) = u32::try_from(skill)
        .ok()
        .and_then(|id| data.lord.skills.iter().find(|k| k.id == id))
    else {
        return defined(ctx, who, TID_GAME_LORD_SKILL_USE_E002);
    };
    let Some(lord) = target_of(ctx, who.0) else {
        return;
    };
    if !s.event.lord.is_lord(lord.character) {
        return defined(ctx, who, TID_GAME_LORD_SKILL_USE_E001);
    }
    if def.passive != 0 {
        return defined(ctx, who, TID_GAME_LORD_SKILL_USE_E003);
    }
    let target = if def.target == LTT_SELF {
        None
    } else {
        let aim = s
            .presence
            .by_name(target_name)
            .and_then(|e| e.online)
            .and_then(|o| target_of(ctx, o.obj));
        match aim {
            Some(t) if !(def.target == LTT_OTHER && t.obj == lord.obj) => Some(t),
            _ => return defined(ctx, who, TID_GAME_LORD_SKILL_USE_E004),
        }
    };
    if let Some(t) = &target {
        if def.range != 0.0 {
            // FIX: players of different worlds or layers are out of range
            let near = t.world == lord.world
                && t.layer == lord.layer
                && distance_sq(lord.pos, t.pos) <= def.range * def.range;
            if !near {
                return defined(ctx, who, TID_GAME_LORD_SKILL_USE_E008);
            }
        }
        if def.id == LI_RAGE && t.game_master {
            return defined(ctx, who, TID_GAME_LORD_SKILL_USE_E004);
        }
        if def.kind == LT_SUMMON || def.kind == LT_TELEPORT {
            if t.game_master {
                return defined(ctx, who, TID_GAME_LORD_SKILL_USE_E004);
            }
            if !is_teleportable(s, ctx, lord.obj) {
                return defined(ctx, who, TID_GAME_LORD_SKILL_USE_E009);
            }
            if !is_teleportable(s, ctx, t.obj) {
                return defined(ctx, who, TID_GAME_LORD_SKILL_USE_E010);
            }
        }
    }
    if s.event.lord.skill_tick(def.id) > 0 {
        return defined(ctx, who, TID_GAME_LORD_SKILL_USE_E005);
    }

    // ---- the commit block: the cooldown starts, then the skill executes ----
    let minutes = def.cooltime.max(0);
    s.event.lord.skill_ticks.insert(def.id, minutes);
    ctx.persist.op(DbOp::SetLordSkillCooldown {
        skill_id: def.id,
        minutes,
    });
    // `LTT_SELF` skills act on the lord himself
    let aimed = target.as_ref().map_or(lord.obj, |t| t.obj);
    let aimed_name = target
        .as_ref()
        .map_or_else(|| lord.name.clone(), |t| t.name.clone());
    let aimed_character = target.as_ref().map_or(lord.character, |t| t.character);
    match def.kind {
        LT_ITEMBUF => {
            if data.items.get(def.item).is_some() {
                // a refusal is logged, the cooldown stays (KEEP)
                if let Err(e) = crate::buff::apply_item(s, ctx, aimed, def.item, Some(lord.obj)) {
                    tracing::info!(skill = def.id, error = ?e, "a lord skill buff was refused");
                }
                if def.id == LI_RAGE {
                    notice::defined(ctx, who, TID_GAME_LORD_SKILL_RAGE_S001, vec![aimed_name]);
                    defined(ctx, PlayerRef(aimed), TID_GAME_LORD_SKILL_RAGE_S002);
                }
            }
        }
        LT_SUMMON => {
            if let Some(t) = &target {
                defined(ctx, PlayerRef(t.obj), TID_GAME_LORD_SKILL_SUMMON_S001);
                if let Err(e) =
                    ctx.worlds
                        .replace(t.obj, lord.world, lord.pos, lord.layer, ReplaceKind::Normal)
                {
                    tracing::info!(error = %e, "a lord summon was refused");
                }
            }
        }
        LT_TELEPORT => {
            if let Some(t) = &target
                && let Err(e) =
                    ctx.worlds
                        .replace(lord.obj, t.world, t.pos, t.layer, ReplaceKind::Normal)
            {
                tracing::info!(error = %e, "a lord teleport was refused");
            }
        }
        _ => {}
    }
    ctx.broadcast(
        Some(&*s),
        lord.obj,
        Audience::NearInclSelf,
        pb::LordSkillUseNtf {
            target: aimed_character.0,
            skill,
        },
    );
    let ev = audit::lord_skill(lord.character, def.id, target.as_ref().map(|t| t.character));
    let ev = match crate::audit::actor_of(ctx, who) {
        Some(actor) => ev.by(actor),
        None => ev,
    };
    ctx.audit.emit(ev);
}

/// The cooldown ticks of P4 (`CTLordSkill::OnTimer`): every skill with minutes left loses one (`SetLordSkillCooldown`),
/// then the lord, when he is in the world, gets `LordSkillTick` with ALL skills in table order.
pub fn minute(s: &mut Services, ctx: &mut GameCtx<'_>) {
    let mut ops = Vec::new();
    for (id, minutes) in &mut s.event.lord.skill_ticks {
        if *minutes > 0 {
            *minutes -= 1;
            ops.push(DbOp::SetLordSkillCooldown {
                skill_id: *id,
                minutes: *minutes,
            });
        }
    }
    if !ops.is_empty() {
        super::send_ops(s, ctx, "lord_skill_tick", ops);
    }
    let Some(lord) = s.event.lord.lord else {
        return;
    };
    let Some(obj) = s
        .presence
        .get(lord)
        .and_then(|e| e.online)
        .map(|o| o.obj)
        .filter(|o| ctx.worlds.player(*o).is_some())
    else {
        return;
    };
    let ticks = skill_entries(s);
    crate::chat::send_to(ctx, PlayerRef(obj), pb::LordSkillTick { ticks });
}

/// The `(skill, minutes)` entries of every skill in table order (`CLordSkill::SerializeTick`).
pub fn skill_entries(s: &Services) -> Vec<pb::LordSkillTickEntry> {
    s.event
        .lord
        .skill_order
        .iter()
        .map(|id| pb::LordSkillTickEntry {
            skill: i32::try_from(*id).unwrap_or(i32::MAX),
            tick: s.event.lord.skill_tick(*id),
        })
        .collect()
}
