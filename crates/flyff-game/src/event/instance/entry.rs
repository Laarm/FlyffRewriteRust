//! The entry of a dungeon: `CInstanceDungeonHelper::EnteranceDungeon` and `CInstanceDungeonBase::TeleportToDungeon`
//! (`InstanceDungeonBase.cpp` 631-711, 985-1000) with the creation of the layer (`CreateDungeonLayer`,
//! `CreateMonster`, 109-139, 590-629).

use super::{
    Cooldown, DungeonState, InstanceState, count_of, def_of, first_state, layer_of, monsters_of,
    teleport_of,
};
use crate::chat::ingress::notice;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::movement::intents;
use crate::world::{LayerError, LayerKind, ReplaceKind};
use flyff_common::defs::itemkind::IK3_TEXT_DISGUISE;
use flyff_common::defs::text::{
    TID_GAME_INSTANCE_CLASS, TID_GAME_INSTANCE_COOLTIME, TID_GAME_INSTANCE_DISGUISE02,
    TID_GAME_INSTANCE_LEVEL, TID_GAME_INSTANCE_MAX, TID_GAME_INSTANCE_PARTY,
};
use flyff_common::defs::world::WI_WORLD_MADRIGAL;
use flyff_common::math::Vec3;
use flyff_common::rng::GameRng;
use flyff_common::{GameMs, LayerId, ObjId, PartyId, WorldId};
use flyff_data::lua_config::DungeonDef;
use flyff_db::audit::ev::INSTANCE_CREATE;
use flyff_sim::num::angle_to_vector_xz;
use std::sync::Arc;

/// What [`on_portal`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortalOutcome {
    /// The target world is not an instance dungeon of the script (or the player is gone): nothing happened.
    NotHandled,
    /// The player was sent into the layer of his party's dungeon (the replace runs at the next `_replace`).
    Entered,
    /// Refused (a text was sent, except for an unloaded dungeon world); the player was pushed back.
    Refused,
    /// `fidelity.instance_enter_immediate = false`: the dungeon was created, the player stays outside (the
    /// legacy second trigger enters).
    Pending,
}

/// The push of a refused entrance (`fPushPower = 0.5f`).
const PUSH_POWER: f32 = 0.5;

/// `EnteranceDungeon(player, world)` for the portal of the player `p` whose target is the dungeon `world` at
/// `tele_pos`: the party gate, `TeleportToDungeon`, and the push-back of a refusal. `NotHandled` when `world` has no
/// definition in `PartyDungeon.lua`.
pub fn on_portal(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    p: ObjId,
    world: WorldId,
    tele_pos: Vec3,
) -> PortalOutcome {
    let data = Arc::clone(ctx.data);
    let Some(def) = def_of(&data, world) else {
        return PortalOutcome::NotHandled;
    };
    let max = data
        .lua
        .party_dungeon
        .as_ref()
        .map_or(0, |c| c.max_instance_dungeon);
    let Some((character, level, job)) = ctx.worlds.player(p).and_then(|m| {
        m.player()
            .map(|pl| (pl.ident.character_id, m.mv.level, m.mv.job))
    }) else {
        return PortalOutcome::NotHandled;
    };
    let who = PlayerRef(p);
    let outcome = try_enter(
        svc, ctx, who, character, level, job, world, tele_pos, def, max,
    );
    if outcome == PortalOutcome::Refused {
        push_back(ctx, p);
    }
    outcome
}

