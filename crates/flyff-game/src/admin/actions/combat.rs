//! Combat commands of the console (Plan/25 §3.12, §11.5): heal, resurrect, kill, despawn and clear an area.
//! **WP-ADMIN-12.** The bodies call the services the GM commands call: the pools are `/heal`'s, the kill is `/kill`'s
//! forced hit through the damage pipeline (so EXP, drops, the death penalty and every other death rule run), the
//! resurrection is the revival of 17 §9.4, the despawn is `/rmvnpc`'s `CObj::Delete`.
//!
//! Every group command answers a [`TargetResult`] per object in request order. The ids are the object ids of the map
//! frames; a target that is gone, dead, in another place than the console expected, or a player that outranks the
//! actor is reported and skipped (`stale_target` / `target_outranks`).

use super::common::{
    Place, check_group, check_rank, fail, failed, live_core, online_player, report,
};
use super::{TargetId, TargetResult};
use crate::admin::map::EntityKind;
use crate::admin::types::{AdminErrorCode, AdminFailure};
use crate::combat::arbiter::{DamageRequest, send_damage};
use crate::combat::context::with_action_at;
use crate::combat::death::{REVIVAL_INVULNERABLE_TICKS, revive_pools};
use crate::combat::{SimFx, is_die};
use crate::ctx::{GameCtx, Services};
use crate::input::{AdminActor, AdminOutput};
use crate::mover::Mover;
use crate::stats::Out;
use crate::stats::pool::{fill_pools_silent, send_pools_absolute};
use flyff_common::{CharacterId, LayerId, ObjId, WorldId};
use flyff_proto::pb;
use flyff_rules::combat::{AtkFlags, DamageParam};
use flyff_rules::consts::defs::neuz::AII_PET;
use serde_json::{Value, json};

/// Where the console expected an object to be: the world and layer of the map it acted on. An object that is
/// somewhere else is a stale target.
pub type ExpectedPlace = Option<(WorldId, LayerId)>;

/// Heal movers to full (`Heal`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Heal {
    /// The movers.
    pub targets: Vec<ObjId>,
    /// The world and layer the console saw them in.
    pub expect: ExpectedPlace,
}

/// Bring a dead character back (`Resurrect`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resurrect {
    /// The character.
    pub character: CharacterId,
}

/// Kill objects (`KillObject`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KillObject {
    /// The objects.
    pub objs: Vec<ObjId>,
    /// `true`: the normal death with drops and EXP; `false`: the death without either (the damage ledger is cleared
    /// first, so nobody is credited).
    pub drops: bool,
    /// The world and layer the console saw them in.
    pub expect: ExpectedPlace,
}

/// Remove objects without a death (`DespawnObject`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DespawnObject {
    /// The objects.
    pub objs: Vec<ObjId>,
    /// The world and layer the console saw them in.
    pub expect: ExpectedPlace,
}

/// Remove everything of some kinds inside a rectangle (`DespawnArea`).
#[derive(Clone, Debug, PartialEq)]
pub struct DespawnArea {
    /// World.
    pub world: WorldId,
    /// Layer.
    pub layer: LayerId,
    /// `[x0, z0, x1, z1]` in metres.
    pub rect: [f32; 4],
    /// A bitset of [`EntityKind::bit`](crate::admin::map::EntityKind::bit); players are never removed.
    pub kinds: u32,
    /// The most objects the command may remove (`admin.max_objects_per_action`): more answers `too_many_targets`
    /// and removes nothing (25 §3.12 "Clear area, <= 500 objects").
    pub max_objects: u32,
}

fn kind_of(m: &Mover) -> EntityKind {
    if m.is_player() {
        EntityKind::Player
    } else if m.npc().is_some() {
        EntityKind::Npc
    } else if m.monster().is_some_and(|e| e.ai.kind == AII_PET) {
        EntityKind::Pet
    } else {
        EntityKind::Monster
    }
}