#[allow(clippy::too_many_arguments)]
fn try_enter(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    character: flyff_common::CharacterId,
    level: i32,
    job: i32,
    world: WorldId,
    tele_pos: Vec3,
    def: &DungeonDef,
    max: i32,
) -> PortalOutcome {
    // `EnteranceDungeon` of the party dungeon: `m_idparty > 0`
    let Some(party) = svc.social.party.party_of(character) else {
        notice::defined(ctx, who, TID_GAME_INSTANCE_PARTY, Vec::new());
        return PortalOutcome::Refused;
    };
    let Some(layer) = layer_of(party) else {
        // FIX: a layer id is an `i32`
        tracing::warn!(
            party = party.0,
            "instance: the party id does not fit a layer id"
        );
        notice::defined(ctx, who, TID_GAME_INSTANCE_MAX, Vec::new());
        return PortalOutcome::Refused;
    };
    // `CheckClassLevel`
    let class = flyff_rules::jobs::legend_class(&svc.stats.core().rules, job);
    if def.class_mask & (1u32 << class) == 0 {
        notice::defined(ctx, who, TID_GAME_INSTANCE_CLASS, Vec::new());
        return PortalOutcome::Refused;
    }
    if level < def.min_level || level > def.max_level {
        notice::defined(
            ctx,
            who,
            TID_GAME_INSTANCE_LEVEL,
            vec![def.min_level.to_string(), def.max_level.to_string()],
        );
        return PortalOutcome::Refused;
    }
    // a disguise cannot enter (`__INSTANCE_DUNGEON`)
    if crate::buff::api::has_ik3(ctx, who.0, IK3_TEXT_DISGUISE) {
        notice::defined(ctx, who, TID_GAME_INSTANCE_DISGUISE02, Vec::new());
        return PortalOutcome::Refused;
    }
    // `if( !pWorld ) return FALSE;` comes before the cooldown and says nothing
    if ctx.worlds.world(world).is_none() {
        return PortalOutcome::Refused;
    }
    // the cooldown of another dungeon of this world
    let now = ctx.clock.game_ms;
    let remaining = svc
        .event
        .instances
        .cooldowns_of(character)
        .iter()
        .find(|c| c.world == world)
        .filter(|c| c.party != Some(party) && c.until > now)
        .map(|c| c.until.0 - now.0);
    if let Some(r) = remaining {
        notice::defined(
            ctx,
            who,
            TID_GAME_INSTANCE_COOLTIME,
            vec![(r / 60_000).to_string(), ((r % 60_000) / 1000).to_string()],
        );
        return PortalOutcome::Refused;
    }
    let key = (world, party);
    if !svc.event.instances.instances.contains_key(&key) {
        // the dungeon does not exist: ask for a new one (`SendInstanceDungeonCreate`)
        let running = i64::try_from(svc.event.instances.instances.len()).unwrap_or(i64::MAX);
        if running >= i64::from(max) {
            notice::defined(ctx, who, TID_GAME_INSTANCE_MAX, Vec::new());
            return PortalOutcome::Refused;
        }
        if let Err(e) = create(svc, ctx, who, def, world, party, layer) {
            tracing::warn!(world = world.0, party = party.0, "instance: no layer: {e}");
            notice::defined(ctx, who, TID_GAME_INSTANCE_MAX, Vec::new());
            return PortalOutcome::Refused;
        }
        if !ctx.cfg.rules.fidelity.instance_enter_immediate {
            return PortalOutcome::Pending;
        }
    }
    enter(
        svc, ctx, who, character, def, world, party, layer, tele_pos, now,
    )
}

/// The `m_linkMap.GetLinkMap` branch of `TeleportToDungeon`: the marking, the jitter, the replace, the counter and
/// the cooldown.
#[allow(clippy::too_many_arguments)]
fn enter(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    character: flyff_common::CharacterId,
    def: &DungeonDef,
    world: WorldId,
    party: PartyId,
    layer: LayerId,
    tele_pos: Vec3,
    now: GameMs,
) -> PortalOutcome {
    let key = (world, party);
    let Some(state) = svc.event.instances.instances.get(&key).map(|i| i.state) else {
        return PortalOutcome::Refused;
    };
    // `GetTeleportPos` of the current state, else the portal's own target
    let target = teleport_of(def, state).unwrap_or(tele_pos);
    let (mark_world, mark_pos) = marking_for(ctx, world, target);
    if let Some(pl) = ctx.worlds.player_mut(who.0).and_then(|m| m.player_mut()) {
        // KEEP: the persisted Lodelight mark is overwritten
        pl.misc.marking_world = mark_world;
        pl.misc.marking_pos = mark_pos;
    }
    // KEEP-QUIRK: `xRandom(4) - 2` is asymmetric (-2 ..= 1), x then z, from the event stream
    let rng = ctx.rng.event();
    let jx = rng.random_below(4) as f32 - 2.0;
    let jz = rng.random_below(4) as f32 - 2.0;
    let pos = Vec3::new(target.x + jx, target.y, target.z + jz);
    if let Err(e) = ctx
        .worlds
        .replace(who.0, world, pos, layer, ReplaceKind::Normal)
    {
        tracing::warn!(
            world = world.0,
            party = party.0,
            "instance: the entry teleport was refused: {e}"
        );
    }
    if let Some(i) = svc.event.instances.instances.get_mut(&key) {
        i.player_count += 1;
    }
    // a re-entry of a dungeon that is still running does not renew the cooldown
    let until = GameMs(now.0 + u64::from(def.cool_time_ms));
    let list = svc.event.instances.cooldowns.entry(character).or_default();
    match list.iter_mut().find(|c| c.world == world) {
        Some(c) if c.party == Some(party) => {}
        Some(c) => {
            c.party = Some(party);
            c.until = until;
        }
        None => list.push(Cooldown {
            world,
            party: Some(party),
            until,
        }),
    }
    PortalOutcome::Entered
}