fn kind_name(k: EntityKind) -> &'static str {
    match k {
        EntityKind::Player => "player",
        EntityKind::Monster => "monster",
        EntityKind::Npc => "npc",
        EntityKind::Pet => "pet",
        EntityKind::Item => "item",
        EntityKind::Ctrl => "ctrl",
    }
}

/// A live object that is where the console expected it.
fn expected(ctx: &GameCtx<'_>, obj: ObjId, expect: ExpectedPlace) -> Result<(), AdminFailure> {
    let core = live_core(ctx, obj)?;
    match expect {
        Some((w, l)) if (core.world, core.layer) != (w, l) => Err(fail(
            AdminErrorCode::StaleTarget,
            format!(
                "object {} is in world {} layer {} now, not in world {} layer {}",
                obj.0, core.world.0, core.layer.0, w.0, l.0
            ),
        )),
        _ => Ok(()),
    }
}

/// A live mover (a player, a monster or an NPC) that is where the console expected it, with its rank checked.
fn live_mover<'a>(
    ctx: &'a GameCtx<'_>,
    actor: &AdminActor,
    obj: ObjId,
    expect: ExpectedPlace,
) -> Result<&'a Mover, AdminFailure> {
    expected(ctx, obj, expect)?;
    let m = ctx.worlds.mover(obj).ok_or_else(|| {
        fail(
            AdminErrorCode::Invalid,
            format!("object {} is not a mover", obj.0),
        )
    })?;
    check_rank(actor, ctx, obj)?;
    Ok(m)
}

// ---- Heal -------------------------------------------------------------------------------------------------------------

/// `Heal`: HP, MP and FP of every live mover in the list go to their maxima and are sent to the viewers
/// (`SetPointParam`, the effect of `/heal`). A dead mover is reported as stale ("resurrect it first"): it is not
/// revived. Info per target: `{hp_before, hp_after}`.
pub(super) fn heal(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    actor: AdminActor,
    cmd: Heal,
) -> AdminOutput {
    if let Err(f) = check_group(cmd.targets.len(), "targets") {
        return AdminOutput::Failed(f);
    }
    let core = svc.stats.handle();
    let mut per_target = Vec::with_capacity(cmd.targets.len());
    for obj in cmd.targets.iter().copied() {
        let target = TargetId::Object(obj);
        let one = (|| -> Result<Value, AdminFailure> {
            let m = live_mover(ctx, &actor, obj, cmd.expect)?;
            if is_die(m) || m.mv.hp <= 0 {
                return Err(fail(
                    AdminErrorCode::StaleTarget,
                    format!("object {} is dead: resurrect it first", obj.0),
                ));
            }
            let before = m.mv.hp;
            let mut out = Out::new();
            let after = {
                let Some(m) = ctx.worlds.mover_mut(obj) else {
                    return Err(fail(AdminErrorCode::StaleTarget, "the object is gone"));
                };
                fill_pools_silent(m, &core);
                send_pools_absolute(m, &core, &mut out);
                m.mv.hp
            };
            out.flush(ctx, obj);
            Ok(json!({"hp_before": before, "hp_after": after}))
        })();
        per_target.push(match one {
            Ok(info) => TargetResult::ok(target).with_info(info),
            Err(f) => TargetResult::failed(target, f),
        });
    }
    report(per_target)
}

// ---- Resurrect --------------------------------------------------------------------------------------------------------