/// `SetLeaveMarkingPos`: Madrigal's drive-out point, then the exit portal of the dungeon world nearest (3-D, squared)
/// to `target`; the first of equal distances wins (strict `<` from `INT_MAX`).
pub(crate) fn marking_for(ctx: &GameCtx<'_>, world: WorldId, target: Vec3) -> (WorldId, Vec3) {
    let mut mark = (
        WorldId(WI_WORLD_MADRIGAL),
        crate::world::revival::MADRIGAL_DRIVE_OUT,
    );
    let Some(w) = ctx.worlds.world(world) else {
        return mark;
    };
    let mut best = i32::MAX as f32;
    for r in &w.statics.regions {
        if r.tele_world == 0 {
            continue;
        }
        let d = (target - Vec3::new(r.pos[0], r.pos[1], r.pos[2])).length_squared();
        if d < best {
            best = d;
            mark = (
                WorldId(r.tele_world),
                Vec3::new(r.tele_pos[0], r.tele_pos[1], r.tele_pos[2]),
            );
        }
    }
    mark
}

/// `CInstanceDungeonBase::CreateDungeonLayer`: the layer, the first state, its monsters, the audit line.
fn create(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    def: &DungeonDef,
    world: WorldId,
    party: PartyId,
    layer: LayerId,
) -> Result<(), LayerError> {
    ctx.worlds.create_layer(
        world,
        layer,
        LayerKind::Instance,
        ctx.data,
        &svc.event.flags,
        ctx.rng.event(),
    )?;
    let state = first_state(def);
    svc.event.instances.instances.insert(
        (world, party),
        InstanceState {
            state,
            kill_count: 0,
            player_count: 0,
            created_at: ctx.clock.game_ms,
        },
    );
    spawn_state(ctx, def, world, layer, state);
    crate::audit::emit_for(
        ctx,
        who,
        INSTANCE_CREATE,
        serde_json::json!({
            "world": world.0,
            "layer": layer.0,
            "party": party.0,
            "state": state.kind(),
            "monsters": count_of(def, state),
        }),
    );
    Ok(())
}

/// `CInstanceDungeonBase::CreateMonster`: the monsters of `state` in script order, `m_bActiveAttack = bRed`, and
/// the DPS meter of the bosses (`InstanceDungeonBase.cpp:617-624`). A mover id without a prop is skipped.
pub(crate) fn spawn_state(
    ctx: &mut GameCtx<'_>,
    def: &DungeonDef,
    world: WorldId,
    layer: LayerId,
    state: DungeonState,
) {
    let dps = ctx.cfg.rules.features.dps_meter;
    for mon in monsters_of(def, state) {
        let spawn = crate::ai::MonsterSpawn {
            mover: mon.mover,
            world,
            layer,
            pos: Vec3::new(mon.pos[0], mon.pos[1], mon.pos[2]),
            active_attack: mon.aggressive,
            gold: None,
            owner_ai: None,
        };
        let Some(id) = crate::ai::spawn::commit(ctx.worlds, ctx.data, ctx.rng.event(), &spawn)
        else {
            tracing::warn!(
                mover = mon.mover,
                world = world.0,
                "instance: a monster was not created"
            );
            continue;
        };
        if dps && let Some(m) = ctx.worlds.mover_mut(id) {
            crate::combat::dps::mark_instance_spawn(m, &ctx.data.dps_monsters);
        }
    }
}

/// The push-back of a refused entrance (`CInstanceDungeonHelper::EnteranceDungeon`): half a metre backwards
/// (`angle + 180`), a normal replace in the same world and layer, and the `PushPower` intent.
fn push_back(ctx: &mut GameCtx<'_>, p: ObjId) {
    let Some((world, layer, pos, angle)) = ctx
        .worlds
        .player(p)
        .map(|m| (m.core.world, m.core.layer, m.core.pos, m.core.angle))
    else {
        return;
    };
    let back = angle + 180.0;
    let target = pos + angle_to_vector_xz(back, PUSH_POWER);
    if let Err(e) = ctx
        .worlds
        .replace(p, world, target, layer, ReplaceKind::Normal)
    {
        tracing::debug!(player = p.0, "instance: the push-back was refused: {e}");
    }
    if let Some(m) = ctx.worlds.player_mut(p) {
        intents::push(m, back, PUSH_POWER);
    }
}