/// `Resurrect`: a dead online character stands up where it fell with full HP, MP and FP, no EXP penalty and the five
/// seconds of invulnerability of a revival (17 §9.4); the buffs that survived are sent again. A character that is not
/// dead is reported as stale.
pub(super) fn resurrect(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    actor: AdminActor,
    cmd: Resurrect,
) -> AdminOutput {
    let target = TargetId::Character(cmd.character);
    let one = (|| -> Result<Value, AdminFailure> {
        let who = online_player(svc, ctx, cmd.character)?;
        check_rank(&actor, ctx, who.0)?;
        let alive = ctx
            .worlds
            .player(who.0)
            .is_some_and(|m| !is_die(m) && m.mv.hp > 0);
        if alive {
            return Err(fail(
                AdminErrorCode::StaleTarget,
                format!("character {} is not dead", cmd.character.0),
            ));
        }
        let core = svc.stats.handle();
        if let Some(m) = ctx.worlds.player_mut(who.0) {
            m.combat.resurrection = None;
            m.combat.dead_ticks = REVIVAL_INVULNERABLE_TICKS;
        }
        let frame = ctx.clock.frame0;
        with_action_at(svc, ctx, who.0, frame, |cx| {
            cx.arb.fx.sim.push((who.0, SimFx::ClearState));
            cx.arb.fx.near(who.0, pb::RevivalNtf {});
        });
        let mut out = Out::new();
        let hp = ctx.worlds.player_mut(who.0).map_or(0, |m| {
            revive_pools(m, &core, 1.0, false, &mut out);
            m.mv.hp
        });
        out.flush(ctx, who.0);
        (svc.combat.death.after_revival)(svc, ctx, who);
        Ok(json!({"hp_after": hp}))
    })();
    report(vec![match one {
        Ok(info) => TargetResult::ok(target).with_info(info),
        Err(f) => TargetResult::failed(target, f),
    }])
}

// ---- KillObject -------------------------------------------------------------------------------------------------------

/// `KillObject`: one `AF_FORCE` hit of the current HP through the damage pipeline, so the death rules run (17 §9: the
/// death state, the EXP and drops of the players on the damage ledger, the death penalty of a player). The victim hits
/// itself: the PvP rules neither refuse the hit nor flag anybody as a killer (the rule of `/kill` for a player). With
/// `drops = false` the damage ledger is cleared first, so nobody is credited and the monster leaves nothing.
/// Players and monsters can be killed; NPCs, pets and the other objects are despawned instead.
pub(super) fn kill_object(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    actor: AdminActor,
    cmd: KillObject,
) -> AdminOutput {
    if let Err(f) = check_group(cmd.objs.len(), "objects") {
        return AdminOutput::Failed(f);
    }
    let mut per_target = Vec::with_capacity(cmd.objs.len());
    for obj in cmd.objs.iter().copied() {
        let target = TargetId::Object(obj);
        let one = (|| -> Result<Value, AdminFailure> {
            let m = live_mover(ctx, &actor, obj, cmd.expect)?;
            let kind = kind_of(m);
            if !matches!(kind, EntityKind::Player | EntityKind::Monster) {
                return Err(fail(
                    AdminErrorCode::Invalid,
                    format!("a {} cannot be killed: despawn it instead", kind_name(kind)),
                ));
            }
            if is_die(m) || m.mv.hp <= 0 {
                return Err(fail(
                    AdminErrorCode::StaleTarget,
                    format!("object {} is already dead", obj.0),
                ));
            }
            let hp = m.mv.hp;
            if kind == EntityKind::Monster
                && !cmd.drops
                && let Some(v) = ctx.worlds.mover_mut(obj)
            {
                v.combat.hate.clear();
            }
            send_damage(
                svc,
                ctx,
                DamageRequest::new(obj, obj, AtkFlags::FORCE, DamageParam::force(hp)),
            );
            Ok(json!({"kind": kind_name(kind), "hp_before": hp, "drops": cmd.drops}))
        })();
        per_target.push(match one {
            Ok(info) => TargetResult::ok(target).with_info(info),
            Err(f) => TargetResult::failed(target, f),
        });
    }
    report(per_target)
}

// ---- DespawnObject ----------------------------------------------------------------------------------------------------

/// `DespawnObject`: `CObj::Delete` of monsters, NPCs, pets, ground items and ctrls (the effect of `/rmvnpc`, widened
/// to the kinds of the catalogue). A monster of a respawn line is replaced by the line's own rules. A player is never
/// despawned (kick it).
pub(super) fn despawn_object(
    _svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    _actor: AdminActor,
    cmd: DespawnObject,
) -> AdminOutput {
    if let Err(f) = check_group(cmd.objs.len(), "objects") {
        return AdminOutput::Failed(f);
    }
    let mut per_target = Vec::with_capacity(cmd.objs.len());
    for obj in cmd.objs.iter().copied() {
        let target = TargetId::Object(obj);
        let one = (|| -> Result<Value, AdminFailure> {
            expected(ctx, obj, cmd.expect)?;
            let kind = match ctx.worlds.mover(obj) {
                Some(m) => kind_of(m),
                None => match ctx.worlds.core(obj).map(|c| c.kind) {
                    Some(crate::world::ObjKind::Item) => EntityKind::Item,
                    _ => EntityKind::Ctrl,
                },
            };
            if kind == EntityKind::Player {
                return Err(fail(
                    AdminErrorCode::Invalid,
                    "a player is not despawned: kick it",
                ));
            }
            let place = ctx
                .worlds
                .core(obj)
                .map(|c| {
                    Place {
                        world: c.world,
                        layer: c.layer,
                        pos: c.pos,
                    }
                    .json()
                })
                .unwrap_or(Value::Null);
            ctx.worlds.delete(obj);
            Ok(json!({"kind": kind_name(kind), "at": place}))
        })();
        per_target.push(match one {
            Ok(info) => TargetResult::ok(target).with_info(info),
            Err(f) => TargetResult::failed(target, f),
        });
    }
    report(per_target)
}

// ---- DespawnArea ------------------------------------------------------------------------------------------------------

/// `DespawnArea`: every linked object of the chosen kinds inside the rectangle of the layer is deleted (monsters,
/// pets, NPCs, ground items, ctrls; never a player). More than `max_objects` matches answer `too_many_targets` and
/// remove nothing. One [`TargetResult`] per removed object, in the layer's member order.
pub(super) fn despawn_area(
    _svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    _actor: AdminActor,
    cmd: DespawnArea,
) -> AdminOutput {
    let [x0, z0, x1, z1] = cmd.rect;
    if !cmd.rect.iter().all(|v| v.is_finite()) {
        return failed(AdminErrorCode::Invalid, "the rectangle is not finite");
    }
    let (x0, x1) = (x0.min(x1), x0.max(x1));
    let (z0, z1) = (z0.min(z1), z0.max(z1));
    if cmd.kinds == 0 {
        return failed(AdminErrorCode::Invalid, "no kind of object chosen");
    }
    if cmd.kinds & EntityKind::Player.bit() != 0 {
        return failed(
            AdminErrorCode::Invalid,
            "players are never removed by an area",
        );
    }
    let Some(layer) = ctx.worlds.layer(cmd.world, cmd.layer) else {
        return failed(
            AdminErrorCode::Invalid,
            format!("world {} has no layer {}", cmd.world.0, cmd.layer.0),
        );
    };
    let store = ctx.worlds.store();
    let max = usize::try_from(cmd.max_objects).unwrap_or(usize::MAX);
    let mut found: Vec<(ObjId, EntityKind)> = Vec::new();
    for id in layer.members.iter().copied() {
        let Some(core) = store.core(id) else { continue };
        if core.deleted || core.pos.x < x0 || core.pos.x > x1 || core.pos.z < z0 || core.pos.z > z1
        {
            continue;
        }
        let kind = match store.mover(id) {
            Some(m) => kind_of(m),
            None if core.kind == crate::world::ObjKind::Item => EntityKind::Item,
            None => EntityKind::Ctrl,
        };
        if cmd.kinds & kind.bit() == 0 {
            continue;
        }
        if found.len() >= max {
            return failed(
                AdminErrorCode::TooManyTargets,
                format!("more than {max} objects in the area: nothing was removed"),
            );
        }
        found.push((id, kind));
    }
    let mut per_target = Vec::with_capacity(found.len());
    for (id, kind) in found {
        ctx.worlds.delete(id);
        per_target.push(
            TargetResult::ok(TargetId::Object(id)).with_info(json!({"kind": kind_name(kind)})),
        );
    }
    report(per_target)
}
